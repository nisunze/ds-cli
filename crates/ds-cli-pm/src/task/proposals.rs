//! Task proposals — `ds pm task propose | request-admission | admit | decline
//! | log-hours` (ds-brain `docs/contracts/task-proposals.md`).
//!
//! A third party proposes work; the PM admits it. Every command here is
//! Server-native: it asks the native owner through
//! `ds_cli_auth::project_management`, reads the plan's revision, and sends
//! ONE governed command against it. Nothing is folded locally — the refusal a
//! caller sees is the gateway's own reason, given a code and a remedy here.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_management::Command as PmCommand;
use serde_json::{Value, json};

use crate::{LANE_ARG, TASK_ARG};

// ---------------------------------------------------------------------------
// Refusals the contract names, with the remedy each one has
// ---------------------------------------------------------------------------

pub const TASK_ALREADY_PLACED: Refusal = Refusal {
    code: "task_already_placed",
    when: "the task is already in the WBS; only an Inbox task asks for admission",
    remedy: "nothing to ask — the task is placed; read it with `ds pm plan`",
};
pub const ADMISSION_ALREADY_REQUESTED: Refusal = Refusal {
    code: "admission_already_requested",
    when: "the task already awaits the PM's decision",
    remedy: "wait for the PM; `ds pm plan` lists it under proposals",
};
pub const ADMISSION_NOT_REQUESTED: Refusal = Refusal {
    code: "admission_not_requested",
    when: "the task has no open admission request to decide",
    remedy: "the proposer asks first with `ds pm task request-admission`, or the task was already decided",
};
pub const PARENT_UNKNOWN: Refusal = Refusal {
    code: "parent_unknown",
    when: "--under names a task that is not in this project",
    remedy: "pick a parent from `ds pm plan` (or `root` for a top-level task)",
};
pub const NOT_TASK_PROPOSER: Refusal = Refusal {
    code: "not_task_proposer",
    when: "the signed-in user neither created the task nor holds it",
    remedy: "only the proposer asks; a schedule editor admits directly",
};
pub const NOT_TASK_ASSIGNEE: Refusal = Refusal {
    code: "not_task_assignee",
    when: "the signed-in user is not assigned to the task",
    remedy: "hours are the assignee's declaration; ask the PM to assign you first",
};
pub const INVALID_HOURS: Refusal = Refusal {
    code: "invalid_hours",
    when: "--hours or --days is negative, not a number, or past its bound",
    remedy: "pass e.g. --hours 12 --days 3; `ds pm plan` publishes the bounds in vocabulary",
};
pub const TASK_NOT_FOUND: Refusal = Refusal {
    code: "task_not_found",
    when: "--task names a task that is not in this project",
    remedy: "take the id from `ds pm plan` proposals or `ds pm task list`",
};
pub const TASK_IS_MILESTONE: Refusal = Refusal {
    code: "task_is_milestone",
    when: "hours were logged on a milestone",
    remedy: "log them on the work under it",
};
pub const HOURS_ENTRY_EXISTS: Refusal = Refusal {
    code: "hours_entry_exists",
    when: "--id names an entry already on the task under another command",
    remedy: "mint a new --id, or omit it",
};
pub const BOUND_EXCEEDED: Refusal = Refusal {
    code: "bound_exceeded",
    when: "the note or reason is over 300 characters, or the task carries 500 entries",
    remedy: "shorten the text; the bound travels in the refusal detail",
};
pub const INVALID_COMMAND_ID: Refusal = Refusal {
    code: "invalid_command_id",
    when: "--id is not 8 to 128 letters, digits, `-` or `_` starting with a letter or digit",
    remedy: "pass e.g. --id hours-2026-09-20-a, or omit it and keep the minted id from the receipt",
};
pub const TITLE_REQUIRED: Refusal = Refusal {
    code: "title_required",
    when: "--title is blank",
    remedy: "say what the work is",
};
pub const REASON_REQUIRED: Refusal = Refusal {
    code: "reason_required",
    when: "--reason is blank",
    remedy: "say why, so the proposer can revise and ask again",
};
pub const PROPOSAL_REQUESTED_LATER: Refusal = Refusal {
    code: "proposal_created_not_requested",
    when: "the task was created but the admission request was refused",
    remedy: "run `ds pm task request-admission --task <id> --yes`; the id is in detail",
};

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

