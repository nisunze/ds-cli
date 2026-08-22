//! `ds solar` — solar batches, over the `ds-solar` process contract.
//!
//! The `ds-solar` binary is one of three adapters over the same solar
//! runtime, and it models the same two-phase flow the product does. That
//! split is the domain's central rule, and this domain preserves it rather
//! than smoothing it over:
//!
//! * **`prepare` may reach the network.** It resolves weather cache-first and
//!   commits prepared inputs.
//! * **`run` may not.** It performs no intake and no network call of any
//!   kind, and receives only prepared inputs.
//!
//! Those are different effects and different failure modes, so they are
//! different commands with different declared contracts. Collapsing them into
//! one `ds solar batch` would hide exactly the property the desktop and cloud
//! paths depend on — and would make an offline run indistinguishable, from
//! the outside, from one that quietly fetched.

pub mod engine;
pub mod prepare;
pub mod run;
pub mod weather;

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

/// Preparation resolves weather, possibly over the network, for many cities.
pub const PREPARE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// A batch run is pure compute over prepared inputs and can legitimately take
/// a long time on a large city set.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

pub static DOMAIN: Domain = Domain {
    id: "solar",
    summary: "Solar batches: prepare inputs, run them offline, verify weather.",
    commands: &[
        &engine::COMMAND,
        &prepare::COMMAND,
        &run::COMMAND,
        &weather::COMMAND,
    ],
};
