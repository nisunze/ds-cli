//! `ds dsgrid` — the canonical grid model.
//!
//! This domain answers questions about a `.dsgrid` package someone already
//! has: what is it, what is in it, and is it sound. It is a transport, not an
//! engine. It reads bytes, hands them to the `ds-network` crates the desktop
//! application already links, and shapes the result into a bounded
//! projection. It computes nothing: every count, fingerprint, extent and
//! validation issue it prints came out of `ds-grid-model`, `ds-grid-engine`
//! or `ds-grid-exchange`. A second implementation of any of that here would
//! be a second answer to a question that must have one.
//!
//! Manufacturing a `.dsgrid` — classifying foreign sources, planning a
//! conversion, executing one — is deliberately *not* here. It lives in
//! `ds-cli-dsgrid-exchange`. The split is not tidiness: every command in this
//! domain reads or revision-gates one canonical package the caller already
//! has. `apply` writes a new package, never the source; manufacturing from a
//! foreign format remains in the exchange domain.

pub mod apply;
pub mod describe;
pub mod inspect;
pub mod output;
pub mod package;
pub mod project;
pub mod validate;

use ds_cli_contract::spec::Domain;

pub static DOMAIN: Domain = Domain {
    id: "dsgrid",
    summary: "Canonical .dsgrid models: inspect, validate, describe, revise.",
    commands: &[
        &inspect::COMMAND,
        &validate::COMMAND,
        &describe::COMMAND,
        &project::COMMAND,
        &apply::COMMAND,
    ],
};
