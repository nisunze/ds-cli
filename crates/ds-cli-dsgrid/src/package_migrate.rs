//! Explicit format migration; no authored revision advance or project write.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_exchange::package_migration::{MigrationOutput, migrate};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path};
pub static COMMAND: Command = Command {
    id: "dsgrid.package.migrate",
    path: &["dsgrid", "package", "migrate"],
    contract: 1,
    summary: "Verify one old package and save its lossless current-format migration.",
    purpose: "The one external migration verifies exact historical source schema, safe membership, every member digest, counts and content fingerprints before transforming named typed fields. Authored revision, voltages, engineering values, qualification, native assets and attachment/history bytes remain unchanged. Dry-run emits the exact source/output SHA and preservation receipt without writing. --yes writes a new local directory containing model.dsgrid and migration.receipt.json; the source is never overwritten. No project publication or version bump occurs. Old artifacts cannot read migrated production heads; retain the current-format artifact floor and use a governed expected-head publication of migrated prior engineering facts for restoration.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
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
    output: "Source/output SHA, original/current schema, strict output verification, exact transformations and preserved metadata; source_modified=false, published=false. Successful materialization returns model.dsgrid and migration.receipt.json. Receipt written last commits the local directory; directories without a receipt are incomplete and never opened.",
    examples: &[],
    refusals: &[
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
    reference: None,
    search: &["schema", "autoheal", "migration"],
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
    if let Some(original) = original {
        if std::fs::read(out.join("original.dsgrid")).map_err(output_error)? != original {
            return Err(output_error("original archive readback differs"));
        }
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
pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let dry = inputs.switch("dry-run");
    if dry == inputs.switch("yes") {
        return Err(Failure::invalid(
            "package_migration_confirmation",
            "choose dry-run or yes",
        ));
    }
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
    let output = migrate(&bytes)
        .map_err(|error| Failure::invalid("package_migration_invalid", error.to_string()))?;
    let files = if dry {
        Value::Null
    } else {
        materialize(Path::new(inputs.require("out")?), &output)?
    };
    Ok(
        json!({"dry_run":dry,"source_path":source_path,"source_modified":false,"published":false,"receipt":output.receipt,"files":files}),
    )
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
        migrate(&blank.bytes).unwrap()
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
