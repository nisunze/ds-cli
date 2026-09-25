//! `ds design comment redact` — a moderator clears one comment's text.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::comment::read::THREAD_ARG;
use crate::{LANE_ARG, PROJECT_ARG};

const COMMENT_ARG: Arg = Arg::value(
    "comment",
    "<comment-id>",
    "The comment, from `ds design comment read`.",
)
.required();
const EXPECTED_VERSION_ARG: Arg = Arg::value(
    "expected-version",
    "<n>",
    "The thread version you read the comment at (1 or more), from `read`.",
)
.required();
const REASON_ARG: Arg = Arg::value(
    "reason",
    "<text>",
    "Why the text is removed, for the audit record (up to 2000 characters).",
)
.required();

pub static COMMAND: Command = Command {
    id: "design.comment.redact",
    path: &["design", "comment", "redact"],
    contract: 1,
    summary: "Redact one comment's text as a moderator, irreversibly.",
    purpose: "\
Clears the body of one comment and records who removed it and why; the \
comment keeps its author, its place in the sequence and its time, and `read` \
shows it as redacted. The text is not retained, so this cannot be undone. The \
write is fenced on the thread version you read the comment at: read the thread \
first, check what you are removing, and pass that version, so a thread that \
moved since is refused rather than redacted blind. Needs the moderation \
capability.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        THREAD_ARG,
        COMMENT_ARG,
        EXPECTED_VERSION_ARG,
        REASON_ARG,
        PROJECT_ARG,
        LANE_ARG,
    ],
    output: "The project, the `thread`, the `comment`, its `author` and `sequence`, whether it was `already_redacted`, and the thread's committed `version`.",
    examples: &[Example {
        command: "ds design comment redact --project <id> --thread thread-clearance --comment c-phone-7f3a --expected-version 4 --reason \"personal phone number\" --yes",
        note: "Read the thread first; .data.version there is --expected-version here.",
        runnable: false,
    }],
    refusals: &crate::headless_refusals!(
        crate::INVALID_NUMBER,
        crate::NOT_PERMITTED,
        crate::READ_ONLY,
        crate::CONFLICT,
        crate::CONFIRMATION_REQUIRED,
    ),
    reference: Some("docs/reference/design.md"),
    search: &["moderate comment", "remove comment text"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let expected_version = crate::integer(
        inputs.require("expected-version")?,
        "expected-version",
        1,
        i64::MAX,
    )?;
    crate::headless::perform(
        "design.comment.redact",
        json!({
            "thread": inputs.require("thread")?,
            "comment": inputs.require("comment")?,
            "expected_version": expected_version,
            "reason": inputs.require("reason")?,
        }),
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
    )
}

pub fn render(data: &Value) -> String {
    format!(
        "{} in {} {} · by {} · v{}\n",
        data["comment"].as_str().unwrap_or("?"),
        data["thread"].as_str().unwrap_or("?"),
        if data["already_redacted"] == true {
            "was already redacted"
        } else {
            "redacted"
        },
        data["author"].as_str().unwrap_or("?"),
        data["version"].as_u64().unwrap_or(0),
    )
}
