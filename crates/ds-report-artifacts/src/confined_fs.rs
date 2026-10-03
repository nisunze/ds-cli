//! Handle-relative filesystem capability used by the Network Reporter bridge.
//!
//! A checked `PathBuf` is not an authority: another process owned by the same
//! account can rename an ancestor between inspection and mutation.  This
//! module walks an absolute path one component at a time, refuses links and
//! reparse points, and retains the resulting directory handles.  Unix
//! mutations use `*at` syscalls relative to those handles.  Windows has no
//! stable Win32 `openat`; it retains every ancestor with backup semantics,
//! opens reparse points rather than following them, and deliberately omits
//! `FILE_SHARE_DELETE`, making pathname replacement fail while the capability
//! is live.  Every opened/renamed child is compared with its held file identity.

use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString};
use std::fs::File;
#[cfg(windows)]
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    first: u64,
    second: u64,
}

#[cfg(unix)]
fn metadata_identity(metadata: &std::fs::Metadata) -> FileIdentity {
    use std::os::unix::fs::MetadataExt;

    FileIdentity {
        first: metadata.dev(),
        second: metadata.ino(),
    }
}

#[cfg(unix)]
fn file_identity(file: &File) -> Result<FileIdentity, String> {
    file.metadata()
        .map(|metadata| metadata_identity(&metadata))
        .map_err(|error| format!("could not read held Network Reporter file identity: {error}"))
}

#[cfg(windows)]
fn file_identity(file: &File) -> Result<FileIdentity, String> {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[repr(C)]
    struct ByHandleFileInformation {
        attributes: u32,
        creation_time: FileTime,
        last_access_time: FileTime,
        last_write_time: FileTime,
        volume_serial_number: u32,
        file_size_high: u32,
        file_size_low: u32,
        number_of_links: u32,
        file_index_high: u32,
        file_index_low: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandle(
            handle: *mut c_void,
            information: *mut ByHandleFileInformation,
        ) -> i32;
    }

    let mut information = std::mem::MaybeUninit::<ByHandleFileInformation>::uninit();
    // SAFETY: the file owns a live Win32 handle and the output points to
    // correctly sized writable storage for the duration of the call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), information.as_mut_ptr()) }
        == 0
    {
        return Err(format!(
            "could not read held Network Reporter file identity: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: a successful GetFileInformationByHandle initialized the record.
    let information = unsafe { information.assume_init() };
    Ok(FileIdentity {
        first: u64::from(information.volume_serial_number),
        second: (u64::from(information.file_index_high) << 32)
            | u64::from(information.file_index_low),
    })
}

#[cfg(unix)]
fn metadata_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn metadata_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[derive(Debug)]
struct HeldDirectoryInner {
    path: PathBuf,
    // On Windows the open handle IS the capability: holding it denies
    // delete/rename sharing for the directory. Unix reads it for *at syscalls.
    #[cfg_attr(windows, allow(dead_code))]
    file: File,
    identity: FileIdentity,
    // Retaining the parent chain matters on Windows: each handle denies
    // delete sharing, so neither this directory nor any ancestor can be
    // renamed out from under a full-path Win32 operation.
    _parent: Option<Arc<HeldDirectoryInner>>,
}

#[derive(Clone, Debug)]
pub struct HeldDirectory {
    inner: Arc<HeldDirectoryInner>,
}

/// Exclusive mutation lease for one file below a held directory.  Unix uses
/// a kernel `flock`, which is released when the process closes the descriptor;
/// Windows opens the file without write/delete sharing, so a crashed owner is
/// likewise distinguished by handle lifetime rather than marker age.
#[derive(Debug)]
pub struct HeldFileLock {
    // On Windows the held handle itself is the lock (no-write/delete share
    // mode); on Unix Drop releases the flock. Both branches also read it to
    // restamp the owner record on `renew`.
    file: File,
    /// The record this owner stamped into the marker, absent only for a probe
    /// that deliberately left the previous holder's evidence intact.
    owner: Option<LockOwner>,
}

impl HeldFileLock {
    /// Restamp the heartbeat while this owner is still working.
    ///
    /// A kernel lock says nothing about progress: a holder that is alive but
    /// wedged, and a holder whose lock outlived it on a filesystem that does
    /// not release advisory locks, look identical from outside. The heartbeat
    /// is what separates them, so any operation that can hold this lock for
    /// long critical sections renews it as it works.
    /// A failed renew is not fatal — the work below the lock is still sound —
    /// so this reports the failure instead of ending the critical section.
    pub fn renew(&mut self, now_ms: u64) -> Result<(), String> {
        let Some(owner) = self.owner.as_mut() else {
            return Ok(());
        };
        owner.heartbeat_at_ms = now_ms.max(owner.heartbeat_at_ms);
        let body = owner.encoded()?;
        self.file
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.file.set_len(0))
            .and_then(|_| self.file.write_all(&body))
            .and_then(|_| self.file.sync_data())
            .map_err(|error| format!("could not renew the lock heartbeat: {error}"))
    }

    /// What this owner stamped, for a caller that reports who holds the lock.
    pub fn owner(&self) -> Option<&LockOwner> {
        self.owner.as_ref()
    }
}

/// How long a heartbeat may go unrenewed before the record stops counting as
/// a live holder's claim. Generous on purpose: a suspended laptop, a paused
/// container and a slow disk all stall a heartbeat without losing the lock,
/// and breaking a lock that is merely slow is worse than waiting.
pub const LOCK_STALE_AFTER_MS: u64 = 120_000;

/// The schema of the owner record written into a lock marker.
pub const LOCK_OWNER_SCHEMA: u32 = 1;

/// Who holds a lock marker, in enough detail to prove the holder is gone.
///
/// A pid alone is not evidence. Pids are reused, so a marker naming pid 3872
/// says nothing about whether *that* holder still runs — the operating system
/// may have handed 3872 to something unrelated an hour later. Three facts
/// together are evidence: the boot the record was written in (a record from an
/// earlier boot can have no live holder at all), the holder's process start
/// time (a live pid with a different start time is a reused pid, not the
/// holder), and the heartbeat (a holder that stopped renewing long ago is not
/// working below the lock). Only all three agreeing that the holder is gone
/// may break a lock; anything less waits, because silently stealing a lock
/// from a live writer is a worse defect than the wedge it would cure.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LockOwner {
    pub schema: u32,
    pub pid: u32,
    /// The machine boot this record was written in, where the platform can
    /// name one. `None` means "this platform cannot say", never "same boot".
    #[serde(default)]
    pub boot: Option<String>,
    /// The holder's process start time, where the platform can read one.
    #[serde(default)]
    pub process_started_at: Option<u64>,
    /// The executable that took the lock, so an operator reads a name and not
    /// only a number.
    #[serde(default)]
    pub worker: String,
    pub taken_at_ms: u64,
    pub heartbeat_at_ms: u64,
}

impl LockOwner {
    /// The record this process writes when it takes a lock.
    pub fn here(now_ms: u64) -> Self {
        let pid = std::process::id();
        Self {
            schema: LOCK_OWNER_SCHEMA,
            pid,
            boot: boot_marker(),
            process_started_at: process_started_at(pid),
            worker: current_worker_name(),
            taken_at_ms: now_ms,
            heartbeat_at_ms: now_ms,
        }
    }

    fn encoded(&self) -> Result<Vec<u8>, String> {
        let mut body = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode the lock owner record: {error}"))?;
        body.push(b'\n');
        Ok(body)
    }

    /// Parse a marker's contents, accepting the bare pid written by releases
    /// before this record existed. An installed machine carries markers from
    /// the previous build across an upgrade, and refusing to read them would
    /// turn every upgrade into the wedge this record exists to end.
    pub fn decode(body: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(body).ok()?.trim();
        if text.is_empty() {
            return None;
        }
        if let Ok(owner) = serde_json::from_str::<Self>(text) {
            return Some(owner);
        }
        let pid = text.lines().next()?.trim().parse::<u32>().ok()?;
        Some(Self {
            schema: 0,
            pid,
            boot: None,
            process_started_at: None,
            worker: String::new(),
            taken_at_ms: 0,
            heartbeat_at_ms: 0,
        })
    }

    /// How long ago this record was last renewed, when both clocks are usable.
    pub fn heartbeat_age_ms(&self, now_ms: u64) -> Option<u64> {
        if self.heartbeat_at_ms == 0 {
            return None;
        }
        Some(now_ms.saturating_sub(self.heartbeat_at_ms))
    }

    /// Whether the holder named here is provably gone, and why.
    pub fn liveness(&self) -> HolderLiveness {
        if let (Some(recorded), Some(current)) = (self.boot.as_deref(), boot_marker()) {
            if recorded != current {
                return HolderLiveness::Gone("the record was written before this machine booted");
            }
        }
        match process_is_live(self.pid) {
            Some(false) => HolderLiveness::Gone("the recorded process has exited"),
            Some(true) => match (self.process_started_at, process_started_at(self.pid)) {
                (Some(recorded), Some(current)) if recorded != current => {
                    HolderLiveness::Gone("the recorded process exited and its pid was reused")
                }
                _ => HolderLiveness::Live,
            },
            None => HolderLiveness::Unknown,
        }
    }

    /// The single sentence an operator reads about the holder.
    pub fn describe(&self, now_ms: u64) -> String {
        let worker = if self.worker.is_empty() {
            "unknown process".to_string()
        } else {
            self.worker.clone()
        };
        match self.heartbeat_age_ms(now_ms) {
            Some(age) => format!(
                "{worker} (pid {}), last heartbeat {}s ago",
                self.pid,
                age / 1_000
            ),
            None => format!("{worker} (pid {}), no heartbeat recorded", self.pid),
        }
    }
}

/// What the liveness evidence says about a marker's recorded holder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HolderLiveness {
    /// The holder is running: never break this lock.
    Live,
    /// The holder cannot be running, for the named reason.
    Gone(&'static str),
    /// This platform cannot say. Treated exactly like `Live` for breaking.
    Unknown,
}

impl HolderLiveness {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Gone(_) => "gone",
            Self::Unknown => "unknown",
        }
    }

    pub fn reason(self) -> Option<&'static str> {
        match self {
            Self::Gone(reason) => Some(reason),
            _ => None,
        }
    }
}

