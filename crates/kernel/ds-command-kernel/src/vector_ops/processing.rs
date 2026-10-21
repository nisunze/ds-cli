//! Complete portable vector processing. No paths, credentials or host effects.
use super::*;
use serde_json::json;
use std::collections::BTreeMap;

pub const SCHEMA: &str = "ds.vector-processing/v1";

/// This changes only the output projection, never the input feature limit.
/// `CompleteProduced` returns every result made from the admitted, bounded
/// inputs. It does not claim that withheld source features were processed.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultProjection {
    #[default]
    Inline,
    CompleteProduced,
}

fn default_segments() -> u32 {
    8
}

/// One closed JSON boundary shared by native hosts and the WASM export.
/// GeoJSON is a document object here; CLI/MCP resolve their declared file or
/// JSON-text input before calling it. Coordinates and geometry algorithms
/// retain the existing vector contracts: finite longitude/latitude pairs,
/// spherical measures/buffers and degree-space crossing points. In particular,
/// a produced buffer is not automatically an admitted provider-query polygon.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum VectorRequest {
    Buffer {
        schema: String,
        source: Value,
        radius_m: f64,
        #[serde(default = "default_segments")]
        segments: u32,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        result_projection: ResultProjection,
    },
    Sample {
        schema: String,
        source: Value,
        interval_m: f64,
        #[serde(default)]
        include_ends: bool,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        result_projection: ResultProjection,
    },
    Intersect {
        schema: String,
        source: Value,
        against: Value,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        result_projection: ResultProjection,
    },
    Measure {
        schema: String,
        source: Value,
        #[serde(default)]
        limit: Option<usize>,
    },
}

impl VectorRequest {
    fn schema(&self) -> &str {
        match self {
            Self::Buffer { schema, .. }
            | Self::Sample { schema, .. }
            | Self::Intersect { schema, .. }
            | Self::Measure { schema, .. } => schema,
        }
    }

    fn limit(&self) -> Option<usize> {
        match self {
            Self::Buffer { limit, .. }
            | Self::Sample { limit, .. }
            | Self::Intersect { limit, .. }
            | Self::Measure { limit, .. } => *limit,
        }
    }
}

fn request_invalid(message: impl Into<String>) -> VectorRefusal {
    VectorRefusal::new(
        "vector_request_invalid",
        message,
        "Pass a ds.vector-processing/v1 request with one declared operation and its fields.",
    )
}

/// Parse, admit and execute one request. The native error retains its typed
/// code/message/remedy; the WASM binding serializes that same refusal.
pub fn evaluate(input: &[u8]) -> Result<String, VectorRefusal> {
    let request: VectorRequest = serde_json::from_slice(input)
        .map_err(|error| request_invalid(format!("Invalid vector request: {error}")))?;
    let result = execute(&request)?;
    serde_json::to_string(&result)
        .map_err(|error| request_invalid(format!("Cannot encode vector result: {error}")))
}

