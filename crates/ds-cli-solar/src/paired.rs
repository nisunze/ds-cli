//! Shared contract for Solar operations performed by the paired desktop.
//!
//! The application is the cache boundary. It may capture a city input,
//! refresh its authenticated source material, and run native Solar locally;
//! this CLI only asks a fixed semantic operation and receives its receipt.
//! There is intentionally no IndexedDB protocol, generic bridge operation, or
//! credential-shaped argument here.

use std::time::Duration;

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Availability, Refusal};
use ds_cli_desktop::bridge;
use serde_json::Value;

pub static REFUSALS: &[Refusal] = &[
    Refusal {
        code: "desktop_not_paired",
        when: "no DS GridDesign session is running on this machine",
        remedy: "start DS GridDesign, sign in, and retry",
    },
    Refusal {
        code: "desktop_ambiguous",
        when: "more than one DS GridDesign session is running",
        remedy: "name one with --desktop-descriptor <path>",
    },
    Refusal {
        code: "desktop_unreachable",
        when: "the bridge descriptor names a session that does not answer",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_unreadable",
        when: "the paired session's reply could not be read",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_operation_unsupported",
        when: "this DS GridDesign build does not offer the named Solar operation",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "desktop_refused",
        when: "the paired application declined the requested Solar operation",
        remedy: "read the refusal detail, correct the application state, and retry",
    },
    Refusal {
        code: "desktop_contract_mismatch",
        when: "the paired session returned a reply outside this command contract",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "pairing_rejected",
        when: "the descriptor's pairing secret is stale",
        remedy: "restart DS GridDesign to publish a fresh descriptor",
    },
];

/// `solar.run.start` also validates the concurrency bound before asking the
/// desktop. It gets its own list so command help names that local refusal.
pub static START_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "invalid_concurrency",
        when: "--concurrency is not a whole number from 1 through 32",
        remedy: "pass an integer from 1 through 32, or omit the flag",
    },
    Refusal {
        code: "desktop_not_paired",
        when: "no DS GridDesign session is running on this machine",
        remedy: "start DS GridDesign, sign in, and retry",
    },
    Refusal {
        code: "desktop_ambiguous",
        when: "more than one DS GridDesign session is running",
        remedy: "name one with --desktop-descriptor <path>",
    },
    Refusal {
        code: "desktop_unreachable",
        when: "the bridge descriptor names a session that does not answer",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_unreadable",
        when: "the paired session's reply could not be read",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_operation_unsupported",
        when: "this DS GridDesign build does not offer the named Solar operation",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "desktop_refused",
        when: "the paired application declined the requested Solar operation",
        remedy: "read the refusal detail, correct the application state, and retry",
    },
    Refusal {
        code: "desktop_contract_mismatch",
        when: "the paired session returned a reply outside this command contract",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "pairing_rejected",
        when: "the descriptor's pairing secret is stale",
        remedy: "restart DS GridDesign to publish a fresh descriptor",
    },
];

/// Desktop availability is checked at invocation, not discovery. A capability
/// descriptor remains useful on a machine where the application is not
/// running; `desktop_not_paired` says exactly how to make it runnable.
pub fn available() -> Availability {
    Availability::Available
}

/// Invoke one fixed paired Solar operation and reject an untyped reply.
pub fn invoke(
    inputs: &Inputs,
    operation: &'static str,
    arguments: Value,
    timeout: Duration,
) -> Result<Value, Failure> {
    let found = bridge::paired(inputs.value("desktop-descriptor"))?;
    let result = bridge::invoke(&found.descriptor, operation, arguments, timeout)?;
    if !result.is_object() || result["status"].as_str().is_none_or(str::is_empty) {
        return Err(Failure::unavailable(
            "desktop_contract_mismatch",
            format!("the paired session returned no status receipt for `{operation}`"),
        )
        .remedy("update DS GridDesign and ds to matching releases"));
    }
    Ok(result)
}

/// A run lifecycle receipt must identify the run that the next command reads.
pub fn require_run_id(result: Value, operation: &'static str) -> Result<Value, Failure> {
    if result["run_id"].as_str().is_none_or(str::is_empty) {
        return Err(Failure::unavailable(
            "desktop_contract_mismatch",
            format!("the paired session returned no run id for `{operation}`"),
        )
        .remedy("update DS GridDesign and ds to matching releases"));
    }
    Ok(result)
}
