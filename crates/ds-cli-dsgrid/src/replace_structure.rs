//! Bounded file transport for the exchange-owned structure definition
//! replacement: `import-structure`'s sibling for a name the model already
//! holds. The rows change through the engine's `replace_structure_definition`
//! as one revision; this command reads the bytes, pins the head and writes.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::CommandError;
use ds_grid_exchange::package::PackageError;
use ds_grid_exchange::structure_import::{StructureReplaceError, replace_structure_package};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

use crate::package;

pub static COMMAND: Command = Command {
    id: "dsgrid.replace-structure",
    path: &["dsgrid", "replace-structure"],
    contract: 1,
    summary: "Replace one structure definition with new native bytes.",
    purpose: "Replaces the definition of a structure type the model already holds (raised allowable tables, corrected attachments) with one exact native structure file, as ONE engine revision. The type id is kept, so every placed structure and strung support stays bound; supports rebind by attachment set and slot, and a set or slot in use that the new file drops refuses. Resource bytes, support properties, geometry and the analytical capacity are derived from the new file; a weight-span basis and case bindings the model declared survive. The receipt compares the capacity tables and names the placements whose usage screen changes. Writes a new package, never the source; the previous bytes stay in it as history.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("package", "<path>", "Existing embedded .dsgrid package.").required(),
        Arg::value(
            "source",
            "<path>",
            "One exact native structure file; its filename names the type it replaces.",
        )
        .required(),
        Arg::value(
            "out",
            "<path>",
            "New .dsgrid output; required unless --dry-run; never overwritten.",
        ),
        Arg::value(
            "revision",
            "<rev>",
            "Optional expected authored revision from the model receipt.",
        ),
        Arg::value(
            "expect-sha256",
            "<sha256:hex>",
            "Optional exact native source digest.",
        ),
        Arg::switch(
            "dry-run",
            "Evaluate the replacement and its capacity screen; write nothing.",
        ),
        Arg::value(
            "limit",
            "<n>",
            "Cap the listed placements whose screen changed.",
        )
        .default(package::DEFAULT_LIMIT),
    ],
    output: "Source and resulting authored revision, package revision, type and resource ids, previous and new native digests, placed structures and strung supports kept bound, the span-limit table before and after, the capacity screen (request, changed placements with status and usage before and after, unchanged count, or why it could not run), notes, and the persisted artifact path, bytes and SHA-256. `more.truncated` names a shortened list. No engineering approval is implied.",
    examples: &[
        Example {
            command: "ds dsgrid replace-structure --help",
            note: "Read the replacement contract before changing a definition.",
            runnable: true,
        },
        Example {
            command: "ds dsgrid replace-structure --package ./model.dsgrid --source ./native/a-w-S255.012 --dry-run --output json",
            note: "See the capacity tables and the placements whose screen changes; nothing is written.",
            runnable: false,
        },
    ],
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
            remedy: "restore the exact native member",
        },
        Refusal {
            code: "revision_conflict",
            when: "the package authored head differs from --revision",
            remedy: "inspect the current package and decide against its exact head",
        },
        Refusal {
            code: "structure_not_found",
            when: "no structure type of the model is named like the source file",
            remedy: "name the file exactly as the type (`ds dsgrid run --operation project_table`); add a new one with `ds dsgrid import-structure`",
        },
        Refusal {
            code: "structure_not_native",
            when: "the named type is DS-authored and has no native definition resource",
            remedy: "edit a DS-authored type through its authoring commands",
        },
        Refusal {
            code: "structure_identity_mismatch",
            when: "the new file translates to a different structure type id than the model's",
            remedy: "replace only with a definition of the same invariant name and identity",
        },
        Refusal {
            code: "definition_unchanged",
            when: "the model already holds exactly these bytes for the type",
            remedy: "nothing to replace; keep the current package",
        },
        Refusal {
            code: "replacement_refused",
            when: "the engine refuses the replacement, e.g. the new file drops an attachment set or slot a section is strung on",
            remedy: "read detail.engine; re-string those sections or keep the attachment in the definition",
        },
        Refusal {
            code: "model_validation_failed",
            when: "the replacement would introduce a canonical model error, e.g. a strung set whose slot count changed",
            remedy: "read detail.issues; nothing was written",
        },
        Refusal {
            code: "structure_replace_refused",
            when: "the native owner rejects the source or its kind",
            remedy: "use one supported standalone native structure definition",
        },
        Refusal {
            code: "capacity_screen_failed",
            when: "the before/after usage screen of the type's placements cannot be computed",
            remedy: "report the engine error in detail with the model",
        },
        Refusal {
            code: "library_resolution_required",
            when: "the package requires external pinned releases",
            remedy: "use a release-aware operation; do not remove package pins",
        },
        Refusal {
            code: "package_import_failed",
            when: "the source package or resulting resource closure cannot be verified, or its revision overflows",
            remedy: "validate the source package and report the owner error",
        },
        Refusal {
            code: "invalid_limit",
            when: "--limit is not a whole number in 1..5000",
            remedy: "pass a limit between 1 and 5000",
        },
        Refusal {
            code: "output_required",
            when: "a write omits --out",
            remedy: "name a new .dsgrid file, or pass --dry-run",
        },
        Refusal {
            code: "output_exists",
            when: "the output path already exists",
            remedy: "choose a new path; replacement never overwrites",
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
    search: &[
        "replace definition",
        "allowable tables",
        "raised capacity",
        "structure file",
        "capacity",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let dry_run = inputs.switch("dry-run");
    let limit = package::parse_limit(inputs.value("limit"))?;
    let out = match (inputs.value("out"), dry_run) {
        (Some(out), _) => {
            crate::apply::validate_output_path(out)?;
            Some(out)
        }
        (None, true) => None,
        (None, false) => {
            return Err(Failure::invalid(
                "output_required",
                "a replacement that writes needs --out",
            )
            .remedy("name a new .dsgrid file, or pass --dry-run"));
        }
    };
    let package = package::read_bytes(inputs.require("package")?)?;
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
    let native = crate::import_structure::read_source(source)?;
    let writing = out.is_some() && !dry_run;
    let result = replace_structure_package(
        &package,
        leaf,
        &native,
        inputs.value("revision"),
        inputs.value("expect-sha256"),
        writing,
    )
    .map_err(owner_failure)?;
    let mut receipt = serde_json::to_value(&result).expect("typed replacement receipt serializes");
    let changed = receipt["capacity_screen"]["changed"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let total = changed.len();
    let (shown, withheld) = package::take(changed, limit);
    receipt["capacity_screen"]["changed"] = json!(shown);
    if withheld > 0 {
        receipt["more"] = json!({ "truncated": [{
            "field": "capacity_screen.changed",
            "total": total,
            "shown": limit,
            "withheld": withheld,
            "limit": limit,
        }] });
    }
    receipt["persisted"] = json!(false);
    if let (true, Some(out)) = (writing, out) {
        crate::apply::write_new(out, &result.bytes)?;
        receipt["persisted"] = json!(true);
        receipt["artifact"] = json!({
            "path": out, "byte_len": result.bytes.len(),
            "sha256": format!("sha256:{:x}", Sha256::digest(&result.bytes)),
        });
    }
    Ok(receipt)
}

fn owner_failure(error: StructureReplaceError) -> Failure {
    let message = error.to_string();
    match error {
        StructureReplaceError::RevisionConflict { .. } => {
            Failure::invalid("revision_conflict", message)
                .remedy("inspect the package and decide against its exact head")
        }
        StructureReplaceError::SourceDigestMismatch { .. } => {
            Failure::invalid("source_digest_mismatch", message)
                .remedy("restore the exact native member")
        }
        StructureReplaceError::StructureNotFound(_) => Failure::invalid(
            "structure_not_found",
            message,
        )
        .remedy(
            "name the file exactly as the type; add a new one with `ds dsgrid import-structure`",
        ),
        StructureReplaceError::NotNative { .. } => {
            Failure::invalid("structure_not_native", message)
                .remedy("edit a DS-authored type through its authoring commands")
        }
        StructureReplaceError::IdentityMismatch { .. } => {
            Failure::invalid("structure_identity_mismatch", message)
                .remedy("replace only with a definition of the same invariant name and identity")
        }
        StructureReplaceError::Unchanged(_) => Failure::invalid("definition_unchanged", message)
            .remedy("nothing to replace; keep the current package"),
        StructureReplaceError::Engine(CommandError::Validation { issues }) => Failure::failed(
            "model_validation_failed",
            "the replacement would introduce new canonical model errors",
        )
        .remedy("read detail.issues; nothing was written")
        .detail(json!({ "issues": issues })),
        StructureReplaceError::Engine(error) => Failure::invalid(
            "replacement_refused",
            "the engine refused the replacement",
        )
        .remedy(
            "read detail.engine; re-string those sections or keep the attachment in the definition",
        )
        .detail(json!({ "engine": error.to_string() })),
        StructureReplaceError::Native(_) | StructureReplaceError::NotStructure => {
            Failure::invalid("structure_replace_refused", message)
                .remedy("use one supported standalone native structure definition")
        }
        StructureReplaceError::Screen(_) => Failure::failed("capacity_screen_failed", message)
            .remedy("report the engine error with the model"),
        StructureReplaceError::Package(PackageError::LibraryResolutionRequired) => {
            Failure::invalid("library_resolution_required", message)
                .remedy("use a release-aware operation without removing pins")
        }
        StructureReplaceError::Package(_) | StructureReplaceError::RevisionOverflow => {
            Failure::invalid("package_import_failed", message)
                .remedy("validate the package and report the owner error")
        }
    }
}

pub fn render(data: &Value) -> String {
    let screen = &data["capacity_screen"];
    let mut out = format!(
        "{} {}\n  revision {} -> {}\n  placed {}, strung supports {}\n",
        if data["persisted"].as_bool().unwrap_or(false) {
            "replaced"
        } else {
            "dry run: would replace"
        },
        data["engineering_name"].as_str().unwrap_or("?"),
        data["source_revision"].as_str().unwrap_or("?"),
        data["resulting_revision"].as_str().unwrap_or("?"),
        data["placed_structures"],
        data["strung_supports"],
    );
    match screen["unavailable"].as_str() {
        Some(reason) => out.push_str(&format!("  capacity screen not run: {reason}\n")),
        None => out.push_str(&format!(
            "  capacity screen: {} changed, {} unchanged\n",
            screen["changed"].as_array().map_or(0, Vec::len),
            screen["unchanged_count"]
        )),
    }
    if let Some(path) = data["artifact"]["path"].as_str() {
        out.push_str(&format!("  written {path}\n"));
    }
    out
}
