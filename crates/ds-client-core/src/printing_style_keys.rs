//! The governed printing-template migration from legacy `style_refs` to exact
//! `style_keys`: a read-only plan of one library (or one setup in it), a
//! platform administrator's read-only census page of every library, and the
//! apply of exactly a reviewed plan. ds-brain holds the setups and the
//! governed catalogue; ds-command-kernel decides every setup. The route is
//! hard-coded here, outside the profile digest, like every route added after
//! the profile's closed vocabulary.
use crate::authorized::Authority;
use crate::profile::ClientProfile;
use crate::transport::{CallAuthority, SecretRequestBody, CORRELATION_HEADERS};
use crate::{AuthorizedTransport, ClientError, ErrorKind, ServiceRefusal, TransportResponse};
use ds_command_kernel::printing::identifier;
use ds_command_kernel::style_resolution::{Ink, Key, Target};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const PRINT_STYLE_KEYS_PATH: &str = "/api/v1/printing/style_keys";
pub const PRINT_STYLE_KEYS_RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
pub const PRINT_STYLE_KEYS_PLAN_SCHEMA: &str = "ds.print-style-key-migration/v1";
pub const PRINT_STYLE_KEYS_RECEIPT_SCHEMA: &str = "ds.print-style-key-migration-receipt/v1";
pub const PRINT_STYLE_KEYS_CENSUS_SCHEMA: &str = "ds.print-style-key-census/v1";
const PRINT_STYLE_KEYS_TIMEOUT_SECONDS: u64 = 120;

/// An owner's exact governed key for a layer the Kernel named, per setup id.
pub type PrintStyleKeyChoices = BTreeMap<String, BTreeMap<String, Key>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrintStyleKeysRequest {
    /// The Kernel's plan for one library, or one setup in it. Writes nothing.
    Plan {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        choices: PrintStyleKeyChoices,
    },
    /// One page of the global library and every project library. Writes
    /// nothing; each library carries the digest its own apply checks.
    Census {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    },
    /// Apply exactly the reviewed plan; ds-brain keeps every replaced setup.
    Apply {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        choices: PrintStyleKeyChoices,
        expected_plan_sha256: String,
    },
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

fn valid_setup(id: &Option<String>) -> Result<(), ClientError> {
    match id {
        Some(id) if !identifier(id) => Err(invalid("Invalid print setup id")),
        _ => Ok(()),
    }
}

/// Bounded, and each choice names one exact colour print key; the Kernel
/// decides whether the catalogue declares it.
fn valid_choices(choices: &PrintStyleKeyChoices) -> Result<(), ClientError> {
    let refused = || {
        invalid("Choices name at most 200 setups, each with 1-100 layers and one exact colour print key")
    };
    if choices.len() > 200 {
        return Err(refused());
    }
    for (setup, layers) in choices {
        if !identifier(setup) || layers.is_empty() || layers.len() > 100 {
            return Err(refused());
        }
        for (layer, key) in layers {
            if !identifier(layer)
                || key.target != Target::Print
                || key.ink != Ink::Colour
                || key.validate().is_err()
            {
                return Err(refused());
            }
        }
    }
    Ok(())
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl PrintStyleKeysRequest {
    /// `project` is the library: empty for the global one. A census names none.
    pub fn validate_for(&self, project: &str) -> Result<(), ClientError> {
        match self {
            Self::Census { cursor, limit } => {
                if !project.is_empty() {
                    return Err(invalid(
                        "A template census covers every library; it names no project",
                    ));
                }
                if limit.is_some_and(|limit| limit == 0 || limit > 25) {
                    return Err(invalid("A template census page holds 1-25 libraries"));
                }
                if cursor
                    .as_ref()
                    .is_some_and(|cursor| cursor.is_empty() || cursor.len() > 512)
                {
                    return Err(invalid("Invalid template census cursor"));
                }
                Ok(())
            }
            Self::Plan { id, choices } => {
                if !project.is_empty() {
                    crate::tiles::validate_project(project)?;
                }
                valid_setup(id)?;
                valid_choices(choices)
            }
            Self::Apply {
                id,
                choices,
                expected_plan_sha256,
            } => {
                if !project.is_empty() {
                    crate::tiles::validate_project(project)?;
                }
                valid_setup(id)?;
                valid_choices(choices)?;
                if !digest(expected_plan_sha256) {
                    return Err(invalid("Apply names the reviewed plan's exact sha256"));
                }
                Ok(())
            }
        }
    }

    fn body(&self, project: &str) -> Vec<u8> {
        let mut value = serde_json::to_value(self).expect("closed template migration request");
        if !matches!(self, Self::Census { .. }) {
            value["scope"] = json!(if project.is_empty() {
                "global"
            } else {
                "project"
            });
            if !project.is_empty() {
                value["project_id"] = json!(project);
            }
        }
        serde_json::to_vec(&value).expect("closed template migration request")
    }
}

/// One authorized call. A transport can inspect it but cannot redirect it.
pub struct PrintStyleKeysCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) request: &'a PrintStyleKeysRequest,
    pub(crate) project: &'a str,
    pub(crate) authority: CallAuthority<'a>,
}

