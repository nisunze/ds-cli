//! `ds design lv process` — bounded native LV batch compute.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_network_contracts::lv_processing::{NativeLvError, MAX_NATIVE_LV_INPUT_BYTES};
use serde_json::{json, Value};

use super::artifact::{ensure_absent, sha256, write_new, RESULT};

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
        Arg::value(
            "project",
            "<id>",
            "Take --input as the page's LV batch for this project and write its batch output to --out.",
        ),
    ],
    output: "Input/result SHA-256, output path and bytes, captured executable SHA-256, engine identity and development-override status, runtime, worker count and ordered job statuses with preservation counts. A job that would change approved rows keeps them exactly as supplied, still returns its voltage-drop analysis, and names the withheld drafting in an approved_network_kept warning. The setting keep_network_as_drawn keeps every supplied row exactly as drawn (network_kept_as_drawn), so a supplied design saves without redrawing; renumber_lines=false keeps supplied line numbers. Full layers and diagnostics go to --out.",
    examples: &[Example {
        command: "ds design lv process --input ./lv-request.json --out ./lv-result.json --output json",
        note: "Run the closed local batch without a Desktop session or project identity.",
        runnable: false,
    }],
    refusals: &[
        Refusal {code:"network_engine_missing",when:"the compatible native Network engine is absent",remedy:"install the matching Network engine addon"},
        Refusal {code:"network_engine_incompatible",when:"the captured Network executable lacks the required protocol or capability, including its LV processor catalogue (lv_processor_catalog_v1)",remedy:"install the compatible Network engine"},
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
            remedy: "pass a readable ds.lv.process.request/v1 or ds.fast-lv.request/v1 file",
        },
        Refusal {
            code: "fast_lv_input_too_large",
            when: "the request file exceeds 64 MiB",
            remedy: "split the work into smaller closed batches",
        },
        Refusal {
            code: "fast_lv_input_invalid",
            when: "the request is malformed, has unknown fields/settings, is a processed result document, or a layer is not a FeatureCollection",
            remedy: "write the exact LV processing request shape shown in the design reference; give the request, never a result file",
        },
        Refusal {
            code: "fast_lv_schema_unsupported",
            when: "the request schema is neither ds.lv.process.request/v1 nor ds.fast-lv.request/v1",
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
        super::catalogue::SETTING_NOT_OFFERED,
        super::catalogue::CATALOGUE_INVALID,
        Refusal {
            code: "lv_process_all_jobs_failed",
            when: "every job of the batch failed, e.g. a request exported without --project-config; the result document is still written",
            remedy: "read detail.results for each error; for an outdated configuration, export again with design lv project-export --project-config",
        },
    ],
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: || ds_network_client::available(),
};

/// The command: the batch receipt, or a failure when no job of a plain batch
/// succeeded. The page batch (`--project`) answers its host through `--out`
/// alone, so it keeps the receipt.
pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let receipt = run_receipt(inputs, context)?;
    let jobs = receipt["jobs"].as_u64().unwrap_or(0);
    if inputs.value("project").is_none() && jobs > 0 && receipt["failed"].as_u64() == Some(jobs) {
        return Err(all_jobs_failed(&receipt));
    }
    Ok(receipt)
}

/// The batch receipt whatever the job outcomes; `design lv project-run` judges
/// its single job itself.
pub(super) fn run_receipt(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let input_path = PathBuf::from(inputs.require("input")?);
    let output_path = PathBuf::from(inputs.require("out")?);
    ensure_absent(&output_path, &RESULT)?;

    let input = bounded_read(&input_path)?;
    let input_sha256 = sha256(&input);
    // A page batch names its project: Core makes it the native request here and
    // answers the page's own batch output, so no host converts either side.
    let project = inputs.value("project");
    let captured = project
        .map(|project| ds_command_kernel::design_lv::capture_bytes(project, &input))
        .transpose()
        .map_err(map_application_error)?;
    let request = match &captured {
        Some(captured) => captured.native_request().map_err(map_application_error)?,
        None => input,
    };
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
        &request,
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
                "error": job.error,
                "preservation": job.preservation,
            })
        })
        .collect::<Vec<_>>();
    let engine_core_version = result.engine_core_version.clone();
    let output = match captured {
        Some(captured) => serde_json::to_vec(
            &captured
                .response_from_native(result)
                .map_err(map_application_error)?,
        )
        .map_err(|error| {
            Failure::internal(
                "fast_lv_result_encoding_failed",
                format!("Could not encode the page batch output: {error}"),
            )
        })?,
        None => outcome.bytes,
    };
    let result_sha256 = sha256(&output);
    write_new(&output_path, &output, &RESULT)?;

    let receipt = json!({
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
        "warning_count": failed,
    });
    Ok(receipt)
}

/// Every job of the batch failed: the result document was written, but no
/// transformer was processed, so the command must not exit as a success.
fn all_jobs_failed(receipt: &Value) -> Failure {
    let results = receipt["results"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let first = results
        .iter()
        .find_map(|row| row["error_code"].as_str().or_else(|| row["error"].as_str()));
    let outdated = results.iter().any(|row| {
        row["error"].as_str().is_some_and(|error| {
            error.contains("project_settings") || error.contains("network_config")
        })
    });
    let remedy = if outdated {
        "export the request again with design lv project-export --project-config, then process the new file"
    } else {
        "read detail.results for each transformer's error and the result document at detail.out, correct the request, and process it again at a new output path"
    };
    Failure::failed(
        "lv_process_all_jobs_failed",
        format!(
            "all {} job(s) of the LV batch failed{}; the result document was still written",
            receipt["jobs"],
            first
                .map(|error| format!(" (first error: {error})"))
                .unwrap_or_default(),
        ),
    )
    .remedy(remedy)
    .detail(json!({
        "out": receipt["out"],
        "result_sha256": receipt["result_sha256"],
        "jobs": receipt["jobs"],
        "failed": receipt["failed"],
        "results": receipt["results"],
    }))
}

fn map_application_error(error: ds_command_kernel::envelope::CommandRefusal) -> Failure {
    error.into_failure().unwrap_or_else(|_| {
        Failure::internal(
            "fast_lv_result_encoding_failed",
            "The LV application owner returned a malformed refusal.",
        )
    })
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
        "fast_lv_publication_invalid" => Failure::invalid(code, message)
            .remedy("Process the unchanged fenced source again and inspect every job outcome."),
        _ => Failure::internal("fast_lv_result_encoding_failed", message),
    }
}
pub(crate) fn map_network_error(error: ds_network_client::NetworkFailure) -> Failure {
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
        // The installed engine's catalogue does not offer what the request asks.
        "lv_process_setting_not_offered" | "lv_process_preset_not_offered" => {
            Failure::invalid(code, message)
        }
        "lv_processor_catalog_invalid" => Failure::failed(code, message),
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
#[path = "process_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "process_exchange_tests.rs"]
mod exchange_tests;
