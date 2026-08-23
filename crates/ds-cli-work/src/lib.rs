//! `ds work` — Project Work: the plan, its tasks, the assignment loop and the
//! record of what happened.
//!
//! ## Why this domain is a bridge domain
//!
//! Project Work is governed shared state. Its graph lives behind ds-brain,
//! which is the only gateway and the only authority: it decides who may write,
//! arbitrates two people accepting the same request in the same second, and
//! refuses a command authored against a revision that has moved. None of that
//! is reachable from a file, and none of it may be reached with an ambient
//! credential — so every command here is one named semantic operation the
//! *paired application* performs under the session it already holds.
//!
//! `ds` therefore carries no token, no project id it trusts, and no copy of
//! the rules. It asks the application for an outcome.
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

pub mod plan;
pub mod record;
pub mod task;

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Domain, Refusal};
use serde_json::json;

// The paired-application primitives every bridge domain shares. They are
// declared once in `ds-cli-desktop` — the authority surface — so a caller who
// learned `--desktop-descriptor` and the pairing refusals from `ds map` has
// learned them here too.
pub use ds_cli_desktop::ops::{
    AMBIGUOUS, BridgeOp, DESCRIPTOR_ARG, INVALID_NUMBER, NOT_PAIRED, PAIRING_REJECTED, REFUSED,
    SIGNED_OUT, SIGNED_OUT_MARKERS, UNREACHABLE, UNREADABLE, UNSUPPORTED, classify_signed_out,
    integer, invoke, paired, paired_availability, plural,
};

pub static DOMAIN: Domain = Domain {
    id: "work",
    summary: "Project Work: the plan, its tasks, assignments and records.",
    commands: &[
        &plan::COMMAND,
        &task::list::COMMAND,
        &task::read::COMMAND,
        &task::create::COMMAND,
        &task::update::COMMAND,
        &task::assign::COMMAND,
        &task::respond::COMMAND,
        &record::list::COMMAND,
        &record::read::COMMAND,
    ],
};

// ---------------------------------------------------------------------------
// The declared wire contract
// ---------------------------------------------------------------------------

pub const TASKS_LIST: BridgeOp = BridgeOp {
    operation: "work.tasks.list",
    arguments: &[
        "query",
        "state",
        "assignee",
        "discipline",
        "placement",
        "limit",
        "page",
    ],
};
pub const TASK_READ: BridgeOp = BridgeOp {
    operation: "work.task.read",
    arguments: &["task"],
};
pub const TASK_CREATE: BridgeOp = BridgeOp {
    operation: "work.task.create",
    arguments: &[
        "id",
        "title",
        "description",
        "kind",
        "parent",
        "startDate",
        "finishDate",
        "discipline",
    ],
};
pub const TASK_UPDATE: BridgeOp = BridgeOp {
    operation: "work.task.update",
    arguments: &[
        "task",
        // The task's own authored fields, sent as one patch because the
        // application folds them into a single `update_task_fields` command.
        "fields.title",
        "fields.description",
        "fields.discipline",
        "fields.priority",
        "fields.type",
        "fields.placement",
        "fields.schedulingMode",
        "delivery",
        "review",
        "closeout",
        "progress",
        "startDate",
        "finishDate",
    ],
};
pub const TASK_ASSIGN: BridgeOp = BridgeOp {
    operation: "work.task.assign",
    arguments: &["task", "owner", "request"],
};
pub const TASK_RESPOND: BridgeOp = BridgeOp {
    operation: "work.task.respond",
    arguments: &["task", "response"],
};
pub const PLAN_READ: BridgeOp = BridgeOp {
    operation: "work.plan.read",
    arguments: &["limit"],
};
pub const RECORDS_LIST: BridgeOp = BridgeOp {
    operation: "work.records.list",
    arguments: &["query", "category", "limit", "page"],
};
pub const RECORDS_READ: BridgeOp = BridgeOp {
    operation: "work.records.read",
    arguments: &["record"],
};

/// Every operation this domain can send, for the parity test to walk. A new
/// operation absent from this list cannot be sent: [`invoke`] takes a
/// [`BridgeOp`], and the test requires each one to be an operation the
/// application actually implements.
pub const BRIDGE_OPS: &[&BridgeOp] = &[
    &PLAN_READ,
    &TASKS_LIST,
    &TASK_READ,
    &TASK_CREATE,
    &TASK_UPDATE,
    &TASK_ASSIGN,
    &TASK_RESPOND,
    &RECORDS_LIST,
    &RECORDS_READ,
];

/// The engine's bound on the people one assignment request may name.
///
/// A hand copy of `MaxTaskAssignees` in ds-brain's `projectwork` package,
/// which the graph also publishes per project as `fieldModel.maxAssignees` —
/// and the field model is the authority. This copy exists only so a caller who
/// pasted a distribution list learns the bound from a local refusal instead of
/// from a rejected write, and `tests/bridge_parity.rs` holds it to the same
/// number the application falls back to.
pub const MAX_ASSIGNEES: usize = 20;

/// The largest page of tasks or records one read returns. The application
/// bounds its own projections to the same number; the total is always
/// reported, so a truncated page is never silent.
pub const MAX_PAGE_SIZE: i64 = 250;

