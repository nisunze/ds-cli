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
/// Unlike `ds-report`, `ds-solar` is **not** a bundled desktop sidecar, and
/// the remedy says so precisely rather than pointing at a component that will
/// not install.
///
/// The solar *engine* is already shipped: `ds-solar-engine` and
/// `ds-solar-contracts` are linked into the desktop application, and
/// `ds-web`'s own packaging contract records why — "the native Rust Solar
/// runtime is ordinary linked source, not an installed component, so a
/// `solar` component row may only linger in a dormant, pin-free state until
/// it is deleted from the catalog entirely"
/// (`desktop-build-windows.sh`). The `solar` component in
/// `desktop-components.json` is that dormant row: `planned`, pin-free, and on
/// its way out.
///
/// So "install the solar component" would be a remedy a caller cannot follow.
/// What they can do is point at a built `ds-solar`. See
/// `docs/reference/solar.md` for the two ways this becomes available on a
/// stock install, and why neither is a 27 MB sidecar duplicate of an engine
/// the application already carries.
pub static DS_SOLAR: External = External {
    name: "ds-solar",
    env_override: "DS_SOLAR_BIN",
    owner: "ds-solar",
    remedy: "set DS_SOLAR_BIN to a built ds-solar (cargo build --release --package ds-solar-cli)",
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
