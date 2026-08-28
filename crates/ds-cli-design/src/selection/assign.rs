//! `ds design selection assign` — scope one project work task to a selection.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;
use crate::selection::read::SELECTION_ARG;

const TITLE_ARG: Arg = Arg {
    name: "title",
    kind: ArgKind::Value,
    value: "<text>",
    required: true,
    default: None,
    choices: &[],
    summary: "What the work is, e.g. \"Nixon: review these transformers\".",
};

const OWNER_ARG: Arg = Arg {
    name: "owner",
    kind: ArgKind::Value,
    value: "<email>",
    required: false,
    default: None,
    choices: &[],
    summary: "The person responsible. Omit to leave the task unassigned.",
};

const PURPOSE_ARG: Arg = Arg {
    name: "purpose",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Why the work was assigned. Recorded on the receipt.",
};

pub static COMMAND: Command = Command {
    id: "design.selection.assign",
    path: &["design", "selection", "assign"],
    contract: 1,
    summary: "Create one project work task scoped to a saved selection.",
    purpose: "\
Creates an ordinary Project Work task carrying a link to the selection, and \
writes an immutable receipt pinning the selection's version, its member digest \
and the exact transformer ids that resolved at that moment. The task never \
holds a copy of the transformer data, and a later edit of the selection cannot \
change what was assigned. The application re-evaluates membership first and \
refuses if it moved since the read — a promotion assigns a set somebody \
approved, or it does not happen. Members that no longer resolve are reported on \
the receipt rather than silently included.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        SELECTION_ARG,
        TITLE_ARG,
        OWNER_ARG,
        PURPOSE_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
The project, the `selection`, the minted `assignment` and `task` ids, the \
pinned `memberDigest`, the assigned `members`, any `missing` members the \
selection could not resolve, and the `committedRevision` the plan moved to.",
    examples: &[Example {
        command: "ds design selection assign --selection sel-week-32 --title \"Review LV designs\" --owner nixon@example.com --yes",
        note: "Read .data.memberDigest on the receipt to see exactly what was assigned.",
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
    arguments.insert("title".into(), json!(inputs.require("title")?));
    for flag in ["owner", "purpose"] {
        if let Some(value) = inputs.value(flag) {
            arguments.insert(flag.into(), json!(value));
        }
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::SELECTION_ASSIGN,
        Value::Object(arguments),
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let members = data["members"].as_array().map_or(0, Vec::len) as u64;
    let mut out = format!(
        "assigned {} as {} in {} · revision {}\n",
        crate::plural(members, "transformer"),
        data["task"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        data["committedRevision"].as_u64().unwrap_or(0),
    );
    let missing = data["missing"].as_array().map_or(0, Vec::len) as u64;
    if missing > 0 {
        out.push_str(&format!(
            "  ! {} could not be resolved and was not assigned\n",
            crate::plural(missing, "member"),
        ));
    }
    if let Some(digest) = data["memberDigest"].as_str() {
        out.push_str(&format!(
            "  pinned digest {}\n",
            &digest[..digest.len().min(16)]
        ));
    }
    out
}
