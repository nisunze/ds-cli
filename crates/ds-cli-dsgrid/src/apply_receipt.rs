//! Apply a complete whole-model spotting receipt as one engineering revision.
//!
//! `ds dsgrid run --operation plan_whole_model_spotting` returns every
//! alignment's digest-sealed plan in one receipt. Handing those plans back
//! through `--params` to `spotting_layout_application` bounds them at 16 MiB,
//! and a plan's digest covers its diagnostic rejected rows, so they cannot be
//! trimmed: a whole model planned with many reported rejections (a 233 MiB
//! receipt, 228 MiB of it diagnostics) had no apply path short of planning
//! again. This door reads the plans straight from the exact pinned receipt
//! file, binds them to the exact source package and revision, and lets the
//! engine verify every plan digest and build the one `apply_spotting_layouts`
//! revision, written to a new package. Nothing is re-planned, trimmed or
//! re-derived here; the receipt gate is shared with `preview-receipt`
//! ([`crate::spotting_receipt`]).

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::{
    CommandEnvelope, GridCommand, SpottingLayoutApplyError, SpottingLayoutApplyRequest,
    SpottingPlan, spotting_layout_application, spotting_layout_command_id,
};
use ds_grid_exchange::{PackOptions, dsgrid};
use serde_json::{Value, json};

use crate::spotting_receipt::{
    self, PinnedReceipt, ReceiptBounds, ReceiptEncoding, sha256_hex, verify_complete_batch,
    verify_revision, verify_source_package,
};
use crate::{apply, package};

/// A complete receipt carries every plan's diagnostic rows. Its bound is the
/// package read bound, on disk and once expanded.
const BOUNDS: ReceiptBounds = ReceiptBounds {
    file_bytes: package::MAX_PACKAGE_BYTES,
    expanded_bytes: package::MAX_PACKAGE_BYTES,
};
const VERIFICATION_LEVEL: &str = "digest_verified_plans";

