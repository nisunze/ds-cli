//! Materialize a visualization-only DS Grid package from a pinned spotting receipt.
//!
//! The normal digest-verified apply route remains unchanged. This adapter accepts
//! exact compressed receipt bytes, binds them to a source package/revision, and
//! lets the engine skip only digest recomputation for CLI-truncated rejected rows.
//! The isolated output is for visualization and review, not engineering approval.

use std::io::Read;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::{
    CommandEnvelope, GridCommand, GridSession, SpottingLayoutApplyRequest, SpottingPlan,
    spotting_layout_preview_application,
};
use ds_grid_exchange::{PackOptions, dsgrid};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{apply, package};

const MAX_RECEIPT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_EXPANDED_RECEIPT_BYTES: u64 = 256 * 1024 * 1024;
const PREVIEW_KIND: &str = "visualization_preview_from_truncated_diagnostics";

pub static COMMAND: Command = Command {
    id: "dsgrid.spotting.preview-receipt",
    path: &["dsgrid", "spotting", "preview-receipt"],
    contract: 1,
    summary: "Create an isolated visualization model from a pinned spotting receipt.",
    purpose: "\
Reads one exact-byte-pinned DS spotting receipt whose only truncation is in \
diagnostic rejected rows, verifies its source package identity and revision, \
and writes a new isolated .dsgrid visualization preview. The normal plan \
digest-verified apply path remains unchanged. This output is not engineering- \
proved or publishable.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<path>", "Exact base .dsgrid package.").required(),
        Arg::value(
            "receipt",
            "<path>",
            "Compressed DS spotting receipt (.json.zst).",
        )
        .required(),
        Arg::value(
            "receipt-sha256",
            "<hex>",
            "Expected SHA-256 of exact compressed receipt bytes.",
        )
        .required(),
        Arg::value(
            "out",
            "<new.dsgrid>",
            "New isolated visualization-preview package path.",
        )
        .required(),
        Arg::switch(
            "dry-run",
            "Run receipt, model and engine gates without writing output.",
        ),
    ],
    output: "\
A revision-gated local visualization preview package and receipt SHA-256. \
verification_level is visualization_preview_from_truncated_diagnostics; \
the artifact is not engineering-proved or publishable.",
    examples: &[
        Example {
            command: "ds dsgrid spotting preview-receipt --model ./base.dsgrid --receipt ./m1-A.json.zst --receipt-sha256 <manifest-sha256> --out ./m1-A-preview.dsgrid --dry-run --output json",
            note: "Prove the exact receipt and model gates before writing an isolated package.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid spotting preview-receipt --model ./base.dsgrid --receipt ./m1-A.json.zst --receipt-sha256 <manifest-sha256> --out ./m1-A-preview.dsgrid --output json",
            note: "Write the visualization-only scenario package for native projection and print.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "receipt_not_found",
            when: "receipt path does not name a regular file",
            remedy: "pass one compressed .json.zst receipt file",
        },
        Refusal {
            code: "receipt_too_large",
            when: "compressed or expanded receipt exceeds its read bound",
            remedy: "use one bounded spotting receipt",
        },
        Refusal {
            code: "receipt_unreadable",
            when: "receipt bytes cannot be read or decompressed",
            remedy: "check the path and preserve the original receipt",
        },
        Refusal {
            code: "receipt_digest_invalid",
            when: "expected digest is not a SHA-256 hex digest",
            remedy: "copy the raw receipt SHA-256 from its manifest",
        },
        Refusal {
            code: "receipt_digest_mismatch",
            when: "compressed receipt bytes do not match the supplied digest",
            remedy: "use the receipt whose bytes match the manifest digest",
        },
        Refusal {
            code: "receipt_invalid",
            when: "bytes are not a successful read-only whole-model spotting receipt",
            remedy: "use one successful dsgrid.run whole-model spotting receipt",
        },
        Refusal {
            code: "receipt_truncation_scope",
            when: "receipt has no CLI truncation or truncates beyond diagnostic rejected rows",
            remedy: "use a receipt whose only truncation is plan rejected.rows",
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
            code: "spotting_preview_refused",
            when: "native schema, status, revision or model-layout gate refuses",
            remedy: "read detail.engine and use only the receipt's exact base package",
        },
        Refusal {
            code: "output_required",
            when: "an output path is missing",
            remedy: "pass --out <new.dsgrid>",
        },
        Refusal {
            code: "output_exists",
            when: "output path already exists",
            remedy: "choose a new isolated preview path",
        },
        Refusal {
            code: "output_parent_missing",
            when: "output directory does not exist",
            remedy: "create the intended output directory, then retry",
        },
        Refusal {
            code: "package_emit_failed",
            when: "native package emitter refuses the isolated model",
            remedy: "report source model and receipt digests with engine detail",
        },
        Refusal {
            code: "preview_receipt_encode_failed",
            when: "the preview marker sidecar cannot be serialized",
            remedy: "report this serialization failure with the receipt digest",
        },
        Refusal {
            code: "output_unwritable",
            when: "isolated package cannot be written",
            remedy: "check output path permissions and disk space",
        },
    ],
    reference: Some("docs/reference/dsgrid.md"),
    search: &["truncated", "rejected rows", "scenario"],
    requires: Requires::Server,
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

