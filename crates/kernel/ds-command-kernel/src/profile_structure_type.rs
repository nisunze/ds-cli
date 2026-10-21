//! Profile structure-type suggestions shared by native and browser hosts.
//!
//! The host supplies library rows and one projected alignment. This module
//! owns tolerant angle-range decoding, bend calculation, and the deterministic
//! "smallest covering range wins" rule. It performs no IO and never commits a
//! placement; the result is an authoring suggestion the operator may ignore.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const SCHEMA: &str = "ds.profile-structure-type-selection/v1";
pub const MAX_INPUT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_TYPES: usize = 4096;
pub const MAX_COORDINATES: usize = 100_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructureTypeInput {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub height_m: Option<f64>,
    pub payload: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AngleRange {
    pub min_deg: f64,
    pub max_deg: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureTypeOption {
    pub id: String,
    pub name: String,
    pub height_m: Option<f64>,
    pub angle_range: Option<AngleRange>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Normalize {
        types: Vec<StructureTypeInput>,
    },
    Select {
        types: Vec<StructureTypeInput>,
        coordinates: Vec<Vec<f64>>,
        station_m: f64,
    },
}

#[derive(Debug, Serialize)]
struct Reply {
    schema: &'static str,
    types: Vec<StructureTypeOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deflection_deg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected: Option<StructureTypeOption>,
}

fn finite_number(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(number)) => number.as_f64().filter(|value| value.is_finite()),
        Some(Value::String(text)) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite()),
        _ => None,
    }
}

/// Decode the characterized angle aliases in precedence order. Missing and
/// malformed declarations remain absent; the kernel never guesses a range.
pub fn structure_type_angle_range(payload: &Map<String, Value>) -> Option<AngleRange> {
    let min = [
        "allowable_line_angle_min_deg",
        "line_angle_min_deg",
        "min_line_angle_deg",
    ]
    .into_iter()
    .find_map(|key| finite_number(payload.get(key)));
    let max = [
        "allowable_line_angle_max_deg",
        "line_angle_max_deg",
        "max_line_angle_deg",
    ]
    .into_iter()
    .find_map(|key| finite_number(payload.get(key)))?;
    let min = min.unwrap_or(0.0);
    (max >= min).then_some(AngleRange {
        min_deg: min,
        max_deg: max,
    })
}

fn validate_text(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        return Err(format!(
            "profile structure type {field} must be nonempty, bounded text"
        ));
    }
    Ok(())
}

pub fn normalize_types(types: Vec<StructureTypeInput>) -> Result<Vec<StructureTypeOption>, String> {
    if types.len() > MAX_TYPES {
        return Err(format!("profile structure type count exceeds {MAX_TYPES}"));
    }
    types
        .into_iter()
        .map(|candidate| {
            validate_text(&candidate.id, "id")?;
            validate_text(&candidate.name, "name")?;
            if candidate.height_m.is_some_and(|value| !value.is_finite()) {
                return Err("profile structure type height must be finite".to_string());
            }
            Ok(StructureTypeOption {
                id: candidate.id,
                name: candidate.name,
                height_m: candidate.height_m,
                angle_range: structure_type_angle_range(&candidate.payload),
            })
        })
        .collect()
}

