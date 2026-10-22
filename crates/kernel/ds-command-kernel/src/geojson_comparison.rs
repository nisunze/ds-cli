//! Application entry for the portable comparison owner used by native Design.
pub use ds_geo_lite::comparison::{
    compare_geojson_layers, validate_comparison_response, ComparisonError,
    GeoJsonLayerComparisonInput, GeoJsonLayerComparisonResult,
};

pub fn decode_comparison(
    input: &GeoJsonLayerComparisonInput,
    bytes: &[u8],
) -> Result<GeoJsonLayerComparisonResult, ComparisonError> {
    let result = serde_json::from_slice(bytes)
        .map_err(|error| ComparisonError::InvalidResponse(error.to_string()))?;
    validate_comparison_response(input, result)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = compareGeoJsonLayers)]
    pub fn compare(
        input: tsify::Ts<GeoJsonLayerComparisonInput>,
    ) -> Result<tsify::Ts<GeoJsonLayerComparisonResult>, JsError> {
        let input = input.to_rust().map_err(|error| {
            JsError::new(&ComparisonError::InvalidRequest(error.to_string()).to_string())
        })?;
        compare_geojson_layers(input)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }

    #[wasm_bindgen(js_name = decodeGeoJsonComparison)]
    pub fn decode(
        input: tsify::Ts<GeoJsonLayerComparisonInput>,
        result: tsify::Ts<GeoJsonLayerComparisonResult>,
    ) -> Result<tsify::Ts<GeoJsonLayerComparisonResult>, JsError> {
        let input = input.to_rust().map_err(|error| {
            JsError::new(&ComparisonError::InvalidRequest(error.to_string()).to_string())
        })?;
        let result = result.to_rust().map_err(|error| {
            JsError::new(&ComparisonError::InvalidResponse(error.to_string()).to_string())
        })?;
        validate_comparison_response(&input, result)
            .map_err(|error| JsError::new(&error.to_string()))?
            .into_ts()
            .map_err(|error| JsError::new(&error.to_string()))
    }
}
