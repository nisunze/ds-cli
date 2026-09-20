//! `ds pm` — Project Management: the plan, its tasks, the assignment loop and
//! the record of what happened.
//!
//! ## Why this domain is headless
//!
//! Project Management is governed shared state. Its graph lives behind
//! ds-brain, which is the only gateway and the only authority: it decides who
//! may write, arbitrates two people accepting the same request in the same
//! second, and refuses a command authored against a revision that has moved.
//! `POST /api/v1/pm` is published on both gateway lanes and authenticates from
//! the bearer alone — no pairing, no device, no window. So every command here
//! is one governed action the native client sends under the restored user or
//! device credential, for the audience-fenced project `ds auth project use`
//! selected. Until 2026-09-20 eight of the nine relayed through the paired
//! desktop to the page's own adapters instead; on a server with no window the
//! owner could file nothing. The window was habit, never contract.
//!
//! `ds` therefore carries no copy of the rules: what a graph MEANS — the plan,
//! a task list, one task, which command a flag becomes — is decided once in
//! `ds_command_kernel::project_management`, for the CLI, MCP, the Server and
//! the page alike.
//!
//! ## What the family is
//!
//! ```text
//!   plan → task list → task read → task create | update | assign | respond
//!                                  record list → record read
//! ```
//!
//! Reads are bounded projections of the same canonical graph the Plan,
//! Dashboard, Board, Gantt, Table and Records surfaces render — the CLI has
//! no second model and computes no second answer. Writes are ordinary project
//! commands: one optimistic base revision, applied atomically or refused
//! whole, with the refusal naming what it refused.
//!
//! ## What is deliberately absent
//!
//! **A messaging door.** Assigning work, answering a request and changing a
//! delivery state all *cause* notifications, and they flow through the
//! canonical spine as side effects of the governed action. What `ds` cannot
//! do is send a message: `messages-v1` is human-only, and a domain that could
//! compose one would be the same mistake as a domain that could run code
//! inside the application.
//!
//! **A window path.** `--desktop-descriptor` is not an input of any `ds pm`
//! command; a caller that still passes it is told `requires_window_retired`
//! by the parser, with the remedy of dropping the flag.

pub mod geometry;
pub mod plan;
pub mod record;
pub mod task;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Domain, Refusal};
use ds_command_kernel::project_management::{CanonicalProjectGraph, reads, writes};
use serde_json::{Value, json};

// Neutral argument helpers: a numeric bound and an English count say
// nothing about a transport, so they come from the contract crate.
pub use ds_cli_contract::args::{INVALID_NUMBER, integer, plural};

pub static DOMAIN: Domain = Domain {
    id: "pm",
    summary: "Tasks, milestones, records and the plan they sit in.",
    commands: &[
        &plan::COMMAND,
        &task::list::COMMAND,
        &task::read::COMMAND,
        &task::create::COMMAND,
        &task::update::COMMAND,
        &task::assign::COMMAND,
        &task::respond::COMMAND,
        &geometry::read::COMMAND,
        &geometry::set::COMMAND,
        &geometry::clear::COMMAND,
        &record::list::COMMAND,
        &record::read::COMMAND,
    ],
};

// ---------------------------------------------------------------------------
// Bounds — the kernel's, republished so help and the parser say one number
// ---------------------------------------------------------------------------

/// The engine's bound on the people one assignment request may name when the
/// graph predates the field model (`fieldModel.maxAssignees` is the
/// authority). Republished so `--help` can state it.
pub const MAX_ASSIGNEES: usize = writes::DEFAULT_MAX_ASSIGNEES;

/// The largest page of tasks or records one read returns. The total is
/// always reported, so a truncated page is never silent.
pub const MAX_PAGE_SIZE: i64 = reads::MAX_PAGE_SIZE;

/// The largest related collection returned by a detail read.
pub const MAX_RELATED_ROWS: usize = reads::MAX_RELATED_ROWS;

