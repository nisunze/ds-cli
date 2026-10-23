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
//! The `project` family is the other door: the governed report service
//! produces a Compounded Report ZIP in the background for the
//! CLI-selected project, with no local engine, map, or Desktop. See
//! [`project`].
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

pub mod artifact;
pub mod bundle;
pub mod engine;
pub mod export;
pub mod inventory;
pub mod inventory_copy;
pub mod layout;
mod layout_preview;
pub mod mv_frontmatter;
pub mod outbox;
pub mod plan_profile;
pub mod plan_profile_config;
pub mod planning;
pub mod project;
pub mod publication;
pub mod publication_download;
pub mod retained_preview;
pub mod spatial_workbook;
pub mod standard;
pub mod style_keys;
pub mod tasks;
pub mod touch;
pub mod voltage_drop_combined;

use std::time::Duration;

use ds_cli_contract::spec::Domain;
pub use ds_cli_exec::addons::REPORTER as DS_REPORT;

/// Local consumers select a native lane without account/project acquisition.
const ENGINE_LANE: ds_cli_contract::spec::Arg = ds_cli_contract::spec::LANE
    .summary("Native addon lane; stable is the default. No account or project is read.");

const ENGINE_MISSING: ds_cli_contract::spec::Refusal = ds_cli_contract::spec::Refusal {
    code: "reporter_engine_missing",
    when: "the selected lane has no managed Reporter addon",
    remedy: "install Reporter for the selected --lane",
};
const ENGINE_RECEIPT_INVALID: ds_cli_contract::spec::Refusal = ds_cli_contract::spec::Refusal {
    code: "module_receipt_invalid",
    when: "the selected lane's managed receipt or executable bytes are invalid",
    remedy: "repair or roll back Reporter in the selected lane",
};

fn capture_engine(
    inputs: &ds_cli_contract::Inputs,
) -> Result<ds_cli_exec::CapturedExternal, ds_cli_contract::Failure> {
    capture_engine_at(inputs, &ds_cli_exec::addons::managed_module_base()?)
}

/// Native-owned relocation seam, never a command-supplied artifact path.
fn capture_engine_at(
    inputs: &ds_cli_contract::Inputs,
    base: &std::path::Path,
) -> Result<ds_cli_exec::CapturedExternal, ds_cli_contract::Failure> {
    DS_REPORT.capture_for_lane_in(inputs.value("lane").unwrap_or("stable"), base)
}

#[cfg(test)]
mod engine_selection_tests;

/// Discovery calls answer immediately or something is wrong.
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);

/// An export reads local bytes and writes artifacts. It performs no network
/// call at all — the reporter declares `"network": "none"` for both tasks —
/// so a long run is real work, not a stalled request.
pub const EXPORT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub static DOMAIN: Domain = Domain {
    id: "report",
    summary: "Deliverables: transformer and combined report artifacts.",
    commands: &[
        &artifact::REMOVE,
        &planning::TRANSFORMERS,
        &planning::PLAN,
        &layout::NEW,
        &layout::EDIT,
        &layout::SCHEMA,
        &layout::CONTEXT,
        &layout::SESSION,
        &layout::ADD,
        &layout::STYLE_REF_COMMAND,
        &layout::PENS,
        &layout::RENDER,
        &layout::LIST,
        &layout::GET,
        &layout::CREATE,
        &layout::UPDATE,
        &layout::SAVE,
        &layout::DELETE,
        &layout::COPY,
        &standard::LIST,
        &standard::GET,
        &style_keys::PLAN,
        &style_keys::APPLY,
        &engine::COMMAND,
        &tasks::COMMAND,
        &export::COMMAND,
        &voltage_drop_combined::COMMAND,
        &mv_frontmatter::COMMAND,
        &bundle::COMMAND,
        &plan_profile::COMMAND,
        &plan_profile_config::COMMAND,
        &plan_profile_config::SCHEMA,
        &spatial_workbook::COMMAND,
        &outbox::STATUS,
        &outbox::INVENTORY,
        &inventory::COMMAND,
        &inventory_copy::COMMAND,
        &outbox::DRAIN,
        &publication::LIST,
        &publication::SHOW,
        &publication_download::COMMAND,
        &retained_preview::READ,
        &retained_preview::REFRESH,
        &project::scope::COMMAND,
        &project::combined::COMMAND,
        &project::combined_export::COMMAND,
        &project::archives::COMMAND,
        &project::settings::COMMAND,
        &project::crs::COMMAND,
        &project::settings::OUTPUTS_SET,
        &project::mv_setup::SET,
        &project::mv_setup::RESOLVE,
        &project::export::COMMAND,
        &project::publish::COMMAND,
        &project::map_inputs::COMMAND,
    ],
};
