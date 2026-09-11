//! `ds design collisions` — how many collisions this project has, headlessly.
//!
//! A collision means two or more transformers claim overlapping ground or a
//! clashing identity, and a combined report cannot be produced while one
//! stands. Until now no `ds` command modelled one — the crate's own test said
//! so, and asserted the word "collision" was ABSENT from a refusal, which
//! pinned the gap in place. An operator whose compounded run was going to fail
//! had no way to be told why without opening the application.
//!
//! Detection itself is a governed report action the cloud reporter serves from
//! the project's own transformer data; this command READS the answer it wrote.
//! The count's five-candidate precedence and the states an operator reads are
//! `ds_command_kernel::collisions`', so the card and this command report the
//! same number under the same words.

use ds_cli_auth::TransformerSet;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::transformer::LANE_ARG;

/// The reserved project-wide row the reporter writes collisions into. It is
/// never an LV transformer, which is why it is named here rather than taken.
const COLLISIONS_ROW: &str = "collisions";

pub static COMMAND: Command = Command {
    id: "design.collisions",
    path: &["design", "collisions"],
    contract: 1,
    summary: "Read this project's collision count and detection state.",
    purpose: "\
Reads the project-wide collisions document the report owner writes. The count \
is taken in a fixed precedence, because a run that finds NOTHING writes no \
layer: a project with no collisions reports zero, not unknown, and a malformed \
count stays unknown rather than becoming a number the owner never wrote. \
Detection is a separate governed action; this starts nothing and writes \
nothing. The reference names the precedence.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[LANE_ARG],
    output: "\
Lane and selected-project identity, `checked`, `pairs` (null when unknown), the \
`state` key an operator reads it under, and how many ordinary transformers a \
detection run would cover.",
    examples: &[
        Example {
            command: "ds design collisions --output json",
            note: "`.data.pairs` is null when the project has never been checked.",
            runnable: false,
        },
        Example {
            command: "ds design collisions",
            note: "Read before `ds report project compounded`: a collision blocks it.",
            runnable: false,
        },
    ],
    refusals: super::transformer::status::REFUSALS_READ,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = TransformerSet::new(std::iter::empty::<String>())
        .map_err(|error| Failure::invalid("invalid_transformer_scope", error.to_string()))?;
    let headless = ds_cli_auth::transformer_status(inputs.require("lane")?, &requested)?;
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

/// The key an operator reads the answer under. Keys, never prose: the words
/// belong to whichever surface is rendering them.
fn state_key(checked: bool, pairs: Option<u64>) -> &'static str {
    match (checked, pairs) {
        (false, _) => "pctl_collisions_never_checked",
        (true, None) => "pctl_collisions_unknown",
        (true, Some(0)) => "pctl_collisions_pairs_none",
        (true, Some(1)) => "pctl_collisions_pairs_one",
        (true, Some(_)) => "pctl_collisions_pairs_many",
    }
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
        "project {} ({}) · {} · {} pairs · {} transformers in scope · {}\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        pairs,
        data["transformers"].as_u64().unwrap_or(0),
        data["state"].as_str().unwrap_or("-"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_key_separates_never_checked_from_unknown_and_from_zero() {
        assert_eq!(state_key(false, None), "pctl_collisions_never_checked");
        assert_eq!(state_key(true, None), "pctl_collisions_unknown");
        assert_eq!(state_key(true, Some(0)), "pctl_collisions_pairs_none");
        assert_eq!(state_key(true, Some(1)), "pctl_collisions_pairs_one");
        assert_eq!(state_key(true, Some(7)), "pctl_collisions_pairs_many");
    }

    #[test]
    fn the_project_wide_documents_are_not_transformers_in_scope() {
        for name in ["collisions", "combined_transformer", "mv_data"] {
            assert!(is_reserved(name));
        }
        assert!(!is_reserved("TX-1"));
    }

    /// The precedence is the kernel's; this pins that the command reads the
    /// document through it rather than reaching for one member.
    #[test]
    fn a_zero_result_document_reports_zero_not_unknown() {
        let row = json!({
            "layers": {},
            "report_metadata": {"outcomes": {"collision_detection": {"details": {"pairs": 0}}}}
        });
        assert_eq!(
            ds_command_kernel::collisions::project_count(&row, &Value::Null),
            Some(0)
        );
    }
}
