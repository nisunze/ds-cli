//! `ds design comment post` — open a thread, or append to one.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, KIND_ARG, OBJECT_ARG, VERSION_ARG};

const THREAD_ARG: Arg = Arg {
    name: "thread",
    kind: ArgKind::Value,
    value: "<thread-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "Append to this thread. Omit to open a new one on the object.",
};

const TITLE_ARG: Arg = Arg {
    name: "title",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "What a NEW thread is about. Required unless --thread is given.",
};

const BODY_ARG: Arg = Arg {
    name: "body",
    kind: ArgKind::Value,
    value: "<text>",
    required: true,
    default: None,
    choices: &[],
    summary: "What to say. A thread is never opened empty.",
};

pub static COMMAND: Command = Command {
    id: "design.comment.post",
    path: &["design", "comment", "post"],
    contract: 1,
    summary: "Open a comment thread on a design object, or append to one.",
    purpose: "\
With --thread this appends one comment. Without it, this opens a new thread on \
the object with the given title and this comment as its first — a thread is \
never opened empty, because an empty thread is noise on the object. Appending \
is deliberately not version-checked: two people commenting at the same moment \
both belong in the record, and refusing one of them would lose a real remark. \
The author and their project role at the time are recorded server-side from the \
signed-in session; `ds` cannot claim to be somebody else.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        BODY_ARG,
        THREAD_ARG,
        KIND_ARG,
        OBJECT_ARG,
        TITLE_ARG,
        VERSION_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The project, the `thread`, its `title` when newly opened, the resulting `comments` count and the thread's committed `version`.",
    examples: &[Example {
        command: "ds design comment post --kind mv_model --object mv_line_a --title \"Clearance at span 4\" --body \"Looks short against the road.\" --yes",
        note: "Pass --thread instead of --kind/--object/--title to append to an existing thread.",
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
        crate::INVALID_ANCHOR,
        MISSING_TARGET,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

const MISSING_TARGET: ds_cli_contract::spec::Refusal = ds_cli_contract::spec::Refusal {
    code: "missing_comment_target",
    when: "neither --thread nor a complete --kind/--object/--title was given",
    remedy: "append with --thread <id>, or open one with --kind, --object and --title",
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("body".into(), json!(inputs.require("body")?));
    match inputs.value("thread") {
        Some(thread) => {
            arguments.insert("thread".into(), json!(thread));
        }
        None => {
            // Opening a thread needs the whole anchor AND a title. Sending a
            // partial one would reach the application only to be refused
            // there, with a message about a key rather than about the choice.
            let (Some(kind), Some(object), Some(title)) = (
                inputs.value("kind"),
                inputs.value("object"),
                inputs.value("title"),
            ) else {
                return Err(Failure::invalid(
                    "missing_comment_target",
                    "opening a thread needs --kind, --object and --title; appending needs --thread",
                )
                .remedy(MISSING_TARGET.remedy)
                .next("ds design comment post --help"));
            };
            arguments.insert("kind".into(), json!(kind));
            arguments.insert("object".into(), json!(object));
            arguments.insert("title".into(), json!(title));
            if let Some(version) = inputs.value("version") {
                arguments.insert("version".into(), json!(version));
            }
        }
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::COMMENT_POST,
        Value::Object(arguments),
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    format!(
        "{} now has {} · v{}\n",
        data["thread"].as_str().unwrap_or("?"),
        crate::plural(data["comments"].as_u64().unwrap_or(0), "comment"),
        data["version"].as_u64().unwrap_or(0),
    )
}
