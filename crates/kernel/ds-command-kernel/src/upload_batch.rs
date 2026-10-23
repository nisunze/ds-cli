//! Chunk-level upload cleaning over Arrow IPC: clean one staged chunk, scan it
//! for raw values a replacement plan names, or stamp per-source provenance.
//! The browser keeps chunk I/O and scheduling; the governed Network unit
//! keeps every cleaning decision (`uploadBatch`). The Kernel admits the
//! request, names the effect and validates the reply. Chunk bytes cross the
//! boundary as bytes, never as feature JSON. Vocabulary:
//! `ds_network_contracts::page_data::upload_batch`.
use crate::grid_engine::{GridEffect, UNIT};
use ds_network_contracts::grid_unit::{
    decode_reply as decode_grid_reply, GridArg, GridModelMethod, GridOperation, GridRequest,
    GridResult,
};
pub use ds_network_contracts::page_data::upload_batch::*;

const OPERATION: GridOperation = GridOperation::Model(GridModelMethod::UploadBatch);

fn invalid_request(message: impl Into<String>) -> UploadBatchError {
    UploadBatchError::InvalidRequest(message.into())
}
fn invalid_response(message: impl Into<String>) -> UploadBatchError {
    UploadBatchError::InvalidResponse(message.into())
}

/// The effect for one admitted request over one staged chunk.
pub fn request_effect(
    ipc: &[u8],
    request: &UploadBatchRequest,
) -> Result<GridEffect, UploadBatchError> {
    validate_request(request)?;
    let text =
        serde_json::to_string(request).map_err(|error| invalid_request(error.to_string()))?;
    let request = GridRequest::new(
        OPERATION,
        vec![GridArg::Text(text), GridArg::Bytes(ipc.to_vec())],
    )
    .map_err(|refusal| invalid_request(refusal.message))?;
    Ok(GridEffect {
        unit: UNIT,
        entry: OPERATION.entry(),
        payload: request.encode(),
    })
}

/// Decode the unit's reply to `request`, put its chunk back and validate it.
pub fn decode_unit_reply(
    request: &UploadBatchRequest,
    reply: &[u8],
) -> Result<UploadBatchResponse, UploadBatchError> {
    let (text, mut blobs) = match decode_grid_reply(OPERATION, reply) {
        Err(malformed) => return Err(invalid_response(malformed.message)),
        Ok(Err(refusal)) => return Err(UploadBatchError::Refused(refusal.message)),
        Ok(Ok(GridResult::Parts { text, blobs })) => (text, blobs),
        Ok(Ok(_)) => return Err(invalid_response("the reply does not carry a batch answer")),
    };
    let mut response: UploadBatchResponse =
        serde_json::from_str(&text).map_err(|error| invalid_response(error.to_string()))?;
    let matched = match blobs.pop() {
        Some(ipc) if blobs.is_empty() => response.put_ipc(ipc),
        Some(_) => false,
        None => response.take_ipc().is_none(),
    };
    if !matched {
        return Err(invalid_response(
            "the reply's chunks do not match its answer",
        ));
    }
    validate_response(request, response)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use crate::grid_engine::browser::effect_value;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    fn refuse(error: UploadBatchError) -> JsError {
        JsError::new(&error.to_string())
    }
    fn request(input: tsify::Ts<UploadBatchRequest>) -> Result<UploadBatchRequest, JsError> {
        input
            .to_rust()
            .map_err(|error| refuse(invalid_request(error.to_string())))
    }

    /// The Network unit effect for one staged chunk.
    #[wasm_bindgen(js_name = uploadBatchEffect, unchecked_return_type = "PageDataEffect")]
    pub fn effect(ipc: &[u8], input: tsify::Ts<UploadBatchRequest>) -> Result<JsValue, JsError> {
        effect_value(&request_effect(ipc, &request(input)?).map_err(refuse)?)
            .map_err(|_| refuse(invalid_response("could not assemble the effect")))
    }

    /// Decode and validate the unit's reply to one staged chunk.
    #[wasm_bindgen(js_name = decodeUploadBatchReply)]
    pub fn decode(
        input: tsify::Ts<UploadBatchRequest>,
        reply: &[u8],
    ) -> Result<tsify::Ts<UploadBatchResponse>, JsError> {
        let request = request(input)?;
        decode_unit_reply(&request, reply)
            .map_err(refuse)?
            .into_ts()
            .map_err(|error| refuse(invalid_response(error.to_string())))
    }
}

#[cfg(test)]
#[path = "upload_batch_tests.rs"]
mod tests;
