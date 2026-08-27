//! `ds tile generate` — start a run. Same operation as `plan` with
//! `apply: true`; the confirmation is the CLI's `--yes`.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::{DESCRIPTOR_ARG, FORCE_ARG, TYPE_ARG};

pub static COMMAND: Command = Command {
    id: "tile.generate",
    path: &["tile", "generate"],
    contract: 1,
    summary: "Regenerate one output's vector tiles (needs --yes).",
    purpose: "\
Dispatches the run `ds tile plan` describes through the application's own \
pipeline client: the same staleness rule, the same preflight, the same \
governed request. Returns as soon as ds-brain accepts the job; follow it \
with `ds tile status`. Use --force after a restyle or a Data-cleaning \
catalog change — the output is not dirty then, but the tiles must be \
rebuilt for the legend to reflect the project's vocabulary.",
    chapter: Chapter::VectorTiles,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TYPE_ARG, FORCE_ARG, DESCRIPTOR_ARG],
    output: "\
The plan receipt with `dispatched` and ds-brain's `result` (`status` \
started|current, `message`).",
    examples: &[Example {
        command: "ds tile generate --type design --force --yes",
        note: "Runs take minutes; `ds tile status --type design` reports progress.",
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
    let arguments = crate::plan::arguments(inputs, true)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TILE_GENERATE,
        arguments,
        crate::RUN_TIMEOUT,
    )
    .map_err(crate::classify_tile_failure)
}

pub fn render(data: &Value) -> String {
    crate::plan::render_decision(data)
}
