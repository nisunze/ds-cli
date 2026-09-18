//! `ds data vector …` — geographic vector processing, headless.
//!
//! The engine is `ds-geo-ops` in `ds-network`: the same Rust that the map's
//! tool dock calls through WASM. The decisions — what a document holds, which
//! features an operation can act on, what is refused and how much comes back
//! — are `ds-command-kernel`'s `vector_ops`. This module is the surface over
//! both, and it is deliberately the third thing rather than a new home for
//! either.
//!
//! Why these live under `data` and not a `geo` domain of their own: root help
//! is the most expensive text in the product and costs one line per DOMAIN
//! forever (`crates/ds/tests/context_budget.rs`). These commands read a local
//! file and write a local file with no project, no window and no network,
//! which is exactly what `ds data` already is. What made them unfindable was
//! never the domain — it was that they did not exist and that search matched
//! substrings. Both are fixed here; the domain stays one line.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::vector_ops::{
    self, GeometryKind, PlannedFeature, VectorOperation, VectorPlan, VectorRefusal,
};
use serde_json::{Value, json};

// ── Shared declarations ────────────────────────────────────────────────

const SOURCE: Arg = Arg {
    name: "source",
    kind: ds_cli_contract::spec::ArgKind::Value,
    value: "<path>",
    required: true,
    default: None,
    choices: &[],
    summary: "Path to a GeoJSON document: a FeatureCollection, a Feature, or a bare geometry.",
};

const OUT: Arg = Arg::value(
    "out",
    "<path>",
    "Write the result here as GeoJSON. Omitted, the result comes back inline.",
);

const OVERWRITE: Arg = Arg::switch("overwrite", "Replace --out if it already exists.");

const LIMIT: Arg = Arg::value(
    "limit",
    "<1..20000>",
    "Cap the features read and the features returned inline; `more` states what that withheld.",
);

/// Refusals the whole family shares. Each one is the kernel's verdict, so the
/// code a caller branches on is the same one the Server and the desktop give.
pub const DOCUMENT_MALFORMED: Refusal = Refusal {
    code: "vector_document_malformed",
    when: "The --source file is not GeoJSON this reader recognises.",
    remedy: "Pass a FeatureCollection, a Feature, or a bare geometry object.",
};
pub const DOCUMENT_EMPTY: Refusal = Refusal {
    code: "vector_document_empty",
    when: "The document parsed but holds no features.",
    remedy: "Pass a GeoJSON document with at least one feature.",
};
pub const NO_ELIGIBLE_FEATURE: Refusal = Refusal {
    code: "vector_no_eligible_feature",
    when: "No feature in the document is a geometry class this operation acts on.",
    remedy: "Run `ds data vector measure` to see what geometry classes the document holds.",
};
pub const LIMIT_OUT_OF_RANGE: Refusal = Refusal {
    code: "vector_limit_out_of_range",
    when: "--limit is 0, above 20000, or not a number.",
    remedy: "Pass a limit between 1 and 20000, or omit it for 500.",
};
pub const DISTANCE_OUT_OF_RANGE: Refusal = Refusal {
    code: "vector_distance_out_of_range",
    when: "A distance argument is outside the range this command's help states.",
    remedy: "Pass a distance inside the range this command's help states.",
};
/// Only `buffer` declares this: every eligible geometry has a buffer, so
/// nothing coming back is a defect. An overlay that finds no crossing and a
/// stationing that fits no point are answers, and they are returned as `note`
/// on a successful run rather than dressed up as failures.
pub const ENGINE_PRODUCED_NOTHING: Refusal = Refusal {
    code: "vector_engine_empty",
    when: "The buffer engine returned no ring for any eligible feature.",
    remedy: "Check the coordinates are WGS-84 lon/lat degrees and the radius suits their scale.",
};

// ── Shared plumbing ────────────────────────────────────────────────────

