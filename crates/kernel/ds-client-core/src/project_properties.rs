//! Creating a project and editing its properties, headlessly.
//!
//! Until now a project was created and named from exactly one place: the
//! Projects page in a browser. `survey project create-from-template` could
//! stand a project up on a Server, but it left `project_name` empty and no
//! command could set the display name, country, client, description or
//! location afterwards — which is why the two integration test projects have
//! no name, and why ds-brain cannot seal a report input receipt for them.
//!
//! Nothing about these writes needs a window. `POST /api/v1/projects` is
//! published on both lanes with `create` and `update` in its action
//! vocabulary, ds-brain's `Authentication` middleware admits a restored user
//! and a device credential alike, and `project.create` /
//! `project.properties.edit` on the principal decide the answer.
//!
//! **This module is the DOOR, not the decisions.** It sends a closed pair of
//! actions and returns what the server said. The project document's defaults
//! — the network template, the CRS for a country, the phases and components —
//! are ds-brain's (`handleCreateProject`), and this owner does not restate
//! them.
//!
//! The path is written here rather than declared in the client profile, like
//! `/api/v1/pm` and `/api/v1/governance` beside it: declaring a route there
//! would bump `CLIENT_PROFILE_SCHEMA` and strand every installed `ds` until a
//! package is cut, for nothing this needs.
//!
//! Closed by construction: the [`Command`] enum is the complete set of
//! actions this owner can send, and it writes only the keys the handler reads
//! by name. The five property fields are the ones the browser's Edit Project
//! form edits. Two narrower doors exist for templates and nothing else:
//! [`Command::CreateFromTemplate`] carries a template plan's settings blocks
//! (project type and CRS, components, phases, lanes) into `create`, and
//! [`Command::SetTemplate`] flips the one lifecycle flag a template is. The
//! archive, testing and expiration flags stay out.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    ProjectPropertiesCall, ProjectPropertiesServiceCode, ServiceRefusal,
};

/// The largest receipt this owner will read: one project document's summary.
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;
/// The longest display name, country, client, location or template id.
pub const MAX_TEXT_CHARS: usize = 200;
/// The longest description.
pub const MAX_DESCRIPTION_CHARS: usize = 2000;

/// One governed project-properties action.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Properties {
    pub display_name: Option<String>,
    pub location: Option<String>,
    pub country: Option<String>,
    pub client: Option<String>,
    pub description: Option<String>,
}

impl Properties {
    fn is_empty(&self) -> bool {
        self.display_name.is_none()
            && self.location.is_none()
            && self.country.is_none()
            && self.client.is_none()
            && self.description.is_none()
    }

    fn valid(&self) -> bool {
        optional(&self.display_name, MAX_TEXT_CHARS, false)
            && optional(&self.location, MAX_TEXT_CHARS, false)
            && optional(&self.country, MAX_TEXT_CHARS, false)
            && optional(&self.client, MAX_TEXT_CHARS, false)
            && optional(&self.description, MAX_DESCRIPTION_CHARS, true)
    }

