//! Closed native Sync Center control-plane calls.
//!
//! This is deliberately a closed-operation door. A host can submit the runtime's
//! already-typed JSON declaration, but cannot select an origin, route, method,
//! bearer, or another project for reads and grant admission. Publication is
//! scoped by the server-owned grant, not a client-supplied project field.

use serde_json::{json, Value};

use crate::{ClientError, ErrorKind};

const MAX_BODY_BYTES: usize = 24 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncGatewayOperation {
    InstallHeartbeat,
    ComputeArtifacts,
    ComputeArtifactsOpen,
    ComputeArtifactsFinalize,
    WorkOpen,
    WorkPublish,
}

impl SyncGatewayOperation {
    pub const fn registry_id(self) -> &'static str {
        match self {
            Self::InstallHeartbeat => "domains.app_installs.heartbeat",
            Self::ComputeArtifacts => "domains.compute_artifacts.list",
            Self::ComputeArtifactsOpen => "domains.compute_artifacts.open",
            Self::ComputeArtifactsFinalize => "domains.compute_artifacts.finalize",
            Self::WorkOpen => "domains.work.open",
            Self::WorkPublish => "domains.work.publish",
        }
    }
}

/// Build-pinned non-secret installation facts. The caller never supplies a
/// route or an arbitrary additions tree.
pub struct InstallHeartbeat<'a> {
    pub install_id: &'a str,
    pub device: &'a str,
    pub arch: &'a str,
    pub app_version: &'a str,
    pub channel: &'a str,
    pub os_version: &'a str,
    pub webview_version: &'a str,
    pub addition_name: &'a str,
    pub addition_version: &'a str,
    pub addition_release: &'a str,
    pub addition_state: &'a str,
}

/// Build-pinned installation facts for a heartbeat that intentionally reports
/// no optional engine additions. A native host uses this only while reading
/// the shared record before it knows which sealed report release needs a
/// work grant; `work/open` then registers that exact release atomically.
pub struct InstallHeartbeatWithoutAddition<'a> {
    pub install_id: &'a str,
    pub device: &'a str,
    pub arch: &'a str,
    pub app_version: &'a str,
    pub channel: &'a str,
    pub os_version: &'a str,
    pub webview_version: &'a str,
}

pub struct NativeSyncRequest {
    operation: SyncGatewayOperation,
    body: Value,
}

/// The only Solar calculation declaration a native Server may publish. The
/// caller supplies durable sealed facts; this type fixes the engine,
/// operation family, variant, and single `report-input` output that ds-brain
/// admits for a calculation head.
pub struct SolarCalculationArtifactOpen<'a> {
    pub project_id: &'a str,
    pub client_run_id: &'a str,
    pub city_id: &'a str,
    pub engine_version: &'a str,
    pub engine_build_manifest_sha256: &'a str,
    pub input_base_fingerprint: &'a str,
    pub source_snapshot_sha256: &'a str,
    pub snapshot_receipt_id: &'a str,
    pub output_sha256: &'a str,
    pub output_size_bytes: u64,
}

/// The exact idempotent identity returned by an opened Solar artifact run.
pub struct SolarCalculationArtifactFinalize<'a> {
    pub project_id: &'a str,
    pub work_id: &'a str,
    pub client_run_id: &'a str,
}

impl NativeSyncRequest {
    pub fn heartbeat(identity: InstallHeartbeat<'_>) -> Result<Self, ClientError> {
        for value in [
            identity.install_id,
            identity.device,
            identity.arch,
            identity.app_version,
            identity.channel,
            identity.os_version,
            identity.webview_version,
            identity.addition_name,
            identity.addition_version,
            identity.addition_release,
            identity.addition_state,
        ] {
            bounded(value)?;
        }
        Ok(Self {
            operation: SyncGatewayOperation::InstallHeartbeat,
            body: json!({
                "action": "heartbeat", "install_id": identity.install_id, "device": identity.device,
                "arch": identity.arch, "app_version": identity.app_version, "channel": identity.channel,
                "os_version": identity.os_version, "webview_version": identity.webview_version,
                "additions": { identity.addition_name: { "version": identity.addition_version,
                    "release": identity.addition_release, "state": identity.addition_state }},
            }),
        })
    }

