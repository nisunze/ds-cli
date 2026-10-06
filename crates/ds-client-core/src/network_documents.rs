//! A project's own network documents (`docs/network_template`,
//! `docs/network_config`) copied under a reviewed plan: from another project
//! (a template project, selectively) or, for the migration only, from the
//! global `eds_networks` template the project resolves today. A platform
//! administrator's census pages every project. ds-brain owns the copy
//! (docs/contracts/project-network-documents.md); this module is its closed
//! client. The `/report` action is hard-coded here, outside the profile
//! digest, like `save_config`.
use crate::authorized::Authority;
use crate::profile::{ClientProfile, PROJECT_REPORT_PATH};
use crate::transport::{CallAuthority, SecretRequestBody, CORRELATION_HEADERS};
use crate::{AuthorizedTransport, ClientError, ErrorKind, ServiceRefusal, TransportResponse};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const NETWORK_DOCUMENTS_ACTION: &str = "network_documents";
pub const NETWORK_DOCUMENTS_RESPONSE_LIMIT: usize = 1024 * 1024;
pub const NETWORK_DOCUMENTS_PLAN_SCHEMA: &str = "ds.project-network-documents-plan/v1";
pub const NETWORK_DOCUMENTS_RECEIPT_SCHEMA: &str = "ds.project-network-documents-receipt/v1";
pub const NETWORK_DOCUMENTS_CENSUS_SCHEMA: &str = "ds.project-network-documents-census/v1";
pub const NETWORK_DOCUMENT_PARTS: [&str; 2] = ["network_template", "network_config"];
const NETWORK_DOCUMENTS_TIMEOUT_SECONDS: u64 = 120;
const MAX_CENSUS_PAGE: u32 = 25;

