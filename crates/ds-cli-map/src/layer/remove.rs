//! `ds map layer remove` — remove a machine-local third-party tile reference.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

pub static COMMAND: Command = Command {
    id: "map.layer.remove",
    path: &["map", "layer", "remove"],
    contract: 2,
    summary: "Remove one machine-local remote tile overlay.",
    purpose: "Removes only a third-party XYZ/PMTiles reference named by `map layer remote-list`. It cannot delete project tiles or GeoJSON sketch layers. No map needs to be open.",
    chapter: Chapter::Survey,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value("layer", "<id>", "Id from `ds map layer remote-list`.").required()],
    output: "Layer id, removed true, machine-local persistence, and whether the local registry was updated.",
    examples: &[Example {
        command: "ds map layer remove --layer tile-123-0",
        note: "Does not accept project or sketch layer ids.",
        runnable: false,
    }],
    refusals: &[super::LOCAL_STORE_REFUSAL, crate::INVALID_NUMBER],
    reference: Some("docs/reference/map.md"),
    availability: super::local_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    super::local_edit(ds_layer_store::OverlayEdit::Remove {
        id: inputs.require("layer")?.into(),
    })
}

pub fn render(data: &Value) -> String {
    format!(
        "removed {} from local overlays\n",
        data["layerId"].as_str().unwrap_or("?")
    )
}
