//! Reading a `.dsgrid` package, once.
//!
//! Every command in this domain starts by turning a path into bytes and then
//! into either a manifest or a decoded snapshot. Doing that in one place is
//! not only less code — it is the only way the domain's refusals stay
//! identical. A caller who learns `model_not_found` from `ds dsgrid inspect`
//! must get the same code, with the same remedy, from `ds dsgrid validate`.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_grid_exchange::dsgrid;
use ds_grid_exchange::package::{GridPackage, PackageManifest, unpack};
use serde_json::json;

/// Refuse a file larger than this before reading it. A `.dsgrid` is a zip of
/// compressed Arrow tables; a real one is megabytes. The bound exists so a
/// mistyped path at a disk image fails in a millisecond with a typed reason
/// instead of exhausting memory.
pub const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;

/// Native protected-mode policy, projected by every mutating command.
pub const PROTECTED_MODEL: ds_cli_contract::spec::Refusal = ds_cli_contract::spec::Refusal {
    code: ds_grid_engine::model_protection::PROTECTED_CODE,
    when: "the target is the automatic combined model in protected mode",
    remedy: "edit the owning submodel named in the refusal, then reconcile; combined models remain available for reads, status, tiling and reports",
};

/// The same characterized section-identity refusal exposed by the JSON host.
pub const PLS_SECTION_IDENTITY_UNRESOLVED: ds_cli_contract::spec::Refusal =
    ds_cli_contract::spec::Refusal {
        code: "pls_section_identity_unresolved",
        when: "the native section split cannot carry its PLS identities or paired display-cache evidence without guessing",
        remedy: "inspect the native detail and repair the source binding group before retrying",
    };

/// Stage package metadata from the engine's ordered split deltas. Exchange
/// owns all lineage/cache rules. Failure leaves the input bindings untouched;
/// genuine deletion still retires bindings at the ordinary package emitter.
pub(crate) fn admit_command_bindings(
    current: &BTreeMap<String, Vec<ds_grid_model::ExchangeBindingRow>>,
    deltas: &[ds_grid_engine::CommandDelta],
    idempotent_replay: bool,
) -> Result<BTreeMap<String, Vec<ds_grid_model::ExchangeBindingRow>>, Failure> {
    let mut staged = current.clone();
    if !idempotent_replay && let Some(bindings) = staged.get_mut("pls_cadd") {
        ds_grid_exchange::carry_forward_pls_section_bindings(bindings, deltas).map_err(
            |error| {
                Failure::invalid(PLS_SECTION_IDENTITY_UNRESOLVED.code, error.to_string())
                    .remedy(PLS_SECTION_IDENTITY_UNRESOLVED.remedy)
                    .detail(json!(error))
            },
        )?;
    }
    Ok(staged)
}

pub fn authorize_write(package: &GridPackage, operation: &str) -> Result<(), Failure> {
    ds_grid_exchange::linked_models::authorize_package_write(package, operation, &[]).map_err(
        |error| {
            Failure::invalid("composite_model_protected", error.message)
                .remedy(PROTECTED_MODEL.remedy)
        },
    )
}

/// Opaque byte guard for the local catalogue. Reuse the operation's captured
/// executable; decoding and protected-package policy remain in Network.
pub(crate) fn authorize_write_bytes(
    client: &ds_network_client::CapturedNetworkClient,
    raw_path: &str,
    bytes: &[u8],
    operation: &str,
) -> Result<(), Failure> {
    use ds_network_contracts::{grid::NativeGridQuerySourcePin, grid_write_admission::*};
    use sha2::{Digest, Sha256};
    let request = NativeGridWriteAdmissionRequestV1 {
        schema: GRID_WRITE_ADMISSION_SCHEMA_V1.into(),
        source: NativeGridQuerySourcePin {
            package_sha256: format!("{:x}", Sha256::digest(bytes)),
            expected_revision: None,
            engineering_input_root: None,
        },
        operation: operation.into(),
    };
    let scratch = crate::native::scratch()?;
    client
        .grid_write_admission(&request, bytes, scratch.path(), &|| false)
        .map_err(|error| {
            if error.owner_code() == Some("package_refused") {
                Failure::failed("package_decode_failed", format!("`{raw_path}` did not decode"))
                    .remedy("the canonical reader accepts current schema only; retain the original and use `ds dsgrid package migrate --path <source> --expected-source-sha256 <capture> --dry-run`, then open the saved current package")
                    .detail(json!({ "detail": error.to_string() }))
            } else {
                crate::native::map_failure(error)
            }
        })
        .and_then(|outcome| match outcome.result.verdict {
            NativeGridWriteAdmissionVerdictV1::Admitted => Ok(()),
            NativeGridWriteAdmissionVerdictV1::Denied { message, .. } =>
                Err(Failure::invalid("composite_model_protected", message).remedy(PROTECTED_MODEL.remedy)),
        })
}

