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

/// The two batch shapes, told apart by the JSON value itself rather than by
/// trying each in turn: an untagged enum answered every malformed row with
/// "data did not match any variant of untagged enum BatchInput" and never
/// said which row, which command or which field.
enum BatchInput {
    AtHead(Batch),
    Envelopes(Vec<CommandEnvelope>),
}

/// The at-head object's own fields, with each row left unparsed so a row's
/// failure can be reported against its own index.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchHead {
    #[allow(dead_code)]
    expected_revision: RevisionId,
    commands: Vec<Value>,
}

fn parse_batch(bytes: &[u8]) -> Result<BatchInput, Failure> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| invalid(format!("invalid batch JSON: {error}")))?;
    match &value {
        Value::Object(_) => Batch::deserialize(&value)
            .map(BatchInput::AtHead)
            .map_err(|error| match BatchHead::deserialize(&value) {
                Err(head) => invalid(format!("batch: {head}")),
                Ok(head) => first_bad_row("commands", &head.commands, |row| {
                    BatchCommand::deserialize(row).err()
                })
                .unwrap_or_else(|| invalid(format!("invalid batch: {error}"))),
            }),
        Value::Array(rows) => Vec::<CommandEnvelope>::deserialize(&value)
            .map(BatchInput::Envelopes)
            .map_err(|error| {
                first_bad_row("", rows, |row| CommandEnvelope::deserialize(row).err())
                    .unwrap_or_else(|| invalid(format!("invalid batch: {error}")))
            }),
        _ => Err(invalid(
            "a batch is an object {expected_revision, commands: [{command_id, command}]} \
             or an array of command envelopes",
        )),
    }
}

/// The first row that does not deserialize, refused with its index, its
/// command kind, the place in the command serde's error applies to, and the
/// row type whose exact JSON Schema `ds dsgrid describe` publishes.
fn first_bad_row(
    array: &str,
    rows: &[Value],
    row_error: impl Fn(&Value) -> Option<serde_json::Error>,
) -> Option<Failure> {
    rows.iter().enumerate().find_map(|(index, row)| {
        let error = row_error(row)?;
        Some(row_failure(
            &format!("{array}[{index}]"),
            index,
            row,
            &error,
        ))
    })
}

