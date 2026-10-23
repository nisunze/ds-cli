//! A project's own network documents (`docs/network_template`,
//! `docs/network_config`) copied from another project — a template project,
//! whole or one document at a time — under a reviewed plan. ds-brain owns the
//! copy (docs/contracts/project-network-documents.md); this module is its
//! closed client. The `/report` action is hard-coded here, outside the profile
//! digest, like `save_config`.
use crate::authorized::Authority;
use crate::profile::ClientProfile;
use crate::transport::{CallAuthority, SecretRequestBody};
use crate::{AuthorizedTransport, ClientError, ErrorKind, ServiceRefusal, TransportResponse};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const NETWORK_DOCUMENTS_ACTION: &str = "network_documents";
pub const NETWORK_DOCUMENTS_RESPONSE_LIMIT: usize = 1024 * 1024;
pub const NETWORK_DOCUMENTS_PLAN_SCHEMA: &str = "ds.project-network-documents-plan/v1";
pub const NETWORK_DOCUMENTS_RECEIPT_SCHEMA: &str = "ds.project-network-documents-receipt/v1";
pub const NETWORK_DOCUMENT_PARTS: [&str; 2] = ["network_template", "network_config"];
const NETWORK_DOCUMENTS_TIMEOUT_SECONDS: u64 = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkDocumentsRequest {
    /// The plan of one copy from `source_project` into the named project.
    /// Writes nothing. `parts` empty means every document the source holds.
    Plan {
        source_project: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parts: Vec<String>,
    },
    /// Apply exactly the reviewed plan.
    Apply {
        source_project: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parts: Vec<String>,
        expected_plan_sha256: String,
    },
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl NetworkDocumentsRequest {
    fn copy(&self) -> (&str, &[String]) {
        match self {
            Self::Plan {
                source_project,
                parts,
            }
            | Self::Apply {
                source_project,
                parts,
                ..
            } => (source_project, parts),
        }
    }

    /// `project` is the destination.
    pub fn validate_for(&self, project: &str) -> Result<(), ClientError> {
        let (source, parts) = self.copy();
        crate::tiles::validate_project(project)?;
        crate::tiles::validate_project(source)?;
        if source == project {
            return Err(invalid("A network document copy names another project"));
        }
        let mut seen = Vec::new();
        for part in parts {
            if !NETWORK_DOCUMENT_PARTS.contains(&part.as_str()) || seen.contains(&part) {
                return Err(invalid(
                    "Parts are distinct network_template or network_config",
                ));
            }
            seen.push(part);
        }
        if let Self::Apply {
            expected_plan_sha256,
            ..
        } = self
        {
            if !digest(expected_plan_sha256) {
                return Err(invalid("Apply names the reviewed plan's exact sha256"));
            }
        }
        Ok(())
    }

    fn body(&self, project: &str) -> Vec<u8> {
        let (source, parts) = self.copy();
        let mut body = json!({
            "action": NETWORK_DOCUMENTS_ACTION,
            "eds_project_id": project,
            "source_project": source,
        });
        if !parts.is_empty() {
            body["parts"] = json!(parts);
        }
        match self {
            Self::Plan { .. } => body["operation"] = json!("plan"),
            Self::Apply {
                expected_plan_sha256,
                ..
            } => {
                body["operation"] = json!("apply");
                body["expected_plan_sha256"] = json!(expected_plan_sha256);
            }
        }
        serde_json::to_vec(&body).expect("closed network document request")
    }
}

/// One authorized call. A transport can inspect it but cannot redirect it.
pub struct NetworkDocumentsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) request: &'a NetworkDocumentsRequest,
    pub(crate) project: &'a str,
    pub(crate) authority: CallAuthority<'a>,
}

impl NetworkDocumentsCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DsSystemReport;

    crate::gateway_compat::gateway_common_accessors! {
        gateway_origin: normal,
        content_type: const,
        correlation_headers: const,
        gateway_api_key: normal,
        client_id: const,
        bearer_token: normal,
        canonical_email: normal,
        canonical_uid: normal,
        device_id: normal,
    }
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        NETWORK_DOCUMENTS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        NETWORK_DOCUMENTS_TIMEOUT_SECONDS
    }

    pub fn ds_project(&self) -> &str {
        self.project
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody::new(self.request.body(self.project))
    }
}

pub(crate) fn call<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    request: &NetworkDocumentsRequest,
) -> Result<Value, ClientError> {
    request.validate_for(project)?;
    let response = transport.network_documents(NetworkDocumentsCall {
        profile,
        request,
        project,
        authority: authority.call(),
    })?;
    decode(response, project, request)
}

fn decode(
    response: TransportResponse,
    project: &str,
    request: &NetworkDocumentsRequest,
) -> Result<Value, ClientError> {
    if response.body.len() > NETWORK_DOCUMENTS_RESPONSE_LIMIT {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "Network document response exceeds limit",
        ));
    }
    let parsed: Option<Value> = serde_json::from_slice(&response.body).ok();
    let value = match (response.status, parsed) {
        (200, Some(value)) => value,
        (200, None) => {
            return Err(ClientError::new(
                ErrorKind::UnreadableResponse,
                "Invalid network document response",
            ))
        }
        (status, parsed) => return Err(refusal(status, &parsed.unwrap_or(Value::Null))),
    };
    // ds-brain answers the document itself on /report, without an envelope.
    let (source, _) = request.copy();
    let readable = value["project_id"] == project
        && value["source_project"] == source
        && match request {
            NetworkDocumentsRequest::Plan { .. } => {
                value["schema"] == NETWORK_DOCUMENTS_PLAN_SCHEMA
                    && value["parts"].is_array()
                    && value["plan_sha256"].as_str().is_some_and(digest)
            }
            NetworkDocumentsRequest::Apply {
                expected_plan_sha256,
                ..
            } => {
                value["schema"] == NETWORK_DOCUMENTS_RECEIPT_SCHEMA
                    && value["plan_sha256"] == *expected_plan_sha256
                    && value["written"].is_array()
            }
        };
    if !readable {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "Invalid network document receipt",
        ));
    }
    Ok(value)
}

