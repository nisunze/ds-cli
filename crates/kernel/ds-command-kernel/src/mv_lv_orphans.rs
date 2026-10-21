//! MV/LV orphan join: the MV line ends that reach no transformer, and the
//! transformers that no MV line reaches.
//!
//! Before an MV route and its LV rooms are delivered together a designer needs
//! two lists: every MV line end that touches neither another MV line nor a
//! transformer, and every transformer farther than a tolerance from every MV
//! line. Both are geometry questions over facts the caller already holds: MV
//! lines as GeoJSON (for example the files `ds dsgrid project geojson`
//! writes) and transformer points, either as GeoJSON points or as the
//! `ds design status` envelope whose rows carry
//! `metadata.spatial.representative_point`.
//!
//! Distances are geodesic metres. Point-to-point distances use the haversine
//! sphere; point-to-line distances use a local equirectangular frame centred
//! on the query point. No projection, country or CRS is assumed, so the same
//! answer holds anywhere the inputs are WGS84 longitude/latitude.
//!
//! The kernel decides everything a caller branches on: which features count,
//! why each skipped one was skipped, which line end is a junction, which tip
//! and which transformer is an orphan. Hosts read the files and write the
//! optional GeoJSON of orphans.

use ds_geo_lite::{haversine_m, EARTH_RADIUS_M};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const SCHEMA: &str = "ds.mv-lv-orphans/v1";
/// Line ends closer than this to another MV line are one network junction.
pub const DEFAULT_JUNCTION_M: f64 = 1.0;
/// The largest tolerance or junction distance a caller may ask for.
pub const MAX_DISTANCE_M: f64 = 100_000.0;
pub const DEFAULT_LIMIT: usize = 50;
pub const MAX_LIMIT: usize = 5_000;
/// Skipped features are counted by reason; this many are named.
const SKIPPED_SAMPLES: usize = 20;

/// A refusal a caller can branch on: a stable code, what happened, the fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: Code,
    pub message: String,
    pub remedy: &'static str,
}

/// The closed refusal vocabulary. A host maps it exhaustively, so a code the
/// kernel grows cannot reach a caller undeclared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    DistanceOutOfRange,
    LimitOutOfRange,
    MvMalformed,
    MvEmpty,
    TransformersMalformed,
    TransformersEmpty,
}

impl Code {
    pub const ALL: [Code; 6] = [
        Code::DistanceOutOfRange,
        Code::LimitOutOfRange,
        Code::MvMalformed,
        Code::MvEmpty,
        Code::TransformersMalformed,
        Code::TransformersEmpty,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Code::DistanceOutOfRange => "orphans_distance_out_of_range",
            Code::LimitOutOfRange => "orphans_limit_out_of_range",
            Code::MvMalformed => "orphans_mv_malformed",
            Code::MvEmpty => "orphans_mv_empty",
            Code::TransformersMalformed => "orphans_transformers_malformed",
            Code::TransformersEmpty => "orphans_transformers_empty",
        }
    }
}

fn refuse(code: Code, message: impl Into<String>, remedy: &'static str) -> Refusal {
    Refusal {
        code,
        message: message.into(),
        remedy,
    }
}

/// One MV document and the label its tips and verdicts carry (a file name).
pub struct MvDocument<'a> {
    pub source: &'a str,
    pub document: &'a Value,
}

pub struct Request<'a> {
    pub mv: &'a [MvDocument<'a>],
    /// GeoJSON points, or a `ds design status` envelope (or its `data`).
    pub transformers: &'a Value,
    /// A tip or transformer farther than this from its partner is an orphan.
    pub tolerance_m: f64,
    pub junction_m: f64,
    /// Transformers whose names start with one of these are located (tips
    /// may still end at them) but never judged as orphans themselves.
    pub exclude_prefixes: &'a [String],
}

/// Validate a caller's limit once, for every host.
pub fn limit(raw: Option<usize>) -> Result<usize, Refusal> {
    match raw {
        None => Ok(DEFAULT_LIMIT),
        Some(n) if (1..=MAX_LIMIT).contains(&n) => Ok(n),
        Some(n) => Err(refuse(
            Code::LimitOutOfRange,
            format!("limit {n} is outside 1..={MAX_LIMIT}"),
            "Pass a limit between 1 and 5000, or omit it for 50.",
        )),
    }
}