/// Where a copy comes from. `Global` is the migration source only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkDocumentsSource {
    /// Another project's documents; `parts` empty means every one it holds.
    Project { project: String },
    /// The global template: `template`, else the destination's selected one.
    Global {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        template: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkDocumentsRequest {
    /// The plan of one copy into the named project. Writes nothing.
    Plan {
        source: NetworkDocumentsSource,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parts: Vec<String>,
    },
    /// Apply exactly the reviewed plan.
    Apply {
        source: NetworkDocumentsSource,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parts: Vec<String>,
        expected_plan_sha256: String,
    },
    /// One page of every project: whether it owns the template it reads.
    Census {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
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

fn document_id(value: &str) -> bool {
    let mut chars = value.chars();
    value.len() <= 128
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

fn valid_copy(
    project: &str,
    source: &NetworkDocumentsSource,
    parts: &[String],
) -> Result<(), ClientError> {
    crate::tiles::validate_project(project)?;
    match source {
        NetworkDocumentsSource::Project { project: from } => {
            crate::tiles::validate_project(from)?;
            if from == project {
                return Err(invalid("A network document copy names another project"));
            }
        }
        NetworkDocumentsSource::Global { template } => {
            if template.as_deref().is_some_and(|t| !document_id(t)) {
                return Err(invalid("Invalid network template id"));
            }
            if parts.iter().any(|part| part != "network_template") {
                return Err(invalid("A global source holds only network_template"));
            }
        }
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
    Ok(())
}

impl NetworkDocumentsRequest {
    /// `project` is the destination; a census names none.
    pub fn validate_for(&self, project: &str) -> Result<(), ClientError> {
        match self {
            Self::Census { cursor, limit } => {
                if !project.is_empty() {
                    return Err(invalid(
                        "A network document census covers every project; it names none",
                    ));
                }
                if limit.is_some_and(|limit| limit == 0 || limit > MAX_CENSUS_PAGE) {
                    return Err(invalid("A network document census page holds 1-25 projects"));
                }
                if cursor.as_deref().is_some_and(|c| !document_id(c)) {
                    return Err(invalid("Invalid network document census cursor"));
                }
                Ok(())
            }
            Self::Plan { source, parts } => valid_copy(project, source, parts),
            Self::Apply {
                source,
                parts,
                expected_plan_sha256,
            } => {
                valid_copy(project, source, parts)?;
                if !digest(expected_plan_sha256) {
                    return Err(invalid("Apply names the reviewed plan's exact sha256"));
                }
                Ok(())
            }
        }
    }

    fn body(&self, project: &str) -> Vec<u8> {
        let mut body = json!({"action": NETWORK_DOCUMENTS_ACTION});
        let copy = |body: &mut Value, source: &NetworkDocumentsSource, parts: &[String]| {
            body["eds_project_id"] = json!(project);
            match source {
                NetworkDocumentsSource::Project { project } => {
                    body["source_project"] = json!(project)
                }
                NetworkDocumentsSource::Global {
                    template: Some(template),
                } => body["template_name"] = json!(template),
                NetworkDocumentsSource::Global { template: None } => {}
            }
            if !parts.is_empty() {
                body["parts"] = json!(parts);
            }
        };
        match self {
            Self::Plan { source, parts } => {
                body["operation"] = json!("plan");
                copy(&mut body, source, parts);
            }
            Self::Apply {
                source,
                parts,
                expected_plan_sha256,
            } => {
                body["operation"] = json!("apply");
                copy(&mut body, source, parts);
                body["expected_plan_sha256"] = json!(expected_plan_sha256);
            }
            Self::Census { cursor, limit } => {
                body["operation"] = json!("census");
                if let Some(cursor) = cursor {
                    body["cursor"] = json!(cursor);
                }
                if let Some(limit) = limit {
                    body["limit"] = json!(limit);
                }
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        PROJECT_REPORT_PATH
    }
    pub const fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        NETWORK_DOCUMENTS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        NETWORK_DOCUMENTS_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
    }
    pub fn ds_project(&self) -> &str {
        self.project
    }
    pub fn bearer_token(&self) -> &str {
        self.authority.bearer_token
    }
    pub fn canonical_email(&self) -> &str {
        self.authority.canonical_email
    }
    pub fn canonical_uid(&self) -> &str {
        self.authority.canonical_uid
    }
    pub fn device_id(&self) -> Option<&str> {
        self.authority.device_id
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
    let readable = match request {
        NetworkDocumentsRequest::Plan { .. } => {
            value["schema"] == NETWORK_DOCUMENTS_PLAN_SCHEMA
                && value["project_id"] == project
                && value["parts"].is_array()
                && value["plan_sha256"].as_str().is_some_and(digest)
        }
        NetworkDocumentsRequest::Apply {
            expected_plan_sha256,
            ..
        } => {
            value["schema"] == NETWORK_DOCUMENTS_RECEIPT_SCHEMA
                && value["project_id"] == project
                && value["plan_sha256"] == *expected_plan_sha256
                && value["written"].is_array()
        }
        NetworkDocumentsRequest::Census { .. } => {
            value["schema"] == NETWORK_DOCUMENTS_CENSUS_SCHEMA && value["projects"].is_array()
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

    fn template_copy(parts: Vec<String>) -> NetworkDocumentsRequest {
        NetworkDocumentsRequest::Plan {
            source: NetworkDocumentsSource::Project {
                project: "template_tchad".into(),
            },
            parts,
        }
    }

    #[test]
    fn requests_are_closed_and_name_their_destination() {
        let plan = template_copy(vec!["network_config".into()]);
        plan.validate_for("new_project").unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&plan.body("new_project")).unwrap(),
            json!({"action":"network_documents","operation":"plan","eds_project_id":"new_project",
                "source_project":"template_tchad","parts":["network_config"]})
        );
        let migration = NetworkDocumentsRequest::Apply {
            source: NetworkDocumentsSource::Global {
                template: Some("tchad".into()),
            },
            parts: Vec::new(),
            expected_plan_sha256: DIGEST.into(),
        };
        migration.validate_for("template_tchad").unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&migration.body("template_tchad")).unwrap(),
            json!({"action":"network_documents","operation":"apply","eds_project_id":"template_tchad",
                "template_name":"tchad","expected_plan_sha256":DIGEST})
        );
        let census = NetworkDocumentsRequest::Census {
            cursor: Some("arjgpydw_aderm_loc7".into()),
            limit: Some(25),
        };
        census.validate_for("").unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&census.body("")).unwrap(),
            json!({"action":"network_documents","operation":"census","cursor":"arjgpydw_aderm_loc7","limit":25})
        );

        for (request, project) in [
            (template_copy(Vec::new()), "template_tchad"),
            (template_copy(vec!["layers".into()]), "p"),
            (
                template_copy(vec!["network_config".into(), "network_config".into()]),
                "p",
            ),
            (
                NetworkDocumentsRequest::Plan {
                    source: NetworkDocumentsSource::Global { template: None },
                    parts: vec!["network_config".into()],
                },
                "p",
            ),
            (
                NetworkDocumentsRequest::Plan {
                    source: NetworkDocumentsSource::Global {
                        template: Some("../master".into()),
                    },
                    parts: Vec::new(),
                },
                "p",
            ),
            (
                NetworkDocumentsRequest::Census {
                    cursor: None,
                    limit: Some(26),
                },
                "",
            ),
            (
                NetworkDocumentsRequest::Census {
                    cursor: None,
                    limit: None,
                },
                "p",
            ),
            (
                NetworkDocumentsRequest::Apply {
                    source: NetworkDocumentsSource::Global { template: None },
                    parts: Vec::new(),
                    expected_plan_sha256: "not-a-digest".into(),
                },
                "p",
            ),
        ] {
            assert!(request.validate_for(project).is_err(), "{request:?} {project}");
        }
    }

    #[test]
    fn receipts_must_answer_what_was_asked() {
        let plan = template_copy(Vec::new());
        decode(
            response(200, json!({"schema":NETWORK_DOCUMENTS_PLAN_SCHEMA,"project_id":"p","parts":[],"plan_sha256":DIGEST})),
            "p",
            &plan,
        )
        .unwrap();
        assert!(decode(
            response(200, json!({"schema":NETWORK_DOCUMENTS_PLAN_SCHEMA,"project_id":"other","parts":[],"plan_sha256":DIGEST})),
            "p",
            &plan,
        )
        .is_err());
        let apply = NetworkDocumentsRequest::Apply {
            source: NetworkDocumentsSource::Project {
                project: "template_tchad".into(),
            },
            parts: Vec::new(),
            expected_plan_sha256: DIGEST.into(),
        };
        decode(
            response(200, json!({"schema":NETWORK_DOCUMENTS_RECEIPT_SCHEMA,"project_id":"p","plan_sha256":DIGEST,"written":["network_template"]})),
            "p",
            &apply,
        )
        .unwrap();
        assert!(decode(
            response(200, json!({"schema":NETWORK_DOCUMENTS_RECEIPT_SCHEMA,"project_id":"p","plan_sha256":"b".repeat(64),"written":[]})),
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
            response(404, json!({"detail":"eds_networks/gone","error_code":"network_documents_source_missing"})),
            "p",
            &plan,
        )
        .unwrap_err();
        assert_eq!(missing.kind(), ErrorKind::ResourceNotFound);
        let older = decode(response(400, json!({"detail":"Unknown action"})), "p", &plan).unwrap_err();
        assert_eq!(older.kind(), ErrorKind::RouteUnavailable);
    }
}
