//! `ds pm task update` — change one work item's fields, states or dates.
//!
//! Everything given in one invocation is one saved draft: the kernel folds
//! the flags into the project commands they imply and the native client
//! commits them against a single base revision, so a title change and a state
//! change either both land or neither does. That is the same atomicity the
//! Plan sheet's own save has, and it is why this is one command rather than
//! six.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_management::writes::{self, UpdateTask};
use serde_json::{Map, Value, json};

use crate::{LANE_ARG, TASK_ARG};

const TITLE_ARG: Arg = Arg::value("title", "<text>", "Rename the work item.");
const DESCRIPTION_ARG: Arg = Arg::value("description", "<text>", "Replace what done looks like.");
const DISCIPLINE_ARG: Arg = Arg::value("discipline", "<name>", "Move it to another kind of work.");
const PRIORITY_ARG: Arg = Arg::value("priority", "<level>", "How this competes for attention.")
    .choices(&["low", "normal", "high", "critical"]);
const TYPE_ARG: Arg = Arg::value("type", "<type>", "Turn a task into a milestone, or back.")
    .choices(&["task", "milestone"]);
const PLACEMENT_ARG: Arg =
    Arg::value("placement", "<where>", "Move between plan and inbox.").choices(&["wbs", "inbox"]);
const SCHEDULING_ARG: Arg = Arg::value(
    "scheduling",
    "<mode>",
    "Whether the engine may move the dates itself.",
)
.choices(&["manual", "auto"]);
const DELIVERY_ARG: Arg = Arg::value(
    "delivery",
    "<state>",
    "Delivery state; `ds pm plan` names the vocabulary this project uses.",
);
const REVIEW_ARG: Arg = Arg::value(
    "review",
    "<state>",
    "Review state, from the same vocabulary.",
);
const CLOSEOUT_ARG: Arg = Arg::value(
    "closeout",
    "<state>",
    "Closeout state, from the same vocabulary.",
);
const PROGRESS_ARG: Arg = Arg::value("progress", "<percent>", "Percent complete, 0 through 100.");
const START_ARG: Arg = Arg::value("start", "<yyyy-mm-dd>", "Move the planned start.");
const FINISH_ARG: Arg = Arg::value("finish", "<yyyy-mm-dd>", "Move the planned finish.");

pub static COMMAND: Command = Command {
    id: "pm.task.update",
    path: &["pm", "task", "update"],
    contract: 1,
    summary: "Change one work item's fields, states, progress or dates.",
    purpose: "\
Applies every flag given as one atomic saved draft against a single base \
revision, exactly as the Plan sheet's own save does. Nothing given, nothing \
sent: a flag you omit is untouched, never reset. The engine owns the schedule \
consequences — moving a date may move dependants, and the warnings it returns \
are reported rather than swallowed. Headless: commits to the selected project \
of the signed-in native credential, no window.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TASK_ARG,
        TITLE_ARG,
        DESCRIPTION_ARG,
        DISCIPLINE_ARG,
        PRIORITY_ARG,
        TYPE_ARG,
        PLACEMENT_ARG,
        SCHEDULING_ARG,
        DELIVERY_ARG,
        REVIEW_ARG,
        CLOSEOUT_ARG,
        PROGRESS_ARG,
        START_ARG,
        FINISH_ARG,
        LANE_ARG,
    ],
    output: "\
The project, the `taskId`, `applied`, the `committedRevision`, the list of \
`commands` the flags became, and any `warnings` the engine returned.",
    examples: &[Example {
        command: "ds pm task update --task T-0007 --delivery in_progress --progress 40 --yes",
        note: "Delivery and progress land together or not at all.",
        runnable: false,
    }],
    refusals: &crate::write_refusals::<27>(&[
        crate::INVALID_DATE,
        crate::INVALID_NUMBER,
        crate::INVALID_VALUE,
        Refusal {
            code: "invalid_task_shape",
            when: "a milestone was given two different dates in one update",
            remedy: "give a milestone one date",
        },
        Refusal {
            code: "nothing_to_update",
            when: "no field, state, progress or date flag was given",
            remedy: "name at least one change, e.g. --delivery in_progress",
        },
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "deadline",
        "due",
        "date",
        "reschedule",
        "schedule",
        "milestone",
        "subtask",
        "sub-task",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let task = inputs.require("task")?.to_owned();
    let progress = inputs
        .value("progress")
        .map(|value| crate::integer(value, "progress", 0, 100))
        .transpose()?
        .map(|value| value as u64);
    let start_date = inputs
        .value("start")
        .map(|value| crate::date(value, "start"))
        .transpose()?;
    let finish_date = inputs
        .value("finish")
        .map(|value| crate::date(value, "finish"))
        .transpose()?;
    let update = UpdateTask {
        task: task.clone(),
        title: inputs.value("title").map(str::to_owned),
        description: inputs.value("description").map(str::to_owned),
        discipline: inputs.value("discipline").map(str::to_owned),
        priority: inputs.value("priority").map(str::to_owned),
        task_type: inputs.value("type").map(str::to_owned),
        placement: inputs.value("placement").map(str::to_owned),
        scheduling_mode: inputs.value("scheduling").map(str::to_owned),
        delivery: inputs.value("delivery").map(str::to_owned),
        review: inputs.value("review").map(str::to_owned),
        closeout: inputs.value("closeout").map(str::to_owned),
        progress,
        start_date,
        finish_date,
    };
    // `task` alone is a read wearing a write's confirmation gate. Refusing it
    // here means an empty invocation never spends a project round trip, and
    // never reports `applied` for a change nobody asked for.
    let nothing = update.title.is_none()
        && update.description.is_none()
        && update.discipline.is_none()
        && update.priority.is_none()
        && update.task_type.is_none()
        && update.placement.is_none()
        && update.scheduling_mode.is_none()
        && update.delivery.is_none()
        && update.review.is_none()
        && update.closeout.is_none()
        && update.progress.is_none()
        && update.start_date.is_none()
        && update.finish_date.is_none();
    if nothing {
        return Err(crate::refused(writes::Refusal::NothingToChange));
    }

    let lane = inputs.value("lane").unwrap_or("stable");
    let read = crate::graph(lane)?;
    let (prepared, kinds) = writes::update_task(&read.graph, &update).map_err(crate::refused)?;
    let result = crate::commit_batch(lane, &prepared)?;
    let mut extra = Map::new();
    extra.insert("commands".into(), json!(kinds));
    Ok(writes::write_outcome(
        &read.project_id,
        &task,
        &result,
        extra,
    ))
}

pub fn render(data: &Value) -> String {
    let commands: Vec<&str> = data["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut out = format!(
        "{} {} · revision {} · {}\n",
        if data["applied"].as_bool().unwrap_or(false) {
            "updated"
        } else {
            "not applied:"
        },
        data["taskId"].as_str().unwrap_or("?"),
        data["committedRevision"].as_u64().unwrap_or(0),
        if commands.is_empty() {
            "no change".to_string()
        } else {
            commands.join(", ")
        },
    );
    out.push_str(&super::warnings(data));
    out
}
