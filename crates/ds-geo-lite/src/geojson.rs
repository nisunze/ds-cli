//! Geometry traversal independent of projection selection and bindings.
use serde_json::Value;

/// Map finite XY positions through an explicit caller transform. Additional
/// ordinates, properties, authored bbox fields and malformed leaves are kept.
pub fn map_coordinates(value: &mut Value, transform: impl Fn([f64; 2]) -> [f64; 2]) {
    map_value(value, &transform);
}

fn map_value(value: &mut Value, transform: &impl Fn([f64; 2]) -> [f64; 2]) {
    match value.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => {
            if let Some(features) = value.get_mut("features").and_then(Value::as_array_mut) {
                for feature in features {
                    map_value(feature, transform);
                }
            }
        }
        Some("Feature") => {
            if let Some(geometry) = value.get_mut("geometry") {
                map_geometry(geometry, transform);
            }
        }
        Some(_) => map_geometry(value, transform),
        None => {
            if let Some(items) = value.as_array_mut() {
                for item in items {
                    map_value(item, transform);
                }
            }
        }
    }
}

fn map_geometry(geometry: &mut Value, transform: &impl Fn([f64; 2]) -> [f64; 2]) {
    match geometry.get("type").and_then(Value::as_str) {
        Some("GeometryCollection") => {
            if let Some(geometries) = geometry.get_mut("geometries").and_then(Value::as_array_mut) {
                for inner in geometries {
                    map_geometry(inner, transform);
                }
            }
        }
        Some(_) => {
            if let Some(coordinates) = geometry.get_mut("coordinates") {
                map_positions(coordinates, transform);
            }
        }
        None => {}
    }
}

/// Recursively map the leaf `[x, y, ..]` positions (any nesting depth) through
/// the caller transform, editing the first two ordinates in place.
fn map_positions(node: &mut Value, transform: &impl Fn([f64; 2]) -> [f64; 2]) {
    let Value::Array(items) = node else { return };
    if items.len() >= 2 && items[0].is_number() && items[1].is_number() {
        let x = items[0].as_f64().unwrap_or(f64::NAN);
        let y = items[1].as_f64().unwrap_or(f64::NAN);
        if x.is_finite() && y.is_finite() {
            let [lng, lat] = transform([x, y]);
            items[0] = Value::from(lng);
            items[1] = Value::from(lat);
        }
        return;
    }
    for item in items.iter_mut() {
        map_positions(item, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn traversal_keeps_payloads_and_malformed_leaves() {
        let mut value = json!([{"type":"FeatureCollection","bbox":[0,0,1,1],"features":[
            {"type":"Feature","properties":{"coordinates":[2,3]},"geometry":
                {"type":"GeometryCollection","geometries":[
                    {"type":"Point","coordinates":[2,3,17]},
                    {"type":"LineString","coordinates":[[4,5],["bad",6],[7]]}
                ]}}
        ]}, {"type":"Feature","geometry":null}]);
        map_coordinates(&mut value, |[x, y]| [x + 10., y - 10.]);
        assert_eq!(
            value[0]["features"][0]["geometry"]["geometries"][0]["coordinates"],
            json!([12., -7., 17])
        );
        assert_eq!(
            value[0]["features"][0]["geometry"]["geometries"][1]["coordinates"],
            json!([[14., -5.], ["bad", 6], [7]])
        );
        assert_eq!(
            value[0]["features"][0]["properties"]["coordinates"],
            json!([2, 3])
        );
        assert_eq!(value[0]["bbox"], json!([0, 0, 1, 1]));
        assert!(value[1]["geometry"].is_null());
    }
}
