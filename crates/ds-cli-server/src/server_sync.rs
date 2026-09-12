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

    pub fn project(&self) -> &str {
        &self.project
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
        require_session_project(project, &self.project)?;
        // Two separate facts, and the sync pass needs both: this host still
        // holds its owner's credential (local), and the last gateway refresh
        // reached the gateway (recorded by the background refresher, never
        // asked here — a store pass must not turn into a login attempt).
        let online = || self.authorizer.authorize(&self.owner).is_ok() && auth::gateway_reachable();
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
