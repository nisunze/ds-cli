//! `ds pm task respond` — answer an assignment request.
//!
//! The actor is the signed-in native credential's user and cannot be named as
//! a flag: accepting on someone else's behalf is exactly what this command must
//! not be able to do. Which is also why it is reachable at all for a
//! contributor who may not otherwise edit the schedule — answering a request
//! that names you is not editing the plan.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_management::writes::{self, Response};
use serde_json::{Map, Value, json};

use crate::{LANE_ARG, TASK_ARG};

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
    id: "pm.task.respond",
    path: &["pm", "task", "respond"],
    contract: 1,
    summary: "Accept or decline an assignment request naming you.",
    purpose: "\
Answers as the signed-in native credential's user — there is no flag for who \
is answering, because answering for somebody else is the one thing this must \
not allow. Accepting makes you responsible and closes the request for everyone \
else; declining removes only you and leaves it open, and carries no reason, \
because a justification is how declining stops being real. Headless: commits \
to the named project, no window.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TASK_ARG, RESPONSE_ARG, LANE_ARG, crate::PROJECT_ARG],
    output: "\
The project, the `taskId`, the `response` applied, who is `responsible` \
afterwards, who is still being `requested`, and the `committedRevision`.",
    examples: &[Example {
        command: "ds pm task respond --task T-0007 --response accept --yes --project <exact-id>",
        note: "Refused with desktop_refused when the request was withdrawn or somebody accepted first.",
        runnable: false,
    }],
    refusals: &crate::write_refusals::<23>(&[crate::INVALID_VALUE]),
    reference: Some("docs/reference/pm.md"),
    search: &["assign", "assignee", "responsible"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let task_id = inputs.require("task")?.to_owned();
    let response = Response::parse(inputs.require("response")?).ok_or_else(|| {
        Failure::invalid("invalid_choice", "`--response` is accept or decline")
            .remedy("pass --response accept, or --response decline")
    })?;
    let lane = inputs.value("lane").unwrap_or("stable");
    let read = crate::graph(lane, inputs.require("project")?)?;
    let prepared = writes::respond_task(&read.graph, &task_id, response).map_err(crate::refused)?;
    let before = read.graph.tasks.iter().find(|task| task.id == task_id);
    let result = crate::commit(lane, inputs.require("project")?, &prepared)?;
    let (responsible, requested) = writes::assignment_after(&result, &task_id, before);
    let mut extra = Map::new();
    extra.insert("response".into(), json!(response.token()));
    extra.insert("responsible".into(), json!(responsible));
    extra.insert("requested".into(), json!(requested));
    Ok(writes::write_outcome(
        &read.project_id,
        &task_id,
        &result,
        extra,
    ))
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