type Position = [f64; 2];

struct Line {
    source: String,
    alignment: String,
    points: Vec<Position>,
    /// west, south, east, north in degrees.
    bbox: [f64; 4],
}

struct Located {
    name: String,
    position: Position,
    location: String,
}

struct Tip {
    line: usize,
    end: &'static str,
    position: Position,
    nearest: usize,
    distance_m: f64,
}

struct Judged {
    transformer: usize,
    nearest_line: usize,
    distance_to_mv_m: f64,
    nearest_tip: Option<(usize, f64)>,
}

#[derive(Default)]
struct Skipped {
    by_reason: BTreeMap<&'static str, usize>,
    sample: Vec<Value>,
}

impl Skipped {
    fn push(&mut self, input: &str, index: usize, reason: &'static str) {
        *self.by_reason.entry(reason).or_default() += 1;
        if self.sample.len() < SKIPPED_SAMPLES {
            self.sample
                .push(json!({"input": input, "index": index, "reason": reason}));
        }
    }
    fn total(&self) -> usize {
        self.by_reason.values().sum()
    }
}

/// The complete join. Its receipt is bounded; its GeoJSON holds every orphan.
pub struct Outcome {
    tolerance_m: f64,
    junction_m: f64,
    documents: usize,
    lines: Vec<Line>,
    transformers: Vec<Located>,
    excluded: usize,
    line_ends: usize,
    tips: Vec<Tip>,
    judged: Vec<Judged>,
    skipped: Skipped,
}

fn distance_valid(value: f64, allow_zero: bool) -> bool {
    value.is_finite() && value <= MAX_DISTANCE_M && (value > 0.0 || (allow_zero && value == 0.0))
}

