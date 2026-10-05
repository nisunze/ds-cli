//! Explicit format migration and opt-in native lineage successor; no project write.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_exchange::package_migration::{
    MigrationOutput, SupplementalResource, migrate_with_resources,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
};
pub static COMMAND: Command = Command {
    id: "dsgrid.package.migrate",
    path: &["dsgrid", "package", "migrate"],
    contract: 3,
    summary: "Verify one old package and save its lossless current-format migration.",
    purpose: "Verify historical schema and digests, then migrate named fields without changing engineering facts or revision. Pinned supplements restore declared missing bytes; derived pins require verified recomputation. Dry-run returns a receipt; yes writes fresh local files. Opt-in successor verifies original/current/receipt pins and advances only native revision by one. No publication; original unchanged. See the reference for preservation and restoration.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::switch(
            "successor",
            "Create an explicit format-migration successor, preserving facts and advancing native revision by one. Requires all four successor input pins.",
        ),
        Arg::value(
            "current-path",
            "<file>",
            "Strict-current output of the independently reproducible original migration.",
        ),
        Arg::value(
            "expected-current-sha256",
            "<hex>",
            "Exact current package SHA-256; required with successor.",
        ),
        Arg::value(
            "migration-receipt",
            "<file>",
            "Exact original migration receipt; required with successor.",
        ),
        Arg::value(
            "expected-migration-receipt-sha256",
            "<hex>",
            "Exact receipt SHA-256; required with successor.",
        ),
        Arg::value(
            "supplemental-resources",
            "<json-file>",
            "Optional explicit missing-resource input manifest; no automatic lookup. Requires its exact SHA pin. Source receipt describes declared provenance, never native qualification.",
        ),
        Arg::value(
            "expected-supplemental-sha256",
            "<hex>",
            "Required with supplemental-resources; exact manifest SHA-256.",
        ),
        Arg::value(
            "path",
            "<file>",
            "Exact source .dsgrid file; never modified.",
        )
        .required(),
        Arg::value(
            "expected-source-sha256",
            "<hex>",
            "Exact original source SHA-256 from the capture receipt.",
        )
        .required(),
        Arg::value(
            "out",
            "<new-directory>",
            "New local package/receipt directory; must not exist.",
        ),
        Arg::switch(
            "dry-run",
            "Verify and return the proposed receipt without writing.",
        ),
        Arg::switch(
            "yes",
            "Materialize the validated package and receipt locally.",
        ),
    ],
    output: "Source/output SHA, schemas, strict verification, transformations and preservation receipt; source_modified=false, published=false. Output is model.dsgrid plus the last-written migration.receipt.json; incomplete directories are refused.",
    examples: &[],
    refusals: &[
        Refusal {
            code: "package_migration_supplemental_invalid",
            when: "supplemental manifest/source/receipt/byte pins, bounds or original resource facts differ",
            remedy: "supply explicit exact DS-extracted bytes and retained receipt; never substitute resource rows or a library head",
        },
        Refusal {
            code: "package_migration_invalid",
            when: "unknown source version/schema, malformed table, missing/tampered member, discarded facts, or strict output validation fails",
            remedy: "retain original bytes and report the precise unsupported source shape; do not change fingerprints or engineering facts",
        },
        Refusal {
            code: "package_migration_source_conflict",
            when: "the source digest differs from the captured digest",
            remedy: "review the new source and capture its exact receipt",
        },
        Refusal {
            code: "package_migration_output",
            when: "output exists, is relative, or cannot be atomically materialized",
            remedy: "choose a new absolute writable output directory",
        },
        Refusal {
            code: "package_migration_confirmation",
            when: "neither dry-run nor yes, or both, were given",
            remedy: "use --dry-run first, then --yes for the reviewed local output",
        },
    ],
    reference: Some("docs/reference/dsgrid.md"),
    search: &["schema", "autoheal"],
    requires: Requires::Server,
    availability: || Availability::Available,
};
fn output_error(error: impl std::fmt::Display) -> Failure {
    Failure::failed("package_migration_output", error.to_string()).remedy(
        "choose a new absolute writable directory; retain any incomplete output for diagnostics",
    )
}
/// Reserve a fresh directory atomically. Package and receipt are staged/fsynced;
/// the receipt is the last commit marker. A failed directory is never reused.
#[cfg(test)]
pub(crate) fn materialize(out: &Path, output: &MigrationOutput) -> Result<Value, Failure> {
    materialize_preserving_source(out, output, None)
}

