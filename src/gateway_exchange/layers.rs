//! The project's layer document: the map's sources, layers, styles and drawer
//! groups, the Style Center's catalogue, and the canonical layer order.
//! [`crate::layer_document`] owns each request body and what an answer must
//! hold; these operations send them and admit the answer.

use serde_json::Value;

use super::{Admitted, Exchange, Plan, Refusal, Request};
use crate::layer_document::{self, Action, Order};

pub const INPUT_INVALID: &str = "layers_input_invalid";
/// Every client reads the document assembled for the web map.
const PLATFORM: &str = "web";

/// One layer action for one project.
#[derive(Clone, Debug)]
pub struct LayerAction {
    pub action: Action,
    pub project: String,
    /// The canonical order rows a `reorder` writes; empty otherwise.
    pub orders: Vec<Order>,
}

impl Exchange for LayerAction {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        if self.action == Action::SetDefaultVisibility {
            return Err(Refusal::new(
                INPUT_INVALID,
                "Default visibility is set through the map defaults.",
            ));
        }
        let body = layer_document::request_body(self.action, &self.project, &self.orders, PLATFORM)
            .map_err(|message| Refusal::new(INPUT_INVALID, message))?;
        Ok(Request::post(layer_document::PATH, body)
            .timeout_ms(layer_document::TIMEOUT_SECONDS * 1000)
            .response_cap(self.action.response_limit() as u64)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let admitted = match self.action {
            Action::GetConfig | Action::Refresh => {
                layer_document::admit_document(&self.project, &body)
            }
            Action::GetStyleCatalog => layer_document::admit_style_catalog(&self.project, &body),
            Action::Reorder | Action::SetDefaultVisibility => {
                layer_document::admit_order_receipt(&self.project, self.orders.len(), &body)
            }
        };
        admitted.map(Admitted::Done).map_err(|refusal| Refusal {
            details: (!refusal.issues.is_empty()).then(|| serde_json::json!(refusal.issues)),
            ..Refusal::new(refusal.code, refusal.message)
        })
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// The typed browser exports Svelte calls; each runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{Action, LayerAction, Order};
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};
    use wasm_bindgen::prelude::*;

    /// Save canonical layer order overrides.
    #[wasm_bindgen(js_name = layersReorder, unchecked_return_type = "{ project_id: string; reordered: number }")]
    pub async fn layers_reorder(
        host: GatewayHost,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "{ layer_id: string; order: number }[]")] orders: JsValue,
    ) -> Result<JsValue, JsValue> {
        let orders: Vec<Order> = tsify::serde_wasm_bindgen::from_value(orders)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        let call = LayerAction {
            action: Action::Reorder,
            project,
            orders,
        };
        typed(&run(&host, call).await?)
    }
}

#[cfg(test)]
#[path = "layers_tests.rs"]
mod tests;
