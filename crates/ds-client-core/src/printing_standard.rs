//! Project-owned governed printing documents outside the custom layout library.
//! Every call captures one explicit project; neither the request nor transport
//! can borrow the saved active project or fall back to global records.
use crate::authorized::Authority;
use crate::profile::ClientProfile;
use crate::transport::{CallAuthority, SecretRequestBody, CORRELATION_HEADERS};
use crate::{AuthorizedTransport, ClientError, ErrorKind, ServiceRefusal, TransportResponse};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PRINTING_STANDARD_PATH: &str = "/api/v1/printing/standard";
pub const PRINTING_STANDARD_RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
pub const PRINTING_STANDARD_CATALOG_SCHEMA: &str = "ds.printing-standard-catalog/v1";
const PRINTING_STANDARD_TIMEOUT_SECONDS: u64 = 60;
const MAX_CATALOG_ROWS: usize = 72;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintingStandardKind {
    Standard,
    A4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrintingStandardRequest {
    List {},
    Get {
        kind: PrintingStandardKind,
        id: String,
    },
}

fn document_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

impl PrintingStandardRequest {
    pub fn validate(&self) -> Result<(), ClientError> {
        match self {
            Self::List {} => Ok(()),
            Self::Get { id, .. } if document_id(id) => Ok(()),
            Self::Get { .. } => Err(ClientError::new(
                ErrorKind::InvalidInput,
                "Invalid governed printing document id",
            )),
        }
    }
    pub(crate) fn body(&self, project: &str) -> Vec<u8> {
        let mut body = serde_json::to_value(self).expect("closed governed printing read");
        body["scope"] = serde_json::json!("project");
        body["project_id"] = serde_json::json!(project);
        serde_json::to_vec(&body).expect("closed project printing read")
    }
}

/// One authorized read. A transport can inspect it but cannot redirect it.
pub struct PrintingStandardCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) request: &'a PrintingStandardRequest,
    pub(crate) project: &'a str,
    pub(crate) authority: CallAuthority<'a>,
}

impl PrintingStandardCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        PRINTING_STANDARD_PATH
    }
    pub const fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        PRINTING_STANDARD_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        PRINTING_STANDARD_TIMEOUT_SECONDS
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
    pub fn project_id(&self) -> &str {
        self.project
    }
    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody::new(self.request.body(self.project))
    }
}

pub(crate) fn read<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    request: &PrintingStandardRequest,
) -> Result<Value, ClientError> {
    crate::authorized::validate_tiles(project)?;
    request.validate()?;
    let response = transport.printing_standard(PrintingStandardCall {
        profile,
        request,
        project,
        authority: authority.call(),
    })?;
    decode(response, project, request)
}

pub(crate) fn decode(
    response: TransportResponse,
    project: &str,
    request: &PrintingStandardRequest,
) -> Result<Value, ClientError> {
    if response.body.len() > PRINTING_STANDARD_RESPONSE_LIMIT {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "Governed printing response exceeds limit",
        ));
    }
    let parsed: Option<Value> = serde_json::from_slice(&response.body).ok();
    let value = match (response.status, parsed) {
        (200, Some(value)) if value["success"] == true => value,
        (200, _) => {
            return Err(ClientError::new(
                ErrorKind::UnreadableResponse,
                "Invalid governed printing response",
            ))
        }
        (status, parsed) => return Err(refusal(status, &parsed.unwrap_or(Value::Null))),
    };
    let data = &value["data"];
    if data["scope"] != "project" || data["project_id"] != project {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "Governed printing response differs from the captured project",
        ));
    }
    let readable = match request {
        PrintingStandardRequest::List {} => {
            data["schema"] == PRINTING_STANDARD_CATALOG_SCHEMA
                && data["documents"].as_array().is_some_and(|rows| {
                    rows.len() <= MAX_CATALOG_ROWS
                        && rows.iter().all(|row| {
                            matches!(row["kind"].as_str(), Some("standard" | "a4"))
                                && row["id"].as_str().is_some_and(document_id)
                        })
                })
        }
        PrintingStandardRequest::Get { kind, id } => {
            data["kind"] == serde_json::json!(kind)
                && data["id"] == *id
                && (*kind != PrintingStandardKind::A4
                    || data["document"]["schema"] == "ds.print-a4-document/v1"
                        && data["document"]["project_id"] == project)
        }
    };
    if !readable {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "Invalid governed printing document",
        ));
    }
    Ok(data.clone())
}

