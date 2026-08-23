//! `ds map design report` — export one transformer's report on this machine.
//!
//! The reporting half of a drafting session: after `process` and `save`, this
//! runs the SAME local Network Reporter lane the design Status surface uses
//! and answers with path-free artifact evidence — output ids, filenames,
//! sizes and SHA-256 digests — never a filesystem path. Publication to the
//! project continues through the application's ordinary sync drain.
//!
//! Deliberately absent: `--force`. Forcing bypasses the freshness gate behind
//! an operator password prompt in the application, and a password prompt
//! raised by a background CLI call is an interruption nobody asked for. A
//! stale report says so in its result.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;
use crate::design::TRANSFORMER_ARG;

pub static COMMAND: Command = Command {
    id: "map.design.report",
    path: &["map", "design", "report"],
    contract: 1,
    summary: "Export one transformer's report locally and return artifact evidence.",
    purpose: "\
Runs the local Network Reporter export for one transformer — the same lane \
the design Status surface uses — and reports each committed artifact's output \
id, filename, content type, size and SHA-256. Compute is local; publication \
to the project continues through the application's ordinary artifact sync. \
There is no --force: a report the freshness gate holds back reports that \
instead of silently regenerating.",
    effect: Effect::ArtifactWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, DESCRIPTOR_ARG],
    output: "\
Whether the export regenerated or was already fresh, the artifact count, and \
per artifact: outputId, filename, contentType, sizeBytes, sha256, locator.",
    examples: &[Example {
        command: "ds map design report --transformer T-1042 --yes --output json",
        note: "Read .data.artifacts[].sha256 as the evidence of what was produced.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        super::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a command that writes report artifacts",
            remedy: "re-run with --yes once you intend the export",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;

    let mut arguments = Map::new();
    arguments.insert("transformer".into(), json!(transformer));

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_REPORT,
        Value::Object(arguments),
        crate::DESIGN_PROCESS_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;

    Ok(json!({
        "transformer": transformer,
        "project": result["project"],
        "regenerated": result["regenerated"].as_bool().unwrap_or(false),
        "skipped_fresh": result["skippedFresh"].as_bool().unwrap_or(false),
        "artifact_count": result["artifactCount"].as_u64().unwrap_or(0),
        "artifacts": result["artifacts"],
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} artifact(s) · {}\n",
        data["artifact_count"],
        if data["skipped_fresh"].as_bool().unwrap_or(false) {
            "already fresh"
        } else if data["regenerated"].as_bool().unwrap_or(false) {
            "regenerated locally"
        } else {
            "exported"
        },
    );
    if let Some(artifacts) = data["artifacts"].as_array() {
        for artifact in artifacts {
            out.push_str(&format!(
                "  {:<28} {:>10} B  sha256 {}\n",
                artifact["filename"].as_str().unwrap_or("?"),
                artifact["sizeBytes"].as_u64().unwrap_or(0),
                artifact["sha256"].as_str().unwrap_or("?"),
            ));
        }
    }
    out.push_str("\npublication continues through the application's artifact sync\n");
    out
}
