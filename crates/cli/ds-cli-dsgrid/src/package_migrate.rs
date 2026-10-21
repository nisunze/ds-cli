//! Explicit format migration and opt-in native lineage successor; no project write.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_network_contracts::grid_operation::{
    GridOperationV1, MigrationSuccessorV1, PackageMigrateParamsV1, PackageMigrateResultV1,
    SupplementalResourceV1,
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
pub(crate) fn materialize_preserving_source(
    out: &Path,
    bytes: &[u8],
    receipt: &Value,
    source_sha256: &str,
    original: Option<&[u8]>,
) -> Result<Value, Failure> {
    if !out.is_absolute() {
        return Err(output_error("output must be absolute"));
    }
    std::fs::create_dir(out).map_err(output_error)?;
    let package_path = out.join("model.dsgrid");
    let receipt_path = out.join("migration.receipt.json");
    let mut files = Vec::new();
    if let Some(original_bytes) = original {
        if format!("{:x}", Sha256::digest(original_bytes)) != source_sha256 {
            return Err(output_error("original archive source pin differs"));
        }
        files.push((out.join("original.dsgrid"), original_bytes.to_vec()));
    }
    files.push((package_path.clone(), bytes.to_vec()));
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
    // The addon strictly verified these bytes before returning them; the
    // readback proves the committed file is exactly those bytes.
    if saved != bytes {
        return Err(output_error("readback differs"));
    }
    if let Some(original) = original
        && std::fs::read(out.join("original.dsgrid")).map_err(output_error)? != original
    {
        return Err(output_error("original archive readback differs"));
    }
    // Commit marker comes only after persisted package/original readback.
    let encoded = serde_json::to_vec_pretty(receipt).map_err(output_error)?;
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
    let (supplemental, mut staged) = load_supplemental(inputs, &digest)?;
    let successor = match successor {
        Some((current_path, current_sha, receipt_path, receipt_sha)) => {
            staged.push((
                "current.dsgrid".to_owned(),
                crate::package::read_bytes(current_path)?,
            ));
            staged.push((
                "migration.receipt.json".to_owned(),
                bounded_supplemental_file(Path::new(receipt_path), 16 * 1024 * 1024)?,
            ));
            Some(MigrationSuccessorV1 {
                expected_source_sha256: digest.clone(),
                current_input: "current.dsgrid".into(),
                expected_current_sha256: current_sha.to_owned(),
                receipt_input: "migration.receipt.json".into(),
                expected_receipt_sha256: receipt_sha.to_owned(),
            })
        }
        None => None,
    };
    let is_successor = successor.is_some();
    let mut outcome = crate::native::operation(
        &GridOperationV1::PackageMigrate(PackageMigrateParamsV1 {
            emit_current: !dry,
            supplemental,
            successor,
        }),
        Some(&bytes),
        &staged,
    )?;
    let migrated: PackageMigrateResultV1 = serde_json::from_value(outcome.result.result.clone())
        .map_err(|error| Failure::failed("network_result_invalid", error.to_string()))?;
    let files = if dry {
        Value::Null
    } else {
        let output = outcome.take_artifact("model.dsgrid").ok_or_else(|| {
            Failure::failed(
                "network_result_invalid",
                "the format migration returned no package",
            )
        })?;
        materialize_preserving_source(
            Path::new(inputs.require("out")?),
            &output,
            &migrated.receipt,
            &migrated.source_sha256,
            if is_successor { Some(&bytes) } else { None },
        )?
    };
    Ok(
        json!({"dry_run":dry,"source_path":source_path,"source_modified":false,"published":false,"receipt":migrated.receipt,"files":files}),
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
#[path = "package_migrate_tests.rs"]
mod tests;

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
/// The admitted supplemental records and the staged bytes they name.
type Supplemental = (Vec<SupplementalResourceV1>, Vec<(String, Vec<u8>)>);

fn load_supplemental(inputs: &Inputs, source_sha: &str) -> Result<Supplemental, Failure> {
    let path = inputs.value("supplemental-resources");
    let pin = inputs.value("expected-supplemental-sha256");
    let (Some(path), Some(pin)) = (path, pin) else {
        if path.is_some() || pin.is_some() {
            return Err(supplemental_error(
                "supply supplemental manifest and exact digest together",
            ));
        }
        return Ok((Vec::new(), Vec::new()));
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
    let mut staged = Vec::new();
    for (index, record) in manifest.resources.into_iter().enumerate() {
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
        let input = format!("supplemental-{index:02}");
        staged.push((input.clone(), bytes));
        result.push(SupplementalResourceV1 {
            resource_id: record.resource_id.as_str().to_owned(),
            invariant_leaf: record.invariant_leaf,
            expected_sha256: record.expected_sha256,
            source_package_sha256: record.source_package_sha256,
            extraction_receipt_sha256: record.extraction_receipt_sha256,
            input,
        });
    }
    Ok((result, staged))
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
#[path = "package_migrate_supplemental_tests.rs"]
mod supplemental_input_tests;

#[cfg(test)]
#[path = "package_migrate_confirmation_tests.rs"]
mod confirmation_mode_tests;
