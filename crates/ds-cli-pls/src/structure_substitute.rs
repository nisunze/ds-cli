//! `ds pls structure-substitute` — take structure models from a reviewed library.
//!
//! The member identity, byte substitution and guarded workspace staging remain
//! owned by `ds-grid-tasks` and `ds-io`. This adapter only turns the live CLI
//! contract into the task's typed request and presents its receipt.

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_tasks::{SubstitutePlsStructureModelsRequest, substitute_pls_structure_models};
use serde_json::{Value, json};

use crate::{encode, file_digest, output_path, source_directory, source_path, task_failure};

pub static COMMAND: Command = Command {
    id: "pls.structure-substitute",
    path: &["pls", "structure-substitute"],
    contract: 1,
    summary: "Replace a design's structure models from a reviewed library.",
    purpose: "Reads one digest-pinned PLS-CADD backup and a library workspace, then stages one healed workspace whose PLS-Pole models are taken byte-for-byte from the library wherever the library carries the same model name. A design whose models are thin does not need re-surveying; it needs the models a reviewed library already holds. Route, terrain, criteria, spotting, stringing and every byte outside the substituted models are carried through untouched. A model the library does not carry keeps the design's own bytes and is reported, never approximated. Native Restore/reopen remains required.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "backup",
            "<path>",
            "Raw or ZIP-wrapped PLS-CADD backup whose models are to be replaced.",
        )
        .required(),
        Arg::value(
            "library",
            "<dir>",
            "Workspace directory supplying the models; the unshaded variant of a reviewed project is the intended input.",
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
            "Absent output root for the staged workspace and its receipt.",
        )
        .required(),
    ],
    output: "The project name, workspace path, source and library digests, model and library counts, every substituted model with before/after digests, the models preserved because the library carries none, and the native Restore/reopen gate.",
    examples: &[
        Example {
            command: "ds pls structure-substitute --backup './Gisagararev3.bak' --library './huye70-variants/unshaded' --out './gisagara-rev4' --output json",
            note: "Without a digest this refuses and reports the current source digest.",
            runnable: false,
        },
        Example {
            command: "ds pls structure-substitute --backup './Gisagararev3.bak' --library './huye70-variants/unshaded' --source-sha256 'sha256:…' --out './gisagara-rev4' --yes --output json",
            note: "Substitutes every model the library names, after the source and destination are reviewed.",
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
            remedy: "review the pinned source, the library and the absent output root, then repeat with --yes",
        },
        Refusal {
            code: "task_refused",
            when: "the owner refused the backup, the library, the native projection, or guarded output staging",
            remedy: "read detail.code and detail.detail; preserve the source and correct the named condition",
        },
        crate::RESULT_ENCODING_REFUSAL,
    ],
    reference: Some("docs/reference/pls.md"),
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let source = source_path(inputs.require("backup")?, "backup")?;
    let library = source_directory(inputs.require("library")?, "library")?;
    let output = output_path(inputs.require("out")?)?;

    let Some(expected_source_sha256) = inputs.value("source-sha256") else {
        return Err(Failure::invalid(
            "missing_digest_pin",
            "structure substitution is digest-pinned",
        )
        .remedy("pin the observed digest below with --source-sha256")
        .detail(json!({ "observed": file_digest(&source) })));
    };
    if !context.confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            "replacing the design's structure models requires confirmation",
        )
        .remedy(
            "review the pinned source, the library and the absent output root, then repeat with --yes",
        ));
    }

    let request = SubstitutePlsStructureModelsRequest {
        source_backup_path: source,
        library_workspace: library,
        output_root: output,
        expected_source_sha256: expected_source_sha256.to_string(),
    };
    let result = substitute_pls_structure_models(&request)
        .map_err(|error| task_failure(&error.code, &error.detail))?;
    encode(&result)
}

pub fn render(data: &Value) -> String {
    format!(
        "PLS-CADD structure models substituted\n  project    {}\n  workspace  {}\n  models     {} substituted of {} ({} already identical)\n  library    {} models\n  preserved  {} with no library model\n  native Restore/reopen required\n",
        data["project_name"].as_str().unwrap_or(""),
        data["workspace"].as_str().unwrap_or(""),
        data["substituted_count"],
        data["model_count"],
        data["identical_to_library_count"],
        data["library_model_count"],
        data["preserved_without_library_model"]
            .as_array()
            .map(|rows| rows.len())
            .unwrap_or(0),
    )
}