    fn write(&self, data: &mut BTreeMap<String, Value>) {
        for (key, value) in [
            ("display_name", &self.display_name),
            ("project_location", &self.location),
            ("country", &self.country),
            ("client", &self.client),
            ("description", &self.description),
        ] {
            if let Some(value) = value {
                data.insert(key.to_owned(), Value::String(value.clone()));
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Create one project. The display name is required; the `project_name`
    /// slug the document id is built from is derived from it exactly as the
    /// browser derives it, so the two hosts create the same project from the
    /// same name. Every other field is optional and, when absent, is
    /// ds-brain's default.
    Create {
        display_name: String,
        properties: Properties,
        network_template: Option<String>,
        styling_template: Option<String>,
    },
    /// Edit one or more properties of one project. At least one property
    /// must be named: an update that changes nothing is a request the
    /// operator did not mean.
    Update {
        project_id: String,
        properties: Properties,
    },
    /// Create the project a template plan names
    /// (`ds_command_kernel::project_template`): the operator's identity over
    /// the template's settings blocks, sent through the same `create` action.
    CreateFromTemplate {
        project: ds_command_kernel::project_template::NewProject,
    },
    /// Mark one project template, or take it back to its other state. A
    /// lifecycle-only update: ds-brain admits it on a lifecycle capability
    /// (`project.template.mark`) or `project.properties.edit`.
    SetTemplate { project_id: String, template: bool },
}

/// The settings blocks a template carries into the creation payload, closed.
pub const TEMPLATE_SETTINGS: [&str; 4] = [
    "project_params",
    "project_components",
    "project_phases",
    "processing_lanes",
];
/// The largest settings block the creation payload carries.
pub const MAX_SETTINGS_BYTES: usize = 8192;

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the project properties response violates its envelope contract",
    )
}

/// Non-empty, trimmed, bounded, and free of control characters — with a
/// newline and a tab admitted only where multi-line text is expected.
fn text(value: &str, max: usize, multiline: bool) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= max
        && !value
            .chars()
            .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\t')))
}

fn optional(value: &Option<String>, max: usize, multiline: bool) -> bool {
    value
        .as_ref()
        .is_none_or(|value| text(value, max, multiline))
}

/// The `project_name` slug for a display name: lowercase, every run of
/// characters outside `a-z0-9` collapsed to one underscore, nothing leading or
/// trailing. The browser's `slugify` (`routes/projects/+page.svelte`), so a
/// project created here and one created there from the same name get the
/// same document id.
pub fn slug(display_name: &str) -> String {
    let mut out = String::with_capacity(display_name.len());
    for c in display_name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_owned()
}

