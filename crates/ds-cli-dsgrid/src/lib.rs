//! `ds dsgrid` — the canonical grid model.
//!
//! This domain answers questions about a `.dsgrid` package someone already
//! has — what is it, what is in it, is it sound — and makes typed,
//! revision-pinned edits to one. It is a transport, not an engine. Every
//! decode, count, fingerprint, validation issue, plan, transaction and emitted
//! package comes from the independently installed Network addon, reached
//! through the closed typed process client in `ds-network-client`
//! (DECOUPLING-003). Core keeps what Core owns: argument admission, target
//! resolution, this machine's working-copy catalogue, project authority,
//! confirmation, output files and bounded presentation. No Grid engine,
//! exchange or composition crate is linked; a missing or older addon refuses
//! before any work.
//!
//! Manufacturing a `.dsgrid` from a foreign source — classifying, planning
//! and executing a conversion — is deliberately *not* here; it belongs to the
//! exchange domain. `create` asks the addon for the shared blank model;
//! every edit writes a new package or the working copy's next revision,
//! never the source in place.
//!
//! One family in this domain reaches the paired application instead: a live
//! Profile session ([`profile`]) and project publication of a held model are
//! the application's own transitions.
//!
//! The typed command family (`structure describe|retype`, `criteria
//! clearance set`) shares one host plumbing, [`mutation`]: target selection
//! (`--model` working copy revised in place, or `--package` → `--out`), the
//! revision pin, dry-run/--yes, and one receipt shape. The engine half of each
//! verb is one closed Network operation.
//!
//! Commands of the linked surface that have not yet been restored through the
//! addon are recorded in `ds-architecture/backlog/DECOUPLING.md`
//! (DECOUPLING-003) with the source revision that retains them.

pub mod analyse;
pub mod apply;
pub mod apply_batch;
pub mod apply_receipt;
pub mod create;
pub mod criteria;
pub mod describe;
pub mod feature_codes;
pub mod folder;
mod host_memory;
pub mod inspect;
pub mod model;
pub mod mutation;
mod native;
pub mod package;
pub mod package_migrate;
pub mod preview_receipt;
pub mod profile;
pub mod project;
pub mod report;
pub mod run;
mod spotting_receipt;
pub mod structure;
pub mod validate;

use ds_cli_contract::spec::Domain;

pub static DOMAIN: Domain = Domain {
    id: "dsgrid",
    summary: "Canonical .dsgrid models: inspect, validate, revise, publish.",
    commands: &[
        &package_migrate::COMMAND,
        &project::LIST,
        &project::SHOW,
        &project::VERSIONS,
        &project::DOWNLOAD,
        &project::exports::LIST,
        &project::exports::PUBLISH,
        &project::exports::DOWNLOAD,
        &project::governance::BUMP_VERSION,
        &project::governance::UPDATE,
        &project::governance::SET_APPROVAL,
        &project::governance::BACKUP_DOWNLOAD,
        &project::RETIRE,
        &project::RESTORE,
        &create::COMMAND,
        &inspect::COMMAND,
        &validate::COMMAND,
        &describe::COMMAND,
        &run::COMMAND,
        &preview_receipt::COMMAND,
        &apply_receipt::COMMAND,
        &apply::COMMAND,
        &apply_batch::COMMAND,
        &apply_batch::CORRECTION,
        &profile::open::COMMAND,
        &model::list::COMMAND,
        &model::show::COMMAND,
        &model::create_local::COMMAND,
        &model::import_external::COMMAND,
        &model::link::COMMAND,
        &model::unlink::COMMAND,
        &model::set_active::COMMAND,
        &model::forget::COMMAND,
        &model::prepare_project::COMMAND,
        &structure::describe::COMMAND,
        &structure::retype::COMMAND,
        &report::structures::COMMAND,
        &report::staking::COMMAND,
        &feature_codes::report::COMMAND,
        &criteria::show::COMMAND,
        &criteria::clearance_set::COMMAND,
        &analyse::clearance::COMMAND,
    ],
};

#[cfg(test)]
#[path = "../tests/support/linked_oracle.rs"]
mod linked_oracle;