/// Turn the kernel's verdict into this surface's refusal.
///
/// The kernel decides and the kernel words the message; what this does is
/// name the decision with the `Refusal` this crate declares for it. The match
/// is the point: it is a written-down claim that the kernel's codes and the
/// codes these commands advertise are the same closed set, so a code the
/// kernel grows and no command declares cannot reach a caller undocumented —
/// it lands in the fallback arm and says so. `crates/ds/tests/refusal_coverage.rs`
/// reads this file for exactly that set and cannot read across the crate edge.
fn refuse(refusal: VectorRefusal) -> Failure {
    let remedy = refusal.remedy;
    let message = refusal.message;
    match refusal.code {
        "vector_document_malformed" => Failure::invalid(DOCUMENT_MALFORMED.code, message),
        "vector_document_empty" => Failure::invalid(DOCUMENT_EMPTY.code, message),
        "vector_no_eligible_feature" => Failure::invalid(NO_ELIGIBLE_FEATURE.code, message),
        "vector_limit_out_of_range" => Failure::invalid(LIMIT_OUT_OF_RANGE.code, message),
        "vector_distance_out_of_range" => Failure::invalid(DISTANCE_OUT_OF_RANGE.code, message),
        "vector_engine_empty" => Failure::invalid(ENGINE_PRODUCED_NOTHING.code, message),
        undeclared => {
            return Failure::internal(
                "vector_refusal_undeclared",
                format!(
                    "The vector kernel refused with `{undeclared}`, which no \
                     `ds data vector` command declares."
                ),
            )
            .remedy("Report this: the surface and its kernel disagree about what can be refused.");
        }
    }
    .remedy(remedy)
}

fn read_document(inputs: &Inputs, arg: &str) -> Result<Value, Failure> {
    let path = inputs.value(arg).ok_or_else(|| {
        Failure::invalid("source_unreadable", format!("--{arg} is required."))
            .remedy("Pass the path to a local GeoJSON file.")
    })?;
    let bytes = crate::read_source(path)?;
    serde_json::from_slice(&bytes).map_err(|error| {
        Failure::invalid(
            "vector_document_malformed",
            format!("Could not parse {path}: {error}"),
        )
        .remedy("Pass a FeatureCollection, a Feature, or a bare geometry object.")
    })
}

fn resolve_limit(inputs: &Inputs) -> Result<usize, Failure> {
    let requested = match inputs.value("limit") {
        None => None,
        Some(raw) => Some(raw.parse::<usize>().map_err(|_| {
            Failure::invalid(
                "vector_limit_out_of_range",
                format!("--limit `{raw}` is not a whole number."),
            )
            .remedy("Pass a limit between 1 and 20000, or omit it for 500.")
        })?),
    };
    vector_ops::resolve_limit(requested).map_err(refuse)
}

fn resolve_distance(
    inputs: &Inputs,
    name: &'static str,
    minimum: f64,
    maximum: f64,
) -> Result<f64, Failure> {
    let raw = inputs.value(name).ok_or_else(|| {
        Failure::invalid(
            "vector_distance_out_of_range",
            format!("--{name} is required."),
        )
        .remedy("Pass a distance inside the range this command's help states.")
    })?;
    let value = raw.parse::<f64>().map_err(|_| {
        Failure::invalid(
            "vector_distance_out_of_range",
            format!("--{name} `{raw}` is not a number of metres."),
        )
        .remedy("Pass a plain number of metres, e.g. 25.")
    })?;
    vector_ops::resolve_distance_m(value, name, minimum, maximum).map_err(refuse)
}

fn plan(
    inputs: &Inputs,
    arg: &str,
    operation: VectorOperation,
    limit: usize,
) -> Result<VectorPlan, Failure> {
    let document = read_document(inputs, arg)?;
    vector_ops::plan(&document, operation, limit).map_err(refuse)
}

/// The skipped features, grouped by the kernel's named reason. A caller wants
/// "4 wrong_kind", not four identical rows.
fn skipped_summary(decided: &VectorPlan) -> Value {
    let mut counts: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    for entry in &decided.skipped {
        *counts.entry(entry.reason.token()).or_default() += 1;
    }
    json!(counts)
}

fn ring_to_positions(flat: &[f64]) -> Vec<Value> {
    flat.chunks_exact(2)
        .map(|pair| json!([pair[0], pair[1]]))
        .collect()
}

