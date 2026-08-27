//! `ds workstation` — inspect prerequisites and review setup plans.
//!
//! This crate deliberately owns no installer yet. Discovery, verification,
//! provenance rules, and no-side-effect plans are stable enough to expose;
//! machine mutation remains unavailable until its Windows lifecycle proof is
//! complete. That boundary prevents an interim skill from becoming an
//! unreviewed package manager or settings editor.

pub mod components;
pub mod detect;
pub mod plan;
pub mod policy;
pub mod status;
pub mod verify;

use ds_cli_contract::spec::{Arg, ArgKind, Availability, Domain, Refusal};

pub static DOMAIN: Domain = Domain {
    id: "workstation",
    summary: "Workstation prerequisites and safe setup plans.",
    commands: &[
        &status::COMMAND,
        &plan::COMMAND,
        &verify::COMMAND,
        &components::COMMAND,
    ],
};

pub const COMPONENT_ARG: Arg = Arg {
    name: "component",
    kind: ArgKind::Value,
    value: "<libreoffice|qgis|git-bash|rwanda-reference>",
    required: true,
    default: None,
    choices: &["libreoffice", "qgis", "git-bash", "rwanda-reference"],
    summary: "The governed prerequisite or reference component.",
};

pub const OPTIONAL_COMPONENT_ARG: Arg = Arg {
    required: false,
    ..COMPONENT_ARG
};

pub const UNSUPPORTED_PLATFORM: Refusal = Refusal {
    code: "workstation_platform_unsupported",
    when: "the requested host is a browser or is outside Windows, macOS, and Linux",
    remedy: "run discovery on the local Windows, macOS, or Linux host",
};

pub const COMPONENT_UNKNOWN: Refusal = Refusal {
    code: "workstation_component_unknown",
    when: "the component is outside the governed catalogue",
    remedy: "choose an id from `ds workstation components`",
};

pub const PLAN_INVALID: Refusal = Refusal {
    code: "workstation_plan_invalid",
    when: "the configuration target does not apply",
    remedy: "request one target shown by `ds workstation plan --help`",
};

pub(crate) fn always() -> Availability {
    Availability::Available
}
