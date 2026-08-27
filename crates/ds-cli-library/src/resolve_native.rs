use std::path::Path;

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_exchange::{
    StandardsLibrarySeedError, parse_standards_library_manifest, select_standards_native_member,
    verify_standards_native_member_bytes,
};
use ds_grid_model::EntityId;
use serde_json::{Value, json};

use crate::read;

pub static COMMAND: Command = Command {
    id: "library.resolve-native",
    path: &["library", "resolve-native"],
    contract: 1,
    summary: "Resolve one exact pinned native asset for a differential PLS handoff.",
    purpose: "Opens library/<id>/<version>/manifest.json, requires the expected content-root digest, selects by canonical typed name/invariant leaf and expected native kind, then verifies the exact PLS-CADD bytes. It never chooses latest, basename, master or a repaired substitute and never generates a PLS asset from DS Grid bytes.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "store",
            "<dir>",
            "Local root containing library/<id>/<version>.",
        )
        .required(),
        Arg::value("library-id", "<id>", "Exact pinned library id.").required(),
        Arg::value(
            "library-version",
            "<id>",
            "Exact immutable library version.",
        )
        .required(),
        Arg::value(
            "expect-digest",
            "<sha256:hex>",
            "Expected standards-library content-root digest.",
        )
        .required(),
        Arg::value(
            "native-name",
            "<leaf>",
            "Canonical typed name and invariant native leaf.",
        )
        .required(),
        Arg::value(
            "native-kind",
            "<kind>",
            "Expected native artifact kind from the seed manifest.",
        )
        .required(),
    ],
    output: "Exact library coordinates, canonical native member path, SHA-256 and byte length, plus deterministic patcher/UI handoff ownership.",
    examples: &[Example {
        command: "ds library resolve-native --store ./seed-output --library-id new-design --library-version 2026-08-27-v1 --expect-digest sha256:0123 --native-name pole.012 --native-kind structure --output json",
        note: "Resolve only after replacing the illustrative digest with the exact manifest content root.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "library_manifest_invalid",
            when: "the exact schema-v1 standards manifest cannot be decoded",
            remedy: "verify or reseed the immutable version from its ruled source",
        },
        Refusal {
            code: "library_id_mismatch",
            when: "the manifest does not carry --library-id",
            remedy: "select the exact pinned library id; do not reuse another role",
        },
        Refusal {
            code: "library_version_mismatch",
            when: "the manifest does not carry --library-version",
            remedy: "install or select that exact immutable library version",
        },
        Refusal {
            code: "library_digest_mismatch",
            when: "the manifest content root does not match --expect-digest",
            remedy: "obtain the exact digest-pinned version; do not accept a valid but different release",
        },
        Refusal {
            code: "native_name_missing",
            when: "the canonical typed name is absent",
            remedy: "use the model's exact canonical typed name and pinned library version",
        },
        Refusal {
            code: "native_name_ambiguous",
            when: "the canonical typed name maps to more than one member",
            remedy: "repair the seed as a new version; the current manifest is unusable",
        },
        Refusal {
            code: "native_kind_mismatch",
            when: "the selected name is not the required native artifact kind",
            remedy: "supply the model's expected kind and exact typed name",
        },
        Refusal {
            code: "native_mapping_invalid",
            when: "the typed name, invariant leaf and declared PLS path disagree",
            remedy: "repair the seed manifest as a new immutable version",
        },
        Refusal {
            code: "native_bytes_mismatch",
            when: "the selected native file differs from its declared digest or length",
            remedy: "restore the exact immutable member; never repair or regenerate it from DS Grid",
        },
        Refusal {
            code: "invalid_library_id",
            when: "--library-id is malformed",
            remedy: "pass the stable library id from the model pin",
        },
        Refusal {
            code: "invalid_library_version",
            when: "--library-version is malformed",
            remedy: "pass the exact immutable version from the model pin",
        },
        Refusal {
            code: "library_path_not_found",
            when: "the manifest or selected native member is absent",
            remedy: "install the exact immutable version into --store",
        },
        Refusal {
            code: "library_path_not_file",
            when: "a declared manifest/member path is not a regular file",
            remedy: "restore the verified immutable version",
        },
        Refusal {
            code: "library_file_too_large",
            when: "a manifest/member exceeds the bounded local read limit",
            remedy: "use the curated immutable release rather than an unbounded source",
        },
        Refusal {
            code: "library_read_failed",
            when: "the manifest/member cannot be read",
            remedy: "check local store permissions and integrity",
        },
    ],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};

