//! Shared native Grid transport; no model decode or engineering decisions.
use ds_cli_contract::{Failure, spec::Refusal};
use ds_network_client::{CapturedNetworkClient, GridOperationOutcome, NetworkFailure};
use ds_network_contracts::grid_operation::{
    GRID_OPERATION_REFUSAL_SCHEMA_V1, GridOperationRefusalClassV1, GridOperationRefusalV1,
    GridOperationV1,
};
use std::path::PathBuf;

pub fn capture() -> Result<CapturedNetworkClient, Failure> {
    capture_for_release_lane(ds_cli_auth::reliability::release_lane())
}

fn capture_for_release_lane(lane: &str) -> Result<CapturedNetworkClient, Failure> {
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    // The packaged runtime owns the lane; no account, project or ambient
    // selection may redirect an installed Stable/Canary build.
    match lane {
        "local" => CapturedNetworkClient::capture(workers, &|| false),
        _ => CapturedNetworkClient::capture_for_lane(lane, workers, &|| false),
    }
    .map_err(map_failure)
}

pub fn scratch() -> Result<tempfile::TempDir, Failure> {
    let parent = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|path| {
            path.is_absolute() && !path.starts_with("/tmp") && !path.starts_with("/dev/shm")
        })
        .or_else(|| {
            ds_cli_contract::util::app_data_dir("com.dsplatform.griddesign")
                .map(|path| path.join("network-scratch"))
        })
        .ok_or_else(|| {
            Failure::failed(
                "network_scratch_invalid",
                "No disk-backed application staging path is available.",
            )
        })?;
    std::fs::create_dir_all(&parent)
        .map_err(|error| Failure::failed("network_scratch_invalid", error.to_string()))?;
    tempfile::Builder::new()
        .prefix("grid-query-")
        .tempdir_in(parent)
        .map_err(|error| Failure::failed("network_scratch_invalid", error.to_string()))
}

/// Run one closed Grid file operation through an already captured addon.
pub(crate) fn operation_with(
    client: &CapturedNetworkClient,
    call: &GridOperationV1,
    model: Option<&[u8]>,
    inputs: &[(String, Vec<u8>)],
) -> Result<GridOperationOutcome, Failure> {
    let scratch = scratch()?;
    client
        .grid_operation(call, model, inputs, scratch.path(), &|| false)
        .map_err(operation_failure)
}

/// Capture the lane's addon and run one closed Grid file operation.
pub(crate) fn operation(
    call: &GridOperationV1,
    model: Option<&[u8]>,
    inputs: &[(String, Vec<u8>)],
) -> Result<GridOperationOutcome, Failure> {
    operation_with(&capture()?, call, model, inputs)
}

/// An owner refusal from a closed operation keeps its exit class, follow-up
/// commands and detail exactly as the linked command raised them.
pub(crate) fn operation_failure(error: NetworkFailure) -> Failure {
    let envelope = error
        .owner_detail
        .as_deref()
        .and_then(|detail| serde_json::from_value::<GridOperationRefusalV1>(detail.clone()).ok())
        .filter(|envelope| envelope.schema == GRID_OPERATION_REFUSAL_SCHEMA_V1);
    let Some(envelope) = envelope else {
        return map_failure(error);
    };
    let mut code = error.owner_code().unwrap_or(error.code()).to_owned();
    let message = error.message.clone();
    let mut class = envelope.class;
    if is_package_refusal(&code) {
        code = "package_decode_failed".to_owned();
        class = GridOperationRefusalClassV1::Invalid;
    }
    let stale = is_stale_conflict(&code);
    let mut failure = match class {
        GridOperationRefusalClassV1::Invalid => Failure::invalid(code, message),
        GridOperationRefusalClassV1::Failed => Failure::failed(code, message),
        GridOperationRefusalClassV1::Conflict => Failure::conflict(code, message),
        GridOperationRefusalClassV1::Unavailable => Failure::unavailable(code, message),
        GridOperationRefusalClassV1::Internal => Failure::internal(code, message),
    };
    if let Some(remedy) = error.owner_remedy {
        failure = failure.remedy(remedy.into_string());
    }
    for next in envelope.next {
        failure = failure.next(next);
    }
    if let Some(detail) = envelope.detail {
        failure = failure.detail(detail);
    }
    if stale {
        failure = failure.not_retryable();
    }
    failure
}