/// Typed native entry point, with the same parameter admission as JSON.
pub fn execute(request: &VectorRequest) -> Result<Value, VectorRefusal> {
    if request.schema() != SCHEMA {
        return Err(request_invalid("Unsupported vector request schema."));
    }
    let limit = resolve_limit(request.limit())?;
    match request {
        VectorRequest::Buffer {
            source,
            radius_m,
            segments,
            result_projection,
            ..
        } => {
            let radius = resolve_distance_m(*radius_m, "radius-m", 0.01, 100_000.0)?;
            if !(1..=64).contains(segments) {
                return Err(VectorRefusal::new(
                    "vector_distance_out_of_range",
                    "--segments must be a whole number from 1 to 64.",
                    "Pass a whole number of arc segments from 1 to 64, or omit it for 8.",
                ));
            }
            let decided = plan(source, VectorOperation::Buffer, limit)?;
            let mut features = Vec::new();
            for entry in &decided.planned {
                for part in &entry.parts {
                    let ring = ds_geo_lite::buffer_geometry(
                        entry.kind.engine_code(),
                        part,
                        radius,
                        *segments,
                    );
                    if ring.is_empty() {
                        continue;
                    }
                    features.push(json!({
                        "type": "Feature",
                        "properties": {
                            "source_index": entry.index,
                            "source_id": entry.id,
                            "source_kind": entry.kind.token(),
                            "buffer_radius_m": radius,
                        },
                        "geometry": {
                            "type": "Polygon",
                            "coordinates": [ring_to_positions(&ring)],
                        },
                    }));
                }
            }
            if features.is_empty() {
                return Err(VectorRefusal::new(
                    "vector_engine_empty",
                    "The buffer engine returned no ring for any eligible feature.",
                    "Check the coordinates are WGS-84 lon/lat degrees and the radius suits their scale.",
                ));
            }
            Ok(produced(
                &decided,
                features,
                limit,
                *result_projection,
                "zone",
                json!({ "radius_m": radius, "segments": segments }),
            ))
        }
        VectorRequest::Sample {
            source,
            interval_m,
            include_ends,
            result_projection,
            ..
        } => {
            let interval = resolve_distance_m(*interval_m, "interval-m", 0.01, 1_000_000.0)?;
            let decided = plan(source, VectorOperation::Sample, limit)?;
            let mut features = Vec::new();
            for entry in &decided.planned {
                for (part_index, part) in entry.parts.iter().enumerate() {
                    let stationed = ds_geo_lite::points_along_line(part, interval, *include_ends);
                    for point in stationed.chunks_exact(3) {
                        features.push(json!({
                            "type": "Feature",
                            "properties": {
                                "source_index": entry.index,
                                "source_id": entry.id,
                                "part_index": part_index,
                                "distance_m": point[2],
                            },
                            "geometry": { "type": "Point", "coordinates": [point[0], point[1]] },
                        }));
                    }
                }
            }
            let mut answer = produced(
                &decided,
                features,
                limit,
                *result_projection,
                "point",
                json!({ "interval_m": interval, "include_ends": include_ends }),
            );
            nothing_found(
                &mut answer,
                "No station fits: every line is shorter than --interval-m. Pass a \
                 smaller interval, or --include-ends to place a point at each line end.",
            );
            Ok(answer)
        }
        VectorRequest::Intersect {
            source,
            against,
            result_projection,
            ..
        } => {
            let source = plan(source, VectorOperation::Intersect, limit)?;
            let against = plan(against, VectorOperation::Intersect, limit)?;
            let mut features = Vec::new();
            for entry in &source.planned {
                for part in &entry.parts {
                    for other in &against.planned {
                        for other_part in &other.parts {
                            let hits = ds_geo_lite::find_intersections(part, other_part);
                            for point in hits.chunks_exact(2) {
                                features.push(json!({
                                    "type": "Feature",
                                    "properties": {
                                        "source_index": entry.index,
                                        "source_id": entry.id,
                                        "against_index": other.index,
                                        "against_id": other.id,
                                    },
                                    "geometry": { "type": "Point", "coordinates": [point[0], point[1]] },
                                }));
                            }
                        }
                    }
                }
            }
            let mut answer = produced(
                &source,
                features,
                limit,
                *result_projection,
                "crossing",
                json!({ "against_features": against.source_features }),
            );
            also_withheld(&mut answer, against.withheld_note("against"));
            nothing_found(
                &mut answer,
                "No crossing: no line in --source meets a line in --against.",
            );
            Ok(answer)
        }
        VectorRequest::Measure { source, .. } => measure(source, limit),
    }
}

fn ring_to_positions(flat: &[f64]) -> Vec<Value> {
    flat.chunks_exact(2)
        .map(|pair| json!([pair[0], pair[1]]))
        .collect()
}

fn skipped_summary(decided: &VectorPlan) -> Value {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for entry in &decided.skipped {
        *counts.entry(entry.reason.token()).or_default() += 1;
    }
    json!(counts)
}

