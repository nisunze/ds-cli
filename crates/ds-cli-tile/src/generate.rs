//! `ds tile generate` — start one fixed backend run after CLI confirmation.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::{FORCE_ARG, LANE_ARG, TYPE_ARG};

pub static COMMAND: Command = Command {
    id: "tile.generate",
    path: &["tile", "generate"],
    contract: 1,
    summary: "Regenerate one output's vector tiles (needs --yes).",
    purpose: "\
After CLI confirmation, restores the native user and calls the fixed tile \
generation operation for only its audience-fenced selected project. ds-brain \
owns the staleness decision, preflight, lease and dispatch. Returns as soon as \
the backend answers; follow it with `ds tile status`. Use --force after a \
restyle or Data-cleaning catalog change. No project, Desktop descriptor, URL, \
body or action override is accepted.",
    chapter: Chapter::VectorTiles,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TYPE_ARG, FORCE_ARG, LANE_ARG],
    output: "\
Lane and selected-project identity/status, `type`, `force`, whether work was \
dispatched, and the fixed backend result including status, decision, \
timestamps and bounded diagnostics.",
    examples: &[Example {
        command: "ds tile generate --type design --force --yes",
        note: "Runs take minutes; `ds tile status --type design` reports progress.",
        runnable: false,
    }],
    refusals: crate::NATIVE_WRITE_REFUSALS,
    reference: Some("docs/reference/tile.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let kind = crate::tile_type(inputs.require("type")?);
    let force = inputs.switch("force");
    let headless = ds_cli_auth::tile_generate(inputs.require("lane")?, kind, force)?;
    let result = headless.result();
    let mut output = crate::operation_project(&headless);
    output["type"] = Value::String(kind.token().to_owned());
    output["force"] = Value::Bool(force);
    output["dispatched"] =
        Value::Bool(result.status() == ds_cli_auth::TileOperationStatus::Started);
    output["result"] = crate::operation_json(result);
    Ok(output)
}

pub fn render(data: &Value) -> String {
    crate::plan::render_decision(data)
}
