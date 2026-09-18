//! `ds feedback` records product gaps in the same authenticated backlog as the
//! DS GridDesign `fb` shortcut — and closes them when the gap is gone.
//!
//! The native user reaches one fixed authenticated backend contract without
//! Desktop or a selected project. Desktop is an explicit compatibility host;
//! neither route returns credentials or creates another issue store.
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

pub static DOMAIN: Domain = Domain {
    id: "feedback",
    summary: "Product feedback: report a gap, and close it once it is fixed.",
    commands: &[&submit::COMMAND, &list::COMMAND, &close::COMMAND],
};

// ---------------------------------------------------------------------------
// The declared wire contract
// ---------------------------------------------------------------------------

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
    code: "headless_signed_out",
    when: "this machine has no restored native user for the selected lane",
    remedy: "run `ds auth login --email <address>`, or link this machine from a signed-in Desktop",
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

// The three triage conditions arrive typed from the native owner
// (`ds_cli_auth::feedback`), which maps `feedback_not_found`,
// `feedback_conflict` and `feedback_not_permitted` itself. Recovering them by
// matching the desktop's prose — which is what `classify_feedback_failure` and
// its marker lists did here — is no longer a thing that can be needed.

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

pub const LANE_ARG: ds_cli_contract::spec::Arg =
    ds_cli_contract::spec::Arg::value("lane", "<stable|canary>", "Native credential lane.")
        .choices(&["stable", "canary"])
        .default("stable");
/// This domain's own refusals, then the native user's. There is no host to
/// choose any more, so no target refusal is appended.
pub const fn native_refusals<const N: usize, const M: usize>(old: [Refusal; N]) -> [Refusal; M] {
    let mut out = [INVALID_TEXT; M];
    let mut i = 0;
    while i < N {
        out[i] = old[i];
        i += 1;
    }
    let mut j = 0;
    while j < ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() {
        out[i] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals[j];
        i += 1;
        j += 1;
    }
    out
}
/// The one route: the shared backlog through the restored native user.
pub fn invoke_native(
    inputs: &ds_cli_contract::Inputs,
    operation: &str,
    mut arguments: serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Value, Failure> {
    arguments.insert("operation".into(), serde_json::json!(operation));
    let command: ds_cli_auth::FeedbackCommand =
        serde_json::from_value(serde_json::Value::Object(arguments)).map_err(|_| {
            Failure::invalid("invalid_text", "Invalid feedback command fields")
                .remedy(INVALID_TEXT.remedy)
        })?;
    ds_cli_auth::feedback(inputs.value("lane").unwrap_or("stable"), &command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_names_only_addressed_statuses() {
        assert_eq!(CLOSED_STATUSES, ["resolved", "wont_fix"]);
    }
}
