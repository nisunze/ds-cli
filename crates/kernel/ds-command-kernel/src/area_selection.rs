//! Polygon area selection over caller-supplied GeoJSON layers: the survey
//! lasso, the plan-view grid lasso and the map-data transformer import all
//! ask "which of these features does this polygon select?".
//!
//! The transport is `ds_geo::area_selection` and its predicate the one
//! `ds_geo_lite::region` rule. This module owns the typed application
//! operation, its bounded admission and the reply decoder.
//!
//! The request crosses as the caller's JSON text and the reply returns as JSON
//! text, exactly the bytes the retired Network export exchanged: selected
//! features keep every value, and every object's keys are in sorted order at
//! every level (the order that export's maps always had), so a host that reads
//! the first matching key or derives column order sees the same answer.
pub use ds_geo::area_selection::{
    PolygonIntersectInput as AreaSelectionRequest,
    PolygonIntersectLayerInput as AreaSelectionLayer,
    PolygonIntersectLayerOutput as AreaSelectionLayerResult,
    PolygonIntersectOutput as AreaSelectionResult,
};

/// The most features one selection may carry across all of its layers.
pub const MAX_AREA_SELECTION_FEATURES: usize = 1_000_000;

#[derive(Clone, Debug, PartialEq)]
pub enum AreaSelectionError {
    InvalidRequest(String),
    InvalidResponse(String),
}

impl std::fmt::Display for AreaSelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(f, "area_selection_request_invalid: {message}"),
            Self::InvalidResponse(message) => {
                write!(f, "area_selection_response_invalid: {message}")
            }
        }
    }
}

impl std::error::Error for AreaSelectionError {}

fn invalid_response(message: impl Into<String>) -> AreaSelectionError {
    AreaSelectionError::InvalidResponse(message.into())
}

/// Admit the selector polygon and the bounded feature count.
pub fn validate_request(request: &AreaSelectionRequest) -> Result<(), AreaSelectionError> {
    let features: usize = request
        .layers
        .iter()
        .map(|layer| layer.features.len())
        .sum();
    if features > MAX_AREA_SELECTION_FEATURES {
        return Err(AreaSelectionError::InvalidRequest(format!(
            "{features} features exceed the {MAX_AREA_SELECTION_FEATURES} feature limit"
        )));
    }
    ds_geo::area_selection::selector_rings(&request.polygon)
        .map(|_| ())
        .map_err(AreaSelectionError::InvalidRequest)
}

/// Parse and admit the caller's JSON request text.
pub fn decode_request(bytes: &[u8]) -> Result<AreaSelectionRequest, AreaSelectionError> {
    let request = serde_json::from_slice(bytes)
        .map_err(|error| AreaSelectionError::InvalidRequest(error.to_string()))?;
    validate_request(&request)?;
    Ok(request)
}

/// Select every feature the polygon selects under the request's predicate.
pub fn select(request: &AreaSelectionRequest) -> Result<AreaSelectionResult, AreaSelectionError> {
    validate_request(request)?;
    let mut result = ds_geo::area_selection::filter_layers_by_polygon_intersects(request.clone())
        .map_err(AreaSelectionError::InvalidRequest)?;
    for layer in &mut result.layers {
        layer
            .features
            .iter_mut()
            .for_each(serde_json::Value::sort_all_objects);
    }
    Ok(result)
}

/// The reply text for one JSON request text.
pub fn select_json(bytes: &[u8]) -> Result<String, AreaSelectionError> {
    let result = select(&decode_request(bytes)?)?;
    serde_json::to_string(&result).map_err(|error| invalid_response(error.to_string()))
}

/// Accept a reply only when it answers this request: the same layers in the
/// same order, consistent counts, and selected features that are unchanged
/// input features in input order.
pub fn validate_response(
    request: &AreaSelectionRequest,
    response: AreaSelectionResult,
) -> Result<AreaSelectionResult, AreaSelectionError> {
    validate_request(request)?;
    if response.layers.len() != request.layers.len() {
        return Err(invalid_response(format!(
            "reply has {} layers for {} requested",
            response.layers.len(),
            request.layers.len()
        )));
    }
    let mut total = 0usize;
    for (asked, answered) in request.layers.iter().zip(&response.layers) {
        if asked.name != answered.name {
            return Err(invalid_response(format!(
                "reply layer {:?} does not answer requested layer {:?}",
                answered.name, asked.name
            )));
        }
        if answered.input_count != asked.features.len()
            || answered.selected_count != answered.features.len()
        {
            return Err(invalid_response(format!(
                "layer {:?} counts do not match its features",
                asked.name
            )));
        }
        // Map equality compares entries, not key order.
        let mut inputs = asked.features.iter();
        if !answered
            .features
            .iter()
            .all(|selected| inputs.any(|input| input == selected))
        {
            return Err(invalid_response(format!(
                "layer {:?} returned a feature that is not an unchanged input feature",
                asked.name
            )));
        }
        total += answered.selected_count;
    }
    if total != response.total_selected {
        return Err(invalid_response(format!(
            "total_selected {} does not equal the {total} selected features",
            response.total_selected
        )));
    }
    Ok(response)
}

pub fn decode_response(
    request: &AreaSelectionRequest,
    bytes: &[u8],
) -> Result<AreaSelectionResult, AreaSelectionError> {
    validate_response(
        request,
        serde_json::from_slice(bytes).map_err(|error| invalid_response(error.to_string()))?,
    )
}

/// Validate a reply text against the request text that produced it; the
/// accepted reply is returned unchanged.
pub fn decode_response_json(input: &[u8], reply: &str) -> Result<String, AreaSelectionError> {
    decode_response(&decode_request(input)?, reply.as_bytes())?;
    Ok(reply.to_string())
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = selectLayersByArea)]
    pub fn run(input: &[u8]) -> Result<String, JsError> {
        select_json(input).map_err(|error| JsError::new(&error.to_string()))
    }

    #[wasm_bindgen(js_name = decodeAreaSelection)]
    pub fn decode(input: &[u8], reply: &str) -> Result<String, JsError> {
        decode_response_json(input, reply).map_err(|error| JsError::new(&error.to_string()))
    }
}
