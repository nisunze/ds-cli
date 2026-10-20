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
        Ok(Admitted::Done(data(&body).clone()))
    }

    fn project(&self) -> Option<&str> {
        self.request.get("project_id").and_then(Value::as_str)
    }
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

#[cfg(feature = "durable-store")]
fn action(request: &Map<String, Value>) -> &str {
    request
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default()
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
    Ok(super::retained::kept(edge, kind, None, &key)
        .await
        .map(|held| HeldForms {
            value: held.value,
            stale: held.invalidated,
        }))
}

/// The typed browser export Svelte calls; it runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{held, request, FormsEndpoint};
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use serde_json::{Map, Value};
    use wasm_bindgen::prelude::*;

    fn unreadable(error: impl std::fmt::Display) -> JsValue {
        js_sys::Error::new(&error.to_string()).into()
    }

    fn parts(endpoint: JsValue, request: JsValue) -> Result<(FormsEndpoint, Map<String, Value>), JsValue> {
        Ok((
            tsify::serde_wasm_bindgen::from_value(endpoint).map_err(unreadable)?,
            tsify::serde_wasm_bindgen::from_value(request).map_err(unreadable)?,
        ))
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
