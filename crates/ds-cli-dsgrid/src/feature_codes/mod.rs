//! `ds dsgrid feature-codes report|import|migrate|export` — the feature-code
//! table of a model against the owner-issued standard (program contract 03
//! §2–§4).
//!
//! Every verb is typed inputs and a receipt over the engine's plans
//! (`ds_grid_engine::feature_code_ops`) and the exchange's FEA writer; the
//! classifier, the legacy map and the clearances per class live in the
//! standard the engine bundles. The mutating verbs (`import`, `migrate`)
//! ride the family's [`crate::mutation`] plumbing: `--model` working copy
//! revised in place or `--package` → `--out`, the revision pin, `--dry-run`
//! xor `--yes`, one receipt shape.

pub mod export;
pub mod import;
pub mod migrate;
pub mod report;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use ds_cli_contract::Inputs;
use ds_grid_engine::feature_code_standard::{BUNDLED_STANDARD_NAME, VOLTAGE_CLASSES};
use ds_grid_engine::{FeatureCodeStandard, StandardError};
use serde_json::{Value, json};

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
        code: "invalid_limit",
        when: "--limit is not a whole number in 1..5000",
        remedy: "pass a limit inside the range, or omit it for the default of 50",
    },
];

/// Load the standard named by `--standard`.
pub fn load_standard(inputs: &Inputs) -> Result<FeatureCodeStandard, Failure> {
    let raw = inputs
        .value("standard")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(OWNER_STANDARD_NAME);
    let bundled = raw == OWNER_STANDARD_NAME || raw == BUNDLED_STANDARD_NAME;
    let result = if bundled && !std::path::Path::new(raw).is_file() {
        FeatureCodeStandard::bundled()
    } else {
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
        FeatureCodeStandard::from_bytes(&bytes, raw)
    };
    result.map_err(map_standard_error)
}

pub fn map_standard_error(error: StandardError) -> Failure {
    match error {
        StandardError::Unreadable(detail) => {
            Failure::invalid("standard_invalid", "the standard is not readable JSON")
                .remedy("issue the standard with its build tool")
                .detail(json!({ "detail": detail }))
        }
        StandardError::WrongSchema(detail) => Failure::invalid(
            "standard_invalid",
            "the file does not carry the feature-code standard schema",
        )
        .remedy("pass an issued ds.pls-feature-codes/v1 file")
        .detail(json!({ "detail": detail })),
        StandardError::DigestMismatch {
            version,
            expected,
            found,
        } => Failure::invalid(
            "standard_digest_mismatch",
            format!("standard version {version} was edited outside the owner's build"),
        )
        .remedy("use the bundled standard or the owner's issued file; report a wrong value as a finding")
        .detail(json!({ "version": version, "expected": expected, "found": found })),
        StandardError::VoltageClassUnknown(class) => Failure::invalid(
            "voltage_class_unknown",
            format!("`{class}` is not a voltage class of the standard"),
        )
        .remedy(format!("pass one of {}", VOLTAGE_CLASSES.join(", "))),
    }
}

/// The class named by `--voltage-class`, validated against the standard.
pub fn voltage_class(inputs: &Inputs, standard: &FeatureCodeStandard) -> Result<String, Failure> {
    let class = inputs.require("voltage-class")?.trim().to_string();
    standard
        .voltage_class(&class)
        .map_err(map_standard_error)?;
    Ok(class)
}

/// The standard's identity for a receipt.
pub fn standard_receipt(standard: &FeatureCodeStandard) -> Value {
    json!({
        "name": OWNER_STANDARD_NAME,
        "schema": standard.schema,
        "version": standard.version,
        "issued": standard.issued,
        "digest": standard.digest,
        "pinned": standard.pinned,
        "origin": standard.origin,
        "codes": standard.codes.len(),
    })
}

/// Open the target read-only through the family's selection rules
/// (`--model <local-id>` or `--package <path>`).
pub fn open_read_only(inputs: &Inputs) -> Result<(Value, crate::mutation::Opened), Failure> {
    let target = crate::mutation::Target::resolve(inputs, false)?;
    let opened = crate::mutation::open(target, inputs)?;
    let describe = json!({
        "path": match &opened.target {
            crate::mutation::Target::WorkingCopy { path, .. } => path.display().to_string(),
            crate::mutation::Target::Package { path, .. } => path.clone(),
        },
        "model_id": opened.package.manifest.model.model_id.as_str(),
        "package_revision": opened.package.manifest.model.model_revision,
        "authored_revision": opened.head.as_str(),
    });
    Ok((describe, opened))
}
