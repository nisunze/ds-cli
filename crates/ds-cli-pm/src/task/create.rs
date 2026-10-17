//! `ds pm task create` — add one work item to the plan.
//!
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_management::writes::{self, CreateKind, CreateTask};
use serde_json::{Map, Value, json};

use crate::LANE_ARG;
const DRY_RUN_ARG: Arg = Arg::switch("dry-run", "Preview the prepared task without creating it.");

const ATTACH_ARG: Arg = Arg {
    name: "attach",
    kind: ArgKind::Repeated,
    value: "<path>",
    required: false,
    default: None,
    choices: &[],
    summary: "Attach local files after creation; repeat up to 10, at most 100 MiB combined. All files are checked before creating anything.",
};
const ATTACH_PARTIAL: Refusal = Refusal {
    code: "task_attachments_incomplete",
    when: "the task was committed but one attachment upload was refused, failed or could not be verified",
    remedy: "read the task and detail.taskId/attachments; retain its existing task id and attach only confirmed missing files with pm.task.attach; do not create a replacement task or blindly retry an uncertain upload",
};

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

const REFUSALS: [Refusal; 5] = [
    crate::INVALID_DATE,
    crate::INVALID_VALUE,
    INVALID_TASK_SHAPE,
    super::attach::FILE_INVALID,
    ATTACH_PARTIAL,
];

pub static COMMAND: Command = Command {
    id: "pm.task.create",
    path: &["pm", "task", "create"],
    contract: 2,
    summary: "Add one task or milestone to the project's plan.",
    purpose: "\
Creates one work item through the same governed command the Plan sheet uses, \
so it lands with the sort key, schedule state and duration the surface would \
have given it. A retry that passes the same --id is refused rather than \
duplicated, which is what makes this safe to run again after a lost answer. \
Headless: commits to the named project of the signed-in native credential, \
no window. --dry-run previews the task and creates nothing. With repeatable --attach, the native PM owner creates the task \
then verifies each task-owned upload. The task remains durable if an upload \
fails; the refusal carries its taskId and verified ready receipts. Do not \
repeat task creation to recover an attachment.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: crate::authored_args![
        TITLE_ARG,
        KIND_ARG,
        PARENT_ARG,
        DESCRIPTION_ARG,
        DISCIPLINE_ARG,
        START_ARG,
        FINISH_ARG,
        FROM_RECORD_ARG,
        ID_ARG,
        ATTACH_ARG,
        DRY_RUN_ARG,
        LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "\
The project, the minted `taskId`, the `committedRevision` the plan moved to, \
any `warnings` the engine returned, and `link` — the deep link that opens the \
new item in the app. With --dry-run, the prepared command and no task. With --attach, attachments contains server-verified ready rows and exact local bytes/SHA; an incomplete upload returns task_attachments_incomplete with the committed task and ready receipts.",
    examples: &[
        Example {
            command: "ds pm task create --project <exact-id> --kind inbox --title \"Review crossing\" --attach ./source.eml --attach ./drawing.pdf --yes --output json",
            note: "The task is durable before uploading files; an incomplete upload carries its existing task id and ready receipts.",
            runnable: false,
        },
        Example {
            command: "ds pm task create --title \"Stake MV route\" --kind parent --start 2026-09-01 --finish 2026-09-12 --yes --project <exact-id>",
            note: "Without --yes dispatch refuses before anything is sent.",
            runnable: false,
        },
    ],
    refusals: &crate::write_refusals::<27>(&REFUSALS),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "subtask", "sub-task", "deadline", "due", "date", "backlog", "wbs", "schedule",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut files = Vec::new();
    for path in inputs.repeated("attach") {
        files.push(super::attach::read_attachment(path, None)?);
        ds_client_core::project_management::create_attached::validate_files(&files).map_err(
            |error| {
                Failure::invalid(super::attach::FILE_INVALID.code, error.to_string())
                    .remedy("Pass up to 10 distinct readable files, at most 100 MiB combined")
            },
        )?;
    }
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
    let read = crate::graph(lane, inputs.require("project")?)?;
    let prepared = writes::create_task(
        &read.graph,
        &CreateTask {
            id: id.clone(),
            from_record: inputs.value("from-record").map(str::to_owned),
            title: inputs.require("title")?.to_owned(),
            kind,
            parent: parent.map(str::to_owned),
            description: inputs.value("description").map(str::to_owned),
            discipline: inputs.value("discipline").map(str::to_owned),
            start_date,
            finish_date,
            geometry: None,
            links: Vec::new(),
        },
    )
    .map_err(crate::refused)?;
    if inputs.switch("dry-run") {
        return Ok(json!({
            "project": read.project_id,
            "taskId": id,
            "dry_run": true,
            "kind": kind.token(),
            "revision": read.graph.revision,
            "command": prepared.command,
            "committedRevision": Value::Null,
        }));
    }
    let (result, attachments) = if files.is_empty() {
        (
            crate::authoring::commit(inputs, lane, inputs.require("project")?, &prepared)?,
            None,
        )
    } else {
        let answer = crate::authoring::project(
            inputs,
            lane,
            inputs.require("project")?,
            ds_client_core::project_management::Command::TaskCreateAttached {
                command_id: crate::command_id()?,
                base_revision: prepared.base_revision,
                command: prepared.command.clone(),
                task_id: id.clone(),
                files,
            },
        )?
        .into_result();
        let result = crate::accepted(writes::decode_operation_result(&answer["result"]))?;
        if answer["complete"] != true {
            return Err(Failure::failed(ATTACH_PARTIAL.code, "Task created; not all attachments were verified ready")
                .remedy(ATTACH_PARTIAL.remedy)
                .detail(json!({"project": read.project_id, "taskId": id,
                    "committedRevision": result.committed_revision, "attachments": answer["attachments"],
                    "failed_attachment_index": answer["failed_attachment_index"],
                    "requested_attachment_count": answer["requested_attachment_count"],
                    "failure_kind": answer["failure_kind"]})));
        }
        (result, Some(answer["attachments"].clone()))
    };
    let mut extra = Map::new();
    extra.insert("kind".into(), json!(kind.token()));
    if let Some(attachments) = attachments {
        extra.insert("attachments".into(), attachments);
    }
    Ok(writes::write_outcome(&read.project_id, &id, &result, extra))
}

