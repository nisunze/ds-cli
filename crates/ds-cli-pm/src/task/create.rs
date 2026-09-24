//! `ds pm task create` — add one work item to the plan.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_management::writes::{self, CreateKind, CreateTask};
use serde_json::{Map, Value, json};

use crate::LANE_ARG;

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

const FROM_RECORD_ARG: Arg = Arg::value(
    "from-record",
    "<record-id>",
    "The correspondence record this task answers; the record lists the task under resultingTasks in the same commit. Valid with every --kind.",
);

pub const INVALID_TASK_SHAPE: Refusal = Refusal {
    code: "invalid_task_shape",
    when: "a child has no parent, a root/inbox item names one, or a milestone names two dates",
    remedy: "use --parent only with --kind child or milestone, and give a milestone one date",
};

pub static COMMAND: Command = Command {
    id: "pm.task.create",
    path: &["pm", "task", "create"],
    contract: 1,
    summary: "Add one task or milestone to the project's plan.",
    purpose: "\
Creates one work item through the same governed command the Plan sheet uses, \
so it lands with the sort key, schedule state and duration the surface would \
have given it. A retry that passes the same --id is refused rather than \
duplicated, which is what makes this safe to run again after a lost answer. \
Made --from-record, the task points back at the correspondence it answers \
and the record lists it. Headless: commits to the selected project of the \
signed-in native credential, no window.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TITLE_ARG,
        KIND_ARG,
        PARENT_ARG,
        DESCRIPTION_ARG,
        DISCIPLINE_ARG,
        START_ARG,
        FINISH_ARG,
        FROM_RECORD_ARG,
        ID_ARG,
        LANE_ARG,
    ],
    output: "\
The project, the minted `taskId`, the `committedRevision` the plan moved to, \
any `warnings` the engine returned, and `link` — the deep link that opens the \
new item in the app.",
    examples: &[Example {
        command: "ds pm task create --title \"Stake MV route\" --kind parent --start 2026-09-01 --finish 2026-09-12 --yes",
        note: "Without --yes dispatch refuses before anything is sent.",
        runnable: false,
    }],
    refusals: &crate::write_refusals::<26>(&[
        crate::INVALID_DATE,
        crate::INVALID_VALUE,
        INVALID_TASK_SHAPE,
        crate::RECORD_NOT_FOUND,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "subtask",
        "sub-task",
        "deadline",
        "due",
        "date",
        "backlog",
        "wbs",
        "schedule",
        "from record",
        "correspondence",
        "action from letter",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
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
        .next("ds pm task create --help"));
    }
    let kind = CreateKind::parse(kind).ok_or_else(|| {
        Failure::invalid(
            "invalid_choice",
            "`--kind` is not one of parent, child, inbox, milestone",
        )
        .remedy(crate::INVALID_VALUE.remedy)
    })?;
    let start_date = start.map(|value| crate::date(value, "start")).transpose()?;
    let finish_date = finish
        .map(|value| crate::date(value, "finish"))
        .transpose()?;
    // The id is minted HERE when the caller supplied none: the engine takes
    // identity from the caller, the kernel has no entropy, and a retry that
    // repeats the same --id is what makes a create safe to run again.
    let id = match inputs.value("id") {
        Some(id) => id.to_owned(),
        None => crate::command_id()?,
    };

    let lane = inputs.value("lane").unwrap_or("stable");
    let read = crate::graph(lane)?;
    let prepared = writes::create_task(
        &read.graph,
        &CreateTask {
            id: id.clone(),
            title: inputs.require("title")?.to_owned(),
            kind,
            parent: parent.map(str::to_owned),
            description: inputs.value("description").map(str::to_owned),
            discipline: inputs.value("discipline").map(str::to_owned),
            start_date,
            finish_date,
            from_record: inputs.value("from-record").map(str::to_owned),
        },
    )
    .map_err(crate::refused)?;
    let result = crate::commit(lane, &prepared).map_err(crate::classify)?;
    let mut extra = Map::new();
    extra.insert("kind".into(), json!(kind.token()));
    if let Some(record) = inputs.value("from-record") {
        extra.insert("fromRecordId".into(), json!(record));
    }
    Ok(writes::write_outcome(&read.project_id, &id, &result, extra))
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
