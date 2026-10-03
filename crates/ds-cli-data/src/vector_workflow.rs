//! Workflow host adapter: bounded file input and named exports only. Rust's
//! shared network runner owns graph planning, types, iteration and provenance.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_network::vector::{RunOptions, workflow};
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

const FILE: Arg = Arg {
    required: true,
    ..Arg::value(
        "file",
        "<wf.json>",
        "Local ds.vector-workflow/v1 JSON document, bounded to 8 MiB.",
    )
};
const INPUTS: Arg = Arg::value(
    "inputs",
    "<json-text>",
    "Model parameter bindings as JSON text; omitted uses defaults.",
);
const OUT: Arg = Arg::value(
    "out",
    "<directory>",
    "Export named outputs and provenance here; existing files refuse.",
);
const DRY_RUN: Arg = Arg::switch(
    "dry-run",
    "Compute per-step previews without creating files or folders.",
);
const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "vector_workflow_invalid",
        when: "The model or bindings have an invalid shape or missing inputs.",
        remedy: "Read workflow describe and fix the named JSON path.",
    },
    Refusal {
        code: "vector_workflow_reference_invalid",
        when: "An input, step, output port or declared field reference is absent.",
        remedy: "Fix the named reference; guard outputs of skipped producers.",
    },
    Refusal {
        code: "vector_workflow_type_mismatch",
        when: "Producer and consumer types or geometry kinds do not match.",
        remedy: "Use compatible layer geometry and scalar types at the named path.",
    },
    Refusal {
        code: "vector_workflow_cycle",
        when: "A reference cycle prevents topological execution.",
        remedy: "Remove the back edge named by the blocked step.",
    },
    Refusal {
        code: "vector_workflow_bound_exceeded",
        when: "JSON, feature, iteration or conservative generator limits exceed bounds.",
        remedy: "Use inline projection, larger spacing or larger iterator chunks; split the model.",
    },
    Refusal {
        code: "vector_workflow_step_failed",
        when: "A step failed; completed outputs remain in error.detail.",
        remedy: "Read error.detail.error for the step, path and kernel remedy.",
    },
    super::vector::TOOL_UNKNOWN,
    super::vector::TOOL_ROADMAP,
    super::vector::REQUEST_INVALID,
    super::vector::DATA_BOUND_EXCEEDED,
    super::vector::DOCUMENT_MALFORMED,
    super::vector::DISTANCE_OUT_OF_RANGE,
    super::vector::LIMIT_OUT_OF_RANGE,
    crate::UNREADABLE,
    Refusal {
        code: crate::OUTPUT_REFUSED.code,
        when: "An export path exists, is unwritable or collides with the receipt.",
        remedy: "Choose a fresh writable directory or rename the conflicting output.",
    },
];
pub static DESCRIBE_COMMAND: Command = Command {
    id: "data.vector.workflow.describe",
    path: &["data", "vector", "workflow", "describe"],
    contract: 1,
    summary: "Read compact vector workflow contracts and executable examples.",
    purpose: "Discover ds.vector-workflow/v1, typed model parameters, output ports, conditions, iterators and resource bounds. The catalogue includes only available Rust tools and eight embedded fixture workflows. Use --example to copy one complete workflow, then validate it before running. This needs no project, login or window.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value(
        "example",
        "<1..8>",
        "Return one embedded workflow document by its 1-based index.",
    )],
    output: "Compact JSON model schema, seven available tool contracts, typed inputs/outputs and eight executable examples; --example returns one complete model.",
    examples: &[Example {
        command: "ds data vector workflow describe --output json",
        note: "Discover the model contract and available chained-tool examples.",
        runnable: true,
    }],
    refusals: &[REFUSALS[0]],
    reference: Some("docs/reference/data.md"),
    search: &[
        "model",
        "modelbuilder",
        "chain",
        "geoprocessing",
        "gis",
        "geometry",
        "spatial",
        "json",
    ],
    requires: Requires::Server,
    availability: crate::available,
};
pub static VALIDATE_COMMAND: Command = Command {
    id: "data.vector.workflow.validate",
    path: &["data", "vector", "workflow", "validate"],
    contract: 1,
    summary: "Validate vector workflow references, types and execution order.",
    purpose: "Read a local workflow and optional model input bindings. Rust checks schema, unique ids, references, cycles, producer/consumer types, geometry kinds, conditions and literal or bound parameter values before execution. Computed parameter constraints are checked again when their real values resolve. Unknown and roadmap tools name their step. No computation or file writes occur.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[FILE, INPUTS],
    output: "valid, topological order, dependencies, typed step outputs, model parameters and any required unbound inputs. Errors name step_id, path and remedy.",
    examples: &[Example {
        command: "ds data vector workflow validate --file workflow.json --output json",
        note: "Check a model before running it; the file is user supplied.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/data.md"),
    search: &[
        "graph",
        "cycle",
        "schema",
        "geoprocessing",
        "gis",
        "geometry",
        "spatial",
    ],
    requires: Requires::Server,
    availability: crate::available,
};
pub static RUN_COMMAND: Command = Command {
    id: "data.vector.workflow.run",
    path: &["data", "vector", "workflow", "run"],
    contract: 1,
    summary: "Run a bounded vector workflow with previews and provenance.",
    purpose: "Execute the validated graph in topological order through the shared Rust vector runner. Preconditions skip steps; feature chunks, groups and layer iterators bind iterator.layer. Keep bounded intermediates in memory and return only named outputs unless a failure keeps completed outputs in error.detail. --dry-run computes truthful downstream previews and writes nothing. --out exports named results and their provenance without overwriting files.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[FILE, INPUTS, DRY_RUN, OUT],
    output: "Named layer/report outputs, per-step previews and SHA-256 provenance. First failure stops the graph with completed_outputs in error.detail. Dry runs return bounded samples marked preview_only.",
    examples: &[Example {
        command: "ds data vector workflow run --file workflow.json --dry-run --output json",
        note: "Inspect every step without exporting intermediates or outputs.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/data.md"),
    search: &[
        "chain",
        "modelbuilder",
        "iterator",
        "geoprocessing",
        "gis",
        "geometry",
        "spatial",
    ],
    requires: Requires::Server,
    availability: crate::available,
};
fn refusal(error: workflow::WorkflowError) -> Failure {
    let detail = serde_json::to_value(&error).unwrap();
    let message = format!(
        "{}{}: {}",
        error
            .step_id
            .as_deref()
            .map(|s| format!("Step {s}, "))
            .unwrap_or_default(),
        error.path,
        error.message
    );
    let failure = match error.code.as_str() {
        "vector_workflow_invalid"
        | "vector_workflow_reference_invalid"
        | "vector_workflow_type_mismatch"
        | "vector_workflow_cycle"
        | "vector_workflow_bound_exceeded"
        | "vector_tool_unknown"
        | "vector_tool_roadmap"
        | "vector_document_malformed"
        | "vector_data_bound_exceeded"
        | "vector_request_invalid"
        | "vector_distance_out_of_range"
        | "vector_limit_out_of_range" => Failure::invalid(&error.code, message),
        undeclared => {
            return Failure::internal(
                "vector_workflow_refusal_undeclared",
                format!("Workflow owner returned undeclared refusal {undeclared}."),
            )
            .remedy("Report this workflow contract mismatch.")
            .detail(detail);
        }
    };
    failure.remedy(error.remedy).detail(detail)
}
fn read(inputs: &Inputs) -> Result<(Value, Value), Failure> {
    let path = inputs.value("file").expect("declared required file");
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take(workflow::MAX_DOCUMENT_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|e| {
            Failure::invalid(crate::UNREADABLE.code, e.to_string())
                .remedy("Choose a readable local workflow JSON file.")
        })?;
    let text = std::str::from_utf8(&bytes).map_err(|e| {
        Failure::invalid("vector_workflow_invalid", e.to_string()).remedy("Use UTF-8 JSON.")
    })?;
    Ok((
        workflow::parse(text).map_err(refusal)?,
        workflow::parse(inputs.value("inputs").unwrap_or("{}")).map_err(refusal)?,
    ))
}
type BoundWorkflow = (
    Value,
    Value,
    std::collections::BTreeMap<String, std::sync::Arc<ds_network::vector::Layer>>,
);

fn bind_files(
    mut document: Value,
    mut bindings: Value,
    inputs: &Inputs,
) -> Result<BoundWorkflow, Failure> {
    let base = Path::new(inputs.value("file").unwrap())
        .parent()
        .unwrap_or(Path::new("."));
    let mut layers = std::collections::BTreeMap::new();
    let mut bind = |value: &mut Value, id: String| -> Result<(), Failure> {
        if let Some(file) = value["file"].as_str() {
            let mut reference = value.clone();
            reference["file"] = json!(base.join(file));
            let layer = super::vector::read_layer_reference(&reference)?;
            layers.insert(id.clone(), layer);
            *value = json!({"layer_id":id});
        }
        Ok(())
    };
    if let Some(parameters) = document["inputs"].as_object_mut() {
        for (name, input) in parameters {
            if input["type"] != "layer" {
                continue;
            }
            let many = input["many"] == true;
            let value = if bindings.get(name).is_some() {
                &mut bindings[name]
            } else {
                &mut input["default"]
            };
            if many {
                if let Some(values) = value.as_array_mut() {
                    for (i, value) in values.iter_mut().enumerate() {
                        bind(value, format!("files.{name}.{i}"))?;
                    }
                }
            } else {
                bind(value, format!("files.{name}"))?;
            }
        }
    }
    if let Some(steps) = document["steps"].as_array_mut() {
        for step in steps {
            let id = step["id"].as_str().unwrap_or("unknown").to_owned();
            for port in ["source", "against"] {
                if step["request"].get(port).is_some() {
                    bind(&mut step["request"][port], format!("files.{id}.{port}"))?;
                }
            }
        }
    }
    layers.extend(workflow::import_file_layers(&mut document, &mut bindings).map_err(refusal)?);
    Ok((document, bindings, layers))
}

pub fn run_describe(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    match inputs.value("example") {
        None => Ok(workflow::describe()),
        Some(value) => value
            .parse::<usize>()
            .ok()
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| workflow::examples().get(index).cloned())
            .ok_or_else(|| {
                Failure::invalid("vector_workflow_invalid", "Example must be 1 through 8.")
                    .remedy("Omit --example for the complete example catalogue.")
            }),
    }
}
pub fn run_validate(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let (document, bindings) = read(inputs)?;
    let (document, bindings, layers) = bind_files(document, bindings, inputs)?;
    workflow::validate_layers(document, bindings, layers).map_err(refusal)
}
pub fn run_workflow(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let (document, bindings) = read(inputs)?;
    let dry_run = inputs.switch("dry-run");
    let (document, bindings, layers) = bind_files(document, bindings, inputs)?;
    let result =
        workflow::run(document, bindings, layers, RunOptions { dry_run }).map_err(refusal)?;
    let mut answer = result.metadata;
    if !dry_run && let Some(directory) = inputs.value("out") {
        export(
            &mut answer,
            &result.outputs,
            &result.completed_layers,
            Path::new(directory),
        )?;
    }
    if answer["status"] == "failed" {
        return Err(Failure::failed(
            "vector_workflow_step_failed",
            format!(
                "Step {} at {}: {}",
                answer["error"]["step_id"].as_str().unwrap_or("outputs"),
                answer["error"]["path"].as_str().unwrap_or("$"),
                answer["error"]["message"]
                    .as_str()
                    .unwrap_or("Step failed.")
            ),
        )
        .remedy(
            answer["error"]["remedy"]
                .as_str()
                .unwrap_or("Read error.detail."),
        )
        .detail(answer));
    }
    Ok(answer)
}
fn export(
    answer: &mut Value,
    layers: &std::collections::BTreeMap<String, std::sync::Arc<ds_network::vector::Layer>>,
    completed: &std::collections::BTreeMap<String, std::sync::Arc<ds_network::vector::Layer>>,
    directory: &Path,
) -> Result<(), Failure> {
    let refused = |message: String| {
        Failure::invalid(crate::OUTPUT_REFUSED.code, message)
            .remedy("Choose a writable output directory with no existing named output files.")
    };
    let mut outputs = answer["outputs"].as_object().cloned().unwrap_or_default();
    let mut binary = layers.clone();
    if answer["status"] == "failed" {
        for (name, layer) in completed {
            let name = format!("completed_{name}");
            binary.insert(name.clone(), layer.clone());
            outputs.insert(name, json!({"type":"ds.layer-ref"}));
        }
    }
    let paths = outputs
        .iter()
        .map(|(name, _value)| {
            (
                name.clone(),
                directory.join(format!(
                    "{name}.{}",
                    if binary.contains_key(name) {
                        "arrow"
                    } else {
                        "json"
                    }
                )),
            )
        })
        .collect::<Vec<(String, PathBuf)>>();
    let manifest = directory.join("workflow-result.json");
    let mut unique = std::collections::BTreeSet::new();
    for path in paths
        .iter()
        .map(|(_, p)| p)
        .chain(std::iter::once(&manifest))
    {
        if !unique.insert(path.clone()) {
            return Err(refused(
                "Named output collides with workflow-result.json; rename that output.".into(),
            ));
        }
        if path.try_exists().map_err(|e| refused(e.to_string()))? {
            return Err(refused(format!("{} already exists.", path.display())));
        }
    }
    std::fs::create_dir_all(directory).map_err(|e| refused(e.to_string()))?;
    let mut written = serde_json::Map::new();
    for (name, path) in paths {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| refused(e.to_string()))?;
        if let Some(layer) = binary.get(&name) {
            use std::io::Write;
            let mut file = file;
            file.write_all(&ds_network::vector::layer::ipc::encode(layer).map_err(refused)?)
                .map_err(|e| refused(e.to_string()))?;
        } else {
            serde_json::to_writer(file, &outputs[&name]).map_err(|e| refused(e.to_string()))?;
        }
        written.insert(name, json!(path));
    }
    let mut receipt = answer.clone();
    receipt["outputs"] = Value::Null;
    receipt["written_to"] = json!(written);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest)
        .map_err(|e| refused(e.to_string()))?;
    serde_json::to_writer(file, &receipt).map_err(|e| refused(e.to_string()))?;
    answer["written_to"] = json!(written);
    answer["receipt"] = json!(manifest);
    Ok(())
}
pub fn render(data: &Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(data).unwrap_or_default()
    )
}
