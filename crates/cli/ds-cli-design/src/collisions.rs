//! `ds design collisions` — how many collisions this project has, headlessly.
//!
//! A collision means two or more transformers claim overlapping ground or a
//! clashing identity, and a combined report cannot be produced while one
//! stands. Until now no `ds` command modelled one — the crate's own test said
//! so, and asserted the word "collision" was ABSENT from a refusal, which
//! pinned the gap in place. An operator whose combined run was going to fail
//! had no way to be told why without opening the application.
//!
//! Detection runs on this machine (`ds design collisions detect`) and is stored
//! by `ds design collisions publish`; this command READS the stored answer.
//! The count's five-candidate precedence and the states an operator reads are
//! `ds_command_kernel::collisions`', so the card and this command report the
//! same number under the same words.

/// The key an operator reads the answer under is the kernel's fold, not a
/// second `match` here: this command and the card name one condition one way.
use ds_command_kernel::collisions::state_key;

use ds_cli_auth::TransformerSet;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::transformer::LANE_ARG;

/// The reserved project-wide row the reporter writes collisions into. It is
/// never an LV transformer, which is why it is named here rather than taken.
const COLLISIONS_ROW: &str = "collisions";

pub static COMMAND: Command = Command {
    id: "design.collisions",
    path: &["design", "collisions"],
    contract: 3,
    summary: "Read the project collision count and detection state.",
    purpose: "\
Reads the project-wide collisions document a published detection stored. The count \
is taken in a fixed precedence, because a run that finds NOTHING writes no \
layer: a project with no collisions reports zero, not unknown, and a malformed \
count stays unknown rather than becoming a number the owner never wrote. \
Detection is `ds design collisions detect`; this starts nothing and writes \
nothing. The reference names the precedence.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[crate::PROJECT_ARG, LANE_ARG],
    output: "\
Lane and project identity, `checked`, `pairs` (null when unknown), the \
`state` key an operator reads it under, and how many ordinary transformers a \
detection run would cover.",
    examples: &[Example {
        command: "ds design collisions --project demo",
        note: "Read before `ds report project compounded`: a collision blocks it.",
        runnable: false,
    }],
    refusals: &[
        super::transformer::NATIVE_PROFILE,
        super::transformer::NATIVE_PROFILE_DIGEST,
        super::transformer::NATIVE_PROFILE_UNSAFE,
        super::transformer::HEADLESS_SIGNED_OUT,
        super::transformer::PROJECT_REQUIRED,
        super::transformer::CONTEXT_CORRUPT,
    ],
    reference: Some("docs/reference/design.md"),
    search: &["overlap", "collision count", "duplicate transformers"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = TransformerSet::new(std::iter::empty::<String>())
        .map_err(|error| Failure::invalid("invalid_transformer_scope", error.to_string()))?;
    let headless = ds_cli_auth::transformer_status(
        inputs.require("lane")?,
        inputs.require("project")?,
        &requested,
    )?;
    let mut output = super::transformer::project_receipt(&headless);
    let rows = headless.result().rows();
    let document = rows
        .iter()
        .find(|row| row.name() == COLLISIONS_ROW)
        .map(|row| row.row().clone());
    // Every ordinary transformer is in scope for a detection run; the reserved
    // project-wide rows are not transformers and are not checked.
    let checked = rows.iter().filter(|row| !is_reserved(row.name())).count();
    let pairs = document
        .as_ref()
        .and_then(|row| ds_command_kernel::collisions::project_count(row, &Value::Null));
    let object = output.as_object_mut().expect("receipt is an object");
    object.insert("checked".into(), json!(document.is_some()));
    object.insert("pairs".into(), json!(pairs));
    object.insert("state".into(), json!(state_key(document.is_some(), pairs)));
    object.insert("transformers".into(), json!(checked));
    Ok(output)
}

/// The project-wide rows that are documents rather than transformers.
fn is_reserved(name: &str) -> bool {
    matches!(name, COLLISIONS_ROW | "combined_transformer" | "mv_data")
}

pub fn render(data: &Value) -> String {
    let pairs = data["pairs"]
        .as_u64()
        .map(|count| count.to_string())
        .unwrap_or_else(|| "unknown".into());
    format!(
        "project {} · {} · {} pairs · {} transformers in scope · {}\n",
        super::transformer::project_label(data),
        data["lane"].as_str().unwrap_or("?"),
        pairs,
        data["transformers"].as_u64().unwrap_or(0),
        data["state"].as_str().unwrap_or("-"),
    )
}

#[cfg(test)]
#[path = "collisions_tests.rs"]
mod tests;
