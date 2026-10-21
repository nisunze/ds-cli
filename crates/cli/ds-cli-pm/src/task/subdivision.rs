//! Thin adapters over the server's bounded subdivision and progress commands.
//! Ownership, membership, atomicity and rollups belong to ds-brain. An explicit
//! command id and reviewed revision make an uncertain reply exactly replayable.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_management::Command as PmCommand;
use ds_command_kernel::project_management::writes;
use serde_json::{Value, json};

use crate::{LANE_ARG, PROJECT_ARG, TASK_ARG};

const ID: Arg = Arg::value(
    "id",
    "<command-id>",
    "Stable id (8-128 letters, digits, _ or -); reuse this AND --base-revision after a lost reply.",
)
.required();
const REVISION: Arg = Arg::value(
    "base-revision",
    "<revision>",
    "Plan revision reviewed with `ds pm plan`; preserve it on an exact retry.",
)
.required();
const REQUEST: Arg = Arg {
    name: "request",
    kind: ArgKind::Repeated,
    value: "<email>",
    required: true,
    default: None,
    choices: &[],
    summary: "Ask an active project member to take the child. Repeat; the first acceptance holds it.",
};
const INVALID_PROGRESS: Refusal = Refusal {
    code: "invalid_progress",
    when: "--percent is not a finite number from 0 through 100",
    remedy: "pass a percentage, e.g. --percent 50",
};
pub(crate) const RESPONSE_UNREADABLE: Refusal = Refusal {
    code: "task_write_response_unreadable",
    when: "the server's applied receipt does not match the submitted id and revision",
    remedy: "retry the exact same --id, --base-revision and intent; report a repeated mismatch",
};

