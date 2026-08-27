//! `ds tile list` — the project's tile catalogue.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

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
    summary: "Re-read from ds-brain instead of the application's session cache.",
};

pub static COMMAND: Command = Command {
    id: "tile.list",
    path: &["tile", "list"],
    contract: 1,
    summary: "The tile archives this project renders, own and added.",
    purpose: "\
One row per tile archive the project's map can mount: its own outputs, \
outputs added from other projects, and (with --global) the platform's \
reference tiles. This is the catalogue `ds tile remove` takes ids from.",
    chapter: Chapter::VectorTiles,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[GLOBAL_ARG, REFRESH_ARG, DESCRIPTOR_ARG],
    output: "\
`project`, `total` and `tiles` rows with `id`, `name`, `tile_type`, `scope`, \
`source_project_id`, `total_features`, `tiled_at`.",
    examples: &[Example {
        command: "ds tile list --global --output json",
        note: "A row whose `source_project_id` is another project was added with `ds tile add`.",
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
    crate::invoke(
        &descriptor,
        &crate::TILE_LIST,
        json!({ "global": inputs.switch("global"), "refresh": inputs.switch("refresh") }),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_tile_failure)
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
