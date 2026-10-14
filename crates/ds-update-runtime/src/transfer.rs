//! Shared exact-artifact download transport for native installers.
//!
//! Both optional components and whole-application updates use this module. It
//! owns the retry/resume, byte ceiling, link/reparse refusal, and SHA-256
//! verification so those two installer surfaces cannot drift apart.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

const MAX_DOWNLOAD_BYTES: u64 = 16 * 1024 * 1024 * 1024;
// Windows installer downloads routinely cross slow or briefly quiet field
// links. A short whole-request timeout makes a healthy large transfer fail,
// while an unbounded wait can strand the native worker forever. Keep connection
// setup and response headers bounded separately from the end-to-end transfer.
// ureq's recv_body budget is TOTAL duration, not an inactivity timeout. Never
// give it the short header budget: a healthy slow transfer would be interrupted.
const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const DOWNLOAD_OVERALL_TIMEOUT: Duration = Duration::from_secs(8 * 60 * 60);
const DOWNLOAD_ATTEMPTS: u32 = 12;
// Twelve back-to-back attempts span seconds, so a refusal that clears within
// a minute or two was reported as permanent: the wetlands bundle answered 422
// twelve times, then served the same request unprompted (feedback 33003988).
// Doubling from two seconds, capped at thirty, spreads the attempts over about
// four minutes; a cancellation still ends the wait at once.
const DOWNLOAD_RETRY_BASE: Duration = Duration::from_secs(2);
const DOWNLOAD_RETRY_CAP: Duration = Duration::from_secs(30);

/// Downloads in flight in this process, by partial file. A second request for
/// the same destination used to reopen the `.part` the first was still
/// writing: on Windows every attempt failed at once with a sharing violation
/// ("open existing partial download safely"), the whole attempt budget was
/// spent in seconds, and the first download then finished by itself, so a
/// dataset that installed fine was reported as a permanent failure
/// (feedback 33003988). The second request is now told what is happening.
static IN_FLIGHT: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
    std::sync::LazyLock::new(Default::default);

struct InFlightDownload(PathBuf);

impl InFlightDownload {
    fn claim(partial: &Path, label: &str) -> Result<Self, String> {
        let mut held = IN_FLIGHT
            .lock()
            .map_err(|_| format!("{label} download ledger is unavailable"))?;
        if !held.insert(partial.to_path_buf()) {
            return Err(format!(
                "{label} is already downloading in this Desktop; wait for that download to finish, then retry"
            ));
        }
        Ok(Self(partial.to_path_buf()))
    }
}

impl Drop for InFlightDownload {
    fn drop(&mut self) {
        if let Ok(mut held) = IN_FLIGHT.lock() {
            held.remove(&self.0);
        }
    }
}

/// The pause after the failed attempt numbered `attempt` (1-based).
fn retry_delay(attempt: u32) -> Duration {
    let factor = 1u32
        .checked_shl(attempt.saturating_sub(1))
        .unwrap_or(u32::MAX);
    DOWNLOAD_RETRY_BASE
        .saturating_mul(factor)
        .min(DOWNLOAD_RETRY_CAP)
}