/// The executable that is taking a lock, bounded and path-free.
fn current_worker_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.chars().take(64).collect::<String>())
        })
        .unwrap_or_default()
}

/// A per-boot identifier, where the platform publishes one.
///
/// Only inequality is ever used: two different values prove two different
/// boots, which proves the recorded holder is gone. A platform that cannot
/// answer returns `None` and the decision falls back to pid liveness.
fn boot_marker() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let body = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
        let trimmed = body.trim();
        if trimmed.is_empty() || trimmed.len() > 128 {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// The holder's start time in platform ticks, where it is readable.
///
/// This is the half of the pair that survives pid reuse: a pid that is live
/// but started at a different instant is not the process that took the lock.
fn process_started_at(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let body = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // Field 22 is the start time, and fields 1..2 hold a comm value that
        // may itself contain spaces and parentheses. Split after the last
        // ')' so a process named "(a b)" cannot shift the field index.
        let tail = body.rsplit_once(')')?.1;
        tail.split_whitespace().nth(19)?.parse::<u64>().ok()
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
        use windows_sys::Win32::System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: query-only access; a failed observation cannot establish death.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        let empty = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (empty, empty, empty, empty);
        // SAFETY: live handle and four writable initialized FILETIME structures.
        let queried =
            unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) }
                != 0;
        unsafe { CloseHandle(process) };
        queried
            .then_some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = pid;
        None
    }
}

/// Whether a pid names a running process, where the platform can be asked.
///
/// `None` is "cannot say" and is never treated as death. Windows uses
/// query-only process access; access denial retains the holder's claim.
/// The exclusive marker handle separately prevents concurrent writers.
fn process_is_live(pid: u32) -> Option<bool> {
    #[cfg(unix)]
    {
        if pid == 0 {
            return Some(false);
        }
        let pid = i32::try_from(pid).ok()?;
        // SAFETY: signal 0 performs the permission and existence check only;
        // it delivers nothing and cannot affect the target.
        if unsafe { libc::kill(pid, 0) } == 0 {
            return Some(true);
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Some(false),
            // Alive, owned by another account.
            Some(libc::EPERM) => Some(true),
            _ => None,
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{
            CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, STILL_ACTIVE,
        };
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: query-only access. Only an absent pid proves death;
        // access denial or a failed query leaves liveness unknown.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return (unsafe { GetLastError() } == ERROR_INVALID_PARAMETER).then_some(false);
        }
        let mut code = 0;
        let queried = unsafe { GetExitCodeProcess(process, &mut code) } != 0;
        unsafe { CloseHandle(process) };
        queried.then_some(code == STILL_ACTIVE as u32)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        None
    }
}

#[cfg(all(test, windows))]
mod windows_process_liveness_tests {
    use super::*;

    #[test]
    fn a_live_owner_is_retained_and_its_start_time_distinguishes_pid_reuse() {
        let mut owner = LockOwner::here(now_ms());
        let started = owner
            .process_started_at
            .expect("Windows records process creation time");
        assert!(started > 0);
        assert_eq!(owner.liveness(), HolderLiveness::Live);
        owner.process_started_at = Some(started - 1);
        assert_eq!(
            owner.liveness(),
            HolderLiveness::Gone("the recorded process exited and its pid was reused")
        );
    }

    #[test]
    fn a_pid_that_cannot_name_a_windows_process_is_provably_gone() {
        let mut owner = LockOwner::here(now_ms());
        owner.pid = u32::MAX;
        owner.process_started_at = None;
        assert_eq!(
            owner.liveness(),
            HolderLiveness::Gone("the recorded process has exited")
        );
    }
}

#[cfg(unix)]
impl Drop for HeldFileLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;

        // SAFETY: the owned descriptor remains live for this call. Closing it
        // would release the lock too; the explicit unlock documents the exact
        // end of the pending-upload mutation critical section.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn exact_relative_name(name: &OsStr) -> Result<(), String> {
    let path = Path::new(name);
    if name.is_empty()
        || name == "."
        || name == ".."
        || path.is_absolute()
        || path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err("Network Reporter filesystem component is invalid".to_string());
    }
    Ok(())
}

