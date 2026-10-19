//! `ds tile generate` — retired hosted tiling; refuses and names the edge path.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::{FORCE_ARG, LANE_ARG, PROJECT_ARG, TYPE_ARG};

pub static COMMAND: Command = Command {
    id: "tile.generate",
    path: &["tile", "generate"],
    contract: 1,
    summary: "Retired cloud tiling; refuses and names the Core tile queue.",
    purpose: "\
Project tiling is edge compute: the hosted tile worker is retired and this \
command never reaches it. It refuses with native_compute_required before any \
credential is restored, sends nothing and starts nothing. Tiles are built by \
the Core on this machine from a prepared ds.tiles.prepared/v1 input through \
`ds core tile submit`; no edge step yet prepares that input from project \
survey/design data. `ds tile status` and `ds tile plan` still read the \
published state.",
    chapter: Chapter::VectorTiles,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, TYPE_ARG, FORCE_ARG, LANE_ARG],
    output: "None: the command always refuses with native_compute_required.",
    examples: &[Example {
        command: "ds core tile submit --input tiles.prepared.json --key tiles-001",
        note: "The edge path: the Core on this machine builds the tiles from a prepared input.",
        runnable: false,
    }],
    refusals: crate::GENERATE_REFUSALS,
    reference: Some("docs/reference/tile.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let refusal = crate::PROJECT_TILES_NATIVE_COMPUTE_REQUIRED;
    Err(Failure::unavailable(
        refusal.code,
        "project tile generation no longer runs in the cloud; nothing was sent",
    )
    .remedy(refusal.remedy)
    .next("ds core tile submit"))
}

pub fn render(data: &Value) -> String {
    crate::plan::render_decision(data)
}

#[cfg(test)]
#[path = "generate_tests.rs"]
mod tests;