/// Write the produced features where the caller asked, or hand them back.
///
/// One place decides this so every command in the family answers `--out` the
/// same way, including the refusal when the path is already taken.
fn deliver(inputs: &Inputs, features: Vec<Value>) -> Result<Value, Failure> {
    let collection = json!({ "type": "FeatureCollection", "features": features });
    let Some(path) = inputs.value("out") else {
        return Ok(json!({ "written_to": Value::Null, "result": collection }));
    };
    if std::path::Path::new(path).exists() && !inputs.switch("overwrite") {
        return Err(
            Failure::invalid("output_refused", format!("{path} already exists."))
                .remedy("Choose another --out path, or pass --overwrite to replace it."),
        );
    }
    let bytes = serde_json::to_vec(&collection).map_err(|error| {
        Failure::invalid(
            "output_refused",
            format!("Could not encode the result: {error}"),
        )
        .remedy("Choose another --out path, or pass --overwrite to replace it.")
    })?;
    std::fs::write(path, bytes).map_err(|error| {
        Failure::invalid("output_refused", format!("Could not write {path}: {error}"))
            .remedy("Choose another --out path, or pass --overwrite to replace it.")
    })?;
    Ok(json!({ "written_to": path, "result": Value::Null }))
}

/// The envelope every producing command returns: what it read, what it made,
/// what it skipped and what it withheld.
///
/// Zero produced features is an *answer* here, not a failure: "these two
/// networks do not cross" and "no station fits inside this interval" are
/// exactly what a caller asked for. Only `buffer`, where every eligible
/// geometry must yield a ring, treats nothing as a defect — and it says so
/// before it writes anything.
fn produced(decided: &VectorPlan, produced: usize, delivery: Value, extra: Value) -> Value {
    let mut envelope = json!({
        "operation": decided.operation.token(),
        "source_features": decided.source_features,
        "processed": decided.planned.len(),
        "produced": produced,
        "skipped": skipped_summary(decided),
        "written_to": delivery["written_to"],
        "result": delivery["result"],
    });
    if let Some(more) = &decided.more {
        envelope["more"] = json!(more);
    }
    if let Some(fields) = extra.as_object() {
        for (key, value) in fields {
            envelope[key.as_str()] = value.clone();
        }
    }
    envelope
}

/// Bound what the run *produced*, but only when the answer comes back inline.
///
/// `--limit` bounds the features an operation reads; what it makes is another
/// count entirely. Stationing one 1 km line at 0.5 m makes 2,222 points out of
/// a single eligible feature, and before this the whole FeatureCollection came
/// back inline — 377 kB at a caller who asked one small question, with `more`
/// silent, because nothing had been withheld from the *input*.
///
/// A caller who passed `--out` asked for a file and gets every feature: a file
/// is not a context cost. The kernel owns both the bound and the sentence.
fn bound_inline(
    inputs: &Inputs,
    features: &mut Vec<Value>,
    limit: usize,
    noun: &str,
) -> Option<String> {
    if inputs.value("out").is_some() {
        return None;
    }
    vector_ops::bound_inline(features, limit, noun)
}

/// State a bound a caller could not otherwise see.
///
/// `more` is the one channel for "this answer was cut", so a second cut
/// document joins the sentence already there rather than opening a second
/// field nobody reads.
fn also_withheld(envelope: &mut Value, note: Option<String>) {
    let Some(note) = note else { return };
    let merged = match envelope["more"].as_str() {
        Some(existing) => format!("{existing}; {note}"),
        None => note,
    };
    envelope["more"] = json!(merged);
}

/// Say, in the caller's own terms, why a run that worked produced nothing.
fn nothing_found(envelope: &mut Value, note: &str) {
    if envelope["produced"] == json!(0) {
        envelope["note"] = json!(note);
    }
}

fn render_produced(data: &Value, noun: &str) -> String {
    let mut out = format!(
        "{} {noun}(s) from {} of {} feature(s)\n",
        data["produced"], data["processed"], data["source_features"],
    );
    if let Some(skipped) = data["skipped"].as_object().filter(|map| !map.is_empty()) {
        let reasons: Vec<String> = skipped
            .iter()
            .map(|(reason, count)| format!("{count} {reason}"))
            .collect();
        out.push_str(&format!("  skipped  {}\n", reasons.join(", ")));
    }
    match data["written_to"].as_str() {
        Some(path) => out.push_str(&format!("  written  {path}\n")),
        None => out.push_str("  inline   pass --out <path> to write a file\n"),
    }
    if let Some(note) = data["note"].as_str() {
        out.push_str(&format!("  note     {note}\n"));
    }
    if let Some(more) = data["more"].as_str() {
        out.push_str(&format!("  more     {more}\n"));
    }
    out
}