/// Deflection in degrees at the interior vertex nearest `station_m`.
/// Coordinates are WGS84 `[longitude, latitude, ...]`; terminals have no
/// deflection. The local planar projection is an authoring assist only.
pub fn alignment_deflection_at_station_deg(
    coordinates: &[Vec<f64>],
    station_m: f64,
) -> Result<Option<f64>, String> {
    if coordinates.len() > MAX_COORDINATES {
        return Err(format!(
            "profile alignment coordinate count exceeds {MAX_COORDINATES}"
        ));
    }
    if !station_m.is_finite() || station_m < 0.0 {
        return Err("profile station must be a finite nonnegative number".to_string());
    }
    if coordinates.len() < 3 {
        return Ok(None);
    }
    if coordinates
        .iter()
        .any(|point| point.len() < 2 || !point[0].is_finite() || !point[1].is_finite())
    {
        return Err(
            "profile alignment coordinates must carry finite longitude and latitude".to_string(),
        );
    }

    let mid_lat_rad =
        ((coordinates[0][1] + coordinates[coordinates.len() - 1][1]) / 2.0).to_radians();
    let metres_per_degree_lat = 111_320.0;
    let metres_per_degree_lon = metres_per_degree_lat * mid_lat_rad.cos();
    let points = coordinates
        .iter()
        .map(|point| {
            (
                point[0] * metres_per_degree_lon,
                point[1] * metres_per_degree_lat,
            )
        })
        .collect::<Vec<_>>();
    let mut stations = Vec::with_capacity(points.len());
    stations.push(0.0);
    for index in 1..points.len() {
        let dx = points[index].0 - points[index - 1].0;
        let dy = points[index].1 - points[index - 1].1;
        stations.push(stations[index - 1] + dx.hypot(dy));
    }
    let vertex_index = (1..points.len() - 1)
        .min_by(|left, right| {
            (stations[*left] - station_m)
                .abs()
                .total_cmp(&(stations[*right] - station_m).abs())
        })
        .expect("an alignment with at least three points has one interior vertex");
    let previous = points[vertex_index - 1];
    let vertex = points[vertex_index];
    let next = points[vertex_index + 1];
    let bearing_in = (vertex.0 - previous.0).atan2(vertex.1 - previous.1);
    let bearing_out = (next.0 - vertex.0).atan2(next.1 - vertex.1);
    let mut deflection = (bearing_out - bearing_in).abs().to_degrees();
    if deflection > 180.0 {
        deflection = 360.0 - deflection;
    }
    Ok(Some(deflection))
}

