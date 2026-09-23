//! Closed native Sync Center control-plane calls.
//!
//! This is deliberately a closed-operation door. A host can submit the runtime's
//! already-typed JSON declaration, but cannot select an origin, route, method,
//! bearer, or another project. Every read and every publication is fenced to
//! the selected project; the record asks the session's own authority.

use serde_json::{json, Value};

use crate::{ClientError, ErrorKind};

const MAX_BODY_BYTES: usize = 24 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncGatewayOperation {
    InstallHeartbeat,
    ComputeArtifacts,
    ComputeArtifactsOpen,
    ComputeArtifactsFinalize,
}

impl SyncGatewayOperation {
    pub const fn registry_id(self) -> &'static str {
        match self {
            Self::InstallHeartbeat => "domains.app_installs.heartbeat",
            Self::ComputeArtifacts => "domains.compute_artifacts.list",
            Self::ComputeArtifactsOpen => "domains.compute_artifacts.open",
            Self::ComputeArtifactsFinalize => "domains.compute_artifacts.finalize",
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

/// Build-pinned installation facts for a heartbeat that reports no optional
/// engine additions: the installation's lease, and nothing about which build
/// will compute what.
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

    /// The app-install heartbeat contract permits an empty additions map: the
    /// installation's lease without an engine claim.
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

    /// One report publication on the shared record's door, exactly as the
    /// sync runtime declares it: `open` of one `export-<transformer>` run, or
    /// `finalize` of the run `open` answered with. The shape is closed here and
    /// fenced to the selected project; the engine build it names is provenance
    /// the record keeps, never a credential this request carries.
    pub fn report_publication(selected_project: &str, body: &Value) -> Result<Self, ClientError> {
        bounded(selected_project)?;
        let object = body.as_object().ok_or_else(invalid)?;
        let text = |key: &str| object.get(key).and_then(Value::as_str).ok_or_else(invalid);
        if text("project_id")? != selected_project {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "sync request project is not the selected project",
            ));
        }
        let operation = match text("action")? {
            "open" => {
                const KEYS: [&str; 10] = [
                    "action",
                    "project_id",
                    "client_run_id",
                    "engine",
                    "operation",
                    "variant",
                    "engine_version",
                    "engine_build_manifest_sha256",
                    "input_base_fingerprint",
                    "outputs",
                ];
                if object.len() != KEYS.len() || !KEYS.iter().all(|key| object.contains_key(*key)) {
                    return Err(invalid());
                }
                artifact_id(text("client_run_id")?)?;
                let operation = text("operation")?;
                artifact_id(operation)?;
                if text("engine")? != "network_reporter"
                    || text("variant")? != "default"
                    || !operation.starts_with("export-")
                {
                    return Err(invalid());
                }
                bounded(text("engine_version")?)?;
                let build = text("engine_build_manifest_sha256")?;
                if !build.is_empty() {
                    sha256(build)?;
                }
                sha256(text("input_base_fingerprint")?)?;
                let outputs = object
                    .get("outputs")
                    .and_then(Value::as_array)
                    .filter(|outputs| (1..=32).contains(&outputs.len()))
                    .ok_or_else(invalid)?;
                for output in outputs {
                    let output = output.as_object().ok_or_else(invalid)?;
                    let field = |key: &str| output.get(key).and_then(Value::as_str);
                    if output.len() != 5
                        || output
                            .get("size_bytes")
                            .and_then(Value::as_u64)
                            .is_none_or(|size| size == 0)
                    {
                        return Err(invalid());
                    }
                    artifact_id(field("output_id").ok_or_else(invalid)?)?;
                    bounded(field("format").ok_or_else(invalid)?)?;
                    bounded(field("content_type").ok_or_else(invalid)?)?;
                    sha256(field("sha256").ok_or_else(invalid)?)?;
                }
                SyncGatewayOperation::ComputeArtifactsOpen
            }
            "finalize" => {
                if object.len() != 4 {
                    return Err(invalid());
                }
                artifact_id(text("work_id")?)?;
                artifact_id(text("client_run_id")?)?;
                SyncGatewayOperation::ComputeArtifactsFinalize
            }
            _ => return Err(invalid()),
        };
        if serde_json::to_vec(body).map_err(|_| invalid())?.len() > MAX_BODY_BYTES {
            return Err(invalid());
        }
        Ok(Self {
            operation,
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
        if operation != SyncGatewayOperation::ComputeArtifacts {
            return Err(invalid());
        }
        bounded(selected_project)?;
        let object = body.as_object().ok_or_else(invalid)?;
        if object.get("project_id").and_then(Value::as_str) != Some(selected_project) {
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
    fn a_report_publication_is_closed_to_its_declaration_and_its_project() {
        let open = json!({
            "action": "open", "project_id": "project-a",
            "client_run_id": "publish-76ab72a6-6b1d-4095-a54e-139232b5152c",
            "engine": "network_reporter", "operation": "export-agatare_1", "variant": "default",
            "engine_version": "ds-network-reporter@0.1.0+development",
            "engine_build_manifest_sha256": "5".repeat(64),
            "input_base_fingerprint": "b".repeat(64),
            "outputs": [{"output_id": "xlsx", "format": "xlsx",
                "content_type": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "sha256": "a".repeat(64), "size_bytes": 28469}],
        });
        let request = NativeSyncRequest::report_publication("project-a", &open).unwrap();
        assert_eq!(
            request.operation(),
            SyncGatewayOperation::ComputeArtifactsOpen
        );
        assert_eq!(request.body(), &open);
        let finalize = json!({"action": "finalize", "project_id": "project-a",
            "work_id": "work_abc", "client_run_id": "publish-76ab72a6-6b1d-4095-a54e-139232b5152c"});
        assert_eq!(
            NativeSyncRequest::report_publication("project-a", &finalize)
                .unwrap()
                .operation(),
            SyncGatewayOperation::ComputeArtifactsFinalize
        );

        assert!(NativeSyncRequest::report_publication("project-b", &open).is_err());
        let mut solar = open.clone();
        solar["engine"] = json!("solar");
        let mut extra = open.clone();
        extra["authority_grant_id"] = json!("grant_a");
        let mut unfenced = open.clone();
        unfenced["input_base_fingerprint"] = json!("");
        let mut aggregate = open.clone();
        aggregate["operation"] = json!("calculate-kigali");
        let mut empty = open.clone();
        empty["outputs"] = json!([]);
        let mut list = open.clone();
        list["action"] = json!("list");
        for refused in [solar, extra, unfenced, aggregate, empty, list] {
            assert!(
                NativeSyncRequest::report_publication("project-a", &refused).is_err(),
                "{refused}"
            );
        }
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
