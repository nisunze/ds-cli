//! Atomic file-only application of typed engine commands, packed once.

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_network_contracts::grid_apply_batch::{
    GRID_APPLY_BATCH_REQUEST_SCHEMA_V1, NativeGridApplyBatchRequestV1, NativeGridBatchCommandV1,
    NativeGridCorrectionGuardV1,
};
use ds_network_contracts::grid_operation::{BatchParseResultV1, GridOperationV1};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    apply::{validate_output_path, write_new},
    package,
};

const MAX_BATCH_BYTES: u64 = ds_network_contracts::grid_apply_batch::MAX_GRID_BATCH_BYTES as u64;
const MAX_COMMANDS: usize = ds_network_contracts::grid_apply_batch::MAX_GRID_BATCH_COMMANDS;

pub static COMMAND: Command = Command {
    id: "dsgrid.apply-batch",
    path: &["dsgrid", "apply-batch"],
    contract: 1,
    summary: "Apply a revision-pinned batch atomically to one new .dsgrid file.",
    purpose: "Reads one verified package and a bounded JSON batch. Use {expected_revision, \
commands: [{command_id, command}]} with typed GridCommand values, or an array of \
CommandEnvelope values all pinned to the same expected_revision. The engine \
chains intermediate revisions in memory; any failed command rolls back the \
whole transaction. A dry run exercises identical engine gates without writing. \
Apply retains assets and exchange bindings and packs once to a new output. \
Commands are ordered, IDs must be unique, and intermediate states must validate. \
Batch input is limited to 16 MiB and 4096 commands. No live model is changed. \
For reviewed corrections, --guard pins the exact source package and selected \
alignments, and --select cherry-picks named command IDs in original order. \
Only characterized commands and changes confined to that scope can succeed.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<path>", "The source .dsgrid package.").required(),
        Arg::value(
            "batch",
            "<json-path>",
            "JSON batch with one expected revision and ordered typed commands.",
        )
        .required(),
        Arg::value(
            "out",
            "<path>",
            "New .dsgrid path; required unless --dry-run.",
        ),
        Arg::switch(
            "dry-run",
            "Evaluate the whole transaction and write nothing.",
        ),
        Arg::value(
            "guard",
            "<json-path>",
            "Reviewed correction guard: source_sha256, alignment_ids, allowed_command_kinds.",
        ),
        Arg::repeated(
            "select",
            "<command-id>",
            "Cherry-pick this command ID from the batch; repeat. Requires --guard.",
        ),
    ],
    output: "Source identity and authored revision, batch digest, selected command IDs, \
command counts by kind, resulting authored revision and package revision increment. \
A guarded correction also reports review references and an engineering change \
summary. A successful write adds output path, package revision, byte length and \
SHA-256. Receipt size is independent of entity count.",
    examples: &[
        Example {
            command: "ds dsgrid apply-batch --model ./model.dsgrid --batch ./commands.json --dry-run --output json",
            note: "Evaluate the entire batch against its pinned model revision.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid apply-batch --model ./model.dsgrid --batch ./commands.json --out ./model-revised.dsgrid --output json",
            note: "Write a new revision; the source remains untouched.",
            runnable: false,
        },
    ],
    refusals: &crate::native::extend_refusals::<
        { OWN_REFUSALS.len() + crate::native::REFUSALS.len() },
    >(OWN_REFUSALS),
    reference: Some("docs/reference/dsgrid.md"),
    search: &[],
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
        code: "batch_not_found",
        when: "--batch does not name one regular file",
        remedy: "write one bounded batch JSON file",
    },
    Refusal {
        code: "batch_too_large",
        when: "the batch exceeds 16 MiB or 4096 commands",
        remedy: "use a batch of at most 4096 commands and 16 MiB",
    },
    Refusal {
        code: "batch_unreadable",
        when: "the envelope exists but cannot be read",
        remedy: "check file permissions",
    },
    Refusal {
        code: "batch_invalid",
        when: "a row is malformed (named by row, command_kind, field path and serde error), revision pins differ, IDs repeat, or no command is given",
        remedy: "fix the named field; `ds dsgrid describe --kind types --id <Type>` gives the row shape",
    },
    Refusal {
        code: "selection_invalid",
        when: "a selected command ID is duplicate or absent, or --select omits --guard",
        remedy: "choose unique command IDs present in the batch and supply a correction guard",
    },
    Refusal {
        code: "correction_guard_invalid",
        when: "guard JSON is unreadable, invalid, lacks an exact source package digest and selected scope, or selected commands lack review references",
        remedy: "provide source_sha256, alignment_ids, allowed_command_kinds, and a review_ref for each selected command",
    },
    Refusal {
        code: "correction_not_approved",
        when: "a guarded correction is still a draft or lacks an exact approval decision reference",
        remedy: "dry-run the proposal, then supply review_state approved and an authored decision_ref before writing",
    },
    Refusal {
        code: "source_digest_conflict",
        when: "the source .dsgrid bytes differ from the reviewed package",
        remedy: "re-read the package and re-author the correction guard",
    },
    Refusal {
        code: "correction_scope_violation",
        when: "a guarded command names a missing target, edits a read-only field, or exceeds its selected alignments or permitted kinds",
        remedy: "for read-only fields, remove that edit; for missing entities, select an existing target; for alignment-scope conflicts, split the correction or explicitly include every affected alignment after review",
    },
    Refusal {
        code: "admin_authority_required",
        when: "a batch carries Rwanda admin facts that have not been resolved from the exact local village index",
        remedy: "structure admin refresh is not available in this release; remove the Rwanda admin field edits from the envelope",
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
        remedy: "report this engine failure with the model and batch digests",
    },
    Refusal {
        code: "output_unwritable",
        when: "the new output file cannot be created or fully written",
        remedy: "check the parent path and permissions; a partial file is removed",
    },
];

