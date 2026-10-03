//! `ds data vector …` — geographic vector processing, headless.
//!
//! The `ds-network::vector` descriptor and runner own the complete
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
use ds_network::vector::{self, Refusal as VectorRefusal, RunOptions};
use serde_json::{Value, json};

// ── Shared declarations ────────────────────────────────────────────────

const SOURCE: Arg = Arg {
    name: "source",
    kind: ds_cli_contract::spec::ArgKind::Value,
    value: "<path>",
    required: false,
    default: None,
    choices: &[],
    summary: "Local GeoJSON, KML or Arrow path; choose source or source-json.",
};

const SOURCE_JSON: Arg = Arg::value(
    "source-json",
    "<json-text>",
    "Inline GeoJSON text, not an MCP object; alternative to source.",
);

const OUT: Arg = Arg::value(
    "out",
    "<path>",
    "Write GeoJSON or Arrow IPC here; omitted, return a bounded receipt.",
);

const OVERWRITE: Arg = Arg::switch("overwrite", "Replace --out if it already exists.");

const LIMIT: Arg = Arg::value(
    "limit",
    "<1..20000>",
    "Bound source/output features; receipts include counts and a small sample.",
);

/// Refusals the whole family shares. Each one is the kernel's verdict, so the
/// code a caller branches on is the same one the Server and the desktop give.
pub const DOCUMENT_MALFORMED: Refusal = Refusal {
    code: "vector_document_malformed",
    when: "The source is not supported GeoJSON, KML or Arrow IPC.",
    remedy: "Use a supported layer file; inspect its format and geometry.",
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
        "vector_data_bound_exceeded" => Failure::invalid(DATA_BOUND_EXCEEDED.code, message),
        "vector_external_data_required" => Failure::invalid(EXTERNAL_DATA_REQUIRED.code, message),
        "vector_request_invalid" => Failure::invalid(REQUEST_INVALID.code, message),
        "vector_tool_unknown" => Failure::invalid(TOOL_UNKNOWN.code, message),
        "vector_tool_roadmap" => Failure::invalid(TOOL_ROADMAP.code, message),
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
    let document = parsed.map_err(|error| {
        Failure::invalid(
            DOCUMENT_MALFORMED.code,
            format!("Could not parse --{arg} document: {error}"),
        )
        .remedy(DOCUMENT_MALFORMED.remedy)
    })?;
    vector::Layer::import_geojson(&document).map_err(|error| {
        Failure::invalid(DOCUMENT_MALFORMED.code, error).remedy(DOCUMENT_MALFORMED.remedy)
    })?;
    Ok(document)
}

fn flag_source(inputs: &Inputs, arg: &str) -> Result<Value, Failure> {
    if let (Some(path), None) = (inputs.value(arg), inputs.value(&format!("{arg}-json"))) {
        return Ok(json!({"file":path}));
    }
    Ok(json!({"geojson":read_document(inputs,arg)?}))
}
pub(crate) fn read_layer_reference(
    reference: &Value,
) -> Result<std::sync::Arc<vector::Layer>, Failure> {
    let layer = if let Some(path) = reference["file"].as_str() {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|e| {
            Failure::invalid(crate::UNREADABLE.code, e.to_string())
                .remedy("Check the referenced layer file is readable.")
        })?;
        let mut bytes = Vec::new();
        file.take(vector::layer::ipc::MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| {
                Failure::invalid(crate::UNREADABLE.code, e.to_string())
                    .remedy("Read a local layer file.")
            })?;
        vector::import_layer_file(path, &bytes).map_err(refuse)?
    } else if let Some(value) = reference.get("geojson") {
        vector::Layer::import_geojson(value).map_err(|e| {
            refuse(VectorRefusal {
                code: "vector_document_malformed",
                message: e,
                remedy: DOCUMENT_MALFORMED.remedy.into(),
            })
        })?
    } else {
        return Err(Failure::invalid(REQUEST_INVALID.code,"Native layer references need a file; export the UI input IPC beside the copied request.").remedy("Set source.file/against.file to a local .arrow, .geojson or .kml path."));
    };
    Ok(std::sync::Arc::new(layer))
}

