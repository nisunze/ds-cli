//! `ds tile remove` — drop one archive from the project's catalogue.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

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
    contract: 1,
    summary: "Remove one tile archive from the catalogue (needs --yes).",
    purpose: "\
Removes one archive — an added reference or the project's own output — \
through the same governed action the Pipeline panel uses. ds-brain \
reclaims the storage of an owned output; an added reference is only \
unlinked.",
    chapter: Chapter::VectorTiles,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TILE_ID_ARG, SCOPE_ARG, DESCRIPTOR_ARG],
    output: "`project`, `tileId`, `scope`, `removed: true`.",
    examples: &[Example {
        command: "ds tile remove --tile-id neighbouring-district_design --yes",
        note: "Read the id from `ds tile list` first.",
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
        &crate::TILE_REMOVE,
        json!({
            "tile_id": inputs.require("tile-id")?,
            "scope": inputs.value("scope").unwrap_or("project"),
            "apply": true,
        }),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_tile_failure)
}

pub fn render(data: &Value) -> String {
    format!(
        "removed {} ({}) from project {}\n",
        data["tileId"].as_str().unwrap_or("?"),
        data["scope"].as_str().unwrap_or("project"),
        data["project"].as_str().unwrap_or("?"),
    )
}
