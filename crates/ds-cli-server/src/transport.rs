//! The Server's one door: an owner-only Unix domain socket inside its
//! protected state directory, and the kernel's word on who is at the other
//! end of it.
//!
//! The Server used to listen on a fixed TCP loopback port and admit whoever
//! presented a bearer kept, unchanged across restarts, in `connection.json`.
//! Anything that could bind that port while the Server was down collected
//! the owner's bearer from the next `ds` call. So there is no port and no
//! bearer any more:
//!
//! * the Server binds `<state>/server.sock` — the state directory is 0700 and
//!   the socket itself 0600 — and asks the kernel for the uid of the process
//!   behind every connection it accepts (`SO_PEERCRED`, `getpeereid`); a
//!   request is served only when that uid is the Server's own ([`admit`]);
//! * the client connects only to a socket it owns, in a state directory it
//!   owns, and asks the kernel the same question about the process answering
//!   before it sends a byte ([`call`]).
//!
//! No secret crosses this door in either direction, so there is nothing for
//! an impostor to collect. The HTTP spoken over it is unchanged, so no route
//! changed with it.
//!
//! One `ds server serve` per state directory is held by an advisory lock
//! beside the socket (`server.lock`), taken before the socket is touched: a
//! socket file left by a host that died is removed only by the host that
//! holds the lock, and only when nothing answers on it.
//!
//! Platforms without Unix sockets have no door at all. `ds server serve` was
//! never available there and the protected state it needs is Unix-only, so
//! nothing listens and nothing is sent: both halves refuse by name.

use ds_cli_contract::Failure;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The socket's name inside the protected state directory.
pub const SOCKET: &str = "server.sock";
/// The single-host lock's name inside the protected state directory.
pub const LOCK: &str = "server.lock";

/// Where the Server over `state` answers.
pub fn socket_path(state: &Path) -> PathBuf {
    state.join(SOCKET)
}

/// Who the kernel says is at the other end of one accepted connection.
///
/// It travels as a request extension from the accept loop to the router's
/// one authorization step; a request that arrives without it did not come
/// through this door and is refused like a stranger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    pub uid: u32,
}

