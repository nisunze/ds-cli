use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::{ClientError, ErrorKind, TransportResponse};

const MAX_FORMS: usize = 2_000;
const MAX_FORM_SLUG_CHARS: usize = 160;
const MAX_DISPLAY_NAME_CHARS: usize = 500;
const MAX_GEOMETRY_TYPE_CHARS: usize = 80;
const MAX_ORPHANS: usize = 2_000;
const MAX_ORPHAN_REASON_CHARS: usize = 120;

/// Bounded authoritative project-form catalogue for one selected project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFormsSnapshot {
    ds_project: String,
    forms: Vec<ProjectFormSummary>,
    orphaned_project_forms: Vec<ProjectFormOrphan>,
}

impl ProjectFormsSnapshot {
    pub fn ds_project(&self) -> &str {
        &self.ds_project
    }
    pub fn forms(&self) -> &[ProjectFormSummary] {
        &self.forms
    }
    pub fn orphaned_project_forms(&self) -> &[ProjectFormOrphan] {
        &self.orphaned_project_forms
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFormSummary {
    slug: String,
    display_name: String,
    enabled: bool,
    available: bool,
    settings_revision: u64,
    geometry_type: Option<String>,
}

impl ProjectFormSummary {
    pub fn slug(&self) -> &str {
        &self.slug
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    pub const fn available(&self) -> bool {
        self.available
    }
    pub const fn settings_revision(&self) -> u64 {
        self.settings_revision
    }
    pub fn geometry_type(&self) -> Option<&str> {
        self.geometry_type.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFormOrphan {
    form_slug: String,
    reason: String,
    enabled: bool,
    settings_revision: u64,
}

impl ProjectFormOrphan {
    pub fn form_slug(&self) -> &str {
        &self.form_slug
    }
    pub fn reason(&self) -> &str {
        &self.reason
    }
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    pub const fn settings_revision(&self) -> u64 {
        self.settings_revision
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivationEnvelope {
    status: String,
    project_id: String,
    resolved_at: serde_json::Value,
    user: ActivationUser,
    forms: Vec<FormWire>,
    #[serde(default)]
    orphaned_project_forms: Vec<OrphanWire>,
    #[serde(default)]
    field_types: serde_json::Value,
    #[serde(default)]
    condition_operators: serde_json::Value,
    #[serde(default)]
    type_aliases: serde_json::Value,
    #[serde(default)]
    settings_registry: serde_json::Value,
    #[serde(default)]
    settings_editors: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivationUser {
    email: String,
    roles: Vec<String>,
    is_owner: bool,
    is_admin: bool,
}

#[derive(Deserialize)]
struct FormWire {
    slug: String,
    display_name: String,
    enabled: bool,
    #[serde(default)]
    settings_revision: u64,
    #[serde(default)]
    geometry_type: Option<String>,
    // ds-brain marshals a form that carries no drawing configuration and no
    // metadata map as an explicit `null`, not as an absent key, and serde's
    // `default` only covers the absent key. Declaring these optional is what
    // lets the real activation payload decode: nine of the thirteen forms on
    // zimebnlu_edcl2 send `"drawing_settings": null`, and every one of them
    // used to fail the whole snapshot with `auth_response_unreadable`.
    #[serde(default)]
    drawing_settings: Option<DrawingSettingsWire>,
    #[serde(default)]
    metadata: Option<MetadataWire>,
    #[serde(default)]
    master_availability: Option<String>,
}

#[derive(Default, Deserialize)]
struct DrawingSettingsWire {
    #[serde(default)]
    geometry_type: Option<String>,
}

#[derive(Default, Deserialize)]
struct MetadataWire {
    #[serde(default)]
    unavailable: bool,
}

#[derive(Deserialize)]
struct OrphanWire {
    project_id: String,
    form_slug: String,
    reason: String,
    enabled: bool,
    #[serde(default)]
    settings_revision: u64,
}

pub(crate) fn decode_project_forms(
    response: TransportResponse,
    response_limit: usize,
    ds_project: &str,
    canonical_email: &str,
) -> Result<ProjectFormsSnapshot, ClientError> {
    if response.body.len() > response_limit {
        return Err(unreadable());
    }
    let payload: ActivationEnvelope = serde_json::from_slice(&response.body)
        .map_err(|_| unreadable_message(schema_diagnostic(&response.body)))?;
    for (valid, message) in [
        (payload.status == "success", "the project-forms activation contract refused status: expected success"),
        (payload.project_id == ds_project, "the project-forms activation contract refused project_id: scope mismatch"),
        (payload.user.email == canonical_email, "the project-forms activation contract refused user.email: identity mismatch"),
        (payload.forms.len() <= MAX_FORMS, "the project-forms activation contract refused forms: count exceeds bound"),
        (payload.orphaned_project_forms.len() <= MAX_ORPHANS, "the project-forms activation contract refused orphaned_project_forms: count exceeds bound"),
        (payload.resolved_at.is_number(), "the project-forms activation contract refused resolved_at: expected number"),
        (payload.field_types.is_array(), "the project-forms activation contract refused field_types: expected array"),
        (payload.condition_operators.is_array(), "the project-forms activation contract refused condition_operators: expected array"),
        (payload.type_aliases.is_object(), "the project-forms activation contract refused type_aliases: expected object"),
        (payload.settings_registry.is_null() || payload.settings_registry.is_array(), "the project-forms activation contract refused settings_registry: expected array or null"),
        (payload.settings_editors.is_null() || payload.settings_editors.is_object(), "the project-forms activation contract refused settings_editors: expected object or null"),
        (payload.user.roles.iter().all(|role| bounded(role, 120)), "the project-forms activation contract refused user.roles: invalid bounded role"),
    ] {
        if !valid { return Err(unreadable_message(message)); }
    }
    let _ = (payload.user.is_owner, payload.user.is_admin);

    let mut seen = BTreeSet::new();
    let mut lineage = BTreeMap::new();
    let mut forms = Vec::with_capacity(payload.forms.len());
    for form in payload.forms {
        if !bounded(&form.slug, MAX_FORM_SLUG_CHARS) {
            return Err(unreadable_message(
                "the project-forms activation contract refused forms[].slug: invalid bounded text",
            ));
        }
        if !bounded(&form.display_name, MAX_DISPLAY_NAME_CHARS) {
            return Err(unreadable_message("the project-forms activation contract refused forms[].display_name: invalid bounded text"));
        }
        if !seen.insert(form.slug.clone()) {
            return Err(unreadable_message(
                "the project-forms activation contract refused forms[].slug: duplicate",
            ));
        }
        let master_availability = form
            .master_availability
            .as_deref()
            .unwrap_or("master_available");
        if !matches!(
            master_availability,
            "master_available" | "master_archived" | "master_missing"
        ) {
            return Err(unreadable_message("the project-forms activation contract refused forms[].master_availability: unknown state"));
        }
        let geometry_type = form
            .geometry_type
            .or(form.drawing_settings.unwrap_or_default().geometry_type)
            .filter(|value| !value.is_empty());
        if geometry_type
            .as_deref()
            .is_some_and(|value| !bounded(value, MAX_GEOMETRY_TYPE_CHARS))
        {
            return Err(unreadable_message("the project-forms activation contract refused forms[].geometry_type: invalid bounded text"));
        }
        lineage.insert(
            form.slug.clone(),
            (
                master_availability.to_owned(),
                form.enabled,
                form.settings_revision,
            ),
        );
        forms.push(ProjectFormSummary {
            slug: form.slug,
            display_name: form.display_name,
            enabled: form.enabled,
            available: master_availability == "master_available"
                && !form.metadata.unwrap_or_default().unavailable,
            settings_revision: form.settings_revision,
            geometry_type,
        });
    }
    forms.sort_by(|left, right| left.slug.cmp(&right.slug));

    let mut orphan_slugs = BTreeSet::new();
    let mut orphaned_project_forms = Vec::with_capacity(payload.orphaned_project_forms.len());
    for orphan in payload.orphaned_project_forms {
        if orphan.project_id != ds_project
            || !bounded(&orphan.form_slug, MAX_FORM_SLUG_CHARS)
            || !matches!(orphan.reason.as_str(), "master_archived" | "master_missing")
            || !bounded(&orphan.reason, MAX_ORPHAN_REASON_CHARS)
            || !orphan_slugs.insert(orphan.form_slug.clone())
        {
            return Err(unreadable_message("the project-forms activation contract refused orphaned_project_forms[]: invalid scope, reason or duplicate"));
        }
        // The server also returns renderable archived and missing-master forms
        // in `forms`. Their audit row is a second view of the same binding.
        if let Some((availability, enabled, revision)) = lineage.get(&orphan.form_slug) {
            if availability != &orphan.reason
                || *enabled != orphan.enabled
                || *revision != orphan.settings_revision
            {
                return Err(unreadable_message("the project-forms activation contract refused orphaned_project_forms[]: lineage disagreement"));
            }
        }
        orphaned_project_forms.push(ProjectFormOrphan {
            form_slug: orphan.form_slug,
            reason: orphan.reason,
            enabled: orphan.enabled,
            settings_revision: orphan.settings_revision,
        });
    }
    orphaned_project_forms.sort_by(|left, right| left.form_slug.cmp(&right.form_slug));

    Ok(ProjectFormsSnapshot {
        ds_project: ds_project.to_owned(),
        forms,
        orphaned_project_forms,
    })
}

fn bounded(value: &str, max_chars: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= max_chars
        && !value.chars().any(char::is_control)
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the response is outside the closed project-forms activation contract",
    )
}

// This refusal-only pass names known schema fields and type classes. It never
// admits a response, emits response values/unknown keys, or changes authority.
fn schema_diagnostic(body: &[u8]) -> &'static str {
    use serde_json::Value;
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return "the project-forms activation contract refused response: invalid JSON";
    };
    let Some(root) = value.as_object() else {
        return "the project-forms activation contract refused response: expected object";
    };
    if root.keys().any(|key| {
        !matches!(
            key.as_str(),
            "status"
                | "project_id"
                | "resolved_at"
                | "user"
                | "forms"
                | "orphaned_project_forms"
                | "field_types"
                | "condition_operators"
                | "type_aliases"
                | "settings_registry"
                | "settings_editors"
        )
    }) {
        return "the project-forms activation contract refused response: unknown field";
    }
    for (valid, message) in [
        (
            value["status"].is_string(),
            "the project-forms activation contract refused status: expected string",
        ),
        (
            value["project_id"].is_string(),
            "the project-forms activation contract refused project_id: expected string",
        ),
        (
            root.contains_key("resolved_at"),
            "the project-forms activation contract refused resolved_at: missing field",
        ),
        (
            value["user"].is_object(),
            "the project-forms activation contract refused user: expected object",
        ),
        (
            value["forms"].is_array(),
            "the project-forms activation contract refused forms: expected array",
        ),
    ] {
        if !valid {
            return message;
        }
    }
    let user = value["user"].as_object().expect("checked object");
    if user
        .keys()
        .any(|key| !matches!(key.as_str(), "email" | "roles" | "is_owner" | "is_admin"))
    {
        return "the project-forms activation contract refused user: unknown field";
    }
    for (valid, message) in [
        (
            value["user"]["email"].is_string(),
            "the project-forms activation contract refused user.email: expected string",
        ),
        (
            value["user"]["roles"].is_array(),
            "the project-forms activation contract refused user.roles: expected array",
        ),
        (
            value["user"]["is_owner"].is_boolean(),
            "the project-forms activation contract refused user.is_owner: expected boolean",
        ),
        (
            value["user"]["is_admin"].is_boolean(),
            "the project-forms activation contract refused user.is_admin: expected boolean",
        ),
    ] {
        if !valid {
            return message;
        }
    }
    if value["user"]["roles"]
        .as_array()
        .expect("checked array")
        .iter()
        .any(|role| !role.is_string())
    {
        return "the project-forms activation contract refused user.roles[]: expected string";
    }
    for form in value["forms"].as_array().expect("checked array") {
        for (valid,message) in [
            (form.is_object(), "the project-forms activation contract refused forms[]: expected object"),
            (form["slug"].is_string(), "the project-forms activation contract refused forms[].slug: expected string"),
            (form["display_name"].is_string(), "the project-forms activation contract refused forms[].display_name: expected string"),
            (form["enabled"].is_boolean(), "the project-forms activation contract refused forms[].enabled: expected boolean"),
        ] { if !valid { return message; } }
        if form
            .get("settings_revision")
            .is_some_and(|v| v.as_u64().is_none())
        {
            return "the project-forms activation contract refused forms[].settings_revision: expected unsigned integer";
        }
        if form
            .get("geometry_type")
            .is_some_and(|v| !v.is_null() && !v.is_string())
        {
            return "the project-forms activation contract refused forms[].geometry_type: expected string or null";
        }
        if form
            .get("master_availability")
            .is_some_and(|v| !v.is_null() && !v.is_string())
        {
            return "the project-forms activation contract refused forms[].master_availability: expected string or null";
        }
        if form
            .get("drawing_settings")
            .is_some_and(|v| !v.is_null() && !v.is_object())
        {
            return "the project-forms activation contract refused forms[].drawing_settings: expected object or null";
        }
        if form
            .get("drawing_settings")
            .and_then(|v| v.get("geometry_type"))
            .is_some_and(|v| !v.is_null() && !v.is_string())
        {
            return "the project-forms activation contract refused forms[].drawing_settings.geometry_type: expected string or null";
        }
        if form
            .get("metadata")
            .is_some_and(|v| !v.is_null() && !v.is_object())
        {
            return "the project-forms activation contract refused forms[].metadata: expected object or null";
        }
        if form
            .get("metadata")
            .and_then(|v| v.get("unavailable"))
            .is_some_and(|v| !v.is_boolean())
        {
            return "the project-forms activation contract refused forms[].metadata.unavailable: expected boolean";
        }
    }
    if root
        .get("orphaned_project_forms")
        .is_some_and(|v| !v.is_array())
    {
        return "the project-forms activation contract refused orphaned_project_forms: expected array";
    }
    "the project-forms activation contract refused response: closed schema mismatch"
}

fn unreadable_message(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, message)
}

#[cfg(test)]
mod geometry_absence_tests {
    use super::*;
    use serde_json::{json, Value};

    fn decode(
        primary: Option<Value>,
        nested: Option<Value>,
    ) -> Result<ProjectFormsSnapshot, ClientError> {
        let mut form = json!({"slug":"example","display_name":"Example","enabled":true});
        if let Some(value) = primary {
            form["geometry_type"] = value;
        }
        if let Some(value) = nested {
            form["drawing_settings"] = json!({"geometry_type":value});
        }
        let body = json!({"status":"success","project_id":"project","resolved_at":1,"user":{"email":"user@example.com","roles":[],"is_owner":false,"is_admin":false},"forms":[form],"field_types":[],"condition_operators":[],"type_aliases":{}});
        decode_project_forms(
            TransportResponse::new(200, serde_json::to_vec(&body).unwrap()),
            65536,
            "project",
            "user@example.com",
        )
    }

    #[test]
    fn geometry_empty_selected_value_is_absent_without_fallback() {
        for (primary, nested) in [
            (None, None),
            (Some(json!("")), None),
            (None, Some(json!(""))),
            (Some(json!("")), Some(json!("Point"))),
            (Some(json!(null)), Some(json!(""))),
        ] {
            assert_eq!(
                decode(primary, nested).unwrap().forms()[0].geometry_type(),
                None
            );
        }
        assert_eq!(
            decode(None, Some(json!("Point"))).unwrap().forms()[0].geometry_type(),
            Some("Point")
        );
        assert_eq!(
            decode(Some(json!("Point")), Some(json!("LineString")))
                .unwrap()
                .forms()[0]
                .geometry_type(),
            Some("Point")
        );
    }

    #[test]
    fn geometry_nonempty_malformed_values_still_refuse() {
        for value in [
            json!(" "),
            json!(" Point"),
            json!("Point "),
            json!("Point\n"),
            json!("x".repeat(81)),
            json!(1),
        ] {
            assert!(decode(Some(value.clone()), None).is_err());
            assert!(decode(None, Some(value)).is_err());
        }
        assert!(decode(Some(json!("x".repeat(80))), None).is_ok());
    }
}
