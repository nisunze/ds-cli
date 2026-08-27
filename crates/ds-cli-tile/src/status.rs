//! `ds tile status` — the published state of the project's tile outputs.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, OPTIONAL_TYPE_ARG};

pub static COMMAND: Command = Command {
    id: "tile.status",
    path: &["tile", "status"],
    contract: 1,
    summary: "The published state of the survey and design tile outputs.",
    purpose: "\
Start here. For each output (survey, design): whether it is published, \
running, failed or never built; when it was tiled; how many features it \
holds; and whether the project's sources changed since (dirty). Reads the \
same status the Pipeline panel shows.",
    chapter: Chapter::VectorTiles,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[OPTIONAL_TYPE_ARG, DESCRIPTOR_ARG],
    output: "\
`project` and `tiles` keyed by type with `status`, `tiled_at`, \
`total_features`, `dirty`, `in_progress`, `cache_available`, `last_error`.",
    examples: &[Example {
        command: "ds tile status --output json",
        note: "`.data.tiles.design.dirty` true means a generate would run.",
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
    ],
    reference: Some("docs/reference/tile.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let mut arguments = serde_json::Map::new();
    if let Some(kind) = inputs.value("type") {
        arguments.insert("type".into(), json!(kind));
    }
    crate::invoke(
        &descriptor,
        &crate::TILE_STATUS,
        Value::Object(arguments),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_tile_failure)
}

pub fn render(data: &Value) -> String {
    let mut out = format!("project {}\n", data["project"].as_str().unwrap_or("?"));
    if let Some(tiles) = data["tiles"].as_object() {
        for (kind, status) in tiles {
            if status.is_null() {
                out.push_str(&format!("  {kind:<7} never built\n"));
            } else {
                out.push_str(&format!("  {kind:<7} {}\n", crate::status_line(status)));
            }
        }
    }
    out
}