pub(crate) fn materialize_preserving_source(
    out: &Path,
    output: &MigrationOutput,
    original: Option<&[u8]>,
) -> Result<Value, Failure> {
    if !out.is_absolute() {
        return Err(output_error("output must be absolute"));
    }
    std::fs::create_dir(out).map_err(output_error)?;
    let package_path = out.join("model.dsgrid");
    let receipt_path = out.join("migration.receipt.json");
    let mut files = Vec::new();
    if let Some(bytes) = original {
        if format!("{:x}", Sha256::digest(bytes)) != output.receipt.source_sha256 {
            return Err(output_error("original archive source pin differs"));
        }
        files.push((out.join("original.dsgrid"), bytes.to_vec()));
    }
    files.push((package_path.clone(), output.bytes.clone()));
    for (path, bytes) in files {
        let staged = path.with_extension("staging");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .map_err(output_error)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(output_error)?;
        // hard_link is atomic and refuses an existing final file.
        std::fs::hard_link(&staged, &path).map_err(output_error)?;
        std::fs::remove_file(staged).map_err(output_error)?;
        std::fs::File::open(out)
            .and_then(|file| file.sync_all())
            .map_err(output_error)?;
    }
    // Re-read both committed files. A receipt is never trusted without its bytes.
    let saved = std::fs::read(&package_path).map_err(output_error)?;
    if saved != output.bytes {
        return Err(output_error("readback differs"));
    }
    ds_grid_exchange::package::unpack(&saved).map_err(output_error)?;
    if let Some(original) = original
        && std::fs::read(out.join("original.dsgrid")).map_err(output_error)? != original
    {
        return Err(output_error("original archive readback differs"));
    }
    // Commit marker comes only after persisted package/original readback.
    let encoded = serde_json::to_vec_pretty(&output.receipt).map_err(output_error)?;
    let staged = receipt_path.with_extension("staging");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)
        .map_err(output_error)?;
    file.write_all(&encoded)
        .and_then(|_| file.sync_all())
        .map_err(output_error)?;
    std::fs::hard_link(&staged, &receipt_path).map_err(output_error)?;
    std::fs::remove_file(staged).map_err(output_error)?;
    std::fs::File::open(out)
        .and_then(|file| file.sync_all())
        .map_err(output_error)?;
    Ok(json!({"package_path":package_path,"receipt_path":receipt_path}))
}
pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let dry = inputs.switch("dry-run");
    if dry == context.confirmed {
        return Err(Failure::invalid(
            "package_migration_confirmation",
            "choose dry-run or yes",
        ));
    }
    let successor = successor_inputs(inputs)?;
    let source_path = inputs.require("path")?;
    let bytes = crate::package::read_bytes(source_path)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if inputs.require("expected-source-sha256")? != digest {
        return Err(Failure::invalid(
            "package_migration_source_conflict",
            "captured source digest differs",
        )
        .detail(json!({"actual_source_sha256":digest})));
    }
    let supplemental = load_supplemental(inputs, &digest)?;
    let output = if let Some((current_path, current_sha, receipt_path, receipt_sha)) = successor {
        let current = crate::package::read_bytes(current_path)?;
        let receipt = bounded_supplemental_file(Path::new(receipt_path), 16 * 1024 * 1024)?;
        ds_grid_exchange::package_migration::migrate_successor(
            &bytes,
            &supplemental,
            &digest,
            &current,
            current_sha,
            &receipt,
            receipt_sha,
        )
    } else {
        migrate_with_resources(&bytes, &supplemental)
    }
    .map_err(|error| Failure::invalid("package_migration_invalid", error.to_string()))?;
    let files = if dry {
        Value::Null
    } else {
        materialize_preserving_source(
            Path::new(inputs.require("out")?),
            &output,
            if successor.is_some() {
                Some(&bytes)
            } else {
                None
            },
        )?
    };
    Ok(
        json!({"dry_run":dry,"source_path":source_path,"source_modified":false,"published":false,"receipt":output.receipt,"files":files}),
    )
}
fn successor_inputs(inputs: &Inputs) -> Result<Option<(&str, &str, &str, &str)>, Failure> {
    let names = [
        "current-path",
        "expected-current-sha256",
        "migration-receipt",
        "expected-migration-receipt-sha256",
    ];
    let supplied = names
        .iter()
        .filter(|name| inputs.value(name).is_some())
        .count();
    if inputs.switch("successor") {
        if supplied != names.len() {
            return Err(Failure::invalid(
                "package_migration_invalid",
                "successor requires current package and migration receipt with both exact SHA pins",
            ));
        }
        Ok(Some((
            inputs.require(names[0])?,
            inputs.require(names[1])?,
            inputs.require(names[2])?,
            inputs.require(names[3])?,
        )))
    } else if supplied != 0 {
        Err(Failure::invalid(
            "package_migration_invalid",
            "successor input pins require --successor",
        ))
    } else {
        Ok(None)
    }
}

