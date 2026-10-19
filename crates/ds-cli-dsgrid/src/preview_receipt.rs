//! Materialize a visualization-only DS Grid package from a pinned spotting receipt.
//!
//! The digest-verified engineering route for a complete receipt is `dsgrid
//! spotting apply-receipt`. This adapter accepts exact compressed receipt
//! bytes, binds them to a source package/revision, and lets the engine skip
//! only digest recomputation for CLI-truncated rejected rows. The isolated
//! output is for visualization and review, not engineering approval. The
//! receipt gate both doors share is [`crate::spotting_receipt`].

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_network_contracts::grid_operation::{
    GridOperationV1, SpottingReceiptModeV1, SpottingReceiptParamsV1,
};
use serde_json::{Value, json};

use crate::spotting_receipt::{self, RECEIPT_INPUT, ReceiptBounds, ReceiptEncoding};
use crate::{apply, native, package};

/// A truncated receipt is small by construction: its rejected rows were cut.
const BOUNDS: ReceiptBounds = ReceiptBounds {
    file_bytes: 16 * 1024 * 1024,
    expanded_bytes: 256 * 1024 * 1024,
};
const PREVIEW_KIND: &str = "visualization_preview_from_truncated_diagnostics";

const OWN_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "receipt_not_found",
        when: "receipt path does not name a regular file",
        remedy: "pass one spotting receipt file",
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
        remedy: "use a receipt whose only truncation is plan rejected.rows; apply a complete one with dsgrid spotting apply-receipt",
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
];
const REFUSALS: &[Refusal] =
    &native::extend_refusals::<{ OWN_REFUSALS.len() + native::REFUSALS.len() }>(OWN_REFUSALS);

pub static COMMAND: Command = Command {
    id: "dsgrid.spotting.preview-receipt",
    path: &["dsgrid", "spotting", "preview-receipt"],
    contract: 1,
    summary: "Create an isolated visualization model from a pinned spotting receipt.",
    purpose: "\
Reads one exact-byte-pinned DS spotting receipt whose only truncation is in \
diagnostic rejected rows, verifies its source package identity and revision, \
and writes a new isolated .dsgrid visualization preview. A complete receipt \
lands its digest-verified plans through `ds dsgrid spotting apply-receipt`. \
This output is not engineering-proved or publishable.",
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
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["truncated", "rejected rows", "scenario"],
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
    let out_path = inputs.require("out")?;
    let dry_run = inputs.switch("dry-run");

    let model_bytes = package::read_bytes(model_path)?;
    apply::validate_output_path(out_path)?;
    let sidecar_path = format!("{out_path}.preview.json");
    if !dry_run {
        apply::validate_output_path(&sidecar_path)?;
    }
    let pinned = spotting_receipt::read_pinned(
        receipt_path,
        expected_receipt_sha,
        ReceiptEncoding::Compressed,
        BOUNDS,
    )?;
    let receipt_sha = pinned.sha256.clone();
    // The owner checks the truncation scope, the exact source package and
    // revision and the complete batch, then skips only digest recomputation
    // for the CLI-truncated rejected rows.
    let mut outcome = native::operation(
        &GridOperationV1::SpottingReceipt(SpottingReceiptParamsV1 {
            mode: SpottingReceiptModeV1::Preview,
            receipt_input: RECEIPT_INPUT.into(),
            receipt_sha256: pinned.sha256,
            write: !dry_run,
        }),
        Some(&model_bytes),
        &[(RECEIPT_INPUT.to_owned(), pinned.json)],
    )
    .map_err(|failure| native::at_path(failure, model_path))?;
    let mut result = std::mem::take(&mut outcome.result.result);
    result["source"]["path"] = json!(model_path);
    result["receipt"]["path"] = json!(receipt_path);
    if dry_run {
        result["output_path"] = json!(out_path);
        return Ok(result);
    }
    let bytes = outcome.take_artifact("model.dsgrid").ok_or_else(|| {
        Failure::failed(
            "package_emit_failed",
            "package emitter returned no preview artifact",
        )
        .remedy("report this package-emitter failure")
    })?;
    let package = &result["package"];
    let sidecar = json!({
        "schema": "ds.grid-spotting-visualization-preview/v1",
        "verification_level": PREVIEW_KIND,
        "review_notice": "Visualization only. Rejected plan diagnostics were truncated in the source receipt; this is not engineering-proved or publishable.",
        "receipt": {"path": receipt_path, "sha256": receipt_sha},
        "source": result["source"],
        "result": {
            "path": out_path,
            "model_package_revision": package["package_revision"],
            "byte_len": package["byte_len"],
            "sha256": package["sha256"],
            "parent_revision": package["parent_revision"],
            "resulting_revision": package["resulting_revision"],
            "application_digest": result["proposal"]["application_digest"],
            "alignment_count": result["proposal"]["alignment_count"],
            "proposed_structure_count": result["proposal"]["proposed_structure_count"],
            "plan_digests": result["proposal"]["plan_digests"],
        },
        "proposal": {
            "operation_id": result["proposal"]["operation_id"],
            "batch_operation_id": result["proposal"]["batch_operation_id"],
            "truncated_diagnostic_collections": result["proposal"]["truncated_diagnostic_collections"],
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
    apply::write_new(out_path, &bytes)?;
    if let Err(sidecar_error) = apply::write_new(&sidecar_path, &sidecar_bytes) {
        let cleanup = match std::fs::remove_file(out_path) {
            Ok(()) => json!({"preview_package": "removed"}),
            Err(error) => json!({"preview_package": "cleanup_failed", "detail": error.to_string()}),
        };
        return Err(sidecar_error.detail(cleanup));
    }
    result["package"]["path"] = json!(out_path);
    result["package"]["sidecar_path"] = json!(sidecar_path);
    Ok(result)
}

pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string())
}
