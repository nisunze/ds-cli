//! Atomic file-only application of typed engine commands, packed once.

use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
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
Batch input is limited to 16 MiB and 4096 commands. No live model is changed.",
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
    ],
    output: "Source identity and authored revision, batch digest, command count, counts \
by command kind, resulting authored revision and package revision increment. \
A successful write adds output path, package revision, byte length and SHA-256. \
Receipt size is independent of entity count; per-command deltas are not returned.",
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

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let model_path = inputs.require("model")?;
    let (batch, batch_digest) = read_batch(inputs.require("batch")?)?;
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
    let package = package::decode(model_path, &bytes)?;
    let mut session = GridSession::open(package.snapshot);
    let head = session.current_revision().revision_id.clone();
    let command_count = batch.commands.len();
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
    let mut receipt = json!({
        "source": {"path": model_path, "model_id": package.manifest.model.model_id.as_str(),
            "package_revision": package.manifest.model.model_revision,
            "authored_revision": head.as_str()},
        "batch_sha256": batch_digest,
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
    if let Some(out_path) = out_path {
        let options = PackOptions {
            presentation: package.manifest.model.presentation.clone(),
            model_id: package.manifest.model.model_id,
            model_revision: package.manifest.model.model_revision + checkpoint.sequence,
            coordinate_system: package.manifest.model.coordinate_system,
            library_pins: Vec::new(),
            library_needs: Vec::new(),
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
