//! Atomic file-only application of typed engine commands, packed once.

use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::correction_scope::CorrectionScope;
use ds_grid_engine::{CommandEnvelope, GridCommand, GridSession, RevisionId};
use ds_grid_exchange::{PackOptions, dsgrid};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    apply::{guard_admin_refresh, map_command_error, validate_output_path, write_new},
    package,
};

const MAX_BATCH_BYTES: u64 = 16 * 1024 * 1024;
const MAX_COMMANDS: usize = 4096;

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
    refusals: &[
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
            when: "the JSON has invalid commands, mixed revision pins, duplicate IDs, or no commands",
            remedy: "read the command descriptor with `ds dsgrid describe --kind commands --id <id>`",
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
            when: "a command or its resulting model change reaches outside selected alignments or its permitted kinds",
            remedy: "split the correction or explicitly include every affected alignment after review",
        },
        Refusal {
            code: "admin_authority_required",
            when: "a batch carries Rwanda admin facts that have not been resolved from the exact local village index",
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
            remedy: "report this engine failure with the model and batch digests",
        },
        Refusal {
            code: "output_unwritable",
            when: "the new output file cannot be created or fully written",
            remedy: "check the parent path and permissions; a partial file is removed",
        },
    ],
    reference: Some("docs/reference/dsgrid.md"),
    search: &[],
    requires: Requires::Server,
    availability: available,
};

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
    Availability::Available
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    expected_revision: RevisionId,
    commands: Vec<BatchCommand>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchCommand {
    command_id: String,
    command: GridCommand,
    #[serde(default)]
    #[serde(alias = "comment_ref")]
    review_ref: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorrectionGuardFile {
    source_sha256: String,
    #[serde(default)]
    review_state: CorrectionReviewState,
    #[serde(default)]
    decision_ref: Option<String>,
    #[serde(default)]
    preservation_source_ref: Option<String>,
    #[serde(flatten)]
    scope: CorrectionScope,
}

#[derive(Clone, Copy, Default, serde::Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CorrectionReviewState {
    #[default]
    Draft,
    Approved,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BatchInput {
    AtHead(Batch),
    Envelopes(Vec<CommandEnvelope>),
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("batch_invalid", message).remedy(
        "supply ordered typed commands with unique IDs and one expected_revision from this package",
    )
}

fn read_batch(raw_path: &str) -> Result<(Batch, String), Failure> {
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
    let input: BatchInput = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("invalid batch JSON: {error}")))?;
    let batch = match input {
        BatchInput::AtHead(batch) => batch,
        BatchInput::Envelopes(envelopes) => {
            let expected_revision = envelopes
                .first()
                .ok_or_else(|| invalid("batch cannot be empty"))?
                .expected_revision
                .clone();
            if envelopes
                .iter()
                .any(|e| e.expected_revision != expected_revision)
            {
                return Err(invalid(
                    "all envelope revision pins must equal the one expected source head",
                ));
            }
            Batch {
                expected_revision,
                commands: envelopes
                    .into_iter()
                    .map(|e| BatchCommand {
                        command_id: e.command_id,
                        command: e.command,
                        review_ref: None,
                    })
                    .collect(),
            }
        }
    };
    if batch.commands.is_empty() {
        return Err(invalid("batch cannot be empty"));
    }
    if batch.commands.len() > MAX_COMMANDS {
        return Err(
            Failure::invalid("batch_too_large", "batch exceeds 4096 commands")
                .remedy("submit at most 4096 commands within 16 MiB"),
        );
    }
    let mut ids = HashSet::new();
    for item in &batch.commands {
        guard_admin_refresh(&item.command)?;
        if item.command_id.trim().is_empty() || !ids.insert(&item.command_id) {
            return Err(invalid(
                "command IDs must be nonempty and unique within the batch",
            ));
        }
    }
    Ok((batch, sha256(&bytes)))
}

fn read_guard(raw_path: &str) -> Result<(CorrectionGuardFile, String), Failure> {
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
    let guard: CorrectionGuardFile = serde_json::from_slice(&bytes).map_err(|error| {
        Failure::invalid(
            "correction_guard_invalid",
            format!("invalid guard JSON: {error}"),
        )
    })?;
    if !guard.source_sha256.starts_with("sha256:")
        || guard.source_sha256.len() != 71
        || !guard.source_sha256[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Failure::invalid(
            "correction_guard_invalid",
            "source_sha256 must be a full SHA-256 digest",
        ));
    }
    Ok((guard, sha256(&bytes)))
}

