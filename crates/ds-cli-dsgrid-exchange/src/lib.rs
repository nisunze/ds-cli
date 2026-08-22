//! `ds dsgrid-exchange` — turning other people's formats into a canonical
//! model, and back out again.
//!
//! This domain is where a `.dsgrid` comes from and where it goes: PLS-CADD
//! workspaces and backups in, GIS and tabular exports out, and composition of
//! several models into one. `ds dsgrid` answers questions about a model that
//! already exists; this domain manufactures one.
//!
//! The split is deliberate and structural. Every command in `ds dsgrid` is
//! `Discovery` or `ReadOnly`; this domain contains the only command in either
//! that writes files. Keeping them apart means a caller who wants to know
//! what a model is never reads a help screen about producing one, and the
//! blast radius of each domain is legible from its name.
//!
//! The three commands are one sequence, and the order is the contract:
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
//! Like `ds-cli-dsgrid`, this crate computes nothing. Classification, the
//! capability matrix, planning and execution all belong to
//! `ds-grid-exchange`. What lives here is argument handling, refusal mapping,
//! bounded projection, and the file-writing rules — never overwrite, never
//! rename an artifact the plan named.

pub mod convert;
pub mod inspect;
pub mod plan;
pub mod refusals;
pub mod render;
pub mod request;
pub mod sources;

use ds_cli_contract::spec::Domain;

pub static DOMAIN: Domain = Domain {
    id: "dsgrid-exchange",
    summary: "Import, export, compose: classify, plan, convert.",
    commands: &[&inspect::COMMAND, &plan::COMMAND, &convert::COMMAND],
};

/// A value's canonical serde token, rather than its `Debug` spelling.
///
/// Same rule as `ds-cli-dsgrid`'s `table_token`: the engine's enum variants
/// are `CamelCase` in Rust and `snake_case` on the wire, and `ds` reports the
/// wire spelling everywhere. Deriving it from the type's own `Serialize`
/// means a variant renamed upstream is renamed in this output too, instead of
/// quietly diverging into a token that no longer matches the engine's.
pub fn token<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}
