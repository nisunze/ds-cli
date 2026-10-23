//! The project model catalogue on the hosted Web page: the Kernel's catalogue
//! flow ([`crate::grid_project_catalog`]) runs here end to end, its store
//! steps on the page's store and its listing walk through the gateway
//! (`POST /api/v1/grid/models` `list_models`). The Desktop reads the same
//! catalogue through Core (`design.model.catalog`).

use ds_gateway_client::Operation as GatewayOperation;
use serde_json::{json, Value};

use super::retained::Edge;
use super::{Admitted, Exchange, Plan, Refusal, Request};
use crate::durable_store::{Call, Request as StoreRequest};
use crate::grid_project_catalog::{
    flow_resume, flow_start, FlowResult, FlowStep, Operation, ReadMode, Scope, INVALID,
};

/// A listing page holds at most 100 heads of at most 16 KiB.
const PAGE_CAP: u64 = 4 * 1024 * 1024;
/// The flow's own bound on steps: a walk of at most 64 pages and its writes.
const STEPS_MAX: usize = 160;

/// One listing page, the flow's body sent as it named it.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelsPage {
    pub body: Value,
    pub timeout_ms: u64,
}

impl Exchange for ModelsPage {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(Request::generated(
            GatewayOperation::ProjectGridModelsAction,
            Some(self.body.clone()),
        )?
        .timeout_ms(self.timeout_ms)
        .response_cap(PAGE_CAP)
        .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        Ok(Admitted::Done(body))
    }

    fn project(&self) -> Option<&str> {
        self.body.get("project_id").and_then(Value::as_str)
    }
}

fn flow(error: String) -> Refusal {
    Refusal::new(INVALID, error)
}

/// One typed member of a store answer, absent when null or unreadable.
fn member<T: serde::de::DeserializeOwned>(answer: &Value, key: &str) -> Option<T> {
    answer
        .get(key)
        .filter(|value| !value.is_null())
        .and_then(|value| serde_json::from_value(value.clone()).ok())
}

async fn store(edge: &impl Edge, operation: Operation) -> Result<Value, Refusal> {
    edge.store(Call {
        request: StoreRequest::GridProjectCatalog { operation },
        expect_owner: None,
    })
    .await
}

/// The catalogue as this page holds it (`Held`), or after one authorized
/// listing compared with it (`Refresh`).
pub async fn read(edge: &impl Edge, scope: Scope, mode: ReadMode) -> Result<Value, Refusal> {
    let mut step = flow_start(scope, mode).map_err(flow)?;
    for _ in 0..STEPS_MAX {
        step = match step {
            FlowStep::Done { catalog } => return Ok(catalog),
            FlowStep::Storage {
                operation,
                continuation,
            } => {
                let answer = store(edge, operation).await?;
                let result = FlowResult::Storage {
                    held: member(&answer, "held"),
                    changes: member(&answer, "changes"),
                };
                flow_resume(continuation, result).map_err(flow)?
            }
            FlowStep::Fetch {
                body,
                timeout_ms,
                continuation,
                ..
            } => {
                let result = match edge.fetch(ModelsPage { body, timeout_ms }).await {
                    Ok(answer) => FlowResult::Fetch {
                        status: 200,
                        body: answer,
                        now: edge.now_ms(),
                    },
                    Err(refusal) => match refusal.status {
                        Some(status) => FlowResult::Fetch {
                            status,
                            body: json!({}),
                            now: edge.now_ms(),
                        },
                        None => FlowResult::Failure,
                    },
                };
                flow_resume(continuation, result).map_err(flow)?
            }
        };
    }
    Err(Refusal::new(
        INVALID,
        "The project model catalogue exceeded its bounded step.",
    ))
}

/// Record the exact version the bytes owner verified and holds.
pub async fn retain(edge: &impl Edge, scope: Scope, version: Value) -> Result<(), Refusal> {
    let model_id = version
        .get("model_id")
        .and_then(Value::as_str)
        .ok_or_else(|| Refusal::new(INVALID, "A retained version names its model."))?
        .to_owned();
    store(
        edge,
        Operation::RetainVersion {
            scope,
            model_id,
            version,
        },
    )
    .await
    .map(drop)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{read, retain};
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use crate::grid_project_catalog::{ReadMode, Scope};
    use wasm_bindgen::prelude::*;

    fn input<T: serde::de::DeserializeOwned>(value: JsValue) -> Result<T, JsValue> {
        tsify::serde_wasm_bindgen::from_value(value)
            .map_err(|error| js_sys::Error::new(&error.to_string()).into())
    }

    /// The project's model catalogue on this page: `held` paints what the page
    /// holds; `refresh` lists the project once and compares.
    #[wasm_bindgen(js_name = gridCatalogRead, unchecked_return_type = "unknown")]
    pub async fn grid_project_catalog(
        host: GatewayHost,
        store: StoreHost,
        #[wasm_bindgen(unchecked_param_type = "{ principal: string; lane: string; audience: string; project: string }")] scope: JsValue,
        #[wasm_bindgen(unchecked_param_type = "'held' | 'refresh'")] mode: JsValue,
    ) -> Result<JsValue, JsValue> {
        let scope: Scope = input(scope)?;
        let mode: ReadMode = input(mode)?;
        let edge = BrowserEdge::new(host, store);
        typed(&read(&edge, scope, mode).await.map_err(|refusal| rejection(&refusal))?)
    }

    /// Record the exact version the bytes owner verified and holds.
    #[wasm_bindgen(js_name = gridCatalogRetain)]
    pub async fn grid_project_catalog_retain(
        host: GatewayHost,
        store: StoreHost,
        #[wasm_bindgen(unchecked_param_type = "{ principal: string; lane: string; audience: string; project: string }")] scope: JsValue,
        #[wasm_bindgen(unchecked_param_type = "{ model_id: string }")] version: JsValue,
    ) -> Result<(), JsValue> {
        let scope: Scope = input(scope)?;
        let version: serde_json::Value = input(version)?;
        let edge = BrowserEdge::new(host, store);
        retain(&edge, scope, version).await.map_err(|refusal| rejection(&refusal))
    }
}

#[cfg(test)]
#[path = "grid_catalog_tests.rs"]
mod tests;