pub fn render(value: &Value) -> String {
    format!("{value}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn output() -> MigrationOutput {
        let blank =
            ds_grid_exchange::create_blank_model(&ds_grid_exchange::BlankModelRequest::default())
                .unwrap();
        ds_grid_exchange::package_migration::migrate(&blank.bytes).unwrap()
    }
    #[test]
    fn materialized_current_package_commits_receipt_and_refuses_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("fresh");
        let output = output();
        let receipt = materialize(&out, &output).unwrap();
        let package = std::fs::read(out.join("model.dsgrid")).unwrap();
        assert_eq!(package, output.bytes);
        assert!(receipt["receipt_path"].is_string());
        let saved_receipt = std::fs::read(out.join("migration.receipt.json")).unwrap();
        assert!(materialize(&out, &output).is_err());
        assert_eq!(std::fs::read(out.join("model.dsgrid")).unwrap(), package);
        assert_eq!(
            std::fs::read(out.join("migration.receipt.json")).unwrap(),
            saved_receipt
        );
    }
    #[test]
    fn relative_and_existing_outputs_refuse_without_replacing_files() {
        let output = output();
        assert!(materialize(Path::new("relative-migration-output"), &output).is_err());
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("existing");
        std::fs::create_dir(&out).unwrap();
        std::fs::write(out.join("keep"), b"original bytes").unwrap();
        assert!(materialize(&out, &output).is_err());
        assert_eq!(std::fs::read(out.join("keep")).unwrap(), b"original bytes");
        assert!(!out.join("migration.receipt.json").exists());
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SupplementalManifest {
    schema: String,
    source_package_sha256: String,
    resources: Vec<SupplementalRecord>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SupplementalRecord {
    resource_id: ds_grid_model::ResourceId,
    invariant_leaf: String,
    path: String,
    expected_sha256: String,
    byte_len: u64,
    source_package_sha256: String,
    extraction_receipt_path: String,
    extraction_receipt_sha256: String,
}
fn supplemental_error(message: impl std::fmt::Display) -> Failure {
    Failure::invalid(
        "package_migration_supplemental_invalid",
        message.to_string(),
    )
}
fn load_supplemental(
    inputs: &Inputs,
    source_sha: &str,
) -> Result<Vec<SupplementalResource>, Failure> {
    let path = inputs.value("supplemental-resources");
    let pin = inputs.value("expected-supplemental-sha256");
    let (Some(path), Some(pin)) = (path, pin) else {
        if path.is_some() || pin.is_some() {
            return Err(supplemental_error(
                "supply supplemental manifest and exact digest together",
            ));
        }
        return Ok(Vec::new());
    };
    let raw = bounded_supplemental_file(Path::new(path), 1024 * 1024)?;
    if format!("{:x}", Sha256::digest(&raw)) != pin {
        return Err(supplemental_error("supplemental manifest digest differs"));
    }
    let manifest: SupplementalManifest =
        serde_json::from_slice(&raw).map_err(supplemental_error)?;
    if manifest.schema != "ds.grid.package-migration-supplemental-resources/v1"
        || manifest.source_package_sha256 != source_sha
        || manifest.resources.is_empty()
        || manifest.resources.len() > 64
    {
        return Err(supplemental_error(
            "supplemental manifest version/source/count differs",
        ));
    }
    let mut result = Vec::new();
    for record in manifest.resources {
        let bytes = bounded_supplemental_file(Path::new(&record.path), 64 * 1024 * 1024)?;
        if bytes.len() as u64 != record.byte_len {
            return Err(supplemental_error("supplemental byte count differs"));
        }
        let receipt =
            bounded_supplemental_file(Path::new(&record.extraction_receipt_path), 1024 * 1024)?;
        if format!("{:x}", Sha256::digest(&receipt)) != record.extraction_receipt_sha256 {
            return Err(supplemental_error("extraction receipt digest differs"));
        }
        let witness: Value = serde_json::from_slice(&receipt).map_err(supplemental_error)?;
        if witness["command"] != "dsgrid.asset.extract"
            || witness["status"] != "ok"
            || witness["data"]["package_sha256"] != record.source_package_sha256
            || witness["data"]["sha256"] != record.expected_sha256
            || witness["data"]["byte_len"] != record.byte_len
            || witness["data"]["leaf"] != record.invariant_leaf
            || witness["data"]["verified"] != true
        {
            return Err(supplemental_error(
                "declared extraction receipt disagrees with supplied byte/source identities",
            ));
        }
        // A JSON witness is retained provenance, never execution authority.
        // The external migration independently rehashes against the original
        // resource row and leaves all qualification/capacity facts unchanged.
        result.push(SupplementalResource {
            resource_id: record.resource_id,
            invariant_leaf: record.invariant_leaf,
            expected_sha256: record.expected_sha256,
            bytes,
            source_package_sha256: record.source_package_sha256,
            extraction_receipt_sha256: record.extraction_receipt_sha256,
        });
    }
    Ok(result)
}
fn bounded_supplemental_file(path: &Path, limit: u64) -> Result<Vec<u8>, Failure> {
    if !path.is_absolute() {
        return Err(supplemental_error("supplemental paths must be absolute"));
    }
    let metadata = std::fs::metadata(path).map_err(supplemental_error)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(supplemental_error(
            "supplemental file exceeds bound or is not regular",
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(supplemental_error)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(supplemental_error)?;
    if bytes.len() as u64 > limit {
        return Err(supplemental_error("supplemental file grew beyond bound"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod supplemental_input_tests {
    use super::*;
    fn inputs(extra: Vec<String>) -> Inputs {
        let mut tokens = vec![
            "--path".into(),
            "unused.dsgrid".into(),
            "--expected-source-sha256".into(),
            "a".repeat(64),
            "--dry-run".into(),
        ];
        tokens.extend(extra);
        ds_cli_contract::parse(&COMMAND, &tokens).unwrap()
    }
    #[test]
    fn successor_flags_are_complete_and_opt_in_before_file_reads() {
        assert!(successor_inputs(&inputs(vec![])).unwrap().is_none());
        assert!(successor_inputs(&inputs(vec!["--successor".into()])).is_err());
        let flags = [
            "--current-path",
            "current.dsgrid",
            "--expected-current-sha256",
            "current-sha",
            "--migration-receipt",
            "receipt.json",
            "--expected-migration-receipt-sha256",
            "receipt-sha",
        ];
        assert!(successor_inputs(&inputs(flags.iter().map(|v| (*v).into()).collect())).is_err());
        let mut complete = vec!["--successor".into()];
        complete.extend(flags.iter().map(|v| (*v).into()));
        assert_eq!(
            successor_inputs(&inputs(complete)).unwrap(),
            Some((
                "current.dsgrid",
                "current-sha",
                "receipt.json",
                "receipt-sha"
            ))
        );
    }

    fn controls_dir() -> std::path::PathBuf {
        let scratch = std::env::var_os("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../out")
            });
        std::fs::create_dir_all(&scratch).expect("create disk scratch parent");
        let scratch = std::fs::canonicalize(scratch).expect("existing disk scratch parent");
        assert!(
            scratch.is_absolute()
                && !scratch.starts_with("/tmp")
                && !scratch.starts_with("/dev/shm"),
            "migration controls require disk TMPDIR"
        );
        let path = scratch.join("package-migrate-input-controls").join(format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::canonicalize(path).unwrap()
    }
    #[test]
    fn source_archive_is_exact_and_receipt_commits_only_after_valid_readback() {
        let root = controls_dir();
        let blank =
            ds_grid_exchange::create_blank_model(&ds_grid_exchange::BlankModelRequest::default())
                .unwrap();
        let output = ds_grid_exchange::package_migration::migrate(&blank.bytes).unwrap();
        let good = root.join("source-archived");
        materialize_preserving_source(&good, &output, Some(&blank.bytes)).unwrap();
        assert_eq!(
            std::fs::read(good.join("original.dsgrid")).unwrap(),
            blank.bytes
        );
        assert_eq!(
            std::fs::read(good.join("model.dsgrid")).unwrap(),
            output.bytes
        );
        assert!(good.join("migration.receipt.json").is_file());
        assert!(materialize_preserving_source(&good, &output, Some(&blank.bytes)).is_err());
        let bad = root.join("wrong-original");
        assert!(materialize_preserving_source(&bad, &output, Some(b"different original")).is_err());
        assert!(!bad.join("migration.receipt.json").exists());
        assert!(!bad.join("model.dsgrid").exists());
        assert!(!bad.join("original.dsgrid").exists());
    }

    fn fixture() -> (std::path::PathBuf, std::path::PathBuf, Value) {
        let out = controls_dir();
        let native = out.join("source.012");
        let bytes = b"exact immutable input bytes";
        std::fs::write(&native, bytes).unwrap();
        let digest = format!("{:x}", Sha256::digest(bytes));
        // This is an input-parser witness, not native execution evidence. The
        // migration separately compares bytes to immutable original row facts.
        let receipt = out.join("extraction.receipt.json");
        let witness = json!({"command":"dsgrid.asset.extract","status":"ok","data":{"package_sha256":"b".repeat(64),"sha256":digest,"byte_len":bytes.len(),"leaf":"source.012","verified":true}});
        let raw = serde_json::to_vec(&witness).unwrap();
        std::fs::write(&receipt, &raw).unwrap();
        let manifest = json!({"schema":"ds.grid.package-migration-supplemental-resources/v1","source_package_sha256":"a".repeat(64),"resources":[{"resource_id":"original-resource","invariant_leaf":"source.012","path":native,"expected_sha256":digest,"byte_len":bytes.len(),"source_package_sha256":"b".repeat(64),"extraction_receipt_path":receipt,"extraction_receipt_sha256":format!("{:x}",Sha256::digest(&raw))}]});
        (out.join("supplemental.json"), receipt, manifest)
    }
    fn manifest_inputs(path: &Path, manifest: &Value) -> Inputs {
        let bytes = serde_json::to_vec(manifest).unwrap();
        std::fs::write(path, &bytes).unwrap();
        inputs(vec![
            "--supplemental-resources".into(),
            path.to_str().unwrap().into(),
            "--expected-supplemental-sha256".into(),
            format!("{:x}", Sha256::digest(&bytes)),
        ])
    }
    #[test]
    fn paired_flags_and_actual_bounded_read_refuse() {
        assert!(
            load_supplemental(&inputs(vec![]), &"a".repeat(64))
                .unwrap()
                .is_empty()
        );
        for extra in [
            vec!["--supplemental-resources".into(), "/unused.json".into()],
            vec!["--expected-supplemental-sha256".into(), "a".repeat(64)],
        ] {
            assert!(load_supplemental(&inputs(extra), &"a".repeat(64)).is_err());
        }
        let file = controls_dir().join("bounded.bin");
        std::fs::write(&file, b"12345").unwrap();
        assert!(bounded_supplemental_file(&file, 4).is_err());
        assert_eq!(bounded_supplemental_file(&file, 5).unwrap(), b"12345");
    }
    #[test]
    fn exact_input_parser_positive_source_manifest_receipt_pin_negatives() {
        let (path, receipt, mut manifest) = fixture();
        let parsed = manifest_inputs(&path, &manifest);
        assert_eq!(
            load_supplemental(&parsed, &"a".repeat(64)).unwrap().len(),
            1
        );
        assert!(load_supplemental(&parsed, &"c".repeat(64)).is_err());
        let stale = inputs(vec![
            "--supplemental-resources".into(),
            path.to_str().unwrap().into(),
            "--expected-supplemental-sha256".into(),
            "0".repeat(64),
        ]);
        assert!(load_supplemental(&stale, &"a".repeat(64)).is_err());
        std::fs::write(&receipt, b"changed receipt").unwrap();
        assert!(load_supplemental(&parsed, &"a".repeat(64)).is_err());
        let (path, _, good) = fixture();
        manifest = good;
        manifest["resources"][0]["source_package_sha256"] = "c".repeat(64).into();
        assert!(load_supplemental(&manifest_inputs(&path, &manifest), &"a".repeat(64)).is_err());
    }
}

#[cfg(test)]
mod confirmation_mode_tests {
    use super::*;
    #[test]
    fn global_confirmation_selects_write_and_refuses_missing_or_conflicting_modes() {
        let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../out/nonexistent-confirmation-source.dsgrid");
        assert!(!source.exists());
        for (dry, confirmed, expected) in [
            (false, false, "package_migration_confirmation"),
            (true, true, "package_migration_confirmation"),
            (true, false, "model_not_found"),
            (false, true, "model_not_found"),
        ] {
            let mut tokens = vec![
                "--path".into(),
                source.to_str().unwrap().into(),
                "--expected-source-sha256".into(),
                "a".repeat(64),
            ];
            if dry {
                tokens.push("--dry-run".into());
            }
            let inputs = ds_cli_contract::parse(&COMMAND, &tokens).unwrap();
            let context = Context {
                confirmed,
                output: ds_cli_contract::Output::resolve(
                    ds_cli_contract::Format::Json,
                    false,
                    true,
                ),
            };
            // Reaching the source guard proves both admitted modes; no files
            // are written. Actual DS materialization proves the end-to-end path.
            assert_eq!(run(&inputs, &context).unwrap_err().code(), expected);
        }
    }
}
