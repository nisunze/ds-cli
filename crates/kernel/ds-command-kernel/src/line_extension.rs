//! Extract the new line work an incoming line layer adds to an authoritative
//! base layer: aligned already-built portions are removed and remaining
//! endpoints are healed onto the base. The topology algorithm and its wire
//! types belong to `ds_geo::line_difference`; this module owns the typed
//! application operation, its refusals and the reply decoder.
//!
//! The request crosses as the caller's JSON text and the reply returns as JSON
//! text with every object's keys in sorted order — the bytes the retired
//! Network export produced — so the layer the host materializes keeps the
//! same column order and values.
pub use ds_geo::line_difference::{LineExtensionDifferenceInput, LineExtensionDifferenceOutput};
use serde_json::Value;

/// The most features one request may carry across source and base.
pub const MAX_LINE_EXTENSION_FEATURES: usize = 500_000;

#[derive(Clone, Debug, PartialEq)]
pub enum LineExtensionError {
    InvalidRequest(String),
    InvalidResponse(String),
}

impl std::fmt::Display for LineExtensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(f, "line_extension_request_invalid: {message}"),
            Self::InvalidResponse(message) => {
                write!(f, "line_extension_response_invalid: {message}")
            }
        }
    }
}

impl std::error::Error for LineExtensionError {}

fn invalid_response(message: impl Into<String>) -> LineExtensionError {
    LineExtensionError::InvalidResponse(message.into())
}

pub fn validate_request(input: &LineExtensionDifferenceInput) -> Result<(), LineExtensionError> {
    let count = input.source_features.len() + input.base_features.len();
    if count > MAX_LINE_EXTENSION_FEATURES {
        return Err(LineExtensionError::InvalidRequest(format!(
            "{count} features exceed the {MAX_LINE_EXTENSION_FEATURES} feature limit"
        )));
    }
    Ok(())
}

/// Parse and admit the caller's JSON request text.
pub fn decode_request(bytes: &[u8]) -> Result<LineExtensionDifferenceInput, LineExtensionError> {
    let input = serde_json::from_slice(bytes)
        .map_err(|error| LineExtensionError::InvalidRequest(error.to_string()))?;
    validate_request(&input)?;
    Ok(input)
}

/// Run the owning topology algorithm; its refusals are request refusals.
pub fn difference(
    input: &LineExtensionDifferenceInput,
) -> Result<LineExtensionDifferenceOutput, LineExtensionError> {
    validate_request(input)?;
    let mut output = ds_geo::line_difference::line_extension_difference(input.clone())
        .map_err(LineExtensionError::InvalidRequest)?;
    output.layer.sort_all_objects();
    Ok(output)
}

/// The reply text for one JSON request text.
pub fn difference_json(bytes: &[u8]) -> Result<String, LineExtensionError> {
    let output = difference(&decode_request(bytes)?)?;
    serde_json::to_string(&output).map_err(|error| invalid_response(error.to_string()))
}

fn finite_line(feature: &Value) -> bool {
    feature.get("type").and_then(Value::as_str) == Some("Feature")
        && feature.pointer("/geometry/type").and_then(Value::as_str) == Some("LineString")
        && feature
            .pointer("/geometry/coordinates")
            .and_then(Value::as_array)
            .is_some_and(|coordinates| {
                coordinates.len() >= 2
                    && coordinates.iter().all(|position| {
                        position.as_array().is_some_and(|pair| {
                            pair.len() >= 2
                                && pair[..2]
                                    .iter()
                                    .all(|value| value.as_f64().is_some_and(f64::is_finite))
                        })
                    })
            })
}

/// Accept a reply only when it answers this request: counts and tolerances
/// echo the request, measured lengths are finite, and the layer holds exactly
/// the counted LineString features.
pub fn validate_response(
    input: &LineExtensionDifferenceInput,
    output: LineExtensionDifferenceOutput,
) -> Result<LineExtensionDifferenceOutput, LineExtensionError> {
    validate_request(input)?;
    if output.source_feature_count != input.source_features.len()
        || output.base_feature_count != input.base_features.len()
    {
        return Err(invalid_response("feature counts do not answer the request"));
    }
    if output.coverage_tolerance_m != input.coverage_tolerance_m
        || output.heal_tolerance_m != input.heal_tolerance_m
    {
        return Err(invalid_response("tolerances do not answer the request"));
    }
    if ![
        output.source_length_m,
        output.covered_length_m,
        output.difference_length_m,
        output.alignment_tolerance_degrees,
    ]
    .iter()
    .all(|value| value.is_finite() && *value >= 0.0)
    {
        return Err(invalid_response(
            "measured lengths must be finite and non-negative",
        ));
    }
    let features = match (
        output.layer.get("type").and_then(Value::as_str),
        output.layer.get("features").and_then(Value::as_array),
    ) {
        (Some("FeatureCollection"), Some(features)) => features,
        _ => {
            return Err(invalid_response(
                "layer must be a GeoJSON FeatureCollection",
            ))
        }
    };
    if features.len() != output.output_feature_count {
        return Err(invalid_response(format!(
            "layer holds {} features; output_feature_count is {}",
            features.len(),
            output.output_feature_count
        )));
    }
    if let Some(index) = features.iter().position(|feature| !finite_line(feature)) {
        return Err(invalid_response(format!(
            "layer feature {index} is not a finite LineString feature"
        )));
    }
    Ok(output)
}

pub fn decode_response(
    input: &LineExtensionDifferenceInput,
    bytes: &[u8],
) -> Result<LineExtensionDifferenceOutput, LineExtensionError> {
    validate_response(
        input,
        serde_json::from_slice(bytes).map_err(|error| invalid_response(error.to_string()))?,
    )
}

/// Validate a reply text against the request text that produced it; the
/// accepted reply is returned unchanged.
pub fn decode_response_json(input: &[u8], reply: &str) -> Result<String, LineExtensionError> {
    decode_response(&decode_request(input)?, reply.as_bytes())?;
    Ok(reply.to_string())
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = lineExtensionDifference)]
    pub fn run(input: &[u8]) -> Result<String, JsError> {
        difference_json(input).map_err(|error| JsError::new(&error.to_string()))
    }

    #[wasm_bindgen(js_name = decodeLineExtensionDifference)]
    pub fn decode(input: &[u8], reply: &str) -> Result<String, JsError> {
        decode_response_json(input, reply).map_err(|error| JsError::new(&error.to_string()))
    }
}
