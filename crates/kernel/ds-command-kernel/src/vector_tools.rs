//! Vector tools — the page's host half. The catalogue, descriptors, typed
//! operations and reply checks are the Network contract
//! (`ds_network_contracts::vector_tools`); the runner runs in the governed
//! Network unit (`vectorTool` through the grid unit envelope). This module
//! builds the unit effect for one operation, with its layer bytes carried
//! beside the JSON as blobs, and admits the unit's reply. It links no
//! geometry algorithm.

pub use ds_network_contracts::vector_tools::*;

use crate::grid_engine::{self, GridArg, GridEffect, GridResult};
use serde_json::{json, Value};

/// The Network unit operation that runs a vector tool.
pub const UNIT_METHOD: &str = "vectorTool";

/// A refusal as the page reads it: the tool's own code, message and remedy,
/// or the envelope's.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorToolFault(pub Value);

impl std::fmt::Display for VectorToolFault {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&self.0.to_string())
    }
}

impl From<Refusal> for VectorToolFault {
    fn from(refusal: Refusal) -> Self {
        Self(json!(refusal))
    }
}

/// The unit effect for one operation: its lifted text, then its blobs.
pub fn effect(operation: &VectorToolOperation) -> Result<GridEffect, VectorToolFault> {
    let (text, blobs) = frame::lift(operation).map_err(request_invalid)?;
    let args = std::iter::once(GridArg::Text(text))
        .chain(blobs.into_iter().map(GridArg::Bytes))
        .collect();
    grid_engine::request("model", UNIT_METHOD, args)
        .map_err(|refusal| VectorToolFault(json!({"code":refusal.code,"message":refusal.message})))
}

/// Admit the unit's reply to this operation: the tool's refusal as it raised
/// it, or a reply in the declared shape for exactly this operation.
pub fn reply(
    operation: &VectorToolOperation,
    reply: &[u8],
) -> Result<VectorToolResponse, VectorToolFault> {
    let answered = grid_engine::decode("model", UNIT_METHOD, reply).map_err(|refusal| {
        let remedy = refusal
            .detail
            .as_deref()
            .and_then(|detail| serde_json::from_str::<Value>(detail).ok())
            .and_then(|detail| detail.get("remedy").cloned());
        let mut fault = json!({"code":refusal.code,"message":refusal.message});
        if let Some(remedy) = remedy {
            fault["remedy"] = remedy;
        }
        VectorToolFault(fault)
    })?;
    let GridResult::Artifact {
        bytes,
        receipt_json,
    } = answered
    else {
        return Err(response_invalid("the reply carries no lifted response").into());
    };
    let blobs = frame::unpack(&bytes).map_err(response_invalid)?;
    let response: VectorToolResponse =
        frame::lower(&receipt_json, blobs).map_err(response_invalid)?;
    validate_response(operation, &response)?;
    Ok(response)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    fn refusal(error: impl std::fmt::Display) -> JsError {
        JsError::new(&error.to_string())
    }

    #[wasm_bindgen(js_name = vectorToolCatalogue)]
    pub fn catalogue() -> Result<tsify::Ts<VectorToolCatalogue>, JsError> {
        super::catalogue()
            .map_err(refusal)?
            .into_ts()
            .map_err(|error| refusal(response_invalid(error)))
    }

    #[wasm_bindgen(js_name = vectorToolId)]
    pub fn tool(selector: &str) -> Result<tsify::Ts<VectorToolId>, JsError> {
        tool_id(selector)
            .map_err(refusal)?
            .into_ts()
            .map_err(|error| refusal(response_invalid(error)))
    }

    /// Step one of a run: `{ unit, entry, payload }` for the Network unit.
    #[wasm_bindgen(js_name = vectorToolEffect, unchecked_return_type = "{ unit: 'network'; entry: 'grid.model'; payload: Uint8Array }")]
    pub fn effect_js(operation: tsify::Ts<VectorToolOperation>) -> Result<JsValue, JsError> {
        let operation = operation
            .to_rust()
            .map_err(|error| refusal(request_invalid(error)))?;
        let effect = effect(&operation).map_err(refusal)?;
        let out = js_sys::Object::new();
        let set = |key: &str, value: &JsValue| {
            js_sys::Reflect::set(&out, &JsValue::from_str(key), value)
                .map(|_| ())
                .map_err(|_| refusal(response_invalid("effect object")))
        };
        set("unit", &JsValue::from_str(effect.unit))?;
        set("entry", &JsValue::from_str("grid.model"))?;
        set(
            "payload",
            &js_sys::Uint8Array::from(effect.payload.as_slice()).into(),
        )?;
        Ok(out.into())
    }

    /// Step two: the unit's admitted reply to this operation.
    #[wasm_bindgen(js_name = vectorToolReply)]
    pub fn reply_js(
        operation: tsify::Ts<VectorToolOperation>,
        reply: &[u8],
    ) -> Result<tsify::Ts<VectorToolResponse>, JsError> {
        let operation = operation
            .to_rust()
            .map_err(|error| refusal(request_invalid(error)))?;
        super::reply(&operation, reply)
            .map_err(refusal)?
            .into_ts()
            .map_err(|error| refusal(response_invalid(error)))
    }

    #[wasm_bindgen(js_name = decodeVectorTool)]
    pub fn decode(
        operation: tsify::Ts<VectorToolOperation>,
        response: tsify::Ts<VectorToolResponse>,
    ) -> Result<tsify::Ts<VectorToolResponse>, JsError> {
        let operation = operation
            .to_rust()
            .map_err(|error| refusal(request_invalid(error)))?;
        let response = response
            .to_rust()
            .map_err(|error| refusal(response_invalid(error)))?;
        validate_response(&operation, &response).map_err(refusal)?;
        response
            .into_ts()
            .map_err(|error| refusal(response_invalid(error)))
    }
}

#[cfg(test)]
#[path = "vector_tools_tests.rs"]
mod tests;
