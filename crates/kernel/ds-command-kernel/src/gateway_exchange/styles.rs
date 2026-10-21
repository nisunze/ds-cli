//! The style family's four Brain actions: publish one authored document, a
//! starter model for a ref with no document, one shared-class document, and
//! one work-map runtime family. [`crate::style_document`] owns each request
//! body and what an answer must hold; these operations send them and admit
//! the answer against what was sent.

use serde_json::Value;

use super::{Admitted, Exchange, Plan, Refusal, Request};
use crate::style_document::{self, Action, PATH, TIMEOUT_SECONDS};

pub const INPUT_INVALID: &str = "styles_input_invalid";

/// One style action and its input, as `style_document` names them.
#[derive(Clone, Debug, PartialEq)]
pub enum StyleAction {
    Publish {
        payload: Value,
    },
    Seed {
        style_ref: String,
        geometry: String,
        seed_kind: Option<String>,
        seed: Option<String>,
    },
    ReadShared {
        style_ref: String,
    },
    AssembleRuntime {
        logical_root: String,
        geometry: String,
        style_data: Value,
    },
}

impl StyleAction {
    fn action(&self) -> Action {
        match self {
            Self::Publish { .. } => Action::Publish,
            Self::Seed { .. } => Action::Seed,
            Self::ReadShared { .. } => Action::ReadShared,
            Self::AssembleRuntime { .. } => Action::AssembleRuntime,
        }
    }

    fn body(&self) -> Result<Value, String> {
        match self {
            Self::Publish { payload } => style_document::publish_body(payload),
            Self::Seed {
                style_ref,
                geometry,
                seed_kind,
                seed,
            } => style_document::seed_body(
                style_ref,
                geometry,
                seed_kind.as_deref(),
                seed.as_deref(),
            ),
            Self::ReadShared { style_ref } => style_document::read_shared_body(style_ref),
            Self::AssembleRuntime {
                logical_root,
                geometry,
                style_data,
            } => style_document::assemble_runtime_body(logical_root, geometry, style_data),
        }
    }
}

impl Exchange for StyleAction {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = self
            .body()
            .map_err(|message| Refusal::new(INPUT_INVALID, message))?;
        Ok(Request::post(PATH, body)
            .timeout_ms(TIMEOUT_SECONDS * 1000)
            .response_cap(self.action().response_limit() as u64)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        // The answer is held to what was sent: the body this call built.
        let sent = self
            .body()
            .map_err(|message| Refusal::new(INPUT_INVALID, message))?;
        let expected = sent.get("data").cloned().unwrap_or(Value::Null);
        let admitted = match self.action() {
            Action::Publish => style_document::admit_publish(&expected, &body),
            Action::Seed => style_document::admit_seed(&expected, &body),
            Action::ReadShared => style_document::admit_read_shared(&expected, &body),
            Action::AssembleRuntime => style_document::admit_runtime(&expected, &body),
        };
        admitted.map(Admitted::Done).map_err(|refusal| Refusal {
            details: (!refusal.issues.is_empty()).then(|| serde_json::json!(refusal.issues)),
            ..Refusal::new(refusal.code, refusal.message)
        })
    }
}

/// A style refusal's details as the issues it names, in text: the backend's
/// per-property complaints, or an admission's. Other details are dropped.
pub fn with_issues(mut refusal: Refusal) -> Refusal {
    if let Some(details) = refusal.details.take() {
        let issues = if details.is_array() {
            details
        } else {
            serde_json::json!(style_document::backend_issues(&details))
        };
        refusal.details = issues
            .as_array()
            .is_some_and(|issues| !issues.is_empty())
            .then_some(issues);
    }
    refusal
}

/// The typed browser exports Svelte calls; each runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{with_issues, StyleAction};
    use crate::gateway_exchange::browser::{attempt, rejection, typed, GatewayHost};
    use serde_json::Value;
    use wasm_bindgen::prelude::*;

    fn json(value: JsValue) -> Result<Value, JsValue> {
        tsify::serde_wasm_bindgen::from_value(value)
            .map_err(|error| js_sys::Error::new(&error.to_string()).into())
    }

    async fn act(host: GatewayHost, call: StyleAction) -> Result<JsValue, JsValue> {
        match attempt(&host, call).await? {
            Ok(answer) => typed(&answer),
            Err(refusal) => Err(rejection(&with_issues(refusal))),
        }
    }

    /// Publish one authored style document; answers the receipt Brain kept.
    #[wasm_bindgen(js_name = stylesPublish, unchecked_return_type = "Record<string, unknown>")]
    pub async fn styles_publish(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] payload: JsValue,
    ) -> Result<JsValue, JsValue> {
        act(host, StyleAction::Publish { payload: json(payload)? }).await
    }

    /// A starter style for a ref with no document; nothing is persisted.
    #[wasm_bindgen(js_name = stylesSeed, unchecked_return_type = "Record<string, unknown>")]
    pub async fn styles_seed(
        host: GatewayHost,
        style_ref: String,
        geometry: String,
        seed_kind: Option<String>,
        seed: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let call = StyleAction::Seed {
            style_ref,
            geometry,
            seed_kind,
            seed,
        };
        act(host, call).await
    }

    /// One shared-class document as Brain holds it.
    #[wasm_bindgen(js_name = stylesReadShared, unchecked_return_type = "Record<string, unknown>")]
    pub async fn styles_read_shared(host: GatewayHost, style_ref: String) -> Result<JsValue, JsValue> {
        act(host, StyleAction::ReadShared { style_ref }).await
    }

    /// One work-map runtime family assembled from an authored document.
    #[wasm_bindgen(js_name = stylesAssembleRuntime, unchecked_return_type = "Record<string, unknown>")]
    pub async fn styles_assemble_runtime(
        host: GatewayHost,
        logical_root: String,
        geometry: String,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] style_data: JsValue,
    ) -> Result<JsValue, JsValue> {
        let call = StyleAction::AssembleRuntime {
            logical_root,
            geometry,
            style_data: json(style_data)?,
        };
        act(host, call).await
    }
}

#[cfg(test)]
#[path = "styles_tests.rs"]
mod tests;
