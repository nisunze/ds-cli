//! `ds workstation` — inspect, plan, and safely prepare prerequisites.
//!
//! Windows lifecycle evidence supports one deliberately narrow mutation path:
//! package-manager LibreOffice installation and selecting an existing VS Code
//! Git Bash profile. Everything else remains discovery/planning until equally
//! strong evidence exists.

pub mod components;
pub mod configure;
pub mod detect;
pub mod install;
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
        &install::COMMAND,
        &configure::COMMAND,
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

pub const MUTATION_UNSUPPORTED: Refusal = Refusal {
    code: "workstation_mutation_unsupported",
    when: "the component, platform, or target has no proven mutation contract",
    remedy: "use `ds workstation plan` and request only an implemented exact action",
};

pub const APPROVAL_REQUIRED: Refusal = Refusal {
    code: "workstation_approval_required",
    when: "a native installer may require interactive operating-system approval",
    remedy: "re-run with `--approval interactive --yes` only while the user is present",
};

pub const SOURCE_UNVERIFIED: Refusal = Refusal {
    code: "workstation_source_unverified",
    when: "the package mechanism, official manifest membership, or artifact hash is unverified",
    remedy: "use the trusted package-manager path or an official manifest/hash flow once supported",
};

pub const VERIFICATION_FAILED: Refusal = Refusal {
    code: "workstation_verification_failed",
    when: "registration, executable identity/version, or the harmless smoke test fails",
    remedy: "inspect the returned receipt and repair the component before retrying",
};

pub const RECEIPT_CONFLICT: Refusal = Refusal {
    code: "workstation_receipt_conflict",
    when: "an existing ownership receipt cannot safely describe this installation",
    remedy: "inspect the existing receipt; never overwrite ownership evidence blindly",
};

pub const SETTINGS_UNSAFE: Refusal = Refusal {
    code: "workstation_settings_unsafe",
    when: "the settings file or selected Git Bash profile cannot be merged conservatively",
    remedy: "define and verify the Git Bash profile in VS Code, then retry the exact target",
};

pub(crate) fn always() -> Availability {
    Availability::Available
}