impl HeldDirectory {
    fn from_open_file(
        path: PathBuf,
        file: File,
        parent: Option<&HeldDirectory>,
    ) -> Result<Self, String> {
        let held = file.metadata().map_err(|error| {
            format!("could not inspect held Network Reporter directory: {error}")
        })?;
        if metadata_is_link_or_reparse(&held) || !held.is_dir() {
            return Err(
                "Network Reporter held directory is linked, reparsed, or non-directory".to_string(),
            );
        }
        let identity = file_identity(&file)?;
        let visible = std::fs::symlink_metadata(&path).map_err(|error| {
            format!("could not verify visible Network Reporter directory: {error}")
        })?;
        if metadata_is_link_or_reparse(&visible) || !visible.is_dir() {
            return Err(
                "Network Reporter directory changed identity while it was opened".to_string(),
            );
        }
        #[cfg(unix)]
        if metadata_identity(&visible) != identity {
            return Err(
                "Network Reporter directory changed identity while it was opened".to_string(),
            );
        }
        #[cfg(windows)]
        if file_identity(&open_directory_path(&path).map_err(|error| {
            format!("could not re-open visible Network Reporter directory: {error}")
        })?)?
            != identity
        {
            return Err(
                "Network Reporter directory changed identity while it was opened".to_string(),
            );
        }
        Ok(Self {
            inner: Arc::new(HeldDirectoryInner {
                path,
                file,
                identity,
                _parent: parent.map(|parent| Arc::clone(&parent.inner)),
            }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub fn identity_matches_path(&self) -> Result<bool, String> {
        let visible = std::fs::symlink_metadata(self.path()).map_err(|error| {
            format!("could not recheck Network Reporter directory identity: {error}")
        })?;
        if metadata_is_link_or_reparse(&visible) || !visible.is_dir() {
            return Ok(false);
        }
        #[cfg(unix)]
        return Ok(metadata_identity(&visible) == self.inner.identity);
        #[cfg(windows)]
        return open_directory_path(self.path())
            .map_err(|error| {
                format!("could not re-open Network Reporter directory identity: {error}")
            })
            .and_then(|file| file_identity(&file))
            .map(|identity| identity == self.inner.identity);
    }

    pub fn ensure_absolute(path: &Path) -> Result<Self, String> {
        walk_absolute(path, true)?.ok_or_else(|| {
            "Network Reporter private directory disappeared during creation".to_string()
        })
    }

    pub fn open_absolute(path: &Path) -> Result<Option<Self>, String> {
        walk_absolute(path, false)
    }

    pub fn create_child(&self, name: &OsStr) -> Result<Self, String> {
        exact_relative_name(name)?;
        create_child_platform(self, name, false)
    }

    pub fn ensure_child(&self, name: &OsStr) -> Result<Self, String> {
        exact_relative_name(name)?;
        create_child_platform(self, name, true)
    }

    pub fn open_child(&self, name: &OsStr) -> Result<Option<Self>, String> {
        exact_relative_name(name)?;
        open_child_platform(self, name)
    }

    pub fn create_new_file(&self, name: &OsStr, label: &str) -> Result<File, String> {
        exact_relative_name(name)?;
        create_new_file_platform(self, name, label)
    }

    pub fn try_lock_file(&self, name: &OsStr, label: &str) -> Result<HeldFileLock, String> {
        exact_relative_name(name)?;
        lock_file_platform(self, name, label, Some(LockOwner::here(now_ms())))
    }

    /// Read a lock marker's owner record WITHOUT taking the lock.
    ///
    /// Reading first is load-bearing: acquiring the lock rewrites the marker,
    /// so a caller that probed before reading would erase the very evidence
    /// that says who wedged the queue.
    pub fn read_lock_owner(&self, name: &OsStr, label: &str) -> Result<Option<LockOwner>, String> {
        exact_relative_name(name)?;
        match self.read_bounded(name, 4 * 1024, label) {
            Ok(body) => Ok(LockOwner::decode(&body)),
            Err(_) => Ok(None),
        }
    }

    /// Whether some other owner holds this lock right now.
    ///
    /// The probe deliberately stamps no owner record: it must not overwrite
    /// the holder's evidence, and a diagnostic read must leave no trace that
    /// a later reclaim could mistake for a live claim.
    pub fn lock_is_held(&self, name: &OsStr, label: &str) -> Result<bool, String> {
        exact_relative_name(name)?;
        match lock_file_platform(self, name, label, None) {
            Ok(_) => Ok(false),
            Err(error) if error == busy(label) => Ok(true),
            Err(error) => Err(error),
        }
    }

    /// Break an abandoned lock marker by unlinking it, after the caller has
    /// PROVED the recorded holder is gone. Never call this on a `Live` or
    /// `Unknown` holder: unlinking a marker a live process holds would let a
    /// second writer into the critical section.
    pub fn break_abandoned_lock(&self, name: &OsStr, label: &str) -> Result<(), String> {
        exact_relative_name(name)?;
        self.remove_file_idempotent(name, label)
    }

    /// Briefly yield to concurrent local maintenance before starting promotion.
    /// Call only without another mutation lock. Each attempt rechecks file identity;
    /// no kernel blocking lock or extension of the caller's authority is allowed.
    pub fn promotion_lock_until(
        &self,
        name: &OsStr,
        label: &str,
        authority_deadline: std::time::Instant,
    ) -> Result<HeldFileLock, String> {
        use std::time::{Duration, Instant};
        let deadline = authority_deadline.min(Instant::now() + Duration::from_secs(2));
        loop {
            if Instant::now() >= deadline {
                return Err(busy(label));
            }
            match self.try_lock_file(name, label) {
                Err(error) if error == busy(label) => {
                    std::thread::sleep(
                        Duration::from_millis(20)
                            .min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
                result => return result,
            }
        }
    }

    pub fn write_new_bounded(
        &self,
        name: &OsStr,
        bytes: &[u8],
        limit: u64,
        label: &str,
    ) -> Result<(), String> {
        if bytes.is_empty() || bytes.len() as u64 > limit {
            return Err(format!("{label} must be 1..={limit} bytes"));
        }
        let mut file = self.create_new_file(name, label)?;
        let result = file
            .write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("could not persist {label}: {error}"));
        if let Err(error) = result {
            drop(file);
            let _ = self.remove_file_idempotent(name, label);
            return Err(error);
        }
        let held = file
            .metadata()
            .map_err(|error| format!("could not inspect written {label}: {error}"))?;
        if !held.is_file() || held.len() != bytes.len() as u64 {
            drop(file);
            let _ = self.remove_file_idempotent(name, label);
            return Err(format!("{label} changed while it was written"));
        }
        Ok(())
    }

    pub fn open_regular_file(&self, name: &OsStr, label: &str) -> Result<File, String> {
        exact_relative_name(name)?;
        open_regular_file_platform(self, name, label)
    }

    pub fn read_bounded(&self, name: &OsStr, limit: u64, label: &str) -> Result<Vec<u8>, String> {
        let mut file = self.open_regular_file(name, label)?;
        let length = file
            .metadata()
            .map_err(|error| format!("could not inspect opened {label}: {error}"))?
            .len();
        if length > limit {
            return Err(format!("{label} exceeds its {limit}-byte limit"));
        }
        let capacity = usize::try_from(length)
            .map_err(|_| format!("{label} is too large for this platform"))?;
        let mut bytes = Vec::with_capacity(capacity);
        std::io::Read::by_ref(&mut file)
            .take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| format!("could not read {label}: {error}"))?;
        if bytes.len() as u64 > limit || bytes.len() as u64 != length {
            return Err(format!("{label} changed while it was read"));
        }
        Ok(bytes)
    }

    pub fn names(&self) -> Result<Vec<OsString>, String> {
        list_names_platform(self)
    }

    pub fn rename_file(&self, from: &OsStr, to: &OsStr, label: &str) -> Result<(), String> {
        exact_relative_name(from)?;
        exact_relative_name(to)?;
        rename_file_platform(self, from, to, label)
    }

    pub fn remove_file_idempotent(&self, name: &OsStr, label: &str) -> Result<(), String> {
        exact_relative_name(name)?;
        remove_file_platform(self, name, label)
    }

    pub fn rename_child(
        &self,
        child: HeldDirectory,
        new_name: &OsStr,
    ) -> Result<HeldDirectory, String> {
        exact_relative_name(new_name)?;
        if child
            .inner
            ._parent
            .as_ref()
            .is_none_or(|parent| !Arc::ptr_eq(parent, &self.inner))
        {
            return Err("Network Reporter rename target is outside its held owner".to_string());
        }
        rename_child_platform(self, child, new_name)
    }

    pub fn remove_child_tree(&self, child: HeldDirectory) -> Result<(), String> {
        if child
            .inner
            ._parent
            .as_ref()
            .is_none_or(|parent| !Arc::ptr_eq(parent, &self.inner))
        {
            return Err("Network Reporter removal target is outside its held owner".to_string());
        }
        remove_child_tree_platform(self, child)
    }

    pub fn sync(&self) -> Result<(), String> {
        sync_directory_platform(self)
    }
}

#[cfg(unix)]
fn c_name(name: &OsStr) -> Result<std::ffi::CString, String> {
    use std::os::unix::ffi::OsStrExt;

    std::ffi::CString::new(name.as_bytes())
        .map_err(|_| "Network Reporter filesystem component contains a NUL byte".to_string())
}

#[cfg(unix)]
fn open_root_platform() -> Result<File, String> {
    use std::os::fd::FromRawFd;

    let root = b"/\0";
    // SAFETY: root is NUL terminated and the successful descriptor is moved
    // immediately into `File` ownership.
    let descriptor = unsafe {
        libc::open(
            root.as_ptr().cast(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(format!(
            "could not hold Network Reporter filesystem root: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: descriptor is newly owned.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(windows)]
fn open_directory_path(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let mut options = OpenOptions::new();
    options
        .read(true)
        // Deliberately omit FILE_SHARE_DELETE. A held directory capability
        // cannot be renamed or deleted during any descendant operation.
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    options.open(path)
}

fn walk_absolute(path: &Path, create: bool) -> Result<Option<HeldDirectory>, String> {
    if !path.is_absolute() {
        return Err("Network Reporter directory authority must be absolute".to_string());
    }

    #[cfg(unix)]
    {
        let mut current_path = PathBuf::from("/");
        let root_file = open_root_platform()?;
        let mut current = HeldDirectory::from_open_file(current_path.clone(), root_file, None)?;
        for component in path.components() {
            match component {
                Component::RootDir => continue,
                Component::Normal(name) => {
                    current_path.push(name);
                    let next = match current.open_child(name)? {
                        Some(next) => next,
                        None if create => current.ensure_child(name)?,
                        None => return Ok(None),
                    };
                    current = next;
                }
                _ => {
                    return Err(
                        "Network Reporter directory contains a non-canonical component".to_string(),
                    )
                }
            }
        }
        if current.path() != path || !current.identity_matches_path()? {
            return Err("Network Reporter directory path changed during secure walk".to_string());
        }
        Ok(Some(current))
    }

    #[cfg(windows)]
    {
        let mut components = path.components();
        let prefix = match components.next() {
            Some(Component::Prefix(prefix)) => prefix.as_os_str(),
            _ => {
                return Err("Network Reporter Windows directory has no absolute prefix".to_string())
            }
        };
        if !matches!(components.next(), Some(Component::RootDir)) {
            return Err("Network Reporter Windows directory is not rooted".to_string());
        }
        let mut current_path = PathBuf::from(prefix);
        current_path.push(Path::new(r"\"));
        let root_file = open_directory_path(&current_path)
            .map_err(|error| format!("could not hold Network Reporter filesystem root: {error}"))?;
        let mut current = HeldDirectory::from_open_file(current_path.clone(), root_file, None)?;
        for component in components {
            let Component::Normal(name) = component else {
                return Err(
                    "Network Reporter directory contains a non-canonical component".to_string(),
                );
            };
            current_path.push(name);
            let next = match current.open_child(name)? {
                Some(next) => next,
                None if create => current.ensure_child(name)?,
                None => return Ok(None),
            };
            current = next;
        }
        if !current.identity_matches_path()? {
            return Err("Network Reporter directory path changed during secure walk".to_string());
        }
        Ok(Some(current))
    }
}

#[cfg(unix)]
fn open_child_platform(
    parent: &HeldDirectory,
    name: &OsStr,
) -> Result<Option<HeldDirectory>, String> {
    use std::os::fd::{AsRawFd, FromRawFd};

    let path = parent.path().join(name);
    let name = c_name(name)?;
    // SAFETY: parent owns a live directory descriptor and name is NUL terminated.
    let descriptor = unsafe {
        libc::openat(
            parent.inner.file.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        if matches!(
            error.raw_os_error(),
            Some(libc::ELOOP) | Some(libc::ENOTDIR)
        ) {
            return Err(
                "Network Reporter directory child is linked, reparsed, or non-directory"
                    .to_string(),
            );
        }
        return Err(format!(
            "could not open held Network Reporter directory child: {error}"
        ));
    }
    // SAFETY: descriptor is newly owned.
    let file = unsafe { File::from_raw_fd(descriptor) };
    HeldDirectory::from_open_file(path, file, Some(parent)).map(Some)
}

#[cfg(windows)]
fn open_child_platform(
    parent: &HeldDirectory,
    name: &OsStr,
) -> Result<Option<HeldDirectory>, String> {
    let path = parent.path().join(name);
    let file = match open_directory_path(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "could not open held Network Reporter directory child: {error}"
            ))
        }
    };
    HeldDirectory::from_open_file(path, file, Some(parent)).map(Some)
}

#[cfg(unix)]
fn create_child_platform(
    parent: &HeldDirectory,
    name: &OsStr,
    allow_existing: bool,
) -> Result<HeldDirectory, String> {
    use std::os::fd::AsRawFd;

    let encoded = c_name(name)?;
    // SAFETY: parent descriptor and encoded name remain live for mkdirat.
    if unsafe { libc::mkdirat(parent.inner.file.as_raw_fd(), encoded.as_ptr(), 0o700) } != 0 {
        let error = std::io::Error::last_os_error();
        if !(allow_existing && error.kind() == std::io::ErrorKind::AlreadyExists) {
            return Err(format!(
                "could not create held Network Reporter directory child: {error}"
            ));
        }
    }
    parent.open_child(name)?.ok_or_else(|| {
        "Network Reporter directory child disappeared after relative creation".to_string()
    })
}

#[cfg(windows)]
fn create_child_platform(
    parent: &HeldDirectory,
    name: &OsStr,
    allow_existing: bool,
) -> Result<HeldDirectory, String> {
    let path = parent.path().join(name);
    match std::fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if allow_existing && error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(format!(
                "could not create held Network Reporter directory child: {error}"
            ))
        }
    }
    parent.open_child(name)?.ok_or_else(|| {
        "Network Reporter directory child disappeared after held creation".to_string()
    })
}

#[cfg(unix)]
fn create_new_file_platform(
    parent: &HeldDirectory,
    name: &OsStr,
    label: &str,
) -> Result<File, String> {
    use std::os::fd::{AsRawFd, FromRawFd};

    let encoded = c_name(name)?;
    // SAFETY: parent descriptor and encoded name remain live for openat.
    let descriptor = unsafe {
        libc::openat(
            parent.inner.file.as_raw_fd(),
            encoded.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if descriptor < 0 {
        return Err(format!(
            "could not create {label} relative to its held directory: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: descriptor is newly owned.
    let file = unsafe { File::from_raw_fd(descriptor) };
    verify_open_file_at(parent, name, &file, label)?;
    Ok(file)
}

#[cfg(windows)]
fn create_new_file_platform(
    parent: &HeldDirectory,
    name: &OsStr,
    label: &str,
) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let path = parent.path().join(name);
    let mut options = OpenOptions::new();
    let file = options
        .write(true)
        .create_new(true)
        // Deny delete and additional writers while the newly created file is
        // initialized and verified.
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&path)
        .map_err(|error| format!("could not create {label} in held directory: {error}"))?;
    verify_open_file_at(parent, name, &file, label)?;
    Ok(file)
}

#[cfg(unix)]
fn open_regular_file_platform(
    parent: &HeldDirectory,
    name: &OsStr,
    label: &str,
) -> Result<File, String> {
    use std::os::fd::{AsRawFd, FromRawFd};

    let encoded = c_name(name)?;
    // SAFETY: parent descriptor and encoded name remain live for openat.
    let descriptor = unsafe {
        libc::openat(
            parent.inner.file.as_raw_fd(),
            encoded.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        let error = std::io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(libc::ELOOP) | Some(libc::EISDIR)) {
            return Err(format!("{label} is not a regular file"));
        }
        return Err(format!(
            "could not open {label} relative to its held directory: {}",
            error
        ));
    }
    // SAFETY: descriptor is newly owned.
    let file = unsafe { File::from_raw_fd(descriptor) };
    verify_open_file_at(parent, name, &file, label)?;
    Ok(file)
}

#[cfg(windows)]
fn open_regular_file_platform(
    parent: &HeldDirectory,
    name: &OsStr,
    label: &str,
) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    let path = parent.path().join(name);
    let mut options = OpenOptions::new();
    let file = options
        .read(true)
        // Deny delete and writes for the full receipt/hash read.
        .share_mode(FILE_SHARE_READ)
        // Open directory handles too, so the held metadata check can return
        // the same non-regular-file refusal as Unix before any byte read.
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(&path)
        .map_err(|error| format!("could not open {label} in held directory: {error}"))?;
    verify_open_file_at(parent, name, &file, label)?;
    Ok(file)
}

/// The one sentence contention is named with. `promotion_lock_until` retries on
/// exactly this, so every platform branch must produce it for "another owner
/// holds the marker" and for nothing else.
pub(crate) fn busy(label: &str) -> String {
    format!("{label} is busy; retry the operation")
}

/// This machine's wall clock in milliseconds, or 0 when it predates the epoch.
/// Lock records are compared against the same reading, and a clock that cannot
/// be read only costs the heartbeat half of the evidence.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            elapsed.as_millis().min(u64::MAX as u128) as u64
        })
}

/// Stamp the acquiring owner into the marker it just locked.
fn stamp_owner(file: &mut File, owner: &LockOwner, label: &str) -> Result<(), String> {
    let body = owner.encoded()?;
    file.seek(SeekFrom::Start(0))
        .and_then(|_| file.set_len(0))
        .and_then(|_| file.write_all(&body))
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("could not persist {label} owner evidence: {error}"))
}

#[cfg(unix)]
fn lock_file_platform(
    parent: &HeldDirectory,
    name: &OsStr,
    label: &str,
    stamp: Option<LockOwner>,
) -> Result<HeldFileLock, String> {
    use std::os::fd::{AsRawFd, FromRawFd};

    let encoded = c_name(name)?;
    // SAFETY: parent owns a live directory descriptor and encoded is a
    // NUL-terminated single component. The successful descriptor is moved
    // immediately into File ownership.
    let descriptor = unsafe {
        libc::openat(
            parent.inner.file.as_raw_fd(),
            encoded.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if descriptor < 0 {
        return Err(format!(
            "could not open {label} relative to its held directory: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: descriptor is newly owned.
    let mut file = unsafe { File::from_raw_fd(descriptor) };
    verify_open_file_at(parent, name, &file, label)?;
    // SAFETY: file owns a live descriptor. LOCK_NB is load-bearing: callers
    // must never wait behind a suspended lane while holding other authority.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = std::io::Error::last_os_error();
        let code = error.raw_os_error();
        if code == Some(libc::EWOULDBLOCK) || code == Some(libc::EAGAIN) {
            return Err(busy(label));
        }
        return Err(format!("could not acquire {label}: {error}"));
    }
    // Recheck the visible identity after acquiring the advisory lock. This
    // rejects a replaced marker before entering the critical section.
    verify_open_file_at(parent, name, &file, label)?;
    if let Some(owner) = stamp.as_ref() {
        stamp_owner(&mut file, owner, label)?;
    }
    Ok(HeldFileLock { file, owner: stamp })
}

#[cfg(windows)]
fn lock_file_platform(
    parent: &HeldDirectory,
    name: &OsStr,
    label: &str,
    stamp: Option<LockOwner>,
) -> Result<HeldFileLock, String> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_LOCK_VIOLATION: i32 = 33;

    let path = parent.path().join(name);
    let mut options = OpenOptions::new();
    let opened = options
        .read(true)
        .write(true)
        .create(true)
        // A live owner permits diagnostics to read the marker but denies a
        // second writer and denies deletion/rename. The operating system
        // closes a dead holder's handle, so on this platform "the marker
        // opens" is itself the liveness proof; the owner record below exists
        // to NAME the holder and to carry the heartbeat, which is what a
        // filesystem that does not release handles (a network share, a
        // suspended container) leaves as the only evidence.
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&path);
    // The refusal a live owner produces is a sharing violation, which is
    // contention and not failure. Naming it as such is what gives Windows the
    // same bounded retry the POSIX branch gets from EWOULDBLOCK; without it a
    // second report finishing in the same second failed outright.
    let mut file = match opened {
        Ok(file) => file,
        Err(error)
            if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION)
                || error.raw_os_error() == Some(ERROR_LOCK_VIOLATION) =>
        {
            return Err(busy(label));
        }
        Err(error) => return Err(format!("could not acquire {label}: {error}")),
    };
    verify_open_file_at(parent, name, &file, label)?;
    if let Some(owner) = stamp.as_ref() {
        stamp_owner(&mut file, owner, label)?;
    }
    Ok(HeldFileLock { file, owner: stamp })
}

fn verify_open_file_at(
    parent: &HeldDirectory,
    name: &OsStr,
    file: &File,
    label: &str,
) -> Result<(), String> {
    let held = file
        .metadata()
        .map_err(|error| format!("could not inspect opened {label}: {error}"))?;
    if metadata_is_link_or_reparse(&held) || !held.is_file() {
        return Err(format!("{label} is not a held regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;

        let encoded = c_name(name)?;
        let mut visible: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: parent/name/output storage are valid; do not follow a final
        // component link when comparing it with the opened file.
        if unsafe {
            libc::fstatat(
                parent.inner.file.as_raw_fd(),
                encoded.as_ptr(),
                &mut visible,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(format!(
                "could not recheck opened {label}: {}",
                std::io::Error::last_os_error()
            ));
        }
        let visible_identity = FileIdentity {
            first: visible.st_dev as u64,
            second: visible.st_ino as u64,
        };
        if (visible.st_mode & libc::S_IFMT) != libc::S_IFREG
            || visible_identity != metadata_identity(&held)
        {
            return Err(format!("{label} changed identity while it was opened"));
        }
    }
    #[cfg(windows)]
    {
        let visible = std::fs::symlink_metadata(parent.path().join(name))
            .map_err(|error| format!("could not recheck opened {label}: {error}"))?;
        if metadata_is_link_or_reparse(&visible) || !visible.is_file() {
            return Err(format!("{label} changed identity while it was opened"));
        }
        let visible_file = open_regular_file_path_windows(&parent.path().join(name), label)?;
        if file_identity(&visible_file)? != file_identity(file)? {
            return Err(format!("{label} changed identity while it was opened"));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn open_regular_file_path_windows(path: &Path, label: &str) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let mut options = OpenOptions::new();
    options
        // Attributes-only identity probe. Requesting zero access exempts this
        // open from the Win32 sharing check, so it can never fail with a
        // sharing violation against a live writer — including the caller's own
        // held write handle during create/lock verification, which a
        // read-access probe with FILE_SHARE_READ deterministically did
        // (os error 32). GetFileInformationByHandle works on such a handle;
        // the security decision is the identity comparison against the held
        // handle, not this probe's share mode.
        .access_mode(0)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| format!("could not re-open visible {label}: {error}"))
}

#[cfg(unix)]
struct DirectoryStream(*mut libc::DIR);

#[cfg(unix)]
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        // SAFETY: DirectoryStream uniquely owns the DIR pointer.
        unsafe {
            libc::closedir(self.0);
        }
    }
}

#[cfg(unix)]
fn list_names_platform(directory: &HeldDirectory) -> Result<Vec<OsString>, String> {
    use std::ffi::CStr;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStringExt;

    // A duplicated directory descriptor shares its enumeration offset with
    // the original open-file description. Reusing `names()` would therefore
    // see EOF and could make a non-empty rollback target look empty. Open `.`
    // relative to the held capability to obtain an independent description;
    // fdopendir then owns it and DirectoryStream closes it.
    let dot = b".\0";
    // SAFETY: the held descriptor and NUL-terminated relative component are
    // valid for the call; the successful descriptor is transferred below.
    let duplicate = unsafe {
        libc::openat(
            directory.inner.file.as_raw_fd(),
            dot.as_ptr().cast(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if duplicate < 0 {
        return Err(format!(
            "could not re-open held Network Reporter directory for enumeration: {}",
            std::io::Error::last_os_error()
        ));
    }
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        // SAFETY: fdopendir did not consume the descriptor on failure.
        unsafe { libc::close(duplicate) };
        return Err(format!(
            "could not enumerate held Network Reporter directory: {}",
            std::io::Error::last_os_error()
        ));
    }
    let stream = DirectoryStream(stream);
    let mut names = Vec::new();
    loop {
        // readdir returns null for EOF. An I/O error will be observed again by
        // the following handle-relative fstat/open operation and fails closed.
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            break;
        }
        // SAFETY: d_name is NUL terminated for a live dirent.
        let raw = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if raw == b"." || raw == b".." {
            continue;
        }
        names.push(OsString::from_vec(raw.to_vec()));
    }
    Ok(names)
}

#[cfg(windows)]
fn list_names_platform(directory: &HeldDirectory) -> Result<Vec<OsString>, String> {
    std::fs::read_dir(directory.path())
        .map_err(|error| format!("could not enumerate held Network Reporter directory: {error}"))?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(|error| format!("could not enumerate Network Reporter member: {error}"))
        })
        .collect()
}

#[cfg(unix)]
fn rename_file_platform(
    parent: &HeldDirectory,
    from: &OsStr,
    to: &OsStr,
    label: &str,
) -> Result<(), String> {
    use std::os::fd::AsRawFd;

    let held = parent.open_regular_file(from, label)?;
    let identity = file_identity(&held)?;
    let to_name = to.to_os_string();
    let from = c_name(from)?;
    let to = c_name(to)?;
    // SAFETY: both names are relative to the same held directory descriptor.
    if unsafe {
        libc::renameat(
            parent.inner.file.as_raw_fd(),
            from.as_ptr(),
            parent.inner.file.as_raw_fd(),
            to.as_ptr(),
        )
    } != 0
    {
        return Err(format!(
            "could not rename {label} inside its held directory: {}",
            std::io::Error::last_os_error()
        ));
    }
    let renamed = parent.open_regular_file(&to_name, label)?;
    let renamed_identity = file_identity(&renamed)?;
    if renamed_identity != identity {
        return Err(format!("{label} changed identity during relative rename"));
    }
    parent.sync()
}

#[cfg(windows)]
fn rename_file_platform(
    parent: &HeldDirectory,
    from: &OsStr,
    to: &OsStr,
    label: &str,
) -> Result<(), String> {
    let held = parent.open_regular_file(from, label)?;
    let identity = file_identity(&held)?;
    // Windows refuses rename while our no-delete-share file handle is live.
    // The parent and all its ancestors remain held, so the operation cannot
    // leave the capability subtree during this narrow close/rename/reopen.
    drop(held);
    std::fs::rename(parent.path().join(from), parent.path().join(to))
        .map_err(|error| format!("could not rename {label} inside held directory: {error}"))?;
    let renamed = parent.open_regular_file(to, label)?;
    if file_identity(&renamed)? != identity {
        return Err(format!("{label} changed identity during held rename"));
    }
    Ok(())
}

#[cfg(unix)]
fn remove_file_platform(parent: &HeldDirectory, name: &OsStr, label: &str) -> Result<(), String> {
    use std::os::fd::AsRawFd;

    let encoded = c_name(name)?;
    // SAFETY: unlinkat removes the directory entry itself and never follows a
    // symlink; the name is relative to the held parent.
    if unsafe { libc::unlinkat(parent.inner.file.as_raw_fd(), encoded.as_ptr(), 0) } == 0 {
        parent.sync()?;
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(format!(
            "could not remove {label} from held directory: {error}"
        ))
    }
}

#[cfg(windows)]
fn remove_file_platform(parent: &HeldDirectory, name: &OsStr, label: &str) -> Result<(), String> {
    let path = parent.path().join(name);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "could not remove {label} from held directory: {error}"
        )),
    }
}

#[cfg(unix)]
fn rename_child_platform(
    parent: &HeldDirectory,
    child: HeldDirectory,
    new_name: &OsStr,
) -> Result<HeldDirectory, String> {
    use std::os::fd::AsRawFd;

    let old_name = child
        .path()
        .file_name()
        .ok_or_else(|| "Network Reporter held child has no relative name".to_string())?;
    let old = c_name(old_name)?;
    let new = c_name(new_name)?;
    let identity = child.inner.identity;
    if rename_directory_noreplace(parent.inner.file.as_raw_fd(), old.as_ptr(), new.as_ptr()) != 0 {
        return Err(format!(
            "could not rename held Network Reporter directory: {}",
            std::io::Error::last_os_error()
        ));
    }
    let renamed = parent
        .open_child(new_name)?
        .ok_or_else(|| "renamed Network Reporter directory disappeared".to_string())?;
    if renamed.inner.identity != identity {
        return Err(
            "Network Reporter directory changed identity during relative rename".to_string(),
        );
    }
    parent.sync()?;
    Ok(renamed)
}

#[cfg(target_os = "linux")]
fn rename_directory_noreplace(
    directory_fd: std::os::raw::c_int,
    old: *const std::os::raw::c_char,
    new: *const std::os::raw::c_char,
) -> std::os::raw::c_int {
    // SAFETY: both pointers are live NUL-terminated component names and both
    // directory arguments are the same held descriptor. RENAME_NOREPLACE is
    // the kernel-enforced publication collision boundary.
    unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            directory_fd,
            old,
            directory_fd,
            new,
            libc::RENAME_NOREPLACE,
        ) as std::os::raw::c_int
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn rename_directory_noreplace(
    directory_fd: std::os::raw::c_int,
    old: *const std::os::raw::c_char,
    new: *const std::os::raw::c_char,
) -> std::os::raw::c_int {
    // SAFETY: renameatx_np receives two live relative component pointers and
    // the held parent descriptor; RENAME_EXCL supplies atomic no-replace.
    unsafe { libc::renameatx_np(directory_fd, old, directory_fd, new, libc::RENAME_EXCL) }
}

#[cfg(all(
    unix,
    not(target_os = "linux"),
    not(target_os = "macos"),
    not(target_os = "ios")
))]
fn rename_directory_noreplace(
    _directory_fd: std::os::raw::c_int,
    _old: *const std::os::raw::c_char,
    _new: *const std::os::raw::c_char,
) -> std::os::raw::c_int {
    // There is no portable no-replace directory rename. Refuse publication
    // instead of emulating it with a check-then-rename race.
    -1
}

#[cfg(windows)]
fn rename_child_platform(
    parent: &HeldDirectory,
    child: HeldDirectory,
    new_name: &OsStr,
) -> Result<HeldDirectory, String> {
    let old_path = child.path().to_path_buf();
    let identity = child.inner.identity;
    if !child.identity_matches_path()? {
        return Err("Network Reporter directory changed before held rename".to_string());
    }
    drop(child);
    let new_path = parent.path().join(new_name);
    std::fs::rename(&old_path, &new_path)
        .map_err(|error| format!("could not rename held Network Reporter directory: {error}"))?;
    let renamed = parent
        .open_child(new_name)?
        .ok_or_else(|| "renamed Network Reporter directory disappeared".to_string())?;
    if renamed.inner.identity != identity {
        return Err("Network Reporter directory changed identity during held rename".to_string());
    }
    Ok(renamed)
}

#[cfg(unix)]
fn remove_directory_contents(directory: &HeldDirectory) -> Result<(), String> {
    use std::os::fd::AsRawFd;

    for name in directory.names()? {
        let encoded = c_name(&name)?;
        let mut status: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: output storage and relative name are valid; links are not followed.
        if unsafe {
            libc::fstatat(
                directory.inner.file.as_raw_fd(),
                encoded.as_ptr(),
                &mut status,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::NotFound {
                continue;
            }
            return Err(format!(
                "could not inspect held Network Reporter member: {error}"
            ));
        }
        if (status.st_mode & libc::S_IFMT) == libc::S_IFDIR {
            let child = directory
                .open_child(&name)?
                .ok_or_else(|| "Network Reporter directory member disappeared".to_string())?;
            remove_directory_contents(&child)?;
            let expected = child.inner.identity;
            let mut visible: libc::stat = unsafe { std::mem::zeroed() };
            if unsafe {
                libc::fstatat(
                    directory.inner.file.as_raw_fd(),
                    encoded.as_ptr(),
                    &mut visible,
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
                || (FileIdentity {
                    first: visible.st_dev as u64,
                    second: visible.st_ino as u64,
                }) != expected
            {
                return Err("Network Reporter directory member changed before removal".to_string());
            }
            // SAFETY: unlinkat removes the verified empty direct child only.
            if unsafe {
                libc::unlinkat(
                    directory.inner.file.as_raw_fd(),
                    encoded.as_ptr(),
                    libc::AT_REMOVEDIR,
                )
            } != 0
            {
                return Err(format!(
                    "could not remove held Network Reporter directory member: {}",
                    std::io::Error::last_os_error()
                ));
            }
        } else {
            // SAFETY: unlinking a non-directory direct member never follows it.
            if unsafe { libc::unlinkat(directory.inner.file.as_raw_fd(), encoded.as_ptr(), 0) } != 0
            {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err(format!(
                        "could not remove held Network Reporter file member: {error}"
                    ));
                }
            }
        }
    }
    directory.sync()
}

#[cfg(windows)]
fn remove_directory_contents(directory: &HeldDirectory) -> Result<(), String> {
    for name in directory.names()? {
        let path = directory.path().join(&name);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "could not inspect held Network Reporter member: {error}"
                ))
            }
        };
        if metadata.is_dir() && !metadata_is_link_or_reparse(&metadata) {
            let child = directory
                .open_child(&name)?
                .ok_or_else(|| "Network Reporter directory member disappeared".to_string())?;
            remove_directory_contents(&child)?;
            let identity = child.inner.identity;
            drop(child);
            let visible = std::fs::symlink_metadata(&path).map_err(|error| {
                format!("Network Reporter directory member changed before removal: {error}")
            })?;
            if metadata_is_link_or_reparse(&visible) || !visible.is_dir() {
                return Err("Network Reporter directory member changed before removal".to_string());
            }
            let visible_handle = open_directory_path(&path).map_err(|error| {
                format!("could not re-open Network Reporter directory member: {error}")
            })?;
            if file_identity(&visible_handle)? != identity {
                return Err("Network Reporter directory member changed before removal".to_string());
            }
            drop(visible_handle);
            std::fs::remove_dir(&path).map_err(|error| {
                format!("could not remove held Network Reporter directory member: {error}")
            })?;
        } else if metadata.is_dir() {
            // Remove a junction/reparse directory itself, never its target.
            std::fs::remove_dir(&path).map_err(|error| {
                format!("could not remove Network Reporter reparse member: {error}")
            })?;
        } else {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "could not remove held Network Reporter file member: {error}"
                    ))
                }
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn remove_child_tree_platform(parent: &HeldDirectory, child: HeldDirectory) -> Result<(), String> {
    use std::os::fd::AsRawFd;

    remove_directory_contents(&child)?;
    let name = child
        .path()
        .file_name()
        .ok_or_else(|| "Network Reporter held child has no relative name".to_string())?;
    let encoded = c_name(name)?;
    let mut visible: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: output storage and relative name are valid.
    if unsafe {
        libc::fstatat(
            parent.inner.file.as_raw_fd(),
            encoded.as_ptr(),
            &mut visible,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
        || (FileIdentity {
            first: visible.st_dev as u64,
            second: visible.st_ino as u64,
        }) != child.inner.identity
    {
        return Err("Network Reporter held directory changed before exact removal".to_string());
    }
    // SAFETY: remove the verified empty direct child relative to held parent.
    if unsafe {
        libc::unlinkat(
            parent.inner.file.as_raw_fd(),
            encoded.as_ptr(),
            libc::AT_REMOVEDIR,
        )
    } != 0
    {
        return Err(format!(
            "could not remove exact held Network Reporter directory: {}",
            std::io::Error::last_os_error()
        ));
    }
    parent.sync()
}

#[cfg(windows)]
fn remove_child_tree_platform(parent: &HeldDirectory, child: HeldDirectory) -> Result<(), String> {
    remove_directory_contents(&child)?;
    let path = child.path().to_path_buf();
    let identity = child.inner.identity;
    if !child.identity_matches_path()? {
        return Err("Network Reporter held directory changed before exact removal".to_string());
    }
    drop(child);
    let visible = match std::fs::symlink_metadata(&path) {
        Ok(visible) => visible,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "could not recheck Network Reporter directory for removal: {error}"
            ))
        }
    };
    if metadata_is_link_or_reparse(&visible) || !visible.is_dir() {
        return Err("Network Reporter held directory changed before exact removal".to_string());
    }
    let visible_handle = open_directory_path(&path).map_err(|error| {
        format!("could not re-open Network Reporter directory for removal: {error}")
    })?;
    if file_identity(&visible_handle)? != identity {
        return Err("Network Reporter held directory changed before exact removal".to_string());
    }
    drop(visible_handle);
    // Every ancestor including parent remains open without FILE_SHARE_DELETE.
    std::fs::remove_dir(&path).map_err(|error| {
        format!("could not remove exact held Network Reporter directory: {error}")
    })?;
    if !parent.identity_matches_path()? {
        return Err("Network Reporter held parent changed during exact removal".to_string());
    }
    Ok(())
}

