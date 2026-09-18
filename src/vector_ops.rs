//! Vector-processing decisions: what a geoprocessing run will do, and why.
//!
//! The engine that bends coordinates is `ds-network`'s `ds-geo-ops`. This
//! module owns none of that arithmetic and deliberately cannot reach it —
//! `docs/contracts/ds-kernel-network-boundary.md` keeps the kernel free of a
//! Cargo edge to `ds-network` so the kernel still compiles on a field device.
//!
//! What the kernel owns is every *decision* a vector operation makes before
//! and after the geometry runs:
//!
//! * how a caller's document is read (a FeatureCollection, a bare Feature, or
//!   a naked geometry all name the same thing);
//! * which features an operation can act on, and the named reason each
//!   skipped feature was skipped;
//! * whether the parameters are in range, as one stable refusal code with a
//!   remedy rather than a panic or an empty answer;
//! * how much of the answer a caller gets by default, and what `more` has to
//!   say when the rest was withheld.
//!
//! A caller therefore learns what will happen before anything happens, and
//! the CLI, the Server and the desktop reach the same verdict because they
//! read it from here.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The default number of output features a single run returns. Larger runs
/// are legitimate; they are opt-in through the caller's own limit so the
/// default projection stays inside an agent's context.
pub const DEFAULT_LIMIT: usize = 500;

/// The hard ceiling on a caller-supplied limit. Past this the answer stops
/// being something to read and becomes a file to write.
pub const MAX_LIMIT: usize = 20_000;

/// The geometry classes vector operations branch on. Multi-part geometries
/// are the same class as their single-part form: a MultiLineString is lines,
/// and an operation that works on lines works on each part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeometryKind {
    Point,
    Line,
    Polygon,
}

impl GeometryKind {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Line => "line",
            Self::Polygon => "polygon",
        }
    }

    /// The engine's geometry selector. `ds-geo-ops` takes the kind as a `u32`
    /// on its flat-array boundary; naming the mapping once here is what stops
    /// each call site from inventing its own.
    pub const fn engine_code(self) -> u32 {
        match self {
            Self::Point => 0,
            Self::Line => 1,
            Self::Polygon => 2,
        }
    }

    fn from_geojson_type(value: &str) -> Option<Self> {
        match value {
            "Point" | "MultiPoint" => Some(Self::Point),
            "LineString" | "MultiLineString" => Some(Self::Line),
            "Polygon" | "MultiPolygon" => Some(Self::Polygon),
            _ => None,
        }
    }
}

/// The operations this family plans. Each one names what it needs, so the
/// eligibility rule is declared next to the operation rather than repeated at
/// every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VectorOperation {
    /// A zone of a fixed distance around each feature.
    Buffer,
    /// The points where the lines of one document cross another's.
    Intersect,
    /// Points placed along each line at a fixed interval.
    Sample,
    /// Length, area and vertex counts per feature.
    Measure,
}

impl VectorOperation {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Buffer => "buffer",
            Self::Intersect => "intersect",
            Self::Sample => "sample",
            Self::Measure => "measure",
        }
    }

    /// Which geometry classes this operation acts on. Anything else is
    /// skipped with a reason, never silently dropped.
    pub const fn accepts(self) -> &'static [GeometryKind] {
        match self {
            Self::Buffer => &[
                GeometryKind::Point,
                GeometryKind::Line,
                GeometryKind::Polygon,
            ],
            Self::Intersect | Self::Sample => &[GeometryKind::Line],
            Self::Measure => &[
                GeometryKind::Point,
                GeometryKind::Line,
                GeometryKind::Polygon,
            ],
        }
    }

    fn accepts_kind(self, kind: GeometryKind) -> bool {
        let mut index = 0;
        let accepted = self.accepts();
        while index < accepted.len() {
            if accepted[index] as u8 == kind as u8 {
                return true;
            }
            index += 1;
        }
        false
    }

    /// What a caller should do when a document holds nothing this operation
    /// can act on. Named per operation because "convert your points to lines"
    /// is useless advice for `measure`.
    fn empty_remedy(self) -> &'static str {
        match self {
            Self::Buffer => "Pass a document holding point, line or polygon features.",
            Self::Intersect | Self::Sample => {
                "This operation reads lines; pass a LineString or MultiLineString document."
            }
            Self::Measure => "Pass a document holding at least one feature with a geometry.",
        }
    }
}

