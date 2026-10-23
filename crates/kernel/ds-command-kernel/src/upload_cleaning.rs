//! Upload cleaning application operations: apply a column plan to parsed
//! upload layers, preview one column, validate replacement candidates, stamp
//! upload provenance, resolve a layer's column vocabulary and merge an upload
//! into an existing layer by import key. The Kernel admits the typed request
//! and names the effect; the governed Network unit computes it
//! (`uploadCleaning`), and the Kernel validates the reply against the
//! request. Vocabulary: `ds_network_contracts::page_data::upload_cleaning`.
use crate::grid_engine::{GridEffect, UNIT};
use ds_network_contracts::grid_unit::{
    decode_reply as decode_grid_reply, GridArg, GridModelMethod, GridOperation, GridRequest,
    GridResult,
};
pub use ds_network_contracts::page_data::upload_cleaning::*;

const OPERATION: GridOperation = GridOperation::Model(GridModelMethod::UploadCleaning);

/// The effect for one admitted request from its owning wire bytes.
pub fn request_effect(input: &[u8]) -> Result<GridEffect, UploadCleaningError> {
    decode_request(input)?;
    let request = GridRequest::new(OPERATION, vec![GridArg::Bytes(input.to_vec())])
        .map_err(|refusal| invalid_request(refusal.message))?;
    Ok(GridEffect {
        unit: UNIT,
        entry: OPERATION.entry(),
        payload: request.encode(),
    })
}

/// Decode the unit's reply to `request` and validate it.
pub fn decode_unit_reply(
    request: &UploadCleaningRequest,
    reply: &[u8],
) -> Result<UploadCleaningResponse, UploadCleaningError> {
    match decode_grid_reply(OPERATION, reply) {
        Err(malformed) => Err(invalid_response(malformed.message)),
        Ok(Err(refusal)) => Err(UploadCleaningError::Refused(refusal.message)),
        Ok(Ok(GridResult::Text(text))) => decode_response(request, text.as_bytes()),
        Ok(Ok(_)) => Err(invalid_response(
            "the reply does not carry a cleaning answer",
        )),
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use crate::grid_engine::browser::effect_value;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    fn refuse(error: UploadCleaningError) -> JsError {
        JsError::new(&error.to_string())
    }

    /// The Network unit effect for one upload cleaning request.
    #[wasm_bindgen(js_name = uploadCleaningEffect, unchecked_return_type = "PageDataEffect")]
    pub fn request(input: &[u8]) -> Result<JsValue, JsError> {
        effect_value(&request_effect(input).map_err(refuse)?)
            .map_err(|_| refuse(invalid_response("could not assemble the effect")))
    }

    /// Validate a reply a worker already decoded against its request.
    #[wasm_bindgen(js_name = decodeUploadCleaning)]
    pub fn decode_object(
        input: &[u8],
        reply: tsify::Ts<UploadCleaningResponse>,
    ) -> Result<tsify::Ts<UploadCleaningResponse>, JsError> {
        let request = decode_request(input).map_err(refuse)?;
        let reply = reply
            .to_rust()
            .map_err(|error| refuse(invalid_response(error.to_string())))?;
        validate_response(&request, reply)
            .map_err(refuse)?
            .into_ts()
            .map_err(|error| refuse(invalid_response(error.to_string())))
    }

    /// Decode and validate the unit's reply to an upload cleaning request.
    #[wasm_bindgen(js_name = decodeUploadCleaningReply)]
    pub fn decode(
        input: &[u8],
        reply: &[u8],
    ) -> Result<tsify::Ts<UploadCleaningResponse>, JsError> {
        let request = decode_request(input).map_err(refuse)?;
        decode_unit_reply(&request, reply)
            .map_err(refuse)?
            .into_ts()
            .map_err(|error| refuse(invalid_response(error.to_string())))
    }
}

#[cfg(test)]
#[path = "upload_cleaning_tests.rs"]
mod tests;