#[cfg(unix)]
fn sync_directory_platform(directory: &HeldDirectory) -> Result<(), String> {
    directory
        .inner
        .file
        .sync_all()
        .map_err(|error| format!("could not sync held Network Reporter directory: {error}"))
}

#[cfg(windows)]
fn sync_directory_platform(_directory: &HeldDirectory) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let root = std::fs::canonicalize(std::env::temp_dir())
                .expect("canonical platform temporary directory")
                .join(format!(
                    "ds-web-network-reporter-fs-{label}-{}-{:016x}",
                    std::process::id(),
                    rand::random::<u64>()
                ));
            std::fs::create_dir_all(&root).expect("create test directory");
            Self(root)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn held_directory_write_stays_beneath_original_after_ancestor_swap() {
        use std::os::unix::fs::symlink;

        let test = TestDirectory::new("ancestor-swap-write");
        let owned = test.0.join("owned");
        let original = owned.join("queue");
        let outside = test.0.join("outside");
        std::fs::create_dir_all(&original).unwrap();
        std::fs::create_dir_all(outside.join("queue")).unwrap();
        let held = HeldDirectory::open_absolute(&original).unwrap().unwrap();

        let moved = test.0.join("moved-owned");
        std::fs::rename(&owned, &moved).unwrap();
        symlink(&outside, &owned).unwrap();
        held.write_new_bounded(
            OsStr::new("receipt.json"),
            b"held bytes",
            1024,
            "ancestor-swap fixture",
        )
        .unwrap();

        assert_eq!(
            std::fs::read(moved.join("queue/receipt.json")).unwrap(),
            b"held bytes"
        );
        assert!(!outside.join("queue/receipt.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn held_tree_removal_cannot_follow_swapped_ancestor() {
        use std::os::unix::fs::symlink;

        let test = TestDirectory::new("ancestor-swap-remove");
        let owned = test.0.join("owned");
        let child_path = owned.join("run");
        let outside = test.0.join("outside");
        std::fs::create_dir_all(&child_path).unwrap();
        std::fs::write(child_path.join("staged.bin"), b"staged").unwrap();
        std::fs::create_dir_all(outside.join("run")).unwrap();
        std::fs::write(outside.join("run/keep.bin"), b"outside").unwrap();
        let parent = HeldDirectory::open_absolute(&owned).unwrap().unwrap();
        let child = parent.open_child(OsStr::new("run")).unwrap().unwrap();

        let moved = test.0.join("moved-owned");
        std::fs::rename(&owned, &moved).unwrap();
        symlink(&outside, &owned).unwrap();
        parent.remove_child_tree(child).unwrap();

        assert!(!moved.join("run").exists());
        assert_eq!(
            std::fs::read(outside.join("run/keep.bin")).unwrap(),
            b"outside"
        );
    }

    #[cfg(unix)]
    #[test]
    fn promotion_lock_yields_to_short_maintenance_but_respects_deadline() {
        use std::time::{Duration, Instant};
        let test = TestDirectory::new("promotion-contention");
        let held = HeldDirectory::open_absolute(&test.0).unwrap().unwrap();
        let name = OsStr::new(".publication.lock");
        let first = held.try_lock_file(name, "publication lock").unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            drop(first);
        });
        let acquired = held
            .promotion_lock_until(
                name,
                "publication lock",
                Instant::now() + Duration::from_secs(1),
            )
            .unwrap();
        release.join().unwrap();
        let start = Instant::now();
        assert!(held
            .promotion_lock_until(name, "publication lock", start + Duration::from_millis(50))
            .unwrap_err()
            .contains("busy"));
        assert!(start.elapsed() < Duration::from_secs(1));
        drop(acquired);
        assert!(held
            .promotion_lock_until(name, "publication lock", Instant::now())
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn held_file_lock_is_exclusive_and_process_crash_releasable() {
        let test = TestDirectory::new("exclusive-lock");
        let held = HeldDirectory::open_absolute(&test.0).unwrap().unwrap();
        let first = held
            .try_lock_file(OsStr::new(".publication.lock"), "publication lock")
            .expect("first owner acquires lock");
        assert!(held
            .try_lock_file(OsStr::new(".publication.lock"), "publication lock")
            .expect_err("second owner must fail immediately")
            .contains("busy"));
        drop(first);
        held.try_lock_file(OsStr::new(".publication.lock"), "publication lock")
            .expect("descriptor close releases lock");
    }

    #[cfg(unix)]
    #[test]
    fn held_file_lock_rejects_a_linked_marker() {
        use std::os::unix::fs::symlink;

        let test = TestDirectory::new("linked-lock");
        let outside = test.0.join("outside.lock");
        std::fs::write(&outside, b"outside").unwrap();
        symlink(&outside, test.0.join(".publication.lock")).unwrap();
        let held = HeldDirectory::open_absolute(&test.0).unwrap().unwrap();
        assert!(held
            .try_lock_file(OsStr::new(".publication.lock"), "publication lock")
            .expect_err("linked marker must fail")
            .contains("could not open"));
    }

    #[test]
    fn windows_source_holds_ancestors_and_opens_reparse_points_without_delete_share() {
        let source = include_str!("confined_fs.rs");
        assert!(source.contains("FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT"));
        assert!(source.contains(".share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)"));
        assert!(source.contains("Deliberately omit FILE_SHARE_DELETE"));
        assert!(source.contains(".share_mode(FILE_SHARE_READ)"));
        assert!(source.contains("fn lock_file_platform"));
        assert!(source.contains("write(true)"));
    }

    #[test]
    fn windows_identity_probe_is_attributes_only_and_share_exempt() {
        // The visible-path identity recheck runs while the caller may still
        // hold a WRITE handle on the very same file (create_new_file and
        // try_lock_file verify before releasing). A probe that requests read
        // access with FILE_SHARE_READ is refused by Win32 against that held
        // writer — ERROR_SHARING_VIOLATION, the field "os error 32" that broke
        // every desktop report export. The probe must stay attributes-only
        // (access 0) with full sharing so it is exempt from the share check.
        let source = include_str!("confined_fs.rs");
        assert!(source.contains(".access_mode(0)"));
        assert!(
            source.contains(".share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)")
        );
    }

    #[test]
    fn staged_write_and_lock_verify_while_their_own_write_handles_are_live() {
        // Cross-platform regression for the Windows os-error-32 failure:
        // write_new_bounded holds its freshly created WRITE handle while
        // verify_open_file_at re-opens the visible path, and try_lock_file
        // does the same with a read+write handle. On Windows this test drives
        // the real sharing semantics; on Unix it pins the shared call shape.
        let test = TestDirectory::new("verify-under-own-writer");
        let held = HeldDirectory::open_absolute(&test.0).unwrap().unwrap();
        held.write_new_bounded(
            OsStr::new("transformer.json"),
            b"{\"layers\":{}}",
            1024,
            "test transformer input",
        )
        .expect("staged write verifies while its own write handle is live");
        let lock = held
            .try_lock_file(OsStr::new(".publication.lock"), "test publication lock")
            .expect("lock verifies while its own read+write handle is live");
        drop(lock);
    }

    #[test]
    fn unix_publication_rename_is_kernel_no_replace() {
        let source = include_str!("confined_fs.rs");
        assert!(source.contains("libc::SYS_renameat2"));
        assert!(source.contains("libc::RENAME_NOREPLACE"));
        assert!(source.contains("libc::RENAME_EXCL"));
    }
}