// ── data vector buffer ─────────────────────────────────────────────────

pub static BUFFER_COMMAND: Command = Command {
    id: "data.vector.buffer",
    path: &["data", "vector", "buffer"],
    contract: 1,
    summary: "Buffer each feature by a fixed distance into a polygon zone.",
    purpose: "\
Grows a zone of --radius-m metres around every point, line and polygon in a \
GeoJSON document and returns it as polygons. Runs the same geodesic buffer \
the map's tools run, on this machine, with no project and no window. Each \
output keeps its source feature's properties and gains `buffer_radius_m`, so \
a corridor, a setback or a service area stays traceable to what produced it.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        SOURCE,
        Arg::value("radius-m", "<0.01..100000>", "Buffer distance in metres.").required(),
        Arg::value(
            "segments",
            "<1..64>",
            "Arc segments per quarter turn; a circle has four times this many vertices.",
        )
        .default("8"),
        OUT,
        OVERWRITE,
        LIMIT,
    ],
    output: "\
`produced`, `processed`, `source_features`, `skipped` counted by reason, and \
either `written_to` or an inline `result` FeatureCollection of polygons. \
`more` states what the limit withheld: eligible features not read, and zones \
made but not returned inline. `--out` writes every one of them.",
    examples: &[Example {
        command: "ds data vector buffer --source ./poles.geojson --radius-m 30 --out ./zone.geojson",
        note: "A 30 m zone around every pole, written as GeoJSON.",
        runnable: false,
    }],
    refusals: &[
        crate::UNREADABLE,
        DOCUMENT_MALFORMED,
        DOCUMENT_EMPTY,
        NO_ELIGIBLE_FEATURE,
        DISTANCE_OUT_OF_RANGE,
        LIMIT_OUT_OF_RANGE,
        ENGINE_PRODUCED_NOTHING,
        crate::OUTPUT_REFUSED,
    ],
    reference: Some("docs/reference/data.md"),
    search: &[
        "geoprocessing",
        "gis",
        "geometry",
        "spatial",
        "corridor",
        "setback",
        "proximity",
        "st_buffer",
        "dilate",
    ],
    requires: Requires::Server,
    availability: crate::available,
};

