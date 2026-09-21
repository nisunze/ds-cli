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
//! domain creates a blank canonical package or reads/revision-gates an
//! existing one. `create` calls the shared blank-model authority; `apply`
//! writes a new package, never the source. Conversion from a foreign format
//! remains in the exchange domain.
//!
//! One family in this domain reaches an owner instead of linking one. A
//! *local model* is a live session and a durable store inside the running
//! application, not a file, so [`model`] asks the paired application for each
//! named transition. Its module header states the boundary that family holds;
//! nothing in it manufactures a `.dsgrid` either.
//!
//! The typed command family (`structure describe|retype`, `report
//! structures`, program contract 01 §2) shares one plumbing, [`mutation`]:
//! target selection (`--model` working copy revised in place, or `--package`
//! → `--out`), the revision pin, dry-run/--yes, and one receipt shape. A new
//! typed verb is its inputs, its engine commands and its render; nothing
//! else.

pub mod analyse;
pub mod apply;
pub mod backup;
pub mod create;
pub mod criteria;
pub mod describe;
pub mod feature_codes;
pub mod folder;
pub mod import_structure;
pub mod inspect;
pub mod model;
pub mod mutation;
pub mod objects;
pub mod package;
pub mod profile;
pub mod project;
pub mod report;
pub mod run;
pub mod structure;
pub mod validate;

use ds_cli_contract::spec::Domain;

pub static DOMAIN: Domain = Domain {
    id: "dsgrid",
    summary: "Canonical .dsgrid models: inspect, validate, revise, publish.",
    commands: &[
        &backup::COMMAND,
        &project::LIST,
        &project::DOWNLOAD,
        &create::COMMAND,
        &import_structure::COMMAND,
        &inspect::COMMAND,
        &validate::COMMAND,
        &describe::COMMAND,
        &run::COMMAND,
        &apply::COMMAND,
        &model::list::COMMAND,
        &model::show::COMMAND,
        &model::create_local::COMMAND,
        &model::import_external::COMMAND,
        &model::link::COMMAND,
        &model::set_active::COMMAND,
        &model::forget::COMMAND,
        &model::prepare_project::COMMAND,
        &model::publish_version::COMMAND,
        &structure::describe::COMMAND,
        &structure::retype::COMMAND,
        &report::structures::COMMAND,
        &feature_codes::report::COMMAND,
        &feature_codes::import::COMMAND,
        &feature_codes::migrate::COMMAND,
        &feature_codes::export::COMMAND,
        &criteria::show::COMMAND,
        &criteria::clearance_set::COMMAND,
        &analyse::clearance::COMMAND,
    ],
};