/// Select the first narrowest declared range that covers the deflection.
/// Input order breaks equal-width ties, preserving library order.
pub fn select_structure_type_for_angle(
    types: &[StructureTypeOption],
    deflection_deg: f64,
) -> Result<Option<StructureTypeOption>, String> {
    if !deflection_deg.is_finite() || !(0.0..=180.0).contains(&deflection_deg) {
        return Err("profile deflection must be finite and within 0..=180 degrees".to_string());
    }
    let mut selected: Option<(&StructureTypeOption, f64)> = None;
    for candidate in types {
        let Some(range) = candidate.angle_range else {
            continue;
        };
        if deflection_deg < range.min_deg || deflection_deg > range.max_deg {
            continue;
        }
        let width = range.max_deg - range.min_deg;
        if selected
            .as_ref()
            .is_none_or(|(_, selected_width)| width < *selected_width)
        {
            selected = Some((candidate, width));
        }
    }
    Ok(selected.map(|(candidate, _)| candidate.clone()))
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "profile structure type request exceeds {MAX_INPUT_BYTES} bytes"
        ));
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid profile structure type request: {error}"))?;
    let reply = match request {
        Request::Normalize { types } => Reply {
            schema: SCHEMA,
            types: normalize_types(types)?,
            deflection_deg: None,
            selected: None,
        },
        Request::Select {
            types,
            coordinates,
            station_m,
        } => {
            let types = normalize_types(types)?;
            let deflection_deg = alignment_deflection_at_station_deg(&coordinates, station_m)?;
            let selected = deflection_deg
                .map(|angle| select_structure_type_for_angle(&types, angle))
                .transpose()?
                .flatten();
            Reply {
                schema: SCHEMA,
                types,
                deflection_deg,
                selected,
            }
        }
    };
    serde_json::to_string(&reply)
        .map_err(|error| format!("encode profile structure type reply: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn candidate(id: &str, min: Option<Value>, max: Option<Value>) -> StructureTypeInput {
        let mut payload = Map::new();
        if let Some(value) = min {
            payload.insert("allowable_line_angle_min_deg".to_string(), value);
        }
        if let Some(value) = max {
            payload.insert("allowable_line_angle_max_deg".to_string(), value);
        }
        StructureTypeInput {
            id: id.to_string(),
            name: id.to_string(),
            height_m: Some(10.0),
            payload,
        }
    }

    #[test]
    fn angle_aliases_are_tolerant_without_guessing() {
        let primary = serde_json::from_value::<Map<String, Value>>(json!({
            "allowable_line_angle_min_deg": "2",
            "allowable_line_angle_max_deg": 30
        }))
        .unwrap();
        assert_eq!(
            structure_type_angle_range(&primary),
            Some(AngleRange {
                min_deg: 2.0,
                max_deg: 30.0
            })
        );
        let legacy = serde_json::from_value::<Map<String, Value>>(json!({
            "max_line_angle_deg": 15
        }))
        .unwrap();
        assert_eq!(
            structure_type_angle_range(&legacy),
            Some(AngleRange {
                min_deg: 0.0,
                max_deg: 15.0
            })
        );
        let invalid = serde_json::from_value::<Map<String, Value>>(json!({
            "line_angle_min_deg": 30,
            "line_angle_max_deg": 10
        }))
        .unwrap();
        assert_eq!(structure_type_angle_range(&invalid), None);
        assert_eq!(structure_type_angle_range(&Map::new()), None);
    }

    #[test]
    fn nearest_interior_vertex_produces_the_expected_deflection() {
        let angle = alignment_deflection_at_station_deg(
            &[vec![30.0, 0.0], vec![30.01, 0.0], vec![30.01, 0.01]],
            1_100.0,
        )
        .unwrap()
        .unwrap();
        assert!((angle - 90.0).abs() < 0.5);
        assert_eq!(
            alignment_deflection_at_station_deg(&[vec![30.0, 0.0], vec![30.01, 0.0]], 500.0)
                .unwrap(),
            None
        );
    }

    #[test]
    fn narrowest_cover_wins_and_equal_width_preserves_library_order() {
        let types = normalize_types(vec![
            candidate("strain", Some(json!(0)), Some(json!(90))),
            candidate("angle", Some(json!(0)), Some(json!(35))),
            candidate("suspension", Some(json!(0)), Some(json!(5))),
            candidate("angle-equal", Some(json!(0)), Some(json!(35))),
            candidate("undeclared", None, None),
        ])
        .unwrap();
        assert_eq!(
            select_structure_type_for_angle(&types, 3.0)
                .unwrap()
                .unwrap()
                .id,
            "suspension"
        );
        assert_eq!(
            select_structure_type_for_angle(&types, 20.0)
                .unwrap()
                .unwrap()
                .id,
            "angle"
        );
        assert_eq!(
            select_structure_type_for_angle(&types, 70.0)
                .unwrap()
                .unwrap()
                .id,
            "strain"
        );
        assert_eq!(
            select_structure_type_for_angle(&types, 120.0).unwrap(),
            None
        );
    }

    #[test]
    fn json_contract_returns_normalized_types_and_one_suggestion() {
        let reply: Value = serde_json::from_str(
            &evaluate(
                &serde_json::to_vec(&json!({
                    "operation": "select",
                    "types": [
                        {"id":"wide","name":"Wide","height_m":12,"payload":{"max_line_angle_deg":90}},
                        {"id":"corner","name":"Corner","height_m":10,"payload":{"allowable_line_angle_min_deg":45,"allowable_line_angle_max_deg":95}}
                    ],
                    "coordinates": [[30,0],[30.01,0],[30.01,0.01]],
                    "station_m": 1100
                }))
                .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(reply["schema"], SCHEMA);
        assert_eq!(reply["selected"]["id"], "corner");
        assert_eq!(reply["types"][0]["angleRange"]["minDeg"], 0.0);
    }
}