fn wait_before_retry<C>(delay: Duration, check_cancelled: C) -> Result<(), String>
where
    C: Fn() -> Result<(), String>,
{
    let deadline = Instant::now() + delay;
    loop {
        check_cancelled()?;
        let now = Instant::now();
        if now >= deadline {
            return Ok(());
        }
        std::thread::sleep((deadline - now).min(Duration::from_millis(250)));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransferTimeouts {
    connect: Duration,
    idle: Duration,
    overall: Duration,
}

fn transfer_timeouts_after(elapsed: Duration) -> Option<TransferTimeouts> {
    let remaining = DOWNLOAD_OVERALL_TIMEOUT.checked_sub(elapsed)?;
    if remaining.is_zero() {
        return None;
    }
    Some(TransferTimeouts {
        connect: DOWNLOAD_CONNECT_TIMEOUT.min(remaining),
        idle: DOWNLOAD_IDLE_TIMEOUT.min(remaining),
        overall: remaining,
    })
}

#[derive(Clone, Debug)]
pub struct TransferProgress {
    pub status: &'static str,
    pub progress: u8,
    pub message: String,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
}

/// Keeps verified bytes open without write/delete sharing on Windows until
/// the caller has activated or launched them.
pub struct VerifiedDownload {
    path: PathBuf,
    _held: std::fs::File,
}

impl VerifiedDownload {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(not(windows))]
pub fn metadata_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
pub fn metadata_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_type().is_symlink() || metadata.file_attributes() & 0x0000_0400 != 0
}

// The ds-layer-store/private.rs ownership rule explicitly permits the same
// 0700 mode inline where reaching that crate would add a product dependency.
fn create_owned_directory(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}
fn tighten_owned_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.is_dir() && !metadata_is_link_or_reparse(&metadata) {
            std::fs::set_permissions(
                path,
                std::fs::Permissions::from_mode(metadata.permissions().mode() & 0o700),
            )?;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub fn ensure_owned_directory(base: &Path, relative: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(base)
        .map_err(|error| format!("create owned base {}: {error}", base.display()))?;
    let base_metadata = std::fs::symlink_metadata(base)
        .map_err(|error| format!("inspect owned base {}: {error}", base.display()))?;
    if metadata_is_link_or_reparse(&base_metadata) || !base_metadata.is_dir() {
        return Err(format!(
            "owned base is linked, reparsed, or not a directory: {}",
            base.display()
        ));
    }
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("owned directory has an unsafe relative path".to_string());
        };
        current.push(name);
        // Every component below the base is DS-owned: created 0700, and one
        // from before the owner rule tightened (best effort, never through a
        // link). The base itself may be the platform's shared local-data
        // directory, so it keeps whatever modes it has.
        match create_owned_directory(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = tighten_owned_directory(&current);
            }
            Err(error) => {
                return Err(format!(
                    "create owned directory {}: {error}",
                    current.display()
                ));
            }
        }
        let metadata = std::fs::symlink_metadata(&current)
            .map_err(|error| format!("inspect owned directory {}: {error}", current.display()))?;
        if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
            return Err(format!(
                "owned directory is linked, reparsed, or not a directory: {}",
                current.display()
            ));
        }
    }
    Ok(current)
}

pub fn create_fresh_private_directory(parent: &Path, prefix: &str) -> Result<PathBuf, String> {
    let parent_metadata = std::fs::symlink_metadata(parent)
        .map_err(|error| format!("inspect private-directory parent: {error}"))?;
    if metadata_is_link_or_reparse(&parent_metadata) || !parent_metadata.is_dir() {
        return Err("private-directory parent is linked, reparsed, or not a directory".to_string());
    }
    for _ in 0..16 {
        let path = parent.join(format!("{prefix}-{}", uuid::Uuid::new_v4().simple()));
        match std::fs::create_dir(&path) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                        .map_err(|error| format!("protect private directory: {error}"))?;
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("create fresh private directory: {error}")),
        }
    }
    Err("could not reserve a fresh private installation directory".to_string())
}

pub fn remove_owned_directory_if_present(path: &Path, label: &str) -> Result<(), String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("inspect {label}: {error}")),
    };
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(format!("{label} is linked, reparsed, or not a directory"));
    }
    std::fs::remove_dir_all(path).map_err(|error| format!("remove {label}: {error}"))
}

pub fn exact_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Host checks kept separate from the portable transfer algorithm.
#[derive(Clone, Copy)]
pub struct TransferAdmission<C, A> {
    pub check_cancelled: C,
    pub admit_transport: A,
}

pub fn download_verified<F, C, A>(
    url: &str,
    destination: &Path,
    expected_sha256: &str,
    expected_size: u64,
    label: &str,
    report: F,
    admission: TransferAdmission<C, A>,
) -> Result<VerifiedDownload, String>
where
    F: FnMut(TransferProgress),
    C: Fn() -> Result<(), String> + Copy,
    A: Fn() -> Result<(), String> + Copy,
{
    download_pinned(
        url,
        destination,
        PinnedChecksum::Sha256(expected_sha256),
        expected_size,
        label,
        report,
        admission,
    )
}