const REQUEST: Arg = Arg::value(
    "request",
    "<json-text>",
    "Exact descriptor request JSON; alternative to source and parameter flags.",
);
const DRY_RUN: Arg = Arg::switch(
    "dry-run",
    "Return counts, fields, warnings and at most five sample features; write nothing.",
);
pub const DATA_BOUND_EXCEEDED: Refusal = Refusal {
    code: "vector_data_bound_exceeded",
    when: "Typed input or projected output exceeds 32 MiB or 20000 rows.",
    remedy: "Split the layer into bounded chunks, or increase spacing.",
};
pub const EXTERNAL_DATA_REQUIRED: Refusal = Refusal {
    code: "vector_external_data_required",
    when: "Requested terrain readings are absent or do not match returned points.",
    remedy: "Acquire elevations and provide elevations_m in point order, using null for unavailable readings.",
};
pub const REQUEST_INVALID: Refusal = Refusal {
    code: "vector_request_invalid",
    when: "The request violates the tool schema or parameter relationships.",
    remedy: "Read ds data vector describe --tool <id> and follow its input_schema.",
};
pub const TOOL_UNKNOWN: Refusal = Refusal {
    code: "vector_tool_unknown",
    when: "No descriptor has the requested id or command.",
    remedy: "List ds data vector describe and choose an id.",
};
pub const TOOL_ROADMAP: Refusal = Refusal {
    code: "vector_tool_roadmap",
    when: "The tool has a schema but no executable runner.",
    remedy: "Choose a descriptor with status available.",
};

