//! Closed Survey control-plane calls. Publication authority remains with the backend.
use crate::{ClientError, ErrorKind};
use ds_command_kernel::survey::project_forms::ProjectFormChange;
use ds_command_kernel::survey_control as kernel;
pub use kernel::{FormPurge, FormPurgePlan};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", content = "arguments", deny_unknown_fields)]
pub enum SurveyControlCommand {
    #[serde(rename = "survey.forms.list")]
    FormsList(CatalogQuery),
    #[serde(rename = "survey.forms.purge-candidates")]
    FormPurgeCandidates(Empty),
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

pub(crate) fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "Survey control request violates its bounded contract",
    )
}
pub(crate) fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "Survey control response violates its identity or shape contract",
    )
}
/// The command names a project other than the session's selected one.
///
/// This is NOT a malformed request, and saying so was the defect: the project
/// named may be perfectly valid, visible and active, and it read back exactly
/// like a typo. What stands in the way is the session's own selection, which
/// `--project` cannot change — so a caller told their request "violates its
/// bounded contract" was sent to re-check a spelling that was never wrong.
/// The refusal has to name the selection, because only moving the selection
/// can make the call succeed.
pub(crate) fn project_not_selected() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "Survey control runs against the selected project, and this command names a different one: \
         select that project first with `ds auth project use --project <id>`",
    )
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
    pub fn validate(&self) -> Result<(), ClientError> {
        kernel_command(self).and_then(|command| command.validate().map_err(|_| invalid()))
    }
}

/// Only this private enum can select a Survey HTTP route.
pub(crate) enum SurveyRoute {
    Factory,
    ProjectForms,
    Templates,
    Template(String),
    Projects,
    FromTemplate,
}
impl SurveyRoute {
    pub(crate) fn generated_operation(&self) -> Option<ds_gateway_client::Operation> {
        match self {
            Self::Factory => Some(ds_gateway_client::Operation::FormFactoryV2),
            Self::ProjectForms => Some(ds_gateway_client::Operation::ProjectForms),
            Self::Templates => Some(ds_gateway_client::Operation::TemplatesAction),
            Self::Projects => Some(ds_gateway_client::Operation::ProjectsAction),
            Self::FromTemplate => Some(ds_gateway_client::Operation::ProjectFromTemplate),
            Self::Template(_) => None,
        }
    }

    pub(crate) fn path(&self) -> String {
        match self {
            Self::Factory => ds_gateway_client::Operation::FormFactoryV2
                .path_template()
                .into(),
            Self::ProjectForms => ds_gateway_client::Operation::ProjectForms
                .path_template()
                .into(),
            Self::Templates => ds_gateway_client::Operation::TemplatesAction
                .path_template()
                .into(),
            Self::Projects => ds_gateway_client::Operation::ProjectsAction
                .path_template()
                .into(),
            Self::FromTemplate => ds_gateway_client::Operation::ProjectFromTemplate
                .path_template()
                .into(),
            Self::Template(slug) => format!("/api/v1/projects/templates/{slug}"),
        }
    }
    pub(crate) fn method(&self) -> &'static str {
        self.generated_operation()
            .map_or("GET", |operation| operation.method().as_str())
    }
}
pub(crate) fn response(response: crate::TransportResponse) -> Result<Value, ClientError> {
    kernel::response(&response.body).map_err(|_| unreadable())
}
pub(crate) fn form_summary(form: &Value, detail: bool) -> Value {
    kernel::form_summary(form, detail)
}
pub(crate) fn page_form(form: Value, request: &FormRead) -> Result<Value, ClientError> {
    let request = serde_json::from_value(
        serde_json::to_value(request).expect("Survey form-read request serializes"),
    )
    .map_err(|_| invalid())?;
    kernel::page_form(form, &request).map_err(|_| unreadable())
}

fn kernel_command(
    command: &SurveyControlCommand,
) -> Result<kernel::SurveyControlCommand, ClientError> {
    serde_json::to_value(command)
        .map_err(|_| invalid())
        .and_then(|value| serde_json::from_value(value).map_err(|_| invalid()))
}
