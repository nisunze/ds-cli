//! Windows scheduling/IO adapter for the same Server router as Linux.
//! Named pipes are local-only and ACL-restricted to the full current-user SID.
//! Both ends ask Windows for the connected process's token before HTTP is sent.

use super::{AccountId, CallError, Peer, Reply};
use hyper_util::rt::TokioIo;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer};
use tower::ServiceExt;
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetLengthSid,
    GetTokenInformation, OWNER_SECURITY_INFORMATION, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_OVERLAPPED,
    FILE_SHARE_READ, FILE_SHARE_WRITE, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn last() -> String {
    io::Error::last_os_error().to_string()
}

/// Copy the entire SID while the aligned TOKEN_USER buffer is held.
fn process_account(process: HANDLE) -> Result<AccountId, String> {
    let mut token = null_mut();
    // SAFETY: valid borrowed process handle and initialized output pointer.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(last());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut size = 0;
    unsafe { GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut size) };
    if !(std::mem::size_of::<TOKEN_USER>() as u32..=65536).contains(&size) {
        return Err("invalid Windows token size".into());
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    } == 0
    {
        return Err(last());
    }
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    if user.User.Sid.is_null() {
        return Err("missing Windows account SID".into());
    }
    let length = unsafe { GetLengthSid(user.User.Sid) } as usize;
    if length == 0 || length > 68 {
        return Err("invalid Windows account SID".into());
    }
    let mut sid = [0; 68];
    sid[..length]
        .copy_from_slice(unsafe { std::slice::from_raw_parts(user.User.Sid.cast::<u8>(), length) });
    Ok(AccountId::Windows(sid))
}
pub(super) fn current_account() -> Result<AccountId, String> {
    process_account(unsafe { GetCurrentProcess() })
}

fn peer_account(handle: HANDLE, server: bool) -> Result<AccountId, String> {
    let mut pid = 0;
    let result = unsafe {
        if server {
            GetNamedPipeServerProcessId(handle, &mut pid)
        } else {
            GetNamedPipeClientProcessId(handle, &mut pid)
        }
    };
    if result == 0 {
        return Err(last());
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(last());
    }
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    process_account(process.as_raw_handle())
}

struct Descriptor(*mut std::ffi::c_void);
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}
impl Descriptor {
    fn owner_only(inherit: bool) -> Result<Self, String> {
        let AccountId::Windows(mut sid) = current_account()? else {
            return Err("no Windows account".into());
        };
        let mut text = null_mut();
        if unsafe { ConvertSidToStringSidW(sid.as_mut_ptr().cast(), &mut text) } == 0 {
            return Err(last());
        }
        let mut len = 0;
        while unsafe { *text.add(len) } != 0 {
            len += 1;
        }
        let owner = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
        unsafe { LocalFree(text.cast()) };
        let sddl = format!(
            "O:{owner}D:P(A;{};FA;;;{owner})",
            if inherit { "OICI" } else { "" }
        );
        let mut descriptor = null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(std::ffi::OsStr::new(&sddl)).as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(last());
        }
        Ok(Self(descriptor))
    }
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
}

fn validate_handle(handle: HANDLE) -> Result<(), String> {
    let AccountId::Windows(mut own) = current_account()? else {
        return Err("no Windows account".into());
    };
    let own_sid: PSID = own.as_mut_ptr().cast();
    let mut owner = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut sd = null_mut();
    let result = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut sd,
        )
    };
    let _descriptor = Descriptor(sd);
    if result != 0 || owner.is_null() || dacl.is_null() || unsafe { EqualSid(owner, own_sid) } == 0
    {
        return Err(
            "server state must belong to this Windows account and have a private DACL".into(),
        );
    }
    let mut owner_allowed = false;
    for index in 0..unsafe { (*dacl).AceCount } as u32 {
        let mut ace = null_mut();
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
            return Err(last());
        }
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        // Accept only explicit/inherited owner allow entries. Unknown ACE forms
        // cannot establish a private root, and are refused rather than ignored.
        if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE {
            return Err("server state DACL has an unsupported entry".into());
        }
        let allow = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        let sid = std::ptr::addr_of!(allow.SidStart).cast_mut().cast();
        if unsafe { EqualSid(sid, own_sid) } == 0 {
            return Err("server state grants access to another Windows account".into());
        }
        owner_allowed = true;
    }
    if !owner_allowed {
        return Err("server state DACL does not grant its owner access".into());
    }
    Ok(())
}

