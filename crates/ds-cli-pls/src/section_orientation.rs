//! `ds pls section-orientation` — is a DON section oriented as its alignment
//! says?
//!
//! This is the one task in the domain whose request is genuinely structured:
//! it needs the alignment's ordered structure numbers and its two boundary
//! kinds, which is a nested object, not a flag.
//!
//! So it takes `--request <file>` and publishes the schema through
//! `ds pls section-orientation --schema`. Inventing a flag per nested field
//! would produce exactly the "enormous collection of ambiguous flags" a typed
//! request document exists to avoid — and the schema comes from the task
//! itself, so it cannot drift from what the task will accept.

use std::path::PathBuf;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_tasks::{
    DiagnosePlsSectionOrientationRequest, diagnose_pls_section_orientation,
    diagnose_pls_section_orientation_request_schema,
};
use serde_json::{Value, json};

use crate::task_failure;

/// Bound the request document. It describes one section's alignment, which is
/// kilobytes at most.
const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

pub static COMMAND: Command = Command {
    id: "pls.section-orientation",
    path: &["pls", "section-orientation"],
    contract: 1,
    summary: "Diagnose a DON section against its declared alignment.",
    purpose: "\
Checks whether a tension section in a .don is oriented the way its alignment \
declares — the ordered structure numbers and the kind of boundary at each end. \
The request is a nested document rather than a set of flags, because the \
alignment evidence is structured; run with --schema to get exactly what this \
build accepts. Read-only.",
    chapter: Chapter::PlsCadd,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("request", "<path>", "The typed request document."),
        Arg::switch("schema", "Print the request schema instead of running."),
    ],
    output: "\
With --schema, the JSON Schema this build accepts. Otherwise the source's leaf \
and digest, and the orientation diagnosis.",
    examples: &[
        Example {
            command: "ds pls section-orientation --schema --output json",
            note: "Read the request contract before authoring one.",
            runnable: true,
        },
        Example {
            command: "ds pls section-orientation --request ./section.json --output json",
            note: "",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "missing_request",
            when: "neither --request nor --schema was given",
            remedy: "run with --schema to see the contract, then pass --request <path>",
        },
        Refusal {
            code: "request_not_found",
            when: "--request does not name a readable file within its 1 MiB bound",
            remedy: "check the path",
        },
        Refusal {
            code: "request_invalid",
            when: "the document does not match the schema this build accepts",
            remedy: "run with --schema and compare field by field",
        },
        Refusal {
            code: "task_refused",
            when: "the task ran and refused — an unreadable .don, or a section that does not exist",
            remedy: "read detail.code and detail.detail for the task's own reason",
        },
        crate::RESULT_ENCODING_REFUSAL,
    ],
    reference: Some("docs/reference/pls.md"),
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    if inputs.switch("schema") {
        return Ok(json!({
            "schema": diagnose_pls_section_orientation_request_schema(),
        }));
    }

    let Some(raw) = inputs.value("request") else {
        return Err(Failure::invalid(
            "missing_request",
            "this command needs a typed request document",
        )
        .remedy("run with --schema to see the contract, then pass --request <path>")
        .next("ds pls section-orientation --schema"));
    };

    let path = PathBuf::from(raw);
    let metadata = std::fs::metadata(&path).map_err(|error| {
        Failure::invalid("request_not_found", format!("cannot read `{raw}`"))
            .remedy("check the path")
            .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    if !metadata.is_file() || metadata.len() > MAX_REQUEST_BYTES {
        return Err(Failure::invalid(
            "request_not_found",
            format!("`{raw}` is not a readable request document within its bound"),
        )
        .remedy("check the path; the request is a small JSON document"));
    }

    let bytes = std::fs::read(&path).map_err(|error| {
        Failure::invalid("request_not_found", format!("cannot read `{raw}`"))
            .remedy("check file permissions")
            .detail(json!({ "detail": error.kind().to_string() }))
    })?;

    let request: DiagnosePlsSectionOrientationRequest =
        serde_json::from_slice(&bytes).map_err(|error| {
            Failure::invalid(
                "request_invalid",
                "the request does not match the schema this build accepts",
            )
            .remedy("run with --schema and compare field by field")
            .next("ds pls section-orientation --schema")
            .detail(json!({ "detail": error.to_string() }))
        })?;

    let result = diagnose_pls_section_orientation(&request)
        .map_err(|error| task_failure(&error.code, &error.detail))?;

    serde_json::to_value(&result).map_err(|error| {
        Failure::internal(
            "result_unserializable",
            "the task result could not be encoded",
        )
        .detail(json!({ "detail": error.to_string() }))
    })
}

pub fn render(data: &Value) -> String {
    if let Some(schema) = data.get("schema") {
        return serde_json::to_string_pretty(schema).unwrap_or_default();
    }
    format!(
        "{}\n  {} bytes\n\n{}",
        data["source_leaf"].as_str().unwrap_or(""),
        data["source_bytes"],
        serde_json::to_string_pretty(&data["diagnosis"]).unwrap_or_default(),
    )
}
