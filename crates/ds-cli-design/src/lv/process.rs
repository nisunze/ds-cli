//! `ds design lv process` — bounded native LV batch compute.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_network_contracts::lv_processing::{MAX_NATIVE_LV_INPUT_BYTES, NativeLvError};
use serde_json::{Value, json};

use super::artifact::{RESULT, ensure_absent, sha256, write_new};

pub static COMMAND: Command = Command {
    id: "design.lv.process",
    path: &["design", "lv", "process"],
    contract: 4,
    summary: "Process transformer batches headlessly in parallel from local files.",
    purpose: "Process one ds.lv.process.request/v1 batch through the admitted offline Network executable: 1..=32 jobs, at most 100,000 features, with intended settings and configuration. Historical ds.fast-lv.request/v1 remains explicit read compatibility and retains its result schema. Optional differential intent selects feeders or captured edits through the shared Rust LV owner. Results retain input order. Full layers go to --out; keep the receipt for design.lv.project-save. See the reference for schemas and preservation rules.",
    chapter: Chapter::Design,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "input",
            "<path>",
            "Closed ds.lv.process.request/v1 batch document (historical ds.fast-lv.request/v1 accepted); optional job differential uses the map.design.process JSON shape (maximum 64 MiB).",
        )
        .required(),
        Arg::value(
            "out",
            "<path>",
            "Absent path for the complete ds.lv.process.result/v1 document, or the historical result schema matching the request.",
        )
        .required(),
    ],
    output: "Input/result SHA-256, output path and bytes, captured executable SHA-256, engine identity and development-override status, runtime, worker count and ordered job statuses with preservation counts. A job that would change approved rows keeps them exactly as supplied, still returns its voltage-drop analysis, and names the withheld drafting in an approved_network_kept warning. The setting keep_network_as_drawn keeps every supplied row exactly as drawn (network_kept_as_drawn), so a supplied design saves without redrawing; renumber_lines=false keeps supplied line numbers. Full layers and diagnostics go to --out.",
    examples: &[Example {
        command: "ds design lv process --input ./lv-request.json --out ./lv-result.json --output json",
        note: "Run the closed local batch without a Desktop session or project identity.",
        runnable: false,
    }],
    refusals: &[
        Refusal {code:"network_engine_missing",when:"the compatible native Network engine is absent",remedy:"install the matching Network engine addon"},
        Refusal {code:"network_engine_incompatible",when:"the captured Network executable lacks the required protocol or capability",remedy:"install the compatible Network engine"},
        Refusal {code:"network_serialization_incompatible",when:"the captured engine differs from this consumer's preserved JSON byte profile",remedy:"install the insertion-order Network component for this host"},
        Refusal {code:"network_engine_path_invalid",when:"the explicit development engine path is not absolute",remedy:"set DS_NETWORK_ENGINE_BIN to an absolute compatible executable path"},
        Refusal {code:"network_engine_invalid",when:"the captured executable cannot be read within its byte bound",remedy:"install an intact compatible Network component"},
        Refusal {code:"network_engine_unqualified",when:"a normally installed candidate lacks source or dependency-closure qualification",remedy:"install a qualified component; explicit development overrides remain labeled"},
        Refusal {code:"network_engine_identity_invalid",when:"the identity probe is malformed, oversized or unsuccessful",remedy:"install an intact compatible Network component"},
        Refusal {code:"network_engine_changed",when:"the captured executable changes during admission or execution",remedy:"retain immutable version directories and retry with an intact component"},
        Refusal {code:"network_engine_refused",when:"the executable exits with an unrecognized owner refusal",remedy:"inspect the bounded failure and use a compatible component"},
        Refusal {code:"network_scratch_invalid",when:"private staging cannot be created beside the output",remedy:"choose an absent output in a writable ordinary directory"},
        Refusal {code:"network_request_io",when:"the admitted request cannot be staged durably",remedy:"choose a writable output directory and retry from unchanged input"},
        Refusal {code:"network_input_changed",when:"the staged request changes during execution",remedy:"retain the unchanged input and retry in a protected output directory"},
        Refusal {code:"network_receipt_bounded",when:"the engine exceeds its control-receipt bound",remedy:"install a compatible component; complete results belong in artifacts"},
        Refusal {code:"network_receipt_invalid",when:"the terminal receipt is absent, malformed or inconsistent with admission",remedy:"retain the unchanged input and use an intact compatible component"},
        Refusal {code:"network_refusal_invalid",when:"a failed engine does not return an admitted refusal",remedy:"retain the unchanged input and use an intact compatible component"},
        Refusal {code:"network_result_invalid",when:"the result is incomplete, unsafe or inconsistent with ordered inputs",remedy:"retain the unchanged input and use an intact compatible component"},
        Refusal {code:"network_result_digest_mismatch",when:"the artifact bytes differ from the terminal receipt",remedy:"retain the unchanged input and use an intact compatible component"},
        Refusal {code:"network_workers_invalid",when:"the engine rejects the positive worker budget",remedy:"use a compatible component with the admitted worker contract"},
        Refusal {code:"network_task_cancelled",when:"the batch does not complete after cooperative cancellation",remedy:"retry the unchanged input at a new output path"},
        Refusal {code:"network_control_unavailable",when:"the supervised control channel cannot be established",remedy:"retry on a host supporting the admitted native supervision boundary"},
        Refusal {code:"network_control_invalid",when:"the owner rejects the fixed task control document",remedy:"use a compatible Network component"},
        Refusal {code:"network_input_invalid",when:"the owner cannot admit its staged input",remedy:"retain the unchanged input and use a compatible component"},
        Refusal {code:"network_output_invalid",when:"the owner cannot admit its private output directory",remedy:"choose a writable ordinary output directory"},
        Refusal {code:"network_artifact_io",when:"the owner cannot durably emit its complete artifact",remedy:"provide disk space and retry unchanged input at a new output path"},
        Refusal {code:"callee_cancelled",when:"the host cancels the child or it completes after cancellation",remedy:"retry the unchanged input at a new output path"},
        Refusal {code:"callee_timed_out",when:"the supervised identity probe or task exceeds its bound",remedy:"use a compatible component and split large batches"},
        Refusal {code:"callee_wait_failed",when:"the host cannot collect the owned child status",remedy:"inspect host process availability and retry unchanged input"},
        Refusal {code:"callee_supervision_failed",when:"the host cannot establish process-tree supervision",remedy:"use a host supporting the admitted native supervision boundary"},
        Refusal {code:"callee_stdin_failed",when:"the child cannot receive its fixed control input",remedy:"use an intact compatible component"},
        Refusal {
            code: "fast_lv_source_not_found",
            when: "--input is absent, not a regular file, or cannot be read",
            remedy: "pass a readable local ds.lv.process.request/v1 or historical ds.fast-lv.request/v1 file",
        },
        Refusal {
            code: "fast_lv_input_too_large",
            when: "the request file exceeds 64 MiB",
            remedy: "split the work into smaller closed batches",
        },
        Refusal {
            code: "fast_lv_input_invalid",
            when: "the request is malformed, has unknown fields/settings, or a layer is not a FeatureCollection",
            remedy: "write the exact LV processing request shape shown in the design reference",
        },
        Refusal {
            code: "fast_lv_schema_unsupported",
            when: "the request schema is neither ds.lv.process.request/v1 nor historical ds.fast-lv.request/v1",
            remedy: "write a supported LV processing v1 request",
        },
        Refusal {
            code: "fast_lv_bound_refused",
            when: "job, feature, layer, config-sheet, name, or uniqueness bounds are exceeded",
            remedy: "split the batch or shorten the named field reported by the refusal",
        },
        Refusal {
            code: "fast_lv_output_exists",
            when: "--out already exists",
            remedy: "choose a new result path; this command never overwrites",
        },
        Refusal {
            code: "fast_lv_output_write_failed",
            when: "the result cannot be durably written at --out",
            remedy: "choose a writable absent path and retry from the unchanged input",
        },
        Refusal {
            code: "fast_lv_output_too_large",
            when: "the complete result exceeds 256 MiB",
            remedy: "split the request into smaller batches; results are never truncated",
        },
        Refusal {
            code: "fast_lv_result_encoding_failed",
            when: "the owner result cannot be encoded as its v1 document",
            remedy: "keep the input unchanged and report this internal encoding failure",
        },
    ],
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: || ds_network_client::available(),
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let input_path = PathBuf::from(inputs.require("input")?);
    let output_path = PathBuf::from(inputs.require("out")?);
    ensure_absent(&output_path, &RESULT)?;

    let input = bounded_read(&input_path)?;
    let input_sha256 = sha256(&input);
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    // Staging follows the caller's output volume rather than a RAM-backed
    // system temporary directory. The client owns and removes its private job.
    let scratch = std::fs::canonicalize(
        output_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )
    .map_err(|error| {
        Failure::failed(
            "fast_lv_output_write_failed",
            format!("Could not resolve the LV result directory: {error}"),
        )
    })?;
    let outcome = ds_network_client::process_lv_with_order(
        &input,
        workers,
        &scratch,
        &|| false,
        ds_network_contracts::engine::JsonObjectOrder::Insertion,
    )
    .map_err(map_network_error)?;
    if outcome.cancelled {
        return Err(Failure::failed(
            "network_task_cancelled",
            "The Network engine did not complete the LV processing batch.",
        )
        .remedy("Retry the unchanged input at a new output path."));
    }
    let runtime = outcome.runtime;
    let result = outcome.result;
    let jobs = result.jobs.len();
    let succeeded = result.jobs.iter().filter(|job| job.ok).count();
    let failed = jobs - succeeded;
    let statuses = result
        .jobs
        .iter()
        .map(|job| {
            json!({
                "transformer_name": job.transformer_name,
                "ok": job.ok,
                "error_code": job.error_code,
                "preservation": job.preservation,
            })
        })
        .collect::<Vec<_>>();
    let engine_core_version = result.engine_core_version.clone();
    let output = outcome.bytes;
    let result_sha256 = sha256(&output);
    write_new(&output_path, &output, &RESULT)?;

    Ok(json!({
        "out": output_path,
        "input_sha256": input_sha256,
        "result_sha256": result_sha256,
        "byte_count": output.len(),
        "engine_core_version": engine_core_version,
        "execution_environment": runtime.environment.as_str(),
        "rayon_worker_threads": runtime.internal_worker_threads,
        "engine_identity": outcome.identity,
        "executable_sha256": outcome.executable_sha256,
        "development_override": outcome.development_override,
        "jobs": jobs,
        "succeeded": succeeded,
        "failed": failed,
        "results": statuses,
    }))
}

