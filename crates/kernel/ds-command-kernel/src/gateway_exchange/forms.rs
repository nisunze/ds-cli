//! Survey forms: one request of the Form Factory (the global form schemas and
//! their governance) or of a project's form bindings and settings, admitted
//! only when Brain serves its action. Brain validates every schema, setting
//! and grant; these requests answer Brain's `data` as served.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};

/// A cold Form Factory scan reads every project's forms.
const TIMEOUT_MS: u64 = 120_000;
const RESPONSE_CAP: u64 = 32 * 1024 * 1024;
pub const INPUT_INVALID: &str = "forms_input_invalid";
pub const ANSWER_INVALID: &str = "forms_answer_invalid";

/// Which of Brain's form endpoints a request belongs to.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum FormsEndpoint {
    /// The global Form Factory.
    FormFactory,
    /// One project's form bindings and settings.
    ProjectForms,
}

impl FormsEndpoint {
    fn path(self) -> &'static str {
        match self {
            Self::FormFactory => "/api/v1/form-factory",
            Self::ProjectForms => "/api/v1/project-forms",
        }
    }

    /// The actions Brain serves on the endpoint.
    pub fn actions(self) -> &'static [&'static str] {
        match self {
            Self::FormFactory => &[
                "list",
                "get",
                "create",
                "update",
                "duplicate",
                "archive",
                "restore",
                "delete",
                "publish",
                "unpublish",
                "purge_plan",
                "purge_candidates",
                "purge",
                "resolve_project",
                "get_context_forms",
                "get_field_types",
                "get_governance",
                "audit_orphans",
                "user_exists",
                "get_forms_index",
                "settings_editor",
                "set_active_project",
                "get_dashboard",
                "get_project_members",
            ],
            Self::ProjectForms => &["activate", "settings_editor", "bulk_save", "diagnose"],
        }
    }

    /// Whether this device keeps the answer: every read, and a project's form
    /// activation (the form set a project opens with).
    pub fn kept(self, action: &str) -> bool {
        self.read(action) || (self == Self::ProjectForms && action == "activate")
    }

    fn read(self, action: &str) -> bool {
        match self {
            Self::FormFactory => matches!(
                action,
                "list"
                    | "get"
                    | "purge_plan"
                    | "purge_candidates"
                    | "resolve_project"
                    | "get_context_forms"
                    | "get_field_types"
                    | "get_governance"
                    | "audit_orphans"
                    | "user_exists"
                    | "get_forms_index"
                    | "settings_editor"
                    | "get_dashboard"
                    | "get_project_members"
            ),
            Self::ProjectForms => matches!(action, "settings_editor" | "diagnose"),
        }
    }
}

/// The export the page calls, and the exchange it runs, for
/// [`crate::capabilities::admit_export`].
pub const EXPORT: &str = "formsRequest";
pub const EXCHANGE: &str = "forms";

/// The capability one action of one form endpoint performs, or `None` for
/// an action no row declares, which the page never runs.
pub fn capability(endpoint: FormsEndpoint, action: &str) -> Option<&'static str> {
    Some(match (endpoint, action) {
        (FormsEndpoint::FormFactory, "list") => "survey.forms.list",
        (FormsEndpoint::FormFactory, "get") => "survey.form.read",
        (FormsEndpoint::FormFactory, "get_field_types") => "survey.form.types",
        (FormsEndpoint::FormFactory, "create") => "survey.form.create",
        (FormsEndpoint::FormFactory, "update") => "survey.form.update",
        (
            FormsEndpoint::FormFactory,
            "publish" | "unpublish" | "duplicate" | "archive" | "restore" | "delete",
        ) => "survey.form.lifecycle",
        (FormsEndpoint::FormFactory, "purge_plan" | "purge_candidates") => "survey.form.purge-plan",
        (FormsEndpoint::FormFactory, "purge") => "survey.form.purge",
        (FormsEndpoint::ProjectForms, "activate") => "survey.project-forms.read",
        (FormsEndpoint::ProjectForms, "settings_editor") => "survey.project-form.editor",
        (FormsEndpoint::ProjectForms, "bulk_save") => "survey.project-forms.apply",
        _ => return None,
    })
}

