//! `ds solar` — native Server application and thin compatibility adapters.
//!
//! `application schema` derives closed requests from the Rust Solar owner;
//! `application` invokes them with captured native account/project authority.
//! Preparation, calculation, artifact selection and delivery need no Desktop.
//! `run --prepared ... --out ...` invokes the separate offline city-artifact
//! process contract for already prepared inputs. All routes call Rust owners.

mod local_artifact;

pub mod application;
pub mod calculate_prepared;
pub mod cities;
pub mod compare;
pub mod dashboard;
pub mod engine;
pub mod input_capture;
pub mod input_prepare;
pub mod migrate;
pub mod network_form;
pub mod network_map;
pub mod network_seed;
pub mod portfolio_headless;
pub mod portfolio_management;
pub mod project;
mod project_worker;
pub mod project_sync;
pub mod reference;
pub mod run;
pub mod weather;
pub mod workflow;

use std::time::Duration;

use ds_cli_contract::spec::Domain;
/// The Kernel's one Solar addon client: closed tasks, catalogue checked first.
pub use ds_cli_exec::solar::{Solar, Task as SolarTask};

/// Local file consumers name a native lane without acquiring account/project
/// state. The supervisor captures one immutable selection for the whole call.
const ENGINE_LANE: ds_cli_contract::spec::Arg = ds_cli_contract::spec::LANE
    .summary("Native addon lane; stable is the default. No account or project is read.");

fn capture_engine(inputs: &ds_cli_contract::Inputs) -> Result<Solar, ds_cli_contract::Failure> {
    capture_engine_at(inputs, &ds_cli_exec::addons::managed_module_base()?)
}

/// Privileged native relocation seam, never a command-supplied artifact path.
fn capture_engine_at(
    inputs: &ds_cli_contract::Inputs,
    base: &std::path::Path,
) -> Result<Solar, ds_cli_contract::Failure> {
    Solar::capture_for_lane_in(inputs.value("lane").unwrap_or("stable"), base)
}

#[cfg(test)]
mod engine_selection_tests;
#[cfg(test)]
#[path = "../../../kernel/ds-compute-runtime/tests/common/mod.rs"]
mod prepared_fixture;

pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);

/// A batch run is pure compute over prepared inputs and can legitimately take
/// a long time on a large city set.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

pub static DOMAIN: Domain = Domain {
    id: "solar",
    summary: "Solar preparation, local run lifecycle and artifact execution.",
    commands: &[
        &application::COMMAND,
        &calculate_prepared::COMMAND,
        &application::SCHEMA,
        &dashboard::COMMAND,
        &network_form::COMMAND,
        &network_map::COMMAND,
        &network_form::SAVE,
        &cities::COMMAND,
        &reference::PLAN,
        &reference::IMPORT,
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
        &project::CLEAN,
        &engine::COMMAND,
        &compare::COMMAND,
        &input_capture::COMMAND,
        &input_prepare::COMMAND,
        &run::COMMAND,
        &workflow::RESULTS_READ_COMMAND,
        &workflow::PORTFOLIO_LIST_COMMAND,
        &portfolio_headless::CALCULATE,
        &portfolio_headless::PUBLISHED_READ,
        &portfolio_management::CREATE_COMMAND,
        &portfolio_management::UPDATE_COMMAND,
        &portfolio_management::DELETE_COMMAND,
        &workflow::PORTFOLIO_ANALYSIS_COMMAND,
        &migrate::PLAN_COMMAND,
        &migrate::APPLY_COMMAND,
        &network_seed::COMMAND,
        &weather::COMMAND,
    ],
};
