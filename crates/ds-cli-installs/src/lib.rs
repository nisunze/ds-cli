//! `ds install` — the product's own installation inventory.
//!
//! Every copy of DS GridDesign that runs anywhere registers itself: a Windows
//! desktop, a Linux desktop, and a Linux server running `ds server serve`, all
//! through the same license-refresh call. What none of them had was a way to
//! be *read* without a browser. `ds capabilities --search installation`
//! answered with linked devices — a different thing — so the one inventory
//! that says which installations exist, who signed in on them, when each was
//! last alive and whether its licence is blocked was reachable only from the
//! Governance page. On a server, where an operator is asking exactly those
//! questions, it was unreadable.
//!
//! Three properties this domain deliberately keeps.
//!
//! **The decisions are not here.** Which rows are shown, how they are grouped
//! and ordered, what "last seen" is worth and what an operator should do next
//! all come from `ds_command_kernel::installation_inventory`, the same
//! projection the browser and the Server call. A second reading in a CLI is
//! how a terminal and a page end up disagreeing about whether a machine is
//! licensed.
//!
//! **Silence is never an uninstallation.** A machine that stops reporting is
//! reported as silent, with how long it has been silent, and nothing more. No
//! uninstall hook exists on either platform today, so `ds install retire` is
//! the explicit verb an operator uses to say a product is gone. It records a
//! transition; it never deletes a row, because this family exists to
//! investigate use and a deleted row is destroyed evidence.
//!
//! **Global, so no project.** An installation belongs to the product. Nothing
//! here selects, fences or sends a project id.

pub mod list;
pub mod policy;
pub mod retire;
pub mod show;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Domain, Refusal};
use ds_client_core::installs;
use serde_json::{Value, json};

pub use ds_cli_contract::args::integer;
pub use installs::{MAX_PAGE, MAX_REASON_CHARS};

pub static DOMAIN: Domain = Domain {
    id: "install",
    summary: "Installed copies of the product, their licence state and governance.",
    commands: &[
        &list::COMMAND,
        &show::COMMAND,
        &policy::COMMAND,
        &retire::COMMAND,
    ],
};

pub const LANE_ARG: ds_cli_contract::spec::Arg =
    ds_cli_contract::spec::Arg::value("lane", "<stable|canary>", "Native credential lane.")
        .choices(&["stable", "canary"])
        .default("stable");

pub const INSTALL_ARG: ds_cli_contract::spec::Arg = ds_cli_contract::spec::Arg::value(
    "install",
    "<install-id>",
    "Exact installation id, as `ds install list` prints it.",
)
.required();

pub const REASON_ARG: ds_cli_contract::spec::Arg = ds_cli_contract::spec::Arg::value(
    "reason",
    "<text>",
    "Why this transition is being recorded; appended to the installation's immutable history.",
)
.required();

// The codes below are the ones the route actually emits. The inventory is
// reached through the shared native client, which reports a refused user, a
// rejected request and an absent install under its own codes; a declaration
// that spelt them install-shaped (`install_not_permitted`, …) was a promise
// no caller could plan against, because nothing ever emitted it. What is
// this domain's own here is the WHEN and the REMEDY.

/// ds-brain gates every inventory action on `platform.admin` or `app.admin`.
pub const NOT_PERMITTED: Refusal = Refusal {
    code: "auth_rejected",
    when: "the inventory refused the signed-in account; app-level governance access is required",
    remedy: "ask a platform administrator for app-level governance access",
};

/// A read's inputs, refused before or by the inventory.
pub const INVALID_SELECTION: Refusal = Refusal {
    code: "auth_input_invalid",
    when: "an installation id, cursor or limit is outside its bounded contract",
    remedy: "use an exact id from `ds install list`, and a cursor it printed",
};

/// A write's inputs, or its optimistic-concurrency fence: the route answers
/// a moved revision (HTTP 409) with the same class as a malformed request,
/// and both are a re-read, never a retry.
pub const WRITE_REFUSED: Refusal = Refusal {
    code: "auth_input_invalid",
    when: "the id, revision, status token or reason is outside its contract, or the revision moved since it was read",
    remedy: "run `ds install show --install <id>` again and reapply against the revision it prints",
};

