//! One fixed native prepared-city file task; the Solar owner computes and seals it.

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

use crate::{ENGINE_LANE, RUN_TIMEOUT, Solar, SolarTask, capture_engine};

// A local receipt consumption bound; calculation/input limits remain Solar-owned.
const MAX_RESULT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_COMPLETION_BYTES: usize = 16 * 1024;

pub static COMMAND: Command = Command {
    id: "solar.calculate-prepared",
    path: &["solar", "calculate-prepared"],
    contract: 1,
    summary: "Calculate one prepared Solar city offline into a fresh directory.",
    purpose: "Calculate one complete prepared-city request through the Cloud's Solar owner. The request holds the prepared input, chart choice and run id. One engine is captured from the native addon lane, its task schema checked, and the fixed file task run on that immutable path. Offline: no intake, account, project, provider, publication or Desktop. Completion names local files and claims no sync.",
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
            "New directory for task-result.json and artifacts; never overwritten.",
        )
        .required(),
        ENGINE_LANE,
    ],
    output: "Within 16 KiB: engine identity, request, input and result digests, artifact count and bytes, and the task-result.json and artifacts paths. The typed receipt is read within 64 MiB; artifact bytes stay on disk. Development provenance is never invented.",
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
            remedy: "read detail.engine and detail.native; fix the prepared input or choose an absent output directory",
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
    search: &["calculation", "native"],
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

pub(crate) fn run_captured(inputs: &Inputs, engine: Solar) -> Result<Value, Failure> {
    let request = PathBuf::from(inputs.require("request")?);
    let out = PathBuf::from(inputs.require("out")?);
    // The Kernel client checks the addon's catalogue before any process.
    let identity = engine
        .require(SolarTask::CalculatePrepared)
        .map_err(|failure| {
            if failure.code() == ds_cli_exec::solar::TASK_UNSUPPORTED {
                Failure::unavailable(
                    "solar_prepared_schema_unavailable",
                    "the captured Solar engine does not support the prepared calculation task",
                )
                .remedy(
                    "install compatible ds and Solar addon versions with the calculate-prepared native task",
                )
            } else {
                failure
            }
        })?;
    let identity: EngineIdentity = serde_json::from_value(identity.clone()).map_err(|_| {
        Failure::failed(
            "callee_contract_mismatch",
            "solar build-info is not its typed engine identity",
        )
        .remedy("install compatible ds and Solar addon versions")
    })?;
    let reply = engine.run(SolarTask::CalculatePrepared, &request, &out, RUN_TIMEOUT)?;
    if !reply.succeeded() {
        return Err(reply.refusal());
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
