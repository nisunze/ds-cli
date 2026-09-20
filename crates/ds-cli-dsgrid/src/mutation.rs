//! The plumbing every typed `ds dsgrid <family> <verb>` mutation shares
//! (program contract 01 §2, §3, §5).
//!
//! A typed command is typed inputs plus a receipt; the engine decides
//! everything in between. What this module owns is the part that must be
//! identical across the family, so a caller who learned it from `structure
//! retype` has learned it for `alignment create`:
//!
//! * **Target selection.** `--model <local-id>` names one of this machine's
//!   working copies (`ds dsgrid model list`): the edit is applied through the
//!   engine's journaled session and becomes the copy's next revision, in
//!   place, under the same id. `--package <path> --out <path>` names an
//!   immutable `.dsgrid` file and writes a new one, exactly as `dsgrid apply`.
//!   One of the two, never both.
//! * **The revision pin.** `--revision <rev>` is the authored head the caller
//!   observed; a head that moved refuses `revision_conflict` and nothing is
//!   written. Omitted, the current head is the pin — and a working copy that
//!   another writer advanced between the read and the write still refuses
//!   `revision_conflict`, because the store checks the package digest under
//!   its lock.
//! * **Dry run or confirm.** `--dry-run` evaluates against the exact head and
//!   writes nothing; `--yes` writes. Neither is `confirmation_required`, both
//!   is `mode_conflict`. There is no third mode.
//! * **One receipt shape.** Engine operation ids and descriptor digests, the
//!   source revision and the resulting one, counts touched, warnings, the
//!   working copy's `pls_source` link (contract 02) and the native member
//!   families the change affects, named TYPE VERSION from that link
//!   (`DON 57`); null and empty for an unlinked copy or a package.
//!
//! Adding a typed command means: parse its inputs into `GridCommand`s, call
//! [`run`], and render. Nothing else.

use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::local_models::{LocalModel, Op};
use ds_grid_engine::descriptor::operation_descriptors;
use ds_grid_engine::{CommandError, GridCommand, GridSession, RevisionId, TransactionOutcome};
use ds_grid_exchange::package::GridPackage;
use ds_grid_exchange::{PackOptions, dsgrid};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::model::workspace;
use crate::package;

// ── Declared inputs, shared by every typed mutation ──────────────────────

pub const MODEL_ARG: Arg = Arg::value(
    "model",
    "<local-id>",
    "A working copy on this machine, by the id `ds dsgrid model list` reports; the edit becomes its next revision.",
);
pub const PACKAGE_ARG: Arg = Arg::value(
    "package",
    "<path>",
    "An immutable .dsgrid file instead of a working copy; needs --out.",
);
pub const OUT_ARG: Arg = Arg::value(
    "out",
    "<path>",
    "With --package: the new .dsgrid to write; never overwrites.",
);
pub const REVISION_ARG: Arg = Arg::value(
    "revision",
    "<rev>",
    "The authored head you observed (`rev:…`); a moved head refuses.",
);
pub const DRY_RUN_ARG: Arg = Arg::switch(
    "dry-run",
    "Evaluate against the exact head and write nothing.",
);
pub const YES_ARG: Arg = Arg::switch("yes", "Write the revision.");
pub const LANE_ARG: Arg = workspace::LANE_ARG;
pub const ACCOUNT_ARG: Arg = Arg::value(
    "account",
    "<uid>",
    "The DS account whose catalogue holds --model; omitted, the id is looked up across this machine's catalogues.",
);

