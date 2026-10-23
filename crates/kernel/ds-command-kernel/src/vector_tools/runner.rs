use super::admission::prepare;
use super::collisions;
use super::descriptor::{available, field, output_fields, refuse, tool, Refusal};
use ds_geo::layer::{Coordinates, Feature, Geometry, Layer};
use ds_geo_lite::vector_types::{GeometryKind, PlannedFeature, VectorOperation};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunOptions {
    #[serde(default)]
    pub dry_run: bool,
}
/// Control metadata and the single shared typed layer. JSON has no data twin.
pub struct ToolResult {
    pub metadata: Value,
    pub layer: Option<Arc<Layer>>,
}
/// Every source feature an answer leaves out, with its reason. `skipped` in
/// the answer counts each one by reason; `skipped_features` names them by
/// source index (and id) up to the output limit. A feature is skipped when it
/// is ineligible for the tool, and also when the operation yields no geometry
/// for it — nothing leaves the answer without a reason.
#[derive(Debug, Default)]
pub(crate) struct Skipped {
    counts: BTreeMap<&'static str, usize>,
    features: Vec<(usize, Option<String>, &'static str)>,
}
impl Skipped {
    pub(crate) fn add(&mut self, reason: &'static str, index: usize, id: Option<String>) {
        *self.counts.entry(reason).or_default() += 1;
        self.features.push((index, id, reason));
    }
    pub(crate) fn counts(&self) -> Value {
        json!(self.counts)
    }
    pub(crate) fn named(&self, limit: usize) -> Value {
        let mut rows = self.features.iter().collect::<Vec<_>>();
        rows.sort_by_key(|(index, _, _)| *index);
        Value::Array(
            rows.into_iter()
                .take(limit)
                .map(|(index, id, reason)| {
                    let mut row = json!({"index":index,"reason":reason});
                    if let Some(id) = id {
                        row["id"] = json!(id);
                    }
                    row
                })
                .collect(),
        )
    }
}
pub(crate) fn planned(
    layer: &Layer,
    operation: VectorOperation,
    limit: usize,
    argument: Option<&str>,
) -> Result<(Vec<PlannedFeature>, Skipped, Option<String>), Refusal> {
    if layer.features.is_empty() {
        return Err(refuse(
            "vector_document_empty",
            "The source has no features.",
        ));
    }
    let mut planned = Vec::new();
    let mut skipped = Skipped::default();
    let mut eligible = 0;
    for (index, feature) in layer.features.iter().enumerate() {
        let (geometry, kind) = match &feature.geometry {
            None => {
                let kind = match feature.geometry_kind_hint.as_deref() {
                    Some("Point" | "MultiPoint") => Some(GeometryKind::Point),
                    Some("LineString" | "MultiLineString") => Some(GeometryKind::Line),
                    Some("Polygon" | "MultiPolygon") => Some(GeometryKind::Polygon),
                    _ => None,
                };
                let reason = match kind {
                    Some(kind) if operation.accepts().contains(&kind) => "bad_coordinates",
                    Some(_) => "wrong_kind",
                    None if feature.geometry_kind_hint.is_some() => "wrong_kind",
                    None => "no_geometry",
                };
                skipped.add(reason, index, feature.identity());
                continue;
            }
            Some(g) => match g.engine_kind() {
                Some(0) => (g, GeometryKind::Point),
                Some(1) => (g, GeometryKind::Line),
                Some(2) => (g, GeometryKind::Polygon),
                _ => {
                    skipped.add("wrong_kind", index, feature.identity());
                    continue;
                }
            },
        };
        if !operation.accepts().contains(&kind) {
            skipped.add("wrong_kind", index, feature.identity());
            continue;
        }
        if matches!(geometry.coordinates, Coordinates::Missing) {
            skipped.add("no_geometry", index, feature.identity());
            continue;
        }
        let Some((parts, interior)) = geometry.parts() else {
            skipped.add("bad_coordinates", index, feature.identity());
            continue;
        };
        if parts.is_empty() {
            skipped.add("empty_geometry", index, feature.identity());
            continue;
        }
        eligible += 1;
        if planned.len() < limit {
            planned.push(PlannedFeature {
                index,
                id: feature.identity(),
                kind,
                parts,
                interior,
            })
        }
    }
    if eligible == 0 {
        return Err(refuse(
            "vector_no_eligible_feature",
            "No geometry is eligible for this tool.",
        ));
    }
    let more = (eligible > limit).then(|| match argument {
        Some(argument) => format!("{} feature(s) from --{argument} were not compared; raise --limit (max 20000) to include them", eligible - limit),
        None => format!("{} more eligible feature(s) were not returned; raise the limit (max 20000) to see them", eligible - limit),
    });
    Ok((planned, skipped, more))
}
fn produced_feature(geometry: Geometry, properties: Value) -> Feature {
    Feature {
        geometry_kind_hint: Some(geometry.kind.clone()),
        geometry: Some(geometry),
        properties: properties.as_object().unwrap().clone(),
        id: None,
    }
}
fn measured(entry: &PlannedFeature) -> (f64, f64) {
    let length = entry
        .parts
        .iter()
        .map(|p| {
            p.chunks_exact(2)
                .zip(p.chunks_exact(2).skip(1))
                .map(|(a, b)| ds_geo_lite::haversine_m(a[0], a[1], b[0], b[1]))
                .sum::<f64>()
                + 0.
        })
        .sum::<f64>();
    let area = if entry.kind == GeometryKind::Polygon {
        (entry
            .parts
            .iter()
            .map(|p| ds_geo_lite::spherical_polygon_area_m2(p))
            .sum::<f64>()
            - entry
                .interior
                .iter()
                .map(|p| ds_geo_lite::spherical_polygon_area_m2(p))
                .sum::<f64>())
        .max(0.)
    } else {
        0.
    };
    (
        if entry.kind == GeometryKind::Point {
            0.
        } else {
            length
        },
        area,
    )
}
pub fn run(
    id: &str,
    request: Value,
    source: Arc<Layer>,
    against: Option<Arc<Layer>>,
    options: RunOptions,
) -> Result<ToolResult, Refusal> {
    let t = tool(id)?;
    let request = prepare(id, request)?;
    if !available(&t.command) {
        return Err(refuse(
            "vector_tool_roadmap",
            format!("{} has a schema but no executable runner yet.", t.name),
        ));
    }
    if request["source"].get("geojson").is_some() || request["against"].get("geojson").is_some() {
        return Err(refuse(
            "vector_request_invalid",
            "Import data at the edge; the runner receives typed layers/IPC.",
        ));
    }
    guard_layers(id, &request, &source, against.as_deref())?;
    let p = &request["parameters"];
    let limit = request["output"]["limit"].as_u64().unwrap() as usize;
    let complete = request["output"]["projection"] == "complete";
    let mut answer = json!({"operation":t.command,"source_features":source.features.len(),"processed":0,"produced":0,"result":null});
    let mut layer = Layer::default();
    let mut warnings = Vec::<String>::new();
    match t.command.as_str() {
        "buffer" | "sample" | "intersect" | "measure" => {
            let operation = match t.command.as_str() {
                "buffer" => VectorOperation::Buffer,
                "sample" => VectorOperation::Sample,
                "intersect" => VectorOperation::Intersect,
                _ => VectorOperation::Measure,
            };
            let (entries, mut skipped, more) = planned(
                &source,
                operation,
                if t.command == "measure" {
                    usize::MAX
                } else {
                    limit
                },
                None,
            )?;
            let mut dropped = 0;
            if let Some(more) = more {
                answer["more"] = json!(more)
            }
            if t.command == "measure" {
                let (mut length, mut area, mut vertices) = (0., 0., 0);
                let mut by_kind = BTreeMap::new();
                let mut rows = Vec::new();
                for entry in &entries {
                    let (l, a) = measured(entry);
                    length += l;
                    area += a;
                    vertices += entry.written_vertex_count();
                    *by_kind.entry(entry.kind.token()).or_insert(0) += 1;
                    if rows.len() < limit {
                        rows.push(json!({"index":entry.index,"id":entry.id,"kind":entry.kind.token(),"parts":entry.parts.len(),"holes":entry.interior.len(),"vertices":entry.written_vertex_count(),"length_m":l,"area_m2":a}));
                        let mut feature = source.features[entry.index].clone();
                        for (name, value) in [
                            ("length_m", json!(l)),
                            ("area_m2", json!(a)),
                            ("vertices", json!(entry.written_vertex_count())),
                        ] {
                            feature.properties.insert(name.into(), value);
                        }
                        layer.features.push(feature)
                    }
                }
                answer["totals"] = json!({"features":entries.len(),"by_kind":by_kind,"vertices":vertices,"length_m":length,"area_m2":area});
                answer["features"] = json!(rows);
                if entries.len() > limit {
                    answer["more"] = json!(format!(
                        "{} more feature(s) were measured than listed; the totals count every one. Raise --limit (max 20000) to list more",
                        entries.len() - limit
                    ))
                }
            } else {
                let other = if t.command == "intersect" {
                    let against = against.as_ref().ok_or_else(|| {
                        refuse("vector_request_invalid", "Supply against IPC/layer.")
                    })?;
                    let (e, _, more) = planned(against, operation, limit, Some("against"))?;
                    answer["against_features"] = json!(against.features.len());
                    if let Some(more) = more {
                        answer["more"] = json!(match answer["more"].as_str() {
                            Some(source_more) => format!("{source_more}; {more}"),
                            None => more,
                        });
                    }
                    e
                } else {
                    vec![]
                };
                for entry in &entries {
                    let before = layer.features.len();
                    for (part_index, part) in entry.parts.iter().enumerate() {
                        match t.command.as_str() {
                            "buffer" => {
                                let radius = p["radius_m"].as_f64().unwrap();
                                let segments = p["segments"].as_u64().unwrap() as u32;
                                let ring = ds_geo_lite::buffer_geometry(
                                    entry.kind.engine_code(),
                                    part,
                                    radius,
                                    segments,
                                );
                                if !ring.is_empty() {
                                    layer.features.push(produced_feature(Geometry::polygon(&ring),json!({"source_index":entry.index,"source_id":entry.id,"source_kind":entry.kind.token(),"buffer_radius_m":radius})))
                                }
                                answer["radius_m"] = json!(radius);
                                answer["segments"] = json!(segments)
                            }
                            "sample" => {
                                let interval = p["interval_m"].as_f64().unwrap();
                                let ends = p["include_ends"].as_bool().unwrap();
                                for c in ds_geo_lite::points_along_line(part, interval, ends)
                                    .chunks_exact(3)
                                {
                                    layer.features.push(produced_feature(Geometry::point(c[0],c[1]),json!({"source_index":entry.index,"source_id":entry.id,"part_index":part_index,"distance_m":c[2]})))
                                }
                                answer["interval_m"] = json!(interval);
                                answer["include_ends"] = json!(ends)
                            }
                            _ => {
                                for other in &other {
                                    for op in &other.parts {
                                        for c in ds_geo_lite::find_intersections(part, op)
                                            .chunks_exact(2)
                                        {
                                            layer.features.push(produced_feature(Geometry::point(c[0],c[1]),json!({"source_index":entry.index,"source_id":entry.id,"against_index":other.index,"against_id":other.id})))
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // An eligible feature whose operation yields nothing is
                    // left out by name, never silently (a crossing-free line
                    // is a complete intersect answer, not a loss).
                    if layer.features.len() == before {
                        match t.command.as_str() {
                            "buffer" => skipped.add("engine_empty", entry.index, entry.id.clone()),
                            "sample" => skipped.add("no_station", entry.index, entry.id.clone()),
                            _ => continue,
                        }
                        dropped += 1;
                    }
                }
                if t.command == "buffer" && layer.features.is_empty() {
                    return Err(refuse(
                        "vector_engine_empty",
                        "The buffer engine returned no ring.",
                    ));
                }
                if layer.features.is_empty() {
                    answer["note"] = json!(if t.command == "sample" {
                        "No station fits: every line is shorter than --interval-m. Pass a smaller interval, or --include-ends to place a point at each line end."
                    } else {
                        "No crossing: no line in --source meets a line in --against."
                    })
                }
                if t.command == "sample" {
                    answer["line_part_count"] =
                        json!(entries.iter().map(|e| e.parts.len()).sum::<usize>())
                }
            }
            // processed + skipped (+ any limit-withheld rows named in `more`)
            // accounts for every source feature.
            answer["processed"] = json!(entries.len() - dropped);
            answer["skipped"] = skipped.counts();
            answer["skipped_features"] = skipped.named(limit);
        }
        "outliers" => {
            let result =
                ds_geo::analysis::detect_layer_outliers(ds_geo::analysis::TypedOutlierInput {
                    layer_name: request["source"]["name"].as_str().map(String::from),
                    features: source.features.iter().take(limit).cloned().collect(),
                    options: serde_json::from_value(p.clone())
                        .map_err(|e| refuse("vector_request_invalid", e.to_string()))?,
                });
            answer["processed"] = json!(result.report.feature_count);
            answer["report"] = json!(result.report);
            layer = result.layer;
            if source.features.len() > limit {
                answer["more"] = json!(format!(
                    "{} source features withheld by limit {limit}.",
                    source.features.len() - limit
                ))
            }
        }
        "random-points-area" => {
            let result = random(&source, &request, limit, options.dry_run)?;
            answer = result.0;
            layer = result.1
        }
        "collisions" => {
            answer["processed"] = json!(source.features.len());
            let mut report = collisions::summarize(
                &source.features,
                p["loaded"].as_bool().unwrap(),
                p["computed"].as_bool().unwrap(),
                p["last_read_computed"].as_bool(),
            );
            let rows = report["regions"].as_array_mut().unwrap();
            if !complete && rows.len() > limit {
                warnings.push(format!(
                    "{} regions withheld by limit {limit}.",
                    rows.len() - limit
                ));
                rows.truncate(limit)
            }
            let more =
                (!complete && source.features.len() > limit) || (options.dry_run && rows.len() > 5);
            report["more"] = json!(more);
            answer["report"] = report
        }
        _ => unreachable!(),
    }
    let produced = answer["produced"]
        .as_u64()
        .filter(|_| t.command == "random-points-area")
        .unwrap_or(layer.features.len() as u64);
    answer["produced"] = json!(produced);
    if t.command != "measure"
        && t.command != "collisions"
        && !complete
        && layer.features.len() > limit
    {
        let noun = match t.command.as_str() {
            "buffer" => "zone",
            "sample" => "point",
            "intersect" => "crossing",
            _ => "feature",
        };
        let note = format!(
            "{} more {noun}(s) were produced than returned inline; pass --out <path> to write them all, or raise --limit (max 20000)",
            layer.features.len() - limit
        );
        answer["more"] = json!(answer["more"]
            .as_str()
            .map(|s| format!("{s}; {note}"))
            .unwrap_or(note));
        layer.features.truncate(limit)
    }
    let mut fields = output_fields(&t.command);
    if t.command == "random-points-area" && p["sample_elevation"] == true {
        let readings = p["elevations_m"].as_array().unwrap();
        let delivered = if complete {
            produced as usize
        } else {
            (produced as usize).min(limit)
        };
        if readings.len() != delivered {
            return Err(refuse(
                "vector_external_data_required",
                "Supply elevations_m in returned point order, including null for missing terrain readings; the host acquires terrain separately.",
            ));
        }
        answer["elevation_gaps"] = json!(readings.iter().filter(|r| r.is_null()).count());
        for (f, r) in layer.features.iter_mut().zip(readings) {
            f.properties.insert("elevation_m".into(), r.clone());
        }
        fields.push(field("elevation_m", "number", "m"))
    }
    if t.command == "random-points-area" {
        answer["ui_stats"] = json!({"pointCount":if complete{produced}else{produced.min(limit as u64)},"sourceFeatureCount":answer["processed"],"areaCount":answer["area_count"],"bufferedCount":answer["buffered_count"],"skippedNoBuffer":answer["skipped"]["no_buffer"].as_u64().unwrap_or(0),"elevationSampled":p["sample_elevation"],"elevationGaps":answer["elevation_gaps"].as_u64().unwrap_or(0)})
    }
    let mut columns = BTreeMap::<String, String>::new();
    for f in &fields {
        columns.insert(
            f["name"].as_str().unwrap().into(),
            f["type"].as_str().unwrap().replace("string", "text"),
        );
    }
    for f in &layer.features {
        for (k, v) in &f.properties {
            columns.entry(k.clone()).or_insert_with(|| {
                if v.is_boolean() {
                    "boolean"
                } else if v.is_number() {
                    "number"
                } else {
                    "text"
                }
                .into()
            });
        }
    }
    let family = layer
        .features
        .first()
        .and_then(|f| f.geometry.as_ref())
        .map(Geometry::family)
        .unwrap_or("Point");
    let name = request["output"]["name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            format!(
                "{} {}",
                request["source"]["name"].as_str().unwrap_or("Source"),
                t.name
            )
        });
    answer["output_layer"] = json!({"name":name,"geometry_type":family,"fields":columns.into_iter().map(|(name,kind)|json!({"name":name,"type":kind})).collect::<Vec<_>>()});
    if let Some(more) = answer["more"].as_str() {
        warnings.push(more.into())
    }
    if let Some(note) = answer["note"].as_str() {
        warnings.push(note.into())
    }
    if answer["skipped"].as_object().is_some_and(|s| !s.is_empty()) {
        warnings.push(format!("Skipped geometry: {}", answer["skipped"]))
    }
    if t.command == "measure"
        && source.features.iter().any(|f| {
            ["length_m", "area_m2", "vertices"]
                .iter()
                .any(|k| f.properties.contains_key(*k))
        })
    {
        warnings.push(
            "Authored length_m, area_m2 and vertices fields are replaced in the derived output."
                .into(),
        )
    }
    answer["tool_id"] = json!(t.id);
    answer["dry_run"] = json!(options.dry_run);
    answer["fields_added"] = json!(fields);
    answer["output_layer"]["render_mode"] = json!(if layer.features.len() > 5000
        || layer.estimated_bytes() > 4 * 1024 * 1024
    {
        "vector_tiles"
    } else {
        "geojson"
    });
    answer["preview"] = json!({"counts":{"source_features":answer["source_features"],"processed":answer["processed"],"produced":answer["produced"]},"fields_added":fields,"sample":layer.preview(5)["features"],"warnings":warnings});
    if options.dry_run {
        // Truncate only the lists a receipt has; indexing would insert nulls
        // where an outlier report declares a list.
        if let Some(rows) = answer.get_mut("features").and_then(Value::as_array_mut) {
            rows.truncate(5)
        }
        if let Some(report) = answer.get_mut("report") {
            for name in ["regions", "findings"] {
                if let Some(rows) = report.get_mut(name).and_then(Value::as_array_mut) {
                    rows.truncate(5)
                }
            }
        }
    }
    Ok(ToolResult {
        metadata: answer,
        layer: if options.dry_run || t.command == "collisions" {
            None
        } else {
            Some(Arc::new(layer))
        },
    })
}
fn random(
    source: &Layer,
    request: &Value,
    limit: usize,
    dry_run: bool,
) -> Result<(Value, Layer), Refusal> {
    let (entries, mut skipped, more) = planned(source, VectorOperation::Buffer, limit, None)?;
    let p = &request["parameters"];
    let (min, max, buffer, seed) = (
        p["min_spacing_m"].as_f64().unwrap(),
        p["max_spacing_m"].as_f64().unwrap(),
        p["buffer_distance_m"].as_f64().unwrap(),
        p["seed"].as_f64().unwrap() as u64,
    );
    let retained = if dry_run {
        5
    } else if request["output"]["projection"] == "inline" {
        limit
    } else {
        usize::MAX
    };
    let mut layer = Layer::default();
    let (mut produced, mut areas, mut buffered, mut contributed) = (0, 0, 0, 0);
    for entry in &entries {
        let geometry = source.features[entry.index].geometry.as_ref().unwrap();
        let groups: Vec<Coordinates> = match geometry.kind.as_str() {
            "Polygon" => vec![geometry.coordinates.clone()],
            "MultiPolygon" => geometry.coordinates.as_array().unwrap().clone(),
            _ => entry
                .parts
                .iter()
                .map(|p| Coordinates::Array(vec![Coordinates::from_flat(p)]))
                .collect(),
        };
        let mut used = false;
        for group in groups {
            let rings = group
                .as_array()
                .ok_or_else(|| refuse("vector_document_malformed", "Polygon needs rings."))?;
            let mut packed = vec![rings.len() as f64];
            for r in rings {
                packed.push(
                    (r.as_array()
                        .ok_or_else(|| {
                            refuse("vector_document_malformed", "Ring needs coordinates.")
                        })?
                        .len()
                        * 2) as f64,
                )
            }
            for r in rings {
                for c in r.as_array().unwrap() {
                    let pair = c.pair().ok_or_else(|| {
                        refuse(
                            "vector_document_malformed",
                            "Coordinates must be finite numbers.",
                        )
                    })?;
                    packed.extend(pair)
                }
            }
            let generated = ds_geo_lite::random_points_in_area(
                entry.kind.engine_code(),
                &packed,
                buffer,
                min,
                max,
                seed as f64,
            );
            if generated.is_empty() {
                continue;
            }
            used = true;
            areas += 1;
            if entry.kind != GeometryKind::Polygon {
                buffered += 1
            }
            for c in generated.chunks_exact(2) {
                produced += 1;
                if layer.features.len() >= retained {
                    continue;
                }
                let mut f = produced_feature(
                    Geometry::point(c[0], c[1]),
                    json!({"source_feature_id":entry.id.clone().unwrap_or_else(||format!("feature-{}",entry.index+1)),"source_layer":request["source"]["name"],"point_index":produced}),
                );
                f.id = Some(json!(format!("random-point-{produced}")));
                layer.features.push(f)
            }
        }
        if used {
            contributed += 1
        } else {
            // A point or line has no area without a buffer; anything else
            // the engine returned nothing for is named as such.
            let reason = if entry.kind != GeometryKind::Polygon
                && buffer.partial_cmp(&0.) != Some(std::cmp::Ordering::Greater)
            {
                "no_buffer"
            } else {
                "engine_empty"
            };
            skipped.add(reason, entry.index, entry.id.clone())
        }
    }
    let mut answer = json!({"operation":"random-points-area","source_features":source.features.len(),"processed":contributed,"produced":produced,"area_count":areas,"buffered_count":buffered,"skipped":skipped.counts(),"skipped_features":skipped.named(limit),"result":null});
    let mut notes = more.into_iter().collect::<Vec<_>>();
    if request["output"]["projection"] == "inline" && produced > limit {
        notes.push(format!(
            "{} output points withheld by limit {limit}.",
            produced - limit
        ))
    }
    if !notes.is_empty() {
        answer["more"] = json!(notes.join("; "))
    }
    Ok((answer, layer))
}

/// Resource admission before calling the existing geometry primitives.
pub(crate) fn guard_layers(
    id: &str,
    request: &Value,
    source: &Layer,
    against: Option<&Layer>,
) -> Result<(), Refusal> {
    let command = &tool(id)?.command;
    let p = &request["parameters"];
    let bounded = |message: &str| refuse("vector_data_bound_exceeded", message);
    if source.estimated_bytes() + against.map(Layer::estimated_bytes).unwrap_or(0)
        > ds_geo::layer::ipc::MAX_BYTES
    {
        return Err(bounded("Typed inputs exceed 32 MiB; use bounded chunks."));
    }
    if matches!(command.as_str(), "outliers" | "collisions" | "measure") {
        return Ok(());
    }
    let (entries, _, _) = planned(
        source,
        VectorOperation::Measure,
        request["output"]["limit"].as_u64().unwrap() as usize,
        None,
    )?;
    let (mut length, mut area, mut vertices) = (0., 0., 0usize);
    for e in &entries {
        let (l, a) = measured(e);
        length += l;
        area += a;
        vertices += e.written_vertex_count()
    }
    let estimated = match command.as_str() {
        "sample" => length / p["interval_m"].as_f64().unwrap() + 2. * vertices as f64,
        "random-points-area" => {
            let radius = p["buffer_distance_m"].as_f64().unwrap();
            4. * (area
                + 2. * radius * length
                + std::f64::consts::PI * radius * radius * entries.len() as f64)
                / p["min_spacing_m"].as_f64().unwrap().powi(2)
        }
        "intersect" => {
            let other = against
                .ok_or_else(|| refuse("vector_request_invalid", "Supply against IPC/layer."))?;
            let (other, _, _) = planned(
                other,
                VectorOperation::Intersect,
                request["output"]["limit"].as_u64().unwrap() as usize,
                Some("against"),
            )?;
            let segments = |e: &[PlannedFeature]| {
                e.iter()
                    .map(|f| {
                        f.parts
                            .iter()
                            .map(|p| (p.len() / 2).saturating_sub(1))
                            .sum::<usize>()
                    })
                    .sum::<usize>()
            };
            segments(&entries).saturating_mul(segments(&other)) as f64
        }
        "buffer" => {
            if source
                .estimated_bytes()
                .saturating_mul(2 * p["segments"].as_u64().unwrap() as usize + 8)
                > ds_geo::layer::ipc::MAX_BYTES
            {
                return Err(bounded(
                    "Projected buffer data exceeds 32 MiB; use a feature iterator/chunk.",
                ));
            }
            0.
        }
        _ => 0.,
    };
    if !estimated.is_finite() || estimated > ds_geo::layer::ipc::MAX_ROWS as f64 {
        return Err(bounded(
            "Conservative production bound exceeds 20000 features; increase spacing or use smaller source chunks.",
        ));
    }
    Ok(())
}