/// Admit one request from the page: its capability's row must name
/// [`EXPORT`] and be supported on this build's platform.
pub fn admit_in_page(
    platform: &str,
    endpoint: FormsEndpoint,
    request: &Map<String, Value>,
) -> Result<&'static str, Refusal> {
    let action = action(request);
    let id = capability(endpoint, action).ok_or_else(|| {
        Refusal::new(
            INPUT_INVALID,
            format!("`{action}` is not a form action the page runs."),
        )
    })?;
    crate::capabilities::admit_export_on(platform, id, EXPORT, EXCHANGE)
        .map(|capability| capability.id)
        .map_err(|refused| Refusal::new(refused.code, refused.message))
}

/// One request of one form endpoint.
#[derive(Clone, Debug, PartialEq)]
pub struct Forms {
    pub endpoint: FormsEndpoint,
    pub request: Map<String, Value>,
}

impl Exchange for Forms {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        let action = self
            .request
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !self.endpoint.actions().contains(&action) {
            return Err(Refusal::new(
                INPUT_INVALID,
                format!("`{action}` is not an action of this form endpoint."),
            ));
        }
        if let Some(project) = self
            .request
            .get("project_id")
            .filter(|value| !value.is_null())
        {
            project_reference(project.as_str().unwrap_or_default())?;
        } else if self.endpoint == FormsEndpoint::ProjectForms {
            return Err(Refusal::new(
                INPUT_INVALID,
                "A project form request names its project.",
            ));
        }
        let body = Value::Object(
            self.request
                .iter()
                .filter(|(_, value)| !value.is_null())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        let request = Request::post(self.endpoint.path(), body)
            .timeout_ms(TIMEOUT_MS)
            .response_cap(RESPONSE_CAP);
        Ok(Plan {
            request,
            retries: u32::from(self.endpoint.read(action)),
        })
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        admit_value(self.endpoint, &self.request, data(&body).clone()).map(Admitted::Done)
    }

    fn project(&self) -> Option<&str> {
        self.request.get("project_id").and_then(Value::as_str)
    }
}

fn admit_value(
    endpoint: FormsEndpoint,
    request: &Map<String, Value>,
    mut value: Value,
) -> Result<Value, Refusal> {
    if value["status"] == "error"
        || value["success"] == false
        || value.get("error").is_some_and(|error| {
            error.is_object() || error.as_str().is_some_and(|text| !text.trim().is_empty())
        })
    {
        let mut refusal = Refusal::new(
            &super::backend_code(&value).unwrap_or_else(|| "forms_refused".into()),
            super::backend_message(200, &value),
        );
        refusal.details = super::backend_details(&value);
        return Err(refusal);
    }
    if endpoint == FormsEndpoint::ProjectForms && action(request) == "activate" {
        activation(
            &value,
            request
                .get("project_id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            false,
            false,
        )?;
    }
    if endpoint == FormsEndpoint::FormFactory {
        match action(request) {
            "list" if !value["forms"].is_array() => {
                return Err(Refusal::new(
                    ANSWER_INVALID,
                    "Form Factory returned no authoritative forms array.",
                ))
            }
            "get" if value["form"]["slug"] != request["slug"] => {
                return Err(Refusal::new(
                    ANSWER_INVALID,
                    "Form Factory did not return the requested form.",
                ))
            }
            "get_field_types" => {
                if !value["field_types"].is_array() {
                    return Err(Refusal::new(
                        ANSWER_INVALID,
                        "Form Factory did not return its field-type registry.",
                    ));
                }
                if !value["condition_operators"].is_array() {
                    value["condition_operators"] = serde_json::json!([]);
                }
                if !value["type_aliases"].is_object() {
                    value["type_aliases"] = serde_json::json!({});
                }
            }
            _ => {}
        }
    }
    Ok(value)
}

/// The key a form read is kept under: the digest of its endpoint and request.
#[cfg(feature = "durable-store")]
fn kept_key(endpoint: FormsEndpoint, request: &Map<String, Value>) -> Result<String, Refusal> {
    let request: Map<String, Value> = request
        .iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let identity = serde_json::json!({"endpoint": endpoint, "request": request});
    crate::report_export::jcs::sha256(&identity).map_err(|error| Refusal::new(INPUT_INVALID, error))
}

fn action(request: &Map<String, Value>) -> &str {
    request
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// Why this read is wanted. A restored selection uses only what is held;
/// navigation opens it offline first; Refresh is an explicit fresh request.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadMode {
    Held,
    Open,
    Refresh,
}

/// The one acquired read and whether a later accepted change made it stale.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct AcquiredForms {
    pub value: Value,
    pub stale: bool,
    pub held: bool,
}