/// The declared refusals every typed mutation shares, spliced into each
/// command's own list so the vocabulary is written once.
pub const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "target_required",
        when: "neither --model nor --package names a target, or both do",
        remedy: "pass exactly one of --model <local-id> or --package <path> --out <path>",
    },
    Refusal {
        code: "output_required",
        when: "--package is given without --out and this is not a dry run",
        remedy: "name a new .dsgrid file with --out, or pass --dry-run",
    },
    Refusal {
        code: "output_exists",
        when: "--out already exists",
        remedy: "choose a new path; a typed mutation never overwrites a package",
    },
    Refusal {
        code: "output_parent_missing",
        when: "the parent directory of --out does not exist",
        remedy: "create the intended directory, then retry",
    },
    Refusal {
        code: "confirmation_required",
        when: "neither --dry-run nor --yes was given",
        remedy: "run with --dry-run first; then repeat with --yes",
    },
    Refusal {
        code: "mode_conflict",
        when: "--dry-run and --yes were both given",
        remedy: "choose exactly one mode",
    },
    Refusal {
        code: "local_model_not_found",
        when: "no working copy on this machine carries --model",
        remedy: "run `ds dsgrid model list` and use an id from it",
    },
    Refusal {
        code: "local_model_ambiguous",
        when: "--model was found in more than one account's catalogue on this machine",
        remedy: "pass --account <uid> to say whose working copy you mean",
    },
    Refusal {
        code: "local_model_store_unavailable",
        when: "the machine's catalogue or the package beside it cannot be read or written",
        remedy: "check the local data directory; DS_LAYER_HOME may name an absolute shared directory",
    },
    Refusal {
        code: "model_not_found",
        when: "--package does not name a readable file",
        remedy: "check the path; --package takes one .dsgrid file",
    },
    Refusal {
        code: "not_a_dsgrid_package",
        when: "the target bytes are not a .dsgrid container this build opens",
        remedy: "convert the native source with `ds dsgrid-exchange convert` first",
    },
    Refusal {
        code: "package_decode_failed",
        when: "the package predates this build's canonical schema or does not verify",
        remedy: "re-convert it from its PLS-CADD workspace with `ds dsgrid-exchange convert`, then `ds dsgrid model import-external`",
    },
    Refusal {
        code: "revision_conflict",
        when: "--revision is not the target's authored head, or the working copy advanced between the read and the write",
        remedy: "re-read the head with `ds dsgrid model show` and decide again against it",
    },
    Refusal {
        code: "model_validation_failed",
        when: "the engine would introduce a canonical model error",
        remedy: "read detail.issues; nothing was written",
    },
    Refusal {
        code: "command_invalid",
        when: "the engine rejects a typed value",
        remedy: "read detail.engine and the live descriptor (`ds dsgrid describe --kind commands --id <op>`)",
    },
    Refusal {
        code: "dry_run_only",
        when: "the target is a promoted project head, which has no working copy to advance",
        remedy: "take a working copy first (`ds dsgrid project download`, then `ds dsgrid model import-external`) and edit that",
    },
    Refusal {
        code: "package_emit_failed",
        when: "the revised snapshot cannot be packaged with its retained assets",
        remedy: "report this engine failure with the model and command receipt",
    },
    Refusal {
        code: "output_unwritable",
        when: "the new package cannot be written",
        remedy: "check free space and permissions; a partial file is removed",
    },
];

// ── Mode ────────────────────────────────────────────────────────────────

/// `--dry-run` xor `--yes`, decided once for the whole family.
pub fn write_mode(inputs: &Inputs, context: &Context) -> Result<bool, Failure> {
    match (inputs.switch("dry-run"), context.confirmed) {
        (true, false) => Ok(false),
        (false, true) => Ok(true),
        (true, true) => Err(Failure::invalid(
            "mode_conflict",
            "--dry-run and --yes cannot be combined",
        )
        .remedy("choose exactly one mode")),
        (false, false) => Err(Failure::invalid(
            "confirmation_required",
            "choose a non-writing dry run or confirm the revision",
        )
        .remedy("run with --dry-run first; then repeat with --yes")),
    }
}

// ── Target ──────────────────────────────────────────────────────────────

