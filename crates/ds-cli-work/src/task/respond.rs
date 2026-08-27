//! `ds work task respond` — answer an assignment request.
//!
//! The actor is the application's signed-in user and cannot be named as a
//! flag: accepting on someone else's behalf is exactly what this command must
//! not be able to do. Which is also why it is reachable at all for a
//! contributor who may not otherwise edit the schedule — answering a request
//! that names you is not editing the plan.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, TASK_ARG};

const RESPONSE_ARG: Arg = Arg {
    name: "response",
    kind: ArgKind::Value,
    value: "<answer>",
    required: true,
    default: None,
    choices: &["accept", "decline"],
    summary: "Take the work, or remove yourself from the request.",
};

pub static COMMAND: Command = Command {
    id: "work.task.respond",
    path: &["work", "task", "respond"],
    contract: 1,
    summary: "Accept or decline an assignment request naming you.",
    purpose: "\
Answers as the application's signed-in user — there is no flag for who is \
answering, because answering for somebody else is the one thing this must not \
allow. Accepting makes you responsible and closes the request for everyone \
else; declining removes only you and leaves it open, and carries no reason, \
because a justification is how declining stops being real.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TASK_ARG, RESPONSE_ARG, DESCRIPTOR_ARG],
    output: "\
The project, the `taskId`, the `response` applied, who is `responsible` \
afterwards, who is still being `requested`, and the `committedRevision`.",
    examples: &[Example {
        command: "ds work task respond --task T-0007 --response accept --yes",
        note: "Refused with desktop_refused when the request was withdrawn or somebody accepted first.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::WORK_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::CONFLICT,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/work.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TASK_RESPOND,
        json!({
            "task": inputs.require("task")?,
            "response": inputs.require("response")?,
        }),
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_work_failure)
}

pub fn render(data: &Value) -> String {
    let requested: Vec<&str> = data["requested"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut out = format!(
        "{}ed {} · revision {}\n  responsible: {}\n",
        data["response"].as_str().unwrap_or("answer"),
        data["taskId"].as_str().unwrap_or("?"),
        data["committedRevision"].as_u64().unwrap_or(0),
        data["responsible"].as_str().unwrap_or("unassigned"),
    );
    if !requested.is_empty() {
        out.push_str(&format!("  still asked: {}\n", requested.join(", ")));
    }
    out.push_str(&super::warnings(data));
    out
}
