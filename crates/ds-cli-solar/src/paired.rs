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
use ds_cli_desktop::ops::{self, BridgeOp};
use serde_json::Value;

pub const PREPARE_OP: BridgeOp = BridgeOp {
    operation: "solar.prepare",
    arguments: &["contexts", "overwrite", "language"],
};
pub const RUN_START_OP: BridgeOp = BridgeOp {
    operation: "solar.run.start",
    arguments: &[
        "contexts",
        "portfolio",
        "membership_revision",
        "currency",
        "project_years",
        "discount_rate",
        "representative_city",
        "language",
        "report_intents",
        "render_charts",
        "concurrency",
        "serial",
    ],
};
pub const RUN_PROGRESS_OP: BridgeOp = BridgeOp {
    operation: "solar.run.progress",
    arguments: &["run_id"],
};
pub const RUN_RESULT_OP: BridgeOp = BridgeOp {
    operation: "solar.run.result",
    arguments: &["run_id"],
};
pub const RUN_CANCEL_OP: BridgeOp = BridgeOp {
    operation: "solar.run.cancel",
    arguments: &["run_id"],
};
pub const RESULT_READ_OP: BridgeOp = BridgeOp {
    operation: "solar.result.read",
    arguments: &["run_id", "context", "path"],
};
pub const RESULTS_READ_OP: BridgeOp = BridgeOp {
    operation: "solar.results.read",
    arguments: &["run_id", "context", "section", "path"],
};
pub const SYNC_STATUS_OP: BridgeOp = BridgeOp {
    operation: "solar.sync.status",
    arguments: &["run_id"],
};
pub const PORTFOLIO_LIST_OP: BridgeOp = BridgeOp {
    operation: "solar.portfolio.list",
    arguments: &[],
};
pub const FINAL_IMPORT_OP: BridgeOp = BridgeOp {
    operation: "solar.final.import",
    arguments: &["run_id", "context", "source_path"],
};
pub const FINAL_SUBMIT_OP: BridgeOp = BridgeOp {
    operation: "solar.final.submit",
    arguments: &["run_id", "context"],
};
pub const DOCUMENT_READ_OP: BridgeOp = BridgeOp {
    operation: "solar.document.read",
    arguments: &["run_id", "context", "document", "offset"],
};
pub const PORTFOLIO_READ_OP: BridgeOp = BridgeOp {
    operation: "solar.portfolio.read",
    arguments: &["run_id", "artifact", "offset"],
};

/// Every paired operation this crate can send, with its exact wire keys.
///
/// Dispatch still names a fixed operation in source. This declaration adds a
/// runtime key guard and gives bridge-parity tests one machine-readable source
/// of truth to compare with the paired application.
pub const BRIDGE_OPS: &[&BridgeOp] = &[
    &crate::seed::PREVIEW_OP,
    &crate::seed::APPLY_OP,
    &PREPARE_OP,
    &RUN_START_OP,
    &RUN_PROGRESS_OP,
    &RUN_RESULT_OP,
    &RUN_CANCEL_OP,
    &RESULT_READ_OP,
    &RESULTS_READ_OP,
    &SYNC_STATUS_OP,
    &PORTFOLIO_LIST_OP,
    &FINAL_IMPORT_OP,
    &FINAL_SUBMIT_OP,
    &DOCUMENT_READ_OP,
    &PORTFOLIO_READ_OP,
];

/// Declared operations whose CLI door the paired application has not landed.
///
/// ds-web shipped the Solar seeding CARD (`src/lib/api/solar-seed.ts` and
/// `src/lib/solar/seed-runtime.ts`), which reaches ds-brain through
/// `brainOperation` from the UI. It has not yet added the two operations to
/// `CLI_OPERATIONS` and its CLI dispatcher, so `ds solar seed` currently
/// refuses with `desktop_operation_unsupported` — a named refusal with a
/// remedy, which is the right shipping state for a door that is not open yet.
///
/// This list is a GAP RECORD, not an exemption. `bridge_parity.rs` holds the
/// server contract these operations carry against ds-web's own seeding client
/// today, and fails the moment the application does land one of them so it is
/// promoted to the fully checked set deliberately rather than drifting.
pub const PENDING_DESKTOP_OPS: &[&str] = &[
    crate::seed::PREVIEW_OP.operation,
    crate::seed::APPLY_OP.operation,
];

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

