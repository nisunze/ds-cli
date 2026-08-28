//! `ds design comment resolve` — close a thread, or reopen one.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;
use crate::comment::read::THREAD_ARG;

const REOPEN_ARG: Arg = Arg {
    name: "reopen",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Reopen a resolved thread instead of resolving one.",
};

pub static COMMAND: Command = Command {
    id: "design.comment.resolve",
    path: &["design", "comment", "resolve"],
    contract: 1,
    summary: "Resolve a comment thread, or reopen a resolved one.",
    purpose: "\
Marks the thread resolved so it drops out of the default listing, or reopens \
it. Resolution history survives a reopen: the thread still records that it was \
resolved once, by whom and when. The application reads the thread's current \
version and writes under it, so a comment posted between the read and the \
resolve refuses the resolve rather than closing a conversation that just moved.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[THREAD_ARG, REOPEN_ARG, DESCRIPTOR_ARG],
    output: "The project, the `thread`, its new `state` and the committed `version`.",
    examples: &[Example {
        command: "ds design comment resolve --thread thread-clearance --yes",
        note: "Add --reopen to bring a resolved thread back.",
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
    arguments.insert("thread".into(), json!(inputs.require("thread")?));
    if inputs.switch("reopen") {
        arguments.insert("reopen".into(), json!(true));
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::COMMENT_RESOLVE,
        Value::Object(arguments),
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    format!(
        "{} is now {} · v{}\n",
        data["thread"].as_str().unwrap_or("?"),
        data["state"].as_str().unwrap_or("?"),
        data["version"].as_u64().unwrap_or(0),
    )
}
