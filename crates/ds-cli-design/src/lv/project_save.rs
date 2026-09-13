//! Publish an exact native result through the selected user's fenced save owner.
use std::io::Read;

use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_network::network::native_fast_lv::{
    MAX_NATIVE_FAST_LV_INPUT_BYTES, MAX_NATIVE_FAST_LV_OUTPUT_BYTES, project_native_fast_lv_result,
};
use serde_json::{Value, json};

use super::artifact::sha256;

const LOCAL_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "fast_lv_save_input_invalid",
        when: "a receipt or artifact is absent, oversized, malformed or mismatched",
        remedy: "retain the successful project-export and process JSON receipts and their exact request/result files",
    },
    Refusal {
        code: "fast_lv_publication_invalid",
        when: "the network owner refuses the selected result",
        remedy: "process the unchanged fenced source again and inspect every job outcome",
    },
    Refusal {
        code: "confirmation_required",
        when: "the save lacks --yes",
        remedy: "review the selected processed result and pass --yes",
    },
];
const fn refusals()
-> [Refusal; LOCAL_REFUSALS.len() + super::project_export::COMMAND.refusals.len()] {
    let mut result =
        [LOCAL_REFUSALS[0]; LOCAL_REFUSALS.len() + super::project_export::COMMAND.refusals.len()];
    let mut index = 0;
    while index < LOCAL_REFUSALS.len() {
        result[index] = LOCAL_REFUSALS[index];
        index += 1;
    }
    let mut other = 0;
    while other < super::project_export::COMMAND.refusals.len() {
        result[index] = super::project_export::COMMAND.refusals[other];
        index += 1;
        other += 1;
    }
    result
}

pub static COMMAND: Command = Command {
    id: "design.lv.project-save",
    path: &["design", "lv", "project-save"],
    contract: 1,
    summary: "Save a processed LV transformer online and verify its current version.",
    purpose: "Finish headless LV processing by saving one selected successful result to the signed-in project. Supply the original project-export receipt, configured process input, full result and process receipt. Exact file hashes, source layers, server version/content digest and current project configuration are checked before the bounded save; fresh readback proves the saved result. Tags are preserved. Run report.project.export afterward to print and publish reports. No Desktop is needed.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "source",
            "<receipt.json>",
            "Successful design.lv.project-export JSON envelope for this transformer.",
        )
        .required(),
        Arg::value(
            "input",
            "<request.json>",
            "Exact configured request passed to design.lv.process (up to 64 MiB).",
        )
        .required(),
        Arg::value(
            "result",
            "<result.json>",
            "Full native process result (up to 256 MiB).",
        )
        .required(),
        Arg::value(
            "process-receipt",
            "<receipt.json>",
            "Successful design.lv.process JSON envelope pinning both file hashes.",
        )
        .required(),
        Arg::value(
            "transformer",
            "<name>",
            "Exact successful transformer in the batch and source receipt.",
        )
        .required(),
        Arg::value(
            "operation-id",
            "<id>",
            "Stable idempotency key for this source/result save; retain on retry.",
        )
        .required(),
        Arg::value(
            "lane",
            "<stable|canary>",
            "Native user lane matching the source receipt.",
        )
        .default("stable")
        .choices(&["stable", "canary"]),
    ],
    output: "Selected project/lane, transformer save outcome and fresh verified server version/content digest. A local process result alone is never a saved receipt.",
    examples: &[],
    refusals: &refusals(),
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("fast_lv_save_input_invalid", message)
        .remedy("Retain successful project-export/process JSON receipts and their exact files; export again if the source changed.")
}

fn read(path: &str, maximum: usize) -> Result<Vec<u8>, Failure> {
    let file = std::fs::File::open(path)
        .map_err(|error| invalid(format!("Cannot open {path}: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| invalid(error.to_string()))?;
    if !metadata.is_file() || metadata.len() > maximum as u64 {
        return Err(invalid("Artifact is not a bounded regular file."));
    }
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| invalid(error.to_string()))?;
    if bytes.len() > maximum {
        return Err(invalid("Artifact grew beyond its byte bound."));
    }
    Ok(bytes)
}

fn receipt(bytes: &[u8], command: &str) -> Result<Value, Failure> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| invalid(error.to_string()))?;
    if value["v"] != 1
        || value["command"] != command
        || value["status"] != "ok"
        || !value["data"].is_object()
    {
        return Err(invalid(format!(
            "Expected a successful {command} JSON envelope."
        )));
    }
    Ok(value["data"].clone())
}

