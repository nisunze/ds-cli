//! `ds pls shading-variants` — create shaded and presentation-free workspaces.
//!
//! The byte projection and guarded workspace staging remain owned by
//! `ds-grid-tasks` and `ds-io`. This adapter only turns the live CLI contract
//! into the task's typed request and presents its receipt.

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_tasks::{CreatePlsShadingVariantsRequest, create_pls_shading_variants};
use serde_json::{Value, json};

use crate::{encode, file_digest, output_path, source_path, task_failure};

pub static COMMAND: Command = Command {
    id: "pls.shading-variants",
    path: &["pls", "shading-variants"],
    contract: 1,
    summary: "Create shaded and unshaded workspaces from one native backup.",
    purpose: "Reads one digest-pinned PLS-CADD backup and atomically creates shaded/ and unshaded/ workspaces under one absent output root. Both variants are healed and named from the backup filename, so a placeholder such as A Project never becomes delivery identity. The shaded variant preserves the native presentation. The unshaded variant empties only characterized attachment and drafting slots in PLS-Pole models; structure, material, capacity and all bytes outside those categories are preserved. Native Restore/reopen remains required.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "backup",
            "<path>",
            "Raw or ZIP-wrapped PLS-CADD backup; its filename supplies the project name.",
        )
        .required(),
        Arg::value(
            "source-sha256",
            "<sha256:…>",
            "Expected digest of the complete backup bytes.",
        ),
        Arg::value(
            "out",
            "<new-dir>",
            "Absent output root for shaded/, unshaded/, and the receipt.",
        )
        .required(),
    ],
    output: "The derived project name, both workspace paths, source and per-model digests, model counts, removed presentation categories, byte-preservation verdicts, and the native Restore/reopen gate.",
    examples: &[
        Example {
            command: "ds pls shading-variants --backup './Final Huye Gisagara.bak' --out './Final Huye Gisagara variants' --output json",
            note: "Without a digest this refuses and reports the current source digest.",
            runnable: false,
        },
        Example {
            command: "ds pls shading-variants --backup './Final Huye Gisagara.bak' --source-sha256 'sha256:…' --out './Final Huye Gisagara variants' --yes --output json",
            note: "Creates two properly named workspaces after the source and destination are reviewed.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "source_not_found",
            when: "--backup does not name a file",
            remedy: "pass the raw or ZIP-wrapped native PLS-CADD backup",
        },
        Refusal {
            code: "output_exists",
            when: "--out already exists",
            remedy: "choose a new immutable output root",
        },
        Refusal {
            code: "missing_digest_pin",
            when: "--source-sha256 was not supplied",
            remedy: "use the observed digest returned in detail and retry",
        },
        Refusal {
            code: "confirmation_required",
            when: "--yes was not supplied",
            remedy: "review the pinned source and absent output root, then repeat with --yes",
        },
        Refusal {
            code: "task_refused",
            when: "the owner refused the backup, project name, native projection, or guarded output staging",
            remedy: "read detail.code and detail.detail; preserve the source and correct the named condition",
        },
        crate::RESULT_ENCODING_REFUSAL,
    ],
    reference: Some("docs/reference/pls.md"),
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let source = source_path(inputs.require("backup")?, "backup")?;
    let output = output_path(inputs.require("out")?)?;

    let Some(expected_source_sha256) = inputs.value("source-sha256") else {
        return Err(Failure::invalid(
            "missing_digest_pin",
            "workspace variant creation is digest-pinned",
        )
        .remedy("pin the observed digest below with --source-sha256")
        .detail(json!({ "observed": file_digest(&source) })));
    };
    if !context.confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            "creating the two workspace trees requires confirmation",
        )
        .remedy("review the pinned source and absent output root, then repeat with --yes"));
    }

    let request = CreatePlsShadingVariantsRequest {
        source_backup_path: source,
        output_root: output,
        expected_source_sha256: expected_source_sha256.to_string(),
    };
    let result = create_pls_shading_variants(&request)
        .map_err(|error| task_failure(&error.code, &error.detail))?;
    encode(&result)
}

pub fn render(data: &Value) -> String {
    format!(
        "PLS-CADD shading variants created\n  project {}\n  shaded   {}\n  unshaded {}\n  models   {} changed of {}\n  native Restore/reopen required\n",
        data["project_name"].as_str().unwrap_or(""),
        data["shaded_workspace"].as_str().unwrap_or(""),
        data["unshaded_workspace"].as_str().unwrap_or(""),
        data["changed_model_count"],
        data["model_count"],
    )
}
