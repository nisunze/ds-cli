//! `ds tile add` — mount another project's published output here.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{LANE_ARG, TYPE_ARG};

const SOURCE_PROJECT_ARG: Arg = Arg {
    name: "source-project",
    kind: ArgKind::Value,
    value: "<project-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The project whose published output of this type to add.",
};

pub static COMMAND: Command = Command {
    id: "tile.add",
    path: &["tile", "add"],
    contract: 2,
    summary: "Add another project's published tiles to this project (needs --yes).",
    purpose: "\
Imports another project's published output into destination-owned tile storage. \
Restores the native user and selected project; ds-brain checks access to both \
projects and performs the copy. No desktop is required.",
    chapter: Chapter::VectorTiles,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TYPE_ARG, SOURCE_PROJECT_ARG, LANE_ARG],
    output: "`project`, `type`, `sourceProject`, `added: true`.",
    examples: &[Example {
        command: "ds tile add --type design --source-project neighbouring-district --yes",
        note: "Then `ds tile list` shows the row with its source project.",
        runnable: false,
    }],
    refusals: crate::NATIVE_WRITE_REFUSALS,
    reference: Some("docs/reference/tile.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let result = ds_cli_auth::tile_add(
        inputs.require("lane")?,
        crate::tile_type(inputs.require("type")?),
        inputs.require("source-project")?,
    )?;
    Ok(
        json!({"lane": result.lane(), "project": result.project_id(), "type": inputs.require("type")?,
        "sourceProject": inputs.require("source-project")?, "tileId": result.result().tile_id, "added": true}),
    )
}

pub fn render(data: &Value) -> String {
    format!(
        "added {} tiles from {} to project {}\n",
        data["type"].as_str().unwrap_or("?"),
        data["sourceProject"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
    )
}
