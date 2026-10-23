//! `ds dsgrid-exchange` — turning other people's formats into a canonical
//! model, and back out again.
//!
//! This domain is where a `.dsgrid` comes from and where it goes: PLS-CADD
//! workspaces and backups in, PLS-CADD backups, GIS and tabular exports out,
//! and composition of several models into one. `ds dsgrid` answers questions
//! about a model that already exists; this domain manufactures one.
//!
//! The split is deliberate and structural. Every command in `ds dsgrid` is
//! `Discovery` or `ReadOnly`; this domain contains the only command in either
//! that writes files. Keeping them apart means a caller who wants to know
//! what a model is never reads a help screen about producing one, and the
//! blast radius of each domain is legible from its name.
//!
//! Three commands are one sequence, and the order is the contract:
//!
//! ```text
//! inspect   what are these files, and what could they become?
//! plan      exactly what would a conversion do — and what would it lose?
//! convert   do that, and nothing else
//! ```
//!
//! `plan` pins the digest of every source it reads; `convert` re-digests them
//! and refuses if any changed. That is what makes the sequence worth
//! following rather than skipping to the end: the plan is a commitment, not a
//! guess. It is also why there is no `convert --dry-run` — a dry run is a
//! mode of one command, while these are two effect classes.
//!
//! This crate computes nothing and links no Grid engine (DECOUPLING-003).
//! Classification, the capability matrix, planning and execution belong to
//! the exchange owner in the independently installed Network addon, reached
//! through the closed `grid-operation` exchange calls
//! (`grid_exchange_operations_v1`). What lives here is argument handling,
//! source staging, refusal mapping, bounded presentation, and the
//! file-writing rules — never overwrite, never rename an artifact the plan
//! named. `sync` (writing a working copy's edits back into its linked
//! workspace) stays retired until its own closed operation returns.

pub mod convert;
pub mod inspect;
pub mod plan;
pub mod refusals;
pub mod render;
pub mod request;
pub mod sources;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Domain;
use ds_network_contracts::grid_operation::GridOperationV1;

pub static DOMAIN: Domain = Domain {
    id: "dsgrid-exchange",
    summary: "Import and export: classify, plan, convert.",
    commands: &[&inspect::COMMAND, &plan::COMMAND, &convert::COMMAND],
};

/// Run one exchange call in the captured Network addon over the staged
/// sources, keeping the owner's refusal exactly as it raised it.
pub(crate) fn exchange(
    call: &GridOperationV1,
    inputs: &[(String, Vec<u8>)],
) -> Result<ds_network_client::GridOperationOutcome, Failure> {
    ds_cli_dsgrid::native::operation(call, None, inputs)
}

/// The typed result of one exchange call.
pub(crate) fn result<T: serde::de::DeserializeOwned>(
    outcome: ds_network_client::GridOperationOutcome,
) -> Result<T, Failure> {
    serde_json::from_value(outcome.result.result)
        .map_err(|error| Failure::failed("network_result_invalid", error.to_string()))
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
