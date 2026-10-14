//! Browser adapter for permanent UI interaction algorithms.
use ds_geo_lite::projection as core_projection;
use wasm_bindgen::prelude::*;

mod map_geometry;
mod polygon_edge_hit;
pub use polygon_edge_hit::polygon_edge_hit;
mod gantt_hit_index;
mod profile_labels;
pub use gantt_hit_index::{
    GanttHitIndex, gantt_point_to_polyline_distance, gantt_point_to_segment_distance,
};
pub use map_geometry::{
    geojson_bounds, geojson_line_length_m, measure_frame, spherical_polygon_area_m2,
};
pub use profile_labels::{
    canvas_profile_label_layout, canvas_profile_label_order, canvas_profile_label_placement,
};

mod canvas;
mod canvas_free_card;
mod canvas_free_graph;
mod canvas_graph;
pub use canvas::{
    CanvasLayoutSession, CanvasMeasuredPath, canvas_camera, canvas_gesture, canvas_selection,
};
pub use canvas_free_card::CanvasFreeCardGeometry;
pub use canvas_free_graph::{CanvasFreeGraph, canvas_graph_auto_arrange};
pub use canvas_graph::{
    CanvasGeometryBoundaryError, CanvasGraphSession, MAX_CANVAS_GRAPH_INPUT_BYTES,
    canvas_graph_abi, canvas_graph_arrange, canvas_graph_curve_distance,
    canvas_graph_curve_markers, canvas_graph_route,
};

fn log_info(_message: &str) {}
fn parse_json<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, JsError> {
    serde_json::from_str(input).map_err(|error| JsError::new(&error.to_string()))
}
fn stringify_json<T: serde::Serialize>(value: &T) -> Result<String, JsError> {
    serde_json::to_string(value).map_err(|error| JsError::new(&error.to_string()))
}

#[derive(serde::Deserialize)]
struct ProjectCoordinateInput {
    lng: f64,
    lat: f64,
    #[serde(default)]
    projection: core_projection::ProjectionConfig,
}

#[derive(serde::Serialize)]
struct ProjectCoordinateResult {
    point: [f64; 2],
    projection: core_projection::ProjectionMetadata,
}

/// Project one WGS84 cursor coordinate into the requested metric CRS.
///
/// This is intentionally a standalone API, separate from `SessionHandle`, so
/// UI readouts can stay WASM-owned even before a populated edit-session index
/// is available. MapLibre owns camera scale/zoom; Rust owns CRS selection and
/// coordinate projection.
#[wasm_bindgen(js_name = "projectCoordinate")]
pub fn project_coordinate(input_json: &str) -> Result<String, JsError> {
    let input: ProjectCoordinateInput = parse_json(input_json)?;
    if !input.lng.is_finite() || !input.lat.is_finite() {
        return Err(JsError::new("projectCoordinate: lng/lat must be finite"));
    }
    // Explicit UTM zone gate. The downstream `Self::utm()` constructor used
    // to silently `.clamp(1, 60)` an invalid zone — useful as a last-resort
    // defense, but at this user-facing boundary a bad zone is almost always
    // a frontend bug we want to surface, not paper over. Reject loudly here;
    // the inner clamp stays as defense-in-depth for any non-WASM caller.
    if input.projection.mode == core_projection::ProjectionSelection::Utm
        && let Some(zone) = input.projection.utm_zone
        && !(1..=60).contains(&zone)
    {
        return Err(JsError::new(&format!(
            "projectCoordinate: utm_zone {zone} out of range [1, 60]"
        )));
    }
    let projection = core_projection::ProjectionMetadata::from_lonlat_coords_with_config(
        [[input.lng, input.lat]],
        input.projection,
    );
    let point = projection.project_coord([input.lng, input.lat]);
    let result = ProjectCoordinateResult { point, projection };
    stringify_json(&result)
}

/// Evaluate one transient map interaction. The UI contributes pointer/screen facts.
#[wasm_bindgen(js_name = "mapInteraction")]
pub fn map_interaction(input_json: &str) -> Result<String, JsError> {
    let input: ds_map_interaction::InteractionCommand = parse_json(input_json)?;
    let result = ds_map_interaction::evaluate(input).map_err(|error| JsError::new(&error))?;
    stringify_json(&result)
}

#[derive(serde::Deserialize)]
struct ProjectionTransformInput {
    projection: core_projection::ProjectionMetadata,
    point: [f64; 2],
    inverse: bool,
}

/// Transform transient UI coordinates in an already-resolved frame.
#[wasm_bindgen(js_name = "projectionTransform")]
pub fn projection_transform(input_json: &str) -> Result<String, JsError> {
    let input: ProjectionTransformInput = parse_json(input_json)?;
    if !input.point.into_iter().all(f64::is_finite) {
        return Err(JsError::new(
            "projectionTransform: coordinates must be finite",
        ));
    }
    let point = if input.inverse {
        input.projection.unproject_coord(input.point)
    } else {
        input.projection.project_coord(input.point)
    };
    if !point.into_iter().all(f64::is_finite) {
        return Err(JsError::new("projectionTransform: result must be finite"));
    }
    stringify_json(&point)
}

#[wasm_bindgen]
pub fn haversine_m(a_lng: f64, a_lat: f64, b_lng: f64, b_lat: f64) -> f64 {
    ds_geo_lite::haversine_m(a_lng, a_lat, b_lng, b_lat)
}
#[wasm_bindgen]
pub fn bearing_deg(a_lng: f64, a_lat: f64, b_lng: f64, b_lat: f64) -> f64 {
    ds_geo_lite::bearing_deg(a_lng, a_lat, b_lng, b_lat)
}
#[wasm_bindgen]
pub fn destination_point(lng: f64, lat: f64, bearing_deg: f64, distance_m: f64) -> Vec<f64> {
    ds_geo_lite::destination_point(lng, lat, bearing_deg, distance_m)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_coordinate_uses_core_projection_config() {
        let input = serde_json::json!({
            "lng": 30.0,
            "lat": -2.0,
            "projection": { "mode": "auto" }
        })
        .to_string();
        let out = project_coordinate(&input).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["projection"]["mode"], "nix_itrf2005");
        assert!(parsed["point"][0].as_f64().unwrap().is_finite());
        assert!(parsed["point"][1].as_f64().unwrap().is_finite());
    }
}
