//! Native Server's closed Sync Center authority session.
//!
//! The session owns the restored native provider, persistent installation UUID
//! and signed install lease. Callers receive only `Gateway::post` over the
//! shared record's one route; they never receive a bearer or HTTP handle.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use ds_client_core::{
    Client, ClientError, InstallHeartbeat, InstallHeartbeatWithoutAddition, NativeSyncRequest,
    SyncGatewayOperation,
};
use ds_edge_authority::{
    AuthorityPins, AuthorityVerifier, ExpectedInstall, InstallLeaseCapability,
    VerifiedInstallStatus, VerifyInstallRequest, load_or_create_install_id,
};
use ds_sync_runtime::{Gateway, GatewayError, SyncRoute};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    context::ProviderIdentity,
    profile::Lane,
    state::{NativeRefreshStore, edge_authority_dir},
    transport::NativeTransport,
};

/// The exact same reviewed trust input `src-tauri/build.rs` seals into a
/// desktop release. The native Server does not invent a signer project.
const RELEASE_TRUST: &str = include_str!("../../../../ds-command-kernel/edge-authority-trust.json");

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatAnswer {
    status: String,
    lease_expires_at: String,
    min_supported_version: String,
    server_time: String,
    lease: InstallLeaseCapability,
}

enum SyncProvider {
    Device(Box<crate::device::DeviceSession>),
    Firebase(Box<Client<NativeTransport, NativeRefreshStore>>),
}

impl SyncProvider {
    fn sync_gateway(
        &mut self,
        request: &NativeSyncRequest,
        now: u64,
    ) -> Result<Value, ClientError> {
        match self {
            Self::Device(device) => device.sync_gateway(request),
            Self::Firebase(client) => client.sync_gateway(request, now),
        }
    }
}

struct SessionState {
    client: SyncProvider,
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

/// The two read-only compute-artifact discovery calls. The caller supplies
/// selectors, never an action, route, bearer, or request body.
pub enum PublicationHeadQuery<'a> {
    List {
        limit: u8,
        cursor: Option<&'a str>,
    },
    Show {
        engine: &'a str,
        operation: &'a str,
        variant: &'a str,
    },
}

/// Read the shared publication heads through the native user's project-fenced
/// Sync Center session. This opens no report outbox or local SQLite store.
pub fn publication_heads_for_project(
    lane_value: &str,
    project: &str,
    query: PublicationHeadQuery<'_>,
) -> Result<Value, ds_cli_contract::Failure> {
    use ds_cli_contract::Failure;

    let lane = Lane::parse(lane_value)?;
    let principal = crate::headless_identity_for_named_project(lane_value)?;
    let credential_binding = crate::runtime_credential_binding(lane_value)?;
    let session = NativeSyncSession::open(lane, principal, project.to_owned(), credential_binding)
        .map_err(|detail| {
            Failure::unavailable("publication_unavailable", detail)
                .remedy("check the native account and project, then retry the read")
        })?;
    let body = match query {
        PublicationHeadQuery::List { limit, cursor } => {
            let mut body = json!({
                "action": "list", "project_id": project, "limit": limit,
            });
            if let Some(cursor) = cursor {
                body["cursor"] = json!(cursor);
            }
            body
        }
        PublicationHeadQuery::Show {
            engine,
            operation,
            variant,
        } => json!({
            "action": "read", "project_id": project,
            "engine": engine, "operation": operation, "variant": variant,
        }),
    };
    session
        .post(SyncRoute::ComputeArtifacts, &body)
        .map_err(publication_read_error)
}

pub(crate) fn publication_read_error(error: GatewayError) -> ds_cli_contract::Failure {
    use ds_cli_contract::Failure;

    let detail = json!({
        "http_status": error.status,
        "service_code": error.code,
    });
    match error.status {
        Some(400 | 422) => Failure::invalid(
            "publication_selector_invalid",
            "the publication selector or cursor was refused",
        )
        .detail(detail)
        .remedy("use the exact project, head identity, or cursor returned by a prior list"),
        Some(401 | 403) => Failure::unauthorized(
            "publication_not_permitted",
            "the native account cannot read this project's publications",
        )
        .detail(detail)
        .remedy("check this account's access to the named project"),
        Some(404) => Failure::invalid(
            "publication_not_found",
            "that publication head was not found",
        )
        .detail(detail)
        .remedy("list the project's publication heads and use an exact identity"),
        Some(409) => Failure::conflict(
            "publication_conflict",
            "the shared publication head has an inconsistent record",
        )
        .detail(detail)
        .remedy("report the project and head identity; do not infer publication from local files"),
        _ => Failure::unavailable(
            "publication_unavailable",
            "the shared publication authority did not return a usable answer",
        )
        .detail(detail)
        .remedy("retry the same read when the service is available"),
    }
}

