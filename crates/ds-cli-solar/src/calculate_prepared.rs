//! One fixed native prepared-city file task; the Solar owner computes and seals it.

use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_solar_contracts::{
    EngineIdentity, PREPARED_CALCULATION_TASK_SCHEMA, PreparedCalculationTaskResult,
};
use serde_json::{Value, json};

use crate::{DISCOVERY_TIMEOUT, ENGINE_LANE, RUN_TIMEOUT, SOLAR_ENGINE, capture_engine};

// A local receipt consumption bound; calculation/input limits remain Solar-owned.
const MAX_RESULT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_COMPLETION_BYTES: usize = 16 * 1024;

pub static COMMAND: Command = Command {
    id: "solar.calculate-prepared",
    path: &["solar", "calculate-prepared"],
    contract: 1,
    summary: "Calculate one prepared Solar city offline into a fresh directory.",
    purpose: "Calculate one complete prepared-city request through the same Solar owner as Cloud. The request owns prepared input, chart choice and run id. Capture one engine from the declared native addon lane, require its task schema, and execute the fixed file task using that same immutable path. Offline only: no intake, account/project read, project API, provider, publication or Desktop. Completion names local files and makes no sync claim.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "request",
            "<file>",
            "Native PreparedCalculationRequest JSON containing the complete prepared city.",
        )
        .required(),
        Arg::value(
            "out",
            "<fresh-dir>",
            "Absent directory for the native task-result.json and artifacts; never overwritten.",
        )
        .required(),
        ENGINE_LANE,
    ],
    output: "Completion within 16 KiB: captured engine identity, request/input/result digests, artifact count/bytes and task-result.json/artifacts paths. The native typed receipt is read within 64 MiB; artifact bytes stay on disk and are not re-verified by CLI. Development provenance is never invented.",
    examples: &[Example {
        command: "ds solar calculate-prepared --request ./prepared-request.json --out ./calculation --output json",
        note: "One complete prepared request, with no network acquisition or publication.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "module_receipt_invalid",
            when: "the selected lane's managed addon receipt or executable bytes are invalid",
            remedy: "retain the selected lane; repair or roll back its verified module installation",
        },
        Refusal {
            code: "solar_engine_missing",
            when: "the Solar addon cannot be found",
            remedy: "install the Solar addon, or set DS_SOLAR_ENGINE_BIN to a compatible development solar executable",
        },
        Refusal {
            code: "solar_prepared_schema_unavailable",
            when: "the captured engine does not name ds-solar-engine and advertise the prepared-calculation task schema",
            remedy: "install compatible ds and Solar addon versions with the calculate-prepared native task",
        },
        Refusal {
            code: "solar_prepared_result_invalid",
            when: "the native file result is missing, unsafe, oversized, invalid, or stamped by another schema/build",
            remedy: "retain output; install compatible ds and Solar addon, then retry into a fresh directory",
        },
        Refusal {
            code: "engine_refused",
            when: "the native engine refuses the request or fresh output directory",
            remedy: "read detail.engine and detail.native for the owner's refusal; correct the prepared input or choose an absent output directory",
        },
        Refusal {
            code: "callee_contract_mismatch",
            when: "the captured engine returns invalid, excessive or incompatible JSON",
            remedy: "install compatible ds and Solar addon versions",
        },
        Refusal {
            code: "callee_timed_out",
            when: "the native calculation exceeds four hours",
            remedy: "inspect the native task and its output before retrying; interrupted staging is not a completed result",
        },
        Refusal {
            code: "callee_wait_failed",
            when: "the native task process cannot be observed",
            remedy: "inspect the local process environment before retrying",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    search: &["prepared", "offline", "calculation", "native"],
    requires: Requires::Server,
    availability,
};

fn availability() -> Availability {
    // A missing unmanaged sibling must not hide a valid managed selection.
    Availability::Available
}

fn incompatible_result() -> Failure {
    Failure::failed(
        "solar_prepared_result_invalid",
        "native prepared calculation did not produce its declared bounded file result",
    )
    .remedy(
        "retain output; install compatible ds and Solar addon, then retry into a fresh directory",
    )
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    run_captured(inputs, capture_engine(inputs)?)
}

pub(crate) fn run_captured(
    inputs: &Inputs,
    engine: ds_cli_exec::CapturedExternal,
) -> Result<Value, Failure> {
    let request = inputs.require("request")?;
    let out = PathBuf::from(inputs.require("out")?);
    let identity = engine.call_json("build-info", &[], DISCOVERY_TIMEOUT)?;
    let identity: EngineIdentity = serde_json::from_value(identity).map_err(|_| {
        Failure::failed(
            "callee_contract_mismatch",
            "solar build-info is not its typed engine identity",
        )
        .remedy("install compatible ds and Solar addon versions")
    })?;
    if identity.name != "ds-solar-engine"
        || !identity
            .schemas
            .iter()
            .any(|schema| schema == PREPARED_CALCULATION_TASK_SCHEMA)
    {
        return Err(Failure::unavailable(
            "solar_prepared_schema_unavailable",
            "the captured Solar engine does not support the prepared calculation task",
        )
        .remedy(
            "install compatible ds and Solar addon versions with the calculate-prepared native task",
        ));
    }
    let args = [
        OsString::from("--request"),
        OsString::from(request),
        OsString::from("--out"),
        out.as_os_str().to_owned(),
    ];
    let completed = engine.call("calculate-prepared", &args, RUN_TIMEOUT)?;
    if !completed.succeeded() {
        let mut failure = SOLAR_ENGINE.failure_from(&completed, "calculate-prepared");
        // The native task writes typed refusal JSON on stdout. Keep it bounded,
        // without losing the standard bounded stderr evidence.
        if !completed.truncated
            && completed.stdout.len() <= 8192
            && let Ok(native) = serde_json::from_str::<Value>(&completed.stdout)
            && native["status"] == "error"
        {
            let mut detail = failure.detail_value().cloned().unwrap_or_else(|| json!({}));
            detail["native"] = native["error"].clone();
            failure = failure.detail(detail);
        }
        return Err(failure);
    }
    let result_path = out.join("task-result.json");
    let metadata = std::fs::symlink_metadata(&result_path).map_err(|_| incompatible_result())?;
    if !metadata.is_file() || metadata.len() > MAX_RESULT_BYTES {
        return Err(incompatible_result());
    }
    let mut bytes = Vec::new();
    File::open(&result_path)
        .map_err(|_| incompatible_result())?
        .take(MAX_RESULT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| incompatible_result())?;
    if bytes.len() as u64 > MAX_RESULT_BYTES {
        return Err(incompatible_result());
    }
    let result: PreparedCalculationTaskResult =
        serde_json::from_slice(&bytes).map_err(|_| incompatible_result())?;
    if result.schema_version != PREPARED_CALCULATION_TASK_SCHEMA || result.engine != identity {
        return Err(incompatible_result());
    }
    let artifact_bytes = result
        .outputs
        .iter()
        .try_fold(0_u64, |sum, output| sum.checked_add(output.byte_count))
        .ok_or_else(incompatible_result)?;
    let completion = json!({
        "schema_version": result.schema_version,
        "engine": identity,
        "out": out, "result_file": result_path, "artifacts_dir": out.join("artifacts"),
        "request_sha256": result.request_sha256,
        "prepared_input_digest": result.prepared_input_digest,
        "result_digest": result.result.result_digest,
        "plots_generated": result.plots_generated,
        "artifact_count": result.outputs.len(), "artifact_bytes": artifact_bytes,
        "network": false,
    });
    if serde_json::to_vec(&completion)
        .map_err(|_| incompatible_result())?
        .len()
        > MAX_COMPLETION_BYTES
    {
        return Err(incompatible_result());
    }
    Ok(completion)
}

pub fn render(data: &Value) -> String {
    format!(
        "Prepared Solar calculation completed\n  result: {}\n  artifacts: {} ({} bytes)\n  {}",
        data["result_file"].as_str().unwrap_or("?"),
        data["artifact_count"],
        data["artifact_bytes"],
        data["artifacts_dir"].as_str().unwrap_or("?")
    )
}