    /// The existing app-install heartbeat contract permits an empty additions
    /// map. This preserves the installation/lease authority for record reads
    /// without claiming a reporter release before a sealed `work/open`
    /// declaration selects one.
    pub fn heartbeat_without_addition(
        identity: InstallHeartbeatWithoutAddition<'_>,
    ) -> Result<Self, ClientError> {
        for value in [
            identity.install_id,
            identity.device,
            identity.arch,
            identity.app_version,
            identity.channel,
            identity.os_version,
            identity.webview_version,
        ] {
            bounded(value)?;
        }
        Ok(Self {
            operation: SyncGatewayOperation::InstallHeartbeat,
            body: json!({
                "action": "heartbeat", "install_id": identity.install_id, "device": identity.device,
                "arch": identity.arch, "app_version": identity.app_version, "channel": identity.channel,
                "os_version": identity.os_version, "webview_version": identity.webview_version,
                "additions": {},
            }),
        })
    }

    pub fn solar_calculation_open(
        declaration: SolarCalculationArtifactOpen<'_>,
    ) -> Result<Self, ClientError> {
        for value in [
            declaration.project_id,
            declaration.client_run_id,
            declaration.city_id,
        ] {
            artifact_id(value)?;
        }
        canonical_snapshot_receipt(declaration.snapshot_receipt_id)?;
        bounded(declaration.engine_version)?;
        for digest in [
            declaration.engine_build_manifest_sha256,
            declaration.input_base_fingerprint,
            declaration.source_snapshot_sha256,
            declaration.output_sha256,
        ] {
            sha256(digest)?;
        }
        if !declaration.engine_version.starts_with("ds-solar-engine@")
            || declaration.output_size_bytes == 0
            || declaration.output_size_bytes > 64 * 1024 * 1024
        {
            return Err(invalid());
        }
        let operation = format!("calculate-{}", declaration.city_id);
        artifact_id(&operation)?;
        Ok(Self {
            operation: SyncGatewayOperation::ComputeArtifactsOpen,
            body: json!({
                "action": "open",
                "project_id": declaration.project_id,
                "client_run_id": declaration.client_run_id,
                "engine": "solar",
                "operation": operation,
                "variant": "default",
                "engine_version": declaration.engine_version,
                "engine_build_manifest_sha256": declaration.engine_build_manifest_sha256,
                "input_base_fingerprint": declaration.input_base_fingerprint,
                "source_snapshot_sha256": declaration.source_snapshot_sha256,
                "snapshot_receipt_id": declaration.snapshot_receipt_id,
                "outputs": [{
                    "output_id": "report-input",
                    "format": "json",
                    "content_type": "application/json",
                    "sha256": declaration.output_sha256,
                    "size_bytes": declaration.output_size_bytes,
                }],
            }),
        })
    }

    pub fn solar_calculation_finalize(
        identity: SolarCalculationArtifactFinalize<'_>,
    ) -> Result<Self, ClientError> {
        for value in [
            identity.project_id,
            identity.work_id,
            identity.client_run_id,
        ] {
            artifact_id(value)?;
        }
        Ok(Self {
            operation: SyncGatewayOperation::ComputeArtifactsFinalize,
            body: json!({
                "action": "finalize",
                "project_id": identity.project_id,
                "work_id": identity.work_id,
                "client_run_id": identity.client_run_id,
            }),
        })
    }

    /// Declare outputs against a server-owned work grant. The deployed publish
    /// contract has no project field: the authority resolves the grant and
    /// checks its principal, installation, project entitlement and attribution.
    pub fn work_publish(body: &Value) -> Result<Self, ClientError> {
        let object = body.as_object().ok_or_else(invalid)?;
        if object.len() != 4
            || !object.contains_key("client_publish_id")
            || !object.contains_key("grant_id")
            || !object.get("artifacts").is_some_and(Value::is_array)
            || !object.get("base_revisions").is_some_and(Value::is_object)
        {
            return Err(invalid());
        }
        for key in ["client_publish_id", "grant_id"] {
            artifact_id(
                object
                    .get(key)
                    .and_then(Value::as_str)
                    .ok_or_else(invalid)?,
            )?;
        }
        if serde_json::to_vec(body).map_err(|_| invalid())?.len() > MAX_BODY_BYTES {
            return Err(invalid());
        }
        Ok(Self {
            operation: SyncGatewayOperation::WorkPublish,
            body: body.clone(),
        })
    }

