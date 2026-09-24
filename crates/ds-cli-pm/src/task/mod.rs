//! `ds pm task` — one work item, and the list it lives in.
//!
//! The family is a read, then a decision, then one governed write:
//!
//! ```text
//!   list → read → update | assign → respond
//!               ↘ create
//!   propose → (request-admission) → admit | decline ; log-hours
//! ```
//!
//! The second line is the third-party loop (task-proposals.md): a proposer
//! creates Inbox work about themselves with an estimate and asks; the PM
//! admits or declines; an assignee logs the hours actually taken.
//!
//! Every write is one project command carrying the revision it was authored
//! against. The application refuses a stale one rather than merging it, and
//! the refusal names the revision that moved — so a losing race is "re-read
//! and decide again", never a silent overwrite of somebody's plan.

pub mod assign;
pub mod create;
pub mod list;
pub mod proposals;
pub mod read;
pub mod respond;
pub mod update;

use serde_json::Value;

/// The engine's own warnings, rendered under a write's headline.
///
/// A warning is not a refusal: the command applied. But an accepted plan
/// change that pushed a dependency out, or left a milestone unscheduled, is
/// exactly the thing a caller running unattended needs told — so no write
/// renders without passing them through.
pub fn warnings(data: &Value) -> String {
    let mut out = String::new();
    for warning in data["warnings"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  ! {}\n",
            warning["message"]
                .as_str()
                .or_else(|| warning["code"].as_str())
                .unwrap_or("the engine returned a warning with no message"),
        ));
    }
    out
}
