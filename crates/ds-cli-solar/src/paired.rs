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

pub const PORTFOLIO_BATCH_START_OP: BridgeOp = BridgeOp {
    operation: "solar.portfolio.batch.start",
    arguments: &["request"],
};
pub const PORTFOLIO_BATCH_STATUS_OP: BridgeOp = BridgeOp {
    operation: "solar.portfolio.batch.status",
    arguments: &["run_id"],
};
pub const PORTFOLIO_BATCH_CANCEL_OP: BridgeOp = BridgeOp {
    operation: "solar.portfolio.batch.cancel",
    arguments: &["run_id"],
};
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
        "graph_strategy",
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
pub const REPORT_BUNDLE_READ_OP: BridgeOp = BridgeOp {
    operation: "solar.report.bundle.read",
    arguments: &["run_id", "context", "document", "offset"],
};
pub const PORTFOLIO_READ_OP: BridgeOp = BridgeOp {
    operation: "solar.portfolio.read",
    arguments: &["run_id", "artifact", "offset"],
};
/// The saved-analysis question, asked by portfolio id alone.
///
/// `solar.portfolio.list` answers membership and `solar.portfolio.read` needs a
/// completed run id, so neither could say whether a governed portfolio has a
/// saved analysis at all. The application answers this from the same projection
/// its Pipeline panel renders, so the two cannot disagree.
pub const PORTFOLIO_ANALYSIS_OP: BridgeOp = BridgeOp {
    operation: "solar.portfolio.analysis",
    arguments: &["portfolio_id"],
};

/// Every paired operation this crate can send, with its exact wire keys.
///
/// Dispatch still names a fixed operation in source. This declaration adds a
/// runtime key guard and gives bridge-parity tests one machine-readable source
/// of truth to compare with the paired application.
pub const BRIDGE_OPS: &[&BridgeOp] = &[
    &crate::seed::PREVIEW_OP,
    &crate::seed::APPLY_OP,
    &PORTFOLIO_BATCH_START_OP,
    &PORTFOLIO_BATCH_STATUS_OP,
    &PORTFOLIO_BATCH_CANCEL_OP,
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
    &REPORT_BUNDLE_READ_OP,
    &PORTFOLIO_READ_OP,
    &PORTFOLIO_ANALYSIS_OP,
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
/// exact portfolio membership and graph strategy before asking the desktop. It gets its own
/// list so command help names those local refusals.
pub static START_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "invalid_run_selection",
        when: "the launch has both --city and --portfolio, neither one, or a blank portfolio id",
        remedy: "pass one or more --city values, or one exact --portfolio with its membership revision and graph strategy",
    },
    Refusal {
        code: "portfolio_only_input",
        when: "portfolio membership or graph strategy was passed with --city",
        remedy: "remove the portfolio-only flags, or replace --city with one --portfolio",
    },
    Refusal {
        code: "missing_portfolio_input",
        when: "a portfolio launch omits its exact membership revision or graph strategy",
        remedy: "pass --membership-revision and one --graph-strategy value with --portfolio",
    },
    Refusal {
        code: "invalid_membership_revision",
        when: "--membership-revision is not the exact lowercase SHA-256 value returned by portfolio list",
        remedy: "list portfolios again and pass the selected row's exact membership_revision",
    },
    Refusal {
        code: "invalid_graph_strategy",
        when: "--graph-strategy is not first, round-robin, or city:<exact-member-id>",
        remedy: "choose first, round-robin, or prefix one exact portfolio member id with city:",
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
