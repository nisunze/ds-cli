//! Projects: the signed-in account's project directory, and one action of
//! Brain's closed project vocabularies — a project's lifecycle and forms, its
//! templates, its members and grants, and archiving it. Brain authorizes and
//! applies each action; these operations admit only actions Brain serves and
//! answer Brain's `data` as served.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, Admitted, Exchange, Method, Plan, Refusal, Request};

const DIRECTORY_PATH: &str = "/api/v1/user/projects";
/// A large account's directory.
const DIRECTORY_CAP: u64 = 16 * 1024 * 1024;
/// Applying a template or copying members can take minutes.
const ACTION_TIMEOUT_MS: u64 = 120_000;
const ACTION_CAP: u64 = 16 * 1024 * 1024;
pub const INPUT_INVALID: &str = "projects_input_invalid";
pub const INVALID: &str = "projects_answer_invalid";

/// A project's lifecycle state, as the directory filters it.
pub const STATUSES: [&str; 4] = ["active", "archived", "testing", "template"];

fn input_invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(INPUT_INVALID, message)
}

/// The account's projects in one lifecycle state, as Brain lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Directory {
    pub status: String,
}

impl Exchange for Directory {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        if !STATUSES.contains(&self.status.as_str()) {
            return Err(input_invalid(
                "A project list is active, archived, testing or template.",
            ));
        }
        let request = Request {
            method: Method::Get,
            body: None,
            query: vec![("status".into(), self.status.clone())],
            ..Request::post(DIRECTORY_PATH, Value::Null)
        }
        .response_cap(DIRECTORY_CAP);
        Ok(Plan {
            request,
            retries: 1,
        })
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        // A Brain that predates a lifecycle state answers it with its active
        // list; that list holds none of the asked state.
        if answer.get("status").and_then(Value::as_str) != Some(self.status.as_str())
            && self.status != "active"
        {
            return Ok(Admitted::Done(
                json!({"status": self.status, "projects": []}),
            ));
        }
        if !answer.get("projects").is_some_and(Value::is_array) {
            return Err(Refusal::new(
                INVALID,
                "The project directory answered no project list.",
            ));
        }
        Ok(Admitted::Done(answer.clone()))
    }
}

/// Which of Brain's project endpoints an action belongs to.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum ProjectEndpoint {
    /// A project's lifecycle, template application and forms.
    Projects,
    /// Reusable project templates.
    Templates,
    /// A project's members, form grants and Solar grants.
    Access,
    /// Archiving a project.
    Governance,
}

impl ProjectEndpoint {
    fn path(self) -> &'static str {
        match self {
            Self::Projects => "/api/v1/projects",
            Self::Templates => "/api/v1/projects/templates",
            Self::Access => "/api/v1/projects/access",
            Self::Governance => "/api/v1/governance",
        }
    }

    /// The actions Brain serves on the endpoint.
    pub fn actions(self) -> &'static [&'static str] {
        match self {
            Self::Projects => &[
                "create",
                "update",
                "delete",
                "apply_template",
                "enable_form",
                "disable_form",
                "get_dataset_info",
            ],
            Self::Templates => &["create", "list", "get", "set_public", "delete"],
            Self::Access => &[
                "list_users",
                "share_project",
                "revoke_project",
                "bulk_members",
                "copy_users",
                "member_form_grant_read",
                "member_form_grant_plan",
                "member_form_grant_apply",
                "share_form",
                "revoke_form",
                "share_solar",
                "revoke_solar",
                "list_solar_users",
            ],
            Self::Governance => &["archive_project"],
        }
    }

    fn read(self, action: &str) -> bool {
        matches!(
            (self, action),
            (Self::Projects, "get_dataset_info")
                | (Self::Templates, "list" | "get")
                | (
                    Self::Access,
                    "list_users"
                        | "member_form_grant_read"
                        | "member_form_grant_plan"
                        | "list_solar_users"
                )
        )
    }
}

/// One action on one of Brain's project endpoints.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectAction {
    pub endpoint: ProjectEndpoint,
    pub action: String,
    pub data: Map<String, Value>,
}

impl Exchange for ProjectAction {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        if !self.endpoint.actions().contains(&self.action.as_str()) {
            return Err(input_invalid(format!(
                "`{}` is not an action of this project endpoint.",
                self.action
            )));
        }
        let fields: Map<String, Value> = self
            .data
            .iter()
            .filter(|(_, value)| !value.is_null())
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        for key in [
            "project_id",
            "from_project_id",
            "to_project_id",
            "source_project_id",
        ] {
            if let Some(project) = fields.get(key) {
                let valid = project
                    .as_str()
                    .is_some_and(crate::execution_context::valid_project);
                if !valid {
                    return Err(Refusal::new(
                        super::PROJECT_INVALID,
                        "A request names one valid project.",
                    ));
                }
            }
        }
        let body = json!({"action": self.action, "data": Value::Object(fields)});
        let request = Request::post(self.endpoint.path(), body)
            .timeout_ms(ACTION_TIMEOUT_MS)
            .response_cap(ACTION_CAP);
        Ok(Plan {
            request,
            retries: u32::from(self.endpoint.read(&self.action)),
        })
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        Ok(Admitted::Done(data(&body).clone()))
    }

    fn project(&self) -> Option<&str> {
        self.data.get("project_id").and_then(Value::as_str)
    }
}

/// The typed browser exports Svelte calls; each runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{Directory, ProjectAction, ProjectEndpoint};
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};
    use serde_json::{Map, Value};
    use wasm_bindgen::prelude::*;

    fn unreadable(error: impl std::fmt::Display) -> JsValue {
        js_sys::Error::new(&error.to_string()).into()
    }

    /// The account's projects in one lifecycle state, as Brain lists them.
    #[wasm_bindgen(js_name = projectsDirectory, unchecked_return_type = "Record<string, unknown>")]
    pub async fn projects_directory(host: GatewayHost, status: String) -> Result<JsValue, JsValue> {
        typed(&run(&host, Directory { status }).await?)
    }

    /// One action on one of Brain's project endpoints; answers Brain's `data`.
    #[wasm_bindgen(js_name = projectsAction, unchecked_return_type = "Record<string, unknown>")]
    pub async fn projects_action(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "ProjectEndpoint")] endpoint: JsValue,
        action: String,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] data: JsValue,
    ) -> Result<JsValue, JsValue> {
        let endpoint: ProjectEndpoint =
            tsify::serde_wasm_bindgen::from_value(endpoint).map_err(unreadable)?;
        let data: Map<String, Value> =
            tsify::serde_wasm_bindgen::from_value(data).map_err(unreadable)?;
        typed(
            &run(
                &host,
                ProjectAction {
                    endpoint,
                    action,
                    data,
                },
            )
            .await?,
        )
    }
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;