impl Command {
    pub fn action(&self) -> &'static str {
        match self {
            Self::Create { .. } | Self::CreateFromTemplate { .. } => "create",
            Self::Update { .. } | Self::SetTemplate { .. } => "update",
        }
    }

    pub fn validate(&self) -> Result<(), ClientError> {
        match self {
            Self::Create {
                display_name,
                properties,
                network_template,
                styling_template,
            } => {
                if !text(display_name, MAX_TEXT_CHARS, false) {
                    return Err(invalid(
                        "a project needs a display name: non-empty, trimmed, at most 200 characters",
                    ));
                }
                if slug(display_name).is_empty() {
                    return Err(invalid(
                        "the display name yields no project_name slug; it needs at least one letter or digit",
                    ));
                }
                if properties.display_name.is_some() {
                    return Err(invalid(
                        "create names the project once, through its display name",
                    ));
                }
                if !properties.valid()
                    || !optional(network_template, MAX_TEXT_CHARS, false)
                    || !optional(styling_template, MAX_TEXT_CHARS, false)
                    || styling_template
                        .as_ref()
                        .is_some_and(|id| id.contains('/') || id == "." || id == "..")
                {
                    return Err(invalid(
                        "a project property is empty, untrimmed, outside its bound, or carries a control character",
                    ));
                }
            }
            Self::Update {
                project_id,
                properties,
            } => {
                crate::tiles::validate_project(project_id)?;
                if properties.is_empty() {
                    return Err(invalid("an update names at least one property to change"));
                }
                if !properties.valid() {
                    return Err(invalid(
                        "a project property is empty, untrimmed, outside its bound, or carries a control character",
                    ));
                }
            }
            Self::CreateFromTemplate { project } => {
                if !text(&project.display_name, MAX_TEXT_CHARS, false)
                    || slug(&project.display_name).is_empty()
                    || project.project_name != slug(&project.display_name)
                {
                    return Err(invalid(
                        "a template plan's project needs a display name whose slug is its project_name",
                    ));
                }
                let properties = Properties {
                    display_name: None,
                    location: project.location.clone(),
                    country: project.country.clone(),
                    client: project.client.clone(),
                    description: project.description.clone(),
                };
                if !properties.valid()
                    || !optional(&project.network_template, MAX_TEXT_CHARS, false)
                    || !optional(&project.styling_template_id, MAX_TEXT_CHARS, false)
                    || project
                        .styling_template_id
                        .as_ref()
                        .is_some_and(|id| id.contains('/') || id == "." || id == "..")
                {
                    return Err(invalid(
                        "a project property is empty, untrimmed, outside its bound, or carries a control character",
                    ));
                }
                for (key, value) in &project.settings {
                    if !TEMPLATE_SETTINGS.contains(&key.as_str())
                        || !value.is_object()
                        || serde_json::to_vec(value).map_or(usize::MAX, |b| b.len())
                            > MAX_SETTINGS_BYTES
                    {
                        return Err(invalid(
                            "a template settings block is unknown, not an object, or over 8 KiB",
                        ));
                    }
                }
            }
            Self::SetTemplate { project_id, .. } => {
                crate::tiles::validate_project(project_id)?;
            }
        }
        Ok(())
    }

    /// The exact bytes `POST /api/v1/projects` receives.
    pub fn body_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&self.payload()).expect("closed project properties request")
    }

    fn payload(&self) -> Value {
        // Sorted keys, so the bytes are the same whichever serde_json feature
        // set the host built with.
        let mut data = BTreeMap::new();
        match self {
            Self::Create {
                display_name,
                properties,
                network_template,
                styling_template,
            } => {
                data.insert("display_name".into(), Value::String(display_name.clone()));
                data.insert("project_name".into(), Value::String(slug(display_name)));
                properties.write(&mut data);
                if let Some(template) = network_template {
                    data.insert("network_template".into(), Value::String(template.clone()));
                }
                if let Some(template) = styling_template {
                    data.insert(
                        "styling_template_id".into(),
                        Value::String(template.clone()),
                    );
                }
            }
            Self::Update {
                project_id,
                properties,
            } => {
                data.insert("project_id".into(), Value::String(project_id.clone()));
                properties.write(&mut data);
            }
            Self::CreateFromTemplate { project } => {
                data.insert(
                    "display_name".into(),
                    Value::String(project.display_name.clone()),
                );
                data.insert(
                    "project_name".into(),
                    Value::String(project.project_name.clone()),
                );
                Properties {
                    display_name: None,
                    location: project.location.clone(),
                    country: project.country.clone(),
                    client: project.client.clone(),
                    description: project.description.clone(),
                }
                .write(&mut data);
                for (key, value) in [
                    ("network_template", &project.network_template),
                    ("styling_template_id", &project.styling_template_id),
                ] {
                    if let Some(value) = value {
                        data.insert(key.into(), Value::String(value.clone()));
                    }
                }
                for (key, value) in &project.settings {
                    // Sorted at every depth: a block keeps whatever order the
                    // serde_json feature set read it in, and the bytes may not.
                    let mut value = value.clone();
                    value.sort_all_objects();
                    data.insert(key.clone(), value);
                }
            }
            Self::SetTemplate {
                project_id,
                template,
            } => {
                data.insert("project_id".into(), Value::String(project_id.clone()));
                data.insert("is_template".into(), Value::Bool(*template));
            }
        }
        json!({ "action": self.action(), "data": data })
    }
}

/// What `create` answered: the project ds-brain stood up.
///
/// The five scalar identities are this owner's; the parameter blocks
/// (`project_params`, `project_phases`, `project_components`,
/// `processing_lanes`, `crs`) are ds-brain's own documents, carried whole and
/// unread because their meaning belongs to the server that defaulted them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateReceipt {
    pub project_id: String,
    pub project_name: String,
    pub display_name: String,
    #[serde(default)]
    pub project_location: String,
    pub network_template: String,
    #[serde(default)]
    pub styling_template_id: String,
    #[serde(default)]
    pub project_type: String,
    #[serde(default)]
    pub project_params: Value,
    #[serde(default)]
    pub project_phases: Value,
    #[serde(default)]
    pub project_components: Value,
    #[serde(default)]
    pub processing_lanes: Value,
    #[serde(default)]
    pub crs: Value,
}