fn row_failure(at: &str, index: usize, row: &Value, row_error: &serde_json::Error) -> Failure {
    let command = row.get("command");
    let kind = command
        .and_then(|command| command.get("command_kind"))
        .and_then(Value::as_str);
    let command_error = command.and_then(|command| GridCommand::deserialize(command).err());
    let mut detail = json!({
        "row": index,
        "command_id": row.get("command_id"),
        "command_kind": kind,
    });
    // The row's own fields are sound and the command is not: the error lives
    // inside the command, where serde has lost its place.
    let Some((command, command_error)) = command.zip(command_error) else {
        let label = kind.map(|kind| format!(" ({kind})")).unwrap_or_default();
        detail["serde_error"] = json!(row_error.to_string());
        return Failure::invalid("batch_invalid", format!("row {index}{label}: {row_error}"))
            .remedy(
                "each row is {command_id, command, review_ref?}; an envelope row also carries \
             command_schema_version and expected_revision",
            )
            .detail(detail);
    };
    let serde_error = command_error.to_string();
    let catalog = ds_grid_engine::describe_commands();
    let known: Vec<&str> = catalog
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["operation_id"].as_str())
        .collect();
    // An unknown tag makes serde list every command kind; name the one given
    // and the nearest real one instead. A known kind whose nested enum holds
    // an unknown variant is located like any other field.
    if let Some(kind) = kind
        && !known.contains(&kind)
    {
        let mut failure = Failure::invalid(
            "batch_invalid",
            format!("row {index}: unknown command_kind `{kind}`"),
        );
        failure = match ds_cli_contract::args::nearest(kind, known.iter().copied()) {
            Some(suggestion) => failure.remedy(format!("did you mean `{suggestion}`?")),
            None => failure.remedy("name a command_kind the engine publishes"),
        };
        detail["path"] = json!(format!("{at}.command.command_kind"));
        return failure
            .next("ds dsgrid describe --kind commands")
            .detail(detail);
    }
    let label = kind.map(|kind| format!(" ({kind})")).unwrap_or_default();
    let located = crate::command_shape::locate(command, &serde_error);
    let place = located
        .as_ref()
        .filter(|located| !located.path.is_empty())
        .map(|located| format!("{}: ", located.path))
        .unwrap_or_default();
    let message = format!("row {index}{label}: {place}{serde_error}");
    let command_path = located
        .as_ref()
        .filter(|located| !located.path.is_empty())
        .map(|located| format!(".{}", located.path))
        .unwrap_or_default();
    detail["path"] = json!(format!("{at}.command{command_path}"));
    detail["serde_error"] = json!(serde_error);
    // Point at the row type's published JSON Schema when the engine publishes
    // it; a place directly on the command points at the command's descriptor.
    let published = located
        .and_then(|located| located.type_name)
        .filter(|name| {
            ds_grid_engine::describe_types()
                .as_array()
                .is_some_and(|types| types.iter().any(|entry| entry["id"] == name.as_str()))
        });
    let pointer = match &published {
        Some(name) => {
            detail["type"] = json!(name);
            format!("ds dsgrid describe --kind types --id {name}")
        }
        None => match kind {
            Some(kind) => format!("ds dsgrid describe --kind commands --id {kind}"),
            None => "ds dsgrid describe --kind commands".to_owned(),
        },
    };
    Failure::invalid("batch_invalid", message)
        .remedy(format!(
            "correct that field; `{pointer}` gives the exact shape the engine accepts"
        ))
        .next(pointer)
        .detail(detail)
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
    let batch = match parse_batch(&bytes)? {
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

    fn refused(batch: Value) -> Failure {
        match parse_batch(&serde_json::to_vec(&batch).unwrap()) {
            Ok(_) => panic!("the malformed batch parsed: {batch}"),
            Err(failure) => failure,
        }
    }

    fn policy_row() -> Value {
        json!({
            "id": "policy-1",
            "label": "EDCL MV",
            "release_identity": "edcl-mv-2026",
            "content_digest": "sha256:0",
            "applicability": "33 kV",
            "verification": "unverified",
        })
    }

    fn duty_profile() -> Value {
        json!({
            "policy_id": "policy-1",
            "sequence": 0,
            "structure_resource_id": "resource-1",
            "family": "SP",
            "material": "wood",
            "portal_configuration": false,
            "duties": ["inline_suspension"],
            "preferred_span_upper_m": null,
            "review_span_upper_m": null,
            "verification": "unverified",
            "evidence": null,
        })
    }

    fn move_node(z_m: Value) -> Value {
        json!({"command_kind": "move_route_node", "id": "node-1", "x_m": 1.0, "y_m": 2.0, "z_m": z_m})
    }

    #[test]
    fn a_well_formed_batch_of_either_shape_still_parses() {
        let policy = json!({
            "command_kind": "author_design_policy",
            "row": policy_row(),
            "duty_profiles": [duty_profile()],
        });
        let at_head = json!({
            "expected_revision": "rev-1",
            "commands": [
                {"command_id": "c-0", "command": move_node(json!(3.0))},
                {"command_id": "c-1", "command": policy, "review_ref": "comment-7"},
            ],
        });
        let Ok(BatchInput::AtHead(batch)) = parse_batch(&serde_json::to_vec(&at_head).unwrap())
        else {
            panic!("the at-head batch must parse");
        };
        assert_eq!(batch.commands.len(), 2);
        let envelopes = json!([{
            "command_id": "c-0",
            "command_schema_version": ds_grid_engine::COMMAND_SCHEMA_VERSION,
            "expected_revision": "rev-1",
            "command": move_node(json!(3.0)),
        }]);
        assert!(matches!(
            parse_batch(&serde_json::to_vec(&envelopes).unwrap()),
            Ok(BatchInput::Envelopes(rows)) if rows.len() == 1
        ));
    }

    /// The 2026-09-25 incident: a duty profile without `verification` was
    /// refused as "data did not match any variant of untagged enum
    /// BatchInput". It is now refused at its row, command and field.
    #[test]
    fn a_design_policy_missing_a_duty_profile_field_names_row_field_and_type() {
        let mut profile = duty_profile();
        profile.as_object_mut().unwrap().remove("verification");
        let failure = refused(json!({
            "expected_revision": "rev-1",
            "commands": [
                {"command_id": "c-0", "command": move_node(json!(3.0))},
                {"command_id": "c-1", "command": move_node(json!(4.0))},
                {"command_id": "c-2", "command": move_node(json!(5.0))},
                {"command_id": "policy", "command": {
                    "command_kind": "author_design_policy",
                    "row": policy_row(),
                    "duty_profiles": [profile],
                }},
            ],
        }));
        assert_eq!(failure.code(), "batch_invalid");
        assert_eq!(
            failure.message(),
            "row 3 (author_design_policy): duty_profiles[0]: missing field `verification`"
        );
        assert_eq!(
            failure.next_commands(),
            ["ds dsgrid describe --kind types --id StructureDutyProfileRow"]
        );
        let detail = failure.detail_value().unwrap();
        assert_eq!(detail["row"], 3);
        assert_eq!(detail["command_id"], "policy");
        assert_eq!(detail["path"], "commands[3].command.duty_profiles[0]");
        assert_eq!(detail["type"], "StructureDutyProfileRow");
        assert_eq!(detail["serde_error"], "missing field `verification`");

        // The policy row carries a `verification` too; its absence is named
        // at the row, not at the profile.
        let mut row = policy_row();
        row.as_object_mut().unwrap().remove("verification");
        let failure = refused(json!({
            "expected_revision": "rev-1",
            "commands": [{"command_id": "policy", "command": {
                "command_kind": "author_design_policy",
                "row": row,
                "duty_profiles": [duty_profile()],
            }}],
        }));
        assert_eq!(
            failure.message(),
            "row 0 (author_design_policy): row: missing field `verification`"
        );
        assert_eq!(
            failure.next_commands(),
            ["ds dsgrid describe --kind types --id DesignPolicyRow"]
        );
    }

    #[test]
    fn a_wrong_field_on_another_command_is_named_where_it_is() {
        // A misspelled required field: the command itself lacks it.
        let mut command = move_node(json!(3.0));
        let x = command.as_object_mut().unwrap().remove("x_m").unwrap();
        command["x"] = x;
        let failure = refused(json!({
            "expected_revision": "rev-1",
            "commands": [{"command_id": "c-0", "command": command}],
        }));
        assert_eq!(
            failure.message(),
            "row 0 (move_route_node): missing field `x_m`"
        );
        assert_eq!(
            failure.next_commands(),
            ["ds dsgrid describe --kind commands --id move_route_node"]
        );

        // A wrong type, in the envelope-array shape.
        let failure = refused(json!([
            {"command_id": "c-0", "command_schema_version": 10, "expected_revision": "rev-1",
             "command": move_node(json!(3.0))},
            {"command_id": "c-1", "command_schema_version": 10, "expected_revision": "rev-1",
             "command": move_node(json!("12"))},
        ]));
        assert_eq!(
            failure.message(),
            "row 1 (move_route_node): z_m: invalid type: string \"12\", expected f64"
        );
        assert_eq!(failure.detail_value().unwrap()["path"], "[1].command.z_m");

        // An unknown field inside a type that denies them.
        let failure = refused(json!({
            "expected_revision": "rev-1",
            "commands": [{"command_id": "c-0", "command": {
                "command_kind": "set_spotting_settings",
                "settings": {"design_policy_id": "policy-1", "station_step": 10.0},
            }}],
        }));
        assert!(
            failure.message().starts_with(
                "row 0 (set_spotting_settings): settings: unknown field `station_step`"
            ),
            "{}",
            failure.message()
        );
        assert_eq!(
            failure.next_commands(),
            ["ds dsgrid describe --kind types --id SpottingSettings"]
        );

        // A nested enum value, not the command kind, that the engine lacks.
        let mut profile = duty_profile();
        profile["material"] = json!("bamboo");
        let failure = refused(json!({
            "expected_revision": "rev-1",
            "commands": [{"command_id": "policy", "command": {
                "command_kind": "author_design_policy",
                "row": policy_row(),
                "duty_profiles": [profile],
            }}],
        }));
        assert!(
            failure
                .message()
                .starts_with("row 0 (author_design_policy): duty_profiles[0].material: "),
            "{}",
            failure.message()
        );
    }

    #[test]
    fn row_and_batch_level_mistakes_are_named_too() {
        let failure = refused(json!({
            "expected_revision": "rev-1",
            "commands": [{"command_id": "c-0", "command": move_node(json!(3.0)), "reviewref": "x"}],
        }));
        assert!(
            failure
                .message()
                .starts_with("row 0 (move_route_node): unknown field `reviewref`"),
            "{}",
            failure.message()
        );

        let failure = refused(json!({
            "expected_revision": "rev-1",
            "commands": [{"command_id": "c-0", "command": {"command_kind": "move_route_nod"}}],
        }));
        assert_eq!(
            failure.message(),
            "row 0: unknown command_kind `move_route_nod`"
        );
        assert_eq!(
            failure.remedy_text(),
            Some("did you mean `move_route_node`?")
        );

        let failure = refused(json!({
            "expected_revision": "rev-1",
            "commands": [{"command_id": "c-0", "command": {"id": "node-1"}}],
        }));
        assert_eq!(failure.message(), "row 0: missing field `command_kind`");

        let failure = refused(json!({"expected_revision": "rev-1", "command": []}));
        assert!(
            failure
                .message()
                .starts_with("batch: unknown field `command`"),
            "{}",
            failure.message()
        );
        assert_eq!(refused(json!("x")).code(), "batch_invalid");
    }
}
