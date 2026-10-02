//! `ds data vector …` — geographic vector processing, headless.
//!
//! The typed `ds-command-kernel::vector_ops` controller owns the complete
//! processing packet shared with WASM: admission, algorithms and result
//! projection. This surface resolves declared file/JSON-text inputs, parses
//! CLI scalar spelling and writes requested local output without rebuilding
//! the kernel's geometry, properties or result envelope.
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
use ds_command_kernel::vector_ops::{self, ResultProjection, VectorRefusal, VectorRequest};
use serde_json::{Value, json};

// ── Shared declarations ────────────────────────────────────────────────

const SOURCE: Arg = Arg {
    name: "source",
    kind: ds_cli_contract::spec::ArgKind::Value,
    value: "<path>",
    required: false,
    default: None,
    choices: &[],
    summary: "Local GeoJSON path; supply exactly one of source or source-json.",
};

const SOURCE_JSON: Arg = Arg::value(
    "source-json",
    "<json-text>",
    "Inline GeoJSON text, not an MCP object; alternative to source.",
);

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
pub const INPUT_CHOICE_INVALID: Refusal = Refusal {
    code: "vector_input_choice_invalid",
    when: "A document has both file and JSON-text inputs, or neither.",
    remedy: "Supply exactly one of source/source-json, and one of against/against-json for intersect.",
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
    let json_arg = format!("{arg}-json");
    let parsed = match (inputs.value(arg), inputs.value(&json_arg)) {
        (Some(path), None) => serde_json::from_slice(&crate::read_source(path)?),
        (None, Some(text)) => serde_json::from_str(text),
        _ => {
            return Err(Failure::invalid(
                INPUT_CHOICE_INVALID.code,
                format!("Supply exactly one of --{arg} or --{json_arg}."),
            )
            .remedy(format!(
                "Use --{arg} for a local GeoJSON path or --{json_arg} for GeoJSON text."
            )));
        }
    };
    parsed.map_err(|error| {
        Failure::invalid(
            DOCUMENT_MALFORMED.code,
            format!("Could not parse --{arg} document: {error}"),
        )
        .remedy(DOCUMENT_MALFORMED.remedy)
    })
}

fn requested_limit(inputs: &Inputs) -> Result<Option<usize>, Failure> {
    Ok(match inputs.value("limit") {
        None => None,
        Some(raw) => Some(raw.parse::<usize>().map_err(|_| {
            Failure::invalid(
                "vector_limit_out_of_range",
                format!("--limit `{raw}` is not a whole number."),
            )
            .remedy("Pass a limit between 1 and 20000, or omit it for 500.")
        })?),
    })
}

fn requested_distance(inputs: &Inputs, name: &'static str) -> Result<f64, Failure> {
    let raw = inputs.value(name).ok_or_else(|| {
        Failure::invalid(
            "vector_distance_out_of_range",
            format!("--{name} is required."),
        )
        .remedy("Pass a distance inside the range this command's help states.")
    })?;
    raw.parse::<f64>().map_err(|_| {
        Failure::invalid(
            "vector_distance_out_of_range",
            format!("--{name} `{raw}` is not a number of metres."),
        )
        .remedy("Pass a plain number of metres, e.g. 25.")
    })
}

/// The host chooses a file or inline projection; the kernel owns the bound.
fn result_projection(inputs: &Inputs) -> ResultProjection {
    if inputs.value("out").is_some() {
        ResultProjection::CompleteProduced
    } else {
        ResultProjection::Inline
    }
}