#[derive(Deserialize)]
struct RunReceipt {
    v: u32,
    command: String,
    status: String,
    data: RunReceiptData,
}
#[derive(Deserialize)]
struct RunReceiptData {
    source: ReceiptSource,
    operation: ReceiptOperation,
    staged: bool,
    persisted: bool,
    result: ReceiptResult,
    #[serde(default)]
    more: ReceiptMore,
}
#[derive(Deserialize)]
struct ReceiptSource {
    model_id: String,
    package_revision: u64,
    authored_revision: String,
    package_sha256: String,
}
#[derive(Deserialize)]
struct ReceiptOperation {
    id: String,
}
#[derive(Deserialize)]
struct ReceiptResult {
    operation_id: String,
    model_revision: String,
    batch: ReceiptBatch,
}
#[derive(Deserialize)]
struct ReceiptBatch {
    operation_id: String,
    model_revision: String,
    requested_alignments: usize,
    completed_plans: usize,
    refused_requests: usize,
    items: Vec<ReceiptPlanItem>,
}
#[derive(Deserialize)]
struct ReceiptPlanItem {
    plan: SpottingPlan,
}
#[derive(Default, Deserialize)]
struct ReceiptMore {
    #[serde(default)]
    truncated: Vec<ReceiptTruncation>,
}
#[derive(Deserialize)]
struct ReceiptTruncation {
    field: String,
    total: usize,
    shown: usize,
    withheld: usize,
    limit: usize,
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let model_path = inputs.require("model")?;
    let receipt_path = inputs.require("receipt")?;
    let expected_receipt_sha = inputs.require("receipt-sha256")?;
    let out_path = inputs.require("out")?;
    let dry_run = inputs.switch("dry-run");

    let model_bytes = package::read_bytes(model_path)?;
    let model = package::decode(model_path, &model_bytes)?;
    apply::validate_output_path(out_path)?;

    let receipt_bytes = read_receipt_bytes(receipt_path)?;
    let receipt_sha = verify_receipt_sha256(&receipt_bytes, expected_receipt_sha)?;
    let receipt = decode_receipt(&receipt_bytes, receipt_path)?;
    validate_receipt_provenance(&receipt)?;

    let source_package_sha = format!("sha256:{}", sha256_hex(&model_bytes));
    if receipt.data.source.model_id != model.manifest.model.model_id.as_str()
        || receipt.data.source.package_revision != model.manifest.model.model_revision
        || receipt.data.source.package_sha256 != source_package_sha
    {
        return Err(Failure::conflict(
            "receipt_model_mismatch",
            "receipt does not name this exact model package",
        )
        .remedy("use the exact base package named by the receipt")
        .detail(json!({
            "receipt_model_id": receipt.data.source.model_id,
            "model_id": model.manifest.model.model_id.as_str(),
            "receipt_package_revision": receipt.data.source.package_revision,
            "package_revision": model.manifest.model.model_revision,
            "receipt_package_sha256": receipt.data.source.package_sha256,
            "package_sha256": source_package_sha,
        })));
    }

    let source_model_id = model.manifest.model.model_id.clone();
    let source_package_revision = model.manifest.model.model_revision;
    let coordinate_system = model.manifest.model.coordinate_system.clone();
    let assets = model.assets.clone();
    let exchange_bindings = model.exchange_bindings.clone();
    let mut session = GridSession::open(model.snapshot);
    let current_revision = session.current_revision().revision_id.clone();
    let batch = &receipt.data.result.batch;
    if receipt.data.source.authored_revision != current_revision.as_str()
        || receipt.data.result.model_revision != current_revision.as_str()
        || batch.model_revision != current_revision.as_str()
        || batch
            .items
            .iter()
            .any(|item| item.plan.model_revision != current_revision)
    {
        return Err(Failure::conflict("receipt_revision_mismatch", "receipt plans were authored against another model revision")
            .remedy("use the exact base package revision named by the receipt")
            .detail(json!({"receipt_revision": receipt.data.source.authored_revision, "model_revision": current_revision.as_str()})));
    }