/// The refusals every command in this domain shares. Declared here as data so
/// a command can splice them into its own list rather than restating them —
/// a restatement is a second description, and those drift.
pub const SHARED_REFUSALS: &[ds_cli_contract::spec::Refusal] = &[
    ds_cli_contract::spec::Refusal {
        code: "model_not_found",
        when: "the path does not exist or is not a file",
        remedy: "check the path; --model takes a file, not a directory",
    },
    ds_cli_contract::spec::Refusal {
        code: "model_too_large",
        when: "the file is above the 512 MiB read bound",
        remedy: "confirm the file is a .dsgrid package and not a disk image",
    },
    ds_cli_contract::spec::Refusal {
        code: "model_unreadable",
        when: "the file exists but cannot be read",
        remedy: "check file permissions",
    },
    ds_cli_contract::spec::Refusal {
        code: "not_a_dsgrid_package",
        when: "the bytes are not a readable .dsgrid container",
        remedy: "a .dsgrid is a zip containing manifest.json; convert other formats first",
    },
];

pub fn read_bytes(raw_path: &str) -> Result<Vec<u8>, Failure> {
    let path = Path::new(raw_path);
    let metadata = std::fs::metadata(path).map_err(|error| {
        Failure::invalid("model_not_found", format!("cannot read `{raw_path}`"))
            .remedy("check the path; --model takes a .dsgrid file")
            .detail(json!({ "detail": error.kind().to_string() }))
    })?;

    if !metadata.is_file() {
        return Err(
            Failure::invalid("model_not_found", format!("`{raw_path}` is not a file"))
                .remedy("--model takes a .dsgrid file, not a directory"),
        );
    }

    if metadata.len() > MAX_PACKAGE_BYTES {
        return Err(Failure::invalid(
            "model_too_large",
            format!("`{raw_path}` is above the read bound"),
        )
        .remedy("confirm this is a .dsgrid package")
        .detail(json!({ "byte_len": metadata.len(), "max_byte_len": MAX_PACKAGE_BYTES })));
    }

    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_PACKAGE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| {
            Failure::failed("model_unreadable", format!("cannot read `{raw_path}`"))
                .remedy("check file permissions")
                .detail(json!({ "detail": error.kind().to_string() }))
        })?;
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(
            Failure::invalid("model_too_large", "the source grew above the read bound")
                .remedy("use a bounded .dsgrid package")
                .detail(json!({ "max_byte_len": MAX_PACKAGE_BYTES })),
        );
    }
    Ok(bytes)
}

/// The cheap read: the package manifest, with no Arrow table decoded.
pub fn read_manifest(raw_path: &str, bytes: &[u8]) -> Result<PackageManifest, Failure> {
    let inspection = dsgrid::inspect(bytes).map_err(|error| {
        Failure::invalid(
            "not_a_dsgrid_package",
            format!("`{raw_path}` is not a readable .dsgrid package"),
        )
        .remedy("a .dsgrid is a zip containing manifest.json")
        .detail(json!({ "detail": error.to_string() }))
    })?;

    if !inspection.readable {
        return Err(Failure::invalid(
            "not_a_dsgrid_package",
            format!("`{raw_path}` has no package manifest"),
        )
        .remedy("re-export the model as .dsgrid"));
    }

    serde_json::from_value(
        inspection
            .detail
            .get("manifest")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    )
    .map_err(|error| {
        Failure::failed(
            "manifest_unreadable",
            "the package manifest does not match this build's schema",
        )
        .remedy("rebuild the package with a matching ds-network release")
        .detail(json!({ "detail": error.to_string() }))
    })
}

/// The expensive read: full verification and decode.
///
/// `unpack` verifies every member against the manifest's attestations before
/// returning, so a package that decodes here has already proved its bytes
/// match what it claims. That is why `ds dsgrid validate` can report the
/// container and the model as two separate answers.
pub fn decode(raw_path: &str, bytes: &[u8]) -> Result<GridPackage, Failure> {
    unpack(bytes).map_err(|error| {
        Failure::failed(
            "package_decode_failed",
            format!("`{raw_path}` did not decode"),
        )
        .remedy("the canonical reader accepts current schema only; retain the original and use `ds dsgrid package migrate --path <source> --expected-source-sha256 <capture> --dry-run`, then open the saved current package")
        .detail(json!({ "detail": error.to_string() }))
    })
}

