use crate::{engine_failure, read, sha256};
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_exchange::{
    StandardsLibrarySeedOptions, plan_standards_library_seed, portable_backup_seed_members,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub static COMMAND: Command = Command {
    id: "library.seed",
    path: &["library", "seed"],
    contract: 1,
    summary: "Seed an immutable as-built, new-design or custom parallel library.",
    purpose: "Discovers explicit local roots or backups, classifies members by native headers, keeps exact pinned PLS-CADD assets in pls-cadd/, ingests only the characterized PLS-CADD-to-DS-Grid projection into dsgrid/, and atomically promotes library/<id>/<version>. It never publishes, overwrites, opens PLS-CADD or converts DS Grid assets to PLS-CADD.",
    chapter: Chapter::PlsCadd,
    effect: Effect::ArtifactWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::repeated(
            "source",
            "<path>",
            "Source directory, native backup, or standalone member.",
        )
        .required(),
        Arg::value(
            "out",
            "<dir>",
            "Local parent under which library/<id>/<version> is created.",
        )
        .required(),
        Arg::value("library-id", "<id>", "Stable library role/id.").required(),
        Arg::value("library-version", "<id>", "New immutable library version.").required(),
        Arg::value("role", "<role>", "as_built, new_design or custom.")
            .required()
            .choices(&["as_built", "new_design", "custom"]),
        Arg::value("status", "<status>", "review_pending or approved.")
            .default("review_pending")
            .choices(&["review_pending", "approved"]),
        Arg::value("native-family", "<family>", "Pinned native family.").default("pls-cadd-16.81"),
        Arg::value(
            "provenance",
            "<text>",
            "Source authority/digest ruling carried by every member.",
        )
        .required(),
        Arg::repeated(
            "compatibility",
            "<token>",
            "Compatibility token; repeat as needed.",
        ),
        Arg::value("schema", "<n>", "Manifest schema; only 1 is accepted.").default("1"),
    ],
    output: "Immutable local prefix, manifest/content digests, object/byte counts, loss rollup, execution owner, native-tool handoff and remaining engineer decision.",
    examples: &[Example {
        command: "ds library seed --source ./healed-workspace --out ./seed-output --library-id new-design --library-version 2026-08-27-v1 --role new_design --provenance 'healed workspace digest pinned in ruling' --yes --output json",
        note: "Create one local immutable version; no cloud write occurs.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "library_seed_failed",
            when: "classification, ingestion, schema, digest or immutable-layout planning fails",
            remedy: "resolve the reported leaf/source conflict; do not substitute another authority",
        },
        Refusal {
            code: "seed_version_conflict",
            when: "the target version exists with different bytes",
            remedy: "choose a new version; immutable versions are never overwritten",
        },
        Refusal {
            code: "source_symlink_refused",
            when: "a source tree contains a symlink",
            remedy: "provide a resolved regular-file tree with explicit provenance",
        },
        Refusal {
            code: "invalid_schema",
            when: "--schema is not the supported value 1",
            remedy: "pass --schema 1 or omit the flag",
        },
        Refusal {
            code: "invalid_library_id",
            when: "a library id is empty or malformed",
            remedy: "use one stable non-empty library id",
        },
        Refusal {
            code: "invalid_library_version",
            when: "a library version is empty or malformed",
            remedy: "use one stable non-empty immutable library version",
        },
        Refusal {
            code: "library_path_not_found",
            when: "an explicit source/release/catalogue path does not exist",
            remedy: "check the explicit local path",
        },
        Refusal {
            code: "library_path_not_file",
            when: "a command requiring a file receives a non-file path",
            remedy: "pass the exact file, or use a source directory only with seed",
        },
        Refusal {
            code: "library_file_too_large",
            when: "one file exceeds the bounded 2 GiB local read limit",
            remedy: "provide a bounded curated source or backup",
        },
        Refusal {
            code: "library_read_failed",
            when: "a declared local source cannot be read",
            remedy: "check the path and local permissions",
        },
        Refusal {
            code: "output_unwritable",
            when: "a new output/staging path cannot be created or written",
            remedy: "choose a writable local parent with sufficient space",
        },
        Refusal {
            code: "receipt_encode_failed",
            when: "the deterministic receipt cannot be serialized",
            remedy: "report this engine/CLI defect; no version was promoted",
        },
        Refusal {
            code: "seed_stage_exists",
            when: "this process-specific staging path already exists",
            remedy: "verify no seed is active, then remove only that stale stage",
        },
        Refusal {
            code: "seed_verification_failed",
            when: "staged bytes do not re-read exactly before promotion",
            remedy: "inspect the local filesystem; the immutable target was not promoted",
        },
        Refusal {
            code: "seed_promote_failed",
            when: "the verified stage cannot be atomically renamed to its version",
            remedy: "check same-filesystem permissions; no existing version is overwritten",
        },
    ],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};

