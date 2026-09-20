//! `ds design comment promote` — turn a thread into project work.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::LANE_ARG;
use crate::comment::read::THREAD_ARG;

const TITLE_ARG: Arg = Arg {
    name: "title",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "The task's title. Defaults to the thread's own title.",
};

pub static COMMAND: Command = Command {
    id: "design.comment.promote",
    path: &["design", "comment", "promote"],
    contract: 1,
    summary: "Create one project work task linked to a comment thread.",
    purpose: "\
Creates an ordinary Project Work task carrying links back to the thread and to \
the design object it sits on. The thread is NOT copied: the discussion keeps \
one home, and the task points at it. A thread already linked to a task is \
refused rather than linked twice, so a conversation cannot end up forked across \
two work items.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[THREAD_ARG, TITLE_ARG, LANE_ARG],
    output: "The project, the `thread`, the linked `task` id and the thread's committed `version`.",
    examples: &[Example {
        command: "ds design comment promote --thread thread-clearance --yes",
        note: "Pass --title to give the task a different name from the thread.",
        runnable: false,
    }],
    refusals: &crate::headless_refusals!(
        crate::NOT_PERMITTED,
        crate::READ_ONLY,
        crate::CONFLICT,
        crate::CONFIRMATION_REQUIRED,
    ),
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("thread".into(), json!(inputs.require("thread")?));
    if let Some(title) = inputs.value("title") {
        arguments.insert("title".into(), json!(title));
    }
    crate::headless::perform(
        "design.comment.promote",
        Value::Object(arguments),
        inputs.value("lane").unwrap_or("stable"),
    )
}

pub fn render(data: &Value) -> String {
    format!(
        "{} is linked to {} · v{}\n",
        data["thread"].as_str().unwrap_or("?"),
        data["task"].as_str().unwrap_or("none"),
        data["version"].as_u64().unwrap_or(0),
    )
}