pub fn join(request: &Request<'_>) -> Result<Outcome, Refusal> {
    if !distance_valid(request.tolerance_m, false) || !distance_valid(request.junction_m, true) {
        return Err(refuse(
            Code::DistanceOutOfRange,
            format!(
                "tolerance {} m and junction {} m must be finite; tolerance above 0, junction at least 0, both at most {MAX_DISTANCE_M} m",
                request.tolerance_m, request.junction_m
            ),
            "Pass --tolerance-m above 0 and --junction-m of 0 or more, both at most 100000.",
        ));
    }
    let mut skipped = Skipped::default();
    let mut lines = Vec::new();
    for document in request.mv {
        let features = features(document.document).ok_or_else(|| {
            refuse(
                Code::MvMalformed,
                format!(
                    "{} is not a GeoJSON FeatureCollection or Feature",
                    document.source
                ),
                "Pass MV lines as GeoJSON, for example from `ds dsgrid project geojson`.",
            )
        })?;
        for (index, feature) in features.into_iter().enumerate() {
            read_line(document.source, index, feature, &mut lines, &mut skipped);
        }
    }
    if lines.is_empty() {
        return Err(refuse(
            Code::MvEmpty,
            "no MV document holds a usable LineString or MultiLineString",
            "Pass the MV alignments as GeoJSON lines with WGS84 coordinates.",
        ));
    }
    let (transformers, excluded) =
        read_transformers(request.transformers, request.exclude_prefixes, &mut skipped)?;
    if transformers.is_empty() {
        return Err(refuse(
            Code::TransformersEmpty,
            "no transformer could be located",
            "Pass named GeoJSON points, or `ds design status --output json` for a project whose rows carry metadata.spatial.representative_point.",
        ));
    }
    let mut tips = Vec::new();
    let mut line_ends = 0;
    for (index, line) in lines.iter().enumerate() {
        let first = line.points[0];
        let last = *line.points.last().expect("a line has two points");
        let closed = line.points.len() > 2 && point_point_m(first, last) <= request.junction_m;
        for (end, position) in [("start", first), ("end", last)] {
            line_ends += 1;
            if closed || touches_other_line(&lines, index, position, request.junction_m) {
                continue;
            }
            let (nearest, distance_m) = nearest_transformer(&transformers, position);
            tips.push(Tip {
                line: index,
                end,
                position,
                nearest,
                distance_m,
            });
        }
    }
    let judged = transformers
        .iter()
        .enumerate()
        .filter(|(_, t)| !excluded_name(&t.name, request.exclude_prefixes))
        .map(|(index, transformer)| {
            let (nearest_line, distance_to_mv_m) = nearest_line(&lines, transformer.position);
            let nearest_tip = tips
                .iter()
                .enumerate()
                .map(|(i, tip)| (i, point_point_m(transformer.position, tip.position)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            Judged {
                transformer: index,
                nearest_line,
                distance_to_mv_m,
                nearest_tip,
            }
        })
        .collect();
    Ok(Outcome {
        tolerance_m: request.tolerance_m,
        junction_m: request.junction_m,
        documents: request.mv.len(),
        lines,
        transformers,
        excluded,
        line_ends,
        tips,
        judged,
        skipped,
    })
}

fn excluded_name(name: &str, prefixes: &[String]) -> bool {
    prefixes
        .iter()
        .any(|prefix| !prefix.is_empty() && name.starts_with(prefix.as_str()))
}

fn features(document: &Value) -> Option<Vec<&Value>> {
    match document.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => document
            .get("features")?
            .as_array()
            .map(|features| features.iter().collect()),
        Some("Feature") => Some(vec![document]),
        _ => None,
    }
}

fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn position(value: &Value) -> Option<Position> {
    let pair = value.as_array()?;
    let lon = pair.first()?.as_f64()?;
    let lat = pair.get(1)?.as_f64()?;
    (lon.is_finite() && lat.is_finite() && lon.abs() <= 180.0 && lat.abs() <= 90.0)
        .then_some([lon, lat])
}

fn read_line(
    source: &str,
    index: usize,
    feature: &Value,
    lines: &mut Vec<Line>,
    skipped: &mut Skipped,
) {
    let geometry = &feature["geometry"];
    let parts: Vec<&Value> = match geometry.get("type").and_then(Value::as_str) {
        Some("LineString") => vec![&geometry["coordinates"]],
        Some("MultiLineString") => geometry["coordinates"]
            .as_array()
            .map(|parts| parts.iter().collect())
            .unwrap_or_default(),
        _ => return skipped.push(source, index, "not_a_line"),
    };
    let properties = &feature["properties"];
    let alignment = ["alignment_id", "alignment", "name"]
        .iter()
        .find_map(|key| text(&properties[*key]))
        .or_else(|| text(&feature["id"]))
        .or_else(|| text(&properties["entity_id"]))
        .unwrap_or_else(|| format!("feature-{index}"));
    let mut usable = 0;
    for part in parts {
        let Some(raw) = part.as_array() else {
            continue;
        };
        let mut points: Vec<Position> = Vec::with_capacity(raw.len());
        for value in raw {
            let Some(point) = position(value) else {
                return skipped.push(source, index, "invalid_coordinates");
            };
            if points.last() != Some(&point) {
                points.push(point);
            }
        }
        if points.len() < 2 {
            continue;
        }
        let bbox = points.iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |b, p| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            },
        );
        lines.push(Line {
            source: source.to_owned(),
            alignment: alignment.clone(),
            points,
            bbox,
        });
        usable += 1;
    }
    if usable == 0 {
        skipped.push(source, index, "degenerate_line");
    }
}