/// The account this process runs as, or `None` where there is no such
/// question to ask of a socket.
pub fn own_uid() -> Option<u32> {
    #[cfg(unix)]
    {
        // SAFETY: geteuid reads the calling process identity and has no pointers.
        Some(unsafe { libc::geteuid() })
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// The whole rule of the door: the peer the kernel named is this process's
/// own account. Nothing else is asked — there is no header, no bearer and no
/// second identity a request can name.
///
/// The refusal names nobody: not the peer, not the owner, not a project.
pub fn admit(peer: Option<Peer>, own: Option<u32>) -> Result<(), Failure> {
    match (peer, own) {
        (Some(peer), Some(own)) if peer.uid == own => Ok(()),
        _ => Err(Failure::unauthorized(
            // A literal: the refusal-coverage scan reads literals, and a code
            // it cannot read is a code nothing checks is documented.
            "server_peer_refused",
            "this Server answers only processes running as the account that started it",
        )
        .remedy(crate::PEER_REFUSED.remedy)),
    }
}

/// Why a call reached no answer. Each is decided before or instead of
/// sending the request, except `TooLarge` and `Failed`.
#[derive(Debug)]
pub enum CallError {
    /// Nothing is listening: no socket, or a socket nothing answers on.
    Unreachable(String),
    /// Something other than this account's own Server is at the door. Nothing
    /// was sent to it.
    NotOwner(String),
    /// The answer is larger than the caller's bound.
    TooLarge,
    /// The exchange itself failed or timed out.
    Failed(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(reason) | Self::NotOwner(reason) | Self::Failed(reason) => {
                f.write_str(reason)
            }
            Self::TooLarge => f.write_str("server response exceeds the command's bound"),
        }
    }
}

/// One answer: its status and exactly its bytes.
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

#[cfg(not(unix))]
const NO_SOCKETS: &str = "the native server needs an owner-only Unix socket, which this platform does not provide; run the Server and its callers on Linux";

// ── the server half ─────────────────────────────────────────────────────

#[cfg(unix)]
pub use unix::{Listening, listen, serve};
#[cfg(unix)]
mod unix {
    use super::{Peer, own_uid};
    use hyper::body::Incoming;
    use hyper_util::rt::TokioIo;
    use std::fs::{self, File, OpenOptions};
    use std::future::Future;
    use std::io::ErrorKind;
    use std::os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;
    use tower::ServiceExt;

    /// A bound, owner-only socket and the lock that makes it this host's.
    ///
    /// Dropping it removes the socket file, so a stopped Server is "not
    /// running" to its callers rather than a socket nothing answers on, and
    /// releases the lock for the next host.
    pub struct Listening {
        listener: Option<UnixListener>,
        socket: PathBuf,
        _lock: File,
    }

    impl Listening {
        pub fn socket(&self) -> &Path {
            &self.socket
        }
    }

    impl Drop for Listening {
        fn drop(&mut self) {
            drop(self.listener.take());
            // Only a socket is ever removed, and only this host's: it holds
            // the lock, so no other `ds server serve` can have bound here.
            if fs::symlink_metadata(&self.socket).is_ok_and(|meta| meta.file_type().is_socket()) {
                let _ = fs::remove_file(&self.socket);
            }
        }
    }

    /// Take the state directory's single-host lock and bind its owner-only
    /// socket. The directory itself must already be the protected one.
    pub fn listen(state: &Path) -> Result<Listening, String> {
        let lock_path = state.join(super::LOCK);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&lock_path)
            .map_err(|error| format!("cannot open {}: {error}", lock_path.display()))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                return Err(format!(
                    "a Server is already running on {}; use it, or stop it before starting another",
                    state.display()
                ));
            }
            Err(fs::TryLockError::Error(error)) => {
                return Err(format!("cannot lock {}: {error}", lock_path.display()));
            }
        }
        let socket = super::socket_path(state);
        match fs::symlink_metadata(&socket) {
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot read {}: {error}", socket.display())),
            Ok(meta) if !meta.file_type().is_socket() => {
                return Err(format!(
                    "{} is not a socket; remove it, then start the Server again",
                    socket.display()
                ));
            }
            // A socket and no lock holder: a host that died left it. It is
            // removed only when nothing answers on it — something that does
            // answer is not ours to replace, whatever it is.
            Ok(_) => match UnixStream::connect(&socket) {
                Ok(_) => {
                    return Err(format!(
                        "something answers on {} although no Server holds this state directory; stop it, then start the Server again",
                        socket.display()
                    ));
                }
                Err(error) if error.kind() == ErrorKind::ConnectionRefused => {
                    fs::remove_file(&socket).map_err(|error| {
                        format!("cannot remove the stale {}: {error}", socket.display())
                    })?;
                }
                Err(error) => return Err(format!("cannot probe {}: {error}", socket.display())),
            },
        }
        let listener = UnixListener::bind(&socket).map_err(|error| {
            if error.kind() == ErrorKind::InvalidInput {
                format!(
                    "cannot bind {}: {error}; a Unix socket path is limited to about 100 bytes, so choose a shorter --state-dir",
                    socket.display()
                )
            } else {
                format!("cannot bind {}: {error}", socket.display())
            }
        })?;
        // The directory is already 0700, so nobody else could reach the
        // socket in the moment before this; the mode says so on its own too.
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("cannot protect {}: {error}", socket.display()))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        Ok(Listening {
            listener: Some(listener),
            socket,
            _lock: lock,
        })
    }

    /// Answer `router` on this socket until `shutdown` resolves, then let
    /// every open connection finish its request.
    ///
    /// Every accepted connection is asked for its peer's uid once, by the
    /// kernel; each request on it carries that answer to the router, where
    /// [`super::admit`] decides. A connection whose peer the kernel cannot
    /// name is closed unanswered.
    pub async fn serve(
        mut listening: Listening,
        router: axum::Router,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), String> {
        let listener = listening
            .listener
            .take()
            .ok_or_else(|| "this socket is already being served".to_string())?;
        let listener =
            tokio::net::UnixListener::from_std(listener).map_err(|error| error.to_string())?;
        // Closing `stop` tells the accept loop and every connection to wind
        // down; every connection holds a `done` receiver until it has.
        let (stop, stopping) = tokio::sync::watch::channel(());
        let stop = Arc::new(stop);
        tokio::spawn(async move {
            shutdown.await;
            drop(stopping);
        });
        let (done, finished) = tokio::sync::watch::channel(());
        loop {
            let stream = tokio::select! {
                accepted = listener.accept() => match accepted {
                    Ok((stream, _)) => stream,
                    Err(error) => {
                        // A connection that died in the backlog is that
                        // connection's problem; running out of descriptors is
                        // the host's, and passes.
                        if !matches!(
                            error.kind(),
                            ErrorKind::ConnectionAborted
                                | ErrorKind::ConnectionReset
                                | ErrorKind::Interrupted
                        ) {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        continue;
                    }
                },
                _ = stop.closed() => break,
            };
            let Ok(credential) = stream.peer_cred() else {
                continue;
            };
            let peer = Peer {
                uid: credential.uid(),
            };
            let router = router.clone();
            let service =
                hyper::service::service_fn(move |mut request: hyper::Request<Incoming>| {
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
                        _ = connection.as_mut() => break,
                        _ = &mut stopping, if !winding_down => {
                            winding_down = true;
                            connection.as_mut().graceful_shutdown();
                        }
                    }
                }
                drop(finished);
            });
        }
        drop(listener);
        drop(finished);
        done.closed().await;
        // The socket file goes with `listening`, and so does the lock.
        drop(listening);
        Ok(())
    }

    // ── the client half ─────────────────────────────────────────────────

    pub(super) fn call(
        socket: &Path,
        request: hyper::Request<http_body_util::Full<hyper::body::Bytes>>,
        limit: u64,
        timeout: Duration,
    ) -> Result<super::Reply, super::CallError> {
        use super::CallError;
        let own = own_uid().ok_or_else(|| CallError::NotOwner("no account to compare".into()))?;
        // What is at this path must be this account's own socket before
        // anything connects to it — a symlink or someone else's file is
        // refused without a connection.
        match fs::symlink_metadata(socket) {
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(CallError::Unreachable(
                    "no Server is listening on this state directory; start ds server serve".into(),
                ));
            }
            Err(error) => {
                return Err(CallError::Unreachable(format!(
                    "cannot read {}: {error}",
                    socket.display()
                )));
            }
            Ok(meta) => {
                use std::os::unix::fs::MetadataExt;
                if !meta.file_type().is_socket() || meta.uid() != own {
                    return Err(CallError::NotOwner(format!(
                        "{} is not this account's Server socket; nothing was sent",
                        socket.display()
                    )));
                }
            }
        }
        let stream = match UnixStream::connect(socket) {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::ConnectionRefused | ErrorKind::NotFound
                ) =>
            {
                return Err(CallError::Unreachable(
                    "no Server answers on this state directory; start ds server serve".into(),
                ));
            }
            Err(error) => {
                return Err(CallError::Unreachable(format!(
                    "cannot connect to {}: {error}",
                    socket.display()
                )));
            }
        };
        stream
            .set_nonblocking(true)
            .map_err(|error| CallError::Failed(error.to_string()))?;
        let run = move || -> Result<super::Reply, CallError> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| CallError::Failed(error.to_string()))?;
            runtime.block_on(async move {
                let stream = tokio::net::UnixStream::from_std(stream)
                    .map_err(|error| CallError::Failed(error.to_string()))?;
                // The same question the Server asks, asked of the Server: the
                // process that answers must run as this account.
                let answering = stream
                    .peer_cred()
                    .map_err(|error| CallError::Failed(error.to_string()))?;
                if answering.uid() != own {
                    return Err(CallError::NotOwner(
                        "the process answering on this state directory's socket runs as another account; nothing was sent".into(),
                    ));
                }
                tokio::time::timeout(timeout, exchange(stream, request, limit))
                    .await
                    .map_err(|_| {
                        CallError::Failed(format!(
                            "the Server did not answer within {} s",
                            timeout.as_secs()
                        ))
                    })?
            })
        };
        // A caller already inside a runtime cannot block its thread on
        // another one, so the exchange gets a thread of its own there.
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

    async fn exchange(
        stream: tokio::net::UnixStream,
        request: hyper::Request<http_body_util::Full<hyper::body::Bytes>>,
        limit: u64,
    ) -> Result<super::Reply, super::CallError> {
        use super::CallError;
        use http_body_util::BodyExt;
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| CallError::Failed(error.to_string()))?;
        tokio::spawn(connection);
        let response = sender
            .send_request(request)
            .await
            .map_err(|error| CallError::Failed(error.to_string()))?;
        let status = response.status().as_u16();
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let body = http_body_util::Limited::new(response.into_body(), limit)
            .collect()
            .await
            .map_err(|error| {
                if error.is::<http_body_util::LengthLimitError>() {
                    CallError::TooLarge
                } else {
                    CallError::Failed(error.to_string())
                }
            })?
            .to_bytes();
        Ok(super::Reply {
            status,
            body: body.to_vec(),
        })
    }
}

