//! `ds map layer remove` — remove a desktop-local third-party tile reference.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "map.layer.remove",
    path: &["map", "layer", "remove"],
    contract: 1,
    summary: "Remove one desktop-local remote tile overlay.",
    purpose: "Removes only a third-party XYZ/PMTiles reference named by `map layer remote-list`. It cannot delete project tiles or GeoJSON sketch layers. No map needs to be open.",
    chapter: Chapter::Survey,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("layer", "<id>", "Id from `ds map layer remote-list`.").required(),
        DESCRIPTOR_ARG,
    ],
    output: "Layer id, removed true, desktop-local persistence, and whether an open map was updated.",
    examples: &[Example {
        command: "ds map layer remove --layer tile-123-0",
        note: "Does not accept project or sketch layer ids.",
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
    crate::invoke(
        &descriptor,
        &crate::REMOTE_LAYER_REMOVE,
        json!({ "layerId": inputs.require("layer")? }),
        crate::UI_TIMEOUT,
    )
}
pub fn render(data: &Value) -> String {
    format!(
        "removed {} · map updated: {}\n",
        data["layerId"].as_str().unwrap_or("?"),
        data["mapUpdated"].as_bool().unwrap_or(false)
    )
}