pub fn render(data: &Value) -> String {
    let mut out = if data["dry_run"] == true {
        format!(
            "would create {} `{}` in {} · revision {} · nothing written\n",
            data["kind"].as_str().unwrap_or("task"),
            data["command"]["create_task"]["title"]
                .as_str()
                .unwrap_or("?"),
            data["project"].as_str().unwrap_or("?"),
            data["revision"].as_u64().unwrap_or(0),
        )
    } else {
        format!(
            "created {} in {} · revision {}\n",
            data["taskId"].as_str().unwrap_or("?"),
            data["project"].as_str().unwrap_or("?"),
            data["committedRevision"].as_u64().unwrap_or(0),
        )
    };
    out.push_str(&super::warnings(data));
    if let Some(link) = data["link"].as_str() {
        out.push_str(&format!("  {link}\n"));
    }
    out
}

#[cfg(test)]
mod attachment_create_tests {
    use super::*;
    #[test]
    fn create_discovers_repeatable_files_and_refuses_missing_file_before_graph_read() {
        let args = [
            "--project",
            "project-a",
            "--kind",
            "inbox",
            "--title",
            "Evidence",
            "--attach",
            "missing-first.pdf",
            "--attach",
            "missing-second.pdf",
        ]
        .map(str::to_owned);
        let inputs = ds_cli_contract::parse(&COMMAND, &args).unwrap();
        assert_eq!(inputs.repeated("attach").len(), 2);
        let error = run(
            &inputs,
            &Context {
                confirmed: true,
                output: ds_cli_contract::Output::resolve(
                    ds_cli_contract::Format::Json,
                    false,
                    true,
                ),
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), super::super::attach::FILE_INVALID.code);
    }
}
