//! `ds pm task list` — the project's plan as bounded rows.
//!
//! The entry point when nothing else is known: `read`, `update`, `assign` and
//! `respond` all need a task id, and until now only the application's own
//! Table could supply one.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_management::reads::{PlacementFilter, TaskListFilter};
use serde_json::Value;

use crate::{LANE_ARG, LIMIT_ARG, PAGE_ARG};

const QUERY_ARG: Arg = Arg {
    name: "query",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Match WBS, id, title, description or a person on the task.",
};

const STATE_ARG: Arg = Arg {
    name: "state",
    kind: ArgKind::Value,
    value: "<delivery-state>",
    required: false,
    default: None,
    choices: &[],
    summary: "Only this delivery state; `ds pm plan` names the vocabulary.",
};

const ASSIGNEE_ARG: Arg = Arg {
    name: "assignee",
    kind: ArgKind::Value,
    value: "<email>",
    required: false,
    default: None,
    choices: &[],
    summary: "Only work this person holds, collaborates on or was asked to take.",
};

const DISCIPLINE_ARG: Arg = Arg {
    name: "discipline",
    kind: ArgKind::Value,
    value: "<name>",
    required: false,
    default: None,
    choices: &[],
    summary: "Only this kind of work — the project's own vocabulary.",
};

const PLACEMENT_ARG: Arg = Arg {
    name: "placement",
    kind: ArgKind::Value,
    value: "<where>",
    required: false,
    default: Some("any"),
    choices: &["any", "wbs", "inbox"],
    summary: "Plan rows, the unplaced inbox, or both.",
};

pub static COMMAND: Command = Command {
    id: "pm.task.list",
    path: &["pm", "task", "list"],
    contract: 1,
    summary: "List the project's work items with state and who holds them.",
    purpose: "\
Names every task and milestone in the active project's plan, in WBS order, \
with its delivery state, progress and responsible person. This is where a \
Project Work session starts: every other `ds pm task` command needs an id \
from here. Reads the same canonical graph the Plan and Table surfaces render \
and changes nothing. Headless: the selected project of the signed-in native \
credential, no window.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        QUERY_ARG,
        STATE_ARG,
        ASSIGNEE_ARG,
        DISCIPLINE_ARG,
        PLACEMENT_ARG,
        LIMIT_ARG,
        PAGE_ARG,
        LANE_ARG,
    ],
    output: "\
The project, its graph revision, the matched total, the page bounds, and rows \
of `wbs`, `id`, `title`, `type`, `delivery`, `review`, `closeout`, `progress`, \
`start`, `finish`, `responsible`, `discipline`, `priority`, `blockers` and \
`assignmentOpen` — true while a request is waiting for an answer.",
    examples: &[Example {
        command: "ds pm task list --state blocked --output json",
        note: "Read .data.tasks[].id to feed read, update, assign or respond.",
        runnable: false,
    }],
    refusals: &crate::read_refusals::<17>(&[crate::INVALID_NUMBER]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "subtask",
        "sub-task",
        "backlog",
        "deadline",
        "due",
        "date",
        "gantt",
        "wbs",
        "milestone",
        "schedule",
        "progress",
        "assignee",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let filter = TaskListFilter {
        query: inputs.value("query").unwrap_or_default().to_owned(),
        state: inputs.value("state").map(str::to_owned),
        assignee: inputs.value("assignee").map(str::to_owned),
        discipline: inputs.value("discipline").map(str::to_owned),
        // `any` is the default and matches everything.
        placement: match inputs.value("placement") {
            Some("wbs") => PlacementFilter::Wbs,
            Some("inbox") => PlacementFilter::Inbox,
            _ => PlacementFilter::Any,
        },
        page_size: match inputs.value("limit") {
            Some(limit) => crate::integer(limit, "limit", 1, crate::MAX_PAGE_SIZE)?,
            None => 50,
        },
        page: match inputs.value("page") {
            Some(page) => crate::integer(page, "page", 0, 10_000)?,
            None => 0,
        },
    };
    let read = crate::graph(inputs.value("lane").unwrap_or("stable"))?;
    crate::data(&ds_command_kernel::project_management::reads::task_list(
        &read.graph,
        &filter,
    ))
}

pub fn render(data: &Value) -> String {
    let total = data["total"].as_u64().unwrap_or(0);
    let mut out = format!(
        "{} in {} at revision {}\n",
        crate::plural(total, "work item"),
        data["project"].as_str().unwrap_or("?"),
        data["revision"].as_u64().unwrap_or(0),
    );
    if let Some(rows) = data["tasks"].as_array() {
        for row in rows {
            out.push_str(&crate::task_line(row));
        }
        let through = data["to"].as_u64().unwrap_or(rows.len() as u64);
        if through < total {
            out.push_str(&format!(
                "  … {} more; raise --limit or ask for --page {}\n",
                total - through,
                data["page"].as_u64().unwrap_or(0) + 1,
            ));
        }
    }
    out
}
