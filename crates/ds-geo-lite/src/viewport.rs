//! GeoJSON extent math used by viewport consumers.
//!
//! Contract: `docs/contracts/geojson-foundation-crate.md` §7.

use serde_json::Value;

/// Compute the bbox of arbitrary GeoJSON.
///
/// Accepts Feature, FeatureCollection, Geometry, or GeometryCollection.
/// Returns `[west, south, east, north]` or `None` when the input has no
/// finite WGS84 coordinates (empty collection, all-null geometries,
/// out-of-range coords from a mis-projected file). The intent is to give
/// ds-web a single Rust source for map-fit math so the renderer can
/// `map.fitBounds(...)` without re-walking coordinates in TS.
///
/// Inputs with coordinates outside the WGS84 range are excluded from the
/// extent the same way `indexing::coord_pair` excludes them — silently
/// dropping garbage coordinates and computing bounds over the valid
/// remainder is preferred over returning a nonsensical bbox.
pub fn geojson_bounds(value: &Value) -> Option<[f64; 4]> {
    let mut min_lng = f64::INFINITY;
    let mut min_lat = f64::INFINITY;
    let mut max_lng = f64::NEG_INFINITY;
    let mut max_lat = f64::NEG_INFINITY;
    let mut any = false;

    walk(value, &mut |lng, lat| {
        any = true;
        if lng < min_lng {
            min_lng = lng;
        }
        if lng > max_lng {
            max_lng = lng;
        }
        if lat < min_lat {
            min_lat = lat;
        }
        if lat > max_lat {
            max_lat = lat;
        }
    });

    any.then_some([min_lng, min_lat, max_lng, max_lat])
}

fn walk<F: FnMut(f64, f64)>(value: &Value, extend: &mut F) {
    match value.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => {
            if let Some(features) = value.get("features").and_then(Value::as_array) {
                for feature in features {
                    if let Some(geom) = feature.get("geometry") {
                        walk_geometry(geom, extend);
                    }
                }
            }
        }
        Some("Feature") => {
            if let Some(geom) = value.get("geometry") {
                walk_geometry(geom, extend);
            }
        }
        Some(_) => walk_geometry(value, extend),
        None => {}
    }
}

fn walk_geometry<F: FnMut(f64, f64)>(geom: &Value, extend: &mut F) {
    if geom.is_null() {
        return;
    }
    if geom.get("type").and_then(Value::as_str) == Some("GeometryCollection") {
        if let Some(parts) = geom.get("geometries").and_then(Value::as_array) {
            for part in parts {
                walk_geometry(part, extend);
            }
        }
        return;
    }
    if let Some(coords) = geom.get("coordinates") {
        walk_coords(coords, extend);
    }
}

fn walk_coords<F: FnMut(f64, f64)>(value: &Value, extend: &mut F) {
    let Some(arr) = value.as_array() else { return };
    // Leaf: [lng, lat] pair.
    if let (Some(lng), Some(lat)) = (
        arr.first().and_then(Value::as_f64),
        arr.get(1).and_then(Value::as_f64),
    ) {
        if lng.is_finite()
            && lat.is_finite()
            && (-180.0..=180.0).contains(&lng)
            && (-90.0..=90.0).contains(&lat)
        {
            extend(lng, lat);
        }
        return;
    }
    for child in arr {
        walk_coords(child, extend);
    }
}

#[cfg(test)]
mod bounds_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn computes_feature_collection_bounds() {
        let fc = json!({
            "type": "FeatureCollection",
            "features": [
                {"type": "Feature", "geometry": {"type": "Point", "coordinates": [30.0, -2.0]}, "properties": {}},
                {"type": "Feature", "geometry": {"type": "Point", "coordinates": [30.5, -1.5]}, "properties": {}}
            ]
        });
        assert_eq!(geojson_bounds(&fc), Some([30.0, -2.0, 30.5, -1.5]));
    }

    #[test]
    fn handles_nested_polygon_rings() {
        let poly = json!({
            "type": "Polygon",
            "coordinates": [[[30.0, -2.0], [30.1, -2.0], [30.1, -1.9], [30.0, -1.9], [30.0, -2.0]]]
        });
        assert_eq!(geojson_bounds(&poly), Some([30.0, -2.0, 30.1, -1.9]));
    }

    #[test]
    fn returns_none_for_empty_feature_collection() {
        let empty = json!({"type": "FeatureCollection", "features": []});
        assert!(geojson_bounds(&empty).is_none());
    }

    #[test]
    fn out_of_range_coords_are_excluded() {
        // First feature is mis-projected (UTM eastings); second is valid.
        // Bounds should reflect only the valid coords, not a mashed bbox.
        let fc = json!({
            "type": "FeatureCollection",
            "features": [
                {"type": "Feature", "geometry": {"type": "Point", "coordinates": [504000.0, -2.0]}, "properties": {}},
                {"type": "Feature", "geometry": {"type": "Point", "coordinates": [30.5, -1.5]}, "properties": {}}
            ]
        });
        assert_eq!(geojson_bounds(&fc), Some([30.5, -1.5, 30.5, -1.5]));
    }

    #[test]
    fn handles_geometry_collection() {
        let gc = json!({
            "type": "GeometryCollection",
            "geometries": [
                {"type": "Point", "coordinates": [30.0, -2.0]},
                {"type": "LineString", "coordinates": [[30.5, -1.5], [30.6, -1.4]]}
            ]
        });
        assert_eq!(geojson_bounds(&gc), Some([30.0, -2.0, 30.6, -1.4]));
    }
}
