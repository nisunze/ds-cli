//! `ds dsgrid apply` — revision-gated canonical model editing.
//!
//! The command envelope and mutation rules belong to `ds-grid-engine`; package
//! verification and emission belong to `ds-grid-exchange`. This module owns
//! only the CLI boundary: bounded file reads, typed refusal mapping, a
//! no-overwrite output policy, and a compact receipt.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::{CommandError, GridCommand};
use serde_json::{Value, json};

use crate::package;
use ds_network_contracts::grid_apply::{GRID_APPLY_REQUEST_SCHEMA_V1, NativeGridApplyRequestV1};

const MAX_ENVELOPE_BYTES: u64 =
    ds_network_contracts::grid_apply::MAX_GRID_COMMAND_ENVELOPE_BYTES as u64;

pub static COMMAND: Command = Command {
    id: "dsgrid.apply",
    path: &["dsgrid", "apply"],
    contract: 1,
    summary: "Apply one revision-pinned engine command to a new .dsgrid file.",
    purpose: "\
Reads one verified .dsgrid package and one typed command envelope published by \
the captured Network engine. A dry run evaluates the exact revision gate and \
model invariants without writing. An apply preserves the source package, its \
assets and exchange bindings, advances the package revision, and writes one \
new package at --out. Existing output paths are never overwritten.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<path>", "The source .dsgrid package.").required(),
        Arg::value(
            "envelope",
            "<json-path>",
            "One ds-grid CommandEnvelope JSON document.",
        )
        .required(),
        Arg::value(
            "out",
            "<path>",
            "New .dsgrid path; required unless --dry-run.",
        ),
        Arg::switch("dry-run", "Evaluate the command and write nothing."),
    ],
    output: "\
The source package identity, expected and resulting authored revisions, the \
engine delta and any newly introduced validation issues. A successful apply \
also returns the new package path, package revision, byte length and SHA-256.",
    examples: &[
        Example {
            command: "ds dsgrid apply --model ./model.dsgrid --envelope ./move-pi.json --dry-run --output json",
            note: "Evaluate the exact command against the current model revision.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid apply --model ./model.dsgrid --envelope ./move-pi.json --out ./model-revised.dsgrid --output json",
            note: "Write a new revision; the source remains untouched.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid apply --model ./model.dsgrid --envelope ./set-load-ref.json --dry-run --output json",
            note: "Correct a staking fact such as staking.load_ref (kVA): read editable fields with `ds dsgrid run --model <path> --operation profile_properties`, then send edit_profile_properties.",
            runnable: false,
        },
    ],
    refusals: &crate::native::extend_refusals::<
        { OWN_REFUSALS.len() + crate::native::REFUSALS.len() },
    >(OWN_REFUSALS),
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "staking facts",
        "load reference",
        "kva",
        "earthing",
        "structure correction",
    ],
    requires: Requires::Server,
    availability: available,
};