    if batch.requested_alignments == 0
        || batch.completed_plans != batch.requested_alignments
        || batch.refused_requests != 0
        || batch.items.len() != batch.completed_plans
    {
        return Err(Failure::conflict(
            "receipt_incomplete",
            "whole-model batch is missing plans or contains refused alignments",
        )
        .remedy("use a complete successful whole-model proposal receipt")
        .detail(json!({
            "requested_alignments": batch.requested_alignments,
            "completed_plans": batch.completed_plans,
            "refused_requests": batch.refused_requests,
            "plan_items": batch.items.len(),
        })));
    }

    let request = SpottingLayoutApplyRequest {
        plans: batch.items.iter().map(|item| item.plan.clone()).collect(),
        fixed_structure_retypes: Default::default(),
        prior_warnings: Default::default(),
        warning_band_half_width_m: None,
    };
    let application = spotting_layout_preview_application(
        session.snapshot(),
        session.current_revision(),
        &request,
    )
    .map_err(|error| {
        Failure::failed(
            "spotting_preview_refused",
            "native engine refused the visualization preview",
        )
        .remedy("read detail.engine and use only the receipt's exact base package")
        .detail(json!({"engine": error.to_string()}))
    })?;

    let plan_digests: Vec<&str> = request
        .plans
        .iter()
        .map(|plan| plan.plan_digest.as_str())
        .collect();
    let proposed_structure_count = application.structures.len();
    let alignment_count = application.alignments.len();
    let application_digest = application.apply_digest.clone();
    let envelope = CommandEnvelope::new(
        format!("spotting-visualization-preview:{receipt_sha}"),
        current_revision.clone(),
        GridCommand::ApplySpottingLayouts { application },
    );
    if dry_run {
        let simulation = session
            .simulate_command(&envelope)
            .map_err(apply::map_command_error)?;
        return Ok(json!({
            "verification_level": PREVIEW_KIND,
            "review_notice": "visualization preview from truncated diagnostics; not engineering-proved or publishable",
            "dry_run": true,
            "model_package_written": false,
            "receipt": {"path": receipt_path, "sha256": receipt_sha},
            "source": {
                "path": model_path,
                "model_id": source_model_id.as_str(),
                "package_revision": source_package_revision,
                "authored_revision": current_revision.as_str(),
                "package_sha256": source_package_sha,
            },
            "proposal": {
                "operation_id": receipt.data.operation.id,
                "batch_operation_id": batch.operation_id,
                "alignment_count": alignment_count,
                "proposed_structure_count": proposed_structure_count,
                "plan_digests": plan_digests,
                "application_digest": application_digest,
                "truncated_diagnostic_collections": receipt.data.more.truncated.len(),
            },
            "would_apply": simulation.new_validation_issues.is_empty(),
            "resulting_revision": simulation.resulting_revision.revision_id.as_str(),
            "new_validation_issues": simulation.new_validation_issues,
            "output_path": out_path,
        }));
    }

    let sidecar_path = format!("{out_path}.preview.json");
    apply::validate_output_path(&sidecar_path)?;
    let outcome = session
        .apply_command(envelope)
        .map_err(apply::map_command_error)?;
    let checkpoint = session.checkpoint();
    let options = PackOptions {
        presentation: model.manifest.model.presentation.clone(),
        model_id: source_model_id.clone(),
        model_revision: source_package_revision + checkpoint.sequence,
        coordinate_system,
        library_pins: model.manifest.model.library_pins.clone(),
        library_needs: model.manifest.model.library_needs.clone(),
        assets,
        exchange_bindings,
    };
    let (package_plan, _report) =
        dsgrid::emit(&checkpoint.snapshot, &options).map_err(|error| {
            Failure::failed(
                "package_emit_failed",
                "visualization-preview model could not be packaged",
            )
            .remedy("report source model and receipt digests with engine detail")
            .detail(json!({"engine": error.to_string()}))
        })?;
    let artifact = package_plan.artifacts.first().ok_or_else(|| {
        Failure::failed(
            "package_emit_failed",
            "package emitter returned no preview artifact",
        )
        .remedy("report this package-emitter failure")
    })?;

