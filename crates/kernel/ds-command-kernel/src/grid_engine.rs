//! Application side of the browser DS Grid model session.
//!
//! The Kernel builds each request for the governed Network unit — one closed
//! operation with its admitted arguments — and names the effect a host
//! executes: `{ unit, entry, payload }`. The host calls only that declared
//! entry of the unit it holds and returns the reply bytes here, where they are
//! decoded and admitted. The Kernel computes no grid engineering and links no
//! engine: the vocabulary is the shared `ds_network_contracts::grid_unit`
//! contract. Result text and bytes pass through verbatim, so JSON key order,
//! digests and package bytes are exactly the unit's.

pub use ds_network_contracts::grid_unit::{
    decode_reply, GridArg, GridEntry, GridOperation, GridRefusal, GridRequest, GridResult,
    ENGINE_UNAVAILABLE, METHOD_UNAVAILABLE, REQUEST_INVALID, RESPONSE_INVALID,
};
use serde::Serialize;

/// The governed unit that serves the browser DS Grid session.
pub const UNIT: &str = "network";

/// One declared effect: the unit, its entry and the opaque request bytes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GridEffect {
    pub unit: &'static str,
    pub entry: GridEntry,
    #[serde(skip)]
    pub payload: Vec<u8>,
}

/// Admit one operation and build its effect.
pub fn request(kind: &str, method: &str, args: Vec<GridArg>) -> Result<GridEffect, GridRefusal> {
    let operation = GridOperation::parse(kind, method)?;
    let request = GridRequest::new(operation, args)?;
    Ok(GridEffect {
        unit: UNIT,
        entry: operation.entry(),
        payload: request.encode(),
    })
}

/// Decode the unit's reply to `kind`/`method`: the owner's result or its own
/// typed refusal; a malformed, crossed or incompatible reply is
/// `grid_response_invalid`.
pub fn decode(kind: &str, method: &str, reply: &[u8]) -> Result<GridResult, GridRefusal> {
    let operation = GridOperation::parse(kind, method)?;
    decode_reply(operation, reply)?
}

/// The refusal a host reports when no admitted unit with the entry is held.
pub fn unavailable(reason: &str) -> GridRefusal {
    GridRefusal::new(
        ENGINE_UNAVAILABLE,
        format!("The DS Grid session needs the governed Network unit: {reason}"),
    )
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use wasm_bindgen::prelude::*;

    fn refusal(error: GridRefusal) -> JsValue {
        JsError::new(&error.to_json()).into()
    }

    fn admit(args: &js_sys::Array) -> Result<Vec<GridArg>, JsValue> {
        args.iter()
            .enumerate()
            .map(|(index, value)| {
                if let Some(text) = value.as_string() {
                    Ok(GridArg::Text(text))
                } else if let Some(flag) = value.as_bool() {
                    Ok(GridArg::Bool(flag))
                } else if let Some(number) = value.as_f64() {
                    Ok(GridArg::Number(number))
                } else if value.is_instance_of::<js_sys::Uint8Array>() {
                    Ok(GridArg::Bytes(js_sys::Uint8Array::new(&value).to_vec()))
                } else {
                    Err(refusal(GridRefusal::new(
                        REQUEST_INVALID,
                        format!("argument {index} must be a string, number, boolean or Uint8Array"),
                    )))
                }
            })
            .collect()
    }

    fn set(target: &js_sys::Object, key: &str, value: &JsValue) -> Result<(), JsValue> {
        js_sys::Reflect::set(target, &JsValue::from_str(key), value).map(|_| ())
    }

    /// Build the effect for one operation: `{ unit, entry, payload }`.
    #[wasm_bindgen(js_name = gridEngineRequest, unchecked_return_type = "{ unit: 'network'; entry: 'grid.session' | 'grid.model'; payload: Uint8Array }")]
    pub fn grid_engine_request(
        #[wasm_bindgen(unchecked_param_type = "'open' | 'session' | 'model'")] kind: &str,
        method: &str,
        args: js_sys::Array,
    ) -> Result<JsValue, JsValue> {
        let effect = request(kind, method, admit(&args)?).map_err(refusal)?;
        let out = js_sys::Object::new();
        set(&out, "unit", &JsValue::from_str(effect.unit))?;
        let entry = match effect.entry {
            GridEntry::Session => "grid.session",
            GridEntry::Model => "grid.model",
        };
        set(&out, "entry", &JsValue::from_str(entry))?;
        set(
            &out,
            "payload",
            &js_sys::Uint8Array::from(effect.payload.as_slice()),
        )?;
        Ok(out.into())
    }

    /// Decode one reply: JSON text, bytes, `{ bytes, receiptJson }` or `null`
    /// for an open. Throws the owner's or the Kernel's typed refusal.
    #[wasm_bindgen(js_name = decodeGridEngineReply, unchecked_return_type = "string | Uint8Array | { bytes: Uint8Array; receiptJson: string } | null")]
    pub fn decode_grid_engine_reply(
        #[wasm_bindgen(unchecked_param_type = "'open' | 'session' | 'model'")] kind: &str,
        method: &str,
        reply: &[u8],
    ) -> Result<JsValue, JsValue> {
        Ok(match decode(kind, method, reply).map_err(refusal)? {
            GridResult::Text(text) => JsValue::from_str(&text),
            GridResult::Bytes(bytes) => js_sys::Uint8Array::from(bytes.as_slice()).into(),
            GridResult::Artifact {
                bytes,
                receipt_json,
            } => {
                let out = js_sys::Object::new();
                set(&out, "bytes", &js_sys::Uint8Array::from(bytes.as_slice()))?;
                set(&out, "receiptJson", &JsValue::from_str(&receipt_json))?;
                out.into()
            }
            GridResult::Opened => JsValue::NULL,
        })
    }

    /// The typed refusal for a host without an admitted unit or entry.
    #[wasm_bindgen(js_name = gridEngineUnavailable)]
    pub fn grid_engine_unavailable(reason: &str) -> JsValue {
        refusal(unavailable(reason))
    }
}

#[cfg(test)]
#[path = "grid_engine_tests.rs"]
mod tests;