    /// Fence a runtime-declared request to its one selected project. This is
    /// intentionally not an arbitrary JSON route: operation, project key and
    /// compute action vocabulary are fixed here.
    pub fn for_project(
        operation: SyncGatewayOperation,
        selected_project: &str,
        body: &Value,
    ) -> Result<Self, ClientError> {
        if matches!(
            operation,
            SyncGatewayOperation::InstallHeartbeat
                | SyncGatewayOperation::WorkPublish
                | SyncGatewayOperation::ComputeArtifactsOpen
                | SyncGatewayOperation::ComputeArtifactsFinalize
        ) {
            return Err(invalid());
        }
        bounded(selected_project)?;
        let object = body.as_object().ok_or_else(invalid)?;
        let key = match operation {
            SyncGatewayOperation::ComputeArtifacts => "project_id",
            SyncGatewayOperation::WorkOpen => "ds_project",
            SyncGatewayOperation::InstallHeartbeat
            | SyncGatewayOperation::WorkPublish
            | SyncGatewayOperation::ComputeArtifactsOpen
            | SyncGatewayOperation::ComputeArtifactsFinalize => unreachable!(),
        };
        if object.get(key).and_then(Value::as_str) != Some(selected_project) {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "sync request project is not the selected project",
            ));
        }
        if operation == SyncGatewayOperation::ComputeArtifacts
            && !matches!(
                object.get("action").and_then(Value::as_str),
                Some("list" | "read" | "download")
            )
        {
            return Err(invalid());
        }
        if serde_json::to_vec(body).map_err(|_| invalid())?.len() > MAX_BODY_BYTES {
            return Err(invalid());
        }
        Ok(Self {
            operation,
            body: body.clone(),
        })
    }
    pub const fn operation(&self) -> SyncGatewayOperation {
        self.operation
    }
    pub(crate) fn body(&self) -> &Value {
        &self.body
    }
}