const OWN_REFUSALS: &[Refusal] = &[
    package::PROTECTED_MODEL,
    package::PLS_SECTION_IDENTITY_UNRESOLVED,
    Refusal {
        code: "model_not_found",
        when: "the source path does not exist or is not a file",
        remedy: "check the path; --model takes one .dsgrid file",
    },
    Refusal {
        code: "model_too_large",
        when: "the source is above the 512 MiB read bound",
        remedy: "confirm the file is a .dsgrid package and not a disk image",
    },
    Refusal {
        code: "model_unreadable",
        when: "the source exists but cannot be read",
        remedy: "check file permissions",
    },
    Refusal {
        code: "not_a_dsgrid_package",
        when: "the source bytes are not a readable .dsgrid container",
        remedy: "convert the native source through dsgrid-exchange first",
    },
    Refusal {
        code: "package_decode_failed",
        when: "the source manifest or canonical tables do not verify",
        remedy: "run `ds dsgrid validate --model <path>` and repair the package",
    },
    Refusal {
        code: "envelope_not_found",
        when: "--envelope does not name one regular file",
        remedy: "write one bounded CommandEnvelope JSON file",
    },
    Refusal {
        code: "envelope_too_large",
        when: "the envelope exceeds 16 MiB",
        remedy: "use one bounded engine command; bulk terrain belongs in author_terrain_points",
    },
    Refusal {
        code: "envelope_unreadable",
        when: "the envelope exists but cannot be read",
        remedy: "check file permissions",
    },
    Refusal {
        code: "envelope_invalid",
        when: "the JSON is not the captured Network engine's CommandEnvelope schema",
        remedy: "read the command descriptor with `ds dsgrid describe --kind commands --id <id>`",
    },
    Refusal {
        code: "admin_authority_required",
        when: "a generic command envelope attempts to author Rwanda admin fields without exact local index resolution",
        remedy: "use ds dsgrid structure admin-refresh with a verified village index",
    },
    Refusal {
        code: "revision_conflict",
        when: "expected_revision does not equal the model's current authored revision",
        remedy: "re-read the model and deliberately rebuild the envelope against its current revision",
    },
    Refusal {
        code: "target_not_found",
        when: "the command addresses an entity absent from this model",
        remedy: "use an id from a projection of this exact package revision",
    },
    Refusal {
        code: "command_invalid",
        when: "the engine rejects the command's typed values or semantics",
        remedy: "read detail.engine and the live command descriptor; do not approximate missing values",
    },
    Refusal {
        code: "terrain_acquisition_required",
        when: "elevation interpolation has no effective ground coverage at the requested station",
        remedy: "author a verified terrain source and observations before inserting the point",
    },
    Refusal {
        code: "model_validation_failed",
        when: "the command would introduce new canonical model errors",
        remedy: "read detail.issues and revise the command; no output was written",
    },
    Refusal {
        code: "command_replay_conflict",
        when: "a transaction partially overlaps commands already applied in the live session",
        remedy: "refresh the model and submit one fresh, non-overlapping intent",
    },
    Refusal {
        code: "output_required",
        when: "an apply omits --out",
        remedy: "name a new .dsgrid file, or pass --dry-run",
    },
    Refusal {
        code: "output_exists",
        when: "--out already exists",
        remedy: "choose a new path; apply never overwrites the source or an earlier result",
    },
    Refusal {
        code: "output_parent_missing",
        when: "the output parent directory does not exist",
        remedy: "create the intended directory, then retry",
    },
    Refusal {
        code: "package_emit_failed",
        when: "the revised canonical snapshot cannot be packaged with its retained assets",
        remedy: "report this engine failure with the model and envelope digests",
    },
    Refusal {
        code: "output_unwritable",
        when: "the new output file cannot be created or fully written",
        remedy: "check the parent path and permissions; a partial file is removed",
    },
];

fn available() -> Availability {
    ds_network_client::available()
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let model_path = inputs.require("model")?;
    let envelope_path = inputs.require("envelope")?;
    let dry_run = inputs.switch("dry-run");

    let out_path = if dry_run {
        None
    } else {
        let path = inputs.value("out").ok_or_else(|| {
            Failure::invalid("output_required", "an apply needs a new output path")
                .remedy("pass --out <new.dsgrid>, or use --dry-run")
        })?;
        validate_output_path(path)?;
        Some(path)
    };
    let model_bytes = package::read_bytes(model_path)?;
    let envelope = read_envelope(envelope_path)?;
    let captured = crate::native::capture()?;
    let scratch = crate::native::scratch()?;
    let inspection = captured
        .grid_inspect(&model_bytes, scratch.path(), &|| false)
        .map_err(crate::native::map_failure)?;
    let request = NativeGridApplyRequestV1 {
        schema: GRID_APPLY_REQUEST_SCHEMA_V1.into(),
        source: inspection.result.source,
        envelope,
        dry_run,
    };
    let output = captured
        .grid_apply(&request, &model_bytes, scratch.path(), &|| false)
        .map_err(crate::native::map_failure)?;
    let mut projection = output.metadata.result.projection;
    projection["source"]["path"] = json!(model_path);
    if let Some(path) = out_path {
        let bytes = output.model.ok_or_else(|| {
            Failure::failed(
                "package_emit_failed",
                "the Network owner returned no complete package",
            )
        })?;
        write_new(path, &bytes)?;
        projection["persisted"] = json!(true);
        projection["artifact"]["path"] = json!(path);
    }
    Ok(projection)
}