/// The most context rows per collection one record read fetches — ds-brain's
/// own bound on `get_context`. A project holding more records than this lists
/// them with `truncated: true`; the correspondence contract's server-side
/// `record_list` is the door past it.
pub const MAX_CONTEXT_ROWS: i64 = ds_client_core::project_management::MAX_CONTEXT_LIMIT;

// ---------------------------------------------------------------------------
// Refusals — the headless project set every `ds pm` command shares, plus
// this domain's own
// ---------------------------------------------------------------------------

/// Which native credential lane a `ds pm` command authenticates on.
pub const LANE_ARG: Arg = Arg::value("lane", "<stable|canary>", "Native credential lane.")
    .choices(&["stable", "canary"])
    .default("stable");

/// The refusals the headless project client can answer with, for every
/// command of this domain: profile, state, session, identity, transport and
/// project-context conditions. Declared once in `ds auth`.
pub const HEADLESS_REFUSALS: &[Refusal] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals;

/// The graph reached this build but the kernel could not fold it.
///
/// Distinct from `pm_refused`: the server answered, so the project and the
/// permission were fine. Something in the payload is a shape this build
/// does not understand, which is a contract break rather than an operator
/// mistake.
pub const PLAN_UNREADABLE: Refusal = Refusal {
    code: "plan_unreadable",
    when: "the project's plan is a shape this build cannot fold",
    remedy: "report this with the project id; the CLI and the server disagree about the graph",
};

/// ds-brain declined the command by name.
pub const PM_REFUSED: Refusal = ds_cli_auth::PM_REFUSED_REFUSAL;

pub const NOT_PERMITTED: Refusal = Refusal {
    code: "work_not_permitted",
    when: "the signed-in user may read this project's plan but not change it",
    remedy: "ask a project admin for schedule-editor access",
};
pub const CONFLICT: Refusal = Refusal {
    code: "work_revision_conflict",
    when: "the plan moved while the command was in flight",
    remedy: "re-read with `ds pm task read` and issue the command again",
};
pub const TASK_NOT_FOUND: Refusal = Refusal {
    code: "task_not_found",
    when: "no task or milestone in the selected project's plan carries this id",
    remedy: "check the id with `ds pm task list`",
};
pub const RECORD_NOT_FOUND: Refusal = Refusal {
    code: "record_not_found",
    when: "no record in the fetched context carries this id",
    remedy: "check the id with `ds pm record list`",
};
pub const INVALID_DATE: Refusal = Refusal {
    code: "invalid_date",
    when: "a schedule flag is not a calendar date in YYYY-MM-DD form",
    remedy: "pass e.g. --start 2026-09-01",
};
pub const INVALID_EMAIL: Refusal = Refusal {
    code: "invalid_email",
    when: "a person flag is not an email address",
    remedy: "pass the project member's email, e.g. --request pilot@example.com",
};
pub const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not given for a command that changes the project's plan",
    remedy: "re-run with --yes once you intend the change",
};
/// A value the engine's own vocabulary for this project does not hold.
pub const INVALID_VALUE: Refusal = Refusal {
    code: "invalid_choice",
    when: "a state, priority, type, placement or scheduling flag is outside the vocabulary `ds pm plan` publishes for this project, or a text flag is empty or over its bound",
    remedy: "read .data.vocabulary from `ds pm plan --output json` and pass one of its values",
};
/// The host could not mint an idempotency key for the commit.
pub const RNG_UNAVAILABLE: Refusal = ds_cli_auth::device::RNG_UNAVAILABLE;

