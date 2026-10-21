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
//! The engine half of every verb is one closed Network operation
//! (`typed_mutation`): it decodes the exact package, checks the pin, plans
//! and applies the transaction and emits the package. Adding a typed command
//! means: declare its inputs, add its verb to the closed operation, call
//! [`run`], and render. Nothing else.

use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::local_models::{LocalModel, Op};
use ds_network_contracts::grid_operation::{
    GridOperationV1, TypedMutationParamsV1, TypedMutationResultV1, TypedMutationVerbV1,
};
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
/// command's own list so the vocabulary is written once: the family's own
/// codes and the captured addon's transport refusals.
pub const REFUSALS: &[Refusal] = &crate::native::extend_refusals::<
    { OWN_REFUSALS.len() + crate::native::REFUSALS.len() },
>(OWN_REFUSALS);

const OWN_REFUSALS: &[Refusal] = &[
    package::PLS_SECTION_IDENTITY_UNRESOLVED,
    package::PROTECTED_MODEL,
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
        when: "the package is damaged or carries a table schema this build does not decode",
        remedy: "re-convert it from its PLS-CADD workspace with `ds dsgrid-exchange convert`, then `ds dsgrid model import-external`",
    },
    Refusal {
        code: "revision_conflict",
        when: "--revision is not the target's authored head, or the working copy advanced between the read and the write",
        remedy: "re-read the head with `ds dsgrid model show` and decide again against it",
    },
    Refusal {
        code: "revision_invalid",
        when: "--revision is not a `rev:` token of 64 hex digits",
        remedy: "pass the token `ds dsgrid inspect --include authored-revision` prints",
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

/// Select a shared declaration by code so additions cannot shift a reader's
/// refusal vocabulary. An unknown code fails during command construction.
pub const fn refusal(code: &str) -> Refusal {
    let wanted = code.as_bytes();
    let mut index = 0;
    while index < REFUSALS.len() {
        let candidate = REFUSALS[index].code.as_bytes();
        if candidate.len() == wanted.len() {
            let mut byte = 0;
            while byte < wanted.len() && candidate[byte] == wanted[byte] {
                byte += 1;
            }
            if byte == wanted.len() {
                return REFUSALS[index];
            }
        }
        index += 1;
    }
    panic!("shared mutation refusal is not declared");
}

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
        row: Box<LocalModel>,
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
                    row: Box::new(located.row),
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

    pub(crate) fn path(&self) -> String {
        match self {
            Self::WorkingCopy { path, .. } => path.display().to_string(),
            Self::Package { path, .. } => path.clone(),
        }
    }
}

/// The target's narrow identity on a receipt whose verb planned nothing.
fn short_target(target: &Target) -> Value {
    match target {
        Target::WorkingCopy { row, .. } => json!({ "kind": "working_copy", "model": row.id }),
        Target::Package { path, .. } => json!({ "kind": "package", "path": path }),
    }
}

/// The working copy's PLS-CADD link and the native member families `families`
/// changes, named `TYPE VERSION` from that link; null and empty for an
/// unlinked copy or a package.
fn pls_link(target: &Target, families: &[String]) -> (Value, Vec<String>) {
    match target {
        Target::WorkingCopy { row, .. } => match &row.pls_source {
            Some(link) => (
                serde_json::to_value(link).unwrap_or(Value::Null),
                families
                    .iter()
                    .map(|family| match link.member_versions.get(family) {
                        Some(version) => format!("{family} {version}"),
                        None => format!("{family} (version not recorded on the link)"),
                    })
                    .collect(),
            ),
            None => (Value::Null, Vec::new()),
        },
        Target::Package { .. } => (Value::Null, Vec::new()),
    }
}