/// What the mutation is applied to.
pub enum Target {
    /// One of this machine's working copies: revised in place.
    WorkingCopy {
        scope: ds_command_kernel::local_models::Scope,
        row: LocalModel,
        path: std::path::PathBuf,
    },
    /// An immutable package: a new file is written.
    Package { path: String, out: Option<String> },
}

impl Target {
    /// Resolve exactly one target from the declared inputs, before any bytes
    /// are read, so a refused invocation costs nothing.
    pub fn resolve(inputs: &Inputs, writing: bool) -> Result<Self, Failure> {
        let model = inputs
            .value("model")
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let package = inputs
            .value("package")
            .map(str::trim)
            .filter(|v| !v.is_empty());
        match (model, package) {
            (Some(id), None) => {
                let located = workspace::locate(inputs, id)?;
                Ok(Self::WorkingCopy {
                    scope: located.scope,
                    row: located.row,
                    path: located.path,
                })
            }
            (None, Some(path)) => {
                let out = inputs.value("out").map(str::to_owned);
                if writing {
                    let out = out.as_deref().ok_or_else(|| {
                        Failure::invalid("output_required", "a write to a package needs --out")
                            .remedy("pass --out <new.dsgrid>, or use --dry-run")
                    })?;
                    crate::apply::validate_output_path(out)?;
                }
                Ok(Self::Package {
                    path: path.to_owned(),
                    out,
                })
            }
            (Some(_), Some(_)) => Err(Failure::invalid(
                "target_required",
                "--model and --package name two targets; a mutation has one",
            )
            .remedy("pass exactly one of --model <local-id> or --package <path>")),
            (None, None) => Err(Failure::invalid(
                "target_required",
                "no target: pass --model <local-id> or --package <path>",
            )
            .remedy("`ds dsgrid model list` names this machine's working copies")
            .next("ds dsgrid model list")),
        }
    }

    fn describe(&self) -> Value {
        match self {
            Self::WorkingCopy { scope, row, path } => json!({
                "kind": "working_copy",
                "model": row.id,
                "name": row.display_name,
                "lane": scope.lane,
                "account": scope.uid,
                "path": path.display().to_string(),
                "package_revision": row.model_revision,
                "content_digest": row.sha256,
                "pls_source": row.pls_source.as_ref().and_then(|link| serde_json::to_value(link).ok()),
            }),
            Self::Package { path, out } => json!({
                "kind": "package",
                "path": path,
                "out": out,
            }),
        }
    }

    fn path(&self) -> String {
        match self {
            Self::WorkingCopy { path, .. } => path.display().to_string(),
            Self::Package { path, .. } => path.clone(),
        }
    }
}

/// The opened target: its decoded package and the engine session over it.
pub struct Opened {
    pub target: Target,
    pub bytes: Vec<u8>,
    pub package: GridPackage,
    pub session: GridSession,
    pub head: RevisionId,
}

/// Read the target once and open the engine on it. The revision pin is
/// checked here, before any command is built, so a stale caller learns the
/// head without paying for anything else.
pub fn open(target: Target, inputs: &Inputs) -> Result<Opened, Failure> {
    let path = target.path();
    let bytes = package::read_bytes(&path)?;
    let package = package::decode(&path, &bytes)?;
    let session = GridSession::open(package.snapshot.clone());
    let head = session.current_revision().revision_id.clone();
    if let Some(pinned) = inputs.value("revision").map(str::trim) {
        if pinned != head.as_str() {
            return Err(Failure::conflict(
                "revision_conflict",
                "the head moved since the revision you observed",
            )
            .remedy("re-read the head and decide again against it")
            .detail(json!({
                "expected_revision": pinned,
                "actual_revision": head.as_str(),
            })));
        }
    }
    Ok(Opened {
        target,
        bytes,
        package,
        session,
        head,
    })
}

// ── Applying ────────────────────────────────────────────────────────────

/// One typed command with the id it journals under.
pub struct Planned {
    pub command_id: String,
    pub command: GridCommand,
}

