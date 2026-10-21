use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{ClientError, ErrorKind, TransportResponse};

const MAX_FORM_SLUG_CHARS: usize = 160;
const MAX_DISPLAY_NAME_CHARS: usize = 500;
const MAX_DESCRIPTION_CHARS: usize = 8_000;
const MAX_SECTIONS: usize = 64;
const MAX_FIELDS_PER_SECTION: usize = 128;
const MAX_FIELDS: usize = 1_024;
const MAX_SETTINGS: usize = 512;
const MAX_OPTIONS: usize = 2_000;
const MAX_CHIPS: usize = 2_000;
const MAX_TEXT_CHARS: usize = 16_384;
const MAX_JSON_DEPTH: usize = 32;
const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;

/// One backend-resolved, optimistic-revision-pinned settings editor for a
/// form in the authenticated user's selected project.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectFormSettingsEditor {
    ds_project: String,
    form_slug: String,
    editor: Value,
}

impl ProjectFormSettingsEditor {
    pub fn ds_project(&self) -> &str {
        &self.ds_project
    }

    pub fn form_slug(&self) -> &str {
        &self.form_slug
    }

    /// The validated backend-owned editor projection. The shape is closed by
    /// this crate before a host can observe it.
    pub const fn editor(&self) -> &Value {
        &self.editor
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditorEnvelope {
    status: String,
    project_id: String,
    form_slug: String,
    editor: EditorWire,
    #[serde(default)]
    unavailable: Option<bool>,
    #[serde(default)]
    unavailable_reason: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EditorWire {
    form: FormWire,
    current_settings: Map<String, Value>,
    effective_settings: Map<String, Value>,
    sections: Vec<SectionWire>,
    field_state: BTreeMap<String, FieldStateWire>,
    capabilities: CapabilitiesWire,
    version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    read_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unavailable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unavailable_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FormWire {
    slug: String,
    display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    enabled: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SectionWire {
    key: String,
    label: String,
    fields: Vec<FieldWire>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FieldWire {
    key: String,
    label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(rename = "type")]
    field_type: String,
    value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    options: Option<Vec<OptionWire>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chips: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    show_when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hide_when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    required_when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    options_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    requires_capability: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OptionWire {
    value: String,
    label: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FieldStateWire {
    visible: bool,
    applicable: bool,
    enabled: bool,
    required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CapabilitiesWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    geometry_type: Option<String>,
    supports_drawing: bool,
    supports_gps_location: bool,
    supports_location_picking: bool,
    supports_snapping: bool,
    supports_position_editing: bool,
    supports_geometry_drawing_tools: bool,
    supports_geometry_automation: bool,
}

pub(crate) fn validate_editor_input(ds_project: &str, form_slug: &str) -> Result<(), ClientError> {
    crate::validate_project_id(ds_project)?;
    if !bounded(form_slug, MAX_FORM_SLUG_CHARS) {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "the selected project or form identity is empty, untrimmed, or outside its bound",
        ));
    }
    Ok(())
}

pub(crate) fn decode_editor(
    response: TransportResponse,
    response_limit: usize,
    ds_project: &str,
    form_slug: &str,
) -> Result<ProjectFormSettingsEditor, ClientError> {
    if response.body.len() > response_limit {
        return Err(unreadable());
    }
    let payload: EditorEnvelope =
        serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if payload.status != "success"
        || payload.project_id != ds_project
        || payload.form_slug != form_slug
        || payload.editor.form.slug != form_slug
        || !bounded(&payload.editor.form.display_name, MAX_DISPLAY_NAME_CHARS)
        || payload
            .editor
            .form
            .description
            .as_deref()
            .is_some_and(|value| !bounded_optional(value, MAX_DESCRIPTION_CHARS))
        || payload.editor.sections.len() > MAX_SECTIONS
        || payload.editor.current_settings.len() > MAX_SETTINGS
        || payload.editor.effective_settings.len() > MAX_SETTINGS
        || payload.editor.field_state.len() > MAX_FIELDS
        || payload.editor.version > MAX_SAFE_JSON_INTEGER
    {
        return Err(unreadable());
    }

    validate_map(&payload.editor.current_settings)?;
    validate_map(&payload.editor.effective_settings)?;

    let mut section_keys = BTreeSet::new();
    let mut field_keys = BTreeSet::new();
    let mut field_total = 0usize;
    for section in &payload.editor.sections {
        if !bounded(&section.key, 120)
            || !bounded(&section.label, 500)
            || !section_keys.insert(section.key.as_str())
            || section.fields.len() > MAX_FIELDS_PER_SECTION
        {
            return Err(unreadable());
        }
        field_total = field_total.saturating_add(section.fields.len());
        if field_total > MAX_FIELDS {
            return Err(unreadable());
        }
        for field in &section.fields {
            if !bounded(&field.key, 120)
                || !bounded(&field.label, 500)
                || !bounded(&field.field_type, 80)
                || !field_keys.insert(field.key.as_str())
                || !valid_value(&field.value, 0)
                || field
                    .default
                    .as_ref()
                    .is_some_and(|value| !valid_value(value, 0))
                || !valid_field_text(field)
                || field.options.as_ref().is_some_and(|items| {
                    items.len() > MAX_OPTIONS
                        || items
                            .iter()
                            .any(|item| !bounded(&item.value, 500) || !bounded(&item.label, 500))
                })
                || field.chips.as_ref().is_some_and(|items| {
                    items.len() > MAX_CHIPS
                        || items
                            .iter()
                            .any(|item| !bounded_optional(item, MAX_TEXT_CHARS))
                })
            {
                return Err(unreadable());
            }
        }
    }

    let state_keys = payload
        .editor
        .field_state
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if state_keys != field_keys
        || payload.editor.field_state.values().any(|state| {
            state
                .reason
                .as_deref()
                .is_some_and(|value| !bounded_optional(value, MAX_TEXT_CHARS))
        })
        || payload
            .editor
            .capabilities
            .geometry_type
            .as_deref()
            .is_some_and(|value| !matches!(value, "point" | "line" | "polygon"))
        || payload
            .editor
            .unavailable_reason
            .as_deref()
            .is_some_and(|value| !bounded(value, 200))
        || payload
            .editor
            .message
            .as_deref()
            .is_some_and(|value| !bounded_optional(value, MAX_DESCRIPTION_CHARS))
    {
        return Err(unreadable());
    }

    let unavailable = payload.editor.unavailable.unwrap_or(false);
    if payload.unavailable.unwrap_or(false) != unavailable
        || payload.unavailable_reason.as_deref().unwrap_or("")
            != payload.editor.unavailable_reason.as_deref().unwrap_or("")
        || (unavailable
            && (payload.editor.read_only != Some(true)
                || !payload.editor.sections.is_empty()
                || !payload.editor.field_state.is_empty()))
    {
        return Err(unreadable());
    }

    let editor = serde_json::to_value(payload.editor).map_err(|_| unreadable())?;
    Ok(ProjectFormSettingsEditor {
        ds_project: ds_project.to_owned(),
        form_slug: form_slug.to_owned(),
        editor,
    })
}

fn validate_map(map: &Map<String, Value>) -> Result<(), ClientError> {
    if map
        .iter()
        .any(|(key, value)| !bounded(key, 120) || !valid_value(value, 0))
    {
        return Err(unreadable());
    }
    Ok(())
}

fn valid_field_text(field: &FieldWire) -> bool {
    [
        field.description.as_deref(),
        field.placeholder.as_deref(),
        field.hint.as_deref(),
        field.show_when.as_deref(),
        field.hide_when.as_deref(),
        field.required_when.as_deref(),
        field.options_source.as_deref(),
        field.requires_capability.as_deref(),
    ]
    .into_iter()
    .flatten()
    .all(|value| bounded_optional(value, MAX_TEXT_CHARS))
}

fn valid_value(value: &Value, depth: usize) -> bool {
    if depth > MAX_JSON_DEPTH {
        return false;
    }
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => true,
        Value::String(value) => bounded_optional(value, MAX_TEXT_CHARS),
        Value::Array(values) => {
            values.len() <= MAX_OPTIONS && values.iter().all(|value| valid_value(value, depth + 1))
        }
        Value::Object(values) => {
            values.len() <= MAX_SETTINGS
                && values
                    .iter()
                    .all(|(key, value)| bounded(key, 120) && valid_value(value, depth + 1))
        }
    }
}

fn bounded(value: &str, max_chars: usize) -> bool {
    !value.is_empty() && bounded_optional(value, max_chars)
}

fn bounded_optional(value: &str, max_chars: usize) -> bool {
    value.trim() == value
        && value.chars().count() <= max_chars
        && !value.chars().any(char::is_control)
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the response is outside the closed project-form settings-editor contract",
    )
}
