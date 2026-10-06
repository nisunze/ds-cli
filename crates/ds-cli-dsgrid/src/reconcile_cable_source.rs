//! Immutable cable reconciliation through the native exchange owner only.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::CommandError;
use ds_grid_exchange::cable_source_reconcile::{
    CableSourceReconcileError, CableSourceReconcileRequest, reconcile_cable_source_package,
};
use ds_grid_model::CableId;
use serde_json::{Value, json};

use crate::{apply, package};

pub static COMMAND: Command = Command {
    id: "dsgrid.reconcile-cable-source",
    path: &["dsgrid", "reconcile-cable-source"],
    contract: 1,
    summary: "Reconcile one cable's mechanics from its exact retained .wir source.",
    purpose: "Reconcile one local .dsgrid cable only from its already-linked exact retained resource, fenced by authored revision and source digest. The native owner proves the mechanics and validates a complete new immutable package even on dry-run. Source bytes, link identity and retained history are preserved. Generic apply still severs source authority. No cloud save, project version bump or artifact upload occurs.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "model",
            "<path>",
            "One local source .dsgrid package (at most 512 MiB).",
        )
        .required(),
        Arg::value(
            "cable-id",
            "<id>",
            "Exact stable cable identity in this package.",
        )
        .required(),
        Arg::value(
            "revision",
            "<revision>",
            "Expected authored revision, not the package revision number.",
        )
        .required(),
        Arg::value(
            "expect-source-digest",
            "<sha256:digest>",
            "Expected digest of the already-linked retained .wir resource.",
        )
        .required(),
        Arg::value(
            "out",
            "<path>",
            "New .dsgrid path; required unless --dry-run.",
        ),
        Arg::switch(
            "dry-run",
            "Validate the complete candidate and write nothing; --out is ignored.",
        ),
        Arg::value(
            "limit",
            "<n>",
            "Maximum entries per receipt collection, 1..5000.",
        )
        .default("50"),
    ],
    output: "Native source/result package SHA-256, retained source digest, cable/resource identities, authored revisions, candidate package revision and differences. dry_run/persisted identify the effect; a write adds the exclusive artifact path and byte length. more.truncated names every shortened collection, including authored/source arrays, with exact counts.",
    examples: &[
        Example {
            command: "ds dsgrid reconcile-cable-source --model ./model.dsgrid --cable-id cb-70 --revision rev:<digest> --expect-source-digest sha256:<digest> --dry-run --output json",
            note: "Validate the exact retained-source reconciliation without writing.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid reconcile-cable-source --model ./model.dsgrid --cable-id cb-70 --revision rev:<digest> --expect-source-digest sha256:<digest> --out ./reconciled.dsgrid --output json",
            note: "Write one validated new package, preserving the source file.",
            runnable: false,
        },
    ],
    refusals: &[
        package::PROTECTED_MODEL,
        Refusal {
            code: "model_not_found",
            when: "the source is absent or is not a regular file",
            remedy: "pass one local .dsgrid file",
        },
        Refusal {
            code: "model_too_large",
            when: "the source exceeds 512 MiB",
            remedy: "use a bounded .dsgrid package",
        },
        Refusal {
            code: "model_unreadable",
            when: "the source cannot be read",
            remedy: "check file permissions",
        },
        Refusal {
            code: "request_invalid",
            when: "the cable identity is invalid",
            remedy: "use an exact cable id from this package",
        },
        Refusal {
            code: "invalid_limit",
            when: "--limit is outside 1..5000",
            remedy: "pass 1..5000; the default is 50",
        },
        Refusal {
            code: "revision_conflict",
            when: "the expected authored revision is stale",
            remedy: "inspect this exact package and deliberately refresh the revision fence",
        },
        Refusal {
            code: "cable_not_found",
            when: "the selected cable is absent",
            remedy: "select a cable id from this exact package",
        },
        Refusal {
            code: "source_unavailable",
            when: "the cable has no usable exact retained source",
            remedy: "read detail.blocker; restore the linked retained resource, never a same-named substitute",
        },
        Refusal {
            code: "source_digest_mismatch",
            when: "the linked source differs from the expected digest",
            remedy: "inspect the retained resource and deliberately refresh the source digest fence",
        },
        Refusal {
            code: "source_too_large",
            when: "the retained wire exceeds the native 1 MiB bound",
            remedy: "use a bounded characterized native wire resource",
        },
        Refusal {
            code: "source_mapping_incomplete",
            when: "the native source mechanics cannot be completely mapped",
            remedy: "read detail.engine; retain the current package and report the unsupported source",
        },
        Refusal {
            code: "generated_fields_mismatch",
            when: "the native generated mechanics fail the source proof",
            remedy: "report the native failure with the package and source digests",
        },
        Refusal {
            code: "mechanics_unchanged",
            when: "the authored mechanics already agree with the retained source",
            remedy: "keep the existing package",
        },
        Refusal {
            code: "revision_overflow",
            when: "the package revision cannot advance",
            remedy: "keep the existing package and report the exhausted revision",
        },
        Refusal {
            code: "cable_reconcile_refused",
            when: "the engine rejects the reconciled mechanics",
            remedy: "read detail.engine and validate the model; no output was written",
        },
        Refusal {
            code: "package_reconcile_failed",
            when: "the native package cannot be verified or emitted",
            remedy: "validate this package and report detail.engine",
        },
        Refusal {
            code: "output_required",
            when: "a write omits --out",
            remedy: "pass a new .dsgrid path or --dry-run",
        },
        Refusal {
            code: "output_exists",
            when: "the output already exists",
            remedy: "choose a new path; source and earlier results are never overwritten",
        },
        Refusal {
            code: "output_parent_missing",
            when: "the output parent does not exist",
            remedy: "create the intended output directory",
        },
        Refusal {
            code: "output_unwritable",
            when: "the output cannot be created and fully synchronized",
            remedy: "check permissions and free space; partial output is removed",
        },
    ],
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "cable mechanics",
        "wire",
        "retained source",
        "source authority",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let model = inputs.require("model")?;
    let dry_run = inputs.switch("dry-run");
    let limit = package::parse_limit(inputs.value("limit"))?;
    let request = CableSourceReconcileRequest {
        cable_id: CableId::new(inputs.require("cable-id")?).map_err(|error| {
            Failure::invalid("request_invalid", "invalid cable reconciliation identity")
                .remedy("use an exact cable id from this package")
                .detail(json!({ "engine": error.to_string() }))
        })?,
        expected_revision: serde_json::from_value(json!(inputs.require("revision")?)).map_err(
            |error| {
                Failure::invalid("request_invalid", "invalid authored revision")
                    .remedy("use the exact authored revision of this package")
                    .detail(json!({ "engine": error.to_string() }))
            },
        )?,
        expected_source_digest: inputs.require("expect-source-digest")?.to_owned(),
    };
    let out = if dry_run {
        None
    } else {
        let out = inputs.value("out").ok_or_else(|| {
            Failure::invalid("output_required", "reconciliation needs a new output path")
                .remedy("pass --out <new.dsgrid>, or use --dry-run")
        })?;
        apply::validate_output_path(out)?;
        Some(out)
    };
    let bytes = package::read_bytes(model)?;
    package::authorize_write(&package::decode(model, &bytes)?, "reconcile cable source")?;
    let result = reconcile_cable_source_package(&bytes, &request).map_err(owner_failure)?;
    let mut receipt =
        serde_json::to_value(&result).expect("native reconciliation receipt serializes");
    // The native outcome omits package bytes from its serialized receipt.
    let mut truncated = Vec::new();
    crate::run::bound_value(
        &mut receipt["differences"],
        "differences",
        limit,
        &mut truncated,
    );
    if !truncated.is_empty() {
        receipt["more"] = json!({ "truncated": truncated });
    }
    receipt["source_path"] = json!(model);
    receipt["dry_run"] = json!(dry_run);
    receipt["persisted"] = json!(false);
    if let Some(out) = out {
        apply::write_new(out, &result.bytes)?;
        receipt["persisted"] = json!(true);
        receipt["artifact"] = json!({
            "path": out, "byte_len": result.bytes.len(),
            "sha256": result.resulting_package_digest,
            "package_revision": result.package_revision,
        });
    }
    Ok(receipt)
}