pub fn render(value: &Value) -> String {
    let jobs = value["jobs"].as_u64().unwrap_or(0);
    let succeeded = value["succeeded"].as_u64().unwrap_or(0);
    let failed = value["failed"].as_u64().unwrap_or(0);
    let mut text = format!(
        "LV processed {jobs} transformer(s): {succeeded} succeeded, {failed} failed.\nResult: {}\nSHA-256: {}",
        value["out"].as_str().unwrap_or(""),
        value["result_sha256"].as_str().unwrap_or(""),
    );
    if failed > 0 {
        text.push_str("\nInspect the result document for per-transformer diagnostics.");
    }
    text
}

pub(super) fn bounded_read(path: &Path) -> Result<Vec<u8>, Failure> {
    let mut file = File::open(path).map_err(|error| {
        Failure::invalid(
            "fast_lv_source_not_found",
            format!("Could not open the LV request: {error}"),
        )
        .remedy("Pass a readable local LV processing v1 file.")
    })?;
    let metadata = file.metadata().map_err(|error| {
        Failure::invalid(
            "fast_lv_source_not_found",
            format!("Could not inspect the LV request: {error}"),
        )
    })?;
    if !metadata.is_file() {
        return Err(Failure::invalid(
            "fast_lv_source_not_found",
            "The LV request is not a regular file.",
        )
        .remedy("Pass a readable local LV processing v1 file."));
    }
    if metadata.len() > MAX_NATIVE_LV_INPUT_BYTES as u64 {
        return Err(map_owner_error(NativeLvError::InputTooLarge {
            actual: metadata.len().try_into().unwrap_or(usize::MAX),
            maximum: MAX_NATIVE_LV_INPUT_BYTES,
        }));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take((MAX_NATIVE_LV_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            Failure::invalid(
                "fast_lv_source_not_found",
                format!("Could not read the LV request: {error}"),
            )
        })?;
    if bytes.len() > MAX_NATIVE_LV_INPUT_BYTES {
        return Err(map_owner_error(NativeLvError::InputTooLarge {
            actual: bytes.len(),
            maximum: MAX_NATIVE_LV_INPUT_BYTES,
        }));
    }
    Ok(bytes)
}

pub(super) fn map_owner_error(error: NativeLvError) -> Failure {
    map_owner_message(error.code(), error.to_string())
}
fn map_owner_message(code: &'static str, message: String) -> Failure {
    match code {
        "fast_lv_input_too_large"
        | "fast_lv_input_invalid"
        | "fast_lv_schema_unsupported"
        | "fast_lv_bound_refused" => Failure::invalid(code, message),
        "fast_lv_output_too_large" => Failure::failed(code, message)
            .remedy("Split the request into smaller batches; results are never truncated."),
        "fast_lv_result_encoding_failed" => Failure::internal(code, message),
        _ => Failure::internal("fast_lv_result_encoding_failed", message),
    }
}
fn map_network_error(error: ds_network_client::NetworkFailure) -> Failure {
    if let Some(owner) = error.owner_error() {
        return map_owner_message(owner.code(), owner.to_string());
    }
    if error.owner_code().is_some() && error.code().starts_with("fast_lv_") {
        return map_owner_message(error.code(), error.to_string());
    }
    let code = error.code();
    let message = error.to_string();
    let failure = match code {
        "network_engine_missing" => Failure::unavailable(code, message),
        "network_engine_incompatible"
        | "network_serialization_incompatible"
        | "network_engine_path_invalid"
        | "network_engine_invalid"
        | "network_engine_unqualified"
        | "network_engine_identity_invalid"
        | "network_engine_changed"
        | "network_engine_refused"
        | "network_scratch_invalid"
        | "network_request_io"
        | "network_input_changed"
        | "network_receipt_bounded"
        | "network_receipt_invalid"
        | "network_refusal_invalid"
        | "network_result_invalid"
        | "network_result_digest_mismatch"
        | "network_workers_invalid"
        | "network_task_cancelled"
        | "network_control_unavailable"
        | "network_control_invalid"
        | "network_input_invalid"
        | "network_output_invalid"
        | "network_artifact_io"
        | "callee_cancelled"
        | "callee_timed_out"
        | "callee_wait_failed"
        | "callee_supervision_failed"
        | "callee_stdin_failed" => Failure::failed(code, message),
        _ => Failure::failed("network_engine_refused", message),
    };
    match COMMAND.refusals.iter().find(|r| r.code == failure.code()) {
        Some(refusal) => failure.remedy(refusal.remedy),
        None => failure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_is_offline_local_file_compute() {
        assert_eq!(COMMAND.authority, Authority::None);
        assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
        assert_eq!(COMMAND.path, ["design", "lv", "process"]);
    }

    fn execute(input: &Path, output: &Path) -> Result<Value, Failure> {
        let parsed = ds_cli_contract::parse(
            &COMMAND,
            &[
                "--input".into(),
                input.to_string_lossy().into_owned(),
                "--out".into(),
                output.to_string_lossy().into_owned(),
            ],
        )
        .unwrap();
        run(
            &parsed,
            &Context {
                confirmed: false,
                output: ds_cli_contract::Output::resolve(
                    ds_cli_contract::Format::Json,
                    false,
                    true,
                ),
            },
        )
    }

    #[test]
    fn missing_or_malformed_sources_refuse_without_an_engine_or_artifact() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("request.json");
        let output = directory.path().join("result.json");
        assert_eq!(
            execute(&source, &output).unwrap_err().code(),
            "fast_lv_source_not_found"
        );
        std::fs::write(&source, b"{\"schema\":\"unrecognized\",\"jobs\":[]}").unwrap();
        assert_eq!(
            execute(&source, &output).unwrap_err().code(),
            "fast_lv_schema_unsupported"
        );
        assert!(!output.exists());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    #[ignore = "requires an explicitly selected compatible Network executable"]
    fn actual_network_process_retains_historical_cli_bytes_and_ordered_failures() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("request.json");
        let output = directory.path().join("result.json");
        let request = include_bytes!(
            "../../../../../ds-network/crates/ds-network/tests/fixtures/native-engine-boundary/v1/mixed.request.json"
        );
        let golden = include_bytes!(
            "../../../../../ds-network/crates/ds-network/tests/fixtures/native-engine-boundary/v1/mixed.insertion.result.json"
        );
        std::fs::write(&source, request).unwrap();
        let receipt = execute(&source, &output).unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), request);
        assert_eq!(std::fs::read(&output).unwrap(), golden);
        assert_eq!(receipt["input_sha256"], sha256(request));
        assert_eq!(receipt["result_sha256"], sha256(golden));
        assert_eq!(receipt["jobs"], 2);
        assert_eq!(receipt["succeeded"], 1);
        assert_eq!(receipt["failed"], 1);
        assert_eq!(receipt["results"][0]["transformer_name"], "T_success");
        assert_eq!(receipt["results"][1]["transformer_name"], "T_failure");
        assert_eq!(receipt["engine_identity"]["jsonObjectOrder"], "insertion");
        assert!(
            receipt["engine_identity"]["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c == "lv_processing_v1")
        );
        let executable = std::fs::read(
            std::env::var_os("DS_NETWORK_ENGINE_BIN").expect("explicit actual engine"),
        )
        .unwrap();
        assert_eq!(receipt["executable_sha256"], sha256(&executable));
        assert_eq!(receipt["development_override"], true);
        assert_eq!(
            std::fs::read_dir(directory.path()).unwrap().count(),
            2,
            "private staging is removed"
        );
    }

    #[test]
    fn renderer_never_projects_full_layers_or_errors() {
        let text = render(&json!({
            "jobs": 2,
            "succeeded": 1,
            "failed": 1,
            "out": "result.json",
            "result_sha256": "abc",
            "results": [{ "transformer_name": "T1", "ok": false }],
        }));
        assert!(text.contains("1 succeeded, 1 failed"));
        assert!(!text.contains("T1"));
        assert!(!text.contains("gdfs"));
    }

    #[test]
    #[ignore = "requires an explicitly selected compatible Network executable"]
    fn approved_rows_a_job_would_change_are_kept_and_the_analysis_still_returns() {
        let dir = tempfile::tempdir().unwrap();
        let input_path = dir.path().join("request.json");
        let output_path = dir.path().join("result.json");
        let mut source: Value = serde_json::from_slice(include_bytes!(
            "../../../../../ds-command-kernel/crates/ds-client-core/tests/fixtures/native-lv-save-request.json"
        )).unwrap();
        source["jobs"][0]["gdfs"]["lv_poles"]["features"] = json!([{
            "type":"Feature", "id":"approved-missing-pole", "geometry":null,
            "properties":{"drafting_status":"approved", "struct_type":"manual"}
        }]);
        std::fs::write(&input_path, serde_json::to_vec(&source).unwrap()).unwrap();
        let parsed = ds_cli_contract::parse(
            &COMMAND,
            &[
                "--input".into(),
                input_path.to_string_lossy().into_owned(),
                "--out".into(),
                output_path.to_string_lossy().into_owned(),
            ],
        )
        .unwrap();
        let receipt = run(
            &parsed,
            &Context {
                confirmed: false,
                output: ds_cli_contract::Output::resolve(
                    ds_cli_contract::Format::Json,
                    false,
                    true,
                ),
            },
        )
        .unwrap();
        assert_eq!(receipt["failed"], 0);
        let row = &receipt["results"][0];
        assert!(row["error_code"].is_null());
        assert_eq!(
            row["preservation"]["effective_settings"]["calculate_voltage_drop"],
            true
        );
        let result: Value = serde_json::from_slice(&std::fs::read(output_path).unwrap()).unwrap();
        assert_eq!(result["jobs"][0]["preservation"], row["preservation"]);
        let output = &result["jobs"][0]["output"];
        let pole = &output["gdfs"]["lv_poles"]["features"][0];
        assert_eq!(pole["id"], "approved-missing-pole");
        assert_eq!(pole["properties"]["struct_type"], "manual");
        assert!(output["voltage_drop"]["status"].is_string());
        assert!(
            output["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|warning| {
                    warning["code"] == "approved_network_kept"
                        && warning["message"]
                            .as_str()
                            .unwrap()
                            .contains("lv_poles: @missing 1")
                })
        );
    }

    #[test]
    #[ignore = "requires an explicitly selected compatible Network executable"]
    fn a_supplied_network_kept_as_drawn_comes_back_row_for_row() {
        let dir = tempfile::tempdir().unwrap();
        let input_path = dir.path().join("request.json");
        let output_path = dir.path().join("result.json");
        let mut source: Value = serde_json::from_slice(include_bytes!(
            "../../../../../ds-command-kernel/crates/ds-client-core/tests/fixtures/native-lv-save-request.json"
        )).unwrap();
        source["jobs"][0]["settings"]["keep_network_as_drawn"] = json!(true);
        std::fs::write(&input_path, serde_json::to_vec(&source).unwrap()).unwrap();
        let parsed = ds_cli_contract::parse(
            &COMMAND,
            &[
                "--input".into(),
                input_path.to_string_lossy().into_owned(),
                "--out".into(),
                output_path.to_string_lossy().into_owned(),
            ],
        )
        .unwrap();
        let receipt = run(
            &parsed,
            &Context {
                confirmed: false,
                output: ds_cli_contract::Output::resolve(
                    ds_cli_contract::Format::Json,
                    false,
                    true,
                ),
            },
        )
        .unwrap();
        assert_eq!(receipt["failed"], 0, "{receipt}");
        let result: Value = serde_json::from_slice(&std::fs::read(output_path).unwrap()).unwrap();
        let output = &result["jobs"][0]["output"];
        assert!(
            output["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|warning| warning["code"] == "network_kept_as_drawn"),
            "{}",
            output["warnings"]
        );
        for (layer, supplied) in source["jobs"][0]["gdfs"].as_object().unwrap() {
            let supplied = supplied["features"].as_array().unwrap();
            let kept = output["gdfs"][layer]["features"].as_array().unwrap();
            assert_eq!(kept.len(), supplied.len(), "{layer}");
            for (row, authored) in kept.iter().zip(supplied) {
                assert_eq!(row["id"], authored["id"], "{layer}");
                assert_eq!(row["geometry"], authored["geometry"], "{layer}");
            }
        }
    }
}