fn execute_tool(id: &str, inputs: &Inputs) -> Result<Value, Failure> {
    let parameter_flags = [
        "radius-m",
        "segments",
        "interval-m",
        "threshold",
        "min-features",
        "spatial-isolation",
        "size-outliers",
        "extent-outliers",
        "min-spacing-m",
        "max-spacing-m",
        "buffer-distance-m",
        "seed",
        "loaded",
        "computed",
        "last-read-computed",
    ];
    let mut request = if let Some(text) = inputs.value("request") {
        if ["source", "source-json", "against", "against-json", "limit"]
            .iter()
            .chain(parameter_flags.iter())
            .any(|k| inputs.value(k).is_some())
            || inputs.switch("include-ends")
        {
            return Err(Failure::invalid(
                INPUT_CHOICE_INVALID.code,
                "Use --request or source/parameter flags, not both.",
            )
            .remedy("Keep --request with --dry-run, --out and --overwrite only."));
        }
        serde_json::from_str::<Value>(text).map_err(|e| {
            Failure::invalid(REQUEST_INVALID.code, e.to_string())
                .remedy("Supply one JSON object matching input_schema.")
        })?
    } else {
        let mut value = json!({"source":flag_source(inputs,"source")?,"parameters":{},"output":{}});
        if id == "intersect" {
            value["against"] = flag_source(inputs, "against")?;
        }
        for flag in parameter_flags {
            if let Some(text) = inputs.value(flag) {
                let scalar = serde_json::from_str::<Value>(text)
                    .or_else(|error| text.parse::<f64>().map(|n| json!(n)).map_err(|_| error))
                    .map_err(|_| {
                        Failure::invalid(
                            REQUEST_INVALID.code,
                            format!("--{flag} needs a number or boolean."),
                        )
                        .remedy("Use the descriptor's parameter types.")
                    })?;
                value["parameters"][flag.replace('-', "_")] = scalar;
            }
        }
        if inputs.switch("include-ends") {
            value["parameters"]["include_ends"] = json!(true);
        }
        if let Some(text) = inputs.value("limit") {
            value["output"]["limit"] = json!(text.parse::<usize>().map_err(|_| {
                Failure::invalid(LIMIT_OUT_OF_RANGE.code, "Limit needs an integer.")
                    .remedy("Use 1 through 20000.")
            })?);
        }
        value
    };
    // Flag-style exports retain complete delivery. An explicit JSON request
    // keeps its exact projection, including terrain readings in that point order.
    if inputs.value("request").is_none() && inputs.value("out").is_some() && request.is_object() {
        if request.get("output").is_none() {
            request["output"] = json!({});
        }
        if request["output"].is_object() {
            request["output"]["projection"] = json!("complete");
        }
    }
    let dry_run = inputs.switch("dry-run");
    let request = vector::prepare(id, request).map_err(refuse)?;
    let source = read_layer_reference(&request["source"])?;
    let against = request
        .get("against")
        .map(read_layer_reference)
        .transpose()?;
    let result = vector::run(
        id,
        vector::control_request(request),
        source,
        against,
        RunOptions { dry_run },
    )
    .map_err(refuse)?;
    let mut answer = result.metadata;
    if dry_run {
        return Ok(answer);
    }
    let Some(path) = inputs.value("out") else {
        return Ok(answer);
    };
    if std::path::Path::new(path).exists() && !inputs.switch("overwrite") {
        return Err(Failure::invalid(
            crate::OUTPUT_REFUSED.code,
            format!("{path} already exists."),
        )
        .remedy("Choose another --out path or pass --overwrite."));
    }
    let bytes = if let Some(layer) = result.layer {
        if path.ends_with(".arrow") || path.ends_with(".ipc") {
            vector::layer::ipc::encode(&layer).map_err(|e| {
                Failure::invalid(crate::OUTPUT_REFUSED.code, e)
                    .remedy("Choose a writable Arrow output path.")
            })?
        } else {
            serde_json::to_vec(&layer.export_geojson()).expect("GeoJSON export")
        }
    } else {
        serde_json::to_vec(&answer["report"]).expect("report export")
    };
    std::fs::write(path, bytes).map_err(|e| {
        Failure::invalid(crate::OUTPUT_REFUSED.code, e.to_string())
            .remedy("Check the destination is writable.")
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
        None => out.push_str("  export   pass --out <path> to write layer data\n"),
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
    contract: 4,
    summary: vector::BUFFER_SUMMARY,
    purpose: "\
Grows a zone of --radius-m metres around every point, line and polygon in a \
GeoJSON document and returns it as polygons. Runs the same geodesic buffer \
the map's tools run, on this machine, with no project and no window. Each \
output names its source feature and gains `buffer_radius_m`, so \
a corridor, a setback or a service area stays traceable to what produced it.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        REQUEST,
        DRY_RUN,
        SOURCE,
        SOURCE_JSON,
        Arg::value(
            "radius-m",
            "<0.01..100000>",
            "Buffer distance in metres; Rust defaults to 25.",
        ),
        Arg::value(
            "segments",
            "<1..64>",
            "Arc segments per quarter turn; a circle has four times this many vertices.",
        ),
        OUT,
        OVERWRITE,
        LIMIT,
    ],
    output: "\
`produced`, `processed`, `source_features`, `skipped` counted by reason, and \
a bounded preview and `output_layer` schema. `result` is null; feature data \
exports only through --out as GeoJSON or Arrow IPC. `more` states what the \
source/output limit withheld. --dry-run writes nothing.",
    examples: &[Example {
        command: "ds data vector buffer --source ./poles.geojson --radius-m 30 --out ./zone.geojson",
        note: "A 30 m zone around every pole, written as GeoJSON.",
        runnable: false,
    }],
    refusals: &[
        REQUEST_INVALID,
        DATA_BOUND_EXCEEDED,
        TOOL_UNKNOWN,
        TOOL_ROADMAP,
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
    execute_tool("buffer", inputs)
}

pub fn render_buffer(data: &Value) -> String {
    render_produced(data, "zone")
}

// ── data vector sample ─────────────────────────────────────────────────

pub static SAMPLE_COMMAND: Command = Command {
    id: "data.vector.sample",
    path: &["data", "vector", "sample"],
    contract: 4,
    summary: vector::SAMPLE_SUMMARY,
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
        REQUEST,
        DRY_RUN,
        SOURCE,
        SOURCE_JSON,
        Arg::value(
            "interval-m",
            "<0.01..1000000>",
            "Spacing in metres; Rust defaults to 100.",
        ),
        Arg::switch("include-ends", "Also place a point at each line end."),
        OUT,
        OVERWRITE,
        LIMIT,
    ],
    output: "\
`produced`, `processed`, `source_features`, `skipped` counted by reason, and \
a bounded preview and `output_layer` schema. Exported points carry full \
precision `distance_m` and zero-based `part_index`. `result` is null; --out \
writes GeoJSON or Arrow IPC. `more` states the bounds; `note` explains zero points.",
    examples: &[Example {
        command: "ds data vector sample --source ./route.geojson --interval-m 25 --output json",
        note: "A pole position every 25 m along a route.",
        runnable: false,
    }],
    refusals: &[
        REQUEST_INVALID,
        DATA_BOUND_EXCEEDED,
        TOOL_UNKNOWN,
        TOOL_ROADMAP,
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
    execute_tool("sample", inputs)
}

pub fn render_sample(data: &Value) -> String {
    render_produced(data, "point")
}

// ── data vector intersect ──────────────────────────────────────────────

pub static INTERSECT_COMMAND: Command = Command {
    id: "data.vector.intersect",
    path: &["data", "vector", "intersect"],
    contract: 4,
    summary: vector::INTERSECT_SUMMARY,
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
        REQUEST,
        DRY_RUN,
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
counted by reason, with a bounded preview and output schema. `result` is \
null; --out exports crossing points as GeoJSON or Arrow IPC. `more` states \
what source/output bounds withheld; `note` explains a successful zero-crossing answer.",
    examples: &[Example {
        command: "ds data vector intersect --source ./mv.geojson --against ./roads.geojson",
        note: "Every road crossing on an MV network.",
        runnable: false,
    }],
    refusals: &[
        REQUEST_INVALID,
        DATA_BOUND_EXCEEDED,
        TOOL_UNKNOWN,
        TOOL_ROADMAP,
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
        "line intersections",
        "topology",
    ],
    requires: Requires::Server,
    availability: crate::available,
};

pub fn run_intersect(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    execute_tool("intersect", inputs)
}

pub fn render_intersect(data: &Value) -> String {
    render_produced(data, "crossing")
}

// ── data vector measure ────────────────────────────────────────────────

pub static MEASURE_COMMAND: Command = Command {
    id: "data.vector.measure",
    path: &["data", "vector", "measure"],
    contract: 4,
    summary: vector::MEASURE_SUMMARY,
    purpose: "\
Reports what a GeoJSON document actually contains: each feature's geometry \
class, vertex count, geodesic length in metres for a line and spherical area \
in square metres for a polygon, plus the totals. Read this first when a \
document came from somewhere else — it names the geometry classes the rest of \
this family will accept or skip, so an unexpected refusal never has to be \
guessed at.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[SOURCE, SOURCE_JSON, LIMIT, REQUEST, DRY_RUN, OUT, OVERWRITE],
    output: "\
`totals` (features, by geometry class, length_m, area_m2, vertices) counted \
over the WHOLE document, and a `features` array — bounded by --limit, which \
`more` then says so — carrying each feature's index, id, kind, parts, holes, \
vertices, length_m and area_m2. A polygon's area_m2 is its outer ring less \
its holes. --out exports a derived GeoJSON or Arrow layer with length_m, area_m2 and vertices; result is null.",
    examples: &[Example {
        command: "ds data vector measure --source ./network.geojson --output json",
        note: "Total line length and what geometry classes the file holds.",
        runnable: false,
    }],
    refusals: &[
        REQUEST_INVALID,
        DATA_BOUND_EXCEEDED,
        TOOL_UNKNOWN,
        TOOL_ROADMAP,
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
        "perimeter",
        "distance",
        "st_length",
        "st_area",
        "statistics",
        "add geometry attributes",
        "measure",
        "vertices",
    ],
    requires: Requires::Server,
    availability: crate::available,
};

pub fn run_measure(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    execute_tool("measure", inputs)
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
            &DESCRIBE_COMMAND,
            &OUTLIERS_COMMAND,
            &RANDOM_COMMAND,
            &COLLISIONS_COMMAND,
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
            &DESCRIBE_COMMAND,
            &OUTLIERS_COMMAND,
            &RANDOM_COMMAND,
            &COLLISIONS_COMMAND,
        ] {
            assert_eq!(command.requires, Requires::Server, "{}", command.id);
        }
    }
}

