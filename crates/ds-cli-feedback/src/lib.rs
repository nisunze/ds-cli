//! `ds feedback` records product gaps in the same authenticated backlog as the
//! DS GridDesign `fb` shortcut — and closes them when the gap is gone.
//!
//! This is deliberately a paired-application domain. `ds` never receives a
//! Firebase token and never invents another issue store; the running app sends
//! one typed report through its existing feedback client under the user it has
//! already authenticated. The adapter pins `reporter_kind` to `agent`.
//!
//! ## The family is a loop, not a drop box
//!
//! ```text
//!   submit → (a coding session closes the gap) → list → close
//! ```
//!
//! Closing was the missing half. A gap an agent reported and an agent then
//! fixed stayed open until a person found it in the `fb` tab, so the backlog
//! counted work already done. `close` performs the same governed triage
//! mutation that tab performs — the same status vocabulary, the same
//! optimistic version, the same platform capability — so a report closed from
//! a terminal and one closed from the UI are the same record.
//!
//! ## What is deliberately absent
//!
//! Reopening. `close` names the two addressed statuses only: an agent may
//! retire work it can prove is done, and returning a report to the open
//! backlog stays a human triage decision in the `fb` tab.

pub mod close;
pub mod list;
pub mod submit;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Domain, Refusal};
use ds_cli_desktop::ops::{self, BridgeOp};

pub static DOMAIN: Domain = Domain {
    id: "feedback",
    summary: "Product feedback: report a gap, and close it once it is fixed.",
    commands: &[&submit::COMMAND, &list::COMMAND, &close::COMMAND],
};

// ---------------------------------------------------------------------------
// The declared wire contract
// ---------------------------------------------------------------------------

pub const SUBMIT: BridgeOp = BridgeOp {
    operation: "feedback.submit",
    arguments: &[
        "title",
        "detail",
        "component",
        "kind",
        "severity",
        "agent",
        "model",
        "client",
        "evidence",
        "context",
    ],
};
pub const LIST: BridgeOp = BridgeOp {
    operation: "feedback.list",
    arguments: &["view", "component", "query", "limit", "detail"],
};
pub const CLOSE: BridgeOp = BridgeOp {
    operation: "feedback.close",
    arguments: &["id", "status", "resolution", "expected_version"],
};

pub const BRIDGE_OPS: &[&BridgeOp] = &[&SUBMIT, &LIST, &CLOSE];

/// The two statuses the shared backlog counts as addressed. Held here because
/// the command's choices and the adapter's guard must be the same two words.
pub const CLOSED_STATUSES: &[&str] = &["resolved", "wont_fix"];

/// The longest resolution ds-brain stores, in characters. A hand copy of the
/// service bound, so an over-long resolution is refused locally rather than
/// after a round trip.
pub const MAX_RESOLUTION_CHARS: usize = 1_000;
/// The most rows one listing returns. The service scans further; this is what
/// a caller pays for in context.
pub const MAX_LIST_LIMIT: i64 = 50;

// ---------------------------------------------------------------------------
// Refusals this domain adds to the shared pairing set
// ---------------------------------------------------------------------------

pub const NOT_SIGNED_IN: Refusal = Refusal {
    code: "desktop_signed_out",
    when: "DS GridDesign is running but has no signed-in user",
    remedy: "sign in to DS GridDesign, then run the command again",
};
pub const INVALID_TEXT: Refusal = Refusal {
    code: "invalid_text",
    when: "a required report field is empty, untrimmed, or exceeds its bound",
    remedy: "send a concise title, detail, component and agent name without secrets or customer data",
};
pub const NOT_FOUND: Refusal = Refusal {
    code: "feedback_not_found",
    when: "no report in the shared backlog carries this id",
    remedy: "take the id from `ds feedback list --view all --output json`",
};
pub const CONFLICT: Refusal = Refusal {
    code: "feedback_conflict",
    when: "the report changed between the listing that was read and this close",
    remedy: "list it again, confirm the newer state is still addressed, then close it",
};
pub const NOT_PERMITTED: Refusal = Refusal {
    code: "feedback_not_permitted",
    when: "the signed-in account may read the shared backlog but not triage it",
    remedy: "ask an account that holds the platform triage capability to close it",
};

/// What the application says when the backlog refuses a close. Matched
/// case-insensitively against its own message; the parity test requires each
/// marker to still appear in the adapter's source, and an unmatched refusal
/// stays `desktop_refused` rather than becoming a wrong named one.
pub const NOT_FOUND_MARKERS: &[&str] = &["was not found"];
pub const CONFLICT_MARKERS: &[&str] = &["changed since it was read"];
pub const NOT_PERMITTED_MARKERS: &[&str] = &["not permitted to triage"];

/// Name the three conditions a triage call has that ordinary operation
/// failures do not. Each has its own next step, so leaving them as
/// `desktop_refused` would send a caller to read prose for something that has
/// a code, a remedy and a different command to run.
pub fn classify_feedback_failure(failure: Failure) -> Failure {
    let failure = ops::classify_signed_out(failure);
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let message = failure
        .detail_value()
        .and_then(|detail| detail["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let hit = |markers: &[&str]| markers.iter().any(|marker| message.contains(marker));
    if hit(NOT_FOUND_MARKERS) {
        return Failure::invalid(NOT_FOUND.code, "no feedback report carries this id")
            .remedy(NOT_FOUND.remedy)
            .next("ds feedback list --view all");
    }
    if hit(CONFLICT_MARKERS) {
        return Failure::conflict(
            CONFLICT.code,
            "the report changed since it was read; the close was not applied",
        )
        .remedy(CONFLICT.remedy)
        .next("ds feedback list --view all");
    }
    if hit(NOT_PERMITTED_MARKERS) {
        return Failure::unauthorized(
            NOT_PERMITTED.code,
            "this account may read the shared backlog but not triage it",
        )
        .remedy(NOT_PERMITTED.remedy);
    }
    failure
}

/// A bounded, trimmed, non-empty text flag.
pub fn bounded_text<'a>(value: &'a str, flag: &str, max: usize) -> Result<&'a str, Failure> {
    if value.is_empty() || value.trim() != value || value.chars().count() > max {
        return Err(Failure::invalid(
            INVALID_TEXT.code,
            format!("`--{flag}` must be non-empty, trimmed, and at most {max} characters"),
        )
        .remedy(INVALID_TEXT.remedy));
    }
    Ok(value)
}

/// Fit one column of a human line without breaking a character.
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
    use serde_json::json;

    fn refused(detail: &str) -> Failure {
        Failure::failed("desktop_refused", "the application refused the operation")
            .detail(json!({ "detail": detail }))
    }

    #[test]
    fn triage_conditions_get_their_own_codes() {
        assert_eq!(
            classify_feedback_failure(refused("The feedback report was not found.")).code(),
            "feedback_not_found"
        );
        assert_eq!(
            classify_feedback_failure(refused("The report changed since it was read.")).code(),
            "feedback_conflict"
        );
        assert_eq!(
            classify_feedback_failure(refused(
                "This account is not permitted to triage the shared feedback backlog."
            ))
            .code(),
            "feedback_not_permitted"
        );
        // Anything else keeps the application's own refusal.
        assert_eq!(
            classify_feedback_failure(refused("the backlog is unavailable")).code(),
            "desktop_refused"
        );
    }

    #[test]
    fn closing_names_only_addressed_statuses() {
        assert_eq!(CLOSED_STATUSES, ["resolved", "wont_fix"]);
    }
}