/// A refusal a caller can branch on: a stable code, what happened, and the
/// fix. Carrying the remedy here is what keeps the CLI from writing a second,
/// drifting copy of the advice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VectorRefusal {
    pub code: &'static str,
    pub message: String,
    pub remedy: &'static str,
}

impl VectorRefusal {
    fn new(code: &'static str, message: impl Into<String>, remedy: &'static str) -> Self {
        Self {
            code,
            message: message.into(),
            remedy,
        }
    }
}

/// One feature the operation will act on: where it is in the source document,
/// what it is called, and the flat `[lng, lat, …]` parts the engine consumes.
///
/// The parts are already split: a MultiLineString contributes one part per
/// line, so an engine call is always one simple geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedFeature {
    /// Zero-based position in the source document. A caller correlates the
    /// answer back to their own file with this, not with a name we invented.
    pub index: usize,
    /// The feature's own `id`, or its first string property, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub kind: GeometryKind,
    /// Flat `[lng, lat, lng, lat, …]` per simple part, WGS-84 degrees.
    pub parts: Vec<Vec<f64>>,
}

impl PlannedFeature {
    /// Total coordinate count across every part. Used to price a run before
    /// running it.
    pub fn vertex_count(&self) -> usize {
        self.parts.iter().map(|part| part.len() / 2).sum()
    }
}

/// Why one feature was left out. A named reason, because "3 features were
/// skipped" is a fact a caller cannot act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The geometry is a class this operation does not act on.
    WrongKind,
    /// The feature carries no geometry, or no coordinates.
    NoGeometry,
    /// The geometry is present but holds no positions.
    EmptyGeometry,
    /// A position is not a finite `[lng, lat]` pair.
    BadCoordinates,
}

impl SkipReason {
    pub const fn token(self) -> &'static str {
        match self {
            Self::WrongKind => "wrong_kind",
            Self::NoGeometry => "no_geometry",
            Self::EmptyGeometry => "empty_geometry",
            Self::BadCoordinates => "bad_coordinates",
        }
    }
}

/// A feature the operation will not act on, and the named reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedFeature {
    pub index: usize,
    pub reason: SkipReason,
}

/// The decided shape of one run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorPlan {
    pub operation: VectorOperation,
    /// How many features the source document held in total.
    pub source_features: usize,
    pub planned: Vec<PlannedFeature>,
    pub skipped: Vec<SkippedFeature>,
    /// Set when eligible features were left out of `planned` by the limit.
    /// This is the sentence a bounded answer owes its caller.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more: Option<String>,
}

impl VectorPlan {
    /// How many eligible features the limit withheld.
    pub fn withheld(&self) -> usize {
        self.eligible().saturating_sub(self.planned.len())
    }

    /// Eligible features before the limit was applied.
    pub fn eligible(&self) -> usize {
        self.source_features - self.skipped.len()
    }

    /// The sentence owed for a *second* document the same limit cut.
    ///
    /// `more` speaks for the document an operation produces from. An
    /// operation that compares two documents has another one, and a caller
    /// who is told "500 crossings" while the document being compared against
    /// was quietly cut at 500 has been given a wrong answer, not a bounded
    /// one. Named by the argument it arrived on so the remedy is the flag the
    /// caller typed.
    pub fn withheld_note(&self, argument: &str) -> Option<String> {
        let withheld = self.withheld();
        (withheld > 0).then(|| {
            format!(
                "{withheld} feature(s) from --{argument} were not compared; raise --limit (max {MAX_LIMIT}) to include them"
            )
        })
    }
}

/// Validate a caller's result limit against the family's bounds.
pub fn resolve_limit(requested: Option<usize>) -> Result<usize, VectorRefusal> {
    match requested {
        None => Ok(DEFAULT_LIMIT),
        Some(0) => Err(VectorRefusal::new(
            "vector_limit_out_of_range",
            "A limit of 0 would return nothing.",
            "Pass a limit between 1 and 20000, or omit it for 500.",
        )),
        Some(value) if value > MAX_LIMIT => Err(VectorRefusal::new(
            "vector_limit_out_of_range",
            format!("A limit of {value} is above the {MAX_LIMIT} ceiling."),
            "Pass a limit between 1 and 20000, or omit it for 500.",
        )),
        Some(value) => Ok(value),
    }
}