/// The review workflow's mandatory-scope entry point. It calls the same
/// transaction implementation as apply-batch, but cannot omit the guard.
pub static CORRECTION: Command = Command {
    id: "dsgrid.apply-correction",
    path: &["dsgrid", "apply-correction"],
    contract: 1,
    summary: "Apply reviewed findings within an exact Grid correction scope.",
    purpose: "Reads a revision-pinned typed command batch and a mandatory correction guard. \
The guard pins source .dsgrid bytes, selected alignments or angle intervals, and permitted command kinds. \
Each selected command must carry a review_ref naming a comment or finding. \
--select cherry-picks IDs in original \
batch order. The engine validates each command and the final unselected canonical rows \
before any output is written. A write requires review_state approved and an authored \
decision_ref; a draft can dry-run. Dry-run and write use identical engineering checks.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<path>", "The source .dsgrid package.").required(),
        Arg::value(
            "batch",
            "<json-path>",
            "Revision-pinned typed commands with review_ref values.",
        )
        .required(),
        Arg::value(
            "guard",
            "<json-path>",
            "Exact source digest, selected alignment IDs, and allowed command kinds.",
        )
        .required(),
        Arg::repeated(
            "select",
            "<command-id>",
            "Cherry-pick this command ID; repeat. Omit to apply the full reviewed batch.",
        ),
        Arg::value(
            "out",
            "<path>",
            "New .dsgrid path; required unless --dry-run.",
        ),
        Arg::switch("dry-run", "Evaluate the selection without writing."),
    ],
    output: COMMAND.output,
    examples: &[Example {
        command: "ds dsgrid apply-correction --model ./model.dsgrid --batch ./reviewed.json --guard ./scope.json --select comment-7 --dry-run --output json",
        note: "Preview one reviewed correction on the exact source package.",
        runnable: false,
    }],
    refusals: COMMAND.refusals,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["review comments", "spotted line", "cherry pick", "restring"],
    requires: Requires::Server,
    availability: available,
};

fn available() -> Availability {
    ds_network_client::available()
}

