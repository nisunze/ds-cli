//! `ds tile list` — the project's tile catalogue.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::LANE_ARG;

const GLOBAL_ARG: Arg = Arg {
    name: "global",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Include the platform's global reference tiles.",
};
const REFRESH_ARG: Arg = Arg {
    name: "refresh",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Re-read from ds-brain; native catalogue reads are always fresh.",
};

pub static COMMAND: Command = Command {
    id: "tile.list",
    path: &["tile", "list"],
    contract: 2,
    summary: "The tile archives this project renders, own and added.",
    purpose: "\
One row per tile archive the project's map can mount: its own outputs, \
outputs added from other projects, and (with --global) the platform's \
reference tiles. This is the catalogue `ds tile remove` takes ids from.",
    chapter: Chapter::VectorTiles,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        GLOBAL_ARG,
        REFRESH_ARG,
        Arg::value(
            "limit",
            "<n>",
            "Return at most 1..500 archives; more reports truncation.",
        )
        .default("100"),
        LANE_ARG,
    ],
    output: "\
`project`, `total` and `tiles` rows with `id`, `name`, `tile_type`, `scope`, \
`source_project_id`, `total_features`, `tiled_at`.",
    examples: &[Example {
        command: "ds tile list --global --output json",
        note: "A row whose `source_project_id` is another project was added with `ds tile add`.",
        runnable: false,
    }],
    refusals: crate::NATIVE_LIST_REFUSALS,
    reference: Some("docs/reference/tile.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let result = ds_cli_auth::tile_list(inputs.require("lane")?, inputs.switch("global"))?;
    let limit = ds_cli_desktop::ops::integer(inputs.require("limit")?, "limit", 1, 500)? as usize;
    let rows = &result.result().tiles;
    Ok(
        json!({"lane": result.lane(), "project": result.project_id(), "total": rows.len(),
        "tiles": rows.iter().take(limit).collect::<Vec<_>>(), "more": rows.len() > limit}),
    )
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} for project {}\n",
        crate::plural(data["total"].as_u64().unwrap_or(0), "tile archive"),
        data["project"].as_str().unwrap_or("?"),
    );
    for row in data["tiles"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<28} {:<7} {:<8} {}\n",
            crate::truncate(row["id"].as_str().unwrap_or("?"), 28),
            row["tile_type"].as_str().unwrap_or("?"),
            row["scope"].as_str().unwrap_or("project"),
            row["source_project_id"]
                .as_str()
                .map(|source| format!("from {source}"))
                .unwrap_or_default(),
        ));
    }
    out
}