pub static DESCRIBE_COMMAND: Command = Command {
    id: "data.vector.describe",
    path: &["data", "vector", "describe"],
    contract: 1,
    summary: "Read vector tool schemas, examples, defaults and availability.",
    purpose: "Inspect the Rust-owned vector tool contracts before constructing --request JSON. With --tool, return one complete descriptor. Without it, return the catalogue including roadmap shapes. Requests name Arrow, GeoJSON or KML input files with optional layer provenance; no project, login or Desktop is needed.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value(
        "tool",
        "<id>",
        "Stable descriptor id or vector command; omitted lists the catalogue.",
    )],
    output: "One descriptor or all descriptors: id, category, summary, status, input_schema, output_schema, worked request examples and named refusals.",
    examples: &[Example {
        command: "ds data vector describe --tool sample --output json",
        note: "Learn the exact portable UI/CLI/MCP request shape.",
        runnable: true,
    }],
    refusals: &[TOOL_UNKNOWN],
    reference: Some("docs/reference/data.md"),
    search: &[
        "geoprocessing",
        "gis",
        "spatial",
        "vector",
        "schema",
        "json",
        "forms",
        "catalogue",
        "roadmap",
        "geometry",
    ],
    requires: Requires::Server,
    availability: crate::available,
};
pub fn run_describe(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    vector::describe(inputs.value("tool")).map_err(refuse)
}
pub fn render_describe(data: &Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(data).unwrap_or_default()
    )
}

