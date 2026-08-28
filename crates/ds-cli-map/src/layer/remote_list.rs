//! `ds map layer remote-list` — desktop-local third-party tile references.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "map.layer.remote-list",
    path: &["map", "layer", "remote-list"],
    contract: 1,
    summary: "List desktop-local XYZ and raster PMTiles references.",
    purpose: "Reads third-party tile references persisted in this desktop's IndexedDB. These are local overlays, not project catalogue tiles (`ds tile list`), and listing them does not require sign-in or an open map.",
    chapter: Chapter::Survey,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("limit", "<n>", "Report at most this many layers; 1..100.").default("100"),
        DESCRIPTOR_ARG,
    ],
    output: "Local remote-layer count and rows with id, name, kind, URL, tile size, attribution, visibility, and desktop-local persistence.",
    examples: &[Example {
        command: "ds map layer remote-list --output json",
        note: "No map or project needs to be open.",
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
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = crate::integer(inputs.require("limit")?, "limit", 1, 100)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::LAYERS_LIST,
        json!({ "scope": "remote", "limit": limit }),
        crate::UI_TIMEOUT,
    )?;
    Ok(
        json!({ "map_open": result["mapOpen"], "layer_count": result["remoteTotal"], "layers": result["remote"] }),
    )
}

pub fn render(data: &Value) -> String {
    let mut out = format!("{} desktop-local remote layers\n", data["layer_count"]);
    for layer in data["layers"].as_array().into_iter().flatten() {
        out.push_str(&super::render_remote(layer));
    }
    out
}