fn local_path(path: &Path) -> Result<(), String> {
    use std::path::{Component, Prefix};
    if !path.is_absolute()
        || !matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
    {
        return Err("server state requires an absolute local Windows disk path".into());
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("server state path must not contain traversal".into());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(m) if m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
                return Err("server state must not traverse a Windows reparse point".into());
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
pub(crate) fn protected(path: &Path, directory: bool) -> Result<(), String> {
    local_path(path)?;
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if meta.is_dir() != directory || (!directory && !meta.is_file()) {
        return Err("server state is not a regular owned path".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|e| e.to_string())?;
    validate_handle(file.as_raw_handle())
}
pub(crate) fn prepare_directory(path: &Path) -> Result<(), String> {
    local_path(path)?;
    if !path.exists() {
        let parent = path
            .parent()
            .ok_or("server state needs a parent directory")?;
        // Create only missing namespaces, never alter an existing user's ACL.
        if !parent.exists() {
            prepare_directory(parent)?;
        }
        let descriptor = Descriptor::owner_only(true)?;
        let attributes = descriptor.attributes();
        if unsafe { CreateDirectoryW(wide(path.as_os_str()).as_ptr(), &attributes) } == 0
            && !path.exists()
        {
            return Err(last());
        }
    }
    protected(path, true)
}

pub(super) fn pipe_path(state: &Path) -> PathBuf {
    // The address is derived, never trusted from connection.json. Canonical
    // spelling permits drive/path aliases to share the same single-host lock.
    let canonical = fs::canonicalize(state).unwrap_or_else(|_| state.to_path_buf());
    let mut digest = Sha256::new();
    for word in canonical.to_string_lossy().to_lowercase().encode_utf16() {
        digest.update(word.to_le_bytes());
    }
    if let Ok(AccountId::Windows(sid)) = current_account() {
        digest.update(sid);
    }
    PathBuf::from(format!(r"\\.\pipe\ds-server-{:x}", digest.finalize()))
}

fn create_pipe(path: &Path, first: bool) -> Result<OwnedHandle, String> {
    let descriptor = Descriptor::owner_only(false)?;
    let attributes = descriptor.attributes();
    let flags = PIPE_ACCESS_DUPLEX
        | FILE_FLAG_OVERLAPPED
        | if first {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            0
        };
    let handle = unsafe {
        CreateNamedPipeW(
            wide(path.as_os_str()).as_ptr(),
            flags,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            255,
            65536,
            65536,
            0,
            &attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(last());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}
pub struct Listening {
    handle: OwnedHandle,
    pipe: PathBuf,
    _lock: File,
}
impl Listening {
    pub fn socket(&self) -> &Path {
        &self.pipe
    }
}
pub fn listen(state: &Path) -> Result<Listening, String> {
    protected(state, true)?;
    let lock_path = state.join(super::LOCK);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&lock_path)
        .map_err(|e| e.to_string())?;
    protected(&lock_path, false)?;
    lock.try_lock().map_err(|e| {
        format!(
            "a Server is already running on this state directory, or its lock is unavailable: {e}"
        )
    })?;
    let pipe = pipe_path(state);
    let handle = create_pipe(&pipe, true)?;
    Ok(Listening {
        handle,
        pipe,
        _lock: lock,
    })
}

pub async fn serve(
    listening: Listening,
    router: axum::Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), String> {
    use std::os::windows::io::IntoRawHandle;
    let Listening {
        handle,
        pipe,
        _lock,
    } = listening;
    let mut waiting = unsafe { NamedPipeServer::from_raw_handle(handle.into_raw_handle()) }
        .map_err(|e| e.to_string())?;
    let (stop, stopping) = tokio::sync::watch::channel(());
    let stop = Arc::new(stop);
    tokio::spawn(async move {
        shutdown.await;
        drop(stopping);
    });
    let (done, finished) = tokio::sync::watch::channel(());
    loop {
        tokio::select! { result = waiting.connect() => { result.map_err(|e| e.to_string())?; }, _ = stop.closed() => break }
        // Install the next instance BEFORE moving this connected instance to
        // its worker, so an idle caller always has a listener to connect to.
        let next = unsafe {
            NamedPipeServer::from_raw_handle(create_pipe(&pipe, false)?.into_raw_handle())
        }
        .map_err(|e| e.to_string())?;
        let stream = std::mem::replace(&mut waiting, next);
        let Ok(account) = peer_account(stream.as_raw_handle(), false) else {
            continue;
        };
        if Some(account) != current_account().ok() {
            continue;
        }
        let peer = Peer { account };
        let router = router.clone();
        let service = hyper::service::service_fn(
            move |mut request: hyper::Request<hyper::body::Incoming>| {
                request.extensions_mut().insert(peer);
                router.clone().oneshot(request)
            },
        );
        let stop = stop.clone();
        let finished = finished.clone();
        tokio::spawn(async move {
            let connection = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service);
            tokio::pin!(connection);
            let stopping = stop.closed();
            tokio::pin!(stopping);
            let mut winding_down = false;
            loop {
                tokio::select! { _ = connection.as_mut() => break, _ = &mut stopping, if !winding_down => { winding_down = true; connection.as_mut().graceful_shutdown(); } }
            }
            drop(finished);
        });
    }
    drop(waiting);
    drop(finished);
    done.closed().await;
    drop(_lock);
    Ok(())
}

pub(super) fn call(
    pipe: &Path,
    request: hyper::Request<http_body_util::Full<hyper::body::Bytes>>,
    limit: u64,
    timeout: Duration,
) -> Result<Reply, CallError> {
    let pipe = pipe.to_path_buf();
    let run = move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| CallError::Failed(e.to_string()))?;
        runtime.block_on(async move {
            let start = Instant::now();
            let stream = loop {
                match ClientOptions::new().open(&pipe) {
                    Ok(stream) => break stream,
                    Err(e) if e.raw_os_error() == Some(231) && start.elapsed() < timeout => tokio::time::sleep(Duration::from_millis(10)).await,
                    Err(e) => return Err(CallError::Unreachable(format!("no accessible Server named pipe; start ds server serve: {e}"))),
                }
            };
            let own = current_account().map_err(CallError::NotOwner)?;
            if peer_account(stream.as_raw_handle(), true).map_err(CallError::NotOwner)? != own { return Err(CallError::NotOwner("the process answering on this Server pipe runs as another account; nothing was sent".into())); }
            validate_handle(stream.as_raw_handle()).map_err(CallError::NotOwner)?;
            tokio::time::timeout(timeout.saturating_sub(start.elapsed()), async move {
                use http_body_util::BodyExt;
                let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await.map_err(|e| CallError::Failed(e.to_string()))?;
                tokio::spawn(connection);
                let response = sender.send_request(request).await.map_err(|e| CallError::Failed(e.to_string()))?;
                let status = response.status().as_u16();
                let body = http_body_util::Limited::new(response.into_body(), usize::try_from(limit).unwrap_or(usize::MAX)).collect().await.map_err(|e| if e.is::<http_body_util::LengthLimitError>() { CallError::TooLarge } else { CallError::Failed(e.to_string()) })?.to_bytes();
                Ok(Reply { status, body: body.to_vec() })
            }).await.map_err(|_| CallError::Failed(format!("the Server did not answer within {} s", timeout.as_secs())))?
        })
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::scope(|s| {
            s.spawn(run)
                .join()
                .unwrap_or_else(|_| Err(CallError::Failed("the exchange panicked".into())))
        })
    } else {
        run()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("Windows Server state with spaces");
        prepare_directory(&state).unwrap();
        (temp, state)
    }

    #[test]
    fn private_state_and_children_are_checked_without_rewriting_user_acls() {
        let (temp, state) = state();
        protected(&state, true).unwrap();
        fs::write(state.join("owner.txt"), b"owner").unwrap();
        protected(&state.join("owner.txt"), false).unwrap();
        let ordinary = temp.path().join("ordinary");
        fs::create_dir(&ordinary).unwrap();
        assert!(
            prepare_directory(&ordinary).is_err(),
            "existing inherited broad ACL is not silently changed"
        );
        assert!(prepare_directory(Path::new(r"\\server\share\state")).is_err());
        assert!(prepare_directory(Path::new("relative")).is_err());
    }

    #[test]
    fn one_host_per_state_and_restart_reclaims_the_same_address() {
        let (_temp, state) = state();
        let first = listen(&state).unwrap();
        assert!(listen(&state).err().unwrap().contains("already running"));
        let address = first.socket().to_owned();
        assert_eq!(pipe_path(&fs::canonicalize(&state).unwrap()), address);
        drop(first);
        let again = listen(&state).unwrap();
        assert_eq!(again.socket(), address);
        drop(again);
        // An existing pipe is never replaced, even if no DS lock was held.
        let squatter = create_pipe(&address, true).unwrap();
        assert!(listen(&state).is_err());
        drop(squatter);
        assert!(listen(&state).is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_pipe_serves_parallel_bounded_requests_and_stops_cleanly() {
        let (_temp, state) = state();
        let listening = listen(&state).unwrap();
        let pipe = listening.socket().to_owned();
        let router = axum::Router::new().route(
            "/v1/proof",
            axum::routing::get(|axum::Extension(peer): axum::Extension<Peer>| async move {
                super::super::admit(Some(peer), current_account().ok()).unwrap();
                "native Windows owner"
            }),
        );
        let (stop, stopping) = tokio::sync::oneshot::channel();
        let host = tokio::spawn(serve(listening, router, async {
            let _ = stopping.await;
        }));
        let request = |limit| {
            super::super::call(
                &pipe,
                "GET",
                "/v1/proof",
                &[],
                None,
                limit,
                Duration::from_secs(5),
            )
        };
        let first = request(1024).unwrap();
        assert_eq!(first.status, 200);
        assert_eq!(first.body, b"native Windows owner");
        let results: Vec<_> = std::thread::scope(|scope| {
            (0..8)
                .map(|_| scope.spawn(|| request(1024)))
                .collect::<Vec<_>>()
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .collect()
        });
        assert!(
            results
                .iter()
                .all(|r| r.as_ref().is_ok_and(|reply| reply.status == 200))
        );
        assert!(matches!(request(4), Err(CallError::TooLarge)));
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), host)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(request(1024), Err(CallError::Unreachable(_))));
        assert!(listen(&state).is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn request_deadline_is_enforced() {
        let (_temp, state) = state();
        let listening = listen(&state).unwrap();
        let pipe = listening.socket().to_owned();
        let router = axum::Router::new().route(
            "/v1/slow",
            axum::routing::get(|| async {
                tokio::time::sleep(Duration::from_millis(250)).await;
                "finished"
            }),
        );
        let (stop, stopping) = tokio::sync::oneshot::channel();
        let host = tokio::spawn(serve(listening, router, async {
            let _ = stopping.await;
        }));
        let result = super::super::call(
            &pipe,
            "GET",
            "/v1/slow",
            &[],
            None,
            1024,
            Duration::from_millis(30),
        );
        assert!(matches!(result, Err(CallError::Failed(_))));
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), host)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
