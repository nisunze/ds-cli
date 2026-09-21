//! Bounded file transport for the exchange-owned additive structure import.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_exchange::package::PackageError;
use ds_grid_exchange::pls_cadd_library::PlsCaddLibraryError;
use ds_grid_exchange::structure_import::{StructureImportError, import_structure_package};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

pub static COMMAND: Command = Command {
    id: "dsgrid.import-structure",
    path: &["dsgrid", "import-structure"],
    contract: 1,
    summary: "Add one native structure and its exact bytes to a model.",
    purpose: "Backfills a missing structure definition, its attachments and imported analytical strength tables into one existing .dsgrid package. The exchange owner retains exact source bytes and all existing assets and provenance. Duplicate names refuse; this command neither replaces definitions nor authors spotting eligibility, prices or engineering approval. Pinned packages require a separate release-aware operation and refuse here. Writes a new package revision, never the source.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("package", "<path>", "Existing embedded .dsgrid package.").required(),
        Arg::value(
            "source",
            "<path>",
            "One exact native structure file; its filename is the invariant engineering name.",
        )
        .required(),
        Arg::value("out", "<path>", "New .dsgrid output; never overwritten.").required(),
        Arg::value(
            "revision",
            "<rev>",
            "Optional expected authored revision from the model receipt.",
        ),
        Arg::value(
            "expect-sha256",
            "<sha256:hex>",
            "Optional exact native source digest from the pinned library resolution.",
        ),
    ],
    output: "Source and resulting authored revision, new package revision, added entity/resource IDs and capacity count, exact native digest, notes, and persisted artifact path, bytes and SHA-256. No engineering approval is implied.",
    examples: &[Example {
        command: "ds dsgrid import-structure --help",
        note: "Read the additive import contract before backfilling a model.",
        runnable: true,
    }],
    refusals: &[
        Refusal {
            code: "model_not_found",
            when: "the package path is absent or not a regular file",
            remedy: "name one existing .dsgrid package",
        },
        Refusal {
            code: "model_too_large",
            when: "the package exceeds 512 MiB",
            remedy: "use a bounded .dsgrid package",
        },
        Refusal {
            code: "model_unreadable",
            when: "the package cannot be read",
            remedy: "check package permissions",
        },
        Refusal {
            code: "source_unreadable",
            when: "the source is absent, not a file, unreadable or has no UTF-8 filename",
            remedy: "name one readable native structure file with an invariant filename",
        },
        Refusal {
            code: "source_too_large",
            when: "the source exceeds 64 MiB",
            remedy: "use one bounded native structure definition",
        },
        Refusal {
            code: "source_digest_mismatch",
            when: "source bytes differ from the expected SHA-256",
            remedy: "restore the exact resolved native member",
        },
        Refusal {
            code: "revision_conflict",
            when: "the package authored head differs from --revision",
            remedy: "inspect the current package and decide against its exact head",
        },
        Refusal {
            code: "structure_conflict",
            when: "the native name or imported entity IDs already exist",
            remedy: "use this command only for a missing definition; replacement is a separate operation",
        },
        Refusal {
            code: "structure_import_refused",
            when: "the native owner rejects the source, its kind or resulting model",
            remedy: "use one supported standalone native structure with valid semantic closure",
        },
        Refusal {
            code: "library_resolution_required",
            when: "the package requires external pinned releases",
            remedy: "use a release-aware import operation; do not remove package pins",
        },
        Refusal {
            code: "package_import_failed",
            when: "the source package or resulting resource closure cannot be verified, or its revision overflows",
            remedy: "validate the source package and report the owner error",
        },
        Refusal {
            code: "output_exists",
            when: "the output path already exists",
            remedy: "choose a new path; import never overwrites",
        },
        Refusal {
            code: "output_parent_missing",
            when: "the output parent does not exist",
            remedy: "create the intended output directory",
        },
        Refusal {
            code: "output_unwritable",
            when: "the output cannot be created and synchronized",
            remedy: "check permissions and free space; partial output is removed",
        },
    ],
    reference: Some("docs/reference/dsgrid.md"),
    search: &["backfill", "native", "capacity", "library"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let out = inputs.require("out")?;
    crate::apply::validate_output_path(out)?;
    let package = crate::package::read_bytes(inputs.require("package")?)?;
    let source = inputs.require("source")?;
    let leaf = Path::new(source)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            Failure::invalid(
                "source_unreadable",
                "source needs an invariant UTF-8 filename",
            )
            .remedy("name one readable native structure file")
        })?;
    let native = read_source(source)?;
    let result = import_structure_package(
        &package,
        leaf,
        &native,
        inputs.value("revision"),
        inputs.value("expect-sha256"),
    )
    .map_err(owner_failure)?;
    crate::apply::write_new(out, &result.bytes)?;
    let mut receipt = serde_json::to_value(&result).expect("typed import receipt serializes");
    receipt["persisted"] = json!(true);
    receipt["artifact"] = json!({
        "path": out, "byte_len": result.bytes.len(),
        "sha256": format!("sha256:{:x}", Sha256::digest(&result.bytes)),
    });
    Ok(receipt)
}

fn owner_failure(error: StructureImportError) -> Failure {
    let message = error.to_string();
    match error {
        StructureImportError::RevisionConflict { .. } => {
            Failure::invalid("revision_conflict", message)
                .remedy("inspect the package and decide against its exact head")
        }
        StructureImportError::SourceDigestMismatch { .. } => {
            Failure::invalid("source_digest_mismatch", message)
                .remedy("restore the exact resolved native member")
        }
        StructureImportError::Native(PlsCaddLibraryError::Conflict(_)) => {
            Failure::invalid("structure_conflict", message)
                .remedy("import only a missing definition; replacement is separate")
        }
        StructureImportError::Native(_) | StructureImportError::NotStructure => {
            Failure::invalid("structure_import_refused", message)
                .remedy("use one supported standalone native structure")
        }
        StructureImportError::Package(PackageError::LibraryResolutionRequired) => {
            Failure::invalid("library_resolution_required", message)
                .remedy("use a release-aware operation without removing pins")
        }
        StructureImportError::Package(_) | StructureImportError::RevisionOverflow => {
            Failure::invalid("package_import_failed", message)
                .remedy("validate the package and report the owner error")
        }
    }
}

fn read_source(path: &str) -> Result<Vec<u8>, Failure> {
    const MAX: u64 = 64 * 1024 * 1024;
    let unreadable = |error: std::io::Error| {
        Failure::invalid("source_unreadable", format!("cannot read {path}: {error}"))
            .remedy("name one readable native structure file")
    };
    let file = std::fs::File::open(path).map_err(unreadable)?;
    let metadata = file.metadata().map_err(unreadable)?;
    if !metadata.is_file() {
        return Err(
            Failure::invalid("source_unreadable", "source must be a regular file")
                .remedy("name one native structure file"),
        );
    }
    let too_large = || {
        Failure::invalid("source_too_large", "source exceeds 64 MiB")
            .remedy("use one bounded native structure definition")
    };
    if metadata.len() > MAX {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.take(MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    if bytes.len() as u64 > MAX {
        return Err(too_large());
    }
    Ok(bytes)
}

pub fn render(data: &Value) -> String {
    format!(
        "imported structure into {}\n  revision {}\n",
        data["artifact"]["path"].as_str().unwrap_or("?"),
        data["resulting_revision"].as_str().unwrap_or("?")
    )
}