const HOURS_ARG: Arg = Arg {
    name: "hours",
    kind: ArgKind::Value,
    value: "<n>",
    required: true,
    default: None,
    choices: &[],
    summary: "Hours, a non-negative number (12, 4.5).",
};
const DAYS_ARG: Arg = Arg {
    name: "days",
    kind: ArgKind::Value,
    value: "<n>",
    required: false,
    default: None,
    choices: &[],
    summary: "Estimated duration in whole days, stated before any schedule exists.",
};
const NOTE_ARG: Arg = Arg {
    name: "note",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "A short note (≤ 300 characters), kept on the task.",
};
const ID_ARG: Arg = Arg {
    name: "id",
    kind: ArgKind::Value,
    value: "<command-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "Mint this command id. Reuse it to retry after a lost answer; the same id replays, never repeats.",
};
const TITLE_ARG: Arg = Arg {
    name: "title",
    kind: ArgKind::Value,
    value: "<text>",
    required: true,
    default: None,
    choices: &[],
    summary: "What the work is.",
};
const DESCRIPTION_ARG: Arg = Arg {
    name: "description",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "What done looks like.",
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
const UNDER_ARG: Arg = Arg {
    name: "under",
    kind: ArgKind::Value,
    value: "<parent-task-id|root>",
    required: true,
    default: None,
    choices: &[],
    summary: "Where the task lands in the plan: a parent task's id, or `root` for a top-level task.",
};
const POSITION_ARG: Arg = Arg {
    name: "position",
    kind: ArgKind::Value,
    value: "<index>",
    required: false,
    default: None,
    choices: &[],
    summary: "Zero-based place among the new siblings; omitted appends.",
};
const REASON_ARG: Arg = Arg {
    name: "reason",
    kind: ArgKind::Value,
    value: "<text>",
    required: true,
    default: None,
    choices: &[],
    summary: "Why (≤ 300 characters), kept on the task so the proposer can revise and ask again.",
};

// ---------------------------------------------------------------------------
// Descriptors
// ---------------------------------------------------------------------------

pub static PROPOSE: Command = Command {
    id: "pm.task.propose",
    path: &["pm", "task", "propose"],
    contract: 1,
    summary: "Propose your own work: an Inbox task with an estimate the PM admits.",
    purpose: "\
A third party proposes work; the PM admits it. This creates one Inbox task \
about your own work — you are its responsible person — with the hours (and \
optionally days) you estimate, then asks the project's schedule editors to \
admit it into the plan. Two governed commands, one gesture: if the request \
is refused after the create succeeded, the receipt names the task and the \
next step. The estimate and the hours logged against it later are the facts \
billing and duration learning read.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TITLE_ARG,
        HOURS_ARG,
        DAYS_ARG,
        DESCRIPTION_ARG,
        DISCIPLINE_ARG,
        NOTE_ARG,
        ID_ARG,
        LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "\
`project`, the minted `taskId`, `commandId`, `committedRevision`, `admission` \
(`requested`), `estimatedHours`, `estimatedDays`, `responsible`, any engine \
`warnings`, and `link`.",
    examples: &[Example {
        command: "ds pm task propose --title \"Survey the Kabuga feeder extension\" --hours 12 --days 3 --note \"site visit + as-built sketch\" --yes --project <exact-id>",
        note: "Without --yes dispatch refuses before anything is sent.",
        runnable: false,
    }],
    refusals: &crate::write_refusals::<27>(&[
        TITLE_REQUIRED,
        INVALID_HOURS,
        BOUND_EXCEEDED,
        INVALID_COMMAND_ID,
        PROPOSAL_REQUESTED_LATER,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "third party",
        "consultant",
        "contractor",
        "proposal",
        "hours",
        "days",
        "billing",
        "admission",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static REQUEST_ADMISSION: Command = Command {
    id: "pm.task.request-admission",
    path: &["pm", "task", "request-admission"],
    contract: 1,
    summary: "Ask the PM to admit your Inbox task into the plan.",
    purpose: "\
For a task you created in the Inbox elsewhere (the app, `ds pm task create \
--kind inbox`) or one that was declined and revised: records the request on \
the task and notifies the project's schedule editors. Refused while a request \
is already open, and on a task already placed in the plan.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TASK_ARG, NOTE_ARG, ID_ARG, LANE_ARG, crate::PROJECT_ARG],
    output: "`project`, `taskId`, `commandId`, `committedRevision`, `admission` (`requested`), `warnings`, `link`.",
    examples: &[Example {
        command: "ds pm task request-admission --task T-0031 --note \"revised to 6 h\" --yes --project <exact-id>",
        note: "The PM answers with `ds pm task admit` or `ds pm task decline`.",
        runnable: false,
    }],
    refusals: &crate::write_refusals::<27>(&[
        TASK_ALREADY_PLACED,
        ADMISSION_ALREADY_REQUESTED,
        NOT_TASK_PROPOSER,
        BOUND_EXCEEDED,
        INVALID_COMMAND_ID,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &["proposal", "submit", "resubmit"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static ADMIT: Command = Command {
    id: "pm.task.admit",
    path: &["pm", "task", "admit"],
    contract: 1,
    summary: "Admit a proposed Inbox task into the plan, under a parent or at root.",
    purpose: "\
The PM's yes. One command moves the task (and anything under it) out of the \
Inbox into the WBS — the same reviewed reparent a drag performs — and marks \
the admission `admitted`; the proposer is notified. Identities, assignment, \
the estimate and any logged hours travel with the task. Requires \
schedule-editor access.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TASK_ARG,
        UNDER_ARG,
        POSITION_ARG,
        ID_ARG,
        LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "`project`, `taskId`, `parent` (empty for root), `commandId`, `committedRevision`, `admission` (`admitted`), `warnings`, `link`.",
    examples: &[Example {
        command: "ds pm task admit --task T-0031 --under T-0004 --yes --project <exact-id>",
        note: "`--under root` admits it as a top-level task.",
        runnable: false,
    }],
    refusals: &crate::write_refusals::<26>(&[
        ADMISSION_NOT_REQUESTED,
        PARENT_UNKNOWN,
        crate::INVALID_NUMBER,
        INVALID_COMMAND_ID,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &["proposal", "admission", "approve", "accept", "wbs"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static DECLINE: Command = Command {
    id: "pm.task.decline",
    path: &["pm", "task", "decline"],
    contract: 1,
    summary: "Decline a proposed task with a reason; it stays in the Inbox.",
    purpose: "\
The PM's no. The task keeps its place in the Inbox with the admission marked \
`declined` and the reason on it; the proposer is notified and may revise the \
estimate and ask again. Requires schedule-editor access.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TASK_ARG, REASON_ARG, ID_ARG, LANE_ARG, crate::PROJECT_ARG],
    output: "`project`, `taskId`, `commandId`, `committedRevision`, `admission` (`declined`), `warnings`, `link`.",
    examples: &[Example {
        command: "ds pm task decline --task T-0031 --reason \"out of scope this phase\" --yes --project <exact-id>",
        note: "The proposer sees the reason on the task.",
        runnable: false,
    }],
    refusals: &crate::write_refusals::<26>(&[
        ADMISSION_NOT_REQUESTED,
        REASON_REQUIRED,
        BOUND_EXCEEDED,
        INVALID_COMMAND_ID,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &["proposal", "admission", "reject", "refuse"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static LOG_HOURS: Command = Command {
    id: "pm.task.log-hours",
    path: &["pm", "task", "log-hours"],
    contract: 1,
    summary: "Log hours you worked on a task you are assigned to.",
    purpose: "\
Appends one entry — who, when, how many hours, an optional note — to the \
task's hours log; the task's `actualHours` is the server's sum. The log is \
append-only and only an assignee writes it: these entries are the facts \
billing reads, so nothing edits or deletes one here. Reuse --id to retry a \
lost answer without logging twice.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TASK_ARG,
        HOURS_ARG,
        NOTE_ARG,
        ID_ARG,
        LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "`project`, `taskId`, `commandId` (also the entry id), `committedRevision`, `hours`, `actualHours`, `estimatedHours`, `warnings`, `link`.",
    examples: &[Example {
        command: "ds pm task log-hours --task T-0031 --hours 4 --note \"first day on site\" --yes --project <exact-id>",
        note: "`ds pm plan` flags the task once the sum passes its estimate.",
        runnable: false,
    }],
    refusals: &crate::write_refusals::<27>(&[
        NOT_TASK_ASSIGNEE,
        TASK_IS_MILESTONE,
        INVALID_HOURS,
        HOURS_ENTRY_EXISTS,
        BOUND_EXCEEDED,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &["timesheet", "billing", "effort", "actual"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

/// The plan's head, read once before a write so the command carries the
/// revision it was authored against — the same fence every Plan-sheet write
/// carries. The user's email rides along for the one command whose subject is
/// the caller.
struct Head {
    project: String,
    revision: i64,
    user_email: String,
    max_estimated_hours: i64,
    max_estimated_days: i64,
    max_hours_per_entry: i64,
}

fn head(lane: &str, project: &str) -> Result<Head, Failure> {
    let report = ds_cli_auth::project_management_for_project(lane, project, &PmCommand::Graph)?;
    let project = report.project_id().to_owned();
    let user_email = report.user_email().to_owned();
    let graph = report.into_result();
    let bound = |key: &str| graph["field_model"][key].as_i64().unwrap_or(0);
    Ok(Head {
        project,
        revision: graph["revision"].as_i64().unwrap_or(0),
        user_email,
        max_estimated_hours: bound("max_estimated_hours"),
        max_estimated_days: bound("max_estimated_days"),
        max_hours_per_entry: bound("max_hours_per_entry"),
    })
}

/// A non-negative hours number, refused locally when the plan published a
/// bound it exceeds; the server bounds again.
fn hours(raw: &str, flag: &str, bound: i64) -> Result<f64, Failure> {
    let refuse = |what: &str| {
        Failure::invalid(INVALID_HOURS.code, format!("`--{flag}` {what}"))
            .remedy(INVALID_HOURS.remedy)
            .detail(json!({ "given": raw, "bound": bound }))
    };
    let value: f64 = raw.trim().parse().map_err(|_| refuse("must be a number"))?;
    if !value.is_finite() || value < 0.0 {
        return Err(refuse("must be a non-negative number"));
    }
    if bound > 0 && value > bound as f64 {
        return Err(refuse(&format!("is over the project's bound of {bound}")));
    }
    Ok(value)
}

fn days(raw: &str, bound: i64) -> Result<i64, Failure> {
    let refuse = |what: &str| {
        Failure::invalid(INVALID_HOURS.code, format!("`--days` {what}"))
            .remedy(INVALID_HOURS.remedy)
            .detail(json!({ "given": raw, "bound": bound }))
    };
    let value: i64 = raw
        .trim()
        .parse()
        .map_err(|_| refuse("must be a whole number"))?;
    if value < 0 {
        return Err(refuse("must be non-negative"));
    }
    if bound > 0 && value > bound {
        return Err(refuse(&format!("is over the project's bound of {bound}")));
    }
    Ok(value)
}

/// The command id: `--id` as given, else one minted here and printed in the
/// receipt so a lost answer can be retried against the same ledger entry.
fn command_id(inputs: &Inputs, prefix: &str) -> Result<String, Failure> {
    match inputs.value("id") {
        Some(given) => {
            let given = given.trim();
            let shaped = (8..=128).contains(&given.len())
                && given
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                && given.as_bytes()[0].is_ascii_alphanumeric();
            if !shaped {
                return Err(Failure::invalid(
                    INVALID_COMMAND_ID.code,
                    "`--id` must be 8 to 128 letters, digits, `-` or `_`, starting with a letter or digit",
                )
                .remedy(INVALID_COMMAND_ID.remedy));
            }
            Ok(given.to_owned())
        }
        None => Ok(format!("{prefix}-{}", nonce())),
    }
}

/// Twenty hex characters of OS randomness. Not a UUID crate: the id only has
/// to be unique among one project's commands.
fn nonce() -> String {
    let mut bytes = [0u8; 10];
    let mut file = std::fs::File::open("/dev/urandom").ok();
    if let Some(file) = file.as_mut() {
        use std::io::Read;
        let _ = file.read_exact(&mut bytes);
    }
    if bytes.iter().all(|byte| *byte == 0) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        return format!("{stamp:020x}");
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A text flag bounded as the contract bounds it, refused here before the
/// round trip.
fn bounded_text(inputs: &Inputs, flag: &str) -> Result<Option<String>, Failure> {
    match inputs.value(flag) {
        None => Ok(None),
        Some(raw) => {
            let text = raw.trim();
            if text.chars().count() > 300 {
                return Err(Failure::invalid(
                    BOUND_EXCEEDED.code,
                    format!("`--{flag}` is over 300 characters"),
                )
                .remedy(BOUND_EXCEEDED.remedy)
                .detail(json!({ "what": flag, "bound": 300 })));
            }
            if text.is_empty() {
                Ok(None)
            } else {
                Ok(Some(text.to_owned()))
            }
        }
    }
}

/// Send one governed write and read its result. The gateway answers `200
/// {result}` for an applied command AND for an engine refusal (`applied:
/// false` with `violations`); a named refusal arrives as a non-200 the auth
/// crate turned into a failure carrying `service_code`.
fn send(lane: &str, project: &str, command: &PmCommand) -> Result<Value, Failure> {
    let report =
        ds_cli_auth::project_management_for_project(lane, project, command).map_err(classify)?;
    let data = report.into_result();
    let result = data.get("result").cloned().unwrap_or(data);
    if result["applied"] == false {
        let violations = result["violations"].clone();
        let first = violations[0].clone();
        return Err(Failure::invalid(
            crate::PM_REFUSED.code,
            first["message"]
                .as_str()
                .unwrap_or("Project Work declined the command")
                .to_owned(),
        )
        .remedy(crate::PM_REFUSED.remedy)
        .detail(json!({ "violations": violations })));
    }
    Ok(result)
}

/// Give the gateway's named reason its own code and remedy, and the two
/// conditions every write shares — "you may not" and "you were too late" —
/// theirs. Each arm names the code it emits beside the `Refusal` that
/// documents it, in the class the contract's status gives it.
fn classify(failure: Failure) -> Failure {
    let detail = failure.detail_value().cloned().unwrap_or(Value::Null);
    let status = detail["http_status"].as_u64().unwrap_or(0);
    let code = detail["service_code"].as_str().unwrap_or("").to_owned();
    let sentence = |fallback: &str| {
        detail["service_message"]
            .as_str()
            .filter(|message| !message.is_empty())
            .unwrap_or(fallback)
            .to_owned()
    };
    let with =
        |built: Failure, refusal: &Refusal| built.remedy(refusal.remedy).detail(detail.clone());
    match (status, code.as_str()) {
        (_, "task_already_placed") => with(
            Failure::conflict(TASK_ALREADY_PLACED.code, sentence(TASK_ALREADY_PLACED.when)),
            &TASK_ALREADY_PLACED,
        ),
        (_, "admission_already_requested") => with(
            Failure::conflict(
                ADMISSION_ALREADY_REQUESTED.code,
                sentence(ADMISSION_ALREADY_REQUESTED.when),
            ),
            &ADMISSION_ALREADY_REQUESTED,
        ),
        (_, "admission_not_requested") => with(
            Failure::invalid(
                ADMISSION_NOT_REQUESTED.code,
                sentence(ADMISSION_NOT_REQUESTED.when),
            ),
            &ADMISSION_NOT_REQUESTED,
        ),
        (_, "parent_unknown") => with(
            Failure::invalid(PARENT_UNKNOWN.code, sentence(PARENT_UNKNOWN.when)),
            &PARENT_UNKNOWN,
        ),
        (_, "not_task_proposer") => with(
            Failure::unauthorized(NOT_TASK_PROPOSER.code, sentence(NOT_TASK_PROPOSER.when)),
            &NOT_TASK_PROPOSER,
        ),
        (_, "not_task_assignee") => with(
            Failure::unauthorized(NOT_TASK_ASSIGNEE.code, sentence(NOT_TASK_ASSIGNEE.when)),
            &NOT_TASK_ASSIGNEE,
        ),
        (_, "invalid_hours") => with(
            Failure::invalid(INVALID_HOURS.code, sentence(INVALID_HOURS.when)),
            &INVALID_HOURS,
        ),
        (_, "task_not_found") => with(
            Failure::invalid(TASK_NOT_FOUND.code, sentence(TASK_NOT_FOUND.when)),
            &TASK_NOT_FOUND,
        ),
        (_, "task_is_milestone") => with(
            Failure::invalid(TASK_IS_MILESTONE.code, sentence(TASK_IS_MILESTONE.when)),
            &TASK_IS_MILESTONE,
        ),
        (_, "hours_entry_exists") => with(
            Failure::conflict(HOURS_ENTRY_EXISTS.code, sentence(HOURS_ENTRY_EXISTS.when)),
            &HOURS_ENTRY_EXISTS,
        ),
        (_, "bound_exceeded") => with(
            Failure::invalid(BOUND_EXCEEDED.code, sentence(BOUND_EXCEEDED.when)),
            &BOUND_EXCEEDED,
        ),
        (409, "pm_revision_conflict") => {
            Failure::conflict(crate::CONFLICT.code, crate::CONFLICT.when)
                .remedy(crate::CONFLICT.remedy)
                .next("ds pm plan")
                .detail(detail.clone())
        }
        (403, _) => Failure::unauthorized(
            crate::NOT_PERMITTED.code,
            sentence(crate::NOT_PERMITTED.when),
        )
        .remedy(crate::NOT_PERMITTED.remedy)
        .next("ds pm plan")
        .detail(detail.clone()),
        _ => failure,
    }
}

fn link(project: &str, task_id: &str) -> String {
    format!("/projects/{project}/operations?view=plan&task={task_id}&panel=context")
}

fn task_patch<'a>(result: &'a Value, task_id: &str) -> Option<&'a Value> {
    result["task_patches"]
        .as_array()?
        .iter()
        .find(|patch| patch["id"] == task_id)
}

fn admission_state(result: &Value, task_id: &str) -> Value {
    task_patch(result, task_id)
        .and_then(|patch| patch["data"]["admission"]["state"].as_str())
        .map(Value::from)
        .unwrap_or(Value::Null)
}

fn receipt(head: &Head, task_id: &str, command_id: &str, result: &Value) -> Value {
    json!({
        "project": head.project,
        "taskId": task_id,
        "commandId": command_id,
        "committedRevision": result["committed_revision"],
        "admission": admission_state(result, task_id),
        "warnings": result["warnings"].as_array().cloned().unwrap_or_default(),
        "link": link(&head.project, task_id),
    })
}

pub fn run_propose(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.value("lane").unwrap_or("stable");
    let head = head(lane, inputs.require("project")?)?;
    let title = inputs.require("title")?.trim().to_owned();
    if title.is_empty() {
        return Err(
            Failure::invalid("title_required", "`--title` cannot be empty")
                .remedy("say what the work is"),
        );
    }
    let estimated_hours = hours(inputs.require("hours")?, "hours", head.max_estimated_hours)?;
    let estimated_days = match inputs.value("days") {
        Some(raw) => Some(days(raw, head.max_estimated_days)?),
        None => None,
    };
    let note = bounded_text(inputs, "note")?;
    let description = bounded_text(inputs, "description")?;
    let command_id = command_id(inputs, "propose")?;
    // The task id is the command id: one key mints one task, and a retry
    // with the same --id replays rather than creating a second proposal.
    let task_id = command_id.clone();
    let created = send(
        lane,
        inputs.require("project")?,
        &PmCommand::Propose {
            command_id: command_id.clone(),
            base_revision: head.revision,
            task_id: task_id.clone(),
            title: title.clone(),
            description,
            discipline: inputs.value("discipline").map(str::to_owned),
            responsible_email: head.user_email.clone(),
            estimated_hours,
            estimated_days,
        },
    )?;
    let after_create = created["committed_revision"]
        .as_i64()
        .unwrap_or(head.revision + 1);
    let requested = send(
        lane,
        inputs.require("project")?,
        &PmCommand::RequestAdmission {
            command_id: format!("{command_id}-request"),
            base_revision: after_create,
            task_id: task_id.clone(),
            note,
        },
    );
    match requested {
        Ok(result) => {
            let mut out = receipt(&head, &task_id, &command_id, &result);
            out["estimatedHours"] = json!(estimated_hours);
            out["estimatedDays"] = json!(estimated_days);
            out["responsible"] = json!(head.user_email);
            out["title"] = json!(title);
            Ok(out)
        }
        Err(failure) => Err(Failure::conflict(
            PROPOSAL_REQUESTED_LATER.code,
            format!(
                "task {task_id} was created at revision {after_create}, but the admission request was refused: {}",
                failure.message()
            ),
        )
        .remedy(PROPOSAL_REQUESTED_LATER.remedy)
        .next(format!("ds pm task request-admission --task {task_id} --yes"))
        .detail(json!({
            "taskId": task_id, "committedRevision": after_create,
            "refusal": failure.code(), "refusalDetail": failure.detail_value(),
        }))),
    }
}

pub fn run_request_admission(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.value("lane").unwrap_or("stable");
    let task_id = inputs.require("task")?.trim().to_owned();
    let note = bounded_text(inputs, "note")?;
    let command_id = command_id(inputs, "request")?;
    let head = head(lane, inputs.require("project")?)?;
    let result = send(
        lane,
        inputs.require("project")?,
        &PmCommand::RequestAdmission {
            command_id: command_id.clone(),
            base_revision: head.revision,
            task_id: task_id.clone(),
            note,
        },
    )?;
    Ok(receipt(&head, &task_id, &command_id, &result))
}

pub fn run_admit(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.value("lane").unwrap_or("stable");
    let task_id = inputs.require("task")?.trim().to_owned();
    let under = inputs.require("under")?.trim().to_owned();
    let parent_task_id = if under.eq_ignore_ascii_case("root") {
        String::new()
    } else {
        under
    };
    let target_index = match inputs.value("position") {
        Some(raw) => Some(crate::integer(raw, "position", 0, 100_000)?),
        None => None,
    };
    let command_id = command_id(inputs, "admit")?;
    let head = head(lane, inputs.require("project")?)?;
    let result = send(
        lane,
        inputs.require("project")?,
        &PmCommand::Admit {
            command_id: command_id.clone(),
            base_revision: head.revision,
            task_id: task_id.clone(),
            parent_task_id: parent_task_id.clone(),
            target_index,
        },
    )?;
    let mut out = receipt(&head, &task_id, &command_id, &result);
    out["parent"] = json!(parent_task_id);
    Ok(out)
}

pub fn run_decline(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.value("lane").unwrap_or("stable");
    let task_id = inputs.require("task")?.trim().to_owned();
    let reason = bounded_text(inputs, "reason")?.ok_or_else(|| {
        Failure::invalid(REASON_REQUIRED.code, "`--reason` cannot be empty")
            .remedy(REASON_REQUIRED.remedy)
    })?;
    let command_id = command_id(inputs, "decline")?;
    let head = head(lane, inputs.require("project")?)?;
    let result = send(
        lane,
        inputs.require("project")?,
        &PmCommand::DeclineAdmission {
            command_id: command_id.clone(),
            base_revision: head.revision,
            task_id: task_id.clone(),
            reason,
        },
    )?;
    Ok(receipt(&head, &task_id, &command_id, &result))
}

pub fn run_log_hours(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.value("lane").unwrap_or("stable");
    let task_id = inputs.require("task")?.trim().to_owned();
    let note = bounded_text(inputs, "note")?;
    let command_id = command_id(inputs, "hours")?;
    let head = head(lane, inputs.require("project")?)?;
    let logged = hours(inputs.require("hours")?, "hours", head.max_hours_per_entry)?;
    let result = send(
        lane,
        inputs.require("project")?,
        &PmCommand::LogHours {
            command_id: command_id.clone(),
            base_revision: head.revision,
            task_id: task_id.clone(),
            hours: logged,
            note,
        },
    )?;
    let mut out = receipt(&head, &task_id, &command_id, &result);
    out["hours"] = json!(logged);
    if let Some(patch) = task_patch(&result, &task_id) {
        out["actualHours"] = patch["computed"]["actual_hours"].clone();
        out["estimatedHours"] = patch["data"]["estimated_hours"].clone();
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Renders
// ---------------------------------------------------------------------------

fn render_admission(verb: &str, data: &Value) -> String {
    let mut out = format!(
        "{verb} {} in {} · admission {} · revision {} · command {}\n",
        data["taskId"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        data["admission"].as_str().unwrap_or("—"),
        data["committedRevision"].as_u64().unwrap_or(0),
        data["commandId"].as_str().unwrap_or("?"),
    );
    out.push_str(&super::warnings(data));
    if let Some(link) = data["link"].as_str() {
        out.push_str(&format!("  {link}\n"));
    }
    out
}

pub fn render_propose(data: &Value) -> String {
    let mut out = render_admission("proposed", data);
    out.insert_str(
        0,
        &format!(
            "estimate {} h{} · responsible {}\n",
            data["estimatedHours"].as_f64().unwrap_or(0.0),
            data["estimatedDays"]
                .as_i64()
                .map(|days| format!(" / {days} d"))
                .unwrap_or_default(),
            data["responsible"].as_str().unwrap_or("?"),
        ),
    );
    out
}

pub fn render_request_admission(data: &Value) -> String {
    render_admission("requested admission of", data)
}

pub fn render_admit(data: &Value) -> String {
    let mut out = render_admission("admitted", data);
    out.push_str(&format!(
        "  under {}\n",
        data["parent"]
            .as_str()
            .filter(|parent| !parent.is_empty())
            .unwrap_or("root"),
    ));
    out
}

pub fn render_decline(data: &Value) -> String {
    render_admission("declined", data)
}

pub fn render_log_hours(data: &Value) -> String {
    let mut out = format!(
        "logged {} h on {} in {} · actual {} h{} · revision {} · entry {}\n",
        data["hours"].as_f64().unwrap_or(0.0),
        data["taskId"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        data["actualHours"].as_f64().unwrap_or(0.0),
        data["estimatedHours"]
            .as_f64()
            .map(|estimate| format!(" of {estimate} h estimated"))
            .unwrap_or_default(),
        data["committedRevision"].as_u64().unwrap_or(0),
        data["commandId"].as_str().unwrap_or("?"),
    );
    out.push_str(&super::warnings(data));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hours_and_days_are_refused_locally_when_the_plan_published_a_bound() {
        assert_eq!(hours("12", "hours", 100_000).expect("valid"), 12.0);
        assert_eq!(hours("4.5", "hours", 0).expect("no bound published"), 4.5);
        for bad in ["-1", "abc", "", "inf", "NaN"] {
            assert_eq!(
                hours(bad, "hours", 100).expect_err("must refuse").code(),
                "invalid_hours",
                "`{bad}` was accepted"
            );
        }
        assert_eq!(
            hours("101", "hours", 100).expect_err("over bound").code(),
            "invalid_hours"
        );
        assert_eq!(days("3", 3650).expect("valid"), 3);
        for bad in ["-1", "1.5", "x", "3651"] {
            assert_eq!(
                days(bad, 3650).expect_err("must refuse").code(),
                "invalid_hours"
            );
        }
    }

    #[test]
    fn a_minted_command_id_is_shaped_for_the_ledger_and_a_given_one_is_checked() {
        let minted = nonce();
        assert_eq!(minted.len(), 20);
        assert!(minted.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(nonce(), minted, "two mints differ");
    }

    #[test]
    fn the_gateways_reason_becomes_the_commands_own_code_and_remedy() {
        let refused =
            |status: u64, code: &str| {
                classify(Failure::failed(TASK_NOT_FOUND.code, "refused").detail(json!({
                "http_status": status, "service_code": code, "service_message": "the sentence"
            })))
            };
        assert_eq!(
            refused(409, "task_already_placed").code(),
            "task_already_placed"
        );
        assert_eq!(
            refused(403, "not_task_proposer").code(),
            "not_task_proposer"
        );
        assert_eq!(refused(404, "parent_unknown").code(), "parent_unknown");
        assert_eq!(
            refused(400, "admission_not_requested").code(),
            "admission_not_requested"
        );
        assert_eq!(
            refused(409, "pm_revision_conflict").code(),
            "work_revision_conflict"
        );
        assert_eq!(refused(403, "forbidden").code(), "work_not_permitted");
        // An unnamed refusal keeps what the auth crate said.
        assert_eq!(refused(500, "").code(), TASK_NOT_FOUND.code);
        assert!(
            refused(409, "task_already_placed")
                .message()
                .contains("the sentence")
        );
        assert!(refused(409, "task_already_placed").remedy_text().is_some());
    }
}