pub static SUBDIVIDE: Command = Command {
    id: "pm.task.subdivide",
    path: &["pm", "task", "subdivide"],
    contract: 1,
    summary: "Create one subtask and ask project members to take it, atomically.",
    purpose: "An accepted responsible owner can split an open task into one unscheduled child, without schedule-editor access. The child and its assignment request commit together through the existing PM graph; recipients accept through `ds pm task respond`. Parent links and progress use the existing plan rollups. Schedule editors keep their existing authority. The server checks active project membership and accepted ownership against the committed revision. No reporting-line hierarchy is inferred: recipients are active project members. Keep --id, --base-revision and the complete intent unchanged on a retry.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TASK_ARG,
        Arg::value("title", "<text>", "Child task title, up to 400 characters.").required(),
        Arg::value(
            "description",
            "<text>",
            "What done looks like, up to 4000 characters.",
        ),
        REQUEST,
        ID,
        REVISION,
        LANE_ARG,
        PROJECT_ARG,
    ],
    output: "The explicit project, child taskId, parentTaskId, commandId, baseRevision, requested recipients, committedRevision, warnings and task deep link. One child and request, or no writes.",
    examples: &[Example {
        command: "ds pm task subdivide --project <exact-id> --task <parent-id> --title \"Inspect crossing\" --request field@example.com --id crossing-0001 --base-revision 7 --yes",
        note: "Read the plan revision first. The recipient accepts with pm task respond.",
        runnable: false,
    }],
    refusals: &crate::write_refusals::<27>(&[
        crate::INVALID_EMAIL,
        crate::INVALID_NUMBER,
        super::proposals::INVALID_COMMAND_ID,
        super::assign::TOO_MANY_ASSIGNEES,
        RESPONSE_UNREADABLE,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "subordinate",
        "subdivision",
        "delegate",
        "child",
        "accepted owner",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static PROGRESS: Command = Command {
    id: "pm.task.progress",
    path: &["pm", "task", "progress"],
    contract: 1,
    summary: "Report progress on a leaf task you accepted; its parent rolls up.",
    purpose: "The accepted responsible owner of an open leaf reports primary progress through the ordinary set_progress command. The server checks ownership; summary and milestone progress remain derived. Schedule editors keep their existing authority. Delivery, review and closeout remain their existing independent states. Reuse the exact id, revision and percentage after a lost reply.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TASK_ARG,
        Arg::value("percent", "<0-100>", "Primary progress as a percentage.").required(),
        ID,
        REVISION,
        LANE_ARG,
        PROJECT_ARG,
    ],
    output: "Project, taskId, commandId, baseRevision, committedRevision, warnings and deep link. The ordinary task views show parent progress derived from its children.",
    examples: &[Example {
        command: "ds pm task progress --project <exact-id> --task <child-id> --percent 50 --id progress-0001 --base-revision 9 --yes",
        note: "Accept the assignment first with pm task respond.",
        runnable: false,
    }],
    refusals: &crate::write_refusals::<26>(&[
        crate::INVALID_NUMBER,
        super::proposals::INVALID_COMMAND_ID,
        INVALID_PROGRESS,
        RESPONSE_UNREADABLE,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &["subtask", "percent", "accepted owner", "rollup"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn envelope(inputs: &Inputs) -> Result<(String, i64), Failure> {
    Ok((
        super::proposals::command_id(inputs, "subdivision")?,
        crate::integer(
            inputs.require("base-revision")?,
            "base-revision",
            0,
            i64::MAX,
        )?,
    ))
}

fn subdivision(inputs: &Inputs) -> Result<(PmCommand, String, String, i64), Failure> {
    let (id, revision) = envelope(inputs)?;
    let child_id = format!("child-{id}");
    let mut recipients = Vec::new();
    for raw in inputs.repeated("request") {
        let email = crate::email(raw, "request")?;
        if !recipients.contains(&email) {
            recipients.push(email);
        }
    }
    if recipients.len() > crate::MAX_ASSIGNEES {
        return Err(crate::refused(writes::Refusal::TooManyAssignees {
            given: recipients.len(),
            max: crate::MAX_ASSIGNEES,
        }));
    }
    // Fixed wire intents, with no scheduling/authority decisions in the CLI.
    let commands = vec![
        json!({"kind": "create_task", "create_task": {"id": child_id, "parent_id": inputs.require("task")?, "title": inputs.require("title")?, "description": inputs.value("description").unwrap_or(""), "type": "task", "placement": "wbs", "schedule_state": "unscheduled", "start": ""}}),
        json!({"kind": "request_assignment", "request_assignment": {"task_id": child_id, "emails": recipients}}),
    ];
    Ok((
        PmCommand::CommitBatch {
            command_id: id.clone(),
            base_revision: revision,
            commands,
        },
        child_id,
        id,
        revision,
    ))
}

fn progress(inputs: &Inputs) -> Result<(PmCommand, String, i64), Failure> {
    let (id, revision) = envelope(inputs)?;
    let percent = inputs
        .require("percent")?
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
        .ok_or_else(|| {
            Failure::invalid(
                INVALID_PROGRESS.code,
                "--percent must be from 0 through 100",
            )
            .remedy(INVALID_PROGRESS.remedy)
        })?;
    Ok((
        PmCommand::Commit {
            command_id: id.clone(),
            base_revision: revision,
            command: json!({"kind": "set_progress", "set_progress": {"task_id": inputs.require("task")?, "progress": percent / 100.0}}),
        },
        id,
        revision,
    ))
}

pub(crate) fn send(
    inputs: &Inputs,
    command: &PmCommand,
    task: &str,
    id: &str,
    revision: i64,
) -> Result<Value, Failure> {
    let report = ds_cli_auth::project_management_for_project(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
        command,
    )?;
    let project = report.project_id().to_owned();
    let raw = report.into_result();
    let result = crate::accepted(writes::decode_operation_result(&raw))?;
    if raw["command_id"].as_str() != Some(id) || result.committed_revision != revision + 1 {
        return Err(Failure::unavailable(
            RESPONSE_UNREADABLE.code,
            "task write receipt did not match the submitted command",
        )
        .detail(json!({"project": project, "task": task, "commandId": id, "baseRevision": revision, "outcomeUnknown": true}))
        .remedy(RESPONSE_UNREADABLE.remedy));
    }
    let mut extra = serde_json::Map::new();
    extra.insert("commandId".into(), json!(id));
    extra.insert("baseRevision".into(), json!(revision));
    Ok(writes::write_outcome(&project, task, &result, extra))
}

pub fn run_subdivide(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let (command, child, id, revision) = subdivision(inputs)?;
    let mut outcome = send(inputs, &command, &child, &id, revision)?;
    outcome["parentTaskId"] = json!(inputs.require("task")?);
    if let PmCommand::CommitBatch { commands, .. } = command {
        outcome["requested"] = commands[1]["request_assignment"]["emails"].clone();
    }
    Ok(outcome)
}

pub fn run_progress(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let (command, id, revision) = progress(inputs)?;
    send(inputs, &command, inputs.require("task")?, &id, revision)
}

pub fn render(data: &Value) -> String {
    format!(
        "saved {} in {} · revision {}\n  command {} · base {}\n{}",
        data["taskId"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        data["committedRevision"],
        data["commandId"].as_str().unwrap_or("?"),
        data["baseRevision"],
        super::warnings(data)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inputs(command: &Command, args: &[&str]) -> Inputs {
        ds_cli_contract::args::parse(
            command,
            &args.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
        )
        .expect("valid flags")
    }
    #[test]
    fn subdivision_sends_one_stable_child_and_request_in_one_revision() {
        let input = inputs(
            &SUBDIVIDE,
            &[
                "--project",
                "fixture-project",
                "--task",
                "parent",
                "--title",
                "Inspect crossing",
                "--request",
                "FIELD@example.com",
                "--request",
                "field@example.com",
                "--id",
                "crossing-0001",
                "--base-revision",
                "7",
            ],
        );
        let (
            PmCommand::CommitBatch {
                command_id,
                base_revision,
                commands,
            },
            child,
            _,
            _,
        ) = subdivision(&input).unwrap()
        else {
            panic!("expected one atomic batch")
        };
        assert_eq!(command_id, "crossing-0001");
        assert_eq!(base_revision, 7);
        assert_eq!(child, "child-crossing-0001");
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0]["create_task"]["parent_id"], "parent");
        assert_eq!(commands[0]["create_task"]["schedule_state"], "unscheduled");
        assert_eq!(commands[1]["request_assignment"]["task_id"], child);
        assert_eq!(
            commands[1]["request_assignment"]["emails"],
            json!(["field@example.com"])
        );
        assert_eq!(subdivision(&input).unwrap().1, child);
    }
    #[test]
    fn progress_sends_the_existing_progress_command_and_refuses_invalid_numbers() {
        for (percent, valid) in [
            ("50", true),
            ("100", true),
            ("NaN", false),
            ("inf", false),
            ("101", false),
            ("-1", false),
        ] {
            let input = inputs(
                &PROGRESS,
                &[
                    "--project",
                    "fixture-project",
                    "--task",
                    "child",
                    "--percent",
                    percent,
                    "--id",
                    "progress-0001",
                    "--base-revision",
                    "9",
                ],
            );
            let result = progress(&input);
            assert_eq!(result.is_ok(), valid, "{percent}");
            if percent == "50" {
                let (PmCommand::Commit { command, .. }, _, _) = result.unwrap() else {
                    panic!("single progress command")
                };
                assert_eq!(command["set_progress"]["progress"], 0.5);
            }
        }
    }
}
