//! Native Server adapter for the shared Sync Center runtime.
//!
//! It opens the existing server SQLite database behind one account, deployment,
//! persistent registered installation and project fence. The native auth
//! session is the only gateway door: it refreshes, registers and verifies the
//! signed install lease before every work or artifact request.
//!
//! The project is now an argument, not a startup capture: one session per
//! authorized project, opened by [`sessions::ServerSessions`] when an
//! operation is first admitted into that project. Nothing here reads a saved
//! selection.

/// Declared from here rather than from `lib.rs` so the Server's per-project
/// session map arrives without moving anything the CLI side owns.
#[path = "sessions.rs"]
pub mod sessions;

use std::path::Path;

use ds_compute_runtime::{Authorizer, digest};
use ds_sync_runtime::{Gateway, Producer, Reads, SharedStore, StoreHost, open_store, store::Fence};

use crate::{auth, host::Connection};

/// One refreshed native account and selected project, bound to the server's
/// protected local control identity.
pub struct ServerSyncSession {
    store: SharedStore,
    fence: Fence,
    /// The host identity this session's rows and jobs are fenced by — what
    /// a producer set opened beside this session (`ds report outbox drain`)
    /// reads the compute table with.
    identity: ds_compute_runtime::HostIdentity,
    worker_id: String,
    project: String,
    owner: String,
    gateway: ds_cli_auth::sync::NativeSyncSession,
    authorizer: auth::NativeAuthorizer,
}

impl ServerSyncSession {
    /// Open the Sync Center store in the same SQLite file the native server
    /// already owns, for one explicitly named project. A different native
    /// identity, lane, or connection token cannot borrow this session.
    ///
    /// The project is the caller's, verified before this is reached; this
    /// function never asks what is selected.
    pub fn open(database: &Path, connection: &Connection, project: &str) -> Result<Self, String> {
        let context = ds_cli_auth::headless_principal(&connection.lane)
            .map_err(|error| error.message().to_owned())?;
        let principal = ds_cli_auth::refresh_runtime_identity(&connection.lane)
            .map_err(|error| error.message().to_owned())?;
        let credential_binding = ds_cli_auth::runtime_credential_binding(&connection.lane)
            .map_err(|error| error.message().to_owned())?;
        // The owner fence this machine HOLDS, read locally. The refresh above
        // is the gateway session's own, which is what a Sync Center pass is;
        // the fence itself never depends on an upstream answering.
        let owner = auth::owner_fence(&connection.lane)?;
        let principal_owner = digest(
            &serde_json::to_vec(&(
                principal.uid(),
                principal.lane(),
                principal.credential_audience_sha256(),
            ))
            .map_err(|error| error.to_string())?,
        );
        require_sync_identity(
            &connection.owner,
            &owner,
            &principal_owner,
            context.account_uid(),
            principal.uid(),
        )?;
        let gateway = ds_cli_auth::sync::NativeSyncSession::open(
            ds_cli_auth::Lane::parse(&connection.lane)
                .map_err(|error| error.message().to_owned())?,
            principal,
            project.to_owned(),
            credential_binding,
        )?;
        let install_id = gateway.install_id().to_owned();
        let authorizer = auth::NativeAuthorizer::new(connection.lane.clone())?;
        Ok(Self {
            store: open_store(database)?,
            fence: fence_for(context.account_uid(), context.deployment(), &install_id),
            identity: ds_compute_runtime::HostIdentity {
                owner: owner.clone(),
                principal: ds_command_kernel::execution_context::Principal {
                    uid: context.account_uid().to_owned(),
                    lane: connection.lane.clone(),
                    deployment: context.deployment().to_owned(),
                    install_id: install_id.clone(),
                },
            },
            worker_id: format!(
                "{install_id}#{}#{}",
                digest(gateway.credential_binding().as_bytes()),
                std::process::id()
            ),
            project: project.to_owned(),
            owner,
            gateway,
            authorizer,
        })
    }

    pub fn fence(&self) -> &Fence {
        &self.fence
    }

    /// The host identity this session's rows and jobs are fenced by.
    pub fn identity(&self) -> &ds_compute_runtime::HostIdentity {
        &self.identity
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    /// Free this project's sync lease when its holder is provably gone, and
    /// name the holder that still blocks it otherwise.
    ///
    /// A worker id is `<install>#<credential digest>#<pid>`. A lease is
    /// abandoned only when it was taken on THIS install by another process
    /// that is not running here; a live holder, another install's, or one
    /// whose process cannot be observed keeps it until it expires (at most
    /// 15 minutes). Reclaiming releases it under the holder's own id, the one
    /// release the store accepts.
    pub fn reclaim_abandoned_lease(&self) -> Result<LeaseReading, String> {
        let scope = ds_sync_runtime::rows::store_scope(&self.project);
        let mut store = self
            .store
            .lock()
            .map_err(|_| "The sync gate is unavailable".to_string())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or_default();
        let Some(lease) = store
            .leases_of_fence(&self.fence)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|lease| lease.scope == scope && lease.expires_at_ms > now)
        else {
            return Ok(LeaseReading::default());
        };
        let own_install = &self.identity.principal.install_id;
        if abandoned_holder(
            own_install,
            &self.worker_id,
            &lease.worker_id,
            process_running,
        ) {
            store
                .apply(
                    &self.fence,
                    now,
                    ds_sync_runtime::store::Event::LeaseRelease {
                        scope,
                        worker_id: lease.worker_id.clone(),
                    },
                )
                .map_err(|error| error.to_string())?;
            return Ok(LeaseReading {
                reclaimed_from: Some(lease.worker_id),
                blocked_by: None,
            });
        }
        Ok(LeaseReading {
            reclaimed_from: None,
            blocked_by: Some(BlockingLease {
                holder_running: holder_pid(own_install, &lease.worker_id).and_then(process_running),
                worker_id: lease.worker_id,
                expires_at_ms: lease.expires_at_ms,
            }),
        })
    }

