//! `ds network` — the canonical grid model.
//!
//! This domain is a transport, not an engine. It reads bytes, hands them to
//! the `ds-network` crates the desktop application already links, and shapes
//! the result into a bounded projection. It computes nothing: every count,
//! fingerprint, extent and validation issue it prints came out of
//! `ds-grid-model`, `ds-grid-engine` or `ds-grid-exchange`. A second
//! implementation of any of that here would be a second answer to a question
//! that must have one.

pub mod convert;
pub mod describe;
pub mod inspect;
pub mod package;
pub mod validate;

use ds_cli_contract::spec::Domain;

pub static DOMAIN: Domain = Domain {
    id: "network",
    summary: "Canonical grid models: identity, inventory and validation.",
    commands: &[
        &inspect::COMMAND,
        &validate::COMMAND,
        &convert::COMMAND,
        &describe::COMMAND,
    ],
};
