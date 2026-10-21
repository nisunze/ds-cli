//! `ds dsgrid feature-codes report` — the feature-code table of a model
//! against the owner-issued standard (program contract 03 §2), and the
//! standard arguments the clearance criteria verbs share.
//!
//! The classifier, the legacy map and the clearances per class live in the
//! standard the Network owner bundles; this module only names which standard
//! an operator chose and reads an issued file's bytes for the owner.

pub mod report;

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use serde_json::json;

/// The name the owner's file carries; `--standard` resolves it (and the
/// engine's own bundled name) without a path.
pub const OWNER_STANDARD_NAME: &str = "pls-feature-codes.v1.json";

pub const STANDARD_ARG: Arg = Arg::value(
    "standard",
    "<name|path>",
    "The feature-code standard: the bundled `pls-feature-codes.v1.json` (default) or a path to an issued file; its digest is checked either way.",
)
.default(OWNER_STANDARD_NAME);

pub const VOLTAGE_CLASS_ARG: Arg = Arg::value(
    "voltage-class",
    "<LV|MV|HV_110|HV_220>",
    "The voltage class whose required clearances the codes carry (REG Table 15 for MV).",
)
.choices(&["LV", "MV", "HV_110", "HV_220"]);

pub const STANDARD_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "standard_not_found",
        when: "--standard is neither the bundled name nor a readable file",
        remedy: "pass `pls-feature-codes.v1.json` for the bundled standard, or the path of an issued file",
    },
    Refusal {
        code: "standard_invalid",
        when: "the file is not a ds.pls-feature-codes/v1 document",
        remedy: "issue the standard with its build tool; do not hand-write it",
    },
    Refusal {
        code: "standard_digest_mismatch",
        when: "the file names an issued version but its bytes differ from the issued ones (edited outside the owner's build)",
        remedy: "use the bundled standard or the owner's issued file; a changed value is reported as a finding, never applied",
    },
    Refusal {
        code: "voltage_class_unknown",
        when: "--voltage-class is not LV, MV, HV_110 or HV_220",
        remedy: "pass one of the four classes the standard defines",
    },
];

/// The refusals a read-only verb over the family's target selection shares
/// (the subset of [`crate::mutation::REFUSALS`] a read can raise, plus the
/// package read bound and the list cap).
pub const READ_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "target_required",
        when: "neither --model nor --package names a target, or both do",
        remedy: "pass exactly one of --model <local-id> or --package <path>",
    },
    Refusal {
        code: "local_model_not_found",
        when: "no working copy on this machine carries --model",
        remedy: "run `ds dsgrid model list` and use an id from it",
    },
    Refusal {
        code: "local_model_ambiguous",
        when: "--model was found in more than one account's catalogue on this machine",
        remedy: "pass --account <uid> to say whose working copy you mean",
    },
    Refusal {
        code: "local_model_store_unavailable",
        when: "the machine's catalogue or the package beside it cannot be read",
        remedy: "check the local data directory; DS_LAYER_HOME may name an absolute shared directory",
    },
    Refusal {
        code: "model_not_found",
        when: "--package does not name a readable file",
        remedy: "check the path; --package takes one .dsgrid file",
    },
    Refusal {
        code: "model_too_large",
        when: "the package is above the 512 MiB read bound",
        remedy: "confirm the file is a .dsgrid package and not a disk image",
    },
    Refusal {
        code: "model_unreadable",
        when: "the package exists but cannot be read",
        remedy: "check file permissions",
    },
    Refusal {
        code: "not_a_dsgrid_package",
        when: "the target bytes are not a .dsgrid container this build opens",
        remedy: "convert the native source with `ds dsgrid-exchange convert` first",
    },
    Refusal {
        code: "package_decode_failed",
        when: "the package does not verify against its manifest",
        remedy: "run `ds dsgrid validate --model <path>`; re-convert it from its PLS-CADD workspace if it predates the canonical schema",
    },
    Refusal {
        code: "revision_conflict",
        when: "--revision is not the target's authored head",
        remedy: "re-read the head with `ds dsgrid model show` and decide again against it",
    },
    Refusal {
        code: "revision_invalid",
        when: "--revision is not a `rev:` token of 64 hex digits",
        remedy: "pass the token `ds dsgrid inspect --include authored-revision` prints",
    },
    Refusal {
        code: "invalid_limit",
        when: "--limit is not a whole number in 1..5000",
        remedy: "pass a limit inside the range, or omit it for the default of 50",
    },
];

/// The engine's own name for its bundled standard. A hand copy held to the
/// owner by `bundled_standard_names_match_the_owner`.
pub const BUNDLED_STANDARD_NAME: &str = "feature-codes.v1.json";

/// Which standard `--standard` names: `None` for the bundled one, or the
/// issued file's bytes and the label the operator typed. The standard's
/// schema, digest and classes are checked by the Network owner.
pub fn standard_input(inputs: &Inputs) -> Result<Option<(String, Vec<u8>)>, Failure> {
    let raw = inputs
        .value("standard")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(OWNER_STANDARD_NAME);
    let bundled = raw == OWNER_STANDARD_NAME || raw == BUNDLED_STANDARD_NAME;
    if bundled && !std::path::Path::new(raw).is_file() {
        return Ok(None);
    }
    let path = std::path::Path::new(raw);
    if !path.is_file() {
        return Err(Failure::invalid(
            "standard_not_found",
            format!("`{raw}` is neither the bundled standard nor a file"),
        )
        .remedy("pass `pls-feature-codes.v1.json` or the path of an issued file"));
    }
    let bytes = std::fs::read(path).map_err(|error| {
        Failure::failed("standard_not_found", format!("cannot read `{raw}`"))
            .remedy("check file permissions")
            .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    Ok(Some((raw.to_owned(), bytes)))
}

#[cfg(test)]
#[path = "feature_codes_tests.rs"]
mod tests;