pub static COMMAND: Command = Command {
    id: "dsgrid.spotting.apply-receipt",
    path: &["dsgrid", "spotting", "apply-receipt"],
    contract: 1,
    summary: "Apply a complete whole-model spotting receipt as one new revision.",
    purpose: "\
Lands the digest-sealed alignment plans of one exact-byte-pinned whole-model \
spotting receipt without planning again, however large its diagnostic \
rejected rows. Checks the receipt names this exact package and revision and \
holds every alignment's plan; the engine re-verifies each plan digest and \
builds one apply_spotting_layouts revision. Writes a new .dsgrid; the source \
is untouched. A receipt with withheld rows can only be previewed.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "model",
            "<path>",
            "Exact base .dsgrid package the receipt names.",
        )
        .required(),
        Arg::value(
            "receipt",
            "<path>",
            "The dsgrid run receipt, plain JSON or .json.zst.",
        )
        .required(),
        Arg::value(
            "receipt-sha256",
            "<hex>",
            "Expected SHA-256 of the exact receipt file bytes.",
        )
        .required(),
        Arg::value(
            "out",
            "<new.dsgrid>",
            "New package path; required unless --dry-run.",
        ),
        Arg::switch(
            "dry-run",
            "Run every receipt, plan and model gate; write nothing.",
        ),
    ],
    output: "\
Source identity and authored revision, receipt SHA-256, plan digests, \
application digest, alignment and proposed-structure counts, and the \
resulting revision with any new validation issues. A write adds the new \
package path, package revision, byte length and SHA-256.",
    examples: &[
        Example {
            command: "ds dsgrid spotting apply-receipt --model ./base.dsgrid --receipt ./whole-model.json --receipt-sha256 <sha256> --dry-run --output json",
            note: "Prove the receipt, every plan digest and the model gates without writing.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid spotting apply-receipt --model ./base.dsgrid --receipt ./whole-model.json --receipt-sha256 <sha256> --out ./spotted.dsgrid --output json",
            note: "Write the spotted layout as one new revision; the base package is untouched.",
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
            code: "package_decode_failed",
            when: "the source manifest or canonical tables do not verify",
            remedy: "run `ds dsgrid validate --model <path>` and repair the package",
        },
        package::PROTECTED_MODEL,
        Refusal {
            code: "receipt_not_found",
            when: "receipt path does not name a regular file",
            remedy: "pass one spotting receipt file",
        },
        Refusal {
            code: "receipt_too_large",
            when: "the receipt file or its expansion exceeds the 512 MiB bound",
            remedy: "use one bounded spotting receipt",
        },
        Refusal {
            code: "receipt_unreadable",
            when: "receipt bytes cannot be read",
            remedy: "check the path and preserve the original receipt",
        },
        Refusal {
            code: "receipt_digest_invalid",
            when: "expected digest is not a SHA-256 hex digest",
            remedy: "copy the raw receipt SHA-256 from its manifest",
        },
        Refusal {
            code: "receipt_digest_mismatch",
            when: "receipt file bytes do not match the supplied digest",
            remedy: "use the receipt whose bytes match the manifest digest",
        },
        Refusal {
            code: "receipt_invalid",
            when: "bytes are not a successful read-only whole-model spotting receipt",
            remedy: "use one successful dsgrid.run whole-model spotting receipt",
        },
        Refusal {
            code: "receipt_truncated",
            when: "the receipt withholds rows, so its sealed plans cannot be verified",
            remedy: "apply the complete receipt dsgrid run wrote, or preview this one with dsgrid spotting preview-receipt",
        },
        Refusal {
            code: "receipt_model_mismatch",
            when: "receipt model identity/package revision/SHA differs from the base package",
            remedy: "use the exact base package named by the receipt",
        },
        Refusal {
            code: "receipt_revision_mismatch",
            when: "plans were authored against another model revision",
            remedy: "use the exact base package revision named by the receipt",
        },
        Refusal {
            code: "receipt_incomplete",
            when: "batch has refused, missing or duplicate alignment plans",
            remedy: "use a complete successful whole-model proposal receipt",
        },
        Refusal {
            code: "spotting_application_refused",
            when: "the engine refuses a plan: edited digest, stale revision, schema, status or layout conflict",
            remedy: "read detail.refusal; re-plan only the alignments it names on this exact revision",
        },
        Refusal {
            code: "model_validation_failed",
            when: "the application would introduce new canonical model errors",
            remedy: "read detail.issues; no output was written",
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
            when: "the spotted snapshot cannot be packaged with its retained assets",
            remedy: "report the source model and receipt digests with detail.engine",
        },
        Refusal {
            code: "output_unwritable",
            when: "the new package cannot be written",
            remedy: "check the parent path and permissions; a partial file is removed",
        },
    ],
    reference: Some("docs/reference/dsgrid.md"),
    search: &["spotting receipt", "apply spotting", "whole-model spotting"],
    requires: Requires::Server,
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let model_path = inputs.require("model")?;
    let receipt_path = inputs.require("receipt")?;
    let expected_receipt_sha = inputs.require("receipt-sha256")?;
    let dry_run = inputs.switch("dry-run");
    let out_path = match inputs.value("out") {
        Some(path) => {
            apply::validate_output_path(path)?;
            Some(path)
        }
        None if dry_run => None,
        None => {
            return Err(
                Failure::invalid("output_required", "an apply needs a new output path")
                    .remedy("pass --out <new.dsgrid>, or use --dry-run"),
            );
        }
    };

    let model_bytes = package::read_bytes(model_path)?;
    let model = package::decode(model_path, &model_bytes)?;
    package::authorize_write(&model, "apply spotting receipt")?;

    let PinnedReceipt {
        sha256: receipt_sha,
        receipt,
    } = spotting_receipt::read_pinned(
        receipt_path,
        expected_receipt_sha,
        ReceiptEncoding::CompressedOrPlain,
        BOUNDS,
    )?;
    // A sealed plan is verified over every row it carries; one withheld row
    // leaves a plan no digest check can pass.
    let truncated = &receipt.data.more.truncated;
    if !truncated.is_empty() {
        return Err(Failure::invalid(
            "receipt_truncated",
            "the receipt withholds rows, so its sealed plans cannot be verified",
        )
        .remedy("apply the complete receipt dsgrid run wrote, or preview this one with dsgrid spotting preview-receipt")
        .detail(json!({
            "truncation_count": truncated.len(),
            "fields": truncated.iter().map(|entry| &entry.field).collect::<Vec<_>>(),
        })));
    }
    let source_package_sha = verify_source_package(&receipt, &model, &model_bytes)?;

    let source_model_id = model.manifest.model.model_id.clone();
    let source_package_revision = model.manifest.model.model_revision;
    let mut session = ds_grid_exchange::linked_models::open_session(&model);
    let current_revision = session.current_revision().revision_id.clone();
    verify_revision(&receipt, current_revision.as_str())?;
    verify_complete_batch(&receipt)?;

    // The plans move out of the receipt: a whole model's diagnostics are
    // read once, never copied.
    let receipt_operation_id = receipt.data.operation.id;
    let batch = receipt.data.result.batch;
    let batch_operation_id = batch.operation_id;
    let plans: Vec<SpottingPlan> = batch.items.into_iter().map(|item| item.plan).collect();
    let plan_digests: Vec<String> = plans.iter().map(|plan| plan.plan_digest.clone()).collect();
    let request = SpottingLayoutApplyRequest {
        plans,
        fixed_structure_retypes: Default::default(),
        prior_warnings: Default::default(),
        warning_band_half_width_m: None,
    };
    let application =
        spotting_layout_application(session.snapshot(), session.current_revision(), &request)
            .map_err(application_refused)?;
    let command_id = spotting_layout_command_id(&request);
    drop(request);
    let application_digest = application.apply_digest.clone();
    let alignment_count = application.alignments.len();
    let proposed_structure_count = application.structures.len();
    let envelope = CommandEnvelope::new(
        command_id.clone(),
        current_revision.clone(),
        GridCommand::ApplySpottingLayouts { application },
    );

    let source = json!({
        "path": model_path,
        "model_id": source_model_id.as_str(),
        "package_revision": source_package_revision,
        "authored_revision": current_revision.as_str(),
        "package_sha256": source_package_sha,
    });
    let proposal = json!({
        "operation_id": receipt_operation_id,
        "batch_operation_id": batch_operation_id,
        "alignment_count": alignment_count,
        "proposed_structure_count": proposed_structure_count,
        "plan_digests": plan_digests,
        "application_digest": application_digest,
        "command_id": command_id,
    });
    let receipt_ref = json!({"path": receipt_path, "sha256": receipt_sha});

    let Some(out_path) = out_path else {
        let simulation = session
            .simulate_command(&envelope)
            .map_err(apply::map_command_error)?;
        return Ok(json!({
            "verification_level": VERIFICATION_LEVEL,
            "dry_run": true,
            "persisted": false,
            "source": source,
            "receipt": receipt_ref,
            "proposal": proposal,
            "would_apply": simulation.new_validation_issues.is_empty(),
            "resulting_revision": simulation.resulting_revision.revision_id.as_str(),
            "new_validation_issues": simulation.new_validation_issues,
        }));
    };

    let outcome = session
        .apply_command(envelope)
        .map_err(apply::map_command_error)?;
    let checkpoint = session
        .save_checkpoint()
        .map_err(apply::map_command_error)?;
    let options = PackOptions {
        presentation: model.manifest.model.presentation.clone(),
        model_id: source_model_id.clone(),
        model_revision: source_package_revision + checkpoint.sequence,
        coordinate_system: model.manifest.model.coordinate_system.clone(),
        library_pins: model.manifest.model.library_pins.clone(),
        library_needs: model.manifest.model.library_needs.clone(),
        assets: model.assets.clone(),
        exchange_bindings: model.exchange_bindings.clone(),
    };
    let (package_plan, _report) =
        dsgrid::emit(&checkpoint.snapshot, &options).map_err(|error| {
            Failure::failed(
                "package_emit_failed",
                "the spotted package could not be emitted",
            )
            .remedy("report the source model and receipt digests with detail.engine")
            .detail(json!({"engine": error.to_string()}))
        })?;
    let artifact = package_plan.artifacts.first().ok_or_else(|| {
        Failure::failed(
            "package_emit_failed",
            "the package emitter returned no artifact",
        )
        .remedy("report this engine failure with the source model and receipt digests")
    })?;
    apply::write_new(out_path, &artifact.bytes)?;

    Ok(json!({
        "verification_level": VERIFICATION_LEVEL,
        "dry_run": false,
        "persisted": true,
        "source": source,
        "receipt": receipt_ref,
        "proposal": proposal,
        "idempotent_replay": outcome.idempotent_replay,
        "parent_revision": outcome.parent_revision.as_str(),
        "resulting_revision": outcome.revision.revision_id.as_str(),
        "artifact": {
            "path": out_path,
            "package_revision": options.model_revision,
            "byte_len": artifact.bytes.len(),
            "sha256": sha256_hex(&artifact.bytes),
        },
    }))
}

/// The engine's own refusal to land the plans: an edited plan digest, a stale
/// revision, an unsupported schema or status, or a layout conflict.
fn application_refused(error: SpottingLayoutApplyError) -> Failure {
    Failure::failed(
        "spotting_application_refused",
        "the engine refused to land these plans",
    )
    .remedy("read detail.refusal; re-plan only the alignments it names on this exact revision")
    .detail(json!({
        "engine": error.to_string(),
        "refusal": serde_json::to_value(&error).unwrap_or(Value::Null),
    }))
}

pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string())
}
