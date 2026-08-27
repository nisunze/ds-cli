//! `ds tile add` — mount another project's published output here.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, TYPE_ARG};

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
    contract: 1,
    summary: "Add another project's published tiles to this project (needs --yes).",
    purpose: "\
References another project's published output of the given type from this \
project's catalogue, through the same governed action the Pipeline panel \
uses. ds-brain checks membership on both projects. Nothing is copied or \
re-tiled.",
    chapter: Chapter::VectorTiles,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TYPE_ARG, SOURCE_PROJECT_ARG, DESCRIPTOR_ARG],
    output: "`project`, `type`, `sourceProject`, `added: true`.",
    examples: &[Example {
        command: "ds tile add --type design --source-project neighbouring-district --yes",
        note: "Then `ds tile list` shows the row with its source project.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::TILE_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/tile.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TILE_ADD,
        json!({
            "type": inputs.require("type")?,
            "source_project": inputs.require("source-project")?,
            "apply": true,
        }),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_tile_failure)
}

pub fn render(data: &Value) -> String {
    format!(
        "added {} tiles from {} to project {}\n",
        data["type"].as_str().unwrap_or("?"),
        data["sourceProject"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
    )
}
