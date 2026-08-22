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
//! domain is `Discovery` or `ReadOnly` and can run against a file the caller
//! did not author, while the exchange domain writes. Keeping them in one
//! domain would put "tell me what this is" and "make me a new one" behind the
//! same help screen and the same blast radius.

pub mod describe;
pub mod inspect;
pub mod package;
pub mod validate;

use ds_cli_contract::spec::Domain;

pub static DOMAIN: Domain = Domain {
    id: "dsgrid",
    summary: "Canonical .dsgrid models: identity, inventory, validation.",
    commands: &[&inspect::COMMAND, &validate::COMMAND, &describe::COMMAND],
};