/// A caller-supplied note that travels into the receipt beside the engine's
/// own delta: what the command found before it acted.
pub type Warnings = Vec<Value>;

/// Apply (or simulate) the planned commands as ONE revision of the target and
/// return the family's receipt. `pls_families` names the native member
/// families (`DON`, `STRUCT`, `CRI`, …) the commands change when the working
/// copy is linked to a PLS-CADD workspace; the receipt resolves them to
/// `TYPE VERSION` from the link.
pub fn run(
    opened: Opened,
    planned: Vec<Planned>,
    writing: bool,
    extra: Value,
    warnings: Warnings,
    pls_families: &[&str],
) -> Result<Value, Failure> {
    let Opened {
        target,
        bytes,
        package,
        mut session,
        head,
    } = opened;
    let operations = operation_identities(&planned);
    let commands: Vec<(String, GridCommand)> = planned
        .into_iter()
        .map(|item| (item.command_id, item.command))
        .collect();
    let command_count = commands.len();

    let outcome: TransactionOutcome = if writing {
        session
            .apply_transaction_at_head(head.clone(), commands)
            .map_err(map_command_error)?
    } else {
        // A dry run is the same transaction on a throwaway session: the
        // engine's gates are exercised exactly, and the target is untouched.
        let mut probe = session.clone();
        probe
            .apply_transaction_at_head(head.clone(), commands)
            .map_err(map_command_error)?
    };

    let resulting = outcome.final_revision.revision_id.clone();
    let touched = touched_counts(&outcome);
    let (pls_source, pls_members_affected) = match &target {
        Target::WorkingCopy { row, .. } => match &row.pls_source {
            Some(link) => (
                serde_json::to_value(link).unwrap_or(Value::Null),
                pls_families
                    .iter()
                    .map(|family| match link.member_versions.get(*family) {
                        Some(version) => format!("{family} {version}"),
                        None => format!("{family} (version not recorded on the link)"),
                    })
                    .collect::<Vec<_>>(),
            ),
            None => (Value::Null, Vec::new()),
        },
        Target::Package { .. } => (Value::Null, Vec::new()),
    };
    let mut receipt = json!({
        "target": target.describe(),
        "dry_run": !writing,
        "persisted": false,
        "operations": operations,
        "commands": command_count,
        "source_revision": head.as_str(),
        "resulting_revision": resulting.as_str(),
        "changed": resulting != head,
        "idempotent_replay": outcome.idempotent_replay,
        "touched": touched,
        "deltas": outcome.outcomes.iter().map(|o| json!({
            "command_id": o.delta.command_id,
            "command_kind": o.delta.command_kind,
            "affected_entities": o.delta.affected_entities,
            "changed_tables": o.delta.changed_tables,
        })).collect::<Vec<_>>(),
        "warnings": warnings,
        "pls_source": pls_source,
        "pls_members_affected": pls_members_affected,
    });
    if let Value::Object(map) = extra {
        for (key, value) in map {
            receipt[key] = value;
        }
    }
    if !writing {
        return Ok(receipt);
    }

    // Persist: a new package for the immutable target, or the working copy's
    // next revision in place.
    let checkpoint = session.checkpoint();
    let source_package_revision = package.manifest.model.model_revision;
    let options = PackOptions {
        model_id: package.manifest.model.model_id.clone(),
        model_revision: source_package_revision + checkpoint.sequence,
        coordinate_system: package.manifest.model.coordinate_system.clone(),
        library_pins: Vec::new(),
        library_needs: Vec::new(),
        assets: package.assets.clone(),
        exchange_bindings: package.exchange_bindings.clone(),
    };
    let (plan, _report) = dsgrid::emit(&checkpoint.snapshot, &options).map_err(|error| {
        Failure::failed(
            "package_emit_failed",
            "the revised package could not be emitted",
        )
        .remedy("report this engine failure with the model and the command receipt")
        .detail(json!({ "engine": error.to_string() }))
    })?;
    let artifact = plan.artifacts.first().ok_or_else(|| {
        Failure::failed(
            "package_emit_failed",
            "the package emitter returned no artifact",
        )
        .remedy("report this engine failure")
    })?;
    let new_digest = format!("{:x}", Sha256::digest(&artifact.bytes));

    match target {
        Target::Package { out, .. } => {
            let out = out.expect("resolve() required --out for a writing package target");
            crate::apply::write_new(&out, &artifact.bytes)?;
            receipt["persisted"] = json!(true);
            receipt["artifact"] = json!({
                "path": out,
                "package_revision": options.model_revision,
                "byte_len": artifact.bytes.len(),
                "sha256": format!("sha256:{new_digest}"),
            });
        }
        Target::WorkingCopy { scope, row, path } => {
            let expected = format!("{:x}", Sha256::digest(&bytes));
            let op = Op::Revise {
                id: row.id.clone(),
                expected_sha256: Some(expected),
                model_revision: options.model_revision,
                bytes: artifact.bytes.len() as u64,
                sha256: new_digest.clone(),
                head_revision: Some(resulting.as_str().to_string()),
                revised_at: None,
            };
            let outcome =
                workspace::execute_in(&scope, op, Some(&artifact.bytes)).map_err(revise_refusal)?;
            let revised = outcome.model.as_ref().ok_or_else(|| {
                Failure::internal("local_model_store_unavailable", "nothing was revised")
            })?;
            receipt["persisted"] = json!(true);
            receipt["artifact"] = json!({
                "model": revised.id,
                "path": path.display().to_string(),
                "package_revision": revised.model_revision,
                "byte_len": revised.bytes,
                "sha256": format!("sha256:{}", revised.sha256),
                "head_revision": revised.head_revision,
                "revised_at": revised.revised_at,
            });
        }
    }
    Ok(receipt)
}