fn refusal(status: u16, value: &Value) -> ClientError {
    // ds-brain answers its own envelope; a gateway never taught this path
    // answers a bare 404, which says nothing about any document.
    if status == 404 && value.get("success").is_none() {
        return ClientError::new(
            ErrorKind::RouteUnavailable,
            "this deployment does not serve the governed printing read",
        );
    }
    let envelope = ServiceRefusal::new(
        status,
        value["error"]["code"].as_str(),
        value["error"]["message"]
            .as_str()
            .or_else(|| value["message"].as_str()),
    );
    let (kind, message) = match status {
        400 => (ErrorKind::InvalidInput, "Governed printing read refused"),
        401 | 403 => (
            ErrorKind::AuthenticationRejected,
            "Governed printing read rejected for this identity",
        ),
        404 => (
            ErrorKind::ResourceNotFound,
            "The governed printing document does not exist",
        ),
        429 | 500..=599 => (
            ErrorKind::Transient,
            "The printing service is temporarily unavailable",
        ),
        _ => (
            ErrorKind::UnreadableResponse,
            "Governed printing read refused by ds-brain",
        ),
    };
    if envelope.code().is_none() && envelope.message().is_none() {
        return ClientError::new(kind, message);
    }
    ClientError::with_service_refusal(kind, message, envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response(status: u16, body: Value) -> TransportResponse {
        TransportResponse::new(status, serde_json::to_vec(&body).unwrap())
    }

    #[test]
    fn requests_are_closed_and_ids_are_single_segments() {
        let get = PrintingStandardRequest::Get {
            kind: PrintingStandardKind::A4,
            id: "voltage-drop-a4-v1".into(),
        };
        assert_eq!(
            serde_json::from_slice::<Value>(&get.body("project_a")).unwrap(),
            json!({"action":"get","scope":"project","project_id":"project_a","kind":"a4","id":"voltage-drop-a4-v1"})
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&PrintingStandardRequest::List {}.body("project_a"))
                .unwrap(),
            json!({"action":"list","scope":"project","project_id":"project_a"})
        );
        for id in ["", "../defaults", "a/b", "-lead", &"x".repeat(129)] {
            let request = PrintingStandardRequest::Get {
                kind: PrintingStandardKind::Standard,
                id: id.into(),
            };
            assert!(request.validate().is_err(), "{id:?} was admitted");
        }
        assert!(serde_json::from_value::<PrintingStandardRequest>(
            json!({"action":"get","kind":"layout","id":"x"})
        )
        .is_err());
    }

    #[test]
    fn answers_must_name_what_was_asked() {
        let list = PrintingStandardRequest::List {};
        let catalog = json!({"success":true,"data":{"scope":"project","project_id":"project_a","schema":PRINTING_STANDARD_CATALOG_SCHEMA,
            "documents":[{"kind":"a4","id":"voltage-drop-a4-v1","bound":true}]}});
        assert_eq!(
            decode(response(200, catalog.clone()), "project_a", &list).unwrap(),
            catalog["data"]
        );
        for project in ["project_b", "", " project_a "] {
            assert!(decode(response(200, catalog.clone()), project, &list).is_err());
        }
        let mut global = catalog.clone();
        global["data"]["scope"] = json!("global");
        assert!(decode(response(200, global), "project_a", &list).is_err());
        let mut wrong = catalog.clone();
        wrong["data"]["documents"][0]["kind"] = json!("layout");
        assert!(decode(response(200, wrong), "project_a", &list).is_err());
        let get = PrintingStandardRequest::Get {
            kind: PrintingStandardKind::Standard,
            id: "lv-transformer-a0-v1-network".into(),
        };
        let other = json!({"success":true,"data":{"scope":"project","project_id":"project_a","kind":"standard","id":"lv-transformer-a3-v1-network"}});
        assert!(decode(response(200, other), "project_a", &get).is_err());
        let missing = decode(
            response(404, json!({"success":false,"error":{"code":"print_standard_not_found","message":"Governed printing document not found"}})),
            "project_a",
            &get,
        )
        .unwrap_err();
        assert_eq!(missing.kind(), ErrorKind::ResourceNotFound);
        let unrouted = decode(
            response(
                404,
                json!({"code":404,"message":"The current request is not defined by this API."}),
            ),
            "project_a",
            &get,
        )
        .unwrap_err();
        assert_eq!(unrouted.kind(), ErrorKind::RouteUnavailable);
    }
    #[test]
    fn a4_body_cannot_borrow_another_projects_identity() {
        let request = PrintingStandardRequest::Get {
            kind: PrintingStandardKind::A4,
            id: "voltage-drop-a4-v1".into(),
        };
        let body = json!({"success":true,"data":{"scope":"project","project_id":"project_a","kind":"a4","id":"voltage-drop-a4-v1","document":{"schema":"ds.print-a4-document/v1","project_id":"project_a"}}});
        assert!(decode(response(200, body.clone()), "project_a", &request).is_ok());
        let mut wrong = body.clone();
        wrong["data"]["document"]["project_id"] = json!("project_b");
        assert!(decode(response(200, wrong), "project_a", &request).is_err());
        assert!(decode(response(200, body), "project_b", &request).is_err());
    }
}