    /// The store this session writes through, shared with every host over
    /// this project: the seal records its row here, the pump drains from
    /// here. One store per execution context.
    pub fn store(&self) -> &SharedStore {
        &self.store
    }

    /// This project's artifact rows as the store holds them under this
    /// session's fence. The store is the queue; this is its reading for one
    /// project, decided nowhere else.
    pub fn rows(&self) -> Result<Vec<ds_sync_runtime::store::ArtifactRow>, String> {
        let store = self
            .store
            .lock()
            .map_err(|_| "The sync gate is unavailable".to_string())?;
        store
            .snapshot(
                &self.fence,
                &ds_sync_runtime::rows::store_scope(&self.project),
            )
            .map(|snapshot| snapshot.artifacts)
            .map_err(|error| error.to_string())
    }

    /// Bind this host to the exact Solar engine release that will produce its
    /// rows. The signed install heartbeat happens here and is renewed before
    /// every gateway request; a connection secret never names an install.
    pub fn register_solar_engine(
        &self,
        version: &str,
        release: &str,
    ) -> Result<(), ds_cli_auth::sync::SolarPublicationError> {
        self.gateway
            .register_engine_for_solar(ds_cli_auth::sync::NativeEngineAddition::solar(
                version, release,
            ))
    }

    /// Publish one already-verified prepared Solar result through the
    /// compute-artifact authority. The closure receives only its minted object
    /// session, so runtime output bytes cannot acquire a control-plane bearer.
    pub fn publish_solar_calculation(
        &self,
        declaration: ds_cli_auth::SolarCalculationArtifactOpen<'_>,
        guard: &dyn Fn() -> Result<(), String>,
        transfer: impl FnOnce(&str) -> Result<ds_sync_runtime::TransferReceipt, String>,
    ) -> Result<ds_cli_auth::sync::SolarPublicationReceipt, ds_cli_auth::sync::SolarPublicationError>
    {
        self.gateway
            .publish_solar_calculation(declaration, guard, transfer)
    }

    /// Construct a shared StoreHost for one caller-owned producer and read
    /// cache. The closure prevents the borrowed seams from escaping into a
    /// second queue or transport surface.
    pub fn with_host_for_project<T>(
        &self,
        project: &str,
        producer: &dyn Producer,
        reads: &dyn Reads,
        f: impl FnOnce(&StoreHost<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        ds_sync_runtime::run_contained("server sync host call", || {
            require_session_project(project, &self.project)?;
            // Two separate facts, and the sync pass needs both: this host still
            // holds its owner's credential (local), and the last gateway refresh
            // reached the gateway (recorded by the background refresher, never
            // asked here — a store pass must not turn into a login attempt).
            let online =
                || self.authorizer.authorize(&self.owner).is_ok() && auth::gateway_reachable();
            let host = StoreHost::new(
                self.store.clone(),
                self.fence.clone(),
                self.worker_id.clone(),
                &self.project,
                &self.gateway,
                producer,
                reads,
                &online,
            )?;
            f(&host)
        })
    }

    pub fn gateway(&self) -> &dyn Gateway {
        &self.gateway
    }
}

fn require_sync_identity(
    connection_owner: &str,
    current_owner: &str,
    principal_owner: &str,
    context_uid: &str,
    principal_uid: &str,
) -> Result<(), String> {
    if connection_owner != current_owner
        || current_owner != principal_owner
        || context_uid != principal_uid
    {
        return Err("server identity changed; Sync Center access is fenced".into());
    }
    Ok(())
}

/// What a drain found on its project's sync lease before it ran.
#[derive(Debug, Default)]
pub struct LeaseReading {
    /// A holder proven gone whose lease this session released.
    pub reclaimed_from: Option<String>,
    /// A holder that still blocks the project.
    pub blocked_by: Option<BlockingLease>,
}

#[derive(Debug)]
pub struct BlockingLease {
    pub worker_id: String,
    pub expires_at_ms: u64,
    /// Whether the holder's process runs on this machine; `None` when it is
    /// another install's worker or cannot be observed here.
    pub holder_running: Option<bool>,
}

/// The pid of a worker of THIS install, or `None` for any other id.
fn holder_pid(own_install: &str, worker_id: &str) -> Option<u32> {
    let mut parts = worker_id.split('#');
    let (install, _credential, pid) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || install != own_install {
        return None;
    }
    pid.parse().ok()
}

/// A lease is abandoned only when its holder is another process of this
/// install that is provably not running. Anything unobservable keeps it.
fn abandoned_holder(
    own_install: &str,
    own_worker: &str,
    holder: &str,
    running: impl Fn(u32) -> Option<bool>,
) -> bool {
    holder != own_worker
        && holder_pid(own_install, holder).is_some_and(|pid| running(pid) == Some(false))
}

/// Whether a pid runs on this machine; `None` where that cannot be asked.
fn process_running(pid: u32) -> Option<bool> {
    if cfg!(target_os = "linux") {
        Some(Path::new(&format!("/proc/{pid}")).exists())
    } else {
        None
    }
}

/// The sync fence of this host's identity, as every session of it derives
/// it: account, deployment, registered install — no project.
pub fn fence_of(identity: &ds_compute_runtime::HostIdentity) -> Fence {
    fence_for(
        &identity.principal.uid,
        &identity.principal.deployment,
        &identity.principal.install_id,
    )
}

fn fence_for(account_uid: &str, deployment: &str, install_id: &str) -> Fence {
    Fence {
        account: account_uid.to_owned(),
        deployment: deployment.to_owned(),
        install_id: install_id.to_owned(),
    }
}

/// A session belongs to exactly one project. Asking it to produce another
/// one is a host bug — the caller should have asked `ServerSessions` for that
/// project's own session — and it is refused rather than silently retargeted.
fn require_session_project(project: &str, session_project: &str) -> Result<(), String> {
    (project == session_project).then_some(()).ok_or_else(|| {
        "the produced project differs from the project this session was opened for".into()
    })
}

#[cfg(test)]
mod tests {