fn verify_receipts(
    source: &Value,
    process: &Value,
    transformer: &str,
    lane: &str,
    input: &[u8],
    result: &[u8],
) -> Result<(String, u64, String), Failure> {
    if source["transformer"] != transformer
        || source["lane"] != lane
        || source["source"]["state"] != "fenced"
    {
        return Err(invalid(
            "Source receipt transformer, lane or fenced state does not match.",
        ));
    }
    if process["input_sha256"] != sha256(input) || process["result_sha256"] != sha256(result) {
        return Err(invalid(
            "Process receipt hashes do not match the exact input and result bytes.",
        ));
    }
    let project = source["project"]["ds_project"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("Source receipt has no project."))?;
    let version = source["source"]["version"]
        .as_u64()
        .ok_or_else(|| invalid("Source receipt has no server version."))?;
    let digest = source["source"]["content_digest"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("Source receipt has no content digest."))?;
    Ok((project.to_owned(), version, digest.to_owned()))
}

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    // Refuse bad files before restoring credentials or contacting the project.
    let source = receipt(
        &read(inputs.require("source")?, 1024 * 1024)?,
        "design.lv.project-export",
    )?;
    let process = receipt(
        &read(inputs.require("process-receipt")?, 1024 * 1024)?,
        "design.lv.process",
    )?;
    let input = read(inputs.require("input")?, MAX_NATIVE_FAST_LV_INPUT_BYTES)?;
    let result = read(inputs.require("result")?, MAX_NATIVE_FAST_LV_OUTPUT_BYTES)?;
    let transformer = inputs.require("transformer")?;
    let lane = inputs.require("lane")?;
    let (project_id, base_version, source_content_digest) =
        verify_receipts(&source, &process, transformer, lane, &input, &result)?;
    let projection =
        project_native_fast_lv_result(&input, &result, transformer).map_err(|error| {
            Failure::invalid("fast_lv_publication_invalid", error.to_string())
                .remedy("Process the unchanged fenced source again and inspect every job outcome.")
        })?;
    let saved = ds_cli_auth::save_transformers(
        lane,
        &ds_client_core::TransformerSaveBatch {
            project_id,
            items: vec![ds_client_core::TransformerSaveItem {
                transformer_name: transformer.to_owned(),
                base_version,
                source_content_digest,
                source_layers: projection.source_layers,
                gdfs: projection.gdfs,
                config_dfs: projection.config_dfs,
                process_metadata: projection.process_metadata,
                operation_id: inputs.require("operation-id")?.to_owned(),
            }],
        },
    )?;
    Ok(json!({"project":saved.project_id(), "lane":saved.lane(), "result":saved.result()}))
}

pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_requires_successful_receipts_and_exact_bytes_before_authentication() {
        assert!(
            receipt(
                br#"{"v":1,"command":"design.lv.process","status":"error","data":{}}"#,
                "design.lv.process"
            )
            .is_err()
        );
        let source = json!({"transformer":"T1","lane":"stable","source":{"state":"fenced","version":7,"content_digest":"server-digest"},"project":{"ds_project":"project"}});
        let process = json!({"input_sha256":sha256(b"input"),"result_sha256":sha256(b"result")});
        assert_eq!(
            verify_receipts(&source, &process, "T1", "stable", b"input", b"result").unwrap(),
            ("project".into(), 7, "server-digest".into())
        );
        assert!(verify_receipts(&source, &process, "T2", "stable", b"input", b"result").is_err());
        assert!(verify_receipts(&source, &process, "T1", "canary", b"input", b"result").is_err());
        assert!(verify_receipts(&source, &process, "T1", "stable", b"changed", b"result").is_err());
        assert!(verify_receipts(&source, &process, "T1", "stable", b"input", b"changed").is_err());
    }
}
