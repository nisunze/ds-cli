//! Native Server adapter for the shared Sync Center runtime.
//!
//! It opens the existing server SQLite database behind one account, deployment,
//! persistent registered installation and project fence. The native auth
//! session is the only gateway door: it refreshes, registers and verifies the
//! signed install lease before every work or artifact request.

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
    /// already owns. A different native identity, lane, or connection token
    /// cannot borrow this session.
    pub fn open(database: &Path, connection: &Connection) -> Result<Self, String> {
        let context = ds_cli_auth::headless_sync_context(&connection.lane)
            .map_err(|error| error.message().to_owned())?;
        let principal = ds_cli_auth::refresh_runtime_identity(&connection.lane)
            .map_err(|error| error.message().to_owned())?;
        let credential_binding = ds_cli_auth::runtime_credential_binding(&connection.lane)
            .map_err(|error| error.message().to_owned())?;
        let owner = auth::identity(&connection.lane)?;
        if owner != connection.owner || owner != principal.uid() {
            return Err("server identity changed; Sync Center access is fenced".to_string());
        }
        let gateway = ds_cli_auth::sync::NativeSyncSession::open(
            ds_cli_auth::Lane::parse(&connection.lane)
                .map_err(|error| error.message().to_owned())?,
            principal,
            context.project_id().to_owned(),
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
            project: context.project_id().to_owned(),
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
    pub fn register_solar_engine(&self, version: &str, release: &str) -> Result<(), String> {
        self.gateway
            .register_engine(ds_cli_auth::sync::NativeEngineAddition::solar(
                version, release,
            ))
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
        require_selected_project(project, &self.project)?;
        let online = || self.authorizer.authorize(&self.owner).is_ok();
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

fn fence_for(account_uid: &str, deployment: &str, install_id: &str) -> Fence {
    Fence {
        account: account_uid.to_owned(),
        deployment: deployment.to_owned(),
        install_id: install_id.to_owned(),
    }
}

fn require_selected_project(project: &str, selected_project: &str) -> Result<(), String> {
    (project == selected_project)
        .then_some(())
        .ok_or_else(|| "the produced project differs from the authenticated native project".into())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_produced_project_must_match_the_authenticated_project() {
        assert!(require_selected_project("project-a", "project-a").is_ok());
        assert!(require_selected_project("project-b", "project-a").is_err());
    }
}