/// Execute one typed controller packet, then perform only the declared file IO.
/// The returned GeoJSON, all its properties and the engineering envelope stay
/// the kernel's; delivery changes only `written_to` and the inline `result`.
fn produce(inputs: &Inputs, request: VectorRequest) -> Result<Value, Failure> {
    let mut answer = vector_ops::execute(&request).map_err(refuse)?;
    let Some(path) = inputs.value("out") else {
        return Ok(answer);
    };
    if std::path::Path::new(path).exists() && !inputs.switch("overwrite") {
        return Err(
            Failure::invalid("output_refused", format!("{path} already exists."))
                .remedy("Choose another --out path, or pass --overwrite to replace it."),
        );
    }
    let bytes = serde_json::to_vec(&answer["result"]).map_err(|error| {
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
    answer["written_to"] = json!(path);
    answer["result"] = Value::Null;
    Ok(answer)
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
    contract: 2,
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
        SOURCE_JSON,
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
        INPUT_CHOICE_INVALID,
        NO_ELIGIBLE_FEATURE,
        DISTANCE_OUT_OF_RANGE,
        LIMIT_OUT_OF_RANGE,
        ENGINE_PRODUCED_NOTHING,
        crate::OUTPUT_REFUSED,
    ],
    reference: Some("docs/reference/data.md"),
    search: &[
        "polygons",
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
    let limit = requested_limit(inputs)?;
    let radius_m = requested_distance(inputs, "radius-m")?;
    let segments = inputs
        .value("segments")
        .unwrap_or("8")
        .parse::<u32>()
        .map_err(|_| {
            Failure::invalid(
                "vector_distance_out_of_range",
                "--segments must be a whole number from 1 to 64.",
            )
            .remedy("Pass a whole number of arc segments from 1 to 64, or omit it for 8.")
        })?;
    produce(
        inputs,
        VectorRequest::Buffer {
            schema: vector_ops::SCHEMA.into(),
            source: read_document(inputs, "source")?,
            radius_m,
            segments,
            limit,
            result_projection: result_projection(inputs),
        },
    )
}

pub fn render_buffer(data: &Value) -> String {
    render_produced(data, "zone")
}

// ── data vector sample ─────────────────────────────────────────────────

pub static SAMPLE_COMMAND: Command = Command {
    id: "data.vector.sample",
    path: &["data", "vector", "sample"],
    contract: 2,
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
        SOURCE_JSON,
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
        INPUT_CHOICE_INVALID,
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
        "sampling",
    ],
    requires: Requires::Server,
    availability: crate::available,
};

pub fn run_sample(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = requested_limit(inputs)?;
    let interval_m = requested_distance(inputs, "interval-m")?;
    produce(
        inputs,
        VectorRequest::Sample {
            schema: vector_ops::SCHEMA.into(),
            source: read_document(inputs, "source")?,
            interval_m,
            include_ends: inputs.switch("include-ends"),
            limit,
            result_projection: result_projection(inputs),
        },
    )
}

pub fn render_sample(data: &Value) -> String {
    render_produced(data, "point")
}

// ── data vector intersect ──────────────────────────────────────────────

pub static INTERSECT_COMMAND: Command = Command {
    id: "data.vector.intersect",
    path: &["data", "vector", "intersect"],
    contract: 2,
    summary: "Find the points where two line documents cross.",
    purpose: "\
Compares every line in --source against every line in --against and returns a \
point for each crossing, naming the two features that produced it. This is \
the line-crossing question — where does this feeder cross that road, river \
or feeder — answered from local files or inline GeoJSON text, with no project \
or window. Returns crossing points, not polygon clipping or overlap geometry.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        SOURCE,
        SOURCE_JSON,
        Arg::value(
            "against",
            "<path>",
            "Second line document; alternative to against-json.",
        ),
        Arg::value(
            "against-json",
            "<json-text>",
            "Second line document as JSON text, not an MCP object.",
        ),
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
        INPUT_CHOICE_INVALID,
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
        "topology",
    ],
    requires: Requires::Server,
    availability: crate::available,
};

pub fn run_intersect(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = requested_limit(inputs)?;
    produce(
        inputs,
        VectorRequest::Intersect {
            schema: vector_ops::SCHEMA.into(),
            source: read_document(inputs, "source")?,
            against: read_document(inputs, "against")?,
            limit,
            result_projection: result_projection(inputs),
        },
    )
}

pub fn render_intersect(data: &Value) -> String {
    render_produced(data, "crossing")
}

// ── data vector measure ────────────────────────────────────────────────

pub static MEASURE_COMMAND: Command = Command {
    id: "data.vector.measure",
    path: &["data", "vector", "measure"],
    contract: 2,
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
    args: &[SOURCE, SOURCE_JSON, LIMIT],
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
        INPUT_CHOICE_INVALID,
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

pub fn run_measure(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = requested_limit(inputs)?;
    vector_ops::execute(&VectorRequest::Measure {
        schema: vector_ops::SCHEMA.into(),
        source: read_document(inputs, "source")?,
        limit,
    })
    .map_err(refuse)
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
