//! `ds design selection archive` — retire a saved selection, or restore one.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;
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
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[SELECTION_ARG, RESTORE_ARG, DESCRIPTOR_ARG],
    output: "The project, the `selection` id, its new `state`, and the committed `version`.",
    examples: &[Example {
        command: "ds design selection archive --selection sel-week-32 --yes",
        note: "Add --restore to bring it back.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::NOT_PERMITTED,
        crate::READ_ONLY,
        crate::CONFLICT,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("selection".into(), json!(inputs.require("selection")?));
    if inputs.switch("restore") {
        arguments.insert("restore".into(), json!(true));
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::SELECTION_ARCHIVE,
        Value::Object(arguments),
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
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
