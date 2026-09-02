//! `ds tile status` — the published state of the project's tile outputs.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value};

use crate::{LANE_ARG, OPTIONAL_TYPE_ARG};

pub static COMMAND: Command = Command {
    id: "tile.status",
    path: &["tile", "status"],
    contract: 1,
    summary: "The published state of the survey and design tile outputs.",
    purpose: "\
Start here. Restores the native user and reads only its audience-fenced \
selected project through the fixed tile status call. For each requested \
output it reports whether it is published, running, failed or never built; \
when it was tiled; how many features it holds; and whether its sources \
changed since. No project, Desktop descriptor, URL, body or action override \
is accepted.",
    chapter: Chapter::VectorTiles,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[OPTIONAL_TYPE_ARG, LANE_ARG],
    output: "\
Lane and selected-project identity/status, plus `tiles` keyed by type with \
the fixed backend status, timestamps, feature count, dirty/progress/cache \
state, decision and bounded diagnostics.",
    examples: &[Example {
        command: "ds tile status --output json",
        note: "`.data.tiles.design.dirty` true means a generate would run.",
        runnable: false,
    }],
    refusals: crate::NATIVE_REFUSALS,
    reference: Some("docs/reference/tile.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let types: &[ds_cli_auth::TileType] = match inputs.value("type") {
        Some(kind) => std::slice::from_ref(match kind {
            "survey" => &ds_cli_auth::TileType::Survey,
            "design" => &ds_cli_auth::TileType::Design,
            _ => unreachable!("the command parser enforces tile type choices"),
        }),
        None => &[ds_cli_auth::TileType::Survey, ds_cli_auth::TileType::Design],
    };
    let mut project = None;
    let mut tiles = Map::new();
    for kind in types {
        let headless = ds_cli_auth::tile_status(lane, *kind)?;
        let current = crate::operation_project(&headless);
        if let Some(expected) = &project {
            crate::require_same_project(expected, &current)?;
        } else {
            project = Some(current);
        }
        tiles.insert(
            kind.token().to_owned(),
            crate::operation_json(headless.result()),
        );
    }
    let mut output = project.expect("status always requests at least one type");
    output["tiles"] = Value::Object(tiles);
    Ok(output)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} ({}) · {}\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
    );
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