/// The generic JSON lane cannot prove that a named village came from the
/// exact local index. Only the typed admin-refresh adapter resolves that proof.
pub(crate) fn guard_admin_refresh(command: &GridCommand) -> Result<(), Failure> {
    if matches!(command, GridCommand::RefreshRwandaAdmin { .. }) {
        return Err(Failure::invalid(
            "admin_authority_required",
            "Rwanda admin refresh needs exact village-index resolution",
        )
        .remedy("use ds dsgrid structure admin-refresh with a verified village index"));
    }
    Ok(())
}

fn read_envelope(raw_path: &str) -> Result<Value, Failure> {
    let path = Path::new(raw_path);
    let metadata = std::fs::metadata(path).map_err(|error| {
        Failure::invalid("envelope_not_found", format!("cannot read `{raw_path}`"))
            .remedy("--envelope takes one CommandEnvelope JSON file")
            .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    if !metadata.is_file() {
        return Err(
            Failure::invalid("envelope_not_found", format!("`{raw_path}` is not a file"))
                .remedy("--envelope takes one CommandEnvelope JSON file"),
        );
    }
    if metadata.len() > MAX_ENVELOPE_BYTES {
        return Err(Failure::invalid(
            "envelope_too_large",
            format!("`{raw_path}` exceeds the envelope bound"),
        )
        .remedy("use one bounded command; bulk terrain belongs in author_terrain_points")
        .detail(json!({
            "byte_len": metadata.len(),
            "max_byte_len": MAX_ENVELOPE_BYTES,
        })));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_ENVELOPE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| {
            Failure::failed("envelope_unreadable", format!("cannot read `{raw_path}`"))
                .remedy("check file permissions")
                .detail(json!({ "detail": error.kind().to_string() }))
        })?;
    if bytes.len() as u64 > MAX_ENVELOPE_BYTES {
        return Err(Failure::invalid(
            "envelope_too_large",
            "the command envelope grew above its admitted bound",
        )
        .remedy("use one bounded command envelope"));
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        Failure::invalid(
            "envelope_invalid",
            "the command envelope is not valid for this engine",
        )
        .remedy("read the live command descriptor and rebuild the envelope")
        .detail(json!({ "detail": error.to_string() }))
    })
}

pub(crate) fn map_command_error(error: CommandError) -> Failure {
    match error {
        CommandError::InvalidInput { operation, message }
            if operation == ds_grid_engine::model_protection::PROTECTED_CODE =>
        {
            Failure::invalid("composite_model_protected", message)
                .remedy(package::PROTECTED_MODEL.remedy)
        }
        CommandError::StaleRevision { expected, actual } => Failure::conflict(
            "revision_conflict",
            "the command envelope was authored against a different revision",
        )
        .remedy("re-read the model and deliberately rebuild the envelope")
        .detail(json!({
            "expected_revision": expected.as_str(),
            "actual_revision": actual.as_str(),
        })),
        CommandError::TargetNotFound { kind, id } => Failure::invalid(
            "target_not_found",
            format!("the command targets missing {kind} `{id}`"),
        )
        .remedy("use an id projected from this exact package revision"),
        CommandError::InvalidInput { operation, message } => Failure::invalid(
            "command_invalid",
            format!("the engine rejected `{operation}`"),
        )
        .remedy("read the live command descriptor; do not approximate missing values")
        .detail(json!({ "engine": message })),
        CommandError::TerrainAcquisitionRequired {
            alignment_id,
            station_m,
        } => Failure::failed(
            "terrain_acquisition_required",
            "the requested station has no effective ground coverage",
        )
        .remedy("author a verified terrain source and observations before interpolation")
        .detail(json!({ "alignment_id": alignment_id, "station_m": station_m })),
        CommandError::Validation { issues } => Failure::failed(
            "model_validation_failed",
            "the command would introduce new canonical model errors",
        )
        .remedy("revise the command; no output was written")
        .detail(json!({ "issues": issues })),
        CommandError::PartialTransactionOverlap {
            already_applied,
            fresh,
        } => Failure::conflict(
            "command_replay_conflict",
            "the command transaction only partially overlaps prior work",
        )
        .remedy("refresh the model and submit one fresh intent")
        .detail(json!({ "already_applied": already_applied, "fresh": fresh })),
    }
}

pub(crate) fn validate_output_path(raw_path: &str) -> Result<(), Failure> {
    let path = Path::new(raw_path);
    if path.exists() {
        return Err(
            Failure::conflict("output_exists", format!("`{raw_path}` already exists"))
                .remedy("choose a new output path; native file commands never overwrite"),
        );
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if parent.is_some_and(|parent| !parent.is_dir()) {
        return Err(Failure::invalid(
            "output_parent_missing",
            format!("the parent of `{raw_path}` does not exist"),
        )
        .remedy("create the intended output directory, then retry"));
    }
    Ok(())
}

pub(crate) fn write_new(raw_path: &str, bytes: &[u8]) -> Result<(), Failure> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(raw_path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Failure::conflict("output_exists", format!("`{raw_path}` already exists"))
                    .remedy("choose a new output path; native file commands never overwrite")
            } else {
                Failure::failed("output_unwritable", format!("cannot create `{raw_path}`"))
                    .remedy("choose a new writable output path")
                    .detail(json!({ "detail": error.kind().to_string() }))
            }
        })?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(raw_path);
        return Err(Failure::failed(
            "output_unwritable",
            format!("could not finish writing `{raw_path}`"),
        )
        .remedy("check free space and permissions; the partial file was removed")
        .detail(json!({ "detail": error.kind().to_string() })));
    }
    Ok(())
}

