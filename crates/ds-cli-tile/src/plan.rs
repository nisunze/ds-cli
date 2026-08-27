//! `ds tile plan` — the decision `ds tile generate` would make, and nothing
//! else. Same operation with `apply: false`.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, FORCE_ARG, TYPE_ARG};

pub static COMMAND: Command = Command {
    id: "tile.plan",
    path: &["tile", "plan"],
    contract: 1,
    summary: "Decide whether a run is needed and preflight it, without running.",
    purpose: "\
Applies the Pipeline panel's own rule — never built, sources changed \
(dirty), or --force → a run; current and clean → no run — and, when a run \
would start, performs the preflight so the decision is complete. Nothing is \
dispatched. `ds tile generate --yes` does exactly what this reports.",
    chapter: Chapter::VectorTiles,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TYPE_ARG, FORCE_ARG, DESCRIPTOR_ARG],
    output: "\
`project`, `type`, `force`, `dispatched: false`, `wouldDispatch`, `reason`, \
and `preflight` (null when no run is needed).",
    examples: &[Example {
        command: "ds tile plan --type design --force --output json",
        note: "After a restyle or a vocabulary change the output is not dirty; --force is the honest flag.",
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

pub fn arguments(inputs: &Inputs, apply: bool) -> Result<Value, Failure> {
    Ok(json!({
        "type": inputs.require("type")?,
        "force": inputs.switch("force"),
        "apply": apply,
    }))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = arguments(inputs, false)?;
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
    render_decision(data)
}

pub fn render_decision(data: &Value) -> String {
    let kind = data["type"].as_str().unwrap_or("?");
    let verdict = match (
        data["dispatched"].as_bool().unwrap_or(false),
        data["wouldDispatch"].as_bool().unwrap_or(false),
    ) {
        (true, _) => "run started",
        (false, true) => "a run is needed (not started)",
        (false, false) => "no run needed",
    };
    let mut out = format!("{kind} tiles · {verdict}\n");
    if let Some(reason) = data["reason"].as_str()
        && !reason.is_empty()
    {
        out.push_str(&format!("  {}\n", crate::truncate(reason, 110)));
    }
    if !data["preflight"].is_null() {
        out.push_str(&format!(
            "  preflight: {}\n",
            crate::preflight_line(&data["preflight"])
        ));
    }
    if let Some(message) = data["result"]["message"].as_str()
        && !message.is_empty()
    {
        out.push_str(&format!("  {}\n", crate::truncate(message, 110)));
    }
    out
}
