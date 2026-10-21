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
    let payload: ActivationEnvelope =
        serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if payload.status != "success"
        || payload.project_id != ds_project
        || payload.user.email != canonical_email
        || payload.forms.len() > MAX_FORMS
        || payload.orphaned_project_forms.len() > MAX_ORPHANS
        || !payload.resolved_at.is_number()
        || !payload.field_types.is_array()
        || !payload.condition_operators.is_array()
        || !payload.type_aliases.is_object()
        || (!payload.settings_registry.is_null() && !payload.settings_registry.is_array())
        || (!payload.settings_editors.is_null() && !payload.settings_editors.is_object())
        || payload.user.roles.iter().any(|role| !bounded(role, 120))
    {
        return Err(unreadable());
    }
    let _ = (payload.user.is_owner, payload.user.is_admin);

    let mut seen = BTreeSet::new();
    let mut lineage = BTreeMap::new();
    let mut forms = Vec::with_capacity(payload.forms.len());
    for form in payload.forms {
        if !bounded(&form.slug, MAX_FORM_SLUG_CHARS)
            || !bounded(&form.display_name, MAX_DISPLAY_NAME_CHARS)
            || !seen.insert(form.slug.clone())
        {
            return Err(unreadable());
        }
        let master_availability = form
            .master_availability
            .as_deref()
            .unwrap_or("master_available");
        if !matches!(
            master_availability,
            "master_available" | "master_archived" | "master_missing"
        ) {
            return Err(unreadable());
        }
        let geometry_type = form
            .geometry_type
            .or(form.drawing_settings.unwrap_or_default().geometry_type);
        if geometry_type
            .as_deref()
            .is_some_and(|value| !bounded(value, MAX_GEOMETRY_TYPE_CHARS))
        {
            return Err(unreadable());
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
            return Err(unreadable());
        }
        // The server also returns renderable archived and missing-master forms
        // in `forms`. Their audit row is a second view of the same binding.
        if let Some((availability, enabled, revision)) = lineage.get(&orphan.form_slug) {
            if availability != &orphan.reason
                || *enabled != orphan.enabled
                || *revision != orphan.settings_revision
            {
                return Err(unreadable());
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