/// Read the bounded batch file and let the addon admit it as ordered typed
/// rows. The owner locates the first malformed row against its own command
/// schema; this host keeps the file bound and the exact-bytes digest. A
/// staged guard comes back in the form the apply-batch task admits and echoes.
fn read_batch(
    client: &ds_network_client::CapturedNetworkClient,
    raw_path: &str,
    guard: Option<&[u8]>,
) -> Result<(BatchParseResultV1, String), Failure> {
    let path = Path::new(raw_path);
    let file = std::fs::File::open(path).map_err(|error| {
        Failure::invalid("batch_not_found", format!("cannot open `{raw_path}`"))
            .remedy("--batch takes one readable batch JSON file")
            .detail(json!({"detail": error.kind().to_string()}))
    })?;
    if !file
        .metadata()
        .map_err(|error| {
            Failure::failed("batch_unreadable", error.to_string()).remedy("check file permissions")
        })?
        .is_file()
    {
        return Err(
            Failure::invalid("batch_not_found", "batch must be a regular file")
                .remedy("--batch takes one batch JSON file"),
        );
    }
    let mut bytes = Vec::new();
    file.take(MAX_BATCH_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            Failure::failed("batch_unreadable", error.to_string()).remedy("check file permissions")
        })?;
    if bytes.len() as u64 > MAX_BATCH_BYTES {
        return Err(Failure::invalid("batch_too_large", "batch exceeds 16 MiB")
            .remedy("submit at most 4096 commands within 16 MiB"));
    }
    let digest = sha256(&bytes);
    let mut staged = vec![("batch.json".to_owned(), bytes)];
    if let Some(guard) = guard {
        staged.push(("guard.json".to_owned(), guard.to_vec()));
    }
    let outcome =
        crate::native::operation_with(client, &GridOperationV1::BatchParse, None, &staged)?;
    let batch: BatchParseResultV1 = serde_json::from_value(outcome.result.result)
        .map_err(|error| Failure::failed("network_result_invalid", error.to_string()))?;
    if batch.commands.is_empty() || batch.commands.len() > MAX_COMMANDS {
        return Err(Failure::failed(
            "network_result_invalid",
            "the admitted batch violates its command bound",
        ));
    }
    if batch.guard.is_some() != guard.is_some() {
        return Err(Failure::failed(
            "network_result_invalid",
            "the admitted batch does not answer for exactly the staged guard",
        ));
    }
    Ok((batch, digest))
}

/// The guard file as reviewed bytes, their digest and the document. Its
/// scope, review state and decision are admitted by the Network owner
/// against the exact package; this host pins the bytes and the source digest
/// it names.
fn read_guard(raw_path: &str) -> Result<(Vec<u8>, String, Value), Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(raw_path)
        .map_err(|error| {
            Failure::invalid(
                "correction_guard_invalid",
                format!("cannot open guard: {error}"),
            )
        })?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            Failure::invalid(
                "correction_guard_invalid",
                format!("cannot read guard: {error}"),
            )
        })?;
    if bytes.len() > 64 * 1024 {
        return Err(Failure::invalid(
            "correction_guard_invalid",
            "guard exceeds 64 KiB",
        ));
    }
    let guard: Value = serde_json::from_slice(&bytes).map_err(|error| {
        Failure::invalid(
            "correction_guard_invalid",
            format!("invalid guard JSON: {error}"),
        )
    })?;
    if !guard["source_sha256"].as_str().is_some_and(|digest| {
        ds_cli_contract::util::is_sha256_digest(digest, ds_cli_contract::util::HexCase::Any)
    }) {
        return Err(Failure::invalid(
            "correction_guard_invalid",
            "source_sha256 must be a full SHA-256 digest",
        ));
    }
    let digest = sha256(&bytes);
    Ok((bytes, digest, guard))
}

