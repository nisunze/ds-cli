//! The report job receipts the Notification Center shows, held per account so
//! a report job's row survives a reload, offline too. A run starts only once
//! its receipt is held: a store that cannot keep it refuses.

use serde_json::Value;

use super::retained::{self, Edge};
use super::Refusal;
use crate::retained_reads::Kind;

const KIND: Kind = Kind::ReportJobReceipts;
/// As many rows as the Notification Center keeps.
pub const MAX_RECEIPTS: usize = 200;
pub const INVALID: &str = "report_receipts_invalid";

/// The receipts this edge holds for the signed-in account, newest first.
pub async fn held(edge: &impl Edge) -> Result<Vec<Value>, Refusal> {
    let observed = retained::read::<Vec<Value>>(edge, KIND, None, "").await?;
    Ok(observed.held.map(|held| held.value).unwrap_or_default())
}

/// Hold these receipts, each a row naming its `id`, at most
/// [`MAX_RECEIPTS`]. The store's refusal is the caller's.
pub async fn hold(edge: &impl Edge, receipts: Vec<Value>) -> Result<(), Refusal> {
    let valid = receipts.len() <= MAX_RECEIPTS
        && receipts.iter().all(|row| {
            row.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.is_empty())
        });
    if !valid {
        return Err(Refusal::new(
            INVALID,
            "Report receipts are at most 200 rows that each name an id.",
        ));
    }
    let observed = retained::read::<Vec<Value>>(edge, KIND, None, "").await?;
    retained::write(edge, observed.owner.as_deref(), KIND, None, "", &receipts, None).await
}

/// The typed browser exports Svelte calls.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use serde_json::Value;
    use wasm_bindgen::prelude::*;

    /// The report job receipts this edge holds for the signed-in account.
    #[wasm_bindgen(js_name = reportJobReceiptsHeld, unchecked_return_type = "Record<string, unknown>[]")]
    pub async fn held(host: GatewayHost, store: StoreHost) -> Result<JsValue, JsValue> {
        let held = super::held(&BrowserEdge::new(host, store)).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// Hold the account's report job receipts; rejects when they are not held.
    #[wasm_bindgen(js_name = reportJobReceiptsHold)]
    pub async fn hold(
        host: GatewayHost,
        store: StoreHost,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>[]")] receipts: JsValue,
    ) -> Result<(), JsValue> {
        let receipts: Vec<Value> = tsify::serde_wasm_bindgen::from_value(receipts)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        super::hold(&BrowserEdge::new(host, store), receipts)
            .await
            .map_err(|refusal| rejection(&refusal))
    }
}

#[cfg(test)]
#[path = "report_receipts_tests.rs"]
mod tests;
