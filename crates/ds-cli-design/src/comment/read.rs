//! `ds design comment read` — one thread and everything said in it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::LANE_ARG;

pub const THREAD_ARG: Arg = Arg {
    name: "thread",
    kind: ArgKind::Value,
    value: "<thread-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The thread, from `ds design comment list`.",
};

pub static COMMAND: Command = Command {
    id: "design.comment.read",
    path: &["design", "comment", "read"],
    contract: 1,
    summary: "Read one comment thread in sequence, redactions included.",
    purpose: "\
Returns the thread and its comments in the order they were written, each with \
its author and the project role that author held AT THE TIME — a historical \
fact, so a later role change does not rewrite it. A redacted comment keeps its \
author, its place in the sequence and its time, and is reported as redacted \
with a null body rather than shown as an empty message.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[THREAD_ARG, LANE_ARG],
    output: "\
The project, the `thread`, its `title`, `state`, `version` and linked `task`, \
whether there is `more` than one page of comments, and rows of `comment`, \
`sequence`, `author`, `roles`, `redacted`, `body` and `at`.",
    examples: &[Example {
        command: "ds design comment read --thread thread-clearance --output json",
        note: "Read .data.version before resolving or promoting; both are version-checked.",
        runnable: false,
    }],
    refusals: &crate::headless_refusals!(crate::NOT_PERMITTED,),
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    crate::headless::perform(
        "design.comment.read",
        json!({ "thread": inputs.require("thread")? }),
        inputs.value("lane").unwrap_or("stable"),
    )
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} · {} · v{}{}\n",
        data["title"].as_str().unwrap_or("?"),
        data["state"].as_str().unwrap_or("?"),
        data["version"].as_u64().unwrap_or(0),
        data["task"]
            .as_str()
            .map(|task| format!(" · task {task}"))
            .unwrap_or_default(),
    );
    for comment in data["comments"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  #{} {} — {}\n",
            comment["sequence"].as_u64().unwrap_or(0),
            comment["author"].as_str().unwrap_or("?"),
            if comment["redacted"].as_bool() == Some(true) {
                "[redacted by a moderator]"
            } else {
                comment["body"].as_str().unwrap_or("")
            },
        ));
    }
    if data["more"].as_bool() == Some(true) {
        out.push_str("  … more comments exist than are shown\n");
    }
    out
}
