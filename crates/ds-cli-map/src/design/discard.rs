//! `ds map design discard` — restore one transformer room from the cloud.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;
use crate::design::TRANSFORMER_ARG;

pub static COMMAND: Command = Command {
    id: "map.design.discard",
    path: &["map", "design", "discard"],
    contract: 1,
    summary: "Discard a transformer's unsaved local room.",
    purpose: "Replace one transformer's unsaved local room with the exact current cloud baseline. This never saves or changes project data; retained local map layers are untouched so a reviewed workflow can be replayed.",
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, DESCRIPTOR_ARG],
    output: "Whether unsaved work was discarded, the restored cloud version, and the restored layer and feature counts.",
    examples: &[Example {
        command: "ds map design discard --transformer T-1042 --output json",
        note: "Restores only T-1042's local room; project data and local review layers are untouched.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "no such transformer, or the cloud baseline could not be loaded",
            remedy: "check the transformer and project, then retry once the project is reachable",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_DISCARD,
        json!({ "transformer": transformer }),
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;

    Ok(json!({
        "transformer": transformer,
        "project": result["project"],
        "discarded": result["discarded"],
        "dirty": result["dirty"],
        "server_version": result["serverVersion"],
        "layers": result["layerCount"],
        "features": result["featureCount"],
        "persisted": false,
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "restored  {}  cloud v{}  {} features\n",
        data["transformer"].as_str().unwrap_or("?"),
        data["server_version"]
            .as_i64()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "?".into()),
        data["features"].as_u64().unwrap_or(0),
    )
}