fn collect_tree(
    base: &Path,
    current: &Path,
    prefix: &str,
    out: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), Failure> {
    let metadata = fs::symlink_metadata(current)
        .map_err(|error| Failure::invalid("library_path_not_found", error.to_string()))?;
    if metadata.file_type().is_symlink() {
        return Err(Failure::invalid(
            "source_symlink_refused",
            format!("`{}` is a symlink", current.display()),
        ));
    }
    if metadata.is_file() {
        let rel = current
            .strip_prefix(base)
            .unwrap_or(current)
            .to_string_lossy()
            .replace('\\', "/");
        let path = if prefix.is_empty() {
            rel
        } else {
            format!("{prefix}/{rel}")
        };
        out.push((path, read(current.to_string_lossy().as_ref())?));
        return Ok(());
    }
    let mut entries = fs::read_dir(current)
        .map_err(|error| Failure::invalid("library_read_failed", error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| Failure::invalid("library_read_failed", error.to_string()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        collect_tree(base, &entry.path(), prefix, out)?;
    }
    Ok(())
}

fn sources(inputs: &Inputs) -> Result<Vec<(String, Vec<u8>)>, Failure> {
    let mut out = Vec::new();
    for raw in inputs.repeated("source") {
        let path = Path::new(raw);
        if path.is_dir() {
            let prefix = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("source");
            collect_tree(path, path, prefix, &mut out)?;
        } else {
            let bytes = read(raw)?;
            if raw.to_ascii_lowercase().ends_with(".bak") {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("source.bak");
                out.extend(
                    portable_backup_seed_members(name, &bytes)
                        .map_err(|error| engine_failure("library_seed_failed", error))?,
                );
            } else {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| {
                        Failure::invalid("library_path_not_file", "source has no filename")
                    })?;
                out.push((name.to_string(), bytes));
            }
        }
    }
    out.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(out)
}

fn existing_matches(
    target: &Path,
    files: &BTreeMap<String, Vec<u8>>,
    manifest: &[u8],
    receipt: &[u8],
) -> bool {
    fs::read(target.join("manifest.json")).ok().as_deref() == Some(manifest)
        && fs::read(target.join("receipt.json")).ok().as_deref() == Some(receipt)
        && files
            .iter()
            .all(|(path, bytes)| fs::read(target.join(path)).ok().as_deref() == Some(bytes))
}

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let schema = inputs
        .value("schema")
        .unwrap_or("1")
        .parse::<u32>()
        .map_err(|_| Failure::invalid("invalid_schema", "--schema must be 1"))?;
    let compatibility = {
        let values = inputs.repeated("compatibility");
        if values.is_empty() {
            vec!["dsgrid-schema-v1".to_string(), "pls-cadd-16.81".to_string()]
        } else {
            values.iter().map(|s| s.to_string()).collect()
        }
    };
    let options = StandardsLibrarySeedOptions {
        library_id: inputs.require("library-id")?.to_string(),
        version: inputs.require("library-version")?.to_string(),
        role: inputs.require("role")?.to_string(),
        status: inputs
            .value("status")
            .unwrap_or("review_pending")
            .to_string(),
        native_family: inputs
            .value("native-family")
            .unwrap_or("pls-cadd-16.81")
            .to_string(),
        source_provenance: inputs.require("provenance")?.to_string(),
        compatibility,
    };
    let plan = plan_standards_library_seed(schema, &sources(inputs)?, &options)
        .map_err(|error| engine_failure("library_seed_failed", error))?;
    let receipt_bytes = serde_json::to_vec_pretty(&plan.receipt)
        .map_err(|error| Failure::internal("receipt_encode_failed", error.to_string()))?;
    let target = PathBuf::from(inputs.require("out")?).join(&plan.receipt.immutable_prefix);
    if target.exists() {
        if existing_matches(&target, &plan.files, &plan.manifest_bytes, &receipt_bytes) {
            return Ok(answer(&target, &plan, true));
        }
        return Err(Failure::conflict(
            "seed_version_conflict",
            format!("`{}` exists with different bytes", target.display()),
        )
        .remedy("choose a new immutable version"));
    }
    let parent = target.parent().expect("version has parent");
    fs::create_dir_all(parent)
        .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
    let stage = parent.join(format!(".{}-stage-{}", options.version, std::process::id()));
    if stage.exists() {
        return Err(Failure::conflict(
            "seed_stage_exists",
            format!("`{}` already exists", stage.display()),
        )
        .remedy("remove the stale stage after verifying it is not active"));
    }
    let materialized = (|| -> Result<(), Failure> {
        fs::create_dir(&stage)
            .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
        for (path, bytes) in &plan.files {
            let dest = stage.join(path);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
            }
            fs::write(dest, bytes)
                .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
        }
        fs::write(stage.join("manifest.json"), &plan.manifest_bytes)
            .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
        fs::write(stage.join("receipt.json"), &receipt_bytes)
            .map_err(|error| Failure::failed("output_unwritable", error.to_string()))?;
        if !existing_matches(&stage, &plan.files, &plan.manifest_bytes, &receipt_bytes) {
            return Err(Failure::failed(
                "seed_verification_failed",
                "staged seed bytes did not re-read exactly",
            ));
        }
        fs::rename(&stage, &target)
            .map_err(|error| Failure::failed("seed_promote_failed", error.to_string()))?;
        Ok(())
    })();
    if let Err(error) = materialized {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }
    Ok(answer(&target, &plan, false))
}