fn produced(
    decided: &VectorPlan,
    mut features: Vec<Value>,
    limit: usize,
    projection: ResultProjection,
    noun: &str,
    extra: Value,
) -> Value {
    let count = features.len();
    let withheld = match projection {
        ResultProjection::Inline => bound_inline(&mut features, limit, noun),
        ResultProjection::CompleteProduced => None,
    };
    let mut answer = json!({
        "operation": decided.operation.token(),
        "source_features": decided.source_features,
        "processed": decided.planned.len(),
        "produced": count,
        "skipped": skipped_summary(decided),
        "written_to": Value::Null,
        "result": { "type": "FeatureCollection", "features": features },
    });
    if let Some(more) = &decided.more {
        answer["more"] = json!(more);
    }
    if let Some(fields) = extra.as_object() {
        for (key, value) in fields {
            answer[key.as_str()] = value.clone();
        }
    }
    also_withheld(&mut answer, withheld);
    answer
}

fn also_withheld(answer: &mut Value, note: Option<String>) {
    if let Some(note) = note {
        answer["more"] = json!(match answer["more"].as_str() {
            Some(existing) => format!("{existing}; {note}"),
            None => note,
        });
    }
}

fn nothing_found(answer: &mut Value, note: &str) {
    if answer["produced"] == json!(0) {
        answer["note"] = json!(note);
    }
}

fn part_length_m(part: &[f64]) -> f64 {
    let total: f64 = part
        .chunks_exact(2)
        .zip(part.chunks_exact(2).skip(1))
        .map(|(from, to)| ds_geo_lite::haversine_m(from[0], from[1], to[0], to[1]))
        .sum();
    total + 0.0
}

fn measured(entry: &PlannedFeature) -> (f64, f64) {
    let perimeter: f64 = entry.parts.iter().map(|part| part_length_m(part)).sum();
    match entry.kind {
        GeometryKind::Point => (0.0, 0.0),
        GeometryKind::Line => (perimeter, 0.0),
        GeometryKind::Polygon => {
            let covered: f64 = entry
                .parts
                .iter()
                .map(|part| ds_geo_lite::spherical_polygon_area_m2(part))
                .sum();
            let holes: f64 = entry
                .interior
                .iter()
                .map(|ring| ds_geo_lite::spherical_polygon_area_m2(ring))
                .sum();
            (perimeter, (covered - holes).max(0.0))
        }
    }
}

fn measure(source: &Value, limit: usize) -> Result<Value, VectorRefusal> {
    // The limit bounds listed rows, not the total over eligible source data.
    let decided = plan_over_whole_document(source, VectorOperation::Measure)?;
    let mut by_kind: BTreeMap<&'static str, usize> = BTreeMap::new();
    let (mut length, mut area, mut vertices) = (0.0, 0.0, 0usize);
    let mut features = Vec::new();
    for entry in &decided.planned {
        let (feature_length, feature_area) = measured(entry);
        length += feature_length;
        area += feature_area;
        vertices += entry.written_vertex_count();
        *by_kind.entry(entry.kind.token()).or_default() += 1;
        features.push(json!({
            "index": entry.index,
            "id": entry.id,
            "kind": entry.kind.token(),
            "parts": entry.parts.len(),
            "holes": entry.interior.len(),
            "vertices": entry.written_vertex_count(),
            "length_m": feature_length,
            "area_m2": feature_area,
        }));
    }
    let withheld = bound_listed_rows(&mut features, limit, "feature");
    let mut answer = json!({
        "operation": decided.operation.token(),
        "source_features": decided.source_features,
        "skipped": skipped_summary(&decided),
        "totals": {
            "features": decided.planned.len(),
            "by_kind": by_kind,
            "vertices": vertices,
            "length_m": length,
            "area_m2": area,
        },
        "features": features,
    });
    also_withheld(&mut answer, withheld);
    Ok(answer)
}