/// `solar.run.start` validates its mutually exclusive launch modes and the
/// explicit portfolio assumptions before asking the desktop. It gets its own
/// list so command help names those local refusals.
pub static START_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "invalid_run_selection",
        when: "the launch has both --city and --portfolio, neither one, or a blank portfolio id",
        remedy: "pass one or more --city values, or one exact --portfolio with its explicit inputs",
    },
    Refusal {
        code: "portfolio_only_input",
        when: "a portfolio assumption, language or report intent was passed with --city",
        remedy: "remove the portfolio-only flags, or replace --city with one --portfolio",
    },
    Refusal {
        code: "missing_portfolio_input",
        when: "a portfolio launch omits an explicit assumption, representative city, language or report intent",
        remedy: "pass every portfolio-only input and at least one --report value",
    },
    Refusal {
        code: "invalid_membership_revision",
        when: "--membership-revision is not the exact lowercase SHA-256 value returned by portfolio list",
        remedy: "list portfolios again and pass the selected row's exact membership_revision",
    },
    Refusal {
        code: "invalid_currency",
        when: "--currency is not exactly three uppercase ASCII letters",
        remedy: "pass an explicit three-letter currency such as XAF or USD",
    },
    Refusal {
        code: "invalid_project_years",
        when: "--project-years is not a whole number from 1 through 100",
        remedy: "pass an explicit integer from 1 through 100",
    },
    Refusal {
        code: "invalid_discount_rate",
        when: "--discount-rate is not finite, is negative, or is 1 or greater",
        remedy: "pass an explicit decimal rate from 0 inclusive to 1 exclusive",
    },
    Refusal {
        code: "invalid_representative_city",
        when: "--representative-city is blank, padded or longer than 128 bytes",
        remedy: "pass one exact member id from the selected portfolio",
    },
    Refusal {
        code: "duplicate_report_intent",
        when: "the same --report intent is passed more than once",
        remedy: "pass each report intent at most once",
    },
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
    let op = BRIDGE_OPS
        .iter()
        .copied()
        .find(|candidate| candidate.operation == operation)
        .ok_or_else(|| {
            Failure::internal(
                "undeclared_bridge_argument",
                format!("Solar operation `{operation}` is not declared in BRIDGE_OPS"),
            )
            .remedy("this is a defect in ds; report it with the command you ran")
        })?;
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    let result = ops::invoke(&descriptor, op, arguments, timeout)?;
    if !result.is_object() || result["status"].as_str().is_none_or(str::is_empty) {
        return Err(Failure::unavailable(
            "desktop_contract_mismatch",
            format!("the paired session returned no status receipt for `{operation}`"),
        )
        .remedy("update DS GridDesign and ds to matching releases"));
    }
    Ok(result)
}

/// A launch receipt must identify the new run that later commands will read.
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

/// A non-start receipt must echo the exact identity the caller addressed.
///
/// Merely returning some non-empty run or city would let a stale or malicious
/// paired session splice receipts from different calculations. Keep this check
/// beside the closed-operation guard so every Solar adapter fails the same way.
pub fn require_exact_identity(
    result: Value,
    operation: &'static str,
    expected_run_id: &str,
    expected_context: Option<&str>,
) -> Result<Value, Failure> {
    let run_matches = result["run_id"].as_str() == Some(expected_run_id);
    let context_matches = expected_context
        .map(|context| result["context"].as_str() == Some(context))
        .unwrap_or(true);
    if !run_matches || !context_matches {
        let identity = if expected_context.is_some() {
            "run id and city context"
        } else {
            "run id"
        };
        return Err(Failure::unavailable(
            "desktop_contract_mismatch",
            format!("the paired session returned a different {identity} for `{operation}`"),
        )
        .remedy("update DS GridDesign and ds to matching releases"));
    }
    Ok(result)
}
