//! `ds pm task unblock` — clear a correspondence blocker by hand.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::{LANE_ARG, TASK_ARG};

const RECORD_ARG: Arg = Arg::value(
    "record",
    "<record-id>",
    "The record the task is blocked on.",
)
.required();
const REASON_ARG: Arg = Arg::value(
    "reason",
    "<text>",
    "Why the task no longer waits on the record (at most 300 characters); kept on the blocker.",
)
.required();

pub static COMMAND: Command = Command {
    id: "pm.task.unblock",
    path: &["pm", "task", "unblock"],
    contract: 1,
    summary: "Clear a task's blocker on a record by hand, with a reason.",
    purpose: "\
A correspondence blocker normally clears itself when the record is answered \
or waived. This clears it by hand — the answer came another way, the task \
no longer depends on it — and keeps the reason on the blocker as \
`cleared_by: manual`. The record's own response status is untouched. \
Headless: commits to the named project of the signed-in native \
credential, no window.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TASK_ARG,
        RECORD_ARG,
        REASON_ARG,
        LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "\
The project, `taskId`, `recordId`, the `committedRevision` the plan moved \
to, and the engine's `result`.",
    examples: &[Example {
        command: "ds pm task unblock --task T-0012 --record R-0031 --reason \"agreed by phone on 22 Sep\" --yes --project <exact-id>",
        note: "To settle the record itself, file the reply or waive it instead.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<25>(&[
        crate::TASK_NOT_FOUND,
        crate::RECORD_NOT_FOUND,
        crate::BLOCKER_NOT_FOUND,
        crate::BOUND_EXCEEDED,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "correspondence",
        "blocked on",
        "clear blocker",
        "release",
        "waiting on",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    super::block::blocker(inputs, true)
}

pub fn render(data: &Value) -> String {
    format!(
        "unblocked {} from {} in {} · revision {}\n",
        data["taskId"].as_str().unwrap_or("?"),
        data["recordId"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        data["committedRevision"].as_u64().unwrap_or(0),
    )
}