/// Restate the owner's path-free package refusal with the path the host read.
pub(crate) fn at_path(failure: Failure, path: &str) -> Failure {
    match failure.code() {
        "package_decode_failed" => failure.with_message(format!("`{path}` did not decode")),
        _ => failure,
    }
}

pub fn map_failure(error: NetworkFailure) -> Failure {
    let code = error
        .owner_code()
        .unwrap_or_else(|| error.code())
        .to_owned();
    let message = error.to_string();
    let stale = is_stale_conflict(&code);
    let mut failure = if matches!(code.as_str(), "network_engine_missing" | "module_removed") {
        Failure::unavailable(&code, message)
    } else if is_package_refusal(&code) {
        // A corrupt user file is the caller's input, not a failed run.
        Failure::invalid("package_decode_failed", message)
    } else if stale {
        Failure::conflict(&code, message)
    } else if error.owner_invalid_input == Some(true) {
        Failure::invalid(&code, message)
    } else {
        Failure::failed(&code, message)
    };
    if let Some(remedy) = error.owner_remedy {
        failure = failure.remedy(remedy.into_string());
    } else if let Some(refusal) = REFUSALS.iter().find(|refusal| refusal.code == code) {
        failure = failure.remedy(refusal.remedy);
    }
    if let Some(detail) = error.owner_detail {
        failure = failure.detail(*detail);
    }
    if stale {
        failure = failure.not_retryable();
    }
    failure
}

/// The owner's refusal of a package it cannot read. Whatever door raised it,
/// the caller sees one code and the invalid-input class: the file is the
/// caller's, so no feedback is warranted.
fn is_package_refusal(code: &str) -> bool {
    matches!(code, "package_refused" | "package_decode_failed")
}

/// A stale pin: the caller's view of the model is out of date. The remedy is
/// to re-read and decide again, so repeating the call unchanged is never it.
fn is_stale_conflict(code: &str) -> bool {
    matches!(
        code,
        "revision_conflict"
            | "command_replay_conflict"
            | "engineering_input_root_mismatch"
            | "revision_mismatch"
    )
}

