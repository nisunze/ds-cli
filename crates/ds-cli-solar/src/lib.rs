//! `ds solar` — paired product lifecycle plus headless artifact runner.
//!
//! The paired product route keeps the cache boundary in DS GridDesign:
//! `prepare` captures selected city input cache-first, and `run start` plus
//! its lifecycle commands execute and observe native local Solar. The CLI
//! sends closed semantic operations only; it never reads IndexedDB or carries
//! cache paths, URLs, or credentials.
//!
//! `run` remains the separately useful headless artifact runner over an
//! already prepared directory and the external `ds-solar` process contract.
//! Longest-path dispatch makes `ds solar run start` the paired product launch
//! while preserving `ds solar run --prepared ... --out ...` for reproducible
//! offline artifact work.

pub mod engine;
pub mod exports;
pub mod paired;
pub mod paired_run;
pub mod prepare;
pub mod run;
pub mod weather;
pub mod workflow;

use std::time::Duration;

use ds_cli_contract::spec::Domain;
use ds_cli_exec::External;

/// The solar binary.
///
/// The desktop links the Solar runtime for paired product work and packages
/// this sibling process for the deliberately separate headless artifact
/// route. Both are built from the same release-pinned `ds-solar` checkout.
/// `DS_SOLAR_BIN` remains an explicit developer override; an installed build
/// resolves its packaged sibling first and needs no configuration.
pub static DS_SOLAR: External = External {
    name: "ds-solar",
    env_override: "DS_SOLAR_BIN",
    owner: "ds-solar",
    remedy: "reinstall DS GridDesign, or for development set DS_SOLAR_BIN to an exact compatible ds-solar",
    missing_code: "solar_engine_missing",
};

pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);

/// Paired preparation may capture or refresh city input for many cities.
pub const PREPARE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// A batch run is pure compute over prepared inputs and can legitimately take
/// a long time on a large city set.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

pub static DOMAIN: Domain = Domain {
    id: "solar",
    summary: "Solar preparation, local run lifecycle and artifact execution.",
    commands: &[
        &engine::COMMAND,
        &prepare::COMMAND,
        &run::COMMAND,
        &paired_run::START_COMMAND,
        &paired_run::PROGRESS_COMMAND,
        &paired_run::RESULT_COMMAND,
        &paired_run::CANCEL_COMMAND,
        &paired_run::READ_COMMAND,
        &workflow::RESULTS_READ_COMMAND,
        &workflow::SYNC_STATUS_COMMAND,
        &workflow::PORTFOLIO_LIST_COMMAND,
        &workflow::FINAL_IMPORT_COMMAND,
        &exports::REPORT_EXPORT_COMMAND,
        &exports::PORTFOLIO_EXPORT_COMMAND,
        &weather::COMMAND,
    ],
};