/// Validate a distance in metres against a stated range.
///
/// Every distance this family takes — a buffer radius, a sampling interval —
/// is decided here so one out-of-range answer reads the same whichever
/// command produced it.
pub fn resolve_distance_m(
    value: f64,
    name: &'static str,
    minimum: f64,
    maximum: f64,
) -> Result<f64, VectorRefusal> {
    if !value.is_finite() {
        return Err(VectorRefusal::new(
            "vector_distance_out_of_range",
            format!("{name} must be a finite number of metres."),
            "Pass a plain number of metres, e.g. 25.",
        ));
    }
    if value < minimum || value > maximum {
        return Err(VectorRefusal::new(
            "vector_distance_out_of_range",
            format!("{name} is {value} m; the range is {minimum} m to {maximum} m."),
            "Pass a distance inside the range this command's help states.",
        ));
    }
    Ok(value)
}

/// Read a GeoJSON document and decide the run.
///
/// `document` may be a FeatureCollection, a single Feature, a GeometryCollection
/// or a naked geometry: all four are ways of writing the same thing, and a
/// caller who has one should not have to learn which one we prefer.
pub fn plan(
    document: &Value,
    operation: VectorOperation,
    limit: usize,
) -> Result<VectorPlan, VectorRefusal> {
    let features = read_features(document)?;
    if features.is_empty() {
        return Err(VectorRefusal::new(
            "vector_document_empty",
            "The document holds no features.",
            "Pass a GeoJSON document with at least one feature.",
        ));
    }

    let source_features = features.len();
    let mut planned = Vec::new();
    let mut skipped = Vec::new();
    let mut eligible = 0usize;

    for (index, feature) in features.iter().enumerate() {
        match classify(index, feature, operation) {
            Ok(entry) => {
                eligible += 1;
                if planned.len() < limit {
                    planned.push(entry);
                }
            }
            Err(reason) => skipped.push(SkippedFeature { index, reason }),
        }
    }

    if eligible == 0 {
        return Err(VectorRefusal::new(
            "vector_no_eligible_feature",
            format!(
                "Not one of the {source_features} features is a geometry class `{}` acts on.",
                operation.token()
            ),
            operation.empty_remedy(),
        ));
    }

    let withheld = eligible - planned.len();
    let more = (withheld > 0).then(|| {
        format!(
            "{withheld} more eligible feature(s) were not returned; raise the limit (max {MAX_LIMIT}) to see them"
        )
    });

    Ok(VectorPlan {
        operation,
        source_features,
        planned,
        skipped,
        more,
    })
}

/// Every feature in a document, whichever of the four shapes it was written in.
fn read_features(document: &Value) -> Result<Vec<Value>, VectorRefusal> {
    let malformed = || {
        VectorRefusal::new(
            "vector_document_malformed",
            "The document is not GeoJSON this reader recognises.",
            "Pass a FeatureCollection, a Feature, or a bare geometry object.",
        )
    };
    let Some(object) = document.as_object() else {
        return Err(malformed());
    };
    match object.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => Ok(object
            .get("features")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()),
        Some("Feature") => Ok(vec![document.clone()]),
        Some("GeometryCollection") => Ok(object
            .get("geometries")
            .and_then(Value::as_array)
            .map(|geometries| {
                geometries
                    .iter()
                    .map(|geometry| serde_json::json!({ "type": "Feature", "geometry": geometry }))
                    .collect()
            })
            .unwrap_or_default()),
        Some(other) if GeometryKind::from_geojson_type(other).is_some() => Ok(vec![
            serde_json::json!({ "type": "Feature", "geometry": document.clone() }),
        ]),
        _ => Err(malformed()),
    }
}

/// Decide one feature: eligible with its parts, or skipped with a reason.
fn classify(
    index: usize,
    feature: &Value,
    operation: VectorOperation,
) -> Result<PlannedFeature, SkipReason> {
    let geometry = feature.get("geometry").unwrap_or(feature);
    let Some(type_name) = geometry.get("type").and_then(Value::as_str) else {
        return Err(SkipReason::NoGeometry);
    };
    let Some(kind) = GeometryKind::from_geojson_type(type_name) else {
        return Err(SkipReason::WrongKind);
    };
    if !operation.accepts_kind(kind) {
        return Err(SkipReason::WrongKind);
    }
    let Some(coordinates) = geometry.get("coordinates") else {
        return Err(SkipReason::NoGeometry);
    };
    let parts = flatten_parts(type_name, coordinates).ok_or(SkipReason::BadCoordinates)?;
    if parts.is_empty() {
        return Err(SkipReason::EmptyGeometry);
    }
    Ok(PlannedFeature {
        index,
        id: feature_id(feature),
        kind,
        parts,
    })
}