/// Evaluate (or apply) one typed edit through the addon as ONE revision of
/// the target and return the family's receipt. The addon decodes the exact
/// bytes, checks the revision pin, plans and applies the transaction and
/// emits the package; this host adds the target, mode and link facts and
/// performs the one write: a new package for an immutable target, or the
/// working copy's next revision under the store's digest fence.
pub fn run(
    target: Target,
    verb: TypedMutationVerbV1,
    inputs: &Inputs,
    writing: bool,
    staged: &[(String, Vec<u8>)],
) -> Result<Value, Failure> {
    let path = target.path();
    let bytes = package::read_bytes(&path)?;
    let call = GridOperationV1::TypedMutation(TypedMutationParamsV1 {
        verb,
        expected_revision: package::revision_pin(inputs.value("revision"))?,
        write: writing,
    });
    let mut outcome = crate::native::operation(&call, Some(&bytes), staged)
        .map_err(|failure| crate::native::at_path(failure, &path))
        .map_err(|failure| emit_refusal(failure, &target))?;
    let typed: TypedMutationResultV1 = serde_json::from_value(outcome.result.result.clone())
        .map_err(|error| Failure::failed("network_result_invalid", error.to_string()))?;
    let mut receipt = typed.receipt;
    receipt["dry_run"] = json!(!writing);
    receipt["persisted"] = json!(false);
    if !typed.planned {
        receipt["target"] = short_target(&target);
        return Ok(receipt);
    }
    let (pls_source, pls_members_affected) = pls_link(&target, &typed.pls_families);
    receipt["target"] = target.describe();
    receipt["pls_source"] = pls_source;
    receipt["pls_members_affected"] = json!(pls_members_affected);
    if !writing {
        return Ok(receipt);
    }
    if !typed.changed {
        // Nothing moved (an idempotent replay, or a value already held): a
        // revision that equals its parent is not written, so a repeated
        // command never advances the package revision for no change.
        receipt["warnings"]
            .as_array_mut()
            .ok_or_else(|| Failure::failed("network_result_invalid", "warnings is not an array"))?
            .push(json!({
                "code": "no_change",
                "message": "the target already holds this state; no revision was written",
            }));
        return Ok(receipt);
    }
    let artifact = outcome.take_artifact("model.dsgrid").ok_or_else(|| {
        Failure::failed(
            "package_emit_failed",
            "the package emitter returned no artifact",
        )
        .remedy("report this engine failure")
    })?;
    let package_revision = typed.package_revision.ok_or_else(|| {
        Failure::failed(
            "network_result_invalid",
            "an emitted package carries no package revision",
        )
    })?;
    let new_digest = format!("{:x}", Sha256::digest(&artifact));
    match target {
        Target::Package { out, .. } => {
            let out = out.expect("resolve() required --out for a writing package target");
            crate::apply::write_new(&out, &artifact)?;
            receipt["persisted"] = json!(true);
            receipt["artifact"] = json!({
                "path": out,
                "package_revision": package_revision,
                "byte_len": artifact.len(),
                "sha256": format!("sha256:{new_digest}"),
            });
        }
        Target::WorkingCopy { scope, row, path } => {
            let expected = format!("{:x}", Sha256::digest(&bytes));
            let op = Op::Revise {
                id: row.id.clone(),
                expected_sha256: Some(expected),
                model_revision: package_revision,
                bytes: artifact.len() as u64,
                sha256: new_digest.clone(),
                head_revision: Some(typed.resulting_revision.clone()),
                revised_at: None,
            };
            let outcome =
                workspace::execute_in(&scope, op, Some(&artifact)).map_err(revise_refusal)?;
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

/// An emit refusal over unresolved retype attachments carries the receipt it
/// would have written and the native families it touches; restore the host's
/// target, mode and link facts on that receipt.
fn emit_refusal(failure: Failure, target: &Target) -> Failure {
    if failure.code() != "package_emit_failed" {
        return failure;
    }
    let Some(mut detail) = failure.detail_value().cloned() else {
        return failure;
    };
    let families: Vec<String> = detail
        .as_object_mut()
        .and_then(|map| map.remove("pls_families"))
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    if detail["receipt"].is_object() {
        let (pls_source, pls_members_affected) = pls_link(target, &families);
        detail["receipt"]["target"] = target.describe();
        detail["receipt"]["dry_run"] = json!(false);
        detail["receipt"]["persisted"] = json!(false);
        detail["receipt"]["pls_source"] = pls_source;
        detail["receipt"]["pls_members_affected"] = json!(pls_members_affected);
    }
    failure.detail(detail)
}

fn revise_refusal(failure: Failure) -> Failure {
    if failure.code() == ds_command_kernel::local_models::REVISION_CONFLICT {
        return Failure::conflict(
            "revision_conflict",
            "the working copy advanced between the read and the write",
        )
        .remedy("re-read the head with `ds dsgrid model show` and decide again against it")
        .not_retryable()
        .detail(json!({ "store": failure.message() }));
    }
    failure
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
