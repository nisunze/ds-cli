//! `ds sre` — bounded platform reliability reads through the paired desktop.
//!
//! The Reliability page and ds-brain own every value returned here. This
//! crate only validates flags, sends one operation from a closed bridge set,
//! and renders the owner's bounded projection. It never reads Cloud
//! Monitoring, BigQuery, Firestore, or browser storage itself.
//!
//! Unlike project domains, SRE is platform-global. A signed-in desktop user
//! is required, but an active project is not. The owner separately enforces
//! reliability access (currently platform admin).

pub mod events;
pub mod overview;

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Domain, Refusal};
use serde_json::json;

pub use ds_cli_desktop::ops::{
    AMBIGUOUS, BridgeOp, DESCRIPTOR_ARG, INVALID_NUMBER, NOT_PAIRED, PAIRING_REJECTED, UNREACHABLE,
    UNREADABLE, UNSUPPORTED, integer, invoke, paired, paired_availability,
};

pub static DOMAIN: Domain = Domain {
    id: "sre",
    summary: "Platform reliability: fleet health and bounded request events.",
    commands: &[&overview::COMMAND, &events::COMMAND],
};

pub const OVERVIEW: BridgeOp = BridgeOp {
    operation: "sre.overview",
    arguments: &[],
};

pub const EVENTS: BridgeOp = BridgeOp {
    operation: "sre.events",
    arguments: &[
        "days",
        "limit",
        "scanLimit",
        "service",
        "outcome",
        "category",
        "lane",
        "action",
        "project",
        "source",
    ],
};

/// Every operation this domain may send, walked by the desktop parity suite.
pub const BRIDGE_OPS: &[&BridgeOp] = &[&OVERVIEW, &EVENTS];

pub const MAX_DAYS: i64 = 365;
pub const MAX_EVENTS: i64 = 250;
pub const MAX_SCAN_EVENTS: i64 = 5_000;
pub const MAX_FILTER_CHARS: usize = 200;
pub const MAX_EVENT_TEXT_CHARS: usize = 128;
pub const MAX_ERROR_MESSAGE_CHARS: usize = 1_000;

/// Both owner reads may wait on Cloud Monitoring or a bounded BigQuery scan.
pub const READ_TIMEOUT: Duration = Duration::from_secs(3 * 60);

pub const SRE_REFUSED: Refusal = Refusal {
    code: "desktop_refused",
    when: "the paired Reliability adapter or its owner declined the read",
    remedy: "read detail.detail for the owner's bounded message",
};

/// This wording is intentionally global: SRE requires sign-in, not a project.
pub const SIGNED_OUT: Refusal = Refusal {
    code: "desktop_signed_out",
    when: "the paired application is running but has no signed-in user",
    remedy: "sign in to DS GridDesign; no project selection is required",
};

pub const NOT_PERMITTED: Refusal = Refusal {
    code: "sre_not_permitted",
    when: "the signed-in user does not have reliability access",
    remedy: "ask a platform administrator to grant reliability access",
};

pub const INVALID_TEXT: Refusal = Refusal {
    code: "invalid_text",
    when: "an event filter is empty, untrimmed, or longer than 200 characters",
    remedy: "pass one exact trimmed filter value no longer than 200 characters",
};

/// Owner refusal prose that has a stable, actionable CLI classification.
pub const NOT_PERMITTED_MARKERS: &[&str] = &["reliability access", "platform admin"];
pub const SRE_SIGNED_OUT_MARKERS: &[&str] = &["sign in", "signed out"];

pub fn classify_sre_failure(failure: Failure) -> Failure {
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|value| value["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if SRE_SIGNED_OUT_MARKERS
        .iter()
        .any(|marker| detail.contains(marker))
    {
        return Failure::unauthorized(
            "desktop_signed_out",
            "the paired session has no signed-in user",
        )
        .remedy(SIGNED_OUT.remedy)
        .next("ds desktop status");
    }
    if NOT_PERMITTED_MARKERS
        .iter()
        .any(|marker| detail.contains(marker))
    {
        return Failure::unauthorized(
            "sre_not_permitted",
            "the signed-in user does not have reliability access",
        )
        .remedy(NOT_PERMITTED.remedy);
    }
    failure
}

pub fn bounded_filter<'a>(raw: &'a str, flag: &str) -> Result<&'a str, Failure> {
    if raw.is_empty() || raw.trim() != raw || raw.chars().count() > MAX_FILTER_CHARS {
        return Err(Failure::invalid(
            "invalid_text",
            format!(
                "`--{flag}` must be non-empty, trimmed, and at most {MAX_FILTER_CHARS} characters"
            ),
        )
        .remedy(INVALID_TEXT.remedy)
        .detail(json!({ "flag": flag, "max_chars": MAX_FILTER_CHARS })));
    }
    Ok(raw)
}

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
    fn bridge_operations_are_closed_and_exact() {
        assert_eq!(OVERVIEW.operation, "sre.overview");
        assert!(OVERVIEW.arguments.is_empty());
        assert_eq!(EVENTS.operation, "sre.events");
        assert_eq!(
            EVENTS.arguments,
            [
                "days",
                "limit",
                "scanLimit",
                "service",
                "outcome",
                "category",
                "lane",
                "action",
                "project",
                "source",
            ]
        );
        assert_eq!(BRIDGE_OPS.len(), DOMAIN.commands.len());
    }

    #[test]
    fn permission_and_global_sign_in_refusals_are_typed() {
        let refused = |detail: &str| {
            classify_sre_failure(
                Failure::failed("desktop_refused", "refused").detail(json!({ "detail": detail })),
            )
        };
        assert_eq!(
            refused("Reliability access requires platform admin.").code(),
            "sre_not_permitted"
        );
        assert_eq!(
            refused("Sign in to DS GridDesign before using SRE commands.").code(),
            "desktop_signed_out"
        );
        assert_eq!(
            refused("The request-event query failed.").code(),
            "desktop_refused"
        );
    }

    #[test]
    fn event_filters_are_bounded_before_pairing() {
        assert_eq!(bounded_filter("ds-brain", "service").unwrap(), "ds-brain");
        for bad in ["", " ds-brain", "ds-brain "] {
            assert_eq!(
                bounded_filter(bad, "service").unwrap_err().code(),
                "invalid_text"
            );
        }
        let long = "x".repeat(MAX_FILTER_CHARS + 1);
        assert_eq!(
            bounded_filter(&long, "service").unwrap_err().code(),
            "invalid_text"
        );
    }
}
