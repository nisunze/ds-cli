//! `ds tile remove` — drop one archive from the project's catalogue.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::LANE_ARG;

const TILE_ID_ARG: Arg = Arg {
    name: "tile-id",
    kind: ArgKind::Value,
    value: "<tile-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The archive id, as `ds tile list` reports it.",
};
const SCOPE_ARG: Arg = Arg {
    name: "scope",
    kind: ArgKind::Value,
    value: "<project|global>",
    required: false,
    default: Some("project"),
    choices: &["project", "global"],
    summary: "Where the archive is catalogued. Global removal needs the global-tiles capability.",
};

pub static COMMAND: Command = Command {
    id: "tile.remove",
    path: &["tile", "remove"],
    contract: 2,
    summary: "Remove one tile archive from the catalogue (needs --yes).",
    purpose: "\
Removes one archive — an added reference or the project's own output — \
through the same governed action the Pipeline panel uses. ds-brain \
retires the catalogue row and reclaims owned cached storage. Native user \
authority is restored without a desktop.",
    chapter: Chapter::VectorTiles,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TILE_ID_ARG, SCOPE_ARG, LANE_ARG],
    output: "`project`, `tileId`, `scope`, `removed: true`.",
    examples: &[Example {
        command: "ds tile remove --tile-id neighbouring-district_design --yes",
        note: "Read the id from `ds tile list` first.",
        runnable: false,
    }],
    refusals: crate::NATIVE_WRITE_REFUSALS,
    reference: Some("docs/reference/tile.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let scope = if inputs.require("scope")? == "global" {
        ds_cli_auth::TileScope::Global
    } else {
        ds_cli_auth::TileScope::Project
    };
    let result =
        ds_cli_auth::tile_remove(inputs.require("lane")?, inputs.require("tile-id")?, scope)?;
    Ok(
        json!({"lane": result.lane(), "project": result.project_id(), "tileId": result.result().tile_id,
        "scope": result.result().scope, "removed": true}),
    )
}

pub fn render(data: &Value) -> String {
    format!(
        "removed {} ({}) from project {}\n",
        data["tileId"].as_str().unwrap_or("?"),
        data["scope"].as_str().unwrap_or("project"),
        data["project"].as_str().unwrap_or("?"),
    )
}
