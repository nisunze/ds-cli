//! Native Server's closed Sync Center authority session.
//!
//! The session owns the restored Firebase client, persistent installation UUID
//! and signed install lease. Callers receive only `Gateway::post` over the
//! three Sync Center routes; they never receive a bearer or HTTP handle.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use ds_client_core::{Client, InstallHeartbeat, NativeSyncRequest, SyncGatewayOperation};
use ds_edge_authority::{
    AuthorityPins, AuthorityVerifier, ExpectedInstall, InstallLeaseCapability,
    VerifiedInstallStatus, VerifyInstallRequest, load_or_create_install_id,
};
use ds_sync_runtime::{Gateway, SyncRoute};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    context::ProviderIdentity,
    profile::Lane,
    state::{NativeRefreshStore, edge_authority_dir},
    transport::NativeTransport,
};

/// The exact same reviewed trust input `src-tauri/build.rs` seals into a
/// desktop release. The native Server does not invent a signer project.
const RELEASE_TRUST: &str = include_str!("../../../../ds-web/edge-authority-trust.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseTrust {
    schema: u8,
    contract: String,
    lanes: ReleaseTrustLanes,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseTrustLanes {
    canary: ReleaseTrustLane,
    stable: ReleaseTrustLane,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseTrustLane {
    status: String,
    signer: String,
    issuer: String,
    audience: String,
}

#[derive(Clone, Debug)]
pub struct NativeEngineAddition {
    pub name: String,
    pub version: String,
    pub release: String,
}

impl NativeEngineAddition {
    pub fn solar(version: impl Into<String>, release: impl Into<String>) -> Self {
        Self {
            name: "ds-solar-engine".into(),
            version: version.into(),
            release: release.into(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatAnswer {
    status: String,
    lease_expires_at: String,
    min_supported_version: String,
    server_time: String,
    lease: InstallLeaseCapability,
}

struct SessionState {
    client: Client<NativeTransport, NativeRefreshStore>,
    addition: Option<NativeEngineAddition>,
}

/// A durable native session, fenced to one profile lane, principal, credential
/// instance and installation. Refresh occurs before every Sync Center call so
/// a revoked credential cannot continue on an old install lease.
pub struct NativeSyncSession {
    lane: Lane,
    principal: ProviderIdentity,
    project: String,
    credential_binding: String,
    install_id: String,
    device: String,
    arch: String,
    app_version: String,
    authority_dir: std::path::PathBuf,
    state: Mutex<SessionState>,
}

impl NativeSyncSession {
    pub fn open(
        lane: Lane,
        principal: ProviderIdentity,
        selected_project: String,
        credential_binding: String,
    ) -> Result<Self, String> {
        if selected_project.is_empty()
            || selected_project.len() > 200
            || selected_project.chars().any(char::is_control)
        {
            return Err("native Sync Center selected project is invalid".into());
        }
        let authority_dir =
            edge_authority_dir(lane.token()).map_err(|error| error.message().to_owned())?;
        let install_id = load_or_create_install_id(&authority_dir.join("install-id"))?;
        let profile = crate::profile::load(lane).map_err(|error| error.message().to_owned())?;
        let mut client = Client::new(
            profile,
            NativeTransport,
            NativeRefreshStore::open().map_err(|error| error.message().to_owned())?,
        );
        let user = client
            .restore(now())
            .map_err(client_error)?
            .ok_or("native Sync Center session is signed out")?;
        if user.uid() != principal.uid() {
            return Err("native Sync Center principal changed".into());
        }
        Ok(Self {
            lane,
            principal,
            project: selected_project,
            credential_binding,
            install_id,
            device: native_device()?.into(),
            arch: native_arch()?.into(),
            app_version: env!("DS_NATIVE_APP_VERSION").into(),
            authority_dir,
            state: Mutex::new(SessionState {
                client,
                addition: None,
            }),
        })
    }

    pub fn install_id(&self) -> &str {
        &self.install_id
    }

    /// Register the exact release whose prepared executor will use this
    /// session. A release is not guessed from a connection secret or an env
    /// default; the Solar runtime supplies its sealed engine identity.
    pub fn register_engine(&self, addition: NativeEngineAddition) -> Result<(), String> {
        validate_addition(&addition)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "native Sync Center session is unavailable")?;
        state.addition = Some(addition);
        self.heartbeat_locked(&mut state)
    }

    pub fn credential_binding(&self) -> &str {
        &self.credential_binding
    }
    pub fn lane(&self) -> &str {
        self.lane.token()
    }

    fn heartbeat_locked(&self, state: &mut SessionState) -> Result<(), String> {
        self.assert_runtime_fence()?;
        let addition = state
            .addition
            .as_ref()
            .ok_or("native Sync Center installation has no registered engine release")?;
        let request = NativeSyncRequest::heartbeat(InstallHeartbeat {
            install_id: &self.install_id,
            device: &self.device,
            arch: &self.arch,
            app_version: &self.app_version,
            channel: self.lane.token(),
            os_version: "linux native-server",
            webview_version: "native-server",
            addition_name: &addition.name,
            addition_version: &addition.version,
            addition_release: &addition.release,
            addition_state: "ready",
        })
        .map_err(client_error)?;
        let data = state
            .client
            .sync_gateway(&request, now())
            .map_err(client_error)?;
        self.assert_runtime_fence()?;
        let answer: HeartbeatAnswer = serde_json::from_value(data)
            .map_err(|_| "native install heartbeat response is outside its closed contract")?;
        let pins = pins(self.lane)?;
        let verified = AuthorityVerifier::new(&self.authority_dir, pins)?.verify_install(
            VerifyInstallRequest {
                server_time: answer.server_time,
                capability: answer.lease,
                expected: ExpectedInstall {
                    install_id: self.install_id.clone(),
                    device: self.device.clone(),
                    arch: self.arch.clone(),
                    app_version: self.app_version.clone(),
                    channel: self.lane.token().into(),
                    owner_uid: Some(self.principal.uid().to_owned()),
                    status: answer.status,
                    min_supported_version: answer.min_supported_version,
                    lease_expires_at: answer.lease_expires_at,
                },
            },
        )?;
        if verified.status != VerifiedInstallStatus::Active
            || verified.owner_uid.as_deref() != Some(self.principal.uid())
        {
            return Err("native installation lease is not active for this principal".into());
        }
        Ok(())
    }

    fn assert_runtime_fence(&self) -> Result<(), String> {
        let (principal, _) = crate::probe_headless_identity(self.lane.token())
            .map_err(|error| error.message().to_owned())?
            .ok_or("native Sync Center session is signed out")?;
        let credential_binding = crate::runtime_credential_binding(self.lane.token())
            .map_err(|error| error.message().to_owned())?;
        check_runtime_fence(
            &self.principal,
            &self.credential_binding,
            &principal,
            &credential_binding,
        )
    }

    fn post(&self, route: SyncRoute, body: &Value) -> Result<Value, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "native Sync Center session is unavailable")?;
        // Renew/verify before every operation. This refreshes Firebase first
        // and fails closed for revocation, principal drift, lane drift, or a
        // blocked install instead of relying on an old local lease.
        self.heartbeat_locked(&mut state)?;
        let operation = match route {
            SyncRoute::ComputeArtifacts => SyncGatewayOperation::ComputeArtifacts,
            SyncRoute::WorkOpen => SyncGatewayOperation::WorkOpen,
            SyncRoute::WorkPublish => SyncGatewayOperation::WorkPublish,
        };
        let request =
            NativeSyncRequest::for_project(operation, &self.project, body).map_err(client_error)?;
        state
            .client
            .sync_gateway(&request, now())
            .map_err(client_error)
    }
}

impl Gateway for NativeSyncSession {
    fn post(&self, route: SyncRoute, body: &Value) -> Result<Value, String> {
        self.post(route, body)
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |value| value.as_secs())
}
fn client_error(error: ds_client_core::ClientError) -> String {
    error.to_string()
}
fn native_device() -> Result<&'static str, String> {
    if cfg!(target_os = "linux") {
        Ok("linux")
    } else {
        Err("native Sync Center server requires Linux".into())
    }
}
fn native_arch() -> Result<&'static str, String> {
    if cfg!(target_arch = "x86_64") {
        Ok("x86_64")
    } else {
        Err("native Sync Center server requires x86_64".into())
    }
}
fn validate_addition(addition: &NativeEngineAddition) -> Result<(), String> {
    if addition.name != "ds-solar-engine"
        || addition.version.is_empty()
        || addition.release.is_empty()
        || addition.version.len() > 120
        || addition.release.len() > 256
        || addition.release.contains(char::is_control)
    {
        return Err("native Sync Center engine release is invalid".into());
    }
    Ok(())
}
fn check_runtime_fence(
    expected_principal: &ProviderIdentity,
    expected_credential_binding: &str,
    actual_principal: &ProviderIdentity,
    actual_credential_binding: &str,
) -> Result<(), String> {
    if actual_principal != expected_principal {
        return Err("native Sync Center principal changed".into());
    }
    if actual_credential_binding != expected_credential_binding {
        return Err("native Sync Center credential instance changed".into());
    }
    Ok(())
}
fn pins(lane: Lane) -> Result<AuthorityPins, String> {
    let trust: ReleaseTrust = serde_json::from_str(RELEASE_TRUST)
        .map_err(|_| "native Sync Center release authority trust is malformed")?;
    if trust.schema != 1 || trust.contract != "ds-edge-authority-trust/v1" {
        return Err("native Sync Center release authority trust has an invalid contract".into());
    }
    let selected = match lane {
        Lane::Canary => trust.lanes.canary,
        Lane::Stable => trust.lanes.stable,
    };
    if selected.status != "provisioned" {
        return Err("native Sync Center release authority is not provisioned".into());
    }
    AuthorityPins::new(
        lane.token(),
        selected.signer,
        selected.issuer,
        selected.audience,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_pins_and_application_version_are_sealed_build_inputs() {
        assert!(pins(Lane::Canary).is_ok());
        assert!(pins(Lane::Stable).is_ok());
        assert!(
            env!("DS_NATIVE_APP_VERSION")
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'+'))
        );
        assert_ne!(env!("DS_NATIVE_APP_VERSION"), env!("CARGO_PKG_VERSION"));
    }

    fn identity(uid: &str) -> ProviderIdentity {
        ProviderIdentity::new("canary", &"a".repeat(64), uid).unwrap()
    }

    #[test]
    fn refresh_fence_refuses_a_different_principal_or_credential_instance() {
        let principal = identity("uid-a");
        assert!(
            check_runtime_fence(&principal, "credential-a", &principal, "credential-a").is_ok()
        );
        assert!(
            check_runtime_fence(
                &principal,
                "credential-a",
                &identity("uid-b"),
                "credential-a"
            )
            .is_err()
        );
        assert!(
            check_runtime_fence(&principal, "credential-a", &principal, "credential-b").is_err()
        );
    }
}