/// The engine descriptors the planned commands route through, each with the
/// SHA-256 of its published descriptor, so a receipt names exactly which
/// contract of the engine it exercised.
fn operation_identities(planned: &[Planned]) -> Vec<Value> {
    let descriptors = operation_descriptors();
    let mut seen: Vec<&str> = Vec::new();
    let mut identities = Vec::new();
    for item in planned {
        let kind = item.command.command_kind();
        if seen.contains(&kind) {
            continue;
        }
        seen.push(kind);
        let descriptor = descriptors
            .iter()
            .find(|descriptor| descriptor.operation_id == kind);
        let digest = descriptor
            .and_then(|descriptor| serde_json::to_vec(descriptor).ok())
            .map(|bytes| format!("sha256:{:x}", Sha256::digest(&bytes)));
        identities.push(json!({
            "operation_id": kind,
            "engine": ds_grid_engine::ENGINE_VERSION,
            "semantic_version": descriptor.map(|d| d.semantic_version),
            "descriptor_digest": digest,
            "journaled": descriptor.map(|d| d.journaled),
        }));
    }
    identities
}

/// Counts a reader can plan on: distinct entities and tables the transaction
/// changed, and how many commands actually moved something.
fn touched_counts(outcome: &TransactionOutcome) -> Value {
    let mut entities: Vec<&str> = Vec::new();
    let mut tables: Vec<String> = Vec::new();
    let mut effective = 0usize;
    for item in &outcome.outcomes {
        if !item.delta.changed_tables.is_empty() {
            effective += 1;
        }
        for entity in &item.delta.affected_entities {
            if !entities.contains(&entity.as_str()) {
                entities.push(entity.as_str());
            }
        }
        for table in &item.delta.changed_tables {
            let token = crate::package::table_token(*table);
            if !tables.contains(&token) {
                tables.push(token);
            }
        }
    }
    json!({
        "commands": outcome.outcomes.len(),
        "commands_with_effect": effective,
        "entities": entities.len(),
        "tables": tables,
    })
}

