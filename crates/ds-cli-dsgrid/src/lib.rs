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
//!
//! One family in this domain reaches an owner instead of linking one. A
//! *local model* is a live session and a durable store inside the running
//! application, not a file, so [`model`] asks the paired application for each
//! named transition. Its module header states the boundary that family holds;
//! nothing in it manufactures a `.dsgrid` either.

pub mod apply;
pub mod describe;
pub mod inspect;
pub mod model;
pub mod package;
pub mod run;
pub mod validate;

use ds_cli_contract::spec::Domain;

pub static DOMAIN: Domain = Domain {
    id: "dsgrid",
    summary: "Canonical .dsgrid models: inspect, validate, revise, publish.",
    commands: &[
        &inspect::COMMAND,
        &validate::COMMAND,
        &describe::COMMAND,
        &run::COMMAND,
        &apply::COMMAND,
        &model::list::COMMAND,
        &model::create_local::COMMAND,
        &model::import_external::COMMAND,
        &model::set_active::COMMAND,
        &model::publish_version::COMMAND,
    ],
};