pub fn render(data: &Value) -> String {
    if data["dry_run"].as_bool().unwrap_or(false) {
        return format!(
            "dry run  {}\ncommand  {}\nrevision {} -> {}\nvalid     {}\nwritten   no\n",
            data["source"]["model_id"].as_str().unwrap_or("?"),
            data["command_kind"].as_str().unwrap_or("?"),
            data["source"]["authored_revision"].as_str().unwrap_or("?"),
            data["resulting_revision"].as_str().unwrap_or("?"),
            data["would_apply"].as_bool().unwrap_or(false),
        );
    }

    format!(
        "applied   {}\ncommand   {}\nrevision  {} -> {}\npackage   rev {}\nwritten   {}\nsha256    {}\n",
        data["source"]["model_id"].as_str().unwrap_or("?"),
        data["command_kind"].as_str().unwrap_or("?"),
        data["parent_revision"].as_str().unwrap_or("?"),
        data["resulting_revision"].as_str().unwrap_or("?"),
        data["artifact"]["package_revision"],
        data["artifact"]["path"].as_str().unwrap_or("?"),
        data["artifact"]["sha256"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
mod admin_guard_tests {
    use super::*;

    #[test]
    fn generic_envelope_refuses_unverified_admin_names() {
        let forged = GridCommand::RefreshRwandaAdmin {
            authority_sha256: "unverified".into(),
            rows: Vec::new(),
            preview_token: "unverified".into(),
            accept_existing_conflicts: false,
        };
        let error = guard_admin_refresh(&forged).unwrap_err();
        assert_eq!(error.code(), "admin_authority_required");
    }

    #[test]
    fn generic_envelope_keeps_unrelated_commands_available() {
        let ordinary = GridCommand::EditEntityProperties { edits: Vec::new() };
        assert!(guard_admin_refresh(&ordinary).is_ok());
    }
}
