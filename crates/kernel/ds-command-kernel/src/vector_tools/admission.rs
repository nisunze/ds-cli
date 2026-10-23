//! Request admission. The descriptor schema supplies every default and
//! validation decision; hosts never repair a request.
use super::descriptor::{describe, normalize, refuse, tool, Refusal};
use serde_json::Value;

/// Normalize and validate a request using the descriptor's own defaults.
/// Schema keys are visited in sorted order, so a request with several faults
/// is refused for the same first fault on every host.
pub fn prepare(id: &str, mut request: Value) -> Result<Value, Refusal> {
    let mut d = describe(Some(id))?;
    d.sort_all_objects();
    normalize(&mut request, &d["input_schema"], "request")?;
    for name in ["source", "against"] {
        if let Some(source) = request.get(name) {
            if source.get("geojson").is_none()
                && source.get("layer").is_none()
                && source["layer_id"].as_str().is_none_or(str::is_empty)
                && source["file"].as_str().is_none_or(str::is_empty)
            {
                return Err(refuse(
                    "vector_request_invalid",
                    format!(
                        "request.{name}: supply an imported layer_id, file, or native GeoJSON import."
                    ),
                ));
            }
        }
    }
    let p = &request["parameters"];
    if tool(id)?.command == "random-points-area"
        && p["max_spacing_m"].as_f64() < p["min_spacing_m"].as_f64()
    {
        return Err(refuse(
            "vector_request_invalid",
            "max_spacing_m must be at least min_spacing_m.",
        ));
    }
    if tool(id)?.command == "outliers"
        && !["spatial_isolation", "size_outliers", "extent_outliers"]
            .iter()
            .any(|k| p[*k] == true)
    {
        return Err(refuse(
            "vector_request_invalid",
            "Enable at least one outlier detector.",
        ));
    }
    Ok(request)
}

/// Layer data crosses as typed IPC. A GeoJSON value anywhere in run controls
/// is refused; inline GeoJSON is admitted only at the explicit import edge.
pub fn reject_json_data(value: &Value) -> Result<(), Refusal> {
    match value {
        Value::Object(map) => {
            if matches!(
                value["type"].as_str(),
                Some(
                    "FeatureCollection"
                        | "Feature"
                        | "Point"
                        | "LineString"
                        | "Polygon"
                        | "MultiPoint"
                        | "MultiLineString"
                        | "MultiPolygon"
                        | "GeometryCollection"
                )
            ) {
                return Err(refuse(
                    "vector_request_invalid",
                    "bind IPC layers; GeoJSON is only accepted at file import and MapLibre boundaries",
                ));
            }
            map.values().try_for_each(reject_json_data)
        }
        Value::Array(values) => values.iter().try_for_each(reject_json_data),
        _ => Ok(()),
    }
}