/// The canonical serialized token for a table kind — taken from the model
/// crate's own serde representation rather than re-spelled here, so a table
/// renamed upstream is renamed in this output too.
pub fn table_token(kind: ds_grid_model::TableKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{kind:?}"))
}

/// Take at most `limit`, reporting how many were withheld. Truncation is
/// always visible: a silently shortened list reads as a complete one.
pub fn take<T>(mut items: Vec<T>, limit: usize) -> (Vec<T>, usize) {
    if items.len() <= limit {
        return (items, 0);
    }
    let withheld = items.len() - limit;
    items.truncate(limit);
    (items, withheld)
}

pub const MAX_LIMIT: usize = 5_000;
pub const DEFAULT_LIMIT: &str = "50";

pub fn parse_limit(raw: Option<&str>) -> Result<usize, Failure> {
    let raw = raw.unwrap_or(DEFAULT_LIMIT);
    let parsed: usize = raw.parse().map_err(|_| {
        Failure::invalid("invalid_limit", "--limit must be a whole number")
            .remedy(format!("pass 1..{MAX_LIMIT}"))
    })?;
    if parsed == 0 || parsed > MAX_LIMIT {
        return Err(
            Failure::invalid("invalid_limit", "--limit is outside its accepted range")
                .remedy(format!("pass 1..{MAX_LIMIT}"))
                .detail(json!({ "given": parsed, "min": 1, "max": MAX_LIMIT })),
        );
    }
    Ok(parsed)
}

#[cfg(test)]
mod binding_tests {
    use super::*;
    use ds_grid_model::{EntityId, ExchangeBindingRow};

    fn row(entity: &str, namespace: &str, key: &str) -> ExchangeBindingRow {
        ExchangeBindingRow {
            entity_id: EntityId::new(entity).unwrap(),
            system: "pls_cadd".into(),
            namespace: namespace.into(),
            key: key.into(),
            raw: None,
        }
    }
    fn split(old: &str, origin: &str) -> ds_grid_engine::CommandDelta {
        serde_json::from_value(json!({"command_id":format!("split-{old}"),"command_kind":"create_structure",
            "affected_entities":[],"declared_write_tables":[],"changed_tables":[],"changed_dependency_tokens":[],"projection_refreshes":[],
            "tension_section_splits":[{"retired_section_id":old,"retired_support_structure_ids":["str-a","str-b"],
                "derived_sections":[{"section_id":origin,"support_structure_ids":["str-a","str-x"],"retains_ordered_path_origin":true},
                {"section_id":format!("{old}-suffix"),"support_structure_ids":["str-x","str-b"],"retains_ordered_path_origin":false}]}]})).unwrap()
    }
    #[test]
    fn command_binding_refusals_keep_exact_native_detail_and_source_atomicity() {
        let cases = [
            (
                vec![row("old", "section_group_slot", "wrong")],
                vec![split("old", "origin")],
            ),
            (
                vec![
                    row("old", "section_number", "7"),
                    row("old", "section_group_slot", "7:0"),
                    row("other-phase", "section_group_slot", "7:1"),
                ],
                vec![split("old", "origin")],
            ),
            // The first delta succeeds; the second collides. Admission still
            // publishes none of the first delta's package metadata.
            (
                vec![
                    row("old", "section_number", "1"),
                    row("later", "section_number", "2"),
                    row("occupied", "section_number", "9"),
                ],
                vec![split("old", "origin"), split("later", "occupied")],
            ),
        ];
        for (rows, deltas) in cases {
            let input = BTreeMap::from([
                ("pls_cadd".into(), rows.clone()),
                ("other_adapter".into(), Vec::new()),
            ]);
            let before = serde_json::to_vec(&input).unwrap();
            let mut native_rows = rows;
            let native =
                ds_grid_exchange::carry_forward_pls_section_bindings(&mut native_rows, &deltas)
                    .unwrap_err();
            let refused = admit_command_bindings(&input, &deltas, false).unwrap_err();
            assert_eq!(refused.code(), PLS_SECTION_IDENTITY_UNRESOLVED.code);
            assert_eq!(refused.detail_value(), Some(&json!(native)));
            assert_eq!(serde_json::to_vec(&input).unwrap(), before);
            assert_eq!(
                admit_command_bindings(&input, &deltas, true).unwrap(),
                input,
                "idempotent replay never repeats split effects"
            );
        }
    }
}