/// Some immutable composite GCS objects publish CRC32C, not SHA-256. They use
/// the same resumable transfer contract; never fabricate a SHA from a partial.
pub fn download_crc32c_verified<F, C, A>(
    url: &str,
    destination: &Path,
    crc32c: u32,
    expected_size: u64,
    label: &str,
    report: F,
    admission: TransferAdmission<C, A>,
) -> Result<VerifiedDownload, String>
where
    F: FnMut(TransferProgress),
    C: Fn() -> Result<(), String> + Copy,
    A: Fn() -> Result<(), String> + Copy,
{
    download_pinned(
        url,
        destination,
        PinnedChecksum::Crc32c(crc32c),
        expected_size,
        label,
        report,
        admission,
    )
}

#[derive(Clone, Copy)]
enum PinnedChecksum<'a> {
    Sha256(&'a str),
    Crc32c(u32),
}

fn download_pinned<F, C, A>(
    url: &str,
    destination: &Path,
    checksum: PinnedChecksum<'_>,
    expected_size: u64,
    label: &str,
    mut report: F,
    admission: TransferAdmission<C, A>,
) -> Result<VerifiedDownload, String>
where
    F: FnMut(TransferProgress),
    C: Fn() -> Result<(), String> + Copy,
    A: Fn() -> Result<(), String> + Copy,
{
    let check_cancelled = admission.check_cancelled;
    if expected_size == 0
        || expected_size > MAX_DOWNLOAD_BYTES
        || matches!(checksum, PinnedChecksum::Sha256(value) if !exact_sha256(value))
    {
        return Err(format!("{label} has an invalid catalog size or checksum"));
    }
    if std::fs::symlink_metadata(destination).is_ok() {
        return Err(format!(
            "fresh private download destination already exists for {label}"
        ));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| format!("{label} download destination has no parent"))?;
    let metadata = std::fs::symlink_metadata(parent)
        .map_err(|error| format!("inspect {label} download root: {error}"))?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(format!(
            "{label} download root is not a private regular directory"
        ));
    }

    let partial = destination.with_extension("part");
    let _in_flight = InFlightDownload::claim(&partial, label)?;
    let mut attempt = 0u32;
    let download_started_at = Instant::now();
    loop {
        check_cancelled()?;
        attempt += 1;
        match download_attempt(
            url,
            &partial,
            expected_size,
            label,
            &mut report,
            admission,
            download_started_at,
        ) {
            Ok(()) => break,
            Err(error) if attempt < DOWNLOAD_ATTEMPTS => {
                report(TransferProgress {
                    status: "retrying",
                    progress: 0,
                    message: format!(
                        "{label} interrupted ({error}); retry {attempt}/{DOWNLOAD_ATTEMPTS}"
                    ),
                    bytes_downloaded: std::fs::metadata(&partial).map_or(0, |value| value.len()),
                    bytes_total: expected_size,
                });
                wait_before_retry(retry_delay(attempt), check_cancelled)?;
            }
            Err(error) => {
                return Err(format!(
                    "{label} download failed after {DOWNLOAD_ATTEMPTS} attempts: {error}"
                ));
            }
        }
    }

    report(TransferProgress {
        status: "verifying",
        progress: 96,
        message: format!("Verifying {label}"),
        bytes_downloaded: expected_size,
        bytes_total: expected_size,
    });
    let mut verified_partial = open_download_partial(&partial)?;
    let identity = verify_open_file_identity(
        &mut verified_partial,
        expected_size,
        checksum,
        check_cancelled,
    );
    drop(verified_partial);
    if identity.is_err() {
        let _ = std::fs::remove_file(&partial);
        return Err(format!(
            "{label} does not match its exact catalog size and checksum"
        ));
    }
    std::fs::rename(&partial, destination).map_err(|error| format!("activate {label}: {error}"))?;
    let mut activated = open_locked_regular_file(destination)?;
    if let Err(error) =
        verify_open_file_identity(&mut activated, expected_size, checksum, check_cancelled)
    {
        drop(activated);
        let _ = std::fs::remove_file(destination);
        return Err(format!(
            "{label} changed while its verified partial was activated: {error}"
        ));
    }
    report(TransferProgress {
        status: "complete",
        progress: 100,
        message: format!("Verified downloaded {label}"),
        bytes_downloaded: expected_size,
        bytes_total: expected_size,
    });
    Ok(VerifiedDownload {
        path: destination.to_path_buf(),
        _held: activated,
    })
}