fn owner_failure(error: CableSourceReconcileError) -> Failure {
    use CableSourceReconcileError as E;
    let message = error.to_string();
    match error {
        E::PackageTooLarge => Failure::invalid("model_too_large", message)
            .remedy("use a .dsgrid package within the native 512 MiB bound"),
        E::Package(error) => Failure::failed(
            "package_reconcile_failed",
            "native package verification or emission failed",
        )
        .remedy("validate this package and report detail.engine")
        .detail(json!({ "engine": error.to_string() })),
        E::Command(CommandError::StaleRevision { expected, actual }) => {
            Failure::conflict("revision_conflict", message)
                .remedy("inspect this package and deliberately refresh the authored revision fence")
                .detail(json!({ "expected_revision": expected, "actual_revision": actual }))
        }
        E::Command(error) => Failure::failed(
            "cable_reconcile_refused",
            "the native engine refused reconciliation",
        )
        .remedy("read detail.engine and validate the model; no output was written")
        .detail(json!({ "engine": error.to_string() })),
        E::CableNotFound { cable_id } => Failure::invalid("cable_not_found", message)
            .remedy("select a cable id from this exact package")
            .detail(json!({ "cable_id": cable_id })),
        E::SourceUnavailable { blocker } => Failure::invalid(
            "source_unavailable",
            "the exact retained cable source is unavailable",
        )
        .remedy("read detail.blocker; restore the linked retained resource")
        .detail(json!({ "blocker": blocker })),
        E::SourceDigestMismatch { expected, actual } => Failure::conflict(
            "source_digest_mismatch",
            message,
        )
        .remedy("inspect the retained resource and deliberately refresh the source digest fence")
        .detail(json!({ "expected_source_digest": expected, "actual_source_digest": actual })),
        E::SourceTooLarge => Failure::invalid("source_too_large", message)
            .remedy("use a retained wire within the native 1 MiB source bound"),
        E::IncompleteMapping { detail } => Failure::invalid(
            "source_mapping_incomplete",
            "the source mechanics cannot be completely mapped",
        )
        .remedy("retain the current package and report the unsupported source")
        .detail(json!({ "engine": detail })),
        E::GeneratedFieldsMismatch => Failure::failed("generated_fields_mismatch", message)
            .remedy("report the native failure with the package and source digests"),
        E::Unchanged => {
            Failure::invalid("mechanics_unchanged", message).remedy("keep the existing package")
        }
        E::RevisionOverflow => Failure::invalid("revision_overflow", message)
            .remedy("keep the existing package and report the exhausted revision"),
    }
}

pub fn render(data: &Value) -> String {
    let mut text = format!(
        "{} cable {} from resource {}\nrevision {} -> {}\ndifferences shown {}\nwritten {}\n",
        if data["dry_run"] == true {
            "dry run:"
        } else {
            "reconciled"
        },
        data["cable_id"].as_str().unwrap_or("?"),
        data["resource_id"].as_str().unwrap_or("?"),
        data["source_revision"].as_str().unwrap_or("?"),
        data["resulting_revision"].as_str().unwrap_or("?"),
        data["differences"].as_array().map_or(0, Vec::len),
        data["artifact"]["path"].as_str().unwrap_or("no"),
    );
    if let Some(truncated) = data["more"]["truncated"].as_array() {
        for row in truncated {
            text.push_str(&format!(
                "more {}: {} withheld; --limit up to {}\n",
                row["field"].as_str().unwrap_or("?"),
                row["withheld"],
                package::MAX_LIMIT,
            ));
        }
    }
    text
}