pub fn run_buffer(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = resolve_limit(inputs)?;
    let radius = resolve_distance(inputs, "radius-m", 0.01, 100_000.0)?;
    let segments: u32 = inputs
        .value("segments")
        .unwrap_or("8")
        .parse()
        .ok()
        .filter(|value| (1..=64).contains(value))
        .ok_or_else(|| {
            Failure::invalid(
                "vector_distance_out_of_range",
                "--segments must be a whole number from 1 to 64.",
            )
            .remedy("Pass a whole number of arc segments from 1 to 64, or omit it for 8.")
        })?;

    let decided = plan(inputs, "source", VectorOperation::Buffer, limit)?;
    let mut features = Vec::new();
    for entry in &decided.planned {
        for part in &entry.parts {
            let ring =
                ds_geo_ops::buffer_geometry(entry.kind.engine_code(), part, radius, segments);
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
        // Before `deliver`, deliberately: a refused run that has already
        // written an empty FeatureCollection leaves the caller a wrong answer
        // on disk and an --out path their retry then refuses as taken.
        return Err(Failure::invalid(
            "vector_engine_empty",
            "The buffer engine returned no ring for any eligible feature.",
        )
        .remedy(
            "Check the coordinates are WGS-84 lon/lat degrees and the radius suits their scale.",
        ));
    }
    let count = features.len();
    let withheld = bound_inline(inputs, &mut features, limit, "zone");
    let delivery = deliver(inputs, features)?;
    let mut envelope = produced(
        &decided,
        count,
        delivery,
        json!({ "radius_m": radius, "segments": segments }),
    );
    also_withheld(&mut envelope, withheld);
    Ok(envelope)
}

pub fn render_buffer(data: &Value) -> String {
    render_produced(data, "zone")
}

// ── data vector sample ─────────────────────────────────────────────────

pub static SAMPLE_COMMAND: Command = Command {
    id: "data.vector.sample",
    path: &["data", "vector", "sample"],
    contract: 1,
    summary: "Place points along each line at a fixed interval.",
    purpose: "\
Walks every line in a GeoJSON document and drops a point every --interval-m \
metres, carrying each point's cumulative distance from the start of its line. \
This is the headless twin of the map's points-along tool: same engine, same \
answer, no window. Points and polygons in the document are skipped by name \
rather than silently dropped.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        SOURCE,
        Arg::value("interval-m", "<0.01..1000000>", "Spacing in metres.").required(),
        Arg::switch("include-ends", "Also place a point at each line end."),
        OUT,
        OVERWRITE,
        LIMIT,
    ],
    output: "\
`produced`, `processed`, `source_features`, `skipped` counted by reason, and \
either `written_to` or an inline `result` FeatureCollection of points, each \
carrying `distance_m` along its source line. `more` states what was withheld, \
including points made but not returned inline — `--out` writes every one of \
them; `note` says why a run that worked placed no point.",
    examples: &[Example {
        command: "ds data vector sample --source ./route.geojson --interval-m 25 --output json",
        note: "A pole position every 25 m along a route.",
        runnable: false,
    }],
    refusals: &[
        crate::UNREADABLE,
        DOCUMENT_MALFORMED,
        DOCUMENT_EMPTY,
        NO_ELIGIBLE_FEATURE,
        DISTANCE_OUT_OF_RANGE,
        LIMIT_OUT_OF_RANGE,
        crate::OUTPUT_REFUSED,
    ],
    reference: Some("docs/reference/data.md"),
    search: &[
        "geoprocessing",
        "gis",
        "geometry",
        "spatial",
        "points along",
        "chainage",
        "stationing",
        "interpolate",
        "densify",
        "sampling",
    ],
    requires: Requires::Server,
    availability: crate::available,
};