fn select_commands(
    mut commands: Vec<NativeGridBatchCommandV1>,
    ids: &[String],
) -> Result<Vec<NativeGridBatchCommandV1>, Failure> {
    if ids.is_empty() {
        return Ok(commands);
    }
    let selected: HashSet<_> = ids.iter().map(String::as_str).collect();
    if selected.len() != ids.len() || selected.iter().any(|id| id.is_empty()) {
        return Err(Failure::invalid(
            "selection_invalid",
            "selected command IDs must be unique and nonempty",
        ));
    }
    if !selected
        .iter()
        .all(|id| commands.iter().any(|item| item.command_id == *id))
    {
        return Err(Failure::invalid(
            "selection_invalid",
            "a selected command ID is absent from the batch",
        ));
    }
    commands.retain(|item| selected.contains(item.command_id.as_str()));
    Ok(commands)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let model_path = inputs.require("model")?;
    let captured = crate::native::capture()?;
    // The guard file is read first so the owner admits it with the batch; a
    // batch refusal still answers before a guard refusal, as it always has.
    let guard_file = inputs.value("guard").map(read_guard);
    let staged_guard = match &guard_file {
        Some(Ok((bytes, _, _))) => Some(bytes.as_slice()),
        _ => None,
    };
    let (mut batch, batch_digest) = read_batch(&captured, inputs.require("batch")?, staged_guard)?;
    let selected_ids = inputs.repeated("select");
    let guard = guard_file.transpose()?;
    if !selected_ids.is_empty() && guard.is_none() {
        return Err(Failure::invalid(
            "selection_invalid",
            "--select requires --guard",
        ));
    }
    let commands = select_commands(batch.commands, selected_ids)?;
    let dry_run = inputs.switch("dry-run");
    let out_path = if dry_run {
        None
    } else {
        let out = inputs.value("out").ok_or_else(|| {
            Failure::invalid("output_required", "apply-batch needs a new output path")
                .remedy("pass --out <new.dsgrid>, or use --dry-run")
        })?;
        validate_output_path(out)?;
        Some(out)
    };
    let bytes = package::read_bytes(model_path)?;
    if let Some((_, _, guard)) = &guard {
        let expected = guard["source_sha256"].as_str().unwrap_or_default();
        if !expected.eq_ignore_ascii_case(&sha256(&bytes)) {
            return Err(Failure::conflict(
                "source_digest_conflict",
                "source package differs from the reviewed correction guard",
            )
            .detail(json!({"expected": expected, "actual": sha256(&bytes)})));
        }
    }
    let scratch = crate::native::scratch()?;
    let inspection = captured
        .grid_inspect(&bytes, scratch.path(), &|| false)
        .map_err(crate::native::map_failure)?;
    let request = NativeGridApplyBatchRequestV1 {
        schema: GRID_APPLY_BATCH_REQUEST_SCHEMA_V1.into(),
        source: inspection.result.source,
        expected_revision: batch.expected_revision,
        batch_sha256: batch_digest,
        commands,
        // The exact guard bytes' digest, with the document as the owner
        // admitted it: the task echoes that form and the client compares it.
        guard: guard
            .map(|(_, sha256, _)| {
                batch
                    .guard
                    .take()
                    .map(|document| NativeGridCorrectionGuardV1 { sha256, document })
                    .ok_or_else(|| {
                        Failure::failed(
                            "network_result_invalid",
                            "the admitted batch carries no guard",
                        )
                    })
            })
            .transpose()?,
        dry_run,
    };
    let output = captured
        .grid_apply_batch(&request, &bytes, scratch.path(), &|| false)
        .map_err(crate::native::map_failure)?;
    let mut receipt = output.metadata.result.projection;
    receipt["source"]["path"] = json!(model_path);
    if let Some(out_path) = out_path {
        let bytes = output.model.ok_or_else(|| {
            Failure::failed(
                "package_emit_failed",
                "Network returned no complete package",
            )
        })?;
        write_new(out_path, &bytes)?;
        receipt["persisted"] = json!(true);
        receipt["artifact"]["path"] = json!(out_path);
    }
    Ok(receipt)
}

pub fn run_correction(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    if inputs.value("guard").is_none() {
        return Err(Failure::invalid(
            "correction_guard_invalid",
            "apply-correction requires --guard",
        ));
    }
    run(inputs, context)
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub fn render(data: &Value) -> String {
    format!(
        "commands {}\nrevision {} -> {}\nwritten  {}\n",
        data["commands"],
        data["parent_revision"].as_str().unwrap_or("?"),
        data["resulting_revision"].as_str().unwrap_or("?"),
        data["artifact"]["path"].as_str().unwrap_or("no (dry run)")
    )
}

#[cfg(test)]
#[path = "apply_batch_tests.rs"]
mod tests;
