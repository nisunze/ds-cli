//! `ds design selection archive` — retire a saved selection, or restore one.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{DesignSelectionArchive, DesignSelectionRequest};
use serde_json::{Value, json};

use super::LANE;
use crate::selection::read::SELECTION_ARG;

const RESTORE_ARG: Arg = Arg {
    name: "restore",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Bring an archived selection back instead of archiving one.",
};

pub static COMMAND: Command = Command {
    id: "design.selection.archive",
    path: &["design", "selection", "archive"],
    contract: 1,
    summary: "Archive a saved selection, or restore one, without losing it.",
    purpose: "\
Archiving hides a selection from the default listing and stops it scoping new \
work. Nothing is erased: its members, its digest and every assignment receipt \
it ever produced stay exactly where they were, and --restore brings it back. \
The application reads the selection's current version and archives under it, so \
a concurrent edit is refused rather than overwritten.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[SELECTION_ARG, RESTORE_ARG, LANE],
    output: "The project, the `selection` id, its new `state`, and the committed `version`.",
    examples: &[Example {
        command: "ds design selection archive --selection sel-week-32 --yes",
        note: "Add --restore to bring it back.",
        runnable: false,
    }],
    refusals: super::REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let selection = inputs.require("selection")?;
    // The version is read, never asserted: archiving under a version `ds` never
    // observed would overwrite a concurrent edit instead of refusing.
    let expected_version = super::read_selection(lane, selection)?.1.selection.version;
    let (project, answer) = super::ask(
        lane,
        selection,
        &DesignSelectionRequest::Archive(DesignSelectionArchive {
            selection_id: selection.to_owned(),
            expected_version,
            restore: inputs.switch("restore"),
        }),
    )?;
    let head = super::saved_head(answer)?;
    Ok(json!({
        "project": project,
        "selection": head.selection_id,
        "state": head.state,
        "version": head.version,
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "{} is now {} in {} · v{}\n",
        data["selection"].as_str().unwrap_or("?"),
        data["state"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        data["version"].as_u64().unwrap_or(0),
    )
}