impl NativeSyncSession {
    pub fn open(
        lane: Lane,
        principal: ProviderIdentity,
        selected_project: String,
        credential_binding: String,
    ) -> Result<Self, String> {
        if ds_client_core::validate_project_id(&selected_project).is_err() {
            return Err("native Sync Center selected project is invalid".into());
        }
        let authority_dir =
            edge_authority_dir(lane.token()).map_err(|error| error.message().to_owned())?;
        let install_id = load_or_create_install_id(&authority_dir.join("install-id"))?;
        let client = if let Some(device) =
            crate::device::restore_session(lane).map_err(|error| error.message().to_owned())?
        {
            if device.context().uid() != principal.uid() {
                return Err("native Sync Center principal changed".into());
            }
            SyncProvider::Device(Box::new(device))
        } else {
            let profile = crate::profile::load(lane).map_err(|error| error.message().to_owned())?;
            let mut client = Client::new(
                profile,
                NativeTransport,
                NativeRefreshStore::open().map_err(|error| error.message().to_owned())?,
            );
            let user = client
                .restore(now())
                .map_err(gateway_error)?
                .ok_or("native Sync Center session is signed out")?;
            if user.uid() != principal.uid() {
                return Err("native Sync Center principal changed".into());
            }
            SyncProvider::Firebase(Box::new(client))
        };
        let session = Self {
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
        };
        session.assert_runtime_fence()?;
        Ok(session)
    }

    pub fn install_id(&self) -> &str {
        &self.install_id
    }

    pub fn credential_binding(&self) -> &str {
        &self.credential_binding
    }
    pub fn lane(&self) -> &str {
        self.lane.token()
    }

    fn heartbeat_locked(&self, state: &mut SessionState) -> Result<(), GatewayError> {
        self.refresh_provider(state)?;
        let request = self
            .heartbeat_request(state.addition.as_ref())
            .map_err(gateway_error)?;
        let data = state
            .client
            .sync_gateway(&request, now())
            .map_err(gateway_error)?;
        self.assert_runtime_fence().map_err(credential_error)?;
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
            return Err(credential_error(
                "native installation lease is not active for this principal",
            ));
        }
        Ok(())
    }

    fn refresh_provider(&self, state: &mut SessionState) -> Result<(), GatewayError> {
        self.assert_runtime_fence().map_err(credential_error)?;
        if let SyncProvider::Device(device) = &mut state.client {
            let restored = crate::device::restore_session(self.lane)
                .map_err(classify_device_refresh_error)?
                .ok_or_else(|| {
                    credential_error("native Sync Center device credential was removed")
                })?;
            let before = device.context();
            let after = restored.context();
            if before.uid() != after.uid()
                || before.device_id() != after.device_id()
                || before.fingerprint() != after.fingerprint()
                || before.credential_audience_sha256() != after.credential_audience_sha256()
            {
                return Err(credential_error(
                    "native Sync Center credential instance changed",
                ));
            }
            **device = restored;
        }
        self.assert_runtime_fence().map_err(credential_error)
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

    fn heartbeat_request(
        &self,
        addition: Option<&NativeEngineAddition>,
    ) -> Result<NativeSyncRequest, ClientError> {
        match addition {
            Some(addition) => NativeSyncRequest::heartbeat(InstallHeartbeat {
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
            }),
            None => {
                NativeSyncRequest::heartbeat_without_addition(InstallHeartbeatWithoutAddition {
                    install_id: &self.install_id,
                    device: &self.device,
                    arch: &self.arch,
                    app_version: &self.app_version,
                    channel: self.lane.token(),
                    os_version: "linux native-server",
                    webview_version: "native-server",
                })
            }
        }
    }

    fn execute(
        &self,
        request: NativeSyncRequest,
        addition: Option<NativeEngineAddition>,
    ) -> Result<Value, GatewayError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| GatewayError::from("native Sync Center session is unavailable"))?;
        // Renew/verify the selected native provider before every operation.
        // Fail closed for revocation, principal drift, lane drift, or a
        // blocked install instead of relying on an old local lease.
        if let Some(addition) = addition {
            state.addition = Some(addition);
        }
        self.heartbeat_locked(&mut state)?;
        state
            .client
            .sync_gateway(&request, now())
            .map_err(gateway_error)
    }

    /// One call on the shared record, fenced to this session's project. A
    /// report publication's `open` and `finalize` take their own closed shape;
    /// every other action is a read. The refusal keeps the route's status, so
    /// the loop can tell a verdict on the publication from an outage.
    fn post(&self, route: SyncRoute, body: &Value) -> Result<Value, GatewayError> {
        let SyncRoute::ComputeArtifacts = route;
        let request = match body.get("action").and_then(Value::as_str) {
            Some("open" | "finalize") => NativeSyncRequest::publication(&self.project, body),
            _ => NativeSyncRequest::for_project(
                SyncGatewayOperation::ComputeArtifacts,
                &self.project,
                body,
            ),
        }
        .map_err(gateway_error)?;
        let addition = if body["action"] == "open" {
            let release = body["engine_version"]
                .as_str()
                .ok_or("publication release missing")?;
            let (name, version) = release
                .split_once('@')
                .ok_or("publication release invalid")?;
            let addition = NativeEngineAddition {
                name: name.into(),
                version: version.into(),
                release: release.into(),
            };
            validate_addition(&addition)?;
            Some(addition)
        } else {
            None
        };
        self.execute(request, addition)
    }
}