    /// A drain frees a lease only from a provably dead process of this same
    /// install (b06fad17): a live holder, another install's worker, this
    /// session itself, a malformed id and an unobservable pid all keep it.
    #[test]
    fn a_lease_is_reclaimed_only_from_a_dead_process_of_this_install() {
        let dead = |_pid: u32| Some(false);
        let live = |_pid: u32| Some(true);
        let unknown = |_pid: u32| None;
        let own = "install-a#cred#100";
        assert!(super::abandoned_holder(
            "install-a",
            own,
            "install-a#cred#3930728",
            dead
        ));
        assert!(!super::abandoned_holder(
            "install-a",
            own,
            "install-a#cred#3930728",
            live
        ));
        assert!(!super::abandoned_holder(
            "install-a",
            own,
            "install-a#cred#3930728",
            unknown
        ));
        assert!(!super::abandoned_holder(
            "install-a",
            own,
            "install-b#cred#3930728",
            dead
        ));
        assert!(!super::abandoned_holder("install-a", own, own, dead));
        assert!(!super::abandoned_holder(
            "install-a",
            own,
            "install-a#3930728",
            dead
        ));
        assert!(!super::abandoned_holder(
            "install-a",
            own,
            "install-a#cred#pid",
            dead
        ));
        assert_eq!(
            super::holder_pid("install-a", "install-a#cred#42"),
            Some(42)
        );
        assert_eq!(super::holder_pid("install-a", "install-a#cred#42#x"), None);
    }
    use super::*;

    #[test]
    fn sync_accepts_matching_owner_digests_and_compares_user_ids_separately() {
        let owner = digest(b"uid/lane/audience");
        assert_ne!(owner, "uid-a");
        assert!(require_sync_identity(&owner, &owner, &owner, "uid-a", "uid-a").is_ok());
        assert!(require_sync_identity("previous-owner", &owner, &owner, "uid-a", "uid-a").is_err());
        assert!(
            require_sync_identity(&owner, &owner, "other-principal", "uid-a", "uid-a").is_err()
        );
        assert!(require_sync_identity(&owner, &owner, &owner, "uid-b", "uid-a").is_err());
    }

    #[test]
    fn fence_carries_native_principal_deployment_and_registered_install() {
        let first = fence_for(
            "uid-a",
            "https://gateway.example.com",
            "550e8400-e29b-41d4-a716-446655440000",
        );
        let same = fence_for(
            "uid-a",
            "https://gateway.example.com",
            "550e8400-e29b-41d4-a716-446655440000",
        );
        let different_install = fence_for(
            "uid-a",
            "https://gateway.example.com",
            "550e8400-e29b-41d4-a716-446655440001",
        );
        assert_eq!(first.account, "uid-a");
        assert_eq!(first.deployment, "https://gateway.example.com");
        assert_eq!(first.install_id, same.install_id);
        assert_ne!(first.install_id, different_install.install_id);
    }

    #[test]
    fn a_produced_project_must_match_the_project_its_session_was_opened_for() {
        assert!(require_session_project("project-a", "project-a").is_ok());
        assert!(require_session_project("project-b", "project-a").is_err());
    }
}
