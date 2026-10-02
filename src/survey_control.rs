//! Closed Survey control-plane calls. Publication authority remains with the backend.
use crate::survey::project_forms::ProjectFormChange;
use crate::survey_entry_create::Refusal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", content = "arguments", deny_unknown_fields)]
pub enum SurveyControlCommand {
    #[serde(rename = "survey.forms.list")]
    FormsList(CatalogQuery),
    #[serde(rename = "survey.form.read")]
    FormRead(FormRead),
    #[serde(rename = "survey.form.types")]
    FormTypes(Empty),
    #[serde(rename = "survey.form.create")]
    FormCreate(FormCreate),
    #[serde(rename = "survey.form.update")]
    FormUpdate(FormUpdate),
    #[serde(rename = "survey.form.lifecycle")]
    FormLifecycle(FormLifecycle),
    #[serde(rename = "survey.form.purge-plan")]
    FormPurgePlan(FormPurgePlan),
    #[serde(rename = "survey.form.purge")]
    FormPurge(FormPurge),
    #[serde(rename = "survey.project_forms.read")]
    ProjectFormsRead(ProjectQuery),
    #[serde(rename = "survey.project_form.editor")]
    ProjectFormEditor(ProjectEditor),
    #[serde(rename = "survey.project_forms.plan")]
    ProjectFormsPlan(ProjectChanges),
    #[serde(rename = "survey.project_forms.apply")]
    ProjectFormsApply(ProjectChanges),
    #[serde(rename = "survey.templates.list")]
    TemplatesList(CatalogQuery),
    #[serde(rename = "survey.template.read")]
    TemplateRead(TemplateRead),
    #[serde(rename = "survey.template.create")]
    TemplateCreate(TemplateCreate),
    #[serde(rename = "survey.template.apply")]
    TemplateApply(TemplateApply),
    #[serde(rename = "survey.template.lifecycle")]
    TemplateLifecycle(TemplateLifecycle),
    #[serde(rename = "survey.project.create_from_template")]
    ProjectCreate(ProjectCreate),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    pub limit: usize,
    #[serde(default)]
    pub detail: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FormRead {
    pub slug: String,
    pub field_offset: usize,
    pub field_limit: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormCreate {
    pub schema: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FormUpdate {
    pub slug: String,
    pub expected_version: u64,
    pub schema: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormAction {
    Duplicate,
    Publish,
    Unpublish,
    Archive,
    Restore,
    Delete,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FormLifecycle {
    pub action: FormAction,
    pub slug: String,
    #[serde(default)]
    pub new_display_name: Option<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub expected_version: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormPurgePlan {
    pub slug: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FormPurge {
    pub slug: String,
    pub plan_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectQuery {
    pub project: String,
    pub status: String,
    #[serde(default)]
    pub query: Option<String>,
    pub limit: usize,
    #[serde(default)]
    pub detail: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectEditor {
    pub project: String,
    pub form: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectChanges {
    pub project: String,
    pub changes: Vec<ProjectFormChange>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TemplateRead {
    pub template: String,
    pub form_offset: usize,
    pub form_limit: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateCreate {
    pub project: String,
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    pub visibility: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TemplateApply {
    pub project: String,
    pub template: String,
    pub merge_strategy: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateAction {
    Publish,
    Privatize,
    Delete,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateLifecycle {
    pub action: TemplateAction,
    pub template: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectCreate {
    pub template: String,
    pub project_name: String,
    #[serde(default)]
    pub project_id: Option<String>,
}

pub(crate) fn invalid() -> Refusal {
    Refusal::InvalidInput
}
pub(crate) fn unreadable() -> Refusal {
    Refusal::UnreadableResponse
}
fn text(s: &str, max: usize) -> bool {
    !s.is_empty() && s.trim() == s && s.len() <= max && !s.chars().any(char::is_control)
}
fn slug(s: &str) -> bool {
    text(s, 160)
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
impl SurveyControlCommand {
    pub fn project_hint(&self) -> Option<&str> {
        match self {
            Self::ProjectFormsRead(v) => Some(&v.project),
            Self::ProjectFormEditor(v) => Some(&v.project),
            Self::ProjectFormsPlan(v) | Self::ProjectFormsApply(v) => Some(&v.project),
            Self::TemplateCreate(v) => Some(&v.project),
            Self::TemplateApply(v) => Some(&v.project),
            _ => None,
        }
    }
    pub fn validate(&self) -> Result<(), Refusal> {
        let valid = match self {
            Self::FormsList(q) | Self::TemplatesList(q) => {
                (1..=500).contains(&q.limit)
                    && q.query.as_deref().is_none_or(|s| text(s, 200))
                    && q.status
                        .as_deref()
                        .is_none_or(|s| matches!(s, "active" | "archived" | "all"))
            }
            Self::FormRead(q) => {
                slug(&q.slug) && q.field_offset <= 100_000 && (1..=100).contains(&q.field_limit)
            }
            Self::FormTypes(_) => true,
            Self::FormCreate(v) => v.schema.is_object(),
            Self::FormUpdate(v) => {
                slug(&v.slug)
                    && v.expected_version > 0
                    && v.expected_version <= 9_007_199_254_740_991
                    && v.schema.is_object()
                    && v.schema.get("slug").is_none_or(|s| s == &v.slug)
                    && v.schema
                        .get("version")
                        .is_none_or(|s| s.as_u64() == Some(v.expected_version))
            }
            Self::FormLifecycle(v) => {
                slug(&v.slug)
                    && v.expected_version
                        .is_none_or(|v| v > 0 && v <= 9_007_199_254_740_991)
                    && (!matches!(v.action, FormAction::Duplicate)
                        || v.new_display_name.as_deref().is_some_and(|s| text(s, 200)))
            }
            Self::FormPurgePlan(v) => slug(&v.slug),
            Self::FormPurge(v) => slug(&v.slug) && purge_digest(&v.plan_digest),
            Self::ProjectFormsRead(v) => {
                text(&v.project, 200)
                    && (1..=500).contains(&v.limit)
                    && matches!(
                        v.status.as_str(),
                        "enabled" | "disabled" | "unavailable" | "all"
                    )
                    && v.query.as_deref().is_none_or(|s| text(s, 200))
            }
            Self::ProjectFormEditor(v) => text(&v.project, 200) && slug(&v.form),
            Self::ProjectFormsPlan(v) | Self::ProjectFormsApply(v) => {
                text(&v.project, 200)
                    && crate::survey::project_forms::validate_changes(&v.changes).is_ok()
            }
            Self::TemplateRead(v) => {
                slug(&v.template) && v.form_offset <= 100_000 && (1..=100).contains(&v.form_limit)
            }
            Self::TemplateCreate(v) => {
                text(&v.project, 200)
                    && text(&v.name, 200)
                    && v.slug.as_deref().is_none_or(slug)
                    && v.description.as_deref().is_none_or(|s| text(s, 2000))
                    && v.category.as_deref().is_none_or(|s| text(s, 120))
                    && matches!(v.visibility.as_str(), "private" | "organization" | "public")
            }
            Self::TemplateApply(v) => {
                text(&v.project, 200)
                    && slug(&v.template)
                    && matches!(v.merge_strategy.as_str(), "overwrite" | "preserve")
            }
            Self::TemplateLifecycle(v) => slug(&v.template),
            Self::ProjectCreate(v) => {
                slug(&v.template)
                    && text(&v.project_name, 200)
                    && v.project_id.as_deref().is_none_or(|s| text(s, 200))
            }
        };
        if !valid || serde_json::to_vec(self).map_err(|_| invalid())?.len() > 1024 * 1024 {
            return Err(invalid());
        }
        Ok(())
    }
}

pub fn response(body: &[u8]) -> Result<Value, Refusal> {
    if body.len() > 8 * 1024 * 1024 {
        return Err(unreadable());
    }
    let wire: Value = serde_json::from_slice(body).map_err(|_| unreadable())?;
    if wire["status"] == "success" {
        return Ok(wire);
    }
    if wire["success"] == true && wire["data"].is_object() {
        return Ok(wire["data"].clone());
    }
    Err(unreadable())
}

/// A purge is authorized against one complete, backend-owned scope. Never
/// infer completeness from an HTTP success or an omitted residual count.
pub const FORM_PURGE_COUNTS: &[&str] = &[
    "schemas",
    "styles",
    "project_bindings",
    "entry_documents",
    "other_documents",
    "legacy_deleted_documents",
    "shared_form_documents",
    "template_references",
    "access_grant_references",
    "media_versions",
    "bq_objects",
    "bq_rows",
    "managed_tile_pointers",
];

fn purge_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn purge_counts(value: &Value) -> bool {
    value.as_object().is_some_and(|counts| {
        counts.len() == FORM_PURGE_COUNTS.len()
            && FORM_PURGE_COUNTS.iter().all(|key| {
                counts
                    .get(*key)
                    .and_then(Value::as_u64)
                    .is_some_and(|n| n <= 9_007_199_254_740_991)
            })
    })
}

pub fn form_purge_plan_response(wire: Value, slug: &str) -> Result<Value, Refusal> {
    let plan = &wire["plan"];
    if plan["slug"] != slug
        || !plan["plan_digest"].as_str().is_some_and(purge_digest)
        || !purge_counts(&plan["scope"])
        || !plan["projects"]
            .as_array()
            .is_some_and(|rows| rows.len() <= 1000)
        || !plan["templates"]
            .as_array()
            .is_some_and(|rows| rows.len() <= 100_000)
        || plan
            .get("remaining")
            .is_some_and(|counts| !purge_counts(counts))
    {
        return Err(unreadable());
    }
    if let Some(resume) = plan.get("resume").filter(|value| !value.is_null()) {
        if !matches!(resume["state"].as_str(), Some("partial" | "complete"))
            || resume["plan_digest"] != plan["plan_digest"]
        {
            return Err(unreadable());
        }
    }
    Ok(json!({"plan": plan}))
}

pub fn form_purge_response(wire: Value, request: &FormPurge) -> Result<Value, Refusal> {
    let receipt = &wire["receipt"];
    if receipt["slug"] != request.slug
        || receipt["plan_digest"] != request.plan_digest
        || !matches!(receipt["state"].as_str(), Some("complete" | "partial"))
        || !purge_counts(&receipt["scope"])
        || !purge_counts(&receipt["remaining"])
        || !receipt["projects"]
            .as_array()
            .is_some_and(|rows| rows.len() <= 1000)
        || !receipt["errors"].as_array().is_some_and(|rows| {
            rows.len() <= 1000
                && rows
                    .iter()
                    .all(|row| row.as_str().is_some_and(|s| text(s, 2000)))
        })
        || (receipt["state"] == "complete"
            && FORM_PURGE_COUNTS
                .iter()
                .any(|key| receipt["remaining"][*key] != 0))
    {
        return Err(unreadable());
    }
    Ok(json!({"receipt": receipt}))
}
pub fn form_summary(form: &Value, detail: bool) -> Value {
    let mut result = json!({"slug":form["slug"],"display_name":form["display_name"],"version":form["version"],"visibility":form.get("visibility").cloned().unwrap_or_else(||json!(if form["is_public"]==true{"public"}else{"private"})),"geometry_type":form.get("geometry_type").unwrap_or(&form["drawing_settings"]["geometry_type"]),"field_count":form["fields"].as_array().map_or(0,Vec::len)});
    if detail {
        for key in [
            "description",
            "capture_detailed_location",
            "capabilities",
            "metadata",
        ] {
            result[key] = form[key].clone();
        }
    }
    result
}
pub fn page_form(mut form: Value, request: &FormRead) -> Result<Value, Refusal> {
    if form["slug"] != request.slug {
        return Err(unreadable());
    }
    let fields = form["fields"].as_array().ok_or_else(unreadable)?;
    let total = fields.len();
    let page: Vec<_> = fields
        .iter()
        .skip(request.field_offset)
        .take(request.field_limit)
        .cloned()
        .collect();
    let count = page.len();
    form["fields"] = json!(page);
    Ok(
        json!({"form":form,"fieldTotal":total,"fieldOffset":request.field_offset,"fieldReturned":count,"nextFieldOffset":request.field_offset+count,"complete":request.field_offset+count>=total}),
    )
}
