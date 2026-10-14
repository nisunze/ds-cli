//! Private local pipe adapter. No endpoint, token or arbitrary transport is a
//! public client contract: typed CLI/MCP calls remain the supported interface.
use super::{Account, CallError, Peer, Reply};
use ds_cli_auth::windows_host as protected;
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};
use tower::ServiceExt;
use windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION;
use windows_sys::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId};

pub(super) fn socket_path(state: &Path) -> PathBuf {
    // Callers validate the existing state first. Canonical spelling gives case
    // aliases one name; no caller-provided pipe address is ever admitted.
    let canonical = std::fs::canonicalize(state).unwrap_or_else(|_| state.to_path_buf());
    // Hash lossless UTF-16, including names with unpaired surrogates. A lossy
    // display string could route two distinct protected directories together.
    let path: Vec<u8> = canonical
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    PathBuf::from(format!(r"\\.\pipe\ds-core-{:x}", Sha256::digest(&path)))
}
fn instance(path: &Path, first: bool) -> Result<NamedPipeServer, String> {
    protected::with_owner_security(|attributes| {
        // SAFETY: the protected-state owner keeps this descriptor alive for
        // the synchronous creation call. Handles are non-inheritable.
        unsafe {
            ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .create_with_security_attributes_raw(path, attributes)
        }
        .map_err(|e| e.to_string())
    })
}
pub struct Listening {
    first: Option<NamedPipeServer>,
    socket: PathBuf,
    _lock: protected::RuntimeLock,
}
impl Listening {
    pub fn socket(&self) -> &Path {
        &self.socket
    }
}
/// Must run inside the serving runtime's IO context; the single-owner lock is
/// acquired before any pipe instance or retained connection record is touched.
pub fn listen(state: &Path) -> Result<Listening, String> {
    protected::validate_directory(state)?;
    let lock = protected::RuntimeLock::acquire(&state.join(super::LOCK))?;
    protected::audit_runtime_tree(state, &lock)?;
    let socket = socket_path(state);
    let first = instance(&socket, true)?;
    Ok(Listening {
        first: Some(first),
        socket,
        _lock: lock,
    })
}
fn client_account(pipe: &NamedPipeServer) -> Result<Account, String> {
    let mut pid = 0;
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle().cast(), &mut pid) } == 0
        || pid == 0
    {
        return Err("cannot verify the private pipe peer".into());
    }
    protected::process_account(pid).map(Account::Windows)
}
pub async fn serve(
    mut listening: Listening,
    router: axum::Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), String> {
    let mut next = listening
        .first
        .take()
        .ok_or_else(|| "this pipe is already being served".to_string())?;
    let own = protected::current_account().map(Account::Windows)?;
    let (stop, stopping) = tokio::sync::watch::channel(());
    let stop = Arc::new(stop);
    let shutdown_task = tokio::spawn(async move {
        shutdown.await;
        drop(stopping);
    });
    let (done, finished) = tokio::sync::watch::channel(());
    let outcome = loop {
        tokio::select! {
            connected=next.connect() => { if let Err(e)=connected { break Err(e.to_string()); } },
            _=stop.closed() => break Ok(()),
        }
        // Keep one instance alive while creating the replacement, preventing
        // another first-instance owner from taking the name between clients.
        let replacement = match instance(&listening.socket, false) {
            Ok(pipe) => pipe,
            Err(e) => break Err(e),
        };
        let stream = std::mem::replace(&mut next, replacement);
        let Ok(account) = client_account(&stream) else {
            continue;
        };
        if account != own {
            continue;
        }
        let peer = Peer { account };
        let router = router.clone();
        let service = hyper::service::service_fn(move |mut request: hyper::Request<Incoming>| {
            request.extensions_mut().insert(peer);
            router.clone().oneshot(request)
        });
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
                tokio::select! {
                    _=connection.as_mut()=>break,
                    _=&mut stopping, if !winding_down => { winding_down=true; connection.as_mut().graceful_shutdown(); },
                }
            }
            drop(finished);
        });
    };
    shutdown_task.abort();
    let _ = shutdown_task.await;
    drop(next);
    drop(finished);
    done.closed().await;
    drop(listening);
    outcome
}