#[cfg(not(unix))]
pub struct Listening {
    never: std::convert::Infallible,
}
#[cfg(not(unix))]
impl Listening {
    pub fn socket(&self) -> &Path {
        match self.never {}
    }
}
#[cfg(not(unix))]
pub fn listen(_: &Path) -> Result<Listening, String> {
    Err(NO_SOCKETS.into())
}
#[cfg(not(unix))]
pub async fn serve(
    listening: Listening,
    _: axum::Router,
    _: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<(), String> {
    match listening.never {}
}

// ── the client half ─────────────────────────────────────────────────────

/// One request to the Server answering on `socket`, from this account only.
///
/// Refused without sending anything when the socket is not this account's or
/// the process behind it runs as another account. The answer is bounded by
/// `limit` bytes and the whole exchange by `timeout`.
pub fn call(
    socket: &Path,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
    limit: u64,
    timeout: Duration,
) -> Result<Reply, CallError> {
    let mut request = hyper::Request::builder()
        .method(method)
        .uri(path)
        // HTTP/1.1 wants a host; this one names no machine, because the door
        // is a file.
        .header("host", "localhost");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = request
        .body(http_body_util::Full::new(
            hyper::body::Bytes::copy_from_slice(body.unwrap_or_default()),
        ))
        .map_err(|error| CallError::Failed(error.to_string()))?;
    #[cfg(unix)]
    {
        unix::call(socket, request, limit, timeout)
    }
    #[cfg(not(unix))]
    {
        let _ = (socket, request, limit, timeout);
        Err(CallError::Unreachable(NO_SOCKETS.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_servers_own_account_is_admitted() {
        assert!(admit(Some(Peer { uid: 1000 }), Some(1000)).is_ok());
        // Another account — root included — is a stranger to this door.
        for peer in [1001, 0, u32::MAX] {
            let refused = admit(Some(Peer { uid: peer }), Some(1000)).unwrap_err();
            assert_eq!(refused.code(), "server_peer_refused");
            assert_eq!(
                refused.class(),
                ds_cli_contract::outcome::ExitClass::Unauthorized
            );
            // The refusal names nobody.
            for said in [refused.message(), refused.remedy_text().unwrap_or_default()] {
                assert!(!said.contains(&peer.to_string()), "{said}");
                assert!(!said.contains("1000"), "{said}");
            }
        }
        // A request that did not come through the socket has no peer, and a
        // platform with no sockets has no account to compare: both refused.
        assert!(admit(None, Some(1000)).is_err());
        assert!(admit(Some(Peer { uid: 1000 }), None).is_err());
        assert!(admit(None, None).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_socket_path_that_is_not_this_accounts_socket_is_refused_before_connecting() {
        let dir = tempfile::tempdir().unwrap();
        let absent = call(
            &socket_path(dir.path()),
            "GET",
            "/v1/jobs",
            &[],
            None,
            1024,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(matches!(absent, CallError::Unreachable(_)), "{absent}");
        // A regular file where the socket belongs is not a door.
        std::fs::write(socket_path(dir.path()), b"").unwrap();
        let file = call(
            &socket_path(dir.path()),
            "GET",
            "/v1/jobs",
            &[],
            None,
            1024,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(matches!(file, CallError::NotOwner(_)), "{file}");
    }

    #[cfg(unix)]
    #[test]
    fn one_host_per_state_directory_and_a_dead_hosts_socket_is_replaced() {
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let first = listen(dir.path()).expect("the first host binds");
        let socket = socket_path(dir.path());
        let meta = std::fs::symlink_metadata(&socket).unwrap();
        assert!(meta.file_type().is_socket());
        assert_eq!(
            meta.permissions().mode() & 0o777,
            0o600,
            "owner-only socket"
        );
        // A second host on the same state directory is refused by the lock.
        let second = listen(dir.path())
            .err()
            .expect("one host per state directory");
        assert!(second.contains("already running"), "{second}");
        drop(first);
        assert!(!socket.exists(), "a stopped host leaves no socket behind");

        // A socket left by a host that died — bound, never served, lock gone.
        let dead = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        drop(dead);
        assert!(socket.exists());
        let again = listen(dir.path()).expect("a dead host's socket is replaced");
        drop(again);

        // Something that DOES answer, without the lock, is never replaced.
        let squatter = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let refused = listen(dir.path()).err().expect("a live socket is not ours");
        assert!(refused.contains("something answers"), "{refused}");
        drop(squatter);
    }
}