pub static OUTLIERS_COMMAND: Command = Command {
    id: "data.vector.outliers",
    path: &["data", "vector", "outliers"],
    contract: 1,
    summary: vector::OUTLIERS_SUMMARY,
    purpose: "Find spatial isolation and size or extent outliers with the existing Rust robust-statistics engine. Coordinate metrics remain in source degrees. Use --request for the exact UI shape, or the source and parameter flags. --dry-run returns bounded evidence without writing an output.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        SOURCE,
        SOURCE_JSON,
        REQUEST,
        DRY_RUN,
        LIMIT,
        OUT,
        OVERWRITE,
        Arg::value(
            "threshold",
            "<1..20>",
            "Robust score threshold; Rust defaults to 3.5.",
        ),
        Arg::value(
            "min-features",
            "<3..100>",
            "Minimum analyzable group; Rust defaults to 5.",
        ),
        Arg::value(
            "spatial-isolation",
            "<true|false>",
            "Detect isolated geometry; defaults to true.",
        ),
        Arg::value(
            "size-outliers",
            "<true|false>",
            "Detect unusual size; defaults to true.",
        ),
        Arg::value(
            "extent-outliers",
            "<true|false>",
            "Detect unusual extent; defaults to true.",
        ),
    ],
    output: "Robust statistics report, derived-layer schema and preview with counts, fields, up to five sample features and warnings. Source and output bounds are explicit in more.",
    examples: &[Example {
        command: "ds data vector outliers --source ./points.geojson --dry-run --output json",
        note: "Inspect findings before writing a layer.",
        runnable: false,
    }],
    refusals: &[
        crate::UNREADABLE,
        crate::OUTPUT_REFUSED,
        DOCUMENT_MALFORMED,
        DOCUMENT_EMPTY,
        INPUT_CHOICE_INVALID,
        NO_ELIGIBLE_FEATURE,
        LIMIT_OUT_OF_RANGE,
        REQUEST_INVALID,
        DATA_BOUND_EXCEEDED,
        TOOL_UNKNOWN,
        TOOL_ROADMAP,
    ],
    reference: Some("docs/reference/data.md"),
    search: &[
        "geoprocessing",
        "spatial",
        "vector",
        "gis",
        "geometry",
        "outlier",
        "isolation",
        "statistics",
    ],
    requires: Requires::Server,
    availability: crate::available,
};
pub fn run_outliers(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    execute_tool("outliers", inputs)
}
pub fn render_outliers(data: &Value) -> String {
    render_produced(data, "outlier")
}

