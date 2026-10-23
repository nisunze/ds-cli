//! Stateless table edit proposals. The application owns applying a proposal;
//! the Network unit computes it (`tableEdit` through the grid unit envelope),
//! and the requests, plans and their checks are the Network contract. This
//! module encodes the page's request, checks its held snapshot and admits the
//! unit's plan.
pub use ds_network_contracts::table_edit::{
    decode_request, decode_response, validate_request, validate_response, validate_snapshot,
    AddColumnEditResult, TableEditError, TableEditRequest, TableEditResponse, TableEditSnapshot,
};

/// The Network unit operation that proposes one edit.
pub const UNIT_METHOD: &str = "tableEdit";

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = validateTableEditSnapshot)]
    pub fn current(input: &[u8], snapshot: tsify::Ts<TableEditSnapshot>) -> Result<(), JsError> {
        let request = decode_request(input).map_err(|error| JsError::new(&error.to_string()))?;
        let snapshot = snapshot.to_rust().map_err(|error| {
            JsError::new(&TableEditError::InvalidRequest(error.to_string()).to_string())
        })?;
        validate_snapshot(&request, &snapshot).map_err(|error| JsError::new(&error.to_string()))
    }
    #[wasm_bindgen(js_name=encodeTableEdit)]
    pub fn encode(input: tsify::Ts<TableEditRequest>) -> Result<Vec<u8>, JsError> {
        let request = input.to_rust().map_err(|error| {
            JsError::new(&TableEditError::InvalidRequest(error.to_string()).to_string())
        })?;
        validate_request(&request).map_err(|error| JsError::new(&error.to_string()))?;
        serde_json::to_vec(&request).map_err(|error| {
            JsError::new(&TableEditError::InvalidRequest(error.to_string()).to_string())
        })
    }
    #[wasm_bindgen(js_name=decodeTableEdit)]
    pub fn decode(
        input: &[u8],
        reply: tsify::Ts<TableEditResponse>,
    ) -> Result<tsify::Ts<TableEditResponse>, JsError> {
        let request = decode_request(input).map_err(|error| JsError::new(&error.to_string()))?;
        let reply = reply.to_rust().map_err(|error| {
            JsError::new(&TableEditError::InvalidResponse(error.to_string()).to_string())
        })?;
        validate_response(&request, reply)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }
}