pub(super) fn call(
    socket: &Path,
    request: hyper::Request<http_body_util::Full<hyper::body::Bytes>>,
    limit: u64,
    timeout: Duration,
) -> Result<Reply, CallError> {
    let own = protected::current_account().map_err(CallError::NotOwner)?;
    let run = move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| CallError::Failed(e.to_string()))?;
        runtime.block_on(async move {
            tokio::time::timeout(timeout, async move {
                let stream = loop {
                    // Tokio defaults to identification-only QoS: the server
                    // cannot impersonate this client to use its credentials.
                    match ClientOptions::new()
                        .security_qos_flags(SECURITY_IDENTIFICATION)
                        .open(socket)
                    {
                        Ok(pipe) => break pipe,
                        Err(e) if e.raw_os_error() == Some(231) => {
                            tokio::time::sleep(Duration::from_millis(10)).await
                        }
                        Err(e) => return Err(CallError::Unreachable(e.to_string())),
                    }
                };
                let mut pid = 0;
                if unsafe { GetNamedPipeServerProcessId(stream.as_raw_handle().cast(), &mut pid) }
                    == 0
                    || pid == 0
                    || protected::process_account(pid).map_err(CallError::NotOwner)? != own
                {
                    return Err(CallError::NotOwner(
                        "the answering process is not this account's Core; nothing was sent".into(),
                    ));
                }
                super::exchange(stream, request, limit).await
            })
            .await
            .map_err(|_| CallError::Failed("the private Core exchange timed out".into()))?
        })
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::scope(|scope| {
            scope
                .spawn(run)
                .join()
                .unwrap_or_else(|_| Err(CallError::Failed("the exchange panicked".into())))
        })
    } else {
        run()
    }
}

#[cfg(test)]
fn private_fixture_parent() -> tempfile::TempDir {
    // This managed host deliberately grants sandbox users write access to
    // TEMP. Positive ownership tests must use a safe user-owned parent;
    // the production fence correctly refuses the shared TEMP directory.
    tempfile::Builder::new()
        .prefix("ds-core-qualification-")
        .tempdir_in(std::env::var_os("LOCALAPPDATA").expect("Windows LocalAppData"))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Extension, routing::get};
    use tokio::sync::oneshot;

    fn state() -> (tempfile::TempDir, PathBuf) {
        let temp = super::private_fixture_parent();
        let state = temp.path().join("core");
        protected::prepare_directory(&state).unwrap();
        (temp, state)
    }
    #[test]
    fn full_sid_matches_the_current_process_and_mixed_identities_refuse() {
        let own = protected::current_account().unwrap();
        assert_eq!(protected::process_account(std::process::id()).unwrap(), own);
        let peer = Peer {
            account: Account::Windows(own),
        };
        assert!(super::super::admit(Some(peer), Some(Account::Windows(own))).is_ok());
        let mut foreign = own;
        foreign[8] ^= 1;
        assert!(super::super::admit(Some(peer), Some(Account::Windows(foreign))).is_err());
        assert!(super::super::admit(Some(peer), Some(Account::Unix(0))).is_err());
        assert!(super::super::admit(None, Some(Account::Windows(own))).is_err());
    }
    #[tokio::test]
    async fn one_owner_per_state_and_first_pipe_instance_fences_name_squatting() {
        let (_temp, state) = state();
        let first = listen(&state).unwrap();
        assert!(listen(&state).is_err());
        assert!(instance(first.socket(), true).is_err());
        drop(first);
        let squatter = instance(&socket_path(&state), true).unwrap();
        assert!(
            listen(&state).is_err(),
            "an unowned occupied pipe must never be reused"
        );
        drop(squatter);
        assert!(listen(&state).is_ok(), "closed owner leaves no stale pipe");
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_private_pipe_verifies_peers_bounds_replies_and_restarts() {
        let (_temp, state) = state();
        let own = super::super::own_account().unwrap();
        for _ in 0..2 {
            let listening = listen(&state).unwrap();
            let socket = listening.socket().to_owned();
            let router = axum::Router::new().route(
                "/probe",
                get(move |Extension(peer): Extension<Peer>| async move {
                    super::super::admit(Some(peer), Some(own)).unwrap();
                    "verified-native-peer"
                }),
            );
            let (stop, stopped) = oneshot::channel();
            let server = tokio::spawn(serve(listening, router, async {
                let _ = stopped.await;
            }));
            let reply = super::super::call(
                &socket,
                "GET",
                "/probe",
                &[],
                None,
                128,
                Duration::from_secs(5),
            )
            .unwrap();
            assert_eq!(reply.status, 200);
            assert_eq!(reply.body, b"verified-native-peer");
            let large = super::super::call(
                &socket,
                "GET",
                "/probe",
                &[],
                None,
                1,
                Duration::from_secs(5),
            )
            .unwrap_err();
            assert!(matches!(large, CallError::TooLarge));
            stop.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(5), server)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let missing = super::super::call(
                &socket,
                "GET",
                "/probe",
                &[],
                None,
                128,
                Duration::from_millis(100),
            )
            .unwrap_err();
            assert!(matches!(missing, CallError::Unreachable(_)));
        }
    }
    #[tokio::test]
    async fn busy_pipe_timeout_is_bounded_without_sending_request_bytes() {
        let (_temp, state) = state();
        let path = socket_path(&state);
        let occupied = protected::with_owner_security(|attributes| unsafe {
            ServerOptions::new()
                .first_pipe_instance(true)
                .max_instances(1)
                .create_with_security_attributes_raw(&path, attributes)
                .map_err(|e| e.to_string())
        })
        .unwrap();
        // Reserve the sole instance without accepting or reading a request.
        let client = ClientOptions::new().open(&path).unwrap();
        let started = std::time::Instant::now();
        let refused = super::super::call(
            &path,
            "GET",
            "/never-sent",
            &[],
            None,
            128,
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(matches!(refused, CallError::Failed(_)));
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(client);
        drop(occupied);
    }
}