/// Reopen a previously completed exact download after an application restart.
/// The caller supplies the same catalog identity; size and SHA are rechecked
/// before any byte is trusted.
pub fn reopen_verified_download<C>(
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
    check_cancelled: C,
) -> Result<VerifiedDownload, String>
where
    C: Fn() -> Result<(), String> + Copy,
{
    if expected_size == 0 || expected_size > MAX_DOWNLOAD_BYTES || !exact_sha256(expected_sha256) {
        return Err("saved partial has an invalid catalog identity".to_string());
    }
    let mut file = open_locked_regular_file(path)?;
    verify_open_file_identity(
        &mut file,
        expected_size,
        PinnedChecksum::Sha256(expected_sha256),
        check_cancelled,
    )?;
    Ok(VerifiedDownload {
        path: path.to_path_buf(),
        _held: file,
    })
}

fn download_attempt<F, C, A>(
    url: &str,
    partial: &Path,
    expected_size: u64,
    label: &str,
    report: &mut F,
    admission: TransferAdmission<C, A>,
    download_started_at: Instant,
) -> Result<(), String>
where
    F: FnMut(TransferProgress),
    C: Fn() -> Result<(), String> + Copy,
    A: Fn() -> Result<(), String> + Copy,
{
    let check_cancelled = admission.check_cancelled;
    let admit_transport = admission.admit_transport;
    check_cancelled()?;
    let mut file = open_download_partial(partial)?;
    let mut offset = file
        .metadata()
        .map_err(|error| format!("inspect {label} partial download: {error}"))?
        .len();
    if offset == expected_size {
        return Ok(());
    }
    if offset > expected_size {
        file.set_len(0)
            .map_err(|error| format!("reset {label}: {error}"))?;
        offset = 0;
    }
    let timeouts = transfer_timeouts_after(download_started_at.elapsed())
        .ok_or_else(|| format!("{label} exceeded its bounded download time"))?;
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeouts.overall))
        .timeout_per_call(Some(timeouts.overall))
        .timeout_connect(Some(timeouts.connect))
        .timeout_recv_response(Some(timeouts.idle))
        .timeout_recv_body(Some(timeouts.overall))
        .build()
        .new_agent();
    admit_transport()?;
    let mut request = agent.get(url);
    if offset > 0 {
        request = request.header("Range", &format!("bytes={offset}-"));
    }
    report(TransferProgress {
        status: if offset > 0 {
            "resuming"
        } else {
            "downloading"
        },
        progress: ((offset as f64 / expected_size as f64) * 95.0).floor() as u8,
        message: format!("Downloading {label}"),
        bytes_downloaded: offset,
        bytes_total: expected_size,
    });
    let response = request
        .call()
        .map_err(|error| format!("{label} transport failed: {error}"))?;
    let status = response.status().as_u16();
    if offset > 0 && status == 206 {
        file.seek(SeekFrom::End(0))
            .map_err(|error| format!("resume {label}: {error}"))?;
    } else {
        offset = 0;
        file.set_len(0)
            .and_then(|_| file.seek(SeekFrom::Start(0)).map(|_| ()))
            .map_err(|error| format!("restart {label}: {error}"))?;
    }
    let mut reader = response.into_body().into_reader();
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut received = offset;
    let mut last_progress = 0;
    loop {
        check_cancelled()?;
        admit_transport()?;
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("read {label}: {error}"))?;
        if count == 0 {
            break;
        }
        file.write_all(&buffer[..count])
            .map_err(|error| format!("write {label}: {error}"))?;
        received += count as u64;
        if received > expected_size || received > MAX_DOWNLOAD_BYTES {
            return Err(format!("{label} exceeds its declared size"));
        }
        let progress = ((received as f64 / expected_size as f64) * 95.0).floor() as u8;
        if progress > last_progress {
            last_progress = progress;
            report(TransferProgress {
                status: "downloading",
                progress,
                message: format!("Downloading {label}"),
                bytes_downloaded: received,
                bytes_total: expected_size,
            });
        }
    }
    file.flush()
        .map_err(|error| format!("flush {label}: {error}"))
}