#[cfg(feature = "durable-store")]
pub async fn acquire(
    edge: &impl super::retained::Edge,
    endpoint: FormsEndpoint,
    body: Map<String, Value>,
    mode: ReadMode,
) -> Result<Option<AcquiredForms>, Refusal> {
    if !endpoint.kept(action(&body)) {
        return Err(Refusal::new(
            INPUT_INVALID,
            "Only a form read can be acquired.",
        ));
    }
    if !matches!(mode, ReadMode::Refresh) {
        if let Some(held) = held(edge, endpoint, &body).await? {
            return Ok(Some(AcquiredForms {
                value: held.value,
                stale: held.stale,
                held: true,
            }));
        }
        if matches!(mode, ReadMode::Held) {
            return Ok(None);
        }
    }
    Ok(Some(AcquiredForms {
        value: request(edge, endpoint, body).await?,
        stale: false,
        held: false,
    }))
}

fn activation(value: &Value, project: &str, stale: bool, held: bool) -> Result<Value, Refusal> {
    if value["project_id"] != project {
        let actual = value
            .get("project_id")
            .map(Value::to_string)
            .unwrap_or_else(|| "undefined".into());
        return Err(Refusal::new(
            ANSWER_INVALID,
            format!("Project forms activation returned project {actual}, expected {project}"),
        ));
    }
    if !value["forms"].is_array() {
        return Err(Refusal::new(
            ANSWER_INVALID,
            "Project forms activation returned no authoritative forms array",
        ));
    }
    let mut answer = serde_json::json!({"forms":value["forms"],"settings_editors":if held{serde_json::json!({})}else{value.get("settings_editors").cloned().unwrap_or_else(||serde_json::json!({}))},"cache_refresh_required":stale});
    for key in [
        "orphaned_project_forms",
        "field_types",
        "condition_operators",
    ] {
        if value[key].is_array() {
            answer[key] = value[key].clone();
        }
    }
    if value["type_aliases"].is_object() {
        answer["type_aliases"] = value["type_aliases"].clone();
    }
    Ok(answer)
}

/// An activation projected by the same owner for held restoration, navigation
/// and an explicit refresh. Held settings never authorize a mutation.
#[cfg(feature = "durable-store")]
pub async fn project_acquire(
    edge: &impl super::retained::Edge,
    project: String,
    mode: ReadMode,
) -> Result<Option<Value>, Refusal> {
    super::project_reference(&project)?;
    let body = Map::from_iter([
        ("action".into(), Value::String("activate".into())),
        ("project_id".into(), Value::String(project.clone())),
    ]);
    acquire(edge, FormsEndpoint::ProjectForms, body, mode)
        .await?
        .map(|answer| activation(&answer.value, &project, answer.stale, answer.held))
        .transpose()
}

/// One request of one form endpoint. A read is kept so the next open paints
/// it at once and offline; a change Brain accepted marks every kept form
/// read stale, to be refreshed before it is trusted.
#[cfg(feature = "durable-store")]
pub async fn request(
    edge: &impl super::retained::Edge,
    endpoint: FormsEndpoint,
    request: Map<String, Value>,
) -> Result<Value, Refusal> {
    let kind = crate::retained_reads::Kind::FormsRead;
    let call = Forms {
        endpoint,
        request: request.clone(),
    };
    if endpoint.kept(action(&request)) {
        let key = kept_key(endpoint, &request)?;
        return super::retained::keep_fresh(edge, kind, None, &key, call).await;
    }
    let answer = edge.fetch(call).await?;
    // Marking stale changes no value, so it binds to whoever is signed in.
    let _ = super::retained::invalidate(edge, None, kind, None, None).await;
    Ok(answer)
}

/// A kept form read, and whether a change since made it stale.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct HeldForms {
    pub value: Value,
    pub stale: bool,
}