/// The inventory holds no installation with this id (HTTP 404).
pub const NOT_FOUND: Refusal = Refusal {
    code: "install_not_found",
    when: "no registered install has this id",
    remedy: "run ds install list",
};

pub const PROJECTION_UNAVAILABLE: Refusal = Refusal {
    code: "install_projection_unavailable",
    when: "the inventory answer cannot be projected into installation state",
    remedy: "update ds so its command kernel matches the deployed inventory contract",
};

/// This domain's own refusals, then the native user's. There is no host to
/// choose, so no target refusal is appended.
pub const fn native_refusals<const N: usize, const M: usize>(old: [Refusal; N]) -> [Refusal; M] {
    let mut out = [INVALID_SELECTION; M];
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

/// `ds install list`: no id, so nothing to be not found.
pub const LIST_REFUSALS_LEN: usize = 4 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len();
/// One named installation, read or written.
pub const DETAIL_REFUSALS_LEN: usize = 5 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len();

/// The one route: the installation inventory through the restored native user.
pub fn invoke(
    inputs: &ds_cli_contract::Inputs,
    command: &installs::Command,
) -> Result<Value, Failure> {
    ds_cli_auth::installs(inputs.value("lane").unwrap_or("stable"), command)
}

/// The host's clock, in epoch milliseconds. The kernel holds no clock of its
/// own; every age in the answer is measured against this one value, so an
/// answer is reproducible from the pair (records, now_ms).
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

/// Hand the raw inventory page to the kernel and return its projection.
///
/// `min_supported_version` is deliberately not guessed here. The minimum lives
/// in the admission service and travels on a heartbeat response, not on a
/// governance read, so an inventory answer cannot say whether an installation
/// is below it. The projection reports upgrade state only when a host supplies
/// that minimum; stating nothing beats stating a number nobody checked.
fn project(request: Value) -> Result<Value, Failure> {
    let bytes = serde_json::to_vec(&request).map_err(|error| {
        Failure::internal(PROJECTION_UNAVAILABLE.code, error.to_string())
            .remedy(PROJECTION_UNAVAILABLE.remedy)
    })?;
    ds_command_kernel::installation_inventory::evaluate_request(&bytes).map_err(|error| {
        Failure::internal(PROJECTION_UNAVAILABLE.code, error).remedy(PROJECTION_UNAVAILABLE.remedy)
    })
}

pub fn project_page(
    page: Value,
    search: Option<&str>,
    include_retired: bool,
    limit: i64,
) -> Result<Value, Failure> {
    let now = now_ms();
    project(json!({
        "schema": ds_command_kernel::installation_inventory::SCHEMA,
        "operation": "project",
        "now_ms": now,
        "observed_at_ms": now,
        "include_retired": include_retired,
        "search": search,
        "limit": limit,
        "page": page,
    }))
}

pub fn project_detail(detail: Value) -> Result<Value, Failure> {
    let now = now_ms();
    project(json!({
        "schema": ds_command_kernel::installation_inventory::SCHEMA,
        "operation": "detail",
        "now_ms": now,
        "observed_at_ms": now,
        "detail": detail,
    }))
}

/// One row, rendered the way an operator reads a fleet: what it is, who used
/// it, how long since it proved it was alive, and what to do about it.
///
/// The identity is the heading. The opaque id is a detail on the line below,
/// because a 36-character UUID is not a name for a machine.
pub fn render_row(row: &Value) -> String {
    let identity = &row["identity"];
    let principal = &row["principal"];
    let who = principal["email"]
        .as_str()
        .or_else(|| principal["uid"].as_str())
        .unwrap_or("no authenticated handshake");
    let state = &row["state"];
    let mut marks: Vec<String> = Vec::new();
    if state["retired"].as_bool().unwrap_or(false) {
        marks.push("retired".into());
    }
    if state["governed"].as_str() == Some("blocked") {
        marks.push(format!(
            "blocked (device {}, licence {})",
            state["device"].as_str().unwrap_or("?"),
            state["licence"].as_str().unwrap_or("?")
        ));
    }
    if state["lease"].as_str() != Some("allowed") {
        marks.push(format!("lease {}", state["lease"].as_str().unwrap_or("?")));
    }
    let marks = if marks.is_empty() {
        String::new()
    } else {
        format!(" · {}", marks.join(" · "))
    };
    // "never" and "ahead" are not ages, so they are not printed as one. A row
    // that reads "last seen never ago" is a row an operator stops trusting.
    let seen = match row["last_seen"]["bucket"].as_str().unwrap_or("never") {
        "never" => "never seen: no licence refresh has ever been recorded".to_owned(),
        "ahead" => "last seen ahead of this host's clock, via licence refresh".to_owned(),
        bucket => format!(
            "last seen {bucket} ago ({}, via licence refresh)",
            row["last_seen"]["staleness"].as_str().unwrap_or("silent"),
        ),
    };
    format!(
        "  {} {} {} [{}]\n    {} · {}{}\n    {} · remedy {}\n",
        identity["platform"].as_str().unwrap_or("?"),
        identity["version"].as_str().unwrap_or("?"),
        identity["lane"].as_str().unwrap_or("?"),
        identity["host_kind"].as_str().unwrap_or("?"),
        who,
        seen,
        marks,
        row["install_id"].as_str().unwrap_or("?"),
        row["remedy"].as_str().unwrap_or("none"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CLI states the owner's bounds; it does not hold its own.
    #[test]
    fn the_declared_bounds_are_the_owners() {
        assert_eq!(MAX_PAGE, installs::MAX_PAGE);
        assert_eq!(MAX_REASON_CHARS, installs::MAX_REASON_CHARS);
    }

    /// A row's heading is what it IS, not the opaque id it was filed under.
    #[test]
    fn a_rendered_row_leads_with_identity_and_never_with_a_uuid() {
        let row = json!({
            "install_id": "0cd65a76-a5be-440c-b82e-c3442b8c1e40",
            "identity": {"platform": "windows", "version": "0.1.3", "lane": "stable", "host_kind": "desktop"},
            "principal": {"kind": "account", "email": "user@example.test"},
            "last_seen": {"bucket": "hours", "staleness": "current"},
            "state": {"governed": "allowed", "device": "active", "licence": "active", "lease": "allowed", "retired": false},
            "remedy": "none",
        });
        let rendered = render_row(&row);
        let first = rendered.lines().next().unwrap();
        assert!(first.contains("windows 0.1.3 stable"), "{first}");
        assert!(!first.contains("0cd65a76"), "{first}");
        assert!(rendered.contains("via licence refresh"));
    }

    /// Two tokens that disagree are printed, not collapsed into one word.
    #[test]
    fn a_blocked_row_says_which_token_is_blocked() {
        let row = json!({
            "install_id": "id", "identity": {}, "principal": {},
            "last_seen": {"bucket": "days", "staleness": "stale"},
            "state": {"governed": "blocked", "device": "active", "licence": "blocked", "lease": "blocked", "retired": false},
            "remedy": "blocked_by_operator",
        });
        let rendered = render_row(&row);
        assert!(
            rendered.contains("device active, licence blocked"),
            "{rendered}"
        );
        assert!(rendered.contains("remedy blocked_by_operator"));
    }

    /// An installation that has never refreshed a licence has no age, so it is
    /// not given one. "last seen never ago" is not a fact about a machine.
    #[test]
    fn an_installation_with_no_age_is_not_given_one() {
        let row = json!({
            "install_id": "id", "identity": {}, "principal": {},
            "last_seen": {"at_ms": 0, "bucket": "never", "staleness": "silent"},
            "state": {"governed": "allowed", "device": "active", "licence": "active", "lease": "handshake_required", "retired": false},
            "remedy": "never_handshaked",
        });
        let rendered = render_row(&row);
        assert!(rendered.contains("never seen"), "{rendered}");
        assert!(!rendered.contains("never ago"), "{rendered}");
    }
}