fn revise_refusal(failure: Failure) -> Failure {
    if failure.code() == ds_command_kernel::local_models::REVISION_CONFLICT {
        return Failure::conflict(
            "revision_conflict",
            "the working copy advanced between the read and the write",
        )
        .remedy("re-read the head with `ds dsgrid model show` and decide again against it")
        .detail(json!({ "store": failure.message() }));
    }
    failure
}

/// The engine's refusals under the family's declared names.
pub fn map_command_error(error: CommandError) -> Failure {
    match error {
        CommandError::StaleRevision { expected, actual } => Failure::conflict(
            "revision_conflict",
            "the command was built against a different revision",
        )
        .remedy("re-read the head and decide again against it")
        .detail(json!({
            "expected_revision": expected.as_str(),
            "actual_revision": actual.as_str(),
        })),
        CommandError::TargetNotFound { kind, id } => Failure::invalid(
            "target_not_found",
            format!("the command targets missing {kind} `{id}`"),
        )
        .remedy("use an id projected from this exact revision"),
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
        .remedy("revise the command; nothing was written")
        .detail(json!({ "issues": issues })),
        CommandError::PartialTransactionOverlap {
            already_applied,
            fresh,
        } => Failure::conflict(
            "command_replay_conflict",
            "the transaction only partially overlaps prior work",
        )
        .remedy("refresh the model and submit one fresh intent")
        .detail(json!({ "already_applied": already_applied, "fresh": fresh })),
    }
}

/// A journal id for one typed command: stable for the same intent against
/// the same head, so a replay is idempotent rather than a second edit.
pub fn command_id(verb: &str, head: &RevisionId, subject: &str) -> String {
    let digest = Sha256::digest(format!("{verb}\0{}\0{subject}", head.as_str()).as_bytes());
    format!("ds-{verb}-{:x}", digest)[..40].to_string()
}

/// The path of the target, for a receipt or a read-only report over the
/// same selection rules (no --out, no mode).
pub fn read_target_path(inputs: &Inputs) -> Result<(Value, String), Failure> {
    let target = Target::resolve(inputs, false)?;
    let path = target.path();
    Ok((target.describe(), path))
}

/// Whether a path exists as a file — for a read-only caller that wants the
/// family's `model_not_found` wording without opening the package twice.
pub fn is_file(path: &str) -> bool {
    Path::new(path).is_file()
}

pub fn render_receipt(data: &Value) -> String {
    let dry = data["dry_run"].as_bool().unwrap_or(false);
    let target = &data["target"];
    let mut out = format!(
        "{}  {}\nrevision {} -> {}{}\ntouched  {} command(s), {} entit{}\n",
        if dry { "dry run " } else { "applied " },
        target["model"]
            .as_str()
            .or_else(|| target["path"].as_str())
            .unwrap_or("?"),
        short(data["source_revision"].as_str().unwrap_or("?")),
        short(data["resulting_revision"].as_str().unwrap_or("?")),
        if data["changed"].as_bool().unwrap_or(false) {
            ""
        } else {
            " (no change)"
        },
        data["touched"]["commands_with_effect"],
        data["touched"]["entities"],
        if data["touched"]["entities"] == json!(1) {
            "y"
        } else {
            "ies"
        },
    );
    for warning in data["warnings"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "warning  {}\n",
            warning["message"]
                .as_str()
                .or_else(|| warning.as_str())
                .unwrap_or("?")
        ));
    }
    if data["persisted"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "written  {} (package rev {}, {})\n",
            data["artifact"]["path"].as_str().unwrap_or("?"),
            data["artifact"]["package_revision"],
            data["artifact"]["sha256"].as_str().unwrap_or("?"),
        ));
    } else if dry {
        out.push_str("written  no\n");
    }
    out
}

/// `rev:abcdef12…` for a human line; the JSON carries the whole id.
pub fn short(revision: &str) -> String {
    if revision.len() > 16 {
        format!("{}…", &revision[..16])
    } else {
        revision.to_string()
    }
}
