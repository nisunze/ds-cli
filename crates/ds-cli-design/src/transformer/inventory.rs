//! `ds design transformer inventory` — inspect lifecycle state; the plan
//! step before a retirement or restoration.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use super::{LANE_ARG, TRANSFORMER_ARG};

pub static COMMAND: Command = Command {
    id: "design.transformer.inventory",
    path: &["design", "transformer", "inventory"],
    contract: 1,
    summary: "Inspect which transformers are active, retired, or deleted.",
    purpose: "\
Start here before retiring or restoring. Restores the native user and reads \
only its audience-fenced selected project through the fixed inventory call. \
Without --transformer it lists every transformer document with its lifecycle \
state; with names it answers exactly those names, so the receipt is the plan: \
`active` can be retired, `retired` can be restored, `deleted` has no retirement \
record and `missing` has no document. No project, Desktop descriptor, URL, \
body or action override is accepted.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, LANE_ARG],
    output: "\
Lane and selected-project identity/status, the requested names, active/retired/\
deleted counts, and one row per transformer with `kind`, `state`, and the \
retirement record (reason, who, when, restoration) when one exists.",
    examples: &[Example {
        command: "ds design transformer inventory --transformer TX-1 --transformer TX-2 --output json",
        note: "`.data.transformers[].state` says what each name is today.",
        runnable: false,
    }],
    refusals: super::NATIVE_READ_REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = super::transformer_set(inputs, false)?;
    let headless = ds_cli_auth::transformer_inventory(inputs.require("lane")?, &requested)?;
    let mut output = super::project_receipt(&headless);
    let inventory = super::inventory_json(headless.result());
    output
        .as_object_mut()
        .expect("receipt is an object")
        .extend(
            inventory
                .as_object()
                .expect("inventory is an object")
                .clone(),
        );
    Ok(output)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} ({}) · {} · {} active · {} retired · {} deleted\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["active_count"].as_u64().unwrap_or(0),
        data["retired_count"].as_u64().unwrap_or(0),
        data["deleted_count"].as_u64().unwrap_or(0),
    );
    if let Some(rows) = data["transformers"].as_array() {
        for row in rows {
            let mut line = format!(
                "  {:<32} {:<8} {}",
                row["name"].as_str().unwrap_or("?"),
                row["state"].as_str().unwrap_or("?"),
                row["kind"].as_str().unwrap_or(""),
            );
            if let Some(reason) = row["retirement"]["reason"].as_str() {
                line.push_str(&format!(" · {reason}"));
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }
    out
}