fn answer(
    target: &Path,
    plan: &ds_grid_exchange::StandardsLibrarySeedPlan,
    idempotent: bool,
) -> Value {
    let decision = match plan.manifest.role.as_str() {
        "as_built" => {
            "Engineer must rule whether the source certifies geometry/shading only or also strength authority."
        }
        "new_design" => {
            "Engineer must approve the conservative-strength claim; this seed certifies only parsed bytes and mappings."
        }
        _ => "Engineer retains the stated project/library adoption decision.",
    };
    json!({ "written": target, "idempotent": idempotent, "receipt": plan.receipt, "loss_rollup": plan.manifest.loss_rollup, "manifest_sha256": sha256(&plan.manifest_bytes), "execution_owner": "ds", "deterministic_completion": "local immutable version materialized and every staged byte re-read exactly", "pls_cadd_ui_handoff": { "required": false, "condition": "Only native solver/check or operator acceptance requires PLS-CADD; seeding never opens it.", "artifact": format!("{}/manifest.json", target.display()), "digest": format!("sha256:{}", sha256(&plan.manifest_bytes)), "post_ui_reimport": "Re-import any native-saved workspace as a new authority candidate; never mutate this version." }, "engineer_decision": decision, "cloud_write": false })
}

pub fn render(data: &Value) -> String {
    format!(
        "seeded {}\nmanifest {}\ncloud write: false",
        data["written"].as_str().unwrap_or(""),
        data["manifest_sha256"].as_str().unwrap_or("")
    )
}
