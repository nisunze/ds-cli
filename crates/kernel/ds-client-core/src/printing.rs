//! Closed project print setup operations; global remains historical lineage. Layout validation is shared.
use crate::{ClientError, ErrorKind, ServiceRefusal};
use ds_command_kernel::printing::{identifier, validate, Layout};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const PRINTING_PATH: &str = ds_gateway_client::Operation::PrintingAction.path_template();
pub const PRINTING_METHOD: &str = ds_gateway_client::Operation::PrintingAction
    .method()
    .as_str();
pub const PRINTING_ACTIONS: [&str; 7] =
    ["list", "get", "create", "update", "save", "delete", "copy"];
pub const PRINTING_RESPONSE_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintingScope {
    Global,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintingSource {
    pub scope: PrintingScope,
    /// The project that holds a project-scoped source when it is not the
    /// request's own project: a copy between two projects, as a template's
    /// setups travel into the project made from it. ds-brain authorizes the
    /// source read and the destination write independently. Absent, a
    /// project source is the request's project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub id: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrintingDestination {
    pub scope: PrintingScope,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub expected_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrintingRequest {
    List {},
    Get {
        id: String,
    },
    Create {
        layout: Layout,
    },
    Update {
        layout: Layout,
        expected_revision: String,
    },
    /// Compatibility operation retained while older desktop releases migrate.
    Save {
        layout: Layout,
        expected_revision: String,
    },
    Delete {
        id: String,
        expected_revision: String,
    },
    Copy {
        source: PrintingSource,
        destination: PrintingDestination,
    },
}

impl PrintingRequest {
    pub fn validate(&self) -> Result<(), ClientError> {
        let invalid = |message: &'static str| ClientError::new(ErrorKind::InvalidInput, message);
        match self {
            Self::List {} => Ok(()),
            Self::Get { id } => valid_id(id),
            Self::Create { layout } => {
                validate(layout).map_err(|_| invalid("Invalid print layout"))
            }
            Self::Update {
                layout,
                expected_revision,
            } => {
                validate(layout).map_err(|_| invalid("Invalid print layout"))?;
                valid_revision(expected_revision, false)
            }
            Self::Save {
                layout,
                expected_revision,
            } => {
                validate(layout).map_err(|_| invalid("Invalid print layout"))?;
                valid_revision(expected_revision, true)
            }
            Self::Delete {
                id,
                expected_revision,
            } => {
                valid_id(id)?;
                valid_revision(expected_revision, false)
            }
            Self::Copy {
                source,
                destination,
            } => {
                valid_id(&source.id)?;
                valid_revision(&source.revision, false)?;
                valid_id(&destination.id)?;
                valid_revision(&destination.expected_revision, true)?;
                if let Some(project) = &source.project {
                    if source.scope != PrintingScope::Project
                        || destination.scope != PrintingScope::Project
                    {
                        return Err(invalid(
                            "A copy between projects names two project-scoped setups",
                        ));
                    }
                    crate::tiles::validate_project(project)?;
                } else if source.scope == destination.scope && source.id == destination.id {
                    return Err(invalid("Print copy destination must differ from source"));
                }
                if destination.name.as_ref().is_some_and(|name| {
                    let trimmed = name.trim();
                    trimmed.is_empty() || trimmed.len() > 160
                }) {
                    return Err(invalid("Invalid print setup name"));
                }
                Ok(())
            }
        }
    }

    pub fn needs_project(&self, direct_global: bool) -> bool {
        match self {
            Self::Copy {
                source,
                destination,
            } => {
                source.scope == PrintingScope::Project
                    || destination.scope == PrintingScope::Project
            }
            _ => !direct_global,
        }
    }

    /// A copy between projects must name another project than the one the
    /// request writes; the same project is the ordinary same-library copy.
    pub fn validate_for(&self, project: &str) -> Result<(), ClientError> {
        self.validate()?;
        crate::authorized::validate_tiles(project)?;
        if let Self::Copy {
            source,
            destination,
        } = self
        {
            if source.scope != PrintingScope::Project || destination.scope != PrintingScope::Project
            {
                return Err(ClientError::new(
                    ErrorKind::InvalidInput,
                    "Global printing runtime scope is retired; copy between named projects",
                ));
            }
        }
        if let Self::Copy {
            source:
                PrintingSource {
                    project: Some(source),
                    ..
                },
            ..
        } = self
        {
            if source == project {
                return Err(ClientError::new(
                    ErrorKind::InvalidInput,
                    "A copy between projects names a source project other than the destination",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn body(&self, project: &str) -> Vec<u8> {
        let mut value = serde_json::to_value(self).expect("validated print operation");
        if let Self::Copy {
            source,
            destination,
        } = self
        {
            // The source project is a location, written as the route reads it.
            if let Some(map) = value["source"].as_object_mut() {
                map.remove("project");
            }
            apply_location(
                &mut value["source"],
                source.scope,
                source.project.as_deref().unwrap_or(project),
            )
            .expect("project requirement checked before transport");
            apply_location(&mut value["destination"], destination.scope, project)
                .expect("project requirement checked before transport");
        } else {
            value["scope"] = json!(if project.is_empty() {
                "global"
            } else {
                "project"
            });
            if !project.is_empty() {
                value["project_id"] = json!(project);
            }
            match self {
                Self::Create { layout }
                | Self::Update { layout, .. }
                | Self::Save { layout, .. } => value["id"] = json!(layout.id),
                _ => {}
            }
        }
        serde_json::to_vec(&value).expect("closed validated printing request")
    }
}

fn apply_location(
    value: &mut Value,
    scope: PrintingScope,
    project: &str,
) -> Result<(), ClientError> {
    value["scope"] = json!(match scope {
        PrintingScope::Global => "global",
        PrintingScope::Project => "project",
    });
    if scope == PrintingScope::Project {
        if project.is_empty() {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "Selected project is required for printing",
            ));
        }
        value["project_id"] = json!(project);
    }
    Ok(())
}

fn valid_id(id: &str) -> Result<(), ClientError> {
    if identifier(id) {
        Ok(())
    } else {
        Err(ClientError::new(
            ErrorKind::InvalidInput,
            "Invalid print setup id",
        ))
    }
}

fn valid_revision(revision: &str, allow_empty: bool) -> Result<(), ClientError> {
    if allow_empty && revision.is_empty()
        || revision.len() == 64
            && revision
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(ClientError::new(
            ErrorKind::InvalidInput,
            "Invalid print revision",
        ))
    }
}

pub(crate) fn decode(
    response: crate::TransportResponse,
    request: &PrintingRequest,
) -> Result<Value, ClientError> {
    if response.body.len() > PRINTING_RESPONSE_LIMIT {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "Print response exceeds limit",
        ));
    }
    // A refused response is classified from its STATUS first: a deployment
    // that answers a refusal in a shape this client cannot parse is still a
    // refusal with a known class, not an unreadable success.
    let parsed: Option<Value> = serde_json::from_slice(&response.body).ok();
    let value = match (response.status, parsed) {
        (200, Some(value)) if value["success"] == true => value,
        (200, Some(_)) => return Err(refusal(200, &Value::Null)),
        (200, None) => {
            return Err(ClientError::new(
                ErrorKind::UnreadableResponse,
                "Invalid print response",
            ))
        }
        (status, parsed) => return Err(refusal(status, &parsed.unwrap_or(Value::Null))),
    };
    let data = &value["data"];
    let bad = || {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "Invalid printing setup receipt",
        )
    };
    match request {
        PrintingRequest::List {} => {
            let rows = data["setups"].as_array().ok_or_else(bad)?;
            if rows.len() > 200
                || rows
                    .iter()
                    .any(|r| !r["id"].as_str().is_some_and(identifier))
            {
                return Err(bad());
            }
        }
        PrintingRequest::Get { id } => validate_setup(data, id, None)?,
        PrintingRequest::Create { layout }
        | PrintingRequest::Update { layout, .. }
        | PrintingRequest::Save { layout, .. } => validate_setup(data, &layout.id, Some(layout))?,
        PrintingRequest::Delete {
            id,
            expected_revision,
        } => {
            if data["deleted"] != true
                || data["id"] != *id
                || data["revision"] != *expected_revision
            {
                return Err(bad());
            }
        }
        PrintingRequest::Copy { destination, .. } => {
            validate_setup(data, &destination.id, None)?;
            if let Some(name) = &destination.name {
                if data["name"] != *name {
                    return Err(bad());
                }
            }
        }
    }
    Ok(data.clone())
}

/// Classify one refused printing response.
///
/// The 2026-09-10 production failure is the reason this is not one line. The
/// deployed layout validator answered `422 unknown field \u{0060}composition\u{0060}`; ds-brain
/// collapsed it into an internal error; this decoder collapsed THAT into
/// `UnreadableResponse`; and `ds` rendered the result as
/// `auth_response_unreadable` — an authentication failure, for a document the
/// operator could have fixed in a minute. Every status a governed route
/// deliberately answers with is classified here, and only a body that cannot
/// be read at all is still unreadable.
fn refusal(status: u16, value: &Value) -> ClientError {
    let envelope = ServiceRefusal::new(status, envelope_code(value), envelope_message(value));
    let (kind, message) = match status {
        400 | 422 => (
            ErrorKind::InvalidInput,
            "Printing request refused: the layout is not acceptable to this deployment",
        ),
        401 | 403 => (
            ErrorKind::AuthenticationRejected,
            "Printing request rejected for this identity",
        ),
        404 => (
            ErrorKind::ResourceNotFound,
            "The printing setup does not exist",
        ),
        409 => (
            ErrorKind::InvalidInput,
            "Printing setup changed; reload before saving",
        ),
        // A 200 that says `success:false` is a contradiction, not a refusal a
        // caller can act on; so is any status outside the governed set.
        429 | 500..=599 => (
            ErrorKind::Transient,
            "The printing service is temporarily unavailable",
        ),
        _ => (
            ErrorKind::UnreadableResponse,
            "Printing request refused by ds-brain",
        ),
    };
    // Nothing readable in the body means nothing to say beyond the class.
    if envelope.code().is_none() && envelope.message().is_none() {
        return ClientError::new(kind, message);
    }
    ClientError::with_service_refusal(kind, message, envelope)
}

/// ds-brain's standard envelope names its code under `error.code`.
fn envelope_code(value: &Value) -> Option<&str> {
    value["error"]["code"].as_str()
}

/// The sentence, from the standard envelope or from the older bare shapes a
/// deployed service may still answer with. Reading the older shape is not
/// tolerance for drift: it is how a client deployed before the service can
/// still say something true about a refusal.
fn envelope_message(value: &Value) -> Option<&str> {
    value["error"]["message"]
        .as_str()
        .or_else(|| value["detail"].as_str())
        .or_else(|| value["error"].as_str())
        .or_else(|| value["message"].as_str())
}

fn validate_setup(data: &Value, id: &str, exact: Option<&Layout>) -> Result<(), ClientError> {
    let bad = || {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "Invalid printing setup receipt",
        )
    };
    let layout: Layout = serde_json::from_value(data["layout"].clone()).map_err(|_| bad())?;
    validate(&layout).map_err(|_| bad())?;
    if layout.id != id || data["id"] != id || exact.is_some_and(|wanted| &layout != wanted) {
        return Err(bad());
    }
    let revision = data["revision"].as_str().ok_or_else(bad)?;
    valid_revision(revision, false).map_err(|_| bad())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TransportResponse;

    const REVISION: &str = "3e417f9c61c328262ada243ed6e954ad680839866e830ddc86239a2bf8b1813d";

    fn between_projects(source_project: &str) -> PrintingRequest {
        PrintingRequest::Copy {
            source: PrintingSource {
                scope: PrintingScope::Project,
                project: Some(source_project.into()),
                id: "a3-landscape-gisagara-cjic".into(),
                revision: REVISION.into(),
            },
            destination: PrintingDestination {
                scope: PrintingScope::Project,
                id: "a3-landscape-gisagara-cjic".into(),
                name: None,
                expected_revision: String::new(),
            },
        }
    }

    /// A template's setups travel into the new project under the same id: the
    /// source names its own project, the request names the destination, and
    /// the wire carries each as the route reads a location.
    #[test]
    fn a_copy_between_projects_names_both_locations_on_the_wire() {
        let request = between_projects("czgmdwth_gisagara");
        request.validate_for("uid_nyaruguru_lv").unwrap();
        let body: Value = serde_json::from_slice(&request.body("uid_nyaruguru_lv")).unwrap();
        assert_eq!(
            body,
            json!({"action":"copy",
                "source":{"scope":"project","project_id":"czgmdwth_gisagara","id":"a3-landscape-gisagara-cjic","revision":REVISION},
                "destination":{"scope":"project","project_id":"uid_nyaruguru_lv","id":"a3-landscape-gisagara-cjic","expected_revision":""}})
        );
        // The same project is the same-library copy, which needs another id.
        assert!(request.validate_for("czgmdwth_gisagara").is_err());
        // A global location has no project of its own.
        let mut global = between_projects("czgmdwth_gisagara");
        if let PrintingRequest::Copy { source, .. } = &mut global {
            source.scope = PrintingScope::Global;
        }
        assert!(global.validate().is_err());
        // Without a source project the old rule stands: same scope, same id
        // is no copy.
        let mut same = between_projects("czgmdwth_gisagara");
        if let PrintingRequest::Copy { source, .. } = &mut same {
            source.project = None;
        }
        assert!(same.validate().is_err());
    }

    fn refused(status: u16, body: &str) -> ClientError {
        decode(
            TransportResponse::new(status, body.as_bytes().to_vec()),
            &PrintingRequest::List {},
        )
        .expect_err("a refused response never decodes as a receipt")
    }

    fn brain_envelope(code: &str, message: &str) -> String {
        format!(
            r#"{{"success":false,"service":"data-solutions-backend","version":"1.0.0","error":{{"code":"{code}","message":"{message}","details":{{"validator_status":422}},"timestamp":1}}}}"#
        )
    }

    /// The production defect, per status. Before this, everything that was not
    /// 200 or 409 was `UnreadableResponse`, which `ds` renders as
    /// `auth_response_unreadable` — an authentication failure for what is
    /// actually an authoring mistake or an out-of-date deployment.
    #[test]
    fn every_governed_status_classifies_and_only_an_unreadable_body_stays_unreadable() {
        let invalid = refused(
            400,
            &brain_envelope("print_layout_invalid", "unknown field `composition`"),
        );
        assert_eq!(invalid.kind(), ErrorKind::InvalidInput);
        let refusal = invalid.service_refusal().expect("the server named one");
        assert_eq!(refusal.status(), 400);
        assert_eq!(refusal.code(), Some("print_layout_invalid"));
        assert_eq!(refusal.message(), Some("unknown field `composition`"));

        // 422 is the exact status the deployed validator answered with.
        let unprocessable = refused(
            422,
            &brain_envelope("print_layout_invalid", "unknown field `composition`"),
        );
        assert_eq!(unprocessable.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            unprocessable
                .service_refusal()
                .and_then(ServiceRefusal::code),
            Some("print_layout_invalid")
        );

        for status in [401, 403] {
            assert_eq!(
                refused(status, &brain_envelope("unauthorized", "not permitted")).kind(),
                ErrorKind::AuthenticationRejected,
                "{status}"
            );
        }
        let missing = refused(
            404,
            &brain_envelope("print_setup_not_found", "Printing setup not found"),
        );
        assert_eq!(missing.kind(), ErrorKind::ResourceNotFound);
        assert_eq!(
            missing.service_refusal().and_then(ServiceRefusal::code),
            Some("print_setup_not_found"),
            "a missing setup must say what was missing"
        );
        let conflict = refused(409, &brain_envelope("conflict", "revision moved"));
        assert_eq!(conflict.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            conflict.to_string(),
            "Printing setup changed; reload before saving"
        );
        for status in [429, 500, 502, 503] {
            let transient = refused(
                status,
                &brain_envelope("print_validator_unavailable", "the validator answered 500"),
            );
            assert_eq!(transient.kind(), ErrorKind::Transient, "{status}");
            assert_eq!(
                transient.service_refusal().map(ServiceRefusal::status),
                Some(status)
            );
        }

        // Only a body this client cannot read at all is still unreadable, and
        // a status it does not govern keeps the coarse class.
        let unreadable = refused(418, "<html>teapot</html>");
        assert_eq!(unreadable.kind(), ErrorKind::UnreadableResponse);
        assert!(unreadable.service_refusal().is_none());
        assert_eq!(
            decode(
                TransportResponse::new(200, b"not json".to_vec()),
                &PrintingRequest::List {},
            )
            .expect_err("a 200 that is not JSON is unreadable")
            .kind(),
            ErrorKind::UnreadableResponse
        );
    }

    /// A refusal shaped by an older or plainer service still says something
    /// true, and a refused status whose body carries no sentence keeps the
    /// class without inventing one.
    #[test]
    fn older_envelope_shapes_are_read_and_an_empty_body_adds_nothing() {
        let older = refused(422, r#"{"detail":"unknown field `composition`"}"#);
        assert_eq!(older.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            older.service_refusal().and_then(ServiceRefusal::message),
            Some("unknown field `composition`")
        );
        let bare = refused(422, "{}");
        assert_eq!(bare.kind(), ErrorKind::InvalidInput);
        assert!(bare.service_refusal().is_none());
    }

    /// The sentence crosses; where the service runs does not, and neither does
    /// an unbounded body or a code that is not an identifier.
    #[test]
    fn a_carried_refusal_is_bounded_and_never_names_an_endpoint() {
        let leaky = refused(
            422,
            r#"{"success":false,"error":{"code":"PRINT LAYOUT INVALID","message":"refused by https://reporter-abc.a.run.app/v1/print-layout/validate now","timestamp":1}}"#,
        );
        let refusal = leaky.service_refusal().expect("a message survived");
        assert_eq!(refusal.code(), None, "a non-identifier code is dropped");
        let message = refusal.message().expect("the sentence survived");
        assert!(!message.contains("run.app"), "{message}");
        assert!(message.contains("[address removed]"), "{message}");

        let long = "é".repeat(500);
        let refused = refused(
            422,
            &format!(
                r#"{{"success":false,"error":{{"code":"print_layout_invalid","message":"{long}","timestamp":1}}}}"#
            ),
        );
        let message = refused
            .service_refusal()
            .and_then(ServiceRefusal::message)
            .expect("a bounded sentence");
        assert!(
            message.len() <= ServiceRefusal::MAX_MESSAGE_BYTES + 3,
            "{}",
            message.len()
        );
        assert!(message.ends_with('…'));
    }
}
