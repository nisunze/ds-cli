//! `ds pm task block` — a task waits on an answer somebody owes.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use serde_json::{Value, json};

use crate::{LANE_ARG, TASK_ARG};

const ON_RECORD_ARG: Arg = Arg::value(
    "on-record",
    "<record-id>",
    "The record whose answer the task waits on; its response must still be owed.",
)
.required();

pub static COMMAND: Command = Command {
    id: "pm.task.block",
    path: &["pm", "task", "block"],
    contract: 1,
    summary: "Block a task on a record until the answer it owes arrives.",
    purpose: "\
Records that a task cannot move until an outside party — or the project — \
answers a record. The blocker is a relation on the task, of kind \
awaiting_correspondence, and clears ITSELF the moment the record's \
answer arrives (a reply from the owing side) or is waived; nothing else \
clears it silently, and `ds pm task unblock` clears it by hand with a \
reason. The plan's attention and the task's blocker count update in the \
same commit. A record that owes no answer cannot be waited on. Headless: \
commits to the named project of the signed-in native credential, no \
window.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TASK_ARG, ON_RECORD_ARG, LANE_ARG, crate::PROJECT_ARG],
    output: "\
The project, `taskId`, `recordId`, the `committedRevision` the plan moved \
to, and the engine's `result` (`applied`, `warnings`).",
    examples: &[Example {
        command: "ds pm task block --task T-0012 --on-record R-0031 --yes --project <exact-id>",
        note: "Blocked until R-0031 is answered from the side that owes it, or waived.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<25>(&[
        crate::TASK_NOT_FOUND,
        crate::RECORD_NOT_FOUND,
        crate::RECORD_NOT_AWAITING,
        crate::BLOCKER_EXISTS,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "correspondence",
        "blocked on",
        "waiting on",
        "awaiting answer",
        "blocker",
        "dependency",
        "ball court",
        "hold",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// The blocker commands share one shape: read the plan for its revision,
/// prove the task is on it, send one governed command against that revision.
pub fn blocker(inputs: &Inputs, unblock: bool) -> Result<Value, Failure> {
    let task_id = inputs.require("task")?.to_owned();
    let record_id = inputs
        .require(if unblock { "record" } else { "on-record" })?
        .to_owned();
    let lane = inputs.value("lane").unwrap_or("stable");
    let read = crate::graph(lane, inputs.require("project")?)?;
    if !read.graph.tasks.iter().any(|task| task.id == task_id) {
        return Err(Failure::invalid(
            crate::TASK_NOT_FOUND.code,
            format!("No task {task_id} in this project's plan."),
        )
        .detail(json!({ "task": task_id, "project": read.project_id }))
        .remedy(crate::TASK_NOT_FOUND.remedy)
        .next("ds pm task list"));
    }
    let command_id = crate::command_id()?;
    let action = if unblock {
        Action::TaskUnblock {
            command_id,
            base_revision: read.graph.revision,
            task_id: task_id.clone(),
            record_id: record_id.clone(),
            reason: inputs.require("reason")?.to_owned(),
        }
    } else {
        Action::TaskBlock {
            command_id,
            base_revision: read.graph.revision,
            task_id: task_id.clone(),
            record_id: record_id.clone(),
        }
    };
    let report = crate::correspondence(lane, inputs.require("project")?, &action)?;
    let result = ds_command_kernel::project_management::writes::decode_operation_result(
        &report.into_result(),
    );
    if !result.applied || !result.violations.is_empty() {
        let first = result.violations.first();
        return Err(Failure::invalid(
            crate::PM_REFUSED.code,
            first
                .map(|issue| issue.message.clone())
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| "Project Management did not apply the blocker.".into()),
        )
        .detail(json!({
            "service_code": first.map(|issue| issue.code.clone()),
            "violations": result.violations,
        }))
        .remedy(crate::PM_REFUSED.remedy)
        .next(format!("ds pm task read --task {task_id} --timeline")));
    }
    Ok(json!({
        "project": read.project_id,
        "taskId": task_id,
        "recordId": record_id,
        "committedRevision": result.committed_revision,
        "applied": result.applied,
        "warnings": result.warnings,
        "link": ds_command_kernel::project_management::reads::task_route(&read.project_id, &task_id),
    }))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    blocker(inputs, false)
}

pub fn render(data: &Value) -> String {
    format!(
        "blocked {} on {} in {} · revision {}\n",
        data["taskId"].as_str().unwrap_or("?"),
        data["recordId"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        data["committedRevision"].as_u64().unwrap_or(0),
    )
}