    let sidecar = json!({
        "schema": "ds.grid-spotting-visualization-preview/v1",
        "verification_level": PREVIEW_KIND,
        "review_notice": "Visualization only. Rejected plan diagnostics were truncated in the source receipt; this is not engineering-proved or publishable.",
        "receipt": {"path": receipt_path, "sha256": receipt_sha},
        "source": {
            "path": model_path,
            "model_id": source_model_id.as_str(),
            "package_revision": source_package_revision,
            "authored_revision": current_revision.as_str(),
            "package_sha256": source_package_sha,
        },
        "result": {
            "path": out_path,
            "model_package_revision": options.model_revision,
            "byte_len": artifact.bytes.len(),
            "sha256": sha256_hex(&artifact.bytes),
            "parent_revision": outcome.parent_revision.as_str(),
            "resulting_revision": outcome.revision.revision_id.as_str(),
            "application_digest": application_digest,
            "alignment_count": alignment_count,
            "proposed_structure_count": proposed_structure_count,
            "plan_digests": plan_digests,
        },
        "proposal": {
            "operation_id": receipt.data.operation.id,
            "batch_operation_id": batch.operation_id,
            "truncated_diagnostic_collections": receipt.data.more.truncated.len(),
            "truncation_scope": "plan.rejected.rows only",
        },
    });
    let sidecar_bytes = serde_json::to_vec_pretty(&sidecar).map_err(|error| {
        Failure::failed(
            "preview_receipt_encode_failed",
            "preview marker receipt could not be encoded",
        )
        .remedy("report this serialization failure")
        .detail(json!({"detail": error.to_string()}))
    })?;
    apply::write_new(out_path, &artifact.bytes)?;
    if let Err(sidecar_error) = apply::write_new(&sidecar_path, &sidecar_bytes) {
        let cleanup = match std::fs::remove_file(out_path) {
            Ok(()) => json!({"preview_package": "removed"}),
            Err(error) => json!({"preview_package": "cleanup_failed", "detail": error.to_string()}),
        };
        return Err(sidecar_error.detail(cleanup));
    }

    Ok(json!({
        "verification_level": PREVIEW_KIND,
        "review_notice": "visualization preview from truncated diagnostics; not engineering-proved or publishable",
        "dry_run": false,
        "model_package_written": true,
        "source": {
            "path": model_path,
            "model_id": source_model_id.as_str(),
            "package_revision": source_package_revision,
            "authored_revision": current_revision.as_str(),
            "package_sha256": source_package_sha,
        },
        "receipt": {"path": receipt_path, "sha256": receipt_sha},
        "proposal": {
            "operation_id": receipt.data.operation.id,
            "batch_operation_id": batch.operation_id,
            "alignment_count": alignment_count,
            "proposed_structure_count": proposed_structure_count,
            "plan_digests": plan_digests,
            "application_digest": application_digest,
            "truncated_diagnostic_collections": receipt.data.more.truncated.len(),
        },
        "package": {
            "path": out_path,
            "sidecar_path": sidecar_path,
            "package_revision": options.model_revision,
            "byte_len": artifact.bytes.len(),
            "sha256": sha256_hex(&artifact.bytes),
            "parent_revision": outcome.parent_revision.as_str(),
            "resulting_revision": outcome.revision.revision_id.as_str(),
        },
    }))
}

pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string())
}

fn validate_receipt_provenance(receipt: &RunReceipt) -> Result<(), Failure> {
    if receipt.v != 1
        || receipt.command != "dsgrid.run"
        || receipt.status != "ok"
        || receipt.data.staged
        || receipt.data.persisted
        || receipt.data.operation.id != "plan_whole_model_spotting"
        || receipt.data.result.operation_id != "plan_whole_model_spotting"
        || receipt.data.result.batch.operation_id != "plan_optimum_spotting_batch"
    {
        return Err(Failure::invalid(
            "receipt_invalid",
            "receipt is not a successful read-only whole-model spotting proposal",
        )
        .remedy("use one successful dsgrid.run whole-model spotting receipt"));
    }
    if receipt.data.more.truncated.is_empty()
        || receipt.data.more.truncated.iter().any(|entry| {
            !entry.field.starts_with("result.batch.items[")
                || !entry.field.ends_with(".plan.rejected.rows")
                || entry.total <= entry.shown
                || entry.withheld != entry.total - entry.shown
                || entry.limit != entry.shown
        })
    {
        return Err(Failure::invalid("receipt_truncation_scope", "receipt must truncate diagnostic rejected rows only")
            .remedy("use a receipt whose only truncation is diagnostic plan rejected.rows")
            .detail(json!({
                "truncation_count": receipt.data.more.truncated.len(),
                "fields": receipt.data.more.truncated.iter().map(|entry| &entry.field).collect::<Vec<_>>(),
            })));
    }
    Ok(())
}