/// What `update` answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateReceipt {
    pub project_id: String,
    #[serde(default)]
    pub resources_ensured: bool,
    #[serde(default)]
    pub dataset_exists: bool,
    #[serde(default)]
    pub fields_updated: u64,
}

/// The receipt for whichever action was sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Receipt {
    Created(Box<CreateReceipt>),
    Updated(UpdateReceipt),
}

/// ds-brain's standard success envelope (`pkg/response.APIResponse`), closed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    success: bool,
    #[serde(default)]
    message: String,
    #[serde(default = "Option::default")]
    data: Option<T>,
    #[serde(default)]
    error: Option<Value>,
    #[serde(default)]
    timestamp: String,
    #[serde(default)]
    service: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    metadata: Option<Value>,
    #[serde(default)]
    request_id: String,
}

fn decode_success<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, ClientError> {
    let envelope: Envelope<T> = serde_json::from_slice(body).map_err(|_| unreadable())?;
    let _ = (
        &envelope.message,
        &envelope.error,
        &envelope.timestamp,
        &envelope.service,
        &envelope.version,
        &envelope.metadata,
        &envelope.request_id,
    );
    if !envelope.success {
        return Err(unreadable());
    }
    envelope.data.ok_or_else(unreadable)
}

pub(crate) fn decode_receipt(command: &Command, body: &[u8]) -> Result<Receipt, ClientError> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(unreadable());
    }
    match command {
        Command::Create { .. } | Command::CreateFromTemplate { .. } => {
            let receipt: CreateReceipt = decode_success(body)?;
            if receipt.project_id.is_empty() || receipt.project_name.is_empty() {
                return Err(unreadable());
            }
            Ok(Receipt::Created(Box::new(receipt)))
        }
        Command::Update { project_id, .. } | Command::SetTemplate { project_id, .. } => {
            let receipt: UpdateReceipt = decode_success(body)?;
            if receipt.project_id != *project_id {
                return Err(unreadable());
            }
            Ok(Receipt::Updated(receipt))
        }
    }
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    #[serde(default)]
    error: Option<ErrorWire>,
}