pub const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "module_removed",
        when: "Network was explicitly removed from the selected release lane",
        remedy: "explicitly reinstall Network in the same lane before processing",
    },
    Refusal {
        code: "network_engine_missing",
        when: "the Network addon executable is absent",
        remedy: "install Network or set DS_NETWORK_ENGINE_BIN to a compatible executable",
    },
    Refusal {
        code: "network_engine_incompatible",
        when: "the captured Network protocol or capability is incompatible",
        remedy: "select a compatible Network addon",
    },
    Refusal {
        code: "network_engine_path_invalid",
        when: "the selected executable path is invalid",
        remedy: "use an absolute ordinary executable",
    },
    Refusal {
        code: "network_engine_invalid",
        when: "the executable cannot be admitted",
        remedy: "install an intact compatible addon",
    },
    Refusal {
        code: "network_engine_unqualified",
        when: "the selected component has no qualified identity",
        remedy: "install a verified release; development overrides remain explicit",
    },
    Refusal {
        code: "network_engine_identity_invalid",
        when: "build-info is malformed or unbounded",
        remedy: "install a matching Network release",
    },
    Refusal {
        code: "network_engine_changed",
        when: "the captured executable changed during execution",
        remedy: "retain immutable version directories and retry unchanged input",
    },
    Refusal {
        code: "network_engine_refused",
        when: "the engine refuses its fixed task contract",
        remedy: "read the owning refusal and use a compatible component",
    },
    Refusal {
        code: "network_scratch_invalid",
        when: "private disk staging cannot be admitted",
        remedy: "provide a writable ordinary disk-backed cache directory",
    },
    Refusal {
        code: "network_request_io",
        when: "the captured request cannot be staged",
        remedy: "provide writable disk space",
    },
    Refusal {
        code: "network_input_changed",
        when: "a staged input changed during execution",
        remedy: "retry from retained unchanged inputs",
    },
    Refusal {
        code: "network_receipt_bounded",
        when: "the terminal receipt exceeds its bound",
        remedy: "use a compatible bounded Network task",
    },
    Refusal {
        code: "network_receipt_invalid",
        when: "the receipt differs from captured inputs or identity",
        remedy: "install a matching intact component",
    },
    Refusal {
        code: "network_refusal_invalid",
        when: "the engine refusal has an incompatible identity or shape",
        remedy: "use a compatible component",
    },
    Refusal {
        code: "network_result_invalid",
        when: "the complete artifact or progress does not match its contract",
        remedy: "use an intact compatible addon and unchanged inputs",
    },
    Refusal {
        code: "network_result_digest_mismatch",
        when: "the returned artifact digest or size differs from the receipt",
        remedy: "retry unchanged inputs using an intact component",
    },
    Refusal {
        code: "network_workers_invalid",
        when: "the worker budget cannot be admitted",
        remedy: "use a positive bounded worker allocation",
    },
    Refusal {
        code: "network_task_cancelled",
        when: "the owning task was cancelled",
        remedy: "retry the unchanged input explicitly",
    },
    Refusal {
        code: "network_control_unavailable",
        when: "the task cancellation channel cannot be opened",
        remedy: "use a compatible native supervision host",
    },
    Refusal {
        code: "network_control_invalid",
        when: "the typed cancellation channel is invalid",
        remedy: "use matching host and component contracts",
    },
    Refusal {
        code: "network_input_invalid",
        when: "a fixed native task input cannot be admitted",
        remedy: "use matching prepared inputs",
    },
    Refusal {
        code: "network_output_invalid",
        when: "the fixed output directory cannot be admitted",
        remedy: "provide writable private disk space",
    },
    Refusal {
        code: "network_artifact_io",
        when: "the engine cannot write its complete artifact",
        remedy: "provide writable private disk space",
    },
    Refusal {
        code: "callee_cancelled",
        when: "the host cancels the supervised child",
        remedy: "retry unchanged input explicitly",
    },
    Refusal {
        code: "callee_timed_out",
        when: "the identity probe or task exceeds its deadline",
        remedy: "split large work or select a compatible addon",
    },
    Refusal {
        code: "callee_wait_failed",
        when: "the host cannot reap its owned process",
        remedy: "inspect host process availability",
    },
    Refusal {
        code: "callee_supervision_failed",
        when: "process-tree supervision cannot be established",
        remedy: "use a host supporting the admitted native boundary",
    },
    Refusal {
        code: "callee_stdin_failed",
        when: "the child cannot receive its fixed control input",
        remedy: "use an intact compatible component",
    },
    Refusal {
        code: "grid_task_bound_refused",
        when: "the opaque model or complete native artifact exceeds its bound",
        remedy: "use one bounded model or operation",
    },
    Refusal {
        code: "grid_task_input_invalid",
        when: "the fixed native Grid input contract is invalid",
        remedy: "use the admitted prepared Grid contract",
    },
    Refusal {
        code: "grid_task_schema_unsupported",
        when: "the fixed Grid schema is incompatible",
        remedy: "use matching host and Network contracts",
    },
    Refusal {
        code: "grid_query_schema_unsupported",
        when: "the fixed Grid query schema is incompatible",
        remedy: "use matching host and Network contracts",
    },
    Refusal {
        code: "grid_task_source_digest_mismatch",
        when: "the task source differs from the captured package",
        remedy: "retry from the exact unchanged source",
    },
    Refusal {
        code: "grid_task_engineering_root_mismatch",
        when: "the requested engineering input root differs",
        remedy: "capture the current authored source explicitly",
    },
    Refusal {
        code: "stale_revision",
        when: "the explicit authored revision fence differs",
        remedy: "inspect the intended source revision before retrying",
    },
    Refusal {
        code: "grid_operation_request_invalid",
        when: "the closed Grid file operation or its staged inputs do not match this addon's contract",
        remedy: "use matching host and Network releases",
    },
    Refusal {
        code: "serialize_failed",
        when: "Network cannot encode its complete result",
        remedy: "retain the unchanged source and report the owning encoding failure",
    },
];
pub const fn extend_refusals<const N: usize>(base: &[Refusal]) -> [Refusal; N] {
    assert!(N == base.len() + REFUSALS.len());
    let mut result = [REFUSALS[0]; N];
    let mut index = 0;
    while index < base.len() {
        result[index] = base[index];
        index += 1;
    }
    let mut appended = 0;
    while appended < REFUSALS.len() {
        result[index] = REFUSALS[appended];
        index += 1;
        appended += 1;
    }
    result
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
#[path = "../tests/support/native_admission.rs"]
mod admission_tests;
