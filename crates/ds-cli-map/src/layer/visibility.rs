//! `ds map layer visibility` — idempotently show or hide a remote overlay.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

pub static COMMAND: Command = Command {
    id: "map.layer.visibility",
    path: &["map", "layer", "visibility"],
    contract: 2,
    summary: "Set a machine-local remote overlay visible or hidden.",
    purpose: "Sets, rather than toggles, one persisted XYZ/PMTiles overlay visibility so retries are idempotent. The setting is stored with the local reference and reconciled immediately when a map is open.",
    chapter: Chapter::Survey,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("layer", "<id>", "Id from `ds map layer remote-list`.").required(),
        Arg::value("visible", "<true|false>", "Exact desired state.")
            .required()
            .choices(&["true", "false"]),
    ],
    output: "Layer id, exact visibility, machine-local persistence, and whether the local registry was updated.",
    examples: &[Example {
        command: "ds map layer visibility --layer tile-123-0 --visible false",
        note: "Safe to retry; this is not a toggle.",
        runnable: false,
    }],
    refusals: &[super::LOCAL_STORE_REFUSAL, crate::INVALID_NUMBER],
    reference: Some("docs/reference/map.md"),
    availability: super::local_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    super::local_edit(ds_layer_store::OverlayEdit::Visibility {
        id: inputs.require("layer")?.into(),
        visible: inputs.require("visible")? == "true",
    })
}

pub fn render(data: &Value) -> String {
    format!(
        "{} {} · saved locally\n",
        data["layerId"].as_str().unwrap_or("?"),
        if data["visible"].as_bool().unwrap_or(false) {
            "visible"
        } else {
            "hidden"
        }
    )
}
