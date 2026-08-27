//! `ds work task list` — the project's plan as bounded rows.
//!
//! The entry point when nothing else is known: `read`, `update`, `assign` and
//! `respond` all need a task id, and until now only the application's own
//! Table could supply one.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, LIMIT_ARG, PAGE_ARG};

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
    summary: "Only this delivery state; `ds work plan` names the vocabulary.",
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
    id: "work.task.list",
    path: &["work", "task", "list"],
    contract: 1,
    summary: "List the project's work items with state and who holds them.",
    purpose: "\
Names every task and milestone in the active project's plan, in WBS order, \
with its delivery state, progress and responsible person. This is where a \
Project Work session starts: every other `ds work task` command needs an id \
from here. Reads the same canonical graph the Plan and Table surfaces render \
and changes nothing.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        QUERY_ARG,
        STATE_ARG,
        ASSIGNEE_ARG,
        DISCIPLINE_ARG,
        PLACEMENT_ARG,
        LIMIT_ARG,
        PAGE_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
The project, its graph revision, the matched total, the page bounds, and rows \
of `wbs`, `id`, `title`, `type`, `delivery`, `review`, `closeout`, `progress`, \
`start`, `finish`, `responsible`, `discipline`, `priority`, `blockers` and \
`assignmentOpen` — true while a request is waiting for an answer.",
    examples: &[Example {
        command: "ds work task list --state blocked --output json",
        note: "Read .data.tasks[].id to feed read, update, assign or respond.",
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
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/work.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    for (flag, key) in [
        ("query", "query"),
        ("state", "state"),
        ("assignee", "assignee"),
        ("discipline", "discipline"),
    ] {
        if let Some(value) = inputs.value(flag) {
            arguments.insert(key.into(), json!(value));
        }
    }
    // `any` is the application's own default; sending it would be a key that
    // says nothing, so the absent flag stays absent on the wire.
    if let Some(placement) = inputs.value("placement").filter(|value| *value != "any") {
        arguments.insert("placement".into(), json!(placement));
    }
    if let Some(limit) = inputs.value("limit") {
        arguments.insert(
            "limit".into(),
            json!(crate::integer(limit, "limit", 1, crate::MAX_PAGE_SIZE)?),
        );
    }
    if let Some(page) = inputs.value("page") {
        arguments.insert(
            "page".into(),
            json!(crate::integer(page, "page", 0, 10_000)?),
        );
    }

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TASKS_LIST,
        Value::Object(arguments),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_work_failure)
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
