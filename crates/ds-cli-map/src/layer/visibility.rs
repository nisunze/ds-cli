//! `ds map layer visibility` — idempotently show or hide a remote overlay.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "map.layer.visibility",
    path: &["map", "layer", "visibility"],
    contract: 1,
    summary: "Set a desktop-local remote overlay visible or hidden.",
    purpose: "Sets, rather than toggles, one persisted XYZ/PMTiles overlay visibility so retries are idempotent. The setting is stored with the local reference and reconciled immediately when a map is open.",
    chapter: Chapter::Survey,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("layer", "<id>", "Id from `ds map layer remote-list`.").required(),
        Arg::value("visible", "<true|false>", "Exact desired state.")
            .required()
            .choices(&["true", "false"]),
        DESCRIPTOR_ARG,
    ],
    output: "Layer id, exact visibility, desktop-local persistence, and whether an open map was updated.",
    examples: &[Example {
        command: "ds map layer visibility --layer tile-123-0 --visible false",
        note: "Safe to retry; this is not a toggle.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let visible = inputs.require("visible")? == "true";
    crate::invoke(
        &descriptor,
        &crate::REMOTE_LAYER_VISIBILITY,
        json!({ "layerId": inputs.require("layer")?, "visible": visible }),
        crate::UI_TIMEOUT,
    )
}
pub fn render(data: &Value) -> String {
    format!(
        "{} {} · map updated: {}\n",
        data["layerId"].as_str().unwrap_or("?"),
        if data["visible"].as_bool().unwrap_or(false) {
            "visible"
        } else {
            "hidden"
        },
        data["mapUpdated"].as_bool().unwrap_or(false)
    )
}