fn read_transformers(
    document: &Value,
    exclude_prefixes: &[String],
    skipped: &mut Skipped,
) -> Result<(Vec<Located>, usize), Refusal> {
    const INPUT: &str = "transformers";
    let rows = document
        .pointer("/data/transformers")
        .or_else(|| document.get("transformers"))
        .and_then(Value::as_array);
    let mut located = Vec::new();
    if let Some(rows) = rows {
        for (index, row) in rows.iter().enumerate() {
            let Some(name) = text(&row["name"]) else {
                skipped.push(INPUT, index, "unnamed");
                continue;
            };
            if row
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind != "individual")
            {
                skipped.push(INPUT, index, "not_a_transformer");
                continue;
            }
            let spatial = &row["metadata"]["spatial"];
            let Some(point) = position(&spatial["representative_point"]) else {
                skipped.push(INPUT, index, "no_location");
                continue;
            };
            let location = text(&spatial["source_layer"])
                .map(|layer| format!("layer:{layer}"))
                .unwrap_or_else(|| "extent_centre".to_owned());
            located.push(Located {
                name,
                position: point,
                location,
            });
        }
    } else {
        let features = features(document).ok_or_else(|| {
            refuse(
                Code::TransformersMalformed,
                "the transformer input is neither GeoJSON points nor a `ds design status` envelope",
                "Pass named GeoJSON points or the JSON `ds design status --output json` writes.",
            )
        })?;
        for (index, feature) in features.into_iter().enumerate() {
            let geometry = &feature["geometry"];
            if geometry.get("type").and_then(Value::as_str) != Some("Point") {
                skipped.push(INPUT, index, "not_a_point");
                continue;
            }
            let Some(point) = position(&geometry["coordinates"]) else {
                skipped.push(INPUT, index, "invalid_coordinates");
                continue;
            };
            let properties = &feature["properties"];
            let Some(name) = ["name", "transformer", "transformer_name", "tr_name"]
                .iter()
                .find_map(|key| text(&properties[*key]))
                .or_else(|| text(&feature["id"]))
            else {
                skipped.push(INPUT, index, "unnamed");
                continue;
            };
            located.push(Located {
                name,
                position: point,
                location: "point".to_owned(),
            });
        }
    }
    let excluded = located
        .iter()
        .filter(|t| excluded_name(&t.name, exclude_prefixes))
        .count();
    Ok((located, excluded))
}

fn point_point_m(a: Position, b: Position) -> f64 {
    haversine_m(a[0], a[1], b[0], b[1])
}

/// `q` in metres east/north of `origin`, in the local equirectangular frame.
fn local(origin: Position, q: Position) -> Position {
    let k = EARTH_RADIUS_M * std::f64::consts::PI / 180.0;
    let mut dlon = q[0] - origin[0];
    if dlon > 180.0 {
        dlon -= 360.0;
    } else if dlon < -180.0 {
        dlon += 360.0;
    }
    [
        dlon * k * origin[1].to_radians().cos(),
        (q[1] - origin[1]) * k,
    ]
}

fn point_segment_m(p: Position, a: Position, b: Position) -> f64 {
    let a = local(p, a);
    let b = local(p, b);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length2 = dx * dx + dy * dy;
    let t = if length2 == 0.0 {
        0.0
    } else {
        (-(a[0] * dx + a[1] * dy) / length2).clamp(0.0, 1.0)
    };
    (a[0] + t * dx).hypot(a[1] + t * dy)
}

/// A lower bound on the distance from `p` to anything inside `bbox`.
fn bbox_floor_m(p: Position, bbox: [f64; 4]) -> f64 {
    let nearest = [p[0].clamp(bbox[0], bbox[2]), p[1].clamp(bbox[1], bbox[3])];
    let offset = local(p, nearest);
    offset[0].hypot(offset[1])
}

fn point_line_m(p: Position, line: &Line, ceiling: f64) -> f64 {
    if bbox_floor_m(p, line.bbox) > ceiling {
        return f64::INFINITY;
    }
    line.points
        .windows(2)
        .map(|w| point_segment_m(p, w[0], w[1]))
        .fold(f64::INFINITY, f64::min)
}

fn touches_other_line(lines: &[Line], index: usize, p: Position, junction_m: f64) -> bool {
    lines
        .iter()
        .enumerate()
        .any(|(other, line)| other != index && point_line_m(p, line, junction_m) <= junction_m)
}

fn nearest_line(lines: &[Line], p: Position) -> (usize, f64) {
    let mut best = (0, f64::INFINITY);
    for (index, line) in lines.iter().enumerate() {
        let d = point_line_m(p, line, best.1);
        if d < best.1 {
            best = (index, d);
        }
    }
    best
}