#[derive(Deserialize)]
struct ErrorWire {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

/// Classify one refusal. The route's own envelope code is matched exactly
/// against its status (`handlers_unified.go`, `lifecycle_gate_helper.go`,
/// `pkg/response`); anything else falls back to the status alone. The
/// server's sentence crosses as a bounded [`ServiceRefusal`] because for a
/// `403` it is the one line that names WHICH capability a project admin has
/// to grant.
pub(crate) fn refusal(status: u16, body: &[u8]) -> ClientError {
    let wire = if body.len() <= MAX_RESPONSE_BYTES {
        serde_json::from_slice::<ErrorEnvelope>(body)
            .ok()
            .and_then(|envelope| envelope.error)
    } else {
        None
    };
    let (code, sentence) = wire
        .map(|error| (error.code, error.message))
        .unwrap_or_default();
    let typed = match (status, code.as_str()) {
        (400, "VALIDATION_FAILED") => Some((
            ProjectPropertiesServiceCode::Invalid,
            ErrorKind::InvalidInput,
            "the project properties were refused by the server's own validation",
        )),
        (403, "INSUFFICIENT_PERMISSIONS") => Some((
            ProjectPropertiesServiceCode::CapabilityMissing,
            ErrorKind::AuthenticationRejected,
            "the verified user lacks the project capability this action needs",
        )),
        (403, "PROJECT_ARCHIVED" | "PROJECT_EXPIRED") => Some((
            ProjectPropertiesServiceCode::ProjectReadOnly,
            ErrorKind::AuthenticationRejected,
            "the project is read-only; its properties cannot change until it is active again",
        )),
        (404, "PROJECT_NOT_FOUND") => Some((
            ProjectPropertiesServiceCode::ProjectNotFound,
            ErrorKind::ResourceNotFound,
            "no project carries this id on this lane",
        )),
        _ => None,
    };
    if let Some((typed, kind, message)) = typed {
        let lowered = code.to_ascii_lowercase();
        let envelope = ServiceRefusal::new(status, Some(&lowered), Some(&sentence));
        let carried =
            (envelope.code().is_some() || envelope.message().is_some()).then_some(envelope);
        return ClientError::with_project_properties_service_code(kind, message, typed, carried);
    }
    match status {
        404 => ClientError::new(ErrorKind::ResourceNotFound, "no such project"),
        400 | 409 => invalid("the project properties request was refused"),
        408 | 425 | 429 | 500..=599 => ClientError::new(
            ErrorKind::Transient,
            "project properties are temporarily unavailable",
        ),
        _ => ClientError::new(
            ErrorKind::AuthenticationRejected,
            "the project properties route refused the verified user for this action",
        ),
    }
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    command: &Command,
) -> Result<Receipt, ClientError> {
    command.validate()?;
    let response = transport.project_properties(ProjectPropertiesCall {
        profile,
        authority: authority.call(),
        payload: command.payload(),
    })?;
    if response.status != 200 {
        return Err(refusal(response.status, &response.body));
    }
    decode_receipt(command, &response.body)
}

/// Lifecycle facts shared by native project readers and the browser's WASM.
/// Support contacts are supplied by the project, never installation constants.
pub fn lifecycle_projection(project: &serde_json::Value, now_ms: i64) -> serde_json::Value {
    ds_command_kernel::project_lifecycle::project(project, now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create(display_name: &str) -> Command {
        Command::Create {
            display_name: display_name.into(),
            properties: Properties {
                country: Some("Rwanda".into()),
                client: Some("EDCL".into()),
                ..Properties::default()
            },
            network_template: None,
            styling_template: None,
        }
    }

    fn update(project_id: &str, properties: Properties) -> Command {
        Command::Update {
            project_id: project_id.into(),
            properties,
        }
    }

    #[test]
    fn the_slug_is_the_browsers() {
        assert_eq!(slug("Integration test — Rwanda"), "integration_test_rwanda");
        assert_eq!(slug("  A  Test Project "), "a_test_project");
        assert_eq!(slug("Kigali-2026/LV"), "kigali_2026_lv");
        assert_eq!(slug("———"), "");
    }

    /// The request bodies are exact: only the keys the handler reads by name,
    /// only when they were given, and the slug the browser would send.
    #[test]
    fn create_and_update_send_exactly_the_keys_the_handler_reads() {
        let create = create("Integration test — Rwanda");
        create.validate().expect("valid create");
        assert_eq!(
            String::from_utf8(create.body_bytes()).unwrap(),
            r#"{"action":"create","data":{"client":"EDCL","country":"Rwanda","display_name":"Integration test — Rwanda","project_name":"integration_test_rwanda"}}"#
        );
        let full = Command::Create {
            display_name: "Gisagara LV".into(),
            properties: Properties {
                display_name: None,
                location: Some("Gisagara".into()),
                country: Some("Rwanda".into()),
                client: Some("EDCL".into()),
                description: Some("LV densification\nphase I".into()),
            },
            network_template: Some("master".into()),
            styling_template: Some("edcl-2026".into()),
        };
        full.validate().expect("valid create");
        assert_eq!(
            String::from_utf8(full.body_bytes()).unwrap(),
            r#"{"action":"create","data":{"client":"EDCL","country":"Rwanda","description":"LV densification\nphase I","display_name":"Gisagara LV","network_template":"master","project_location":"Gisagara","project_name":"gisagara_lv","styling_template_id":"edcl-2026"}}"#
        );
        let update = update(
            "it_rwanda",
            Properties {
                display_name: Some("Integration test — Rwanda".into()),
                country: Some("Rwanda".into()),
                ..Properties::default()
            },
        );
        update.validate().expect("valid update");
        assert_eq!(
            String::from_utf8(update.body_bytes()).unwrap(),
            r#"{"action":"update","data":{"country":"Rwanda","display_name":"Integration test — Rwanda","project_id":"it_rwanda"}}"#
        );
    }

    #[test]
    fn a_create_without_a_usable_name_and_an_update_that_changes_nothing_never_leave() {
        for name in ["", "   ", "———", " padded "] {
            let error = create(name).validate().expect_err(name);
            assert_eq!(error.kind(), ErrorKind::InvalidInput, "{name:?}");
        }
        let error = update("it_rwanda", Properties::default()).expect_err_validate();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        let error = update(
            "",
            Properties {
                country: Some("Rwanda".into()),
                ..Properties::default()
            },
        )
        .expect_err_validate();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        // Bounds hold on every property, on both actions.
        let long = "x".repeat(MAX_TEXT_CHARS + 1);
        assert!(update(
            "it_rwanda",
            Properties {
                client: Some(long.clone()),
                ..Properties::default()
            }
        )
        .validate()
        .is_err());
        assert!(update(
            "it_rwanda",
            Properties {
                description: Some("tab\tand newline\nare fine".into()),
                ..Properties::default()
            }
        )
        .validate()
        .is_ok());
        assert!(update(
            "it_rwanda",
            Properties {
                country: Some("no\ncontrol".into()),
                ..Properties::default()
            }
        )
        .validate()
        .is_err());
        assert!(Command::Create {
            display_name: "ok".into(),
            properties: Properties::default(),
            network_template: None,
            styling_template: Some("../escape".into()),
        }
        .validate()
        .is_err());
        // A create names the project once.
        assert!(Command::Create {
            display_name: "ok".into(),
            properties: Properties {
                display_name: Some("twice".into()),
                ..Properties::default()
            },
            network_template: None,
            styling_template: None,
        }
        .validate()
        .is_err());
    }

    trait ExpectErr {
        fn expect_err_validate(self) -> ClientError;
    }
    impl ExpectErr for Command {
        fn expect_err_validate(self) -> ClientError {
            self.validate().expect_err("refused")
        }
    }

    /// The real success envelopes, per action, as `pkg/response.Success`
    /// writes them around `CreateProjectRes` / `UpdateProjectRes`.
    #[test]
    fn each_real_shaped_success_body_decodes_to_its_receipt() {
        let created = json!({
            "success": true,
            "message": "Project created successfully",
            "data": {
                "project_id": "nisunze_gisagara_lv",
                "project_name": "gisagara_lv",
                "display_name": "Gisagara LV",
                "project_location": "Gisagara",
                "network_template": "master",
                "project_params": {"project_type": "new_build", "crs": {"mode": "tm_rwanda"}},
                "project_phases": {"survey": ["I"], "design": ["I"]},
                "project_components": {"survey": true, "design": true, "solar": false},
                "processing_lanes": {"fast": true},
                "project_type": "new_build",
                "crs": {"mode": "tm_rwanda"}
            },
            "timestamp": "2026-09-20T10:00:00Z",
            "service": "data-solutions-backend",
            "version": "1.0.0",
            "request_id": "req-1"
        });
        let receipt = decode_receipt(
            &create("Gisagara LV"),
            &serde_json::to_vec(&created).unwrap(),
        )
        .expect("created");
        match receipt {
            Receipt::Created(receipt) => {
                assert_eq!(receipt.project_id, "nisunze_gisagara_lv");
                assert_eq!(receipt.project_name, "gisagara_lv");
                assert_eq!(receipt.display_name, "Gisagara LV");
                assert_eq!(receipt.network_template, "master");
                assert_eq!(receipt.styling_template_id, "");
                assert_eq!(receipt.project_type, "new_build");
                assert_eq!(receipt.crs["mode"], "tm_rwanda");
            }
            Receipt::Updated(_) => panic!("create answers a create receipt"),
        }

        let updated = json!({
            "success": true,
            "message": "Project updated successfully",
            "data": {
                "project_id": "it_rwanda",
                "resources_ensured": true,
                "dataset_exists": true,
                "fields_updated": 2
            },
            "timestamp": "2026-09-20T10:00:00Z",
            "service": "data-solutions-backend",
            "version": "1.0.0"
        });
        let command = update(
            "it_rwanda",
            Properties {
                country: Some("Rwanda".into()),
                ..Properties::default()
            },
        );
        let receipt =
            decode_receipt(&command, &serde_json::to_vec(&updated).unwrap()).expect("updated");
        assert_eq!(
            receipt,
            Receipt::Updated(UpdateReceipt {
                project_id: "it_rwanda".into(),
                resources_ensured: true,
                dataset_exists: true,
                fields_updated: 2,
            })
        );

        // An answer about a different project, an unknown key, and a
        // `success:false` body are each unreadable rather than half-read.
        let other =
            json!({"success": true, "data": {"project_id": "it_tchad", "fields_updated": 1}});
        assert_eq!(
            decode_receipt(&command, &serde_json::to_vec(&other).unwrap())
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
        let extra = json!({"success": true, "data": {"project_id": "it_rwanda", "fields_updated": 1, "deleted": true}});
        assert_eq!(
            decode_receipt(&command, &serde_json::to_vec(&extra).unwrap())
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
        let failed = json!({"success": false, "data": {"project_id": "it_rwanda"}});
        assert_eq!(
            decode_receipt(&command, &serde_json::to_vec(&failed).unwrap())
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }

    fn error_body(code: &str, message: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "success": false,
            "error": {"code": code, "message": message, "timestamp": 1_758_360_000},
            "timestamp": "2026-09-20T10:00:00Z",
            "service": "data-solutions-backend",
            "version": "1.0.0"
        }))
        .unwrap()
    }

    /// Each refusal the route emits maps to its typed code, its kind, and
    /// carries the server's own sentence — for a 403 that sentence is the
    /// capability a project admin has to grant.
    #[test]
    fn each_refusal_the_route_emits_maps_to_its_typed_code() {
        let cases = [
            (
                400,
                "VALIDATION_FAILED",
                "display_name and project_name are required",
                ProjectPropertiesServiceCode::Invalid,
                ErrorKind::InvalidInput,
            ),
            (
                403,
                "INSUFFICIENT_PERMISSIONS",
                "Requires project.create capability",
                ProjectPropertiesServiceCode::CapabilityMissing,
                ErrorKind::AuthenticationRejected,
            ),
            (
                403,
                "INSUFFICIENT_PERMISSIONS",
                "Requires project.properties.edit capability (project admin)",
                ProjectPropertiesServiceCode::CapabilityMissing,
                ErrorKind::AuthenticationRejected,
            ),
            (
                403,
                "PROJECT_ARCHIVED",
                "This project is archived. Sharing and downloads remain available; mutating actions are denied.",
                ProjectPropertiesServiceCode::ProjectReadOnly,
                ErrorKind::AuthenticationRejected,
            ),
            (
                403,
                "PROJECT_EXPIRED",
                "This project has expired.",
                ProjectPropertiesServiceCode::ProjectReadOnly,
                ErrorKind::AuthenticationRejected,
            ),
            (
                404,
                "PROJECT_NOT_FOUND",
                "Project not found",
                ProjectPropertiesServiceCode::ProjectNotFound,
                ErrorKind::ResourceNotFound,
            ),
        ];
        for (status, code, sentence, typed, kind) in cases {
            let error = refusal(status, &error_body(code, sentence));
            assert_eq!(
                error.project_properties_service_code(),
                Some(typed),
                "{code}"
            );
            assert_eq!(error.kind(), kind, "{code}");
            let carried = error.service_refusal().expect("the sentence crosses");
            assert_eq!(carried.status(), status);
            assert_eq!(carried.code(), Some(code.to_ascii_lowercase().as_str()));
            assert_eq!(carried.message(), Some(sentence));
        }
        // A status without a recognised code keeps the status classification
        // and names no typed code.
        let error = refusal(503, b"upstream");
        assert_eq!(error.kind(), ErrorKind::Transient);
        assert_eq!(error.project_properties_service_code(), None);
        let error = refusal(
            401,
            &error_body("UNAUTHORIZED", "User email not found in context"),
        );
        assert_eq!(error.kind(), ErrorKind::AuthenticationRejected);
        assert_eq!(error.project_properties_service_code(), None);
    }

    fn template_project() -> ds_command_kernel::project_template::NewProject {
        ds_command_kernel::project_template::NewProject {
            display_name: "Nyaruguru LV".into(),
            project_name: "nyaruguru_lv".into(),
            location: Some("Nyaruguru".into()),
            description: None,
            country: Some("Rwanda".into()),
            client: Some("EDCL".into()),
            network_template: Some("master".into()),
            styling_template_id: None,
            settings: BTreeMap::from([
                (
                    "project_params".to_owned(),
                    json!({"project_type":"new_build","crs":{"mode":"tm_rwanda"}}),
                ),
                ("processing_lanes".to_owned(), json!({"fast":true})),
            ]),
        }
    }

    /// The template plan's creation is the same `create` action, carrying
    /// the settings blocks the handler already reads by name.
    #[test]
    fn a_template_plans_creation_is_the_create_action_with_its_settings() {
        let command = Command::CreateFromTemplate {
            project: template_project(),
        };
        command.validate().expect("valid template creation");
        assert_eq!(command.action(), "create");
        assert_eq!(
            String::from_utf8(command.body_bytes()).unwrap(),
            r#"{"action":"create","data":{"client":"EDCL","country":"Rwanda","display_name":"Nyaruguru LV","network_template":"master","processing_lanes":{"fast":true},"project_location":"Nyaruguru","project_name":"nyaruguru_lv","project_params":{"crs":{"mode":"tm_rwanda"},"project_type":"new_build"}}}"#
        );
        let receipt = decode_receipt(
            &command,
            br#"{"success":true,"data":{"project_id":"uid_nyaruguru_lv","project_name":"nyaruguru_lv","display_name":"Nyaruguru LV","network_template":"master"}}"#,
        )
        .unwrap();
        assert!(matches!(receipt, Receipt::Created(_)));

        for broken in [
            // A slug the display name does not derive.
            ds_command_kernel::project_template::NewProject {
                project_name: "other".into(),
                ..template_project()
            },
            // A settings block the handler does not read.
            ds_command_kernel::project_template::NewProject {
                settings: BTreeMap::from([("is_template".to_owned(), json!({}))]),
                ..template_project()
            },
            // A block that is not an object.
            ds_command_kernel::project_template::NewProject {
                settings: BTreeMap::from([("project_phases".to_owned(), json!(["I"]))]),
                ..template_project()
            },
        ] {
            let error = Command::CreateFromTemplate { project: broken }
                .validate()
                .unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidInput);
        }
    }

    /// Marking a template is a lifecycle-only update: it sends the project
    /// and the one flag, nothing else, so ds-brain's archive gate exempts it.
    #[test]
    fn marking_a_template_sends_only_the_flag() {
        for (template, body) in [
            (
                true,
                r#"{"action":"update","data":{"is_template":true,"project_id":"czgmdwth_gisagara"}}"#,
            ),
            (
                false,
                r#"{"action":"update","data":{"is_template":false,"project_id":"czgmdwth_gisagara"}}"#,
            ),
        ] {
            let command = Command::SetTemplate {
                project_id: "czgmdwth_gisagara".into(),
                template,
            };
            command.validate().unwrap();
            assert_eq!(String::from_utf8(command.body_bytes()).unwrap(), body);
        }
        let other = Command::SetTemplate {
            project_id: "czgmdwth_gisagara".into(),
            template: true,
        };
        assert!(decode_receipt(
            &other,
            br#"{"success":true,"data":{"project_id":"someone_else","fields_updated":1}}"#
        )
        .is_err());
        assert!(Command::SetTemplate {
            project_id: " padded".into(),
            template: true
        }
        .validate()
        .is_err());
    }
}
