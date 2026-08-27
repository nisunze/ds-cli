//! `ds map remove` — take a local layer off the paired map.
//!
//! The application only lets this session remove what this session added. A
//! layer an operator drew by hand, and a layer produced by a vector tool, are
//! both refused — so an agent cleaning up after itself cannot erase the
//! operator's work. That boundary belongs to the application; this command
//! documents it as a named refusal rather than discovering it at runtime.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "map.remove",
    path: &["map", "remove"],
    contract: 1,
    summary: "Remove a local layer this session added to the map.",
    purpose: "\
Removes one temporary layer from the running map. Only layers this paired \
session created can be removed: a layer the operator drew, and a layer a \
vector tool produced, are refused by the application, so an agent tidying up \
cannot erase someone else's work. Takes the `layer` id that `ds map view` and \
`ds map draw` report — not the `analysis_id`.",
    chapter: Chapter::Survey,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("layer", "<id>", "The `layer` id from `ds map view`.").required(),
        DESCRIPTOR_ARG,
    ],
    output: "The layer id, that it was removed, and `persisted: false`.",
    examples: &[Example {
        command: "ds map remove --layer sketch-1 --output json",
        note: "The id is `layer`, not `analysis_id`.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "no such layer, or it was not created by this paired session",
            remedy: "run `ds map view`; only layers marked `this_session` can be removed",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let layer = inputs.require("layer")?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let removed = crate::invoke(
        &descriptor,
        &crate::LAYER_REMOVE,
        json!({ "layerId": layer }),
        crate::UI_TIMEOUT,
    )?;

    Ok(json!({
        "layer": layer,
        "removed": removed["removed"].as_bool().unwrap_or(true),
        "persisted": removed["persistedToProject"].as_bool().unwrap_or(false),
    }))
}

pub fn render(data: &Value) -> String {
    if data["removed"].as_bool().unwrap_or(false) {
        format!("removed  {}\n", data["layer"].as_str().unwrap_or(""))
    } else {
        format!("not removed  {}\n", data["layer"].as_str().unwrap_or(""))
    }
}