#[cfg(test)]
mod protected_state_tests {
    use ds_cli_auth::windows_host::*;
    use std::path::Path;
    #[test]
    fn runtime_state_is_sid_private_and_file_replacement_uses_the_existing_owner() {
        use std::io::{Read, Write};
        let temp = super::private_fixture_parent();
        let root = temp.path().join("core");
        prepare_directory(&root).unwrap();
        validate_directory(&root).unwrap();
        let staging = root.join("connection.json.new");
        let destination = root.join("connection.json");
        for content in [b"first".as_slice(), b"second".as_slice()] {
            let mut file = create_file(&staging).unwrap();
            file.write_all(content).unwrap();
            file.sync_all().unwrap();
            drop(file);
            replace_file(&staging, &destination).unwrap();
            let mut actual = Vec::new();
            read_file(&destination)
                .unwrap()
                .read_to_end(&mut actual)
                .unwrap();
            assert_eq!(actual, content);
        }
        let held = open_lock(&root.join("server.lock")).unwrap();
        held.try_lock().unwrap();
        assert!(
            open_lock(&root.join("server.lock"))
                .unwrap()
                .try_lock()
                .is_err()
        );
    }
    fn broad_directory(broad: &Path) {
        use std::os::windows::ffi::OsStrExt;
        use std::ptr::null_mut;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
        use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
        use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;
        let wide: Vec<u16> = broad.as_os_str().encode_wide().chain(Some(0)).collect();
        let sddl: Vec<u16> = "D:P(A;OICI;GA;;;WD)"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    null_mut(),
                )
            },
            0
        );
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let created = unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) };
        unsafe { LocalFree(descriptor) };
        assert_ne!(created, 0);
    }
    #[test]
    fn a_directory_with_foreign_write_grants_is_refused_without_acl_repair() {
        let temp = super::private_fixture_parent();
        let broad = temp.path().join("deliberately-unsafe");
        broad_directory(&broad);
        assert!(validate_directory(&broad).is_err());
        assert!(prepare_directory(&broad).is_err());
        assert!(broad.is_dir(), "unsafe retained state is never removed");
        assert!(prepare_directory(Path::new("relative")).is_err());
    }
    #[tokio::test]
    async fn retained_unsafe_child_refuses_before_a_pipe_or_queue_opens() {
        let temp = super::private_fixture_parent();
        let root = temp.path().join("private-core");
        prepare_directory(&root).unwrap();
        let broad = temp.path().join("deliberately-unsafe");
        broad_directory(&broad);
        let old = broad.join("retained");
        let retained = root.join("store.sqlite");
        std::fs::write(&old, b"retained-pending-fixture").unwrap();
        std::fs::rename(&old, &retained).unwrap();
        assert!(
            read_file(&retained).is_err(),
            "fixture keeps its broader ACL on the same-volume move"
        );
        assert!(super::listen(&root).is_err());
        assert_eq!(
            std::fs::read(&retained).unwrap(),
            b"retained-pending-fixture"
        );
        let absent = super::super::call(
            &super::socket_path(&root),
            "GET",
            "/never-opened",
            &[],
            None,
            128,
            std::time::Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(matches!(absent, super::CallError::Unreachable(_)));
        std::fs::remove_file(&retained).unwrap();
        assert!(
            super::listen(&root).is_ok(),
            "a refused startup releases its owner lock"
        );
    }
}