/// What this device holds of one form read, with no transport.
#[cfg(feature = "durable-store")]
pub async fn held(
    edge: &impl super::retained::Edge,
    endpoint: FormsEndpoint,
    request: &Map<String, Value>,
) -> Result<Option<HeldForms>, Refusal> {
    if !endpoint.kept(action(request)) {
        return Err(Refusal::new(INPUT_INVALID, "Only a form read is held."));
    }
    let kind = crate::retained_reads::Kind::FormsRead;
    let key = kept_key(endpoint, request)?;
    let observed = super::retained::read::<Value>(edge, kind, None, &key).await?;
    observed
        .held
        .map(|held| {
            Ok(HeldForms {
                value: admit_value(endpoint, request, held.value)?,
                stale: held.invalidated,
            })
        })
        .transpose()
}

/// The typed browser export Svelte calls; it runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{acquire, held, project_acquire, request, FormsEndpoint, ReadMode};
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use serde_json::{Map, Value};
    use wasm_bindgen::prelude::*;

    fn unreadable(error: impl std::fmt::Display) -> JsValue {
        js_sys::Error::new(&error.to_string()).into()
    }

    fn parts(
        endpoint: JsValue,
        request: JsValue,
    ) -> Result<(FormsEndpoint, Map<String, Value>), JsValue> {
        Ok((
            tsify::serde_wasm_bindgen::from_value(endpoint).map_err(unreadable)?,
            tsify::serde_wasm_bindgen::from_value(request).map_err(unreadable)?,
        ))
    }

    #[wasm_bindgen(js_name = formsAcquire, unchecked_return_type = "{ value: Record<string, unknown>; stale: boolean; held: boolean } | undefined")]
    pub async fn forms_acquire(
        host: GatewayHost,
        store: StoreHost,
        endpoint: JsValue,
        body: JsValue,
        mode: JsValue,
    ) -> Result<JsValue, JsValue> {
        let (endpoint, body) = parts(endpoint, body)?;
        let mode: ReadMode = tsify::serde_wasm_bindgen::from_value(mode).map_err(unreadable)?;
        typed(
            &acquire(&BrowserEdge::new(host, store), endpoint, body, mode)
                .await
                .map_err(|refusal| rejection(&refusal))?,
        )
    }

    #[wasm_bindgen(js_name = projectFormsAcquire, unchecked_return_type = "Record<string, unknown> | undefined")]
    pub async fn project_forms_acquire(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        mode: JsValue,
    ) -> Result<JsValue, JsValue> {
        let mode: ReadMode = tsify::serde_wasm_bindgen::from_value(mode).map_err(unreadable)?;
        typed(
            &project_acquire(&BrowserEdge::new(host, store), project, mode)
                .await
                .map_err(|refusal| rejection(&refusal))?,
        )
    }

    /// One request of one form endpoint; answers Brain's `data` as served. A
    /// read is kept for the next open; a change marks kept reads stale.
    #[wasm_bindgen(js_name = formsRequest, unchecked_return_type = "Record<string, unknown>")]
    pub async fn forms_request(
        host: GatewayHost,
        store: StoreHost,
        #[wasm_bindgen(unchecked_param_type = "FormsEndpoint")] endpoint: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] body: JsValue,
    ) -> Result<JsValue, JsValue> {
        let (endpoint, body) = parts(endpoint, body)?;
        super::admit_in_page(crate::capabilities::PLATFORM, endpoint, &body)
            .map_err(|refusal| rejection(&refusal))?;
        let edge = BrowserEdge::new(host, store);
        typed(
            &request(&edge, endpoint, body)
                .await
                .map_err(|refusal| rejection(&refusal))?,
        )
    }

    /// What this device holds of one form read, at once and offline; `stale`
    /// after a change since.
    #[wasm_bindgen(
        js_name = formsHeld,
        unchecked_return_type = "{ value: Record<string, unknown>; stale: boolean } | undefined"
    )]
    pub async fn forms_held(
        host: GatewayHost,
        store: StoreHost,
        #[wasm_bindgen(unchecked_param_type = "FormsEndpoint")] endpoint: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] body: JsValue,
    ) -> Result<JsValue, JsValue> {
        let (endpoint, body) = parts(endpoint, body)?;
        let edge = BrowserEdge::new(host, store);
        typed(
            &held(&edge, endpoint, &body)
                .await
                .map_err(|refusal| rejection(&refusal))?,
        )
    }
}

#[cfg(test)]
#[path = "forms_tests.rs"]
mod tests;
