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

pub mod compare;
pub mod engine;
pub mod exports;
pub mod input_capture;
pub mod input_prepare;
pub mod network_seed;
pub mod paired;
pub mod paired_run;
pub mod portfolio_batch;
pub mod portfolio_management;
pub mod prepare;
pub mod project;
pub mod project_sync;
pub mod run;
pub mod seed;
pub mod weather;
pub mod workflow;

use std::time::Duration;

use ds_cli_contract::spec::Domain;
use ds_cli_exec::External;

/// The solar binary.
///
/// The desktop links the Solar runtime for paired product work. Both desktop
/// and headless ds releases package this sibling process for the deliberately
/// separate artifact route, built from the same release-pinned `ds-solar`
/// checkout. `DS_SOLAR_BIN` remains an explicit developer override; an
/// installed build resolves its packaged sibling first and needs no
/// configuration.
pub static DS_SOLAR: External = External {
    name: "ds-solar",
    env_override: "DS_SOLAR_BIN",
    owner: "ds-solar",
    remedy: "reinstall the complete ds release, or for development set DS_SOLAR_BIN to an exact compatible ds-solar",
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
        &project::REBASE,
        &project::CITY_READ,
        &project::CITY_WRITE,
        &project_sync::COMMAND,
        &project::INIT,
        &project::SEED,
        &project::RUN,
        &project::STATUS,
        &project::RESULT,
        &project::OUTBOX,
        &engine::COMMAND,
        &compare::COMMAND,
        &input_capture::COMMAND,
        &input_prepare::COMMAND,
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
        &portfolio_batch::START_COMMAND,
        &portfolio_batch::STATUS_COMMAND,
        &portfolio_batch::CANCEL_COMMAND,
        &portfolio_management::CREATE_COMMAND,
        &portfolio_management::UPDATE_COMMAND,
        &portfolio_management::DELETE_COMMAND,
        &workflow::PORTFOLIO_READ_COMMAND,
        &workflow::PORTFOLIO_ANALYSIS_COMMAND,
        &workflow::FINAL_IMPORT_COMMAND,
        &workflow::FINAL_SUBMIT_COMMAND,
        &exports::REPORT_EXPORT_COMMAND,
        &exports::REPORT_BUNDLE_COMMAND,
        &exports::PORTFOLIO_EXPORT_COMMAND,
        &seed::PREVIEW_COMMAND,
        &seed::APPLY_COMMAND,
        &network_seed::COMMAND,
        &weather::COMMAND,
    ],
};