pub fn open_download_partial(path: &Path) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(0x0020_0000);
    }
    let file = match options.create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let mut existing = std::fs::OpenOptions::new();
            existing.read(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                existing.custom_flags(libc::O_NOFOLLOW);
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                existing.share_mode(0x0000_0001).custom_flags(0x0020_0000);
            }
            existing
                .open(path)
                .map_err(|error| format!("open existing partial download safely: {error}"))?
        }
        Err(error) => return Err(format!("create partial download safely: {error}")),
    };
    let metadata = file
        .metadata()
        .map_err(|error| format!("inspect partial download: {error}"))?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err("partial download is linked, reparsed, or not a regular file".to_string());
    }
    Ok(file)
}

fn open_locked_regular_file(path: &Path) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("open locked regular file {}: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("inspect locked file {}: {error}", path.display()))?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(format!(
            "locked path is linked, reparsed, or not a regular file: {}",
            path.display()
        ));
    }
    Ok(file)
}

fn verify_open_file_identity<C>(
    file: &mut std::fs::File,
    expected_size: u64,
    checksum: PinnedChecksum<'_>,
    check_cancelled: C,
) -> Result<(), String>
where
    C: Fn() -> Result<(), String> + Copy,
{
    let metadata = file
        .metadata()
        .map_err(|error| format!("inspect held artifact: {error}"))?;
    if metadata.len() != expected_size {
        return Err("held artifact changed size before activation".to_string());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("seek held artifact: {error}"))?;
    let mut hasher = Sha256::new();
    let mut crc = 0;
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        check_cancelled()?;
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("read held artifact: {error}"))?;
        if count == 0 {
            break;
        }
        match checksum {
            PinnedChecksum::Sha256(_) => hasher.update(&buffer[..count]),
            PinnedChecksum::Crc32c(_) => crc = crc32c::crc32c_append(crc, &buffer[..count]),
        }
    }
    let matches = match checksum {
        PinnedChecksum::Sha256(expected) => format!("{:x}", hasher.finalize()) == expected,
        PinnedChecksum::Crc32c(expected) => crc == expected,
    };
    if !matches {
        return Err("held artifact failed its pinned checksum before activation".to_string());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("rewind held artifact: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_download_of_the_same_destination_is_told_it_is_in_flight() {
        let partial = std::env::temp_dir().join(format!(
            "ds-in-flight-{}.part",
            uuid::Uuid::new_v4().simple()
        ));
        let first = InFlightDownload::claim(&partial, "Wetlands").expect("first claim");
        let refused = InFlightDownload::claim(&partial, "Wetlands")
            .err()
            .expect("a concurrent claim of the same partial file is refused");
        assert_eq!(
            refused,
            "Wetlands is already downloading in this Desktop; wait for that download to finish, then retry"
        );
        let other = partial.with_extension("other.part");
        drop(InFlightDownload::claim(&other, "Powerlines").expect("another dataset"));
        drop(first);
        drop(InFlightDownload::claim(&partial, "Wetlands").expect("claim after release"));
    }

    #[test]
    fn composite_object_checksum_is_verified_before_activation() {
        let mut file = tempfile::tempfile().unwrap();
        let bytes = b"complete immutable terrain";
        file.write_all(bytes).unwrap();
        let crc = crc32c::crc32c(bytes);
        assert!(verify_open_file_identity(
            &mut file,
            bytes.len() as u64,
            PinnedChecksum::Crc32c(crc),
            || Ok(())
        )
        .is_ok());
        assert!(verify_open_file_identity(
            &mut file,
            bytes.len() as u64,
            PinnedChecksum::Crc32c(crc ^ 1),
            || Ok(())
        )
        .is_err());
        assert!(verify_open_file_identity(
            &mut file,
            bytes.len() as u64 + 1,
            PinnedChecksum::Crc32c(crc),
            || Ok(())
        )
        .is_err());
    }

    #[test]
    fn download_timeout_policy_is_idle_bounded_and_shared_across_retries() {
        let initial = transfer_timeouts_after(Duration::ZERO).expect("initial budget");
        assert_eq!(initial.connect, Duration::from_secs(60));
        assert_eq!(initial.idle, Duration::from_secs(5 * 60));
        assert_eq!(initial.overall, Duration::from_secs(8 * 60 * 60));

        let final_window =
            transfer_timeouts_after(DOWNLOAD_OVERALL_TIMEOUT - Duration::from_secs(10))
                .expect("final bounded window");
        assert_eq!(final_window.connect, Duration::from_secs(10));
        assert_eq!(final_window.idle, Duration::from_secs(10));
        assert_eq!(final_window.overall, Duration::from_secs(10));
        assert!(transfer_timeouts_after(DOWNLOAD_OVERALL_TIMEOUT).is_none());
        assert!(
            transfer_timeouts_after(DOWNLOAD_OVERALL_TIMEOUT + Duration::from_secs(1)).is_none()
        );
    }

    #[test]
    fn failed_attempts_back_off_within_a_bounded_window() {
        assert_eq!(retry_delay(1), Duration::from_secs(2));
        assert_eq!(retry_delay(2), Duration::from_secs(4));
        assert_eq!(retry_delay(4), Duration::from_secs(16));
        assert_eq!(retry_delay(5), DOWNLOAD_RETRY_CAP);
        assert_eq!(retry_delay(u32::MAX), DOWNLOAD_RETRY_CAP);
        // Every pause between the twelve attempts, together: minutes, not
        // seconds (feedback 33003988), and far inside the transfer budget.
        let total: Duration = (1..DOWNLOAD_ATTEMPTS).map(retry_delay).sum();
        assert!(total >= Duration::from_secs(120), "{total:?}");
        assert!(total <= Duration::from_secs(5 * 60), "{total:?}");
        // A cancellation ends the wait at once instead of sleeping it out.
        let started = Instant::now();
        let cancelled = wait_before_retry(Duration::from_secs(30), || Err("cancelled".into()));
        assert_eq!(cancelled, Err("cancelled".to_string()));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_completed_exact_download_can_be_reopened_after_restart() {
        let root = std::env::temp_dir().join(format!("ds-download-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).expect("test root");
        let path = root.join("dataset.geojsonl.gz");
        let bytes = b"exact resumable bytes";
        std::fs::write(&path, bytes).expect("write exact download");
        let sha256 = format!("{:x}", Sha256::digest(bytes));

        let reopened = reopen_verified_download(&path, bytes.len() as u64, &sha256, || Ok(()))
            .expect("reopen exact download");
        assert_eq!(reopened.path(), path);
        drop(reopened);

        assert!(
            reopen_verified_download(&path, bytes.len() as u64, &"0".repeat(64), || Ok(()))
                .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn planted_partial_download_symlink_cannot_be_opened_for_write() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("ds-download-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).expect("test root");
        let outside = root.join("outside.bin");
        std::fs::write(&outside, b"unchanged").expect("outside");
        let partial = root.join("artifact.part");
        symlink(&outside, &partial).expect("partial symlink");
        assert!(open_download_partial(&partial).is_err());
        assert_eq!(std::fs::read(&outside).expect("outside read"), b"unchanged");
        let _ = std::fs::remove_dir_all(root);
    }

    /// Every component an owned directory adds below its base is private
    /// (owner rule): created 0700, and one from before the rule tightened.
    /// The base — possibly the platform's shared local-data directory — is
    /// left exactly as it was.
    #[cfg(unix)]
    #[test]
    fn owned_directories_are_private_below_an_untouched_base() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let base = tempfile::tempdir().unwrap();
        std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let old = base.path().join("rw.datasolutions.desktop.shared");
        std::fs::create_dir(&old).unwrap();
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o775)).unwrap();
        let owned = ensure_owned_directory(
            base.path(),
            Path::new("rw.datasolutions.desktop.shared/foundation/document-runtime"),
        )
        .unwrap();
        assert_eq!(mode(base.path()), 0o755, "the base is not DS-owned");
        assert_eq!(mode(&old), 0o700);
        assert_eq!(mode(owned.parent().unwrap()), 0o700);
        assert_eq!(mode(&owned), 0o700);
    }
}
