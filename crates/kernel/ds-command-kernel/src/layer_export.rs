//! Layer and dashboard file export: GeoJSON, delimited text, XLSX, KMZ and
//! shapefile ZIP from GeoJSON layers, and the styled dashboard workbook. The
//! Kernel admits the typed request and names the effect; the governed
//! Network unit's writers own every exported byte (`layerExport`), and the
//! Kernel checks the reply's container before a host saves it. Hosts own
//! file names and saving. Vocabulary:
//! `ds_network_contracts::page_data::layer_export`.
use crate::grid_engine::{GridEffect, UNIT};
use ds_network_contracts::grid_unit::{
    decode_reply as decode_grid_reply, GridArg, GridModelMethod, GridOperation, GridRequest,
    GridResult,
};
pub use ds_network_contracts::page_data::layer_export::*;

const OPERATION: GridOperation = GridOperation::Model(GridModelMethod::LayerExport);

/// The effect for one admitted export request (its owning JSON bytes).
pub fn request_effect(input: &[u8]) -> Result<GridEffect, LayerExportError> {
    decode_request(input)?;
    let request = GridRequest::new(OPERATION, vec![GridArg::Bytes(input.to_vec())])
        .map_err(|refusal| LayerExportError::InvalidRequest(refusal.message))?;
    Ok(GridEffect {
        unit: UNIT,
        entry: OPERATION.entry(),
        payload: request.encode(),
    })
}

/// The exported file bytes of the unit's reply, after the bounded container
/// check of `format`. A writer's refusal keeps its own message.
pub fn decode_unit_reply(
    format: LayerExportFormat,
    reply: &[u8],
) -> Result<Vec<u8>, LayerExportError> {
    let bytes = match decode_grid_reply(OPERATION, reply) {
        Err(malformed) => return Err(LayerExportError::InvalidResponse(malformed.message)),
        Ok(Err(refusal)) => return Err(LayerExportError::Refused(refusal.message)),
        Ok(Ok(GridResult::Bytes(bytes))) => bytes,
        Ok(Ok(_)) => {
            return Err(LayerExportError::InvalidResponse(
                "the reply does not carry exported bytes".into(),
            ))
        }
    };
    decode_reply(format, &bytes)?;
    Ok(bytes)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use crate::grid_engine::browser::effect_value;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    fn refuse(error: LayerExportError) -> JsError {
        JsError::new(&error.to_string())
    }

    /// The Network unit effect for one typed export request (owning JSON bytes).
    #[wasm_bindgen(js_name = layerExportEffect, unchecked_return_type = "PageDataEffect")]
    pub fn request(input: &[u8]) -> Result<JsValue, JsError> {
        effect_value(&request_effect(input).map_err(refuse)?).map_err(|_| {
            refuse(LayerExportError::InvalidResponse(
                "could not assemble the effect".into(),
            ))
        })
    }

    /// The bounded container check of exported bytes a worker returned.
    #[wasm_bindgen(js_name = decodeLayerExport)]
    pub fn decode_bytes(
        format: tsify::Ts<LayerExportFormat>,
        output: &[u8],
    ) -> Result<tsify::Ts<LayerExportReceipt>, JsError> {
        let format = format
            .to_rust()
            .map_err(|error| refuse(LayerExportError::InvalidRequest(error.to_string())))?;
        decode_reply(format, output)
            .map_err(refuse)?
            .into_ts()
            .map_err(|error| refuse(LayerExportError::InvalidResponse(error.to_string())))
    }

    /// The checked file bytes of the unit's reply.
    #[wasm_bindgen(js_name = decodeLayerExportReply)]
    pub fn decode(format: tsify::Ts<LayerExportFormat>, reply: &[u8]) -> Result<Vec<u8>, JsError> {
        let format = format
            .to_rust()
            .map_err(|error| refuse(LayerExportError::InvalidRequest(error.to_string())))?;
        decode_unit_reply(format, reply).map_err(refuse)
    }
}

#[cfg(test)]
#[path = "layer_export_tests.rs"]
mod tests;
