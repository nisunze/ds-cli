//! The fixed send for `ds` reliability events.
//!
//! `ds_client_core::reporter` decides what an event is and when it is sent;
//! this module is the one closed call that delivers it: the ingest of
//! `domains.sre.client_request_event`, through the same native transport as
//! every other call, with this build's packaged gateway key. No bearer is
//! attached: the ingest attributes such a body to `anonymous`, the contract's
//! signed-out default (ds-command-kernel
//! `docs/contracts/ds-cli-reliability-and-feedback.md` §3.2), and restoring a
//! session at exit could refresh a credential inside the send budget.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ds_client_core::reporter::{
    EventSink, ReporterStore, ReporterStoreError, SendFailure, TransportSink,
};
use serde_json::Value;

/// The kernel reporter, re-exported: the binary reaches the core through this crate.
pub use ds_client_core::reporter;

use crate::profile::{self, Lane};
use crate::transport::NativeTransport;

/// The release lane this build reports on: `canary` or `stable` for an
/// installed release, `local` for a development build (contract §3.4).
pub fn release_lane() -> &'static str {
    if cfg!(debug_assertions) {
        return "local";
    }
    let root = option_env!("DS_NATIVE_CLIENT_PRODUCT_ROOT")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if root.contains("canary") {
        return "canary";
    }
    if !root.is_empty() {
        return "stable";
    }
    // A Windows release carries its catalog beside the executable instead of
    // a compiled product root; its installed folder names the lane.
    if cfg!(windows) {
        let installed = std::env::current_exe()
            .map(|path| path.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        return if installed.contains("canary") {
            "canary"
        } else {
            "stable"
        };
    }
    "local"
}

/// The `X-Request-Id` of the last gateway call this process sent, which joins
/// an event to the ds-brain rows of the same request.
pub fn last_request_id() -> Option<String> {
    crate::transport::last_correlation_id()
}

/// Deliver one reporter body. A build without a usable packaged profile is
/// unreachable, not refused: the event waits in the outbox.
pub fn send_event(event: &Value) -> Result<(), SendFailure> {
    let lane = if release_lane() == "canary" {
        Lane::Canary
    } else {
        Lane::Stable
    };
    let profile = profile::load(lane).map_err(|_| SendFailure::Unreachable)?;
    let mut transport = NativeTransport;
    TransportSink {
        profile: &profile,
        transport: &mut transport,
        bearer_token: None,
    }
    .send(event)
}

const STATE_FILE: &str = "reliability.json";
const LOCK_FILE: &str = "reliability.lock";
const MAX_STATE_BYTES: u64 = 4 * 1024 * 1024;
const LOCK_WAIT: Duration = Duration::from_millis(250);
const STALE_LOCK: Duration = Duration::from_secs(10);

/// The reporter's state document under the DS state root, guarded by a
/// create-new lock file. Never `/tmp` or `/dev/shm`.
pub struct FileStore {
    dir: PathBuf,
    held: bool,
}

impl FileStore {
    pub fn open() -> Option<Self> {
        state_root().map(|dir| Self { dir, held: false })
    }
}

/// `$XDG_STATE_HOME/ds`, otherwise `$HOME/.local/state/ds`, otherwise (on
/// Windows) `%LOCALAPPDATA%\ds\state`: the same private root as the other DS
/// machine state, never a credential namespace.
fn state_root() -> Option<PathBuf> {
    let absolute = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    absolute("XDG_STATE_HOME")
        .map(|base| base.join("ds"))
        .or_else(|| absolute("HOME").map(|home| home.join(".local").join("state").join("ds")))
        .or_else(|| absolute("LOCALAPPDATA").map(|base| base.join("ds").join("state")))
}

fn unavailable() -> ReporterStoreError {
    ReporterStoreError("the reporter state is unavailable")
}

impl ReporterStore for FileStore {
    fn acquire(&mut self) -> Result<(), ReporterStoreError> {
        fs::create_dir_all(&self.dir).map_err(|_| unavailable())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::symlink_metadata(&self.dir)
                && meta.is_dir()
                && meta.permissions().mode() & 0o077 != 0
            {
                let _ = fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700));
            }
        }
        let lock = self.dir.join(LOCK_FILE);
        let deadline = Instant::now() + LOCK_WAIT;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&lock) {
                Ok(_) => {
                    self.held = true;
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    // A lock this old belongs to a process that died holding it.
                    let stale = fs::metadata(&lock)
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|modified| modified.elapsed().ok())
                        .is_some_and(|age| age > STALE_LOCK);
                    if stale {
                        let _ = fs::remove_file(&lock);
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err(ReporterStoreError("the reporter state is busy"));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return Err(unavailable()),
            }
        }
    }

    fn load(&mut self) -> Result<Option<Vec<u8>>, ReporterStoreError> {
        let path = self.dir.join(STATE_FILE);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(unavailable()),
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => Err(unavailable()),
            // An oversized document is not ours to parse; start over.
            Ok(meta) if meta.len() > MAX_STATE_BYTES => Ok(None),
            Ok(_) => fs::read(&path).map(Some).map_err(|_| unavailable()),
        }
    }

    fn save(&mut self, document: &[u8]) -> Result<(), ReporterStoreError> {
        let path = self.dir.join(STATE_FILE);
        let temporary = self
            .dir
            .join(format!("{STATE_FILE}.{}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let written = options
            .open(&temporary)
            .and_then(|mut file| file.write_all(document))
            .and_then(|()| fs::rename(&temporary, &path));
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
            return Err(unavailable());
        }
        Ok(())
    }

    fn release(&mut self) {
        if self.held {
            let _ = fs::remove_file(self.dir.join(LOCK_FILE));
            self.held = false;
        }
    }
}