/// The refusals every read of this domain declares: the headless set, the
/// fold's own, then the command's own. `TOTAL` is `16 + own.len()`, checked
/// at compile time — const generics cannot add, so the caller states it.
pub const fn read_refusals<const TOTAL: usize>(own: &[Refusal]) -> [Refusal; TOTAL] {
    assert!(TOTAL == HEADLESS_REFUSALS.len() + 1 + own.len());
    let mut out = [PLAN_UNREADABLE; TOTAL];
    let mut i = 0;
    while i < HEADLESS_REFUSALS.len() {
        out[i] = HEADLESS_REFUSALS[i];
        i += 1;
    }
    out[i] = PLAN_UNREADABLE;
    i += 1;
    let mut j = 0;
    while j < own.len() {
        out[i + j] = own[j];
        j += 1;
    }
    out
}
/// The read set is 15 headless refusals + `plan_unreadable`.
pub const READ_BASE: usize = 16;
const _: () = assert!(HEADLESS_REFUSALS.len() + 1 == READ_BASE);

/// The refusals every write of this domain shares beyond the read set: the
/// server's three named conditions, the confirmation gate and the host's
/// entropy.
const WRITE_OWN: [Refusal; 6] = [
    PM_REFUSED,
    NOT_PERMITTED,
    CONFLICT,
    TASK_NOT_FOUND,
    CONFIRMATION_REQUIRED,
    RNG_UNAVAILABLE,
];
/// The write set is the read set + [`WRITE_OWN`].
pub const WRITE_BASE: usize = READ_BASE + WRITE_OWN.len();

