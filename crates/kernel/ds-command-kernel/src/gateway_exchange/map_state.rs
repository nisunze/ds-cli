//! A project's map state: the resolved layer document the page last applied,
//! held on this device so the next open paints at once and offline. The page
//! keeps each state it applies; a held state answers with when it was saved
//! and whether something invalidated it since, and the page decides whether
//! to revalidate. Another account's state reads as absent.

use serde::Serialize;
use serde_json::Value;

use super::retained::{self, Edge};
use super::{project_reference, Refusal};
use crate::retained_reads::{Kind, INVALID};

pub const KIND: Kind = Kind::MapState;

/// A held map state and its facts.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct HeldState {
    pub value: Value,
    pub saved_at_ms: u64,
    pub invalidated: bool,
}

/// What this device holds for the project, if it still reads.
pub async fn held(edge: &impl Edge, project: &str) -> Result<Option<HeldState>, Refusal> {
    project_reference(project)?;
    let observed = retained::read::<Value>(edge, KIND, Some(project), "").await?;
    Ok(observed.held.map(|held| HeldState {
        value: held.value,
        saved_at_ms: held.saved_at_ms,
        invalidated: held.invalidated,
    }))
}

/// Keep the state the page applied, under the account the store observes now.
pub async fn keep(edge: &impl Edge, project: &str, state: &Value) -> Result<(), Refusal> {
    project_reference(project)?;
    if state.get("project_id").and_then(Value::as_str) != Some(project) {
        return Err(Refusal::new(
            INVALID,
            "A map state is kept under its own project.",
        ));
    }
    let observed = retained::read::<Value>(edge, KIND, Some(project), "").await?;
    retained::write(
        edge,
        observed.owner.as_deref(),
        KIND,
        Some(project),
        "",
        state,
        None,
    )
    .await
}

/// Forget the project's state, as when its survey data moved away.
pub async fn forget(edge: &impl Edge, project: &str) -> Result<(), Refusal> {
    project_reference(project)?;
    let observed = retained::read::<Value>(edge, KIND, Some(project), "").await?;
    retained::forget(edge, observed.owner.as_deref(), KIND, Some(project), None).await
}

/// The typed browser exports Svelte calls.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use serde_json::Value;
    use wasm_bindgen::prelude::*;

    /// The map state this device holds for a project, or null.
    #[wasm_bindgen(
        js_name = mapStateHeld,
        unchecked_return_type = "{ value: Record<string, unknown>; saved_at_ms: number; invalidated: boolean } | null"
    )]
    pub async fn map_state_held(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let held = super::held(&BrowserEdge::new(host, store), &project).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// Keep the map state the page applied.
    #[wasm_bindgen(js_name = mapStateKeep)]
    pub async fn map_state_keep(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] state: JsValue,
    ) -> Result<(), JsValue> {
        let state: Value = tsify::serde_wasm_bindgen::from_value(state)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        super::keep(&BrowserEdge::new(host, store), &project, &state)
            .await
            .map_err(|refusal| rejection(&refusal))
    }

    /// Forget a project's held map state.
    #[wasm_bindgen(js_name = mapStateForget)]
    pub async fn map_state_forget(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<(), JsValue> {
        super::forget(&BrowserEdge::new(host, store), &project)
            .await
            .map_err(|refusal| rejection(&refusal))
    }
}

#[cfg(test)]
#[path = "map_state_tests.rs"]
mod tests;
