//! `ds map layer remote-list` — machine-local third-party tile references.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "map.layer.remote-list",
    path: &["map", "layer", "remote-list"],
    contract: 2,
    summary: "List machine-local XYZ and raster PMTiles references.",
    purpose: "Reads third-party tile references persisted in the shared native local store. These are local overlays, not project catalogue tiles (`ds tile list`), and listing them does not require sign-in or an open map.",
    chapter: Chapter::Survey,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value("limit", "<n>", "Report at most this many layers; 1..100.").default("100")],
    output: "Local remote-layer count and rows with id, name, kind, URL, tile size, attribution, visibility, and machine-local persistence.",
    examples: &[Example {
        command: "ds map layer remote-list --output json",
        note: "No map or project needs to be open.",
        runnable: false,
    }],
    refusals: &[super::LOCAL_STORE_REFUSAL, crate::INVALID_NUMBER],
    reference: Some("docs/reference/map.md"),
    availability: super::local_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = crate::integer(inputs.require("limit")?, "limit", 1, 100)? as usize;
    let result = ds_layer_store::execute(ds_layer_store::OverlayEdit::List).map_err(|message| {
        Failure::invalid("local_layer_refused", message).remedy(super::LOCAL_STORE_REFUSAL.remedy)
    })?;
    let rows = result["layers"].as_array().expect("typed registry");
    Ok(
        json!({"layer_count":rows.len(),"layers":rows.iter().take(limit).collect::<Vec<_>>(),"more":rows.len()>limit,"persisted":"native_local"}),
    )
}

pub fn render(data: &Value) -> String {
    let mut out = format!("{} machine-local remote layers\n", data["layer_count"]);
    for layer in data["layers"].as_array().into_iter().flatten() {
        out.push_str(&super::render_remote(layer));
    }
    out
}