impl PrintStyleKeysCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        PRINT_STYLE_KEYS_PATH
    }
    pub const fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        PRINT_STYLE_KEYS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        PRINT_STYLE_KEYS_TIMEOUT_SECONDS
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
    request: &PrintStyleKeysRequest,
) -> Result<Value, ClientError> {
    request.validate_for(project)?;
    let response = transport.print_style_keys(PrintStyleKeysCall {
        profile,
        request,
        project,
        authority: authority.call(),
    })?;
    decode(response, request)
}

fn decode(
    response: TransportResponse,
    request: &PrintStyleKeysRequest,
) -> Result<Value, ClientError> {
    if response.body.len() > PRINT_STYLE_KEYS_RESPONSE_LIMIT {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "Template migration response exceeds limit",
        ));
    }
    let parsed: Option<Value> = serde_json::from_slice(&response.body).ok();
    let value = match (response.status, parsed) {
        (200, Some(value)) if value["success"] == true => value,
        (200, _) => {
            return Err(ClientError::new(
                ErrorKind::UnreadableResponse,
                "Invalid template migration response",
            ))
        }
        (status, parsed) => return Err(refusal(status, &parsed.unwrap_or(Value::Null))),
    };
    let data = &value["data"];
    let readable = match request {
        PrintStyleKeysRequest::Plan { .. } => {
            data["schema"] == PRINT_STYLE_KEYS_PLAN_SCHEMA
                && data["setups"].is_array()
                && data["plan_sha256"].as_str().is_some_and(digest)
        }
        PrintStyleKeysRequest::Census { .. } => {
            data["schema"] == PRINT_STYLE_KEYS_CENSUS_SCHEMA && data["libraries"].is_array()
        }
        PrintStyleKeysRequest::Apply {
            expected_plan_sha256,
            ..
        } => {
            data["schema"] == PRINT_STYLE_KEYS_RECEIPT_SCHEMA
                && data["plan_sha256"] == *expected_plan_sha256
                && data["migrated"].is_array()
        }
    };
    if !readable {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "Invalid template migration receipt",
        ));
    }
    Ok(data.clone())
}

