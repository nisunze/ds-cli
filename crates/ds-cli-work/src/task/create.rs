//! `ds work task create` — add one work item to the plan.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

const TITLE_ARG: Arg = Arg {
    name: "title",
    kind: ArgKind::Value,
    value: "<text>",
    required: true,
    default: None,
    choices: &[],
    summary: "What the work is. The one field a plan node cannot be without.",
};

const KIND_ARG: Arg = Arg {
    name: "kind",
    kind: ArgKind::Value,
    value: "<kind>",
    required: false,
    default: Some("parent"),
    choices: &["parent", "child", "inbox", "milestone"],
    summary: "Top-level phase, child of --parent, unplaced inbox item, or milestone.",
};

const PARENT_ARG: Arg = Arg {
    name: "parent",
    kind: ArgKind::Value,
    value: "<task-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "The task this one sits under. Required for --kind child.",
};

const DESCRIPTION_ARG: Arg = Arg {
    name: "description",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "What done looks like. Empty is allowed and common for a milestone.",
};

const DISCIPLINE_ARG: Arg = Arg {
    name: "discipline",
    kind: ArgKind::Value,
    value: "<name>",
    required: false,
    default: None,
    choices: &[],
    summary: "The kind of work — survey, design, finance. The project's vocabulary.",
};

const START_ARG: Arg = Arg {
    name: "start",
    kind: ArgKind::Value,
    value: "<yyyy-mm-dd>",
    required: false,
    default: None,
    choices: &[],
    summary: "Planned start. A milestone takes this or --finish, not both.",
};

const FINISH_ARG: Arg = Arg {
    name: "finish",
    kind: ArgKind::Value,
    value: "<yyyy-mm-dd>",
    required: false,
    default: None,
    choices: &[],
    summary: "Planned finish. Omit both to create the item unscheduled.",
};

const ID_ARG: Arg = Arg {
    name: "id",
    kind: ArgKind::Value,
    value: "<task-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "Mint this id. Reuse it on a retry; a second create is refused.",
};

const INVALID_TASK_SHAPE: Refusal = Refusal {
    code: "invalid_task_shape",
    when: "a child has no parent, a root/inbox item names one, or a milestone names two dates",
    remedy: "use --parent only with --kind child or milestone, and give a milestone one date",
};

pub static COMMAND: Command = Command {
    id: "work.task.create",
    path: &["work", "task", "create"],
    contract: 1,
    summary: "Add one task or milestone to the project's plan.",
    purpose: "\
Creates one work item through the same governed command the Plan sheet uses, \
so it lands with the sort key, schedule state and duration the surface would \
have given it. A retry that passes the same --id is refused rather than \
duplicated, which is what makes this safe to run again after a lost answer.",
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        TITLE_ARG,
        KIND_ARG,
        PARENT_ARG,
        DESCRIPTION_ARG,
        DISCIPLINE_ARG,
        START_ARG,
        FINISH_ARG,
        ID_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
The project, the minted `taskId`, the `committedRevision` the plan moved to, \
any `warnings` the engine returned, and `link` — the deep link that opens the \
new item in the app.",
    examples: &[Example {
        command: "ds work task create --title \"Stake MV route\" --kind parent --start 2026-09-01 --finish 2026-09-12 --yes",
        note: "Without --yes dispatch refuses before the bridge is opened.",
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
        crate::NOT_PERMITTED,
        crate::CONFLICT,
        crate::INVALID_DATE,
        INVALID_TASK_SHAPE,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/work.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let kind = inputs.value("kind").unwrap_or("parent");
    let parent = inputs.value("parent");
    let start = inputs.value("start");
    let finish = inputs.value("finish");
    let invalid_shape = (kind == "child" && parent.is_none())
        || (matches!(kind, "parent" | "inbox") && parent.is_some())
        || (kind == "milestone" && start.is_some() && finish.is_some());
    if invalid_shape {
        return Err(Failure::invalid(
            "invalid_task_shape",
            "the task kind, parent and schedule flags describe conflicting shapes",
        )
        .remedy(INVALID_TASK_SHAPE.remedy)
        .next("ds work task create --help"));
    }

    let mut arguments = Map::new();
    arguments.insert("title".into(), json!(inputs.require("title")?));
    if let Some(kind) = inputs.value("kind") {
        arguments.insert("kind".into(), json!(kind));
    }
    for (flag, key) in [
        ("parent", "parent"),
        ("description", "description"),
        ("discipline", "discipline"),
        ("id", "id"),
    ] {
        if let Some(value) = inputs.value(flag) {
            arguments.insert(key.into(), json!(value));
        }
    }
    for (flag, key, value) in [
        ("start", "startDate", start),
        ("finish", "finishDate", finish),
    ] {
        if let Some(value) = value {
            arguments.insert(key.into(), json!(crate::date(value, flag)?));
        }
    }

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TASK_CREATE,
        Value::Object(arguments),
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_work_failure)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "created {} in {} · revision {}\n",
        data["taskId"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        data["committedRevision"].as_u64().unwrap_or(0),
    );
    out.push_str(&super::warnings(data));
    if let Some(link) = data["link"].as_str() {
        out.push_str(&format!("  {link}\n"));
    }
    out
}