/// A feature's own `id`, when it declares one. A name we invent is worse than
/// no name, so a feature without an id simply has none and is addressed by
/// its index.
fn feature_id(feature: &Value) -> Option<String> {
    match feature.get("id") {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => feature
            .get("properties")
            .and_then(|properties| properties.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

/// Split a GeoJSON geometry into flat simple parts. The outer ring alone is
/// taken from a polygon: this family's operations are ring-level, and a hole
/// silently treated as an outer ring would be a wrong answer, not a slow one.
fn flatten_parts(type_name: &str, coordinates: &Value) -> Option<Vec<Vec<f64>>> {
    let parts = match type_name {
        "Point" => vec![position(coordinates)?],
        "LineString" => vec![positions(coordinates)?],
        // Each point of a MultiPoint is its own part. The engine's point
        // entry takes exactly one position and answers nothing for a packet
        // holding several, so a MultiPoint carried as a single part would be
        // planned, counted as processed, and then silently produce nothing.
        "MultiPoint" => coordinates
            .as_array()?
            .iter()
            .map(position)
            .collect::<Option<Vec<_>>>()?,
        "MultiLineString" => coordinates
            .as_array()?
            .iter()
            .map(positions)
            .collect::<Option<Vec<_>>>()?,
        "Polygon" => vec![positions(coordinates.as_array()?.first()?)?],
        "MultiPolygon" => coordinates
            .as_array()?
            .iter()
            .map(|polygon| positions(polygon.as_array()?.first()?))
            .collect::<Option<Vec<_>>>()?,
        _ => return None,
    };
    Some(parts.into_iter().filter(|part| !part.is_empty()).collect())
}

fn position(value: &Value) -> Option<Vec<f64>> {
    let pair = value.as_array()?;
    let lng = pair.first()?.as_f64()?;
    let lat = pair.get(1)?.as_f64()?;
    (lng.is_finite() && lat.is_finite()).then_some(vec![lng, lat])
}

fn positions(value: &Value) -> Option<Vec<f64>> {
    let mut flat = Vec::new();
    for entry in value.as_array()? {
        flat.extend(position(entry)?);
    }
    Some(flat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn collection(features: Vec<Value>) -> Value {
        json!({ "type": "FeatureCollection", "features": features })
    }

    fn line(coordinates: Value) -> Value {
        json!({
            "type": "Feature",
            "geometry": { "type": "LineString", "coordinates": coordinates },
        })
    }

    #[test]
    fn a_bare_geometry_is_the_same_document_as_a_collection() {
        let geometry =
            json!({ "type": "LineString", "coordinates": [[30.0, -2.0], [30.01, -2.0]] });
        let bare = plan(&geometry, VectorOperation::Sample, DEFAULT_LIMIT).expect("bare geometry");
        let wrapped = plan(
            &collection(vec![line(json!([[30.0, -2.0], [30.01, -2.0]]))]),
            VectorOperation::Sample,
            DEFAULT_LIMIT,
        )
        .expect("collection");
        assert_eq!(bare.planned, wrapped.planned);
    }

    #[test]
    fn a_multi_line_contributes_one_part_per_line() {
        let document = collection(vec![json!({
            "type": "Feature",
            "geometry": {
                "type": "MultiLineString",
                "coordinates": [
                    [[30.0, -2.0], [30.01, -2.0]],
                    [[30.0, -2.1], [30.01, -2.1], [30.02, -2.1]],
                ],
            },
        })]);
        let decided = plan(&document, VectorOperation::Sample, DEFAULT_LIMIT).expect("plan");
        assert_eq!(decided.planned.len(), 1);
        assert_eq!(decided.planned[0].parts.len(), 2);
        assert_eq!(decided.planned[0].vertex_count(), 5);
    }

    #[test]
    fn a_polygon_keeps_its_outer_ring_and_drops_its_holes() {
        let document = json!({
            "type": "Polygon",
            "coordinates": [
                [[30.0, -2.0], [30.1, -2.0], [30.1, -2.1], [30.0, -2.1], [30.0, -2.0]],
                [[30.02, -2.02], [30.03, -2.02], [30.03, -2.03], [30.02, -2.02]],
            ],
        });
        let decided = plan(&document, VectorOperation::Buffer, DEFAULT_LIMIT).expect("plan");
        assert_eq!(decided.planned[0].parts.len(), 1);
        assert_eq!(decided.planned[0].vertex_count(), 5);
        assert_eq!(decided.planned[0].kind, GeometryKind::Polygon);
    }

    #[test]
    fn a_point_is_skipped_by_name_when_the_operation_reads_lines() {
        let document = collection(vec![
            json!({ "type": "Feature", "geometry": { "type": "Point", "coordinates": [30.0, -2.0] } }),
            line(json!([[30.0, -2.0], [30.01, -2.0]])),
        ]);
        let decided = plan(&document, VectorOperation::Sample, DEFAULT_LIMIT).expect("plan");
        assert_eq!(decided.planned.len(), 1);
        assert_eq!(
            decided.skipped,
            vec![SkippedFeature {
                index: 0,
                reason: SkipReason::WrongKind
            }]
        );
    }

    #[test]
    fn a_document_with_nothing_eligible_refuses_with_the_operations_own_remedy() {
        let document = collection(vec![
            json!({ "type": "Feature", "geometry": { "type": "Point", "coordinates": [30.0, -2.0] } }),
        ]);
        let refusal = plan(&document, VectorOperation::Sample, DEFAULT_LIMIT).expect_err("refusal");
        assert_eq!(refusal.code, "vector_no_eligible_feature");
        assert!(refusal.remedy.contains("LineString"));
    }

    #[test]
    fn the_limit_bounds_the_answer_and_says_how_much_it_withheld() {
        let features: Vec<Value> = (0..5)
            .map(|index| {
                let offset = f64::from(index) / 100.0;
                line(json!([[30.0 + offset, -2.0], [30.01 + offset, -2.0]]))
            })
            .collect();
        let decided = plan(&collection(features), VectorOperation::Sample, 2).expect("plan");
        assert_eq!(decided.planned.len(), 2);
        assert_eq!(decided.eligible(), 5);
        assert_eq!(decided.withheld(), 3);
        assert!(decided
            .more
            .as_deref()
            .expect("more")
            .starts_with("3 more eligible feature(s)"));
    }

    #[test]
    fn a_full_answer_says_nothing_about_more() {
        let decided = plan(
            &collection(vec![line(json!([[30.0, -2.0], [30.01, -2.0]]))]),
            VectorOperation::Sample,
            DEFAULT_LIMIT,
        )
        .expect("plan");
        assert!(decided.more.is_none());
        assert_eq!(decided.withheld(), 0);
    }

    #[test]
    fn a_non_geojson_document_refuses_before_any_geometry_runs() {
        let refusal = plan(
            &json!({ "rows": [] }),
            VectorOperation::Buffer,
            DEFAULT_LIMIT,
        )
        .expect_err("refusal");
        assert_eq!(refusal.code, "vector_document_malformed");
    }

    #[test]
    fn a_non_finite_coordinate_is_skipped_rather_than_carried_into_the_engine() {
        let document = collection(vec![
            json!({
                "type": "Feature",
                "geometry": { "type": "LineString", "coordinates": [[30.0, -2.0], ["x", -2.0]] },
            }),
            line(json!([[30.0, -2.0], [30.01, -2.0]])),
        ]);
        let decided = plan(&document, VectorOperation::Sample, DEFAULT_LIMIT).expect("plan");
        assert_eq!(
            decided.skipped,
            vec![SkippedFeature {
                index: 0,
                reason: SkipReason::BadCoordinates
            }]
        );
    }

    #[test]
    fn limits_and_distances_refuse_with_one_code_and_a_remedy() {
        assert_eq!(resolve_limit(None).expect("default"), DEFAULT_LIMIT);
        assert_eq!(
            resolve_limit(Some(0)).expect_err("zero").code,
            "vector_limit_out_of_range"
        );
        assert_eq!(
            resolve_limit(Some(MAX_LIMIT + 1)).expect_err("over").code,
            "vector_limit_out_of_range"
        );
        assert_eq!(resolve_limit(Some(7)).expect("inside"), 7);

        assert_eq!(
            resolve_distance_m(f64::NAN, "--radius-m", 0.1, 100.0)
                .expect_err("nan")
                .code,
            "vector_distance_out_of_range"
        );
        let refusal =
            resolve_distance_m(1_000.0, "--radius-m", 0.1, 100.0).expect_err("over range");
        assert!(refusal.message.contains("0.1 m to 100 m"));
    }

    /// Each point of a MultiPoint has to arrive as its own part. The engine's
    /// point entry answers nothing for a packet holding several positions, so
    /// a MultiPoint carried whole was planned, counted as processed, and then
    /// produced nothing at all — the run refused with advice about coordinate
    /// systems for coordinates that were perfectly good.
    #[test]
    fn every_point_of_a_multi_point_is_its_own_part() {
        let document = json!({
            "type": "MultiPoint",
            "coordinates": [[30.0, -2.0], [30.02, -2.0], [30.04, -2.0]],
        });
        let decided = plan(&document, VectorOperation::Buffer, DEFAULT_LIMIT).expect("plan");
        assert_eq!(decided.planned.len(), 1);
        assert_eq!(decided.planned[0].kind, GeometryKind::Point);
        assert_eq!(decided.planned[0].parts.len(), 3);
        assert!(decided.planned[0].parts.iter().all(|part| part.len() == 2));
        assert_eq!(decided.planned[0].vertex_count(), 3);
    }

    /// A comparison reads two documents under one limit, and the second one
    /// is the one a caller cannot see being cut.
    #[test]
    fn a_compared_document_says_how_much_of_it_was_not_compared() {
        let features: Vec<Value> = (0..5)
            .map(|index| {
                let offset = f64::from(index) / 100.0;
                line(json!([[30.0 + offset, -2.0], [30.01 + offset, -2.0]]))
            })
            .collect();
        let decided = plan(&collection(features), VectorOperation::Intersect, 2).expect("plan");
        let note = decided.withheld_note("against").expect("note");
        assert!(note.starts_with("3 feature(s) from --against"), "{note}");
        assert!(note.contains("--limit"), "{note}");

        let whole = plan(
            &collection(vec![line(json!([[30.0, -2.0], [30.01, -2.0]]))]),
            VectorOperation::Intersect,
            DEFAULT_LIMIT,
        )
        .expect("plan");
        assert_eq!(whole.withheld_note("against"), None);
    }

    /// The refusal a caller reads when a document holds the wrong classes has
    /// to be a sentence, not a verb with `ed` glued on: `sample` became
    /// "sampleed" and `measure` became "measureed".
    #[test]
    fn the_nothing_eligible_refusal_reads_as_english_for_every_operation() {
        let points = collection(vec![
            json!({ "type": "Feature", "geometry": { "type": "Point", "coordinates": [30.0, -2.0] } }),
        ]);
        for operation in [VectorOperation::Sample, VectorOperation::Intersect] {
            let refusal = plan(&points, operation, DEFAULT_LIMIT).expect_err("refusal");
            assert_eq!(refusal.code, "vector_no_eligible_feature");
            assert!(
                !refusal.message.contains("eed"),
                "{}: {}",
                operation.token(),
                refusal.message
            );
            assert!(refusal.message.contains(operation.token()));
        }
    }

    #[test]
    fn the_engine_code_for_each_kind_is_stated_once() {
        assert_eq!(GeometryKind::Point.engine_code(), 0);
        assert_eq!(GeometryKind::Line.engine_code(), 1);
        assert_eq!(GeometryKind::Polygon.engine_code(), 2);
    }

    #[test]
    fn a_feature_is_addressed_by_its_own_id_or_by_its_index() {
        let document = collection(vec![
            json!({
                "type": "Feature",
                "id": "pole-7",
                "geometry": { "type": "LineString", "coordinates": [[30.0, -2.0], [30.01, -2.0]] },
            }),
            line(json!([[30.0, -2.1], [30.01, -2.1]])),
        ]);
        let decided = plan(&document, VectorOperation::Sample, DEFAULT_LIMIT).expect("plan");
        assert_eq!(decided.planned[0].id.as_deref(), Some("pole-7"));
        assert_eq!(decided.planned[1].id, None);
        assert_eq!(decided.planned[1].index, 1);
    }
}
