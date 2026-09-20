//! `ds pm task assign` — ask people to take the work, or transfer it.
//!
//! Assignment in this product is a REQUEST, not a decree, and the CLI must not
//! quietly flatten that. Asking several people is the normal case — whoever is
//! free accepts first and the engine arbitrates — and the current holder is
//! untouched while a request is open, so asking can never orphan work.
//!
//! `--owner` is the other, rarer thing: a direct transfer of accountability,
//! which keeps the former holder as a collaborator rather than removing them
//! from work they know about.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_management::writes::{self, Assignment};
use serde_json::{Map, Value, json};

use crate::{LANE_ARG, TASK_ARG};

const REQUEST_ARG: Arg = Arg {
    name: "request",
    kind: ArgKind::Repeated,
    value: "<email>",
    required: false,
    default: None,
    choices: &[],
    summary: "Ask this person to take the work. Repeat; the first to accept holds it.",
};

const OWNER_ARG: Arg = Arg {
    name: "owner",
    kind: ArgKind::Value,
    value: "<email>",
    required: false,
    default: None,
    choices: &[],
    summary: "Transfer accountability directly; the former holder stays a collaborator.",
};

const WITHDRAW_ARG: Arg = Arg {
    name: "withdraw",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Cancel the open request. Nothing else about the task moves.",
};

const INVALID_ASSIGNMENT: Refusal = Refusal {
    code: "invalid_assignment",
    when: "none of --request, --owner or --withdraw was given, or more than one was",
    remedy: "ask with --request, transfer with --owner, or cancel with --withdraw",
};

pub const TOO_MANY_ASSIGNEES: Refusal = Refusal {
    code: "too_many_assignees",
    when: "more people were named than one request may carry",
    remedy: "ask fewer people; the refusal names the bound",
};

pub static COMMAND: Command = Command {
    id: "pm.task.assign",
    path: &["pm", "task", "assign"],
    contract: 1,
    summary: "Ask people to take a work item, or transfer it outright.",
    purpose: "\
Sends an assignment request to everyone named, leaving the current holder in \
place until somebody accepts — the engine, not this CLI, decides who wins when \
two people answer at once. Use --owner instead to transfer accountability \
directly, or --withdraw to cancel an open request. Every person named must be \
an active member of the project. Headless: commits to the selected project \
of the signed-in native credential, no window.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TASK_ARG, REQUEST_ARG, OWNER_ARG, WITHDRAW_ARG, LANE_ARG],
    output: "\
The project, the `taskId`, the `mode` that was applied — `request`, `owner` or \
`withdraw` — who is `responsible` afterwards, who is still being `requested`, \
the `committedRevision`, and any `warnings`.",
    examples: &[Example {
        command: "ds pm task assign --task T-0007 --request pilot@example.com --request field@example.com --yes",
        note: "Both are asked; the first to run `ds pm task respond --response accept` holds it.",
        runnable: false,
    }],
    refusals: &crate::write_refusals::<26>(&[
        crate::INVALID_EMAIL,
        crate::INVALID_VALUE,
        INVALID_ASSIGNMENT,
        TOO_MANY_ASSIGNEES,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &["assignee", "responsible", "owner", "delegate", "reassign"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = inputs.repeated("request");
    let owner = inputs.value("owner");
    let withdraw = inputs.switch("withdraw");

    let chosen = u8::from(!requested.is_empty()) + u8::from(owner.is_some()) + u8::from(withdraw);
    if chosen != 1 {
        return Err(Failure::invalid(
            "invalid_assignment",
            if chosen == 0 {
                "name who to ask, who to transfer to, or --withdraw"
            } else {
                "--request, --owner and --withdraw are three different intents"
            },
        )
        .remedy(INVALID_ASSIGNMENT.remedy)
        .next("ds pm task assign --help"));
    }

    let assignment = if let Some(owner) = owner {
        Assignment::Owner(crate::email(owner, "owner")?)
    } else if withdraw {
        // An empty list IS the withdrawal, and the engine reads it that way.
        Assignment::Request(Vec::new())
    } else {
        let mut people = Vec::with_capacity(requested.len());
        for raw in requested {
            let address = crate::email(raw, "request")?;
            if !people.contains(&address) {
                people.push(address);
            }
        }
        // The engine's bound is the field model's; this local copy refuses a
        // pasted distribution list before a round trip, by the same number
        // the graph would fall back to.
        if people.len() > crate::MAX_ASSIGNEES {
            return Err(crate::refused(writes::Refusal::TooManyAssignees {
                given: people.len(),
                max: crate::MAX_ASSIGNEES,
            }));
        }
        Assignment::Request(people)
    };

    let task_id = inputs.require("task")?.to_owned();
    let lane = inputs.value("lane").unwrap_or("stable");
    let read = crate::graph(lane)?;
    let (prepared, mode) =
        writes::assign_task(&read.graph, &task_id, &assignment).map_err(crate::refused)?;
    let before = read.graph.tasks.iter().find(|task| task.id == task_id);
    let result = crate::commit(lane, &prepared)?;
    let (responsible, requested) = writes::assignment_after(&result, &task_id, before);
    let mut extra = Map::new();
    extra.insert("mode".into(), json!(mode));
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
        "{} {} · revision {}\n",
        match data["mode"].as_str().unwrap_or("") {
            "owner" => "transferred",
            "withdraw" => "request withdrawn on",
            _ => "requested on",
        },
        data["taskId"].as_str().unwrap_or("?"),
        data["committedRevision"].as_u64().unwrap_or(0),
    );
    out.push_str(&format!(
        "  responsible: {}\n",
        data["responsible"].as_str().unwrap_or("unassigned"),
    ));
    if !requested.is_empty() {
        out.push_str(&format!("  asked: {}\n", requested.join(", ")));
    }
    out.push_str(&super::warnings(data));
    out
}
