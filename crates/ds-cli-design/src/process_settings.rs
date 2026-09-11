//! Which process settings a run would carry — the kernel's answer, headless.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};
use std::io::Read;

fn local() -> Availability {
    Availability::Available
}

const REFUSALS: &[Refusal] = &[Refusal {
    code: "process_settings_invalid",
    when: "A settings file is malformed, or a value is neither boolean nor finite numeric",
    remedy: "Pass the project configuration as `ds design config` returns it and settings as {key: bool|number}",
}];

const LANE: Arg = Arg::value("lane", "<lane>", "Which lane runs the process.")
    .choices(&["standard", "fast"])
    .default("fast");
const PRESET: Arg = Arg::value("preset", "<preset>", "The named preset.")
    .choices(&["drafting", "sketch"])
    .required();
const PROJECT_CONFIG: Arg = Arg::value(
    "project-config",
    "<json-file>",
    "The project's configuration document ({sheets: …}); without it the catalogue alone resolves.",
);
const OPERATOR: Arg = Arg::value(
    "operator",
    "<json-file>",
    "The operator's own toggles ({key: bool|number}) laid over the preset.",
);
const FIRESTORE_DESIGN_DATA: Arg = Arg::switch(
    "firestore-design-data",
    "The project keeps design data in Firestore (no combined mirror).",
);

pub static COMMAND: Command = Command {
    id: "design.process.settings",
    path: &["design", "process", "settings"],
    contract: 1,
    summary: "Resolve the process settings a lane and preset would send.",
    purpose: "What a named preset produces for a project, which dependent flags collapse into the effective contract, which settings the operator may see for a lane and preset, and what a hidden setting puts on the wire are decided once in ds-command-kernel (process_settings) over the engine's own catalogue — the same answer the LV process dialog sends. Every key that leaves the wire is named with its reason.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        LANE,
        PRESET,
        PROJECT_CONFIG,
        OPERATOR,
        FIRESTORE_DESIGN_DATA,
    ],
    output: "{settings, visible_groups, wire_settings, dropped[{key, why, forced_to?}]}.",
    examples: &[Example {
        command: "ds design process settings --preset sketch --lane fast --output json",
        note: "`.data.wire_settings` is the payload a Fast Sketch run carries.",
        runnable: true,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: local,
};

fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("process_settings_invalid", e.to_string())
        .remedy("Pass the project configuration as `ds design config` returns it and settings as {key: bool|number}")
}

fn json_file(path: &str) -> Result<Value, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(invalid)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(invalid("file exceeds 8 MiB"));
    }
    serde_json::from_slice(&bytes).map_err(invalid)
}

pub fn run(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let catalog: Value =
        serde_json::from_str(ds_network::network::PROCESSOR_PRESETS_JSON).map_err(invalid)?;
    let mut request = json!({
        "op": "resolve",
        "catalog": catalog,
        "lane": i.require("lane")?,
        "preset": i.require("preset")?,
        "use_firestore_design_data": i.switch("firestore-design-data"),
    });
    if let Some(path) = i.value("project-config") {
        request["project_config"] = json_file(path)?;
    }
    if let Some(path) = i.value("operator") {
        request["operator"] = json_file(path)?;
    }
    let input = serde_json::to_vec(&request).map_err(invalid)?;
    let reply = ds_command_kernel::process_settings::evaluate(&input).map_err(invalid)?;
    serde_json::from_str(&reply).map_err(invalid)
}

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    if let Some(wire) = data["wire_settings"].as_object() {
        for (key, value) in wire {
            out.push_str(&format!("  {key:<40} {value}\n"));
        }
    }
    if let Some(dropped) = data["dropped"].as_array() {
        for entry in dropped {
            out.push_str(&format!(
                "  - {:<38} {}{}\n",
                entry["key"].as_str().unwrap_or("?"),
                entry["why"].as_str().unwrap_or(""),
                entry["forced_to"]
                    .as_bool()
                    .map(|v| format!(" → {v}"))
                    .unwrap_or_default()
            ));
        }
    }
    out
}
