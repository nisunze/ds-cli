//! Thin UI map-fit and measurement adapters over the existing neutral geometry owner.
use crate::{parse_json, stringify_json};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = "geojsonBounds")]
pub fn geojson_bounds(geojson_json: &str) -> Result<String, JsError> {
    let value: serde_json::Value = parse_json(geojson_json)?;
    stringify_json(&ds_geo_lite::viewport::geojson_bounds(&value))
}
#[wasm_bindgen(js_name = geojsonLineLengthM)]
pub fn geojson_line_length_m(geometry_json: &str) -> Result<Option<f64>, JsError> {
    let geometry = parse_json(geometry_json)?;
    Ok(ds_geo_lite::measurement::geojson_line_length_m(&geometry))
}
#[wasm_bindgen]
pub fn spherical_polygon_area_m2(coords_flat: &[f64]) -> f64 {
    ds_geo_lite::spherical_polygon_area_m2(coords_flat)
}
#[wasm_bindgen]
pub fn measure_frame(input_json: &str) -> Result<String, JsError> {
    let input: ds_geo_lite::measurement::MeasureFrameInput = parse_json(input_json)?;
    stringify_json(&ds_geo_lite::measurement::measure_frame(input))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_adapters_keep_empty_invalid_and_disconnected_geometry_semantics() {
        assert_eq!(
            geojson_bounds(r#"{"type":"FeatureCollection","features":[]}"#).unwrap(),
            "null"
        );
        let input = r#"{"type":"MultiLineString","coordinates":[[[30,-2],[30.001,-2]],[[31,-3],[31.001,-3]]]}"#;
        let value = serde_json::from_str(input).unwrap();
        assert_eq!(
            geojson_line_length_m(input).unwrap(),
            ds_geo_lite::measurement::geojson_line_length_m(&value)
        );
        assert_eq!(
            geojson_line_length_m(r#"{"type":"LineString","coordinates":[[30,-2],[null,-2]]}"#)
                .unwrap(),
            None
        );
    }
    #[test]
    fn measurement_frame_and_bounds_are_exact_neutral_projections() {
        let input = r#"{"vertices":[[30,-2],[30.001,-2],[30.001,-2.001]],"mode":"area"}"#;
        let expected =
            ds_geo_lite::measurement::measure_frame(serde_json::from_str(input).unwrap());
        assert_eq!(
            measure_frame(input).unwrap(),
            serde_json::to_string(&expected).unwrap()
        );
        assert_eq!(
            geojson_bounds(r#"{"type":"MultiPoint","coordinates":[[30,-2],[300,-2],[31,-1]]}"#)
                .unwrap(),
            "[30.0,-2.0,31.0,-1.0]"
        );
    }
}