/// The refusals every write of this domain declares. `TOTAL` is
/// `22 + own.len()`, checked at compile time.
pub const fn write_refusals<const TOTAL: usize>(own: &[Refusal]) -> [Refusal; TOTAL] {
    assert!(TOTAL == WRITE_BASE + own.len());
    let mut out = [PLAN_UNREADABLE; TOTAL];
    let mut i = 0;
    while i < HEADLESS_REFUSALS.len() {
        out[i] = HEADLESS_REFUSALS[i];
        i += 1;
    }
    out[i] = PLAN_UNREADABLE;
    i += 1;
    let mut k = 0;
    while k < WRITE_OWN.len() {
        out[i + k] = WRITE_OWN[k];
        k += 1;
    }
    i += WRITE_OWN.len();
    let mut j = 0;
    while j < own.len() {
        out[i + j] = own[j];
        j += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// The door and the folds
// ---------------------------------------------------------------------------

/// The selected project's graph, decoded, with the lane's project id and name.
pub struct Graph {
    pub lane: &'static str,
    pub project_id: String,
    pub project_name: String,
    /// The signed-in account that read it — the DS Grid catalogue on this
    /// machine is scoped by lane and account, so a `dsgrid:local-…`
    /// reference resolves under the same identity that will write the task.
    pub uid: String,
    pub graph: CanonicalProjectGraph,
    /// The graph as the server published it, for the folds that take it raw
    /// — and for the two task members the fold deliberately leaves out,
    /// `geometry` and `links` (wire.rs), which `ds pm task geometry` reads.
    pub raw: Value,
}

/// Read the selected project's canonical graph through the native client.
pub fn graph(lane: &str) -> Result<Graph, Failure> {
    let report =
        ds_cli_auth::project_management(lane, &ds_client_core::project_management::Command::Graph)?;
    let project_id = report.project_id().to_owned();
    let project_name = report.project_name().to_owned();
    let lane = report.lane();
    let uid = report.identity().uid().to_owned();
    let raw = report.into_result();
    let graph = ds_command_kernel::project_management::decode_project_graph(&raw, &project_id);
    Ok(Graph {
        lane,
        project_id,
        project_name,
        uid,
        graph,
        raw,
    })
}

impl Graph {
    /// One live task row exactly as the server published it — with the
    /// `geometry` and `links` the decoded graph does not carry.
    pub fn raw_task(&self, task_id: &str) -> Option<&Value> {
        self.raw["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|task| task["id"].as_str() == Some(task_id) && task["is_deleted"] != true)
    }
}

/// The selected project's context records — every readable record, up to the
/// server's bound per collection — and whether the server cut the page.
pub fn records(lane: &str) -> Result<(String, Vec<reads::ContextRecord>, bool), Failure> {
    let report = ds_cli_auth::project_management(
        lane,
        &ds_client_core::project_management::Command::Context {
            limit: Some(MAX_CONTEXT_ROWS),
        },
    )?;
    let project_id = report.project_id().to_owned();
    let context = report.into_result();
    let truncated = context["truncated_by_collection"]["records"] == Value::Bool(true)
        || context["next_cursors"]["records"].is_string();
    Ok((
        project_id,
        reads::decode_context_records(&context),
        truncated,
    ))
}

/// Commit one prepared command and fold the engine's answer.
pub fn commit(
    lane: &str,
    prepared: &writes::PreparedCommand,
) -> Result<writes::OperationResult, Failure> {
    let report = ds_cli_auth::project_management(
        lane,
        &ds_client_core::project_management::Command::Commit {
            command_id: command_id()?,
            base_revision: prepared.base_revision,
            command: prepared.command.clone(),
        },
    )?;
    accepted(writes::decode_operation_result(&report.into_result()))
}

/// Commit one prepared draft and fold the engine's answer.
pub fn commit_batch(
    lane: &str,
    prepared: &writes::PreparedBatch,
) -> Result<writes::OperationResult, Failure> {
    let report = ds_cli_auth::project_management(
        lane,
        &ds_client_core::project_management::Command::CommitBatch {
            command_id: command_id()?,
            base_revision: prepared.base_revision,
            commands: prepared.commands.clone(),
        },
    )?;
    accepted(writes::decode_operation_result(&report.into_result()))
}

/// The engine answered 200 — but a 200 with `applied: false` is a refusal
/// too: the engine evaluated the command and declined it, naming the rule
/// (`acceptedOrThrow`, cli-pm.ts:262-271). Never report `applied` for a
/// change that did not land.
fn accepted(result: writes::OperationResult) -> Result<writes::OperationResult, Failure> {
    if result.applied && result.violations.is_empty() {
        return Ok(result);
    }
    let first = result.violations.first();
    Err(Failure::invalid(
        PM_REFUSED.code,
        first
            .map(|issue| issue.message.clone())
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| "Project Management did not apply the change.".into()),
    )
    .detail(json!({
        "service_code": first.map(|issue| issue.code.clone()),
        "violations": result.violations,
        "committedRevision": result.committed_revision,
    }))
    .remedy(PM_REFUSED.remedy)
    .next("ds pm task read --task <task-id>"))
}

/// One idempotency key per commit. The engine deduplicates on it, so a lost
/// answer retried with the SAME key lands once — which is why it is minted
/// here, in the host with entropy, and not in the kernel.
pub fn command_id() -> Result<String, Failure> {
    ds_cli_auth::device::mint_command_id()
}

/// The kernel's refusal of a write before it was sent, as the failure
/// `ds pm` documents for it.
pub fn refused(refusal: writes::Refusal) -> Failure {
    let message = refusal.message();
    match refusal {
        writes::Refusal::NotPermitted(_) => Failure::unauthorized(NOT_PERMITTED.code, message)
            .remedy(NOT_PERMITTED.remedy)
            .next("ds pm plan"),
        writes::Refusal::TaskNotFound(id) => Failure::invalid(TASK_NOT_FOUND.code, message)
            .detail(json!({ "task": id }))
            .remedy(TASK_NOT_FOUND.remedy)
            .next("ds pm task list"),
        writes::Refusal::TaskExists(id) => Failure::conflict(PM_REFUSED.code, message)
            .detail(json!({ "task": id, "service_code": "task_exists" }))
            .remedy("the id already landed — read it, or mint a new --id for new work")
            .next(format!("ds pm task read --task {id}")),
        writes::Refusal::InvalidShape(_) => Failure::invalid("invalid_task_shape", message)
            .remedy(task::create::INVALID_TASK_SHAPE.remedy)
            .next("ds pm task create --help"),
        writes::Refusal::InvalidValue(_) => Failure::invalid(INVALID_VALUE.code, message)
            .remedy(INVALID_VALUE.remedy)
            .next("ds pm plan --output json"),
        writes::Refusal::NothingToChange => Failure::invalid(
            "nothing_to_update",
            "no field, state, progress or date flag was given",
        )
        .remedy("name at least one change, e.g. --delivery in_progress")
        .next("ds pm task update --help"),
        writes::Refusal::TooManyAssignees { given, max } => {
            Failure::invalid("too_many_assignees", message)
                .detail(json!({ "given": given, "max": max }))
                .remedy(task::assign::TOO_MANY_ASSIGNEES.remedy)
        }
    }
}

// ---------------------------------------------------------------------------
// Flag shapes shared across the domain
// ---------------------------------------------------------------------------

pub const TASK_ARG: Arg = Arg {
    name: "task",
    kind: ArgKind::Value,
    value: "<task-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The task, by the id `ds pm task list` reports.",
};

pub const LIMIT_ARG: Arg = Arg {
    name: "limit",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: Some("50"),
    choices: &[],
    summary: "Rows in one page (1-250). The total is always reported.",
};

pub const PAGE_ARG: Arg = Arg {
    name: "page",
    kind: ArgKind::Value,
    value: "<index>",
    required: false,
    default: Some("0"),
    choices: &[],
    summary: "Zero-based page of the bounded result.",
};

/// A calendar date flag, held to the shape the project's schedule uses.
///
/// The check is local because a transposed day and month is the commonest
/// mistake there is, and it is one the engine cannot catch: `2026-13-01`
/// is refused, but `2026-01-09` for the ninth of September is a valid date
/// that quietly schedules the wrong week.
pub fn date(raw: &str, flag: &str) -> Result<String, Failure> {
    let refuse = || {
        Failure::invalid(
            "invalid_date",
            format!("`--{flag}` must be a calendar date in YYYY-MM-DD form"),
        )
        .remedy(format!("pass e.g. --{flag} 2026-09-01"))
        .detail(json!({ "given": raw }))
    };
    let parts: Vec<&str> = raw.split('-').collect();
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return Err(refuse());
    }
    let mut numbers = [0u32; 3];
    for (slot, part) in numbers.iter_mut().zip(&parts) {
        *slot = part.parse::<u32>().map_err(|_| refuse())?;
    }
    let [year, month, day] = numbers;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if !(1970..=2999).contains(&year) || day == 0 || day > days_in_month {
        return Err(refuse());
    }
    Ok(raw.to_string())
}

/// An email flag. Held to the one property that makes it an address rather
/// than a display name; the engine normalises and the project's
/// membership decides whether the person is real.
pub fn email(raw: &str, flag: &str) -> Result<String, Failure> {
    let trimmed = raw.trim().to_ascii_lowercase();
    let (local, domain) = trimmed.split_once('@').unwrap_or(("", ""));
    if local.is_empty() || !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.')
    {
        return Err(Failure::invalid(
            "invalid_email",
            format!("`--{flag}` must be an email address"),
        )
        .remedy(format!("pass e.g. --{flag} pilot@example.com"))
        .detail(json!({ "given": raw })));
    }
    Ok(trimmed)
}

/// Render one task row the same way in every human projection of this domain.
pub fn task_line(row: &serde_json::Value) -> String {
    format!(
        "  {:<10} {:<20} {:<36} {:<18} {:>4}%  {}\n",
        row["wbs"].as_str().unwrap_or("—"),
        truncate(row["id"].as_str().unwrap_or("?"), 20),
        truncate(row["title"].as_str().unwrap_or("?"), 36),
        row["delivery"].as_str().unwrap_or("—"),
        row["progress"].as_u64().unwrap_or(0),
        row["responsible"].as_str().unwrap_or("unassigned"),
    )
}

/// Keep a human line one line wide without hiding that it was cut.
pub fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// Serialise a kernel reply into the envelope's `data`.
pub fn data<T: serde::Serialize>(reply: &T) -> Result<Value, Failure> {
    serde_json::to_value(reply).map_err(|error| {
        Failure::internal(PLAN_UNREADABLE.code, error.to_string()).remedy(PLAN_UNREADABLE.remedy)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_flag_is_refused_before_it_can_schedule_the_wrong_week() {
        assert_eq!(date("2026-09-01", "start").expect("valid"), "2026-09-01");
        for bad in [
            "2026-9-1",
            "01-09-2026",
            "2026/09/01",
            "2026-13-01",
            "2026-09-32",
            "2026-02-29",
            "2026-02-31",
            "",
            "tomorrow",
        ] {
            assert_eq!(
                date(bad, "start").expect_err("must refuse").code(),
                "invalid_date",
                "`{bad}` was accepted as a schedule date"
            );
        }
        assert_eq!(date("2028-02-29", "start").expect("leap day"), "2028-02-29");
    }

    #[test]
    fn a_person_flag_is_an_address_and_is_normalised_to_lower_case() {
        assert_eq!(
            email("  Pilot@Example.COM ", "request").expect("valid"),
            "pilot@example.com"
        );
        for bad in ["pilot", "@example.com", "pilot@example", "pilot@.com", ""] {
            assert_eq!(
                email(bad, "request").expect_err("must refuse").code(),
                "invalid_email",
                "`{bad}` was accepted as a project member"
            );
        }
    }

    #[test]
    fn the_kernels_refusals_each_become_the_code_the_command_documents() {
        let code = |refusal: writes::Refusal| refused(refusal).code().to_owned();
        assert_eq!(
            code(writes::Refusal::NotPermitted("no".into())),
            "work_not_permitted"
        );
        assert_eq!(
            code(writes::Refusal::TaskNotFound("t".into())),
            "task_not_found"
        );
        assert_eq!(code(writes::Refusal::TaskExists("t".into())), "pm_refused");
        assert_eq!(
            code(writes::Refusal::InvalidShape("x".into())),
            "invalid_task_shape"
        );
        assert_eq!(
            code(writes::Refusal::InvalidValue("x".into())),
            "invalid_choice"
        );
        assert_eq!(code(writes::Refusal::NothingToChange), "nothing_to_update");
        assert_eq!(
            code(writes::Refusal::TooManyAssignees { given: 9, max: 3 }),
            "too_many_assignees"
        );
    }

    #[test]
    fn an_engine_answer_that_did_not_apply_is_a_named_refusal_not_a_receipt() {
        let declined = writes::decode_operation_result(&json!({
            "applied": false, "committed_revision": 4,
            "violations": [{"code": "TASK_EXISTS", "message": "task \"T-1\" already exists"}],
        }));
        let failure = accepted(declined).expect_err("not applied");
        assert_eq!(failure.code(), "pm_refused");
        assert!(failure.to_string().contains("already exists"));
        let landed =
            writes::decode_operation_result(&json!({"applied": true, "committed_revision": 5}));
        assert!(accepted(landed).is_ok());
    }

    #[test]
    fn the_refusal_tables_hold_the_headless_set_first_and_no_duplicates() {
        let reads = read_refusals::<17>(&[INVALID_NUMBER]);
        assert_eq!(reads.len(), 17);
        assert_eq!(reads[0].code, HEADLESS_REFUSALS[0].code);
        assert_eq!(reads[15].code, "plan_unreadable");
        assert_eq!(reads[16].code, "invalid_number");
        let writes = write_refusals::<23>(&[INVALID_DATE]);
        let codes: std::collections::BTreeSet<&str> = writes.iter().map(|r| r.code).collect();
        assert_eq!(codes.len(), writes.len(), "a code is declared twice");
        for expected in [
            "pm_refused",
            "work_not_permitted",
            "work_revision_conflict",
            "task_not_found",
            "confirmation_required",
            "device_rng_unavailable",
            "invalid_date",
        ] {
            assert!(codes.contains(expected), "{expected} missing");
        }
    }
}
