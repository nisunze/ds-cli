//! Shared measurements for WGS84 GeoJSON. Rendering hosts only format units.
use serde_json::Value;

/// Geodesic length of a LineString or MultiLineString in metres.
/// Unsupported or malformed geometry has no length; do not measure across
/// disconnected parts or drop invalid vertices to invent connecting segments.
pub fn geojson_line_length_m(geometry: &Value) -> Option<f64> {
    let coordinates = geometry.get("coordinates")?;
    match geometry.get("type")?.as_str()? {
        "LineString" => line_length(coordinates),
        "MultiLineString" => coordinates
            .as_array()?
            .iter()
            .try_fold(0.0, |total, line| Some(total + line_length(line)?)),
        _ => None,
    }
}

fn line_length(coordinates: &Value) -> Option<f64> {
    let points = coordinates.as_array()?;
    if points.len() < 2 {
        return None;
    }
    let mut previous = coordinate(&points[0])?;
    let mut total = 0.0;
    for point in &points[1..] {
        let next = coordinate(point)?;
        total += crate::haversine_m(previous[0], previous[1], next[0], next[1]);
        previous = next;
    }
    Some(total)
}

fn coordinate(value: &Value) -> Option<[f64; 2]> {
    let values = value.as_array()?;
    let x = values.first()?.as_f64()?;
    let y = values.get(1)?.as_f64()?;
    (x.is_finite() && y.is_finite() && (-180.0..=180.0).contains(&x) && (-90.0..=90.0).contains(&y))
        .then_some([x, y])
}

#[derive(serde::Deserialize)]
pub struct MeasureFrameInput {
    #[serde(default)]
    pub vertices: Vec<[f64; 2]>,
    #[serde(default)]
    pub mode: String,
}

#[derive(serde::Serialize)]
pub struct MeasureFrameOutput {
    pub segment_lengths_m: Vec<f64>,
    pub cumulative_lengths_m: Vec<f64>,
    pub total_length_m: f64,
    pub area_m2: Option<f64>,
    pub centroid: Option<[f64; 2]>,
}

/// Compute one measurement frame from inline vertices. MapLibre owns display;
/// WASM owns distance, cumulative length, closing-segment area data, and the
/// centroid used for the area label.
pub fn measure_frame(input: MeasureFrameInput) -> MeasureFrameOutput {
    let vertices = input.vertices;
    let area_mode = input.mode == "area" && vertices.len() >= 3;
    let segment_count = if area_mode {
        vertices.len()
    } else {
        vertices.len().saturating_sub(1)
    };
    let mut segment_lengths_m = Vec::with_capacity(segment_count);
    let mut cumulative_lengths_m = vec![0.0];
    for i in 0..segment_count {
        let a = vertices[i];
        let b = vertices[(i + 1) % vertices.len()];
        let length = crate::haversine_m(a[0], a[1], b[0], b[1]);
        segment_lengths_m.push(length);
        if i + 1 < vertices.len() {
            let previous = *cumulative_lengths_m.last().unwrap_or(&0.0);
            cumulative_lengths_m.push(previous + length);
        }
    }
    let total_length_m = segment_lengths_m.iter().copied().sum();
    let (area_m2, centroid) = if area_mode {
        let flat: Vec<f64> = vertices
            .iter()
            .flat_map(|coord| [coord[0], coord[1]])
            .collect();
        let area = crate::spherical_polygon_area_m2(&flat);
        let mut lng = 0.0;
        let mut lat = 0.0;
        for coord in &vertices {
            lng += coord[0];
            lat += coord[1];
        }
        let count = vertices.len() as f64;
        (Some(area), Some([lng / count, lat / count]))
    } else {
        (None, None)
    };
    MeasureFrameOutput {
        segment_lengths_m,
        cumulative_lengths_m,
        total_length_m,
        area_m2,
        centroid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn measurement_frames_keep_empty_and_closed_ring_label_contracts() {
        let empty = measure_frame(serde_json::from_value(json!({})).unwrap());
        assert!(empty.segment_lengths_m.is_empty());
        assert_eq!(empty.cumulative_lengths_m, vec![0.0]);
        assert_eq!(empty.total_length_m, 0.0);
        assert_eq!(empty.area_m2, None);
        let ring = measure_frame(MeasureFrameInput {
            vertices: vec![[0., 0.], [1., 0.], [1., 1.], [0., 0.]],
            mode: "area".into(),
        });
        assert_eq!(ring.segment_lengths_m.len(), 4);
        assert_eq!(ring.segment_lengths_m[3], 0.0);
        assert_eq!(ring.cumulative_lengths_m.len(), 4);
        assert_eq!(ring.centroid, Some([0.5, 0.25]));
        assert!(ring.area_m2.unwrap() > 0.0);
        let line = measure_frame(MeasureFrameInput {
            vertices: vec![[0., 0.], [0., 1.]],
            mode: "line".into(),
        });
        let meridian = crate::EARTH_RADIUS_M * 1_f64.to_radians();
        assert!((line.total_length_m - meridian).abs() < 1e-9);
        assert_eq!(line.cumulative_lengths_m, vec![0.0, line.total_length_m]);
        assert_eq!(line.centroid, None);
    }

    #[test]
    fn measures_parts_without_connecting_them_and_ignores_altitude() {
        let segment = json!({"type":"LineString","coordinates":[[30,-2,100],[30.01,-2,400]]});
        let length = geojson_line_length_m(&segment).unwrap();
        assert!((1100.0..1120.0).contains(&length));
        let parts = json!({"type":"MultiLineString","coordinates":[[[30,-2],[30.01,-2]],[[50,2],[50.01,2]]]});
        assert!((geojson_line_length_m(&parts).unwrap() - 2.0 * length).abs() < 1e-6);
        assert_eq!(
            geojson_line_length_m(&json!({"type":"LineString","coordinates":[[30,-2],[30,-2]]})),
            Some(0.0)
        );
    }

    #[test]
    fn never_invents_length_for_invalid_or_non_line_geometry() {
        for geometry in [
            json!({"type":"Point","coordinates":[30,-2]}),
            json!({"type":"LineString","coordinates":[[30,-2]]}),
            json!({"type":"LineString","coordinates":[[30,-2],[300000,-20000],[30.01,-2]]}),
            json!({"type":"LineString","coordinates":[[30,-2],[null,-2]]}),
            json!({"type":"MultiLineString","coordinates":[[[30,-2],[30.01,-2]],[]]}),
        ] {
            assert_eq!(geojson_line_length_m(&geometry), None, "{geometry}");
        }
    }
}
