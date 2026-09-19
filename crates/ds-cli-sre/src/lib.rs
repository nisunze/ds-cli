//! `ds sre` — bounded platform reliability reads through the native user.
//!
//! The Reliability page, ds-brain and `ds-client-core::sre` own every value
//! returned here. This crate validates flags, names one of two reads, and
//! renders the owner's bounded projection. It never reads Cloud Monitoring,
//! BigQuery, Firestore or browser storage itself.
//!
//! Unlike project domains, reliability is platform-global. A restored native
//! user is required; an active project is not, and none is ever sent. The owner
//! separately enforces reliability access, which today is `platform.admin`.
//!
//! ## Why there is one route
//!
//! Until 2026-09-18 both commands travelled through a paired desktop window,
//! and both were pure server reads: the window held the session, made the same
//! request, and handed the answer back. That put platform health behind the one
//! host least likely to be running when it matters — on a server, where an
//! agent is asking why a job failed, `ds sre overview` refused with
//! `desktop_not_paired`. A read that needs no window does not get one.

pub mod events;
pub mod overview;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Domain, Refusal};
use ds_client_core::sre;

// Neutral argument helpers: a numeric bound and an English count say nothing
// about a paired window, so they come from the contract crate.
pub use ds_cli_contract::args::{INVALID_NUMBER, integer};

pub static DOMAIN: Domain = Domain {
    id: "sre",
    summary: "Platform reliability: fleet health and bounded request events.",
    commands: &[&overview::COMMAND, &events::COMMAND],
};

/// Every bound this domain enforces is the owner's. A second copy here would be
/// a second contract, and the first thing to go stale.
pub use sre::{
    MAX_DAYS, MAX_ERROR_MESSAGE_CHARS, MAX_EVENT_TEXT_CHARS, MAX_EVENTS, MAX_FILTER_CHARS,
    MAX_OVERVIEW_ROWS, MAX_SCAN_EVENTS,
};

pub const LANE_ARG: ds_cli_contract::spec::Arg =
    ds_cli_contract::spec::Arg::value("lane", "<stable|canary>", "Native credential lane.")
        .choices(&["stable", "canary"])
        .default("stable");

/// The route answers 401/403 through the shared `map_client`, so the code is
/// the one it emits; the reason and remedy are this domain's.
pub const NOT_PERMITTED: Refusal = Refusal {
    code: "auth_rejected",
    when: "the signed-in account may not read platform reliability",
    remedy: "ask a platform administrator to grant reliability access",
};

pub const INVALID_TEXT: Refusal = Refusal {
    code: "invalid_text",
    when: "an event filter is empty, untrimmed, or longer than 200 characters",
    remedy: "pass one exact trimmed filter value no longer than 200 characters",
};

/// This domain's own refusals, then the native user's. There is no host to
/// choose, so no target refusal is appended.
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

/// One bounded, trimmed, non-empty filter, named by the flag that carried it.
///
/// The owner refuses the same values; this refuses them first, so a typo is a
/// local answer with a flag name in it rather than a round trip.
pub fn bounded_filter<'a>(raw: &'a str, flag: &str) -> Result<&'a str, Failure> {
    if raw.is_empty() || raw.trim() != raw || raw.chars().count() > MAX_FILTER_CHARS {
        return Err(Failure::invalid(
            INVALID_TEXT.code,
            format!(
                "`--{flag}` must be non-empty, trimmed, and at most {MAX_FILTER_CHARS} characters"
            ),
        )
        .remedy(INVALID_TEXT.remedy)
        .detail(serde_json::json!({ "flag": flag, "max_chars": MAX_FILTER_CHARS })));
    }
    Ok(raw)
}

/// The one route: platform reliability through the restored native user.
pub fn invoke_native(
    inputs: &ds_cli_contract::Inputs,
    command: &sre::Command,
) -> Result<serde_json::Value, Failure> {
    ds_cli_auth::sre(inputs.value("lane").unwrap_or("stable"), command)
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

    /// The CLI states the owner's bounds, it does not hold its own.
    #[test]
    fn the_declared_bounds_are_the_owners() {
        assert_eq!(MAX_DAYS, sre::MAX_DAYS);
        assert_eq!(MAX_EVENTS, sre::MAX_EVENTS);
        assert_eq!(MAX_SCAN_EVENTS, sre::MAX_SCAN_EVENTS);
        assert_eq!(MAX_FILTER_CHARS, sre::MAX_FILTER_CHARS);
    }

    #[test]
    fn event_filters_are_bounded_before_the_read_is_opened() {
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