pub static RANDOM_COMMAND: Command = Command {
    id: "data.vector.random-points-area",
    path: &["data", "vector", "random-points-area"],
    contract: 1,
    summary: vector::RANDOM_SUMMARY,
    purpose: "Generate points in polygons with holes or buffered point/line corridors using the existing Rust spacing sampler. Seed 0 is the reproducible default. Spacing is enforced within each area. Terrain enrichment is a separate host workflow. --request accepts the UI JSON; --dry-run previews without writing a file.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        SOURCE,
        SOURCE_JSON,
        REQUEST,
        DRY_RUN,
        LIMIT,
        OUT,
        OVERWRITE,
        Arg::value(
            "min-spacing-m",
            "<0.01..1000000>",
            "Minimum point spacing per area in metres; defaults to 50.",
        ),
        Arg::value(
            "max-spacing-m",
            "<0.01..1000000>",
            "Density spacing in metres, at least the minimum; defaults to 100.",
        ),
        Arg::value(
            "buffer-distance-m",
            "<0..1000000>",
            "Metres around points/lines; defaults to 25; polygons need none.",
        ),
        Arg::value(
            "seed",
            "<number>",
            "Reproducible nonnegative seed; defaults to 0.",
        ),
    ],
    output: "Point-layer schema with source_feature_id, source_layer and one-based point_index; area and buffer counts, bounded preview and explicit more when input/output is withheld.",
    examples: &[Example {
        command: "ds data vector random-points-area --source ./boundary.geojson --seed 42 --out ./points.geojson",
        note: "Repeat the exact scatter with the same seed.",
        runnable: false,
    }],
    refusals: &[
        crate::UNREADABLE,
        crate::OUTPUT_REFUSED,
        DOCUMENT_MALFORMED,
        DOCUMENT_EMPTY,
        INPUT_CHOICE_INVALID,
        NO_ELIGIBLE_FEATURE,
        LIMIT_OUT_OF_RANGE,
        EXTERNAL_DATA_REQUIRED,
        DISTANCE_OUT_OF_RANGE,
        REQUEST_INVALID,
        DATA_BOUND_EXCEEDED,
        TOOL_UNKNOWN,
        TOOL_ROADMAP,
    ],
    reference: Some("docs/reference/data.md"),
    search: &[
        "geoprocessing",
        "spatial",
        "vector",
        "gis",
        "geometry",
        "random",
        "sampling",
        "points",
        "polygon",
        "corridor",
    ],
    requires: Requires::Server,
    availability: crate::available,
};
pub fn run_random(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    execute_tool("random-points-area", inputs)
}
pub fn render_random(data: &Value) -> String {
    render_produced(data, "point")
}

pub static COLLISIONS_COMMAND: Command = Command {
    id: "data.vector.collisions",
    path: &["data", "vector", "collisions"],
    contract: 1,
    summary: vector::COLLISIONS_SUMMARY,
    purpose: "Read existing reporter collision-region GeoJSON without recomputing overlaps. Rust ranks the reporter evidence and distinguishes never-computed, zero, found and held answers. Detection remains the authenticated project reporter operation. --request is the same JSON as the project panel; --dry-run bounds the region sample and writes nothing.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        SOURCE,
        SOURCE_JSON,
        REQUEST,
        DRY_RUN,
        LIMIT,
        Arg::value(
            "loaded",
            "<true|false>",
            "Whether a report has been read; defaults to true.",
        ),
        Arg::value(
            "computed",
            "<true|false>",
            "Whether a computed document exists; defaults to true.",
        ),
        Arg::value(
            "last-read-computed",
            "<true|false|null>",
            "Most recent read witness; defaults to null.",
        ),
    ],
    output: "Collision report with phase, counts, ranked regions, reporter evidence and freshness; dry-run exposes at most five regions. Empty collections are authoritative zero when computed is true.",
    examples: &[Example {
        command: "ds data vector collisions --source ./collisions.geojson --dry-run --output json",
        note: "Read reporter evidence without triggering detection.",
        runnable: false,
    }],
    refusals: &[
        crate::UNREADABLE,
        DOCUMENT_MALFORMED,
        INPUT_CHOICE_INVALID,
        LIMIT_OUT_OF_RANGE,
        REQUEST_INVALID,
        DATA_BOUND_EXCEEDED,
        TOOL_UNKNOWN,
        TOOL_ROADMAP,
    ],
    reference: Some("docs/reference/data.md"),
    search: &[
        "geoprocessing",
        "gis",
        "geometry",
        "spatial",
        "vector",
        "collisions",
        "overlap",
        "report",
        "regions",
        "held",
        "evidence",
    ],
    requires: Requires::Server,
    availability: crate::available,
};
pub fn run_collisions(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    execute_tool("collisions", inputs)
}
pub fn render_collisions(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default()
}