fn nearest_transformer(transformers: &[Located], p: Position) -> (usize, f64) {
    transformers
        .iter()
        .enumerate()
        .map(|(index, t)| (index, point_point_m(p, t.position)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("at least one located transformer")
}

fn metres(value: f64) -> Value {
    json!((value * 10.0).round() / 10.0)
}

fn coordinate(p: Position) -> Value {
    json!([(p[0] * 1e7).round() / 1e7, (p[1] * 1e7).round() / 1e7])
}

impl Outcome {
    fn orphan_tips(&self) -> Vec<&Tip> {
        let mut tips: Vec<&Tip> = self
            .tips
            .iter()
            .filter(|tip| tip.distance_m > self.tolerance_m)
            .collect();
        tips.sort_by(|a, b| {
            b.distance_m
                .total_cmp(&a.distance_m)
                .then_with(|| {
                    self.lines[a.line]
                        .alignment
                        .cmp(&self.lines[b.line].alignment)
                })
                .then_with(|| a.end.cmp(b.end))
        });
        tips
    }

    fn orphan_transformers(&self) -> Vec<&Judged> {
        let mut judged: Vec<&Judged> = self
            .judged
            .iter()
            .filter(|j| j.distance_to_mv_m > self.tolerance_m)
            .collect();
        judged.sort_by(|a, b| {
            b.distance_to_mv_m
                .total_cmp(&a.distance_to_mv_m)
                .then_with(|| {
                    self.transformers[a.transformer]
                        .name
                        .cmp(&self.transformers[b.transformer].name)
                })
        });
        judged
    }

    /// Alignments with at least one tip, none of which ends at a transformer.
    fn lonely_alignments(&self) -> Vec<Value> {
        let mut groups: BTreeMap<(&str, &str), Vec<&Tip>> = BTreeMap::new();
        for tip in &self.tips {
            let line = &self.lines[tip.line];
            groups
                .entry((line.source.as_str(), line.alignment.as_str()))
                .or_default()
                .push(tip);
        }
        groups
            .into_iter()
            .filter(|(_, tips)| tips.iter().all(|tip| tip.distance_m > self.tolerance_m))
            .map(|((source, alignment), tips)| {
                json!({
                    "source": source,
                    "alignment": alignment,
                    "tips": tips.iter().map(|tip| json!({
                        "end": tip.end,
                        "nearest_transformer": self.transformers[tip.nearest].name,
                        "distance_m": metres(tip.distance_m),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect()
    }

    fn tip_json(&self, tip: &Tip) -> Map<String, Value> {
        let line = &self.lines[tip.line];
        let mut out = Map::new();
        out.insert("source".into(), json!(line.source));
        out.insert("alignment".into(), json!(line.alignment));
        out.insert("end".into(), json!(tip.end));
        out.insert(
            "nearest_transformer".into(),
            json!(self.transformers[tip.nearest].name),
        );
        out.insert("distance_m".into(), metres(tip.distance_m));
        out
    }

    fn transformer_json(&self, judged: &Judged) -> Map<String, Value> {
        let transformer = &self.transformers[judged.transformer];
        let line = &self.lines[judged.nearest_line];
        let mut out = Map::new();
        out.insert("name".into(), json!(transformer.name));
        out.insert("location".into(), json!(transformer.location));
        out.insert("nearest_mv_source".into(), json!(line.source));
        out.insert("nearest_mv_alignment".into(), json!(line.alignment));
        out.insert("distance_to_mv_m".into(), metres(judged.distance_to_mv_m));
        if let Some((tip, distance)) = judged.nearest_tip {
            let tip = &self.tips[tip];
            out.insert(
                "nearest_tip".into(),
                json!({"alignment": self.lines[tip.line].alignment, "end": tip.end}),
            );
            out.insert("distance_to_tip_m".into(), metres(distance));
        }
        out
    }

    /// The bounded answer: counts are complete, each list holds at most `limit`.
    pub fn receipt(&self, limit: usize) -> Value {
        let tips = self.orphan_tips();
        let transformers = self.orphan_transformers();
        let alignments = self.lonely_alignments();
        let listed = |n: usize| n.min(limit);
        let omitted = json!({
            "orphan_tips": tips.len() - listed(tips.len()),
            "orphan_transformers": transformers.len() - listed(transformers.len()),
            "alignments_ending_at_no_transformer": alignments.len() - listed(alignments.len()),
        });
        let more = [tips.len(), transformers.len(), alignments.len()]
            .iter()
            .any(|n| *n > limit);
        json!({
            "schema": SCHEMA,
            "tolerance_m": self.tolerance_m,
            "junction_m": self.junction_m,
            "counts": {
                "mv_documents": self.documents,
                "mv_lines": self.lines.len(),
                "line_ends": self.line_ends,
                "junctions": self.line_ends - self.tips.len(),
                "tips": self.tips.len(),
                "orphan_tips": tips.len(),
                "transformers_located": self.transformers.len(),
                "transformers_excluded": self.excluded,
                "transformers_judged": self.judged.len(),
                "orphan_transformers": transformers.len(),
                "alignments_ending_at_no_transformer": alignments.len(),
                "skipped": self.skipped.total(),
            },
            "orphan_tips": tips.iter().take(limit).map(|tip| Value::Object(self.tip_json(tip))).collect::<Vec<_>>(),
            "orphan_transformers": transformers.iter().take(limit).map(|j| Value::Object(self.transformer_json(j))).collect::<Vec<_>>(),
            "alignments_ending_at_no_transformer": alignments.into_iter().take(limit).collect::<Vec<_>>(),
            "skipped": {"by_reason": self.skipped.by_reason, "sample": self.skipped.sample},
            "more": if more { omitted } else { Value::Null },
        })
    }

    /// Every orphan as a WGS84 point, ready to register as a local layer.
    pub fn orphan_features(&self) -> Value {
        let tips = self.orphan_tips().into_iter().map(|tip| {
            let mut properties = self.tip_json(tip);
            properties.insert("orphan_kind".into(), json!("mv_tip"));
            json!({"type": "Feature", "geometry": {"type": "Point", "coordinates": coordinate(tip.position)}, "properties": properties})
        });
        let transformers = self.orphan_transformers().into_iter().map(|judged| {
            let mut properties = self.transformer_json(judged);
            properties.insert("orphan_kind".into(), json!("transformer"));
            let position = self.transformers[judged.transformer].position;
            json!({"type": "Feature", "geometry": {"type": "Point", "coordinates": coordinate(position)}, "properties": properties})
        });
        json!({"type": "FeatureCollection", "features": tips.chain(transformers).collect::<Vec<_>>()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// About 111 m of longitude at the equator per 0.001 degree.
    fn line(id: &str, coordinates: &[[f64; 2]]) -> Value {
        json!({"type": "Feature", "properties": {"alignment_id": id},
               "geometry": {"type": "LineString", "coordinates": coordinates}})
    }

    fn points(named: &[(&str, [f64; 2])]) -> Value {
        json!({"type": "FeatureCollection", "features": named.iter().map(|(name, p)| json!({
            "type": "Feature", "properties": {"name": name},
            "geometry": {"type": "Point", "coordinates": p}})).collect::<Vec<_>>()})
    }

    fn run(mv: &Value, transformers: &Value, tolerance_m: f64, exclude: &[String]) -> Value {
        let documents = [MvDocument {
            source: "model.geojson",
            document: mv,
        }];
        join(&Request {
            mv: &documents,
            transformers,
            tolerance_m,
            junction_m: DEFAULT_JUNCTION_M,
            exclude_prefixes: exclude,
        })
        .expect("join")
        .receipt(DEFAULT_LIMIT)
    }

    fn backbone_with_tap() -> Value {
        // A backbone along the equator and a T-off starting on it at 0.002.
        json!({"type": "FeatureCollection", "features": [
            line("backbone", &[[0.0, 0.0], [0.002, 0.0], [0.004, 0.0]]),
            line("t-off", &[[0.002, 0.0], [0.002, 0.003]]),
        ]})
    }

    #[test]
    fn a_tap_start_is_a_junction_and_far_ends_are_orphan_tips() {
        let mv = backbone_with_tap();
        // A transformer at the backbone's east end and one at the T-off end.
        let trs = points(&[("east", [0.004, 0.0001]), ("north", [0.002, 0.0031])]);
        let receipt = run(&mv, &trs, 30.0, &[]);
        let counts = &receipt["counts"];
        assert_eq!(counts["line_ends"], 4, "{receipt}");
        assert_eq!(counts["junctions"], 1, "{receipt}");
        assert_eq!(counts["tips"], 3, "{receipt}");
        // Only the backbone's west end (the source side) reaches nothing.
        assert_eq!(counts["orphan_tips"], 1, "{receipt}");
        let tip = &receipt["orphan_tips"][0];
        assert_eq!(tip["alignment"], "backbone");
        assert_eq!(tip["end"], "start");
        assert_eq!(tip["nearest_transformer"], "north");
        assert_eq!(counts["orphan_transformers"], 0, "{receipt}");
        assert_eq!(
            counts["alignments_ending_at_no_transformer"], 0,
            "{receipt}"
        );
        assert!(receipt["more"].is_null());
    }

    #[test]
    fn a_transformer_beyond_the_tolerance_from_every_line_is_an_orphan() {
        let mv = backbone_with_tap();
        // 0.001 degree of latitude is about 111 m from the backbone.
        let trs = points(&[
            ("east", [0.004, 0.0001]),
            ("north", [0.002, 0.0031]),
            ("stray", [0.001, -0.001]),
        ]);
        let receipt = run(&mv, &trs, 30.0, &[]);
        assert_eq!(receipt["counts"]["orphan_transformers"], 1, "{receipt}");
        let stray = &receipt["orphan_transformers"][0];
        assert_eq!(stray["name"], "stray");
        assert_eq!(stray["nearest_mv_alignment"], "backbone");
        let distance = stray["distance_to_mv_m"].as_f64().unwrap();
        assert!((distance - 111.2).abs() < 0.5, "{distance}");
        // A tolerance above that distance admits it.
        let receipt = run(&mv, &trs, 150.0, &[]);
        assert_eq!(receipt["counts"]["orphan_transformers"], 0, "{receipt}");
    }

    #[test]
    fn an_excluded_prefix_is_located_but_never_judged() {
        let mv = backbone_with_tap();
        let trs = points(&[
            ("east", [0.004, 0.0001]),
            ("fill_in_far", [0.01, 0.01]),
            ("west", [0.0, 0.0001]),
        ]);
        let receipt = run(&mv, &trs, 30.0, &["fill_in_".to_owned()]);
        let counts = &receipt["counts"];
        assert_eq!(counts["transformers_located"], 3, "{receipt}");
        assert_eq!(counts["transformers_excluded"], 1, "{receipt}");
        assert_eq!(counts["transformers_judged"], 2, "{receipt}");
        assert_eq!(counts["orphan_transformers"], 0, "{receipt}");
        // The T-off's north end reaches no transformer, so its alignment is
        // the one ending at none.
        assert_eq!(
            counts["alignments_ending_at_no_transformer"], 1,
            "{receipt}"
        );
        assert_eq!(
            receipt["alignments_ending_at_no_transformer"][0]["alignment"],
            "t-off"
        );
    }

    #[test]
    fn design_status_rows_locate_individual_transformers_only() {
        let mv = backbone_with_tap();
        let status = json!({"v": 1, "data": {"transformers": [
            {"name": "east", "kind": "individual", "metadata": {"spatial": {"representative_point": [0.004, 0.0], "source_layer": "tr"}}},
            {"name": "centre", "kind": "individual", "metadata": {"spatial": {"representative_point": [0.002, 0.003], "source_layer": ""}}},
            {"name": "project_level", "kind": "project_level", "metadata": {"spatial": {"representative_point": [0.0, 0.0]}}},
            {"name": "unplaced", "kind": "individual", "metadata": {}},
        ]}});
        let receipt = run(&mv, &status, 30.0, &[]);
        let counts = &receipt["counts"];
        assert_eq!(counts["transformers_located"], 2, "{receipt}");
        assert_eq!(counts["skipped"], 2, "{receipt}");
        assert_eq!(receipt["skipped"]["by_reason"]["not_a_transformer"], 1);
        assert_eq!(receipt["skipped"]["by_reason"]["no_location"], 1);
        assert_eq!(counts["orphan_tips"], 1, "{receipt}");
    }

    #[test]
    fn every_dropped_feature_is_named_and_points_are_not_lines() {
        let mv = json!({"type": "FeatureCollection", "features": [
            line("ok", &[[0.0, 0.0], [0.001, 0.0]]),
            {"type": "Feature", "properties": {}, "geometry": {"type": "Point", "coordinates": [0.0, 0.0]}},
            line("short", &[[0.0, 0.0], [0.0, 0.0]]),
            line("bad", &[[0.0, 0.0], [500.0, 0.0]]),
        ]});
        let trs = points(&[("a", [0.0, 0.0])]);
        let receipt = run(&mv, &trs, 30.0, &[]);
        let reasons = &receipt["skipped"]["by_reason"];
        assert_eq!(reasons["not_a_line"], 1, "{receipt}");
        assert_eq!(reasons["degenerate_line"], 1, "{receipt}");
        assert_eq!(reasons["invalid_coordinates"], 1, "{receipt}");
        assert_eq!(receipt["counts"]["mv_lines"], 1, "{receipt}");
    }

    #[test]
    fn refusals_are_stable_codes() {
        let mv = backbone_with_tap();
        let trs = points(&[("a", [0.0, 0.0])]);
        let documents = [MvDocument {
            source: "m",
            document: &mv,
        }];
        let request = |tolerance_m, transformers: &Value| {
            join(&Request {
                mv: &documents,
                transformers,
                tolerance_m,
                junction_m: DEFAULT_JUNCTION_M,
                exclude_prefixes: &[],
            })
            .err()
            .map(|r| r.code)
        };
        assert_eq!(request(0.0, &trs), Some(Code::DistanceOutOfRange));
        assert_eq!(request(f64::NAN, &trs), Some(Code::DistanceOutOfRange));
        assert_eq!(
            request(30.0, &json!({"rows": []})),
            Some(Code::TransformersMalformed)
        );
        assert_eq!(request(30.0, &points(&[])), Some(Code::TransformersEmpty));
        let empty = json!({"type": "FeatureCollection", "features": []});
        let documents = [MvDocument {
            source: "m",
            document: &empty,
        }];
        let refused = join(&Request {
            mv: &documents,
            transformers: &trs,
            tolerance_m: 30.0,
            junction_m: DEFAULT_JUNCTION_M,
            exclude_prefixes: &[],
        })
        .err()
        .map(|r| r.code);
        assert_eq!(refused, Some(Code::MvEmpty));
        assert_eq!(
            limit(Some(0)).err().map(|r| r.code),
            Some(Code::LimitOutOfRange)
        );
        assert_eq!(limit(None), Ok(DEFAULT_LIMIT));
    }

    #[test]
    fn the_receipt_is_bounded_and_the_geojson_holds_every_orphan() {
        let mv = json!({"type": "FeatureCollection", "features": [
            line("a", &[[0.0, 0.0], [0.001, 0.0]]),
            line("b", &[[0.0, 0.01], [0.001, 0.01]]),
        ]});
        let trs = points(&[("far", [1.0, 1.0])]);
        let documents = [MvDocument {
            source: "m",
            document: &mv,
        }];
        let outcome = join(&Request {
            mv: &documents,
            transformers: &trs,
            tolerance_m: 30.0,
            junction_m: DEFAULT_JUNCTION_M,
            exclude_prefixes: &[],
        })
        .unwrap();
        let receipt = outcome.receipt(1);
        assert_eq!(receipt["counts"]["orphan_tips"], 4, "{receipt}");
        assert_eq!(receipt["orphan_tips"].as_array().unwrap().len(), 1);
        assert_eq!(receipt["more"]["orphan_tips"], 3, "{receipt}");
        let features = outcome.orphan_features();
        let kinds: Vec<_> = features["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["properties"]["orphan_kind"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(kinds.iter().filter(|k| *k == "mv_tip").count(), 4);
        assert_eq!(kinds.iter().filter(|k| *k == "transformer").count(), 1);
    }
}