fn select_commands(mut batch: Batch, ids: &[String]) -> Result<Batch, Failure> {
    if ids.is_empty() {
        return Ok(batch);
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
        .all(|id| batch.commands.iter().any(|item| item.command_id == *id))
    {
        return Err(Failure::invalid(
            "selection_invalid",
            "a selected command ID is absent from the batch",
        ));
    }
    batch
        .commands
        .retain(|item| selected.contains(item.command_id.as_str()));
    Ok(batch)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let model_path = inputs.require("model")?;
    let (batch, batch_digest) = read_batch(inputs.require("batch")?)?;
    let selected_ids = inputs.repeated("select");
    let guard = inputs.value("guard").map(read_guard).transpose()?;
    if !selected_ids.is_empty() && guard.is_none() {
        return Err(Failure::invalid(
            "selection_invalid",
            "--select requires --guard",
        ));
    }
    let batch = select_commands(batch, selected_ids)?;
    let dry_run = inputs.switch("dry-run");
    if let Some((guard, _)) = &guard {
        let needs_preservation_source = guard.scope.level
            == ds_grid_engine::correction_scope::CorrectionLevel::AngleInterval
            || !guard.scope.preserved_structure_ids.is_empty()
            || !guard.scope.preserved_route_node_ids.is_empty()
            || !guard.scope.preserved_section_ids.is_empty();
        if needs_preservation_source
            && guard
                .preservation_source_ref
                .as_ref()
                .is_none_or(|value| value.trim().is_empty() || value.len() > 512)
        {
            return Err(Failure::invalid(
                "correction_guard_invalid",
                "fixed anchors or preserved assets need an authored preservation_source_ref",
            ));
        }
        if !dry_run
            && (!matches!(guard.review_state, CorrectionReviewState::Approved)
                || guard
                    .decision_ref
                    .as_ref()
                    .is_none_or(|value| value.trim().is_empty() || value.len() > 512))
        {
            return Err(Failure::invalid(
                "correction_not_approved",
                "writing a correction requires review_state approved and an authored decision_ref",
            ));
        }
    }
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
    if let Some((guard, _)) = &guard
        && !guard.source_sha256.eq_ignore_ascii_case(&sha256(&bytes))
    {
        return Err(Failure::conflict(
            "source_digest_conflict",
            "source package differs from the reviewed correction guard",
        )
        .detail(json!({"expected": guard.source_sha256, "actual": sha256(&bytes)})));
    }
    let package = package::decode(model_path, &bytes)?;
    let before = package.snapshot.clone();
    if let Some((guard, _)) = &guard {
        for item in &batch.commands {
            if item
                .review_ref
                .as_ref()
                .is_none_or(|value| value.trim().is_empty() || value.len() > 256)
            {
                return Err(Failure::invalid(
                    "correction_guard_invalid",
                    format!(
                        "{} needs a nonempty review_ref of at most 256 bytes",
                        item.command_id
                    ),
                ));
            }
        }
        guard
            .scope
            .validate(&before)
            .map_err(|error| Failure::invalid("correction_guard_invalid", error))?;
        let mut preview = before.clone();
        for item in &batch.commands {
            guard
                .scope
                .validate_command(&preview, &item.command)
                .map_err(|error| {
                    Failure::invalid(
                        "correction_scope_violation",
                        format!("{}: {error}", item.command_id),
                    )
                })?;
            item.command
                .apply_to(&mut preview)
                .map_err(map_command_error)?;
        }
    }
    let mut session = GridSession::open(package.snapshot);
    let head = session.current_revision().revision_id.clone();
    let command_count = batch.commands.len();
    let applied_command_ids: Vec<_> = batch
        .commands
        .iter()
        .map(|item| item.command_id.clone())
        .collect();
    let selected_reviews: Vec<_> = batch
        .commands
        .iter()
        .map(|item| {
            json!({
                "command_id": item.command_id, "review_ref": item.review_ref,
                "command_kind": item.command.command_kind()
            })
        })
        .collect();
    let mut operations: BTreeMap<String, usize> = BTreeMap::new();
    for item in &batch.commands {
        *operations
            .entry(item.command.command_kind().to_string())
            .or_default() += 1;
    }
    // This session is private to the file operation: dry-run and write use
    // exactly the same atomic engine gates, and neither touches the source.
    let outcome = session
        .apply_transaction_at_head(
            batch.expected_revision,
            batch
                .commands
                .into_iter()
                .map(|item| (item.command_id, item.command))
                .collect(),
        )
        .map_err(map_command_error)?;
    let checkpoint = session.checkpoint();
    if let Some((guard, _)) = &guard {
        guard
            .scope
            .validate_result(&before, &checkpoint.snapshot)
            .map_err(|error| Failure::invalid("correction_scope_violation", error))?;
    }
    let mut receipt = json!({
        "source": {"path": model_path, "model_id": package.manifest.model.model_id.as_str(),
            "package_revision": package.manifest.model.model_revision,
            "authored_revision": head.as_str()},
        "batch_sha256": batch_digest,
        "source_sha256": sha256(&bytes),
        "selected_command_ids": applied_command_ids,
        "commands": command_count,
        "operations": operations,
        "dry_run": dry_run,
        "persisted": false,
        "would_apply": true,
        "idempotent_replay": outcome.idempotent_replay,
        "parent_revision": head.as_str(),
        "resulting_revision": outcome.final_revision.revision_id.as_str(),
        "package_revision_increment": checkpoint.sequence,
    });
    if let Some((guard, digest)) = &guard {
        receipt["correction_guard"] = json!({"sha256": digest,
            "alignment_ids": guard.scope.alignment_ids,
            "allowed_command_kinds": guard.scope.allowed_command_kinds,
            "level": guard.scope.level,
            "intervals": guard.scope.intervals,
            "preserved_structure_ids": guard.scope.preserved_structure_ids,
            "preserved_route_node_ids": guard.scope.preserved_route_node_ids,
            "preserved_section_ids": guard.scope.preserved_section_ids,
            "preservation_source_ref": guard.preservation_source_ref,
            "review_state": guard.review_state,
            "decision_ref": guard.decision_ref});
        receipt["selected_reviews"] = json!(selected_reviews);
        let diff = ds_grid_engine::correction_scope::correction_diff(&before, &checkpoint.snapshot);
        receipt["changes"] = json!({"tables": diff.changed_tables.iter().map(|kind| kind.name()).collect::<Vec<_>>(),
            "added": diff.added_total, "removed": diff.removed_total, "changed": diff.changed_total});
    }
    if let Some(out_path) = out_path {
        let options = PackOptions {
            presentation: package.manifest.model.presentation.clone(),
            model_id: package.manifest.model.model_id,
            model_revision: package.manifest.model.model_revision + checkpoint.sequence,
            coordinate_system: package.manifest.model.coordinate_system,
            library_pins: package.manifest.model.library_pins,
            library_needs: package.manifest.model.library_needs,
            assets: package.assets,
            exchange_bindings: package.exchange_bindings,
        };
        let (plan, _) = dsgrid::emit(&checkpoint.snapshot, &options).map_err(|error| {
            Failure::failed(
                "package_emit_failed",
                "the revised package could not be emitted",
            )
            .remedy("report this engine failure with the source model and batch digest")
            .detail(json!({"engine": error.to_string()}))
        })?;
        let artifact = plan.artifacts.first().ok_or_else(|| {
            Failure::failed(
                "package_emit_failed",
                "package emitter returned no artifact",
            )
            .remedy("report this engine failure")
        })?;
        write_new(out_path, &artifact.bytes)?;
        receipt["persisted"] = json!(true);
        receipt["artifact"] = json!({"path": out_path, "package_revision": options.model_revision,
            "byte_len": artifact.bytes.len(), "sha256": sha256(&artifact.bytes)});
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
mod tests {
    use super::*;
    use ds_grid_model::AlignmentId;

    #[test]
    fn correction_guard_reads_exact_scope_and_rejects_unknown_fields() {
        let raw = json!({
            "source_sha256": format!("sha256:{}", "a".repeat(64)),
            "alignment_ids": ["alignment-1"],
            "allowed_command_kinds": ["retype_structure"]
        });
        let guard: CorrectionGuardFile = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(
            guard.scope.alignment_ids,
            vec![AlignmentId::new("alignment-1").unwrap()]
        );
        let mut extra = raw;
        extra["allow_unscoped"] = json!(true);
        assert!(serde_json::from_value::<CorrectionGuardFile>(extra).is_err());
    }

    #[test]
    fn cherry_pick_preserves_batch_order_and_refuses_unknown_ids() {
        let command = GridCommand::DescribeStructure {
            id: ds_grid_model::StructureId::new("structure-1").unwrap(),
            description: Some("reviewed".into()),
        };
        let make = || Batch {
            expected_revision: RevisionId::from_content_root("revision-1"),
            commands: ["first", "second", "third"]
                .into_iter()
                .map(|id| BatchCommand {
                    command_id: id.into(),
                    command: command.clone(),
                    review_ref: Some(format!("comment:{id}")),
                })
                .collect(),
        };
        let selected = select_commands(make(), &["third".into(), "first".into()]).unwrap();
        assert_eq!(
            selected
                .commands
                .iter()
                .map(|row| row.command_id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "third"]
        );
        assert!(select_commands(make(), &["missing".into()]).is_err());
        assert!(select_commands(make(), &["first".into(), "first".into()]).is_err());
    }
}