/// The largest related collection returned by a detail read. Detail commands
/// have no paging cursor, so every collection reports its full count and
/// carries at most this many rows.
pub const MAX_RELATED_ROWS: usize = 250;

// ---------------------------------------------------------------------------
// Timeouts
// ---------------------------------------------------------------------------

/// A read paints from the application's cached graph and reconciles once.
/// On a field connection that reconciliation is the slow part.
pub const READ_TIMEOUT: Duration = Duration::from_secs(3 * 60);
/// A write is one governed round trip to ds-brain, which is fast — but it may
/// be queued behind the same surface's own reconciliation.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(3 * 60);

// ---------------------------------------------------------------------------
// Refusals this domain adds to the shared pairing set
// ---------------------------------------------------------------------------

pub const WORK_REFUSED: Refusal = Refusal {
    code: "desktop_refused",
    when: "no such task or record, or Project Work declined the command",
    remedy: "check the id with `ds work task list`; read detail.detail for its message",
};
pub const NOT_PERMITTED: Refusal = Refusal {
    code: "work_not_permitted",
    when: "the signed-in user may read this project's plan but not change it",
    remedy: "ask a project admin for schedule-editor access",
};
pub const CONFLICT: Refusal = Refusal {
    code: "work_revision_conflict",
    when: "the plan moved while the command was in flight",
    remedy: "re-read with `ds work task read` and issue the command again",
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

/// What the application says when the signed-in user may read this project's
/// plan but not change it. Hand copies of its prose, held to the application's
/// source by `tests/bridge_parity.rs`.
pub const NOT_PERMITTED_MARKERS: &[&str] = &["schedule editor", "contributor access"];

/// What the application says when the plan moved under a command in flight.
pub const CONFLICT_MARKERS: &[&str] = &["the plan moved", "revision conflict"];

/// Give this domain's two named conditions their own codes.
///
/// Both arrive as ordinary operation refusals — the application answered, and
/// what it answered was "you may not" or "you were too late". Letting them
/// through as `desktop_refused` would send a caller to read `detail` for two
/// conditions that have a name, a remedy, and a different next step: one is
/// permanent until an admin acts, the other is "re-read and try again", which
/// is the whole reason an unattended caller needs to tell them apart.
pub fn classify_work_failure(failure: Failure) -> Failure {
    let failure = classify_signed_out(failure);
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|detail| detail["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if NOT_PERMITTED_MARKERS
        .iter()
        .any(|marker| detail.contains(marker))
    {
        return Failure::unauthorized(
            "work_not_permitted",
            "the signed-in user may read this project's plan but not change it",
        )
        .remedy(NOT_PERMITTED.remedy)
        .next("ds work plan");
    }
    if CONFLICT_MARKERS
        .iter()
        .any(|marker| detail.contains(marker))
    {
        return Failure::conflict(
            "work_revision_conflict",
            "the plan moved while the command was in flight",
        )
        .remedy(CONFLICT.remedy)
        .next("ds work task read --task <task-id>");
    }
    failure
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
    summary: "The task, by the id `ds work task list` reports.",
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
/// mistake there is, and it is one the application cannot catch: `2026-13-01`
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
/// than a display name; the application normalises and the project's
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
    fn the_two_conditions_an_unattended_caller_must_tell_apart_get_their_own_codes() {
        // "You may not" is permanent until an admin acts; "you were too late"
        // means re-read and try again. Both arrive as one refusal from the
        // application, and a caller that cannot tell them apart either retries
        // forever or gives up on a command that would have worked.
        let refused = |detail: &str| {
            classify_work_failure(
                Failure::failed("desktop_refused", "refused").detail(json!({ "detail": detail })),
            )
        };
        assert_eq!(
            refused("Project schedule editor access is required to change this plan.").code(),
            "work_not_permitted"
        );
        assert_eq!(
            refused("Project contributor access is required to answer an assignment request.")
                .code(),
            "work_not_permitted"
        );
        assert_eq!(
            refused("The plan moved to revision 9 while this command was authored against 7.")
                .code(),
            "work_revision_conflict"
        );
        // Still classified by the shared signed-out rule.
        assert_eq!(
            refused("No active project. Open a project first.").code(),
            "desktop_signed_out"
        );
        // And an ordinary engine violation stays what it was.
        assert_eq!(
            refused("task \"T-1\" already exists").code(),
            "desktop_refused"
        );
    }

    #[test]
    fn every_declared_operation_is_listed_for_the_parity_test_to_walk() {
        // An operation a handler can send but the list does not carry is one
        // the parity test never proves against the application. The list is
        // the only thing standing between a typo and a runtime refusal.
        let mut names: Vec<&str> = BRIDGE_OPS.iter().map(|op| op.operation).collect();
        names.sort_unstable();
        let mut unique = names.clone();
        unique.dedup();
        assert_eq!(names, unique, "an operation is declared twice");
        assert_eq!(
            names.len(),
            DOMAIN.commands.len(),
            "every ds work command sends exactly one operation, and every \
             declared operation belongs to a command"
        );
    }
}
