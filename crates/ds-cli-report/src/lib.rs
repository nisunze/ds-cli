//! `ds report` — deliverables, over the reporter's own process contract.
//!
//! `ds-network-reporter` publishes exactly one surface an agent host may
//! call, and wrote down why. Its binary's header is the contract:
//!
//! > one named subcommand per call — never a caller-supplied argv … a typed
//! > request file — not flags built from model output … a machine-readable
//! > result document — never parsed stdout prose.
//!
//! So this domain does not link the reporter's library and does not
//! reimplement any part of it. It builds a typed request, names one
//! subcommand, and reads the document that comes back.
//!
//! Two of the reporter's rules shape everything here:
//!
//! * **The result file must not already exist**, and there is no `--force`.
//!   A caller that finds a stale document where its answer should be cannot
//!   tell the difference between this run and the last one.
//! * **A failed task still writes its document** and exits non-zero. The
//!   blockers are *in the file*; the exit status is only the coarse signal.
//!   That is precisely the shape `ds` exists to improve on — see
//!   [`export`], which reads the document either way and returns typed
//!   blockers instead of an exit code and a path.

pub mod engine;
pub mod export;
pub mod tasks;

use std::time::Duration;

use ds_cli_contract::spec::Domain;
use ds_cli_exec::External;

/// The reporter binary. `ds-report` is already a bundled desktop sidecar, so
/// on an installed machine it sits next to `ds` and needs no configuration.
pub static DS_REPORT: External = External {
    name: "ds-report",
    env_override: "DS_REPORT_BIN",
    owner: "ds-network-reporter",
    remedy: "install the DS GridDesign desktop, or set DS_REPORT_BIN to a built ds-report",
    missing_code: "reporter_engine_missing",
};

/// Discovery calls answer immediately or something is wrong.
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);

/// An export reads local bytes and writes artifacts. It performs no network
/// call at all — the reporter declares `"network": "none"` for both tasks —
/// so a long run is real work, not a stalled request.
pub const EXPORT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub static DOMAIN: Domain = Domain {
    id: "report",
    summary: "Deliverables: transformer and combined report artifacts.",
    commands: &[&engine::COMMAND, &tasks::COMMAND, &export::COMMAND],
};