fn read_receipt_bytes(raw_path: &str) -> Result<Vec<u8>, Failure> {
    let path = Path::new(raw_path);
    let metadata = std::fs::metadata(path).map_err(|error| {
        Failure::invalid("receipt_not_found", format!("cannot read '{raw_path}'"))
            .remedy("pass one compressed .json.zst receipt file")
            .detail(json!({"detail": error.kind().to_string()}))
    })?;
    if !metadata.is_file() {
        return Err(
            Failure::invalid("receipt_not_found", format!("'{raw_path}' is not a file"))
                .remedy("pass one compressed .json.zst receipt file"),
        );
    }
    if metadata.len() > MAX_RECEIPT_BYTES {
        return Err(Failure::invalid(
            "receipt_too_large",
            "compressed receipt is above the read bound",
        )
        .remedy("use one bounded spotting receipt")
        .detail(json!({"byte_len": metadata.len(), "max_byte_len": MAX_RECEIPT_BYTES})));
    }
    std::fs::read(path).map_err(|error| {
        Failure::failed("receipt_unreadable", format!("cannot read '{raw_path}'"))
            .remedy("check the path and preserve the original receipt")
            .detail(json!({"detail": error.kind().to_string()}))
    })
}

fn decode_receipt(bytes: &[u8], path: &str) -> Result<RunReceipt, Failure> {
    let decoder = zstd::stream::read::Decoder::new(bytes).map_err(|error| {
        Failure::invalid(
            "receipt_invalid",
            format!("'{path}' is not a readable Zstandard receipt"),
        )
        .remedy("preserve the original compressed DS receipt")
        .detail(json!({"detail": error.to_string()}))
    })?;
    let mut limited = decoder.take(MAX_EXPANDED_RECEIPT_BYTES + 1);
    let mut expanded = Vec::new();
    limited.read_to_end(&mut expanded).map_err(|error| {
        Failure::invalid("receipt_invalid", "compressed receipt could not be decoded")
            .remedy("preserve the original compressed DS receipt")
            .detail(json!({"detail": error.to_string()}))
    })?;
    if expanded.len() as u64 > MAX_EXPANDED_RECEIPT_BYTES {
        return Err(Failure::invalid(
            "receipt_too_large",
            "expanded receipt is above the read bound",
        )
        .remedy("use one bounded spotting receipt")
        .detail(json!({"byte_len": expanded.len(), "max_byte_len": MAX_EXPANDED_RECEIPT_BYTES})));
    }
    serde_json::from_slice(&expanded).map_err(|error| {
        Failure::invalid("receipt_invalid", "expanded bytes are not a DS CLI receipt")
            .remedy("use one successful dsgrid.run whole-model spotting receipt")
            .detail(json!({"detail": error.to_string()}))
    })
}

fn normalize_sha256(raw: &str) -> Result<String, Failure> {
    let hex = raw.strip_prefix("sha256:").unwrap_or(raw);
    if !ds_cli_contract::util::is_sha256_hex(hex, ds_cli_contract::util::HexCase::Any) {
        return Err(Failure::invalid(
            "receipt_digest_invalid",
            "expected receipt digest is not SHA-256 hex",
        )
        .remedy("copy the raw receipt SHA-256 from its manifest"));
    }
    Ok(hex.to_ascii_lowercase())
}

fn verify_receipt_sha256(bytes: &[u8], expected: &str) -> Result<String, Failure> {
    let expected = normalize_sha256(expected)?;
    let actual = sha256_hex(bytes);
    if actual != expected {
        return Err(Failure::conflict(
            "receipt_digest_mismatch",
            "compressed receipt bytes do not match the expected SHA-256",
        )
        .remedy("use the receipt whose bytes match the manifest digest")
        .detail(json!({"expected": expected, "actual": actual})));
    }
    Ok(actual)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::verify_receipt_sha256;
    use sha2::{Digest, Sha256};

    #[test]
    fn verifies_exact_compressed_receipt_bytes_before_preview() {
        let compressed_receipt = b"exact zstd receipt bytes";
        let expected = Sha256::digest(compressed_receipt)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            verify_receipt_sha256(compressed_receipt, &expected).unwrap(),
            expected
        );
        assert_eq!(
            verify_receipt_sha256(compressed_receipt, &"0".repeat(64))
                .unwrap_err()
                .code(),
            "receipt_digest_mismatch"
        );
    }
}