fn resolution_failure(error: StandardsLibrarySeedError) -> Failure {
    let message = error.to_string();
    match error {
        StandardsLibrarySeedError::LibraryIdMismatch { .. } => {
            Failure::invalid("library_id_mismatch", message)
        }
        StandardsLibrarySeedError::LibraryVersionMismatch { .. } => {
            Failure::invalid("library_version_mismatch", message)
        }
        StandardsLibrarySeedError::LibraryDigestMismatch { .. } => {
            Failure::invalid("library_digest_mismatch", message)
        }
        StandardsLibrarySeedError::NativeNameMissing { .. } => {
            Failure::invalid("native_name_missing", message)
        }
        StandardsLibrarySeedError::NativeNameAmbiguous { .. } => {
            Failure::invalid("native_name_ambiguous", message)
        }
        StandardsLibrarySeedError::NativeKindMismatch { .. } => {
            Failure::invalid("native_kind_mismatch", message)
        }
        StandardsLibrarySeedError::NativeMappingInvalid { .. } => {
            Failure::invalid("native_mapping_invalid", message)
        }
        StandardsLibrarySeedError::NativeBytesMismatch { .. } => {
            Failure::invalid("native_bytes_mismatch", message)
        }
        _ => Failure::invalid("library_manifest_invalid", message),
    }
}

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let store = Path::new(inputs.require("store")?);
    let library_id = EntityId::new(inputs.require("library-id")?)
        .map_err(|error| Failure::invalid("invalid_library_id", error.to_string()))?;
    let version = EntityId::new(inputs.require("library-version")?)
        .map_err(|error| Failure::invalid("invalid_library_version", error.to_string()))?;
    let version_root = store
        .join("library")
        .join(library_id.as_str())
        .join(version.as_str());
    let manifest_path = version_root.join("manifest.json");
    let manifest_bytes = read(manifest_path.to_string_lossy().as_ref())?;
    let manifest = parse_standards_library_manifest(&manifest_bytes).map_err(resolution_failure)?;
    let resolution = select_standards_native_member(
        &manifest,
        library_id.as_str(),
        version.as_str(),
        inputs.require("expect-digest")?,
        inputs.require("native-name")?,
        inputs.require("native-kind")?,
    )
    .map_err(resolution_failure)?;
    let native_path = version_root.join(&resolution.pls_cadd_path);
    let native_bytes = read(native_path.to_string_lossy().as_ref())?;
    verify_standards_native_member_bytes(&resolution, &native_bytes).map_err(resolution_failure)?;
    let digest = format!("sha256:{}", resolution.sha256);
    Ok(json!({
        "library_id": resolution.library_id,
        "library_version": resolution.version,
        "content_root_digest": format!("sha256:{}", resolution.content_root_sha256),
        "canonical_typed_name": resolution.canonical_typed_name,
        "native_kind": resolution.native_kind,
        "native_name": resolution.pls_cadd_native_name,
        "native_path": native_path,
        "byte_length": resolution.byte_length,
        "sha256": digest,
        "execution_owner": "ds",
        "deterministic_completion": "exact pinned native member selected and its declared bytes verified without fallback",
        "native_patcher_handoff": { "required": true, "condition": "A characterized differential model-state patch references this canonical native member.", "artifact": native_path, "digest": digest },
        "pls_cadd_ui_handoff": { "required": false, "condition": "Only native solver judgment, operator-owned PI movement or explicit visual acceptance requires PLS-CADD.", "artifact": native_path, "digest": digest, "post_ui_reimport": "Re-import the exact native-saved workspace and compare library/member digests plus declared model-state changes." },
        "engineer_decision": "Engineer decides that this pinned library version and native member are authoritative and suitable for the intended design/strength scope."
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "resolved {}/{} {} -> {}\n{}",
        data["library_id"].as_str().unwrap_or("?"),
        data["library_version"].as_str().unwrap_or("?"),
        data["canonical_typed_name"].as_str().unwrap_or("?"),
        data["native_path"].as_str().unwrap_or("?"),
        data["sha256"].as_str().unwrap_or("?")
    )
}
