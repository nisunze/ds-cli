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

    /// One engine-neutral publication on the fixed compute-artifacts door.
    /// The kernel validates its closed declaration before credentials attach;
    /// the selected project and the record's admission remain authoritative.
    pub fn publication(selected_project: &str, body: &Value) -> Result<Self, ClientError> {
        use ds_command_kernel::sync::declaration::{validate, Action};
        let operation = match validate(selected_project, body).map_err(|_| invalid())? {
            Action::Open => SyncGatewayOperation::ComputeArtifactsOpen,
            Action::Finalize => SyncGatewayOperation::ComputeArtifactsFinalize,
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
                "sha256": "a".repeat(64), "size_bytes": 28469, "filename":"agatare_1.xlsx"}],
        });
        let request = NativeSyncRequest::publication("project-a", &open).unwrap();
        assert_eq!(
            request.operation(),
            SyncGatewayOperation::ComputeArtifactsOpen
        );
        assert_eq!(request.body(), &open);
        let finalize = json!({"action": "finalize", "project_id": "project-a",
            "work_id": "work_abc", "client_run_id": "publish-76ab72a6-6b1d-4095-a54e-139232b5152c"});
        assert_eq!(
            NativeSyncRequest::publication("project-a", &finalize)
                .unwrap()
                .operation(),
            SyncGatewayOperation::ComputeArtifactsFinalize
        );

        assert!(NativeSyncRequest::publication("project-b", &open).is_err());
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
                NativeSyncRequest::publication("project-a", &refused).is_err(),
                "{refused}"
            );
        }
    }

    #[test]
    fn report_publication_keeps_closed_renderer_receipt_metadata() {
        let mut open = json!({
            "action":"open", "project_id":"project-a", "client_run_id":"publish-1234",
            "engine":"network_reporter", "operation":"export-agahehe", "variant":"default",
            "engine_version":"ds-network-reporter@0.1.0+held-build",
            "engine_build_manifest_sha256":"5".repeat(64),
            "input_base_fingerprint":"b".repeat(64),
            "outputs":[{"output_id":"pdf__a0-landscape-gisagara-cjic", "format":"pdf__a0-landscape-gisagara-cjic",
                "content_type":"application/pdf", "sha256":"a".repeat(64),
                "size_bytes":3586561, "filename":"Agahehe.a0-landscape-gisagara-cjic.pdf"}],
        });
        // Current non-print outputs carry a filename without print metadata.
        assert_eq!(
            NativeSyncRequest::publication("project-a", &open)
                .unwrap()
                .body(),
            &open
        );
        open["outputs"][0]["paper_size"] = json!("A0");
        open["outputs"][0]["presentation"] = json!({
            "layout_id":"a0-landscape-gisagara-cjic", "layout_name":"A0 LV CATEGORICAL - GISAGARA CJIC", "paper_name":"A0",
            "orientation":"landscape", "page_mm":[1189,841],
        });
        assert_eq!(
            NativeSyncRequest::publication("project-a", &open)
                .unwrap()
                .body(),
            &open
        );
        assert!(NativeSyncRequest::publication("project-b", &open).is_err());
        for (path, value) in [
            ("/outputs/0/filename", json!("../report.pdf")),
            ("/outputs/0/filename", json!("report\\escape.pdf")),
            ("/outputs/0/filename", json!("report\n.pdf")),
            ("/outputs/0/filename", json!("a".repeat(256))),
            ("/outputs/0/filename", Value::Null),
            ("/outputs/0/paper_size", json!("A3")),
            ("/outputs/0/presentation", json!({"layout_id":"governed"})),
            ("/outputs/0/presentation", Value::Null),
            ("/outputs/0/presentation/orientation", json!("portrait")),
            ("/outputs/0/presentation/page_mm", json!([1189, 841, 1])),
            ("/outputs/0/presentation/page_mm", json!([0, 841])),
            ("/outputs/0/sha256", json!("bad")),
            ("/outputs/0/size_bytes", json!(0)),
        ] {
            let mut refused = open.clone();
            *refused.pointer_mut(path).unwrap() = value;
            assert!(
                NativeSyncRequest::publication("project-a", &refused).is_err(),
                "{path}: {refused}"
            );
        }
        for path in ["/outputs/0", "/outputs/0/presentation"] {
            let mut refused = open.clone();
            refused
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("authority_grant_id".into(), json!("grant_a"));
            assert!(
                NativeSyncRequest::publication("project-a", &refused).is_err(),
                "{path}"
            );
        }
        let mut missing_paper = open;
        missing_paper["outputs"][0]
            .as_object_mut()
            .unwrap()
            .remove("paper_size");
        assert!(NativeSyncRequest::publication("project-a", &missing_paper).is_err());
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
        let declaration = json!({"project_id": "project-a", "client_run_id": "solar-job-a", "engine_version": "ds-solar-engine@1.2.3", "engine_build_manifest_sha256": &"a".repeat(64), "snapshot_receipt_id": "550e8400-e29b-41d4-a716-446655440000", "engine": "solar", "operation": format!("calculate-{}", "city-a"), "variant": "default", "action": "open", "input_base_fingerprint": &"b".repeat(64), "source_snapshot_sha256": &"c".repeat(64), "outputs": [{"output_id": "report-input", "format": "json", "content_type": "application/json", "sha256": &"d".repeat(64), "size_bytes": 42}]});
        let request = NativeSyncRequest::publication("project-a", &declaration).unwrap();
        let common = NativeSyncRequest::publication("project-a", request.body()).unwrap();
        assert_eq!(common.body(), request.body());
        assert_eq!(common.operation(), request.operation());
        assert!(NativeSyncRequest::publication("project-b", request.body()).is_err());
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
        let request = json!({"project_id": "project-a", "client_run_id": "solar-job-a", "engine_version": "ds-solar-engine@development", "engine_build_manifest_sha256": "", "snapshot_receipt_id": "receipt-a", "engine": "solar", "operation": format!("calculate-{}", "city-a"), "variant": "default", "action": "open", "input_base_fingerprint": &"b".repeat(64), "source_snapshot_sha256": &"c".repeat(64), "outputs": [{"output_id": "report-input", "format": "json", "content_type": "application/json", "sha256": &"d".repeat(64), "size_bytes": 42}]});
        assert!(NativeSyncRequest::publication("project-a", &request).is_err());
    }

    #[test]
    fn solar_calculation_accepts_the_snapshot_authoritys_uuid_v5_receipt() {
        let request = json!({"project_id": "project-a", "client_run_id": "solar-job-a", "engine_version": "ds-solar-engine@1.2.3", "engine_build_manifest_sha256": &"a".repeat(64), "snapshot_receipt_id": "21c655a1-1037-52ce-a601-e9c7e3734d53", "engine": "solar", "operation": format!("calculate-{}", "city-a"), "variant": "default", "action": "open", "input_base_fingerprint": &"b".repeat(64), "source_snapshot_sha256": &"c".repeat(64), "outputs": [{"output_id": "report-input", "format": "json", "content_type": "application/json", "sha256": &"d".repeat(64), "size_bytes": 42}]});
        assert!(NativeSyncRequest::publication("project-a", &request).is_ok());
    }
}
