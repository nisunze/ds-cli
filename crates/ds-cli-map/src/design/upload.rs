//! `ds map design upload inspect` — preview a native source before upload.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

const PATH_ARG: Arg = Arg {
    name: "path",
    kind: ArgKind::Repeated,
    value: "<native-path>",
    required: true,
    default: None,
    choices: &[],
    summary: "Native source file to inspect without applying it. Repeat for a batch.",
};

const PARALLEL_ARG: Arg = Arg {
    name: "parallel",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: None,
    choices: &[],
    summary: "Concurrent desktop inspection jobs (1-32); defaults to the app batch setting.",
};

const NETWORK_ARG: Arg = Arg {
    name: "network",
    kind: ArgKind::Value,
    value: "<lv|mv>",
    required: false,
    default: Some("lv"),
    choices: &["lv", "mv"],
    summary: "Target design vocabulary used for layer and column suggestions.",
};

pub static COMMAND: Command = Command {
    id: "map.design.upload.inspect",
    path: &["map", "design", "upload", "inspect"],
    contract: 1,
    summary: "Inspect a native design source before upload.",
    purpose: "\
Parses one or many would-be design uploads inside the paired desktop and returns a bounded \
inventory of layers, columns, suggested design targets, header mappings, and \
cleaning counts. It never applies, stages, copies, or uploads the source.",
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[PATH_ARG, NETWORK_ARG, PARALLEL_ARG, DESCRIPTOR_ARG],
    output: "\
Per-file success or failure and the network vocabulary; successful files carry \
up to 100 layers and 200 columns per layer, suggested canonical headers, cleaning \
counts, and explicit truncation counts.",
    examples: &[Example {
        command: "ds map design upload inspect --path ./survey.zip --network lv --output json",
        note: "Repeat --path for a batch; review each successful .data.files[] result before apply.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        super::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("paths".into(), json!(inputs.repeated("path")));
    if let Some(network) = inputs.value("network") {
        arguments.insert("network".into(), json!(network));
    }
    if let Some(parallel) = inputs.value("parallel") {
        arguments.insert(
            "parallel".into(),
            json!(crate::integer(parallel, "parallel", 1, 32)?),
        );
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::DESIGN_UPLOAD_INSPECT,
        Value::Object(arguments),
        crate::DESIGN_READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} source(s): {} inspected, {} failed · {} vocabulary · {} parallel\n",
        data["total"].as_u64().unwrap_or(0),
        data["succeeded"].as_u64().unwrap_or(0),
        data["failed"].as_u64().unwrap_or(0),
        data["network"].as_str().unwrap_or("lv"),
        data["parallel"].as_u64().unwrap_or(1),
    );
    if let Some(files) = data["files"].as_array() {
        for file in files {
            if !file["ok"].as_bool().unwrap_or(false) {
                out.push_str(&format!(
                    "  ✗ {}  {}\n",
                    file["path"].as_str().unwrap_or("source"),
                    file["error"].as_str().unwrap_or("inspection failed"),
                ));
                continue;
            }
            out.push_str(&format!(
                "  ✓ {} · {} layer(s)\n",
                file["fileName"].as_str().unwrap_or("source"),
                file["totalLayers"].as_u64().unwrap_or(0),
            ));
        }
    }
    out
}