fn refusal(status: u16, value: &Value) -> ClientError {
    // ds-brain answers its own envelope; a gateway that does not publish this
    // route answers a bare 404, which says nothing about any setup.
    if status == 404 && value.get("success").is_none() {
        return ClientError::new(
            ErrorKind::RouteUnavailable,
            "this deployment does not serve the printing template migration",
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
        400 => (
            ErrorKind::InvalidInput,
            "Template migration request refused",
        ),
        401 | 403 => (
            ErrorKind::AuthenticationRejected,
            "Template migration rejected for this identity",
        ),
        404 => (
            ErrorKind::ResourceNotFound,
            "The printing setup does not exist",
        ),
        409 => (
            ErrorKind::InvalidInput,
            "The reviewed plan or a setup changed; review the plan again",
        ),
        412 => (
            ErrorKind::InvalidInput,
            "The governed style catalogue is not seeded on this deployment",
        ),
        429 | 500..=599 => (
            ErrorKind::Transient,
            "The template migration is temporarily unavailable",
        ),
        _ => (
            ErrorKind::UnreadableResponse,
            "Template migration refused by ds-brain",
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

    const DIGEST: &str = "3e417f9c61c328262ada243ed6e954ad680839866e830ddc86239a2bf8b1813d";

    fn village_choice() -> PrintStyleKeyChoices {
        let key: Key = serde_json::from_value(json!({"entity_class":"village_boundaries",
            "source_kind":"cold_existing","target":"print","role":"project"}))
        .unwrap();
        BTreeMap::from([(
            "a0-landscape-gisagara-cjic".to_string(),
            BTreeMap::from([("village_boundaries".to_string(), key)]),
        )])
    }

    fn response(status: u16, body: Value) -> TransportResponse {
        TransportResponse::new(status, serde_json::to_vec(&body).unwrap())
    }

    #[test]
    fn requests_are_closed_and_name_their_library() {
        let plan = PrintStyleKeysRequest::Plan {
            id: Some("a0-landscape-gisagara-cjic".into()),
            choices: village_choice(),
        };
        plan.validate_for("czgmdwth_gisagara").unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&plan.body("czgmdwth_gisagara")).unwrap(),
            json!({"action":"plan","scope":"project","project_id":"czgmdwth_gisagara",
                "id":"a0-landscape-gisagara-cjic","choices":{"a0-landscape-gisagara-cjic":{"village_boundaries":
                {"entity_class":"village_boundaries","source_kind":"cold_existing","target":"print","role":"project"}}}})
        );
        let census = PrintStyleKeysRequest::Census {
            cursor: Some("eyJhZnRlciI6IiJ9".into()),
            limit: Some(10),
        };
        census.validate_for("").unwrap();
        assert!(census.validate_for("czgmdwth_gisagara").is_err());
        assert_eq!(
            serde_json::from_slice::<Value>(&census.body("")).unwrap(),
            json!({"action":"census","cursor":"eyJhZnRlciI6IiJ9","limit":10})
        );
        let apply = PrintStyleKeysRequest::Apply {
            id: None,
            choices: BTreeMap::new(),
            expected_plan_sha256: DIGEST.into(),
        };
        apply.validate_for("").unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&apply.body("")).unwrap(),
            json!({"action":"apply","scope":"global","expected_plan_sha256":DIGEST})
        );

        let mut monochrome = village_choice();
        monochrome
            .values_mut()
            .flat_map(|layers| layers.values_mut())
            .for_each(|key| key.ink = Ink::Monochrome);
        for (request, project) in [
            (
                PrintStyleKeysRequest::Census {
                    cursor: None,
                    limit: Some(26),
                },
                "",
            ),
            (
                PrintStyleKeysRequest::Plan {
                    id: Some("a/b".into()),
                    choices: BTreeMap::new(),
                },
                "czgmdwth_gisagara",
            ),
            (
                PrintStyleKeysRequest::Plan {
                    id: None,
                    choices: monochrome,
                },
                "czgmdwth_gisagara",
            ),
            (
                PrintStyleKeysRequest::Apply {
                    id: None,
                    choices: BTreeMap::new(),
                    expected_plan_sha256: "not-a-digest".into(),
                },
                "czgmdwth_gisagara",
            ),
        ] {
            assert!(request.validate_for(project).is_err(), "{request:?}");
        }
        assert!(serde_json::from_value::<PrintStyleKeysRequest>(
            json!({"action":"plan","all_projects":true})
        )
        .is_err());
    }

    #[test]
    fn receipts_must_answer_what_was_asked() {
        let ok = |data: Value| response(200, json!({"success":true,"data":data}));
        let plan = PrintStyleKeysRequest::Plan {
            id: None,
            choices: BTreeMap::new(),
        };
        decode(
            ok(json!({"schema":PRINT_STYLE_KEYS_PLAN_SCHEMA,"setups":[],"plan_sha256":DIGEST})),
            &plan,
        )
        .unwrap();
        assert!(decode(
            ok(json!({"schema":PRINT_STYLE_KEYS_PLAN_SCHEMA,"setups":[]})),
            &plan
        )
        .is_err());
        let census = PrintStyleKeysRequest::Census {
            cursor: None,
            limit: None,
        };
        decode(
            ok(json!({"schema":PRINT_STYLE_KEYS_CENSUS_SCHEMA,"libraries":[]})),
            &census,
        )
        .unwrap();
        let apply = PrintStyleKeysRequest::Apply {
            id: None,
            choices: BTreeMap::new(),
            expected_plan_sha256: DIGEST.into(),
        };
        decode(
            ok(json!({"schema":PRINT_STYLE_KEYS_RECEIPT_SCHEMA,"plan_sha256":DIGEST,"migrated":[]})),
            &apply,
        )
        .unwrap();
        assert!(decode(
            ok(json!({"schema":PRINT_STYLE_KEYS_RECEIPT_SCHEMA,"plan_sha256":"b".repeat(64),"migrated":[]})),
            &apply
        )
        .is_err());

        let changed = decode(
            response(409, json!({"success":false,"error":{"code":"print_style_keys_plan_changed","message":"review"}})),
            &apply,
        )
        .unwrap_err();
        assert_eq!(changed.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            changed.service_refusal().and_then(ServiceRefusal::code),
            Some("print_style_keys_plan_changed")
        );
        let unseeded = decode(
            response(412, json!({"success":false,"error":{"code":"print_style_catalogue_unseeded","message":"seed"}})),
            &plan,
        )
        .unwrap_err();
        assert_eq!(unseeded.kind(), ErrorKind::InvalidInput);
        let busy = decode(
            response(503, json!({"success":false,"error":{"code":"print_kernel_unavailable","message":"bridge"}})),
            &plan,
        )
        .unwrap_err();
        assert_eq!(busy.kind(), ErrorKind::Transient);
        let unrouted = decode(
            response(
                404,
                json!({"code":404,"message":"The current request is not defined by this API."}),
            ),
            &plan,
        )
        .unwrap_err();
        assert_eq!(unrouted.kind(), ErrorKind::RouteUnavailable);
    }
}