impl Gateway for NativeSyncSession {
    fn post(&self, route: SyncRoute, body: &Value) -> Result<Value, GatewayError> {
        self.post(route, body)
    }
}

/// A client error as the sync loop reads it: the route's status and code when
/// it refused, so a verdict is never mistaken for an outage.
fn gateway_error(error: ClientError) -> GatewayError {
    let refusal = error
        .service_refusal()
        .map(|refusal| (refusal.status(), refusal.code().map(str::to_owned)));
    let detail = client_error(error);
    match refusal {
        Some((status, code)) => GatewayError::refused(status, code, detail),
        None => detail.into(),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |value| value.as_secs())
}
fn client_error(error: ds_client_core::ClientError) -> String {
    match error.service_refusal() {
        Some(refusal) => format!(
            "{} (HTTP {}{}{})",
            error,
            refusal.status(),
            refusal
                .code()
                .map(|code| format!(", {code}"))
                .unwrap_or_default(),
            refusal
                .message()
                .map(|message| format!(": {message}"))
                .unwrap_or_default(),
        ),
        None => error.to_string(),
    }
}

fn credential_error(detail: impl Into<String>) -> GatewayError {
    GatewayError::refused(401, Some("credential_refused".into()), detail)
}
fn classify_device_refresh_error(error: ds_cli_contract::Failure) -> GatewayError {
    if error.code() == "device_auth_transient" {
        error.message().to_owned().into()
    } else {
        credential_error(error.message())
    }
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
    if !matches!(
        addition.name.as_str(),
        "ds-solar-engine" | "ds-network-reporter"
    ) || addition.version.is_empty()
        || addition.release.is_empty()
        || addition.version.len() > 120
        || addition.release.len() > 256
        || addition.release.contains(char::is_control)
        || (addition.name == "ds-network-reporter"
            && (addition.release != format!("ds-network-reporter@{}", addition.version)
                || !valid_reporter_version(&addition.version)))
    {
        return Err("native Sync Center engine release is invalid".into());
    }
    Ok(())
}

fn valid_reporter_version(version: &str) -> bool {
    let Some(revision) = version.strip_prefix("0.1.0+") else {
        return false;
    };
    ds_cli_contract::util::is_hex(revision, 40, ds_cli_contract::util::HexCase::Lower)
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

    #[test]
    fn reporter_registration_is_bound_to_its_declared_release() {
        let mut addition = NativeEngineAddition {
            name: "ds-network-reporter".into(),
            version: format!("0.1.0+{}", "a".repeat(40)),
            release: format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)),
        };
        assert!(validate_addition(&addition).is_ok());
        addition.release = "ds-solar-engine@0.1.0+abc".into();
        assert!(validate_addition(&addition).is_err());
        addition.name = "unregistered-engine".into();
        assert!(validate_addition(&addition).is_err());
    }

    #[test]
    fn device_refresh_outage_retries_but_removed_authority_blocks() {
        assert!(matches!(
            classify_device_refresh_error(ds_cli_contract::Failure::unavailable(
                "device_auth_transient",
                "offline"
            )),
            GatewayError { status: None, .. }
        ));
        assert!(matches!(
            classify_device_refresh_error(ds_cli_contract::Failure::conflict(
                "auth_context_mismatch",
                "changed"
            )),
            GatewayError {
                status: Some(401),
                ..
            }
        ));
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