pub fn run_sample(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = resolve_limit(inputs)?;
    let interval = resolve_distance(inputs, "interval-m", 0.01, 1_000_000.0)?;
    let include_ends = inputs.switch("include-ends");

    let decided = plan(inputs, "source", VectorOperation::Sample, limit)?;
    let mut features = Vec::new();
    for entry in &decided.planned {
        for (part_index, part) in entry.parts.iter().enumerate() {
            let stationed = ds_geo_ops::points_along_line(part, interval, include_ends);
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

    let count = features.len();
    let withheld = bound_inline(inputs, &mut features, limit, "point");
    let delivery = deliver(inputs, features)?;
    let mut envelope = produced(
        &decided,
        count,
        delivery,
        json!({ "interval_m": interval, "include_ends": include_ends }),
    );
    also_withheld(&mut envelope, withheld);
    nothing_found(
        &mut envelope,
        "No station fits: every line is shorter than --interval-m. Pass a \
         smaller interval, or --include-ends to place a point at each line end.",
    );
    Ok(envelope)
}

pub fn render_sample(data: &Value) -> String {
    render_produced(data, "point")
}

// ── data vector intersect ──────────────────────────────────────────────

pub static INTERSECT_COMMAND: Command = Command {
    id: "data.vector.intersect",
    path: &["data", "vector", "intersect"],
    contract: 1,
    summary: "Find the points where two line documents cross.",
    purpose: "\
Compares every line in --source against every line in --against and returns a \
point for each crossing, naming the two features that produced it. This is \
the overlay question a network engineer asks — where does this feeder cross \
that road, that river, that other feeder — answered from two local files with \
no project and no window.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        SOURCE,
        Arg::value("against", "<path>", "The second GeoJSON document of lines.").required(),
        OUT,
        OVERWRITE,
        LIMIT,
    ],
    output: "\
`produced`, `processed`, `source_features`, `against_features`, `skipped` \
counted by reason, and either `written_to` or an inline `result` \
FeatureCollection of crossing points. `more` states what was withheld from \
either document and what was found but not returned inline — `--out` writes \
every crossing; `note` says so when nothing crosses, which is an answer.",
    examples: &[Example {
        command: "ds data vector intersect --source ./mv.geojson --against ./roads.geojson",
        note: "Every road crossing on an MV network.",
        runnable: false,
    }],
    refusals: &[
        crate::UNREADABLE,
        DOCUMENT_MALFORMED,
        DOCUMENT_EMPTY,
        NO_ELIGIBLE_FEATURE,
        LIMIT_OUT_OF_RANGE,
        crate::OUTPUT_REFUSED,
    ],
    reference: Some("docs/reference/data.md"),
    search: &[
        "geoprocessing",
        "gis",
        "geometry",
        "spatial",
        "overlay",
        "crossing",
        "intersection",
        "st_intersection",
        "topology",
        "clip",
    ],
    requires: Requires::Server,
    availability: crate::available,
};

pub fn run_intersect(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = resolve_limit(inputs)?;
    let source = plan(inputs, "source", VectorOperation::Intersect, limit)?;
    let against = plan(inputs, "against", VectorOperation::Intersect, limit)?;

    let mut features = Vec::new();
    for entry in &source.planned {
        for part in &entry.parts {
            for other in &against.planned {
                for other_part in &other.parts {
                    let hits = ds_geo_ops::find_intersections(part, other_part);
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

    let count = features.len();
    let withheld = bound_inline(inputs, &mut features, limit, "crossing");
    let delivery = deliver(inputs, features)?;
    let mut envelope = produced(
        &source,
        count,
        delivery,
        json!({ "against_features": against.source_features }),
    );
    also_withheld(&mut envelope, withheld);
    also_withheld(&mut envelope, against.withheld_note("against"));
    nothing_found(
        &mut envelope,
        "No crossing: no line in --source meets a line in --against.",
    );
    Ok(envelope)
}

pub fn render_intersect(data: &Value) -> String {
    render_produced(data, "crossing")
}

// ── data vector measure ────────────────────────────────────────────────

pub static MEASURE_COMMAND: Command = Command {
    id: "data.vector.measure",
    path: &["data", "vector", "measure"],
    contract: 1,
    summary: "Length, area and vertex counts for every feature in a document.",
    purpose: "\
Reports what a GeoJSON document actually contains: each feature's geometry \
class, vertex count, geodesic length in metres for a line and spherical area \
in square metres for a polygon, plus the totals. Read this first when a \
document came from somewhere else — it names the geometry classes the rest of \
this family will accept or skip, so an unexpected refusal never has to be \
guessed at.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[SOURCE, LIMIT],
    output: "\
`totals` (features, by geometry class, length_m, area_m2, vertices) counted \
over the WHOLE document, and a `features` array — bounded by --limit, which \
`more` then says so — carrying each feature's index, id, kind, parts, holes, \
vertices, length_m and area_m2. A polygon's area_m2 is its outer ring less \
its holes.",
    examples: &[Example {
        command: "ds data vector measure --source ./network.geojson --output json",
        note: "Total line length and what geometry classes the file holds.",
        runnable: false,
    }],
    refusals: &[
        crate::UNREADABLE,
        DOCUMENT_MALFORMED,
        DOCUMENT_EMPTY,
        NO_ELIGIBLE_FEATURE,
        LIMIT_OUT_OF_RANGE,
    ],
    reference: Some("docs/reference/data.md"),
    search: &[
        "geoprocessing",
        "gis",
        "geometry",
        "spatial",
        "perimeter",
        "distance",
        "st_length",
        "st_area",
        "statistics",
    ],
    requires: Requires::Server,
    availability: crate::available,
};

/// Geodesic length of one flat part, through the engine's single haversine.
///
/// `+ 0.0` is not decoration: Rust sums an empty `f64` iterator to `-0.0`, and
/// a one-vertex line then answered `"length_m": -0.0`, which reads to anyone
/// outside this code like a bug in the measurement.
fn part_length_m(part: &[f64]) -> f64 {
    let total: f64 = part
        .chunks_exact(2)
        .zip(part.chunks_exact(2).skip(1))
        .map(|(from, to)| ds_geo_ops::haversine_m(from[0], from[1], to[0], to[1]))
        .sum();
    total + 0.0
}

/// One feature's length and area.
///
/// A polygon's area is its outer ring *less its holes*: a parcel with a
/// courtyard cut out of it is smaller than its outline, and answering the
/// outline is a wrong number rather than a bounded one. Its length stays the
/// outer ring's perimeter, which is what every GIS means by the word.
fn measured(entry: &PlannedFeature) -> (f64, f64) {
    let perimeter: f64 = entry.parts.iter().map(|part| part_length_m(part)).sum();
    match entry.kind {
        GeometryKind::Point => (0.0, 0.0),
        GeometryKind::Line => (perimeter, 0.0),
        GeometryKind::Polygon => {
            let covered: f64 = entry
                .parts
                .iter()
                .map(|part| ds_geo_ops::spherical_polygon_area_m2(part))
                .sum();
            let holes: f64 = entry
                .interior
                .iter()
                .map(|ring| ds_geo_ops::spherical_polygon_area_m2(ring))
                .sum();
            (perimeter, (covered - holes).max(0.0))
        }
    }
}

pub fn run_measure(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = resolve_limit(inputs)?;
    // Every eligible feature, not the first `limit` of them: a length that
    // silently totals 500 of a 600-feature network is a shorter network than
    // the caller owns, and nothing in the answer says which it is. The limit
    // bounds the ROWS below, where a caller can see exactly what it did.
    let document = read_document(inputs, "source")?;
    let decided = vector_ops::plan_over_whole_document(&document, VectorOperation::Measure)
        .map_err(refuse)?;

    let mut by_kind: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
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

    let measured_features = decided.planned.len();
    let withheld = vector_ops::bound_listed_rows(&mut features, limit, "feature");
    let mut envelope = json!({
        "operation": decided.operation.token(),
        "source_features": decided.source_features,
        "skipped": skipped_summary(&decided),
        "totals": {
            "features": measured_features,
            "by_kind": by_kind,
            "vertices": vertices,
            "length_m": length,
            "area_m2": area,
        },
        "features": features,
    });
    also_withheld(&mut envelope, withheld);
    Ok(envelope)
}

pub fn render_measure(data: &Value) -> String {
    let totals = &data["totals"];
    let mut out = format!(
        "{} feature(s)  {} vertices\n  length  {:.1} m\n  area    {:.1} m2\n",
        totals["features"],
        totals["vertices"],
        totals["length_m"].as_f64().unwrap_or(0.0),
        totals["area_m2"].as_f64().unwrap_or(0.0),
    );
    if let Some(kinds) = totals["by_kind"].as_object().filter(|map| !map.is_empty()) {
        let parts: Vec<String> = kinds
            .iter()
            .map(|(kind, count)| format!("{count} {kind}"))
            .collect();
        out.push_str(&format!("  kinds   {}\n", parts.join(", ")));
    }
    if let Some(skipped) = data["skipped"].as_object().filter(|map| !map.is_empty()) {
        let reasons: Vec<String> = skipped
            .iter()
            .map(|(reason, count)| format!("{count} {reason}"))
            .collect();
        out.push_str(&format!("  skipped {}\n", reasons.join(", ")));
    }
    if let Some(more) = data["more"].as_str() {
        out.push_str(&format!("  more    {more}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The words an outsider reaches for. Every command in the family must
    /// declare all of them: a family is only findable as a family if the
    /// shared vocabulary is on each member, not on whichever one was written
    /// first.
    const FAMILY_TERMS: &[&str] = &["geoprocessing", "gis", "geometry", "spatial"];

    /// The family's shared vocabulary has to actually be on every command, or
    /// an outsider's search finds three of the four.
    #[test]
    fn every_command_declares_the_familys_shared_vocabulary() {
        for command in [
            &BUFFER_COMMAND,
            &SAMPLE_COMMAND,
            &INTERSECT_COMMAND,
            &MEASURE_COMMAND,
        ] {
            for term in FAMILY_TERMS {
                assert!(
                    command.search.contains(term),
                    "`{}` does not declare the search term `{term}`",
                    command.id
                );
            }
        }
    }

    /// This family exists to be reachable without a window. If one of them
    /// ever declares otherwise, the whole point of the slice is gone.
    #[test]
    fn nothing_in_this_family_needs_a_window() {
        for command in [
            &BUFFER_COMMAND,
            &SAMPLE_COMMAND,
            &INTERSECT_COMMAND,
            &MEASURE_COMMAND,
        ] {
            assert_eq!(command.requires, Requires::Server, "{}", command.id);
        }
    }
}