fn refusal(status: u16, value: &Value) -> ClientError {
    let code = value["error_code"].as_str();
    // A /report deployment that predates the action proxies it to ds-system,
    // which knows nothing of it; that says nothing about any project.
    if code.is_none() && matches!(status, 400 | 404) {
        return ClientError::new(
            ErrorKind::RouteUnavailable,
            "this deployment does not serve network document copies",
        );
    }
    let envelope = ServiceRefusal::new(status, code, value["detail"].as_str());
    let (kind, message) = match status {
        400 => (ErrorKind::InvalidInput, "Network document request refused"),
        401 | 403 => (
            ErrorKind::AuthenticationRejected,
            "Network document copy rejected for this identity",
        ),
        404 => (
            ErrorKind::ResourceNotFound,
            "The project or its source document does not exist",
        ),
        409 => (
            ErrorKind::InvalidInput,
            "A network document changed since the plan; review the plan again",
        ),
        429 | 500..=599 => (
            ErrorKind::Transient,
            "Network document copies are temporarily unavailable",
        ),
        _ => (
            ErrorKind::UnreadableResponse,
            "Network document copy refused by ds-brain",
        ),
    };
    ClientError::with_service_refusal(kind, message, envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "3e417f9c61c328262ada243ed6e954ad680839866e830ddc86239a2bf8b1813d";

    fn response(status: u16, body: Value) -> TransportResponse {
        TransportResponse::new(status, serde_json::to_vec(&body).unwrap())
    }

    fn plan(parts: Vec<String>) -> NetworkDocumentsRequest {
        NetworkDocumentsRequest::Plan {
            source_project: "template_tchad".into(),
            parts,
        }
    }

    #[test]
    fn requests_are_closed_and_name_both_projects() {
        let request = plan(vec!["network_config".into()]);
        request.validate_for("new_project").unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&request.body("new_project")).unwrap(),
            json!({"action":"network_documents","operation":"plan","eds_project_id":"new_project",
                "source_project":"template_tchad","parts":["network_config"]})
        );
        let apply = NetworkDocumentsRequest::Apply {
            source_project: "template_tchad".into(),
            parts: Vec::new(),
            expected_plan_sha256: DIGEST.into(),
        };
        apply.validate_for("new_project").unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&apply.body("new_project")).unwrap(),
            json!({"action":"network_documents","operation":"apply","eds_project_id":"new_project",
                "source_project":"template_tchad","expected_plan_sha256":DIGEST})
        );

        for (request, project) in [
            (plan(Vec::new()), "template_tchad"),
            (plan(vec!["layers".into()]), "p"),
            (
                plan(vec!["network_config".into(), "network_config".into()]),
                "p",
            ),
            (
                NetworkDocumentsRequest::Apply {
                    source_project: "template_tchad".into(),
                    parts: Vec::new(),
                    expected_plan_sha256: "not-a-digest".into(),
                },
                "p",
            ),
        ] {
            assert!(
                request.validate_for(project).is_err(),
                "{request:?} {project}"
            );
        }
        assert!(
            serde_json::from_value::<NetworkDocumentsRequest>(json!({"operation":"census"}))
                .is_err()
        );
    }

    #[test]
    fn receipts_must_answer_what_was_asked() {
        let request = plan(Vec::new());
        decode(
            response(200, json!({"schema":NETWORK_DOCUMENTS_PLAN_SCHEMA,"project_id":"p","source_project":"template_tchad","parts":[],"plan_sha256":DIGEST})),
            "p",
            &request,
        )
        .unwrap();
        assert!(decode(
            response(200, json!({"schema":NETWORK_DOCUMENTS_PLAN_SCHEMA,"project_id":"p","source_project":"other","parts":[],"plan_sha256":DIGEST})),
            "p",
            &request,
        )
        .is_err());
        let apply = NetworkDocumentsRequest::Apply {
            source_project: "template_tchad".into(),
            parts: Vec::new(),
            expected_plan_sha256: DIGEST.into(),
        };
        decode(
            response(200, json!({"schema":NETWORK_DOCUMENTS_RECEIPT_SCHEMA,"project_id":"p","source_project":"template_tchad","plan_sha256":DIGEST,"written":["network_template"]})),
            "p",
            &apply,
        )
        .unwrap();
        assert!(decode(
            response(200, json!({"schema":NETWORK_DOCUMENTS_RECEIPT_SCHEMA,"project_id":"p","source_project":"template_tchad","plan_sha256":"b".repeat(64),"written":[]})),
            "p",
            &apply,
        )
        .is_err());

        let changed = decode(
            response(409, json!({"detail":"reviewed x, current y","error_code":"network_documents_plan_changed"})),
            "p",
            &apply,
        )
        .unwrap_err();
        assert_eq!(changed.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            changed.service_refusal().and_then(ServiceRefusal::code),
            Some("network_documents_plan_changed")
        );
        let missing = decode(
            response(404, json!({"detail":"t/network_template","error_code":"network_documents_source_missing"})),
            "p",
            &request,
        )
        .unwrap_err();
        assert_eq!(missing.kind(), ErrorKind::ResourceNotFound);
        let older = decode(
            response(400, json!({"detail":"Unknown action"})),
            "p",
            &request,
        )
        .unwrap_err();
        assert_eq!(older.kind(), ErrorKind::RouteUnavailable);
    }
}
