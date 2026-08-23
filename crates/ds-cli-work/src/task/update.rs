//! `ds work task update` — change one work item's fields, states or dates.
//!
//! Everything given in one invocation is one saved draft: the application
//! folds the flags into the project commands they imply and commits them
//! against a single base revision, so a title change and a state change either
//! both land or neither does. That is the same atomicity the Plan sheet's own
//! save has, and it is why this is one command rather than six.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, TASK_ARG};

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
    "Delivery state; `ds work plan` names the vocabulary this project uses.",
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
    id: "work.task.update",
    path: &["work", "task", "update"],
    contract: 1,
    summary: "Change one work item's fields, states, progress or dates.",
    purpose: "\
Applies every flag given as one atomic saved draft against a single base \
revision, exactly as the Plan sheet's own save does. Nothing given, nothing \
sent: a flag you omit is untouched, never reset. The engine owns the schedule \
consequences — moving a date may move dependants, and the warnings it returns \
are reported rather than swallowed.",
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
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
        DESCRIPTOR_ARG,
    ],
    output: "\
The project, the `taskId`, `applied`, the `committedRevision`, the list of \
`commands` the flags became, and any `warnings` the engine returned.",
    examples: &[Example {
        command: "ds work task update --task T-0007 --delivery in_progress --progress 40 --yes",
        note: "Delivery and progress land together or not at all.",
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
        crate::INVALID_NUMBER,
        crate::CONFIRMATION_REQUIRED,
        Refusal {
            code: "nothing_to_update",
            when: "no field, state, progress or date flag was given",
            remedy: "name at least one change, e.g. --delivery in_progress",
        },
    ],
    reference: Some("docs/reference/work.md"),
    availability: crate::paired_availability,
};

/// The authored fields, as (flag, the key inside the `fields` patch).
///
/// They travel as one nested object because the application folds them into a
/// single `update_task_fields` command — a patch, not a replacement, so a key
/// absent here is a field left alone rather than a field cleared.
const FIELD_FLAGS: &[(&str, &str)] = &[
    ("title", "title"),
    ("description", "description"),
    ("discipline", "discipline"),
    ("priority", "priority"),
    ("type", "type"),
    ("placement", "placement"),
    ("scheduling", "schedulingMode"),
];

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("task".into(), json!(inputs.require("task")?));

    let mut fields = Map::new();
    for (flag, key) in FIELD_FLAGS {
        if let Some(value) = inputs.value(flag) {
            fields.insert((*key).into(), json!(value));
        }
    }
    if !fields.is_empty() {
        arguments.insert("fields".into(), Value::Object(fields));
    }

    for state in ["delivery", "review", "closeout"] {
        if let Some(value) = inputs.value(state) {
            arguments.insert(state.into(), json!(value));
        }
    }
    if let Some(progress) = inputs.value("progress") {
        arguments.insert(
            "progress".into(),
            json!(crate::integer(progress, "progress", 0, 100)?),
        );
    }
    for (flag, key) in [("start", "startDate"), ("finish", "finishDate")] {
        if let Some(value) = inputs.value(flag) {
            arguments.insert(key.into(), json!(crate::date(value, flag)?));
        }
    }

    // `task` alone is a read wearing a write's confirmation gate. Refusing it
    // here means an empty invocation never spends a project round trip, and
    // never reports `applied` for a change nobody asked for.
    if arguments.len() == 1 {
        return Err(Failure::invalid(
            "nothing_to_update",
            "no field, state, progress or date flag was given",
        )
        .remedy("name at least one change, e.g. --delivery in_progress")
        .next("ds work task update --help"));
    }

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TASK_UPDATE,
        Value::Object(arguments),
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_work_failure)
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