fn bounded(value: &str) -> Result<(), ClientError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    Ok(())
}
fn artifact_id(value: &str) -> Result<(), ClientError> {
    if value.is_empty()
        || value.len() > 128
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(invalid());
    }
    Ok(())
}
fn sha256(value: &str) -> Result<(), ClientError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid());
    }
    Ok(())
}
fn canonical_snapshot_receipt(value: &str) -> Result<(), ClientError> {
    let bytes = value.as_bytes();
    if bytes.len() != 36
        || !bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            14 => matches!(*byte, b'1'..=b'5'),
            19 => matches!(*byte, b'8' | b'9' | b'a' | b'b'),
            _ => byte.is_ascii_digit() || matches!(*byte, b'a'..=b'f'),
        })
    {
        return Err(invalid());
    }
    Ok(())
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "native sync request is outside its closed contract",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_and_action_are_closed_before_a_bearer_can_be_attached() {
        let list = json!({"action":"list", "project_id":"project-a", "limit":200});
        assert!(NativeSyncRequest::for_project(
            SyncGatewayOperation::ComputeArtifacts,
            "project-a",
            &list
        )
        .is_ok());
        assert!(NativeSyncRequest::for_project(
            SyncGatewayOperation::ComputeArtifacts,
            "project-b",
            &list
        )
        .is_err());
        let forbidden = json!({"action":"finalize", "project_id":"project-a"});
        assert!(NativeSyncRequest::for_project(
            SyncGatewayOperation::ComputeArtifacts,
            "project-a",
            &forbidden
        )
        .is_err());
    }

    #[test]
    fn work_routes_require_the_ds_project_fence() {
        let open =
            json!({"ds_project":"project-a", "install_id":"550e8400-e29b-41d4-a716-446655440000"});
        assert!(
            NativeSyncRequest::for_project(SyncGatewayOperation::WorkOpen, "project-a", &open)
                .is_ok()
        );
        assert!(NativeSyncRequest::for_project(
            SyncGatewayOperation::WorkPublish,
            "project-b",
            &open
        )
        .is_err());
    }

    #[test]
    fn publication_uses_the_authority_grant_contract_without_an_invented_project() {
        let publish = json!({
            "client_publish_id":"publish-a", "grant_id":"grant_a",
            "artifacts":[], "base_revisions":{}
        });
        let request = NativeSyncRequest::work_publish(&publish).unwrap();
        assert_eq!(request.body(), &publish);
        assert_eq!(request.operation(), SyncGatewayOperation::WorkPublish);
        for key in [
            "grant_id",
            "client_publish_id",
            "artifacts",
            "base_revisions",
        ] {
            let mut missing = publish.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(NativeSyncRequest::work_publish(&missing).is_err());
        }
        let mut injected = publish.clone();
        injected["ds_project"] = json!("project-b");
        assert!(NativeSyncRequest::work_publish(&injected).is_err());
        injected = publish;
        injected["grant_id"] = json!("../other-grant");
        assert!(NativeSyncRequest::work_publish(&injected).is_err());
    }

    #[test]
    fn record_reads_can_renew_an_install_without_preclaiming_an_engine_release() {
        let request =
            NativeSyncRequest::heartbeat_without_addition(InstallHeartbeatWithoutAddition {
                install_id: "550e8400-e29b-41d4-a716-446655440000",
                device: "linux",
                arch: "x86_64",
                app_version: "1.2.3",
                channel: "canary",
                os_version: "linux native-server",
                webview_version: "native-server",
            })
            .unwrap();
        assert_eq!(request.operation(), SyncGatewayOperation::InstallHeartbeat);
        assert_eq!(request.body()["additions"], json!({}));
    }

    #[test]
    fn solar_calculation_declaration_has_no_work_grant_or_output_vocabulary_escape() {
        let declaration = SolarCalculationArtifactOpen {
            project_id: "project-a",
            client_run_id: "solar-job-a",
            city_id: "city-a",
            engine_version: "ds-solar-engine@1.2.3",
            engine_build_manifest_sha256: &"a".repeat(64),
            input_base_fingerprint: &"b".repeat(64),
            source_snapshot_sha256: &"c".repeat(64),
            snapshot_receipt_id: "550e8400-e29b-41d4-a716-446655440000",
            output_sha256: &"d".repeat(64),
            output_size_bytes: 42,
        };
        let request = NativeSyncRequest::solar_calculation_open(declaration).unwrap();
        assert_eq!(
            request.operation(),
            SyncGatewayOperation::ComputeArtifactsOpen
        );
        assert_eq!(request.body()["action"], "open");
        assert_eq!(request.body()["engine"], "solar");
        assert_eq!(request.body()["operation"], "calculate-city-a");
        assert_eq!(request.body()["variant"], "default");
        assert_eq!(request.body()["outputs"][0]["output_id"], "report-input");
        assert_eq!(request.body()["outputs"][0]["format"], "json");
        assert_eq!(
            request.body()["outputs"][0]["content_type"],
            "application/json"
        );
        assert!(request.body().get("grant_id").is_none());
    }

    #[test]
    fn solar_calculation_requires_release_and_snapshot_receipts() {
        let request = SolarCalculationArtifactOpen {
            project_id: "project-a",
            client_run_id: "solar-job-a",
            city_id: "city-a",
            engine_version: "ds-solar-engine@development",
            engine_build_manifest_sha256: "",
            input_base_fingerprint: &"b".repeat(64),
            source_snapshot_sha256: &"c".repeat(64),
            snapshot_receipt_id: "receipt-a",
            output_sha256: &"d".repeat(64),
            output_size_bytes: 42,
        };
        assert!(NativeSyncRequest::solar_calculation_open(request).is_err());
    }

    #[test]
    fn solar_calculation_accepts_the_snapshot_authoritys_uuid_v5_receipt() {
        let request = SolarCalculationArtifactOpen {
            project_id: "project-a",
            client_run_id: "solar-job-a",
            city_id: "city-a",
            engine_version: "ds-solar-engine@1.2.3",
            engine_build_manifest_sha256: &"a".repeat(64),
            input_base_fingerprint: &"b".repeat(64),
            source_snapshot_sha256: &"c".repeat(64),
            snapshot_receipt_id: "21c655a1-1037-52ce-a601-e9c7e3734d53",
            output_sha256: &"d".repeat(64),
            output_size_bytes: 42,
        };
        assert!(NativeSyncRequest::solar_calculation_open(request).is_ok());
    }
}
