//! `ds solar run` — run caller-supplied prepared cities, offline.
//!
//! The engine guarantees this performs no intake and no network call of any
//! kind. Its artifact preparation and run phases are deliberately separate:
//! this adapter exposes the offline run phase only. Native Server preparation
//! belongs to the Solar application owner. A caller reading
//! `authority: none` and `network: no` on this contract is reading something
//! the engine actually enforces.

use ds_solar_contracts::prepared_batch::{
    PREPARED_BATCH_REQUEST_SCHEMA, PreparedBatchOptions, PreparedBatchProgress,
    PreparedBatchRequest,
};
use ds_solar_io::cancel::CancelToken;
use ds_solar_process::{BatchEngine, ProcessFailure, ProcessFailureKind};
use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::{
    cell::Cell,
    sync::Mutex,
    time::{Duration, Instant},
};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{ENGINE_LANE, RUN_TIMEOUT, capture_engine};
use ds_command_kernel::module_engine::schema_family;

const BATCH_SCHEMA: &str = "ds-solar.artifact-batch/v1";
const MAX_BATCH_RECEIPT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_BATCH_ARTIFACTS: usize = 25_000;
const MAX_RECEIPT_ARTIFACTS: usize = 500;
const ARTIFACT_ROLES: &[&str] = &[
    "report_input",
    "city_result",
    "city_report_model",
    "portfolio_contribution",
    "portfolio_calculation",
    "portfolio_run_input",
    "chart",
    "portfolio_result",
    "export",
    "report",
];

pub static COMMAND: Command = Command {
    id: "solar.run",
    path: &["solar", "run"],
    contract: 3,
    summary: "Run externally prepared Solar artifacts offline.",
    purpose: "\
Executes caller-supplied prepared Solar cities and writes their results, the \
city batch document and any charts into an output directory. It performs no \
intake, portfolio aggregation or network call of any kind. This is the \
headless city-artifact route. For native Server preparation, city/portfolio \
jobs and delivery, discover `ds solar application schema`. Contract 3 admits at most 512 cities, 256 UTF-8 bytes per identity, 64 MiB per prepared input file and 512 MiB for the serialized batch request. Closed outputs are bounded to 64 MiB per file and 4 GiB total.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        ENGINE_LANE,
        Arg::value(
            "prepared",
            "<dir>",
            "Directory of prepared inputs from the ds-solar artifact contract.",
        )
        .required(),
        Arg::value(
            "out",
            "<dir>",
            "Directory to write results, the batch and charts into.",
        )
        .required(),
        Arg::repeated(
            "city",
            "<id>",
            "City context id. Repeat to select; omit to run all.",
        ),
        Arg::value(
            "concurrency",
            "<n>",
            "Cities to run at once; 1 is strictly serial.",
        )
        .default("10"),
        Arg::value(
            "run-id",
            "<id>",
            "Run id echoed into results; excluded from result digests.",
        ),
        Arg::switch("charts", "Render chart artifacts."),
    ],
    output: "The v1 city batch id/digest after validating the closed batch manifest, plus at most 500 declared city results, reports and optional charts. `more.artifacts_omitted` reports inventory truncation. Every artifact is verified before local commitment. `engine` is the selected typed engine identity; engine_path, lane, network:false, request_sha256 and completion_build_manifest_sha256 identify the captured offline task. Failed cities preserve successful local artifacts before engine_refused. This does not run the separate native portfolio command.",
    examples: &[
        Example {
            command: "ds solar run --prepared ./prepared --out ./results --output json",
            note: "Every prepared city, serially written into ./results.",
            runnable: false,
        },
        Example {
            command: "ds solar run --prepared ./prepared --out ./results --city kigali --concurrency 1 --output json",
            note: "One city, strictly serial.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "solar_engine_missing",
            when: "the selected lane has no managed Solar addon and no explicit development override",
            remedy: "install the Solar addon for the selected lane, or explicitly set DS_SOLAR_ENGINE_BIN for development",
        },
        Refusal {
            code: "module_receipt_invalid",
            when: "the selected managed receipt or retained executable fails verification",
            remedy: "repair the selected installation; inspect any completed computation details before explicitly retrying",
        },
        Refusal {
            code: "local_output_failed",
            when: "calculation completed but local artifact commitment failed",
            remedy: "inspect the candidate, destination and detail.completion before retrying; calculation may have completed",
        },
        Refusal {
            code: "prepared_not_found",
            when: "--prepared does not name a directory",
            remedy: "supply a prepared artifact directory produced by ds-solar",
        },
        Refusal {
            code: "prepared_batch_invalid",
            when: "the prepared batch exceeds fixed-task count, identity or serialized request admission before engine selection",
            remedy: "use at most 512 cities, 256 UTF-8 bytes per project/root/city/run identity and a 512 MiB serialized request; split oversized batches explicitly",
        },
        Refusal {
            code: "invalid_concurrency",
            when: "--concurrency is not a positive whole number",
            remedy: "pass 1 or more; 1 is strictly serial",
        },
        Refusal {
            code: "engine_refused",
            when: "local prepared-file validation or the selected native calculation refused",
            remedy: "inspect the typed failure and any detail.completion before explicitly retrying",
        },
        Refusal {
            code: "batch_receipt_missing",
            when: "the engine succeeds without a readable batch.json",
            remedy: "keep --out on a writable local filesystem and retry",
        },
        Refusal {
            code: "batch_receipt_oversized",
            when: "batch.json exceeds the bounded 16 MiB receipt contract",
            remedy: "run fewer cities or charts, then retry into a new output directory",
        },
        Refusal {
            code: "batch_receipt_invalid",
            when: "batch.json is not the closed native batch contract it declares",
            remedy: "install matching ds and ds-solar releases, then rerun into a new output directory",
        },
        Refusal {
            code: "callee_timed_out",
            when: "the batch exceeded the four-hour bound",
            remedy: "inspect any completed local scope before explicitly retrying with fewer cities",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    search: &[],
    requires: Requires::Server,
    availability,
};

fn availability() -> Availability {
    // Discovery cannot select a lane or substitute an unmanaged sibling.
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    run_with_capture(inputs, || capture_engine(inputs), RUN_TIMEOUT)
}

fn timeout_failure() -> Failure {
    Failure::failed(
        "callee_timed_out",
        "Solar run exceeded its four-hour task deadline",
    )
    .remedy("inspect any completed local output before explicitly retrying with fewer cities")
}
fn remaining(deadline: Instant) -> Result<Duration, Failure> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or_else(timeout_failure)
}
fn process_failure(error: ProcessFailure) -> Failure {
    let message = format!("selected Solar task was not accepted: {}", error.message);
    let failure = if error.kind == ProcessFailureKind::Deadline {
        Failure::failed("callee_timed_out", message)
    } else {
        Failure::failed("engine_refused", message)
    };
    failure
        .remedy("inspect the typed failure and any completed scope before explicitly retrying; no engine fallback was attempted")
        .detail(json!({"native": error.refusal, "kind": format!("{:?}",error.kind)}))
}
fn completed_failure(failure: Failure, completion: &Value) -> Failure {
    let mut detail = failure.detail_value().cloned().unwrap_or_else(|| json!({}));
    detail["completion"] = completion.clone();
    failure.detail(detail).remedy("calculation completed; inspect the destination and any retained candidate before explicitly retrying")
}

fn run_with_capture(
    inputs: &Inputs,
    capture: impl FnOnce() -> Result<ds_cli_exec::CapturedExternal, Failure>,
    timeout: Duration,
) -> Result<Value, Failure> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(timeout_failure)?;
    let prepared_path = inputs.require("prepared")?;
    if !Path::new(prepared_path).is_dir() {
        return Err(Failure::invalid(
            "prepared_not_found",
            format!("`{prepared_path}` is not a directory of prepared inputs"),
        )
        .remedy("supply a prepared artifact directory produced by the Solar preparation owner")
        .next("ds solar run --help"));
    }
    let concurrency_text = inputs.value("concurrency").unwrap_or("10");
    let concurrency = concurrency_text
        .parse::<usize>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| {
            Failure::invalid(
                "invalid_concurrency",
                "--concurrency must be a positive whole number",
            )
            .remedy("pass 1 or more; 1 is strictly serial")
        })?;
    let cities = inputs.repeated("city");
    let selected = cities.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
    let prepared =
        ds_solar_project::preparation::load_prepared_for_run(Path::new(prepared_path), &selected)
            .map_err(|error| {
            Failure::failed(
                "engine_refused",
                format!("prepared Solar inputs refused before execution: {error:#}"),
            )
            .remedy("repair the prepared inputs; this offline route does not acquire references")
        })?;
    let out = inputs.require("out")?;
    remaining(deadline)?;
    let request = PreparedBatchRequest {
        schema_version: PREPARED_BATCH_REQUEST_SCHEMA.into(),
        project_id: prepared[0].city.identity.project_id.clone(),
        root: prepared[0].city.identity.root.clone(),
        prepared,
        options: PreparedBatchOptions {
            run_id: inputs.value("run-id").map(str::to_owned),
            render_charts: inputs.switch("charts"),
            generate_documents: true,
            ..Default::default()
        },
        concurrency: Some(concurrency),
    };
    BatchEngine::validate_request(&request).map_err(|error| {
        Failure::invalid("prepared_batch_invalid", format!("prepared batch refused before engine selection: {}", error.message))
            .detail(json!({"native_message":error.message}))
            .remedy("supply a batch within the declared fixed-task identity, count and serialized request limits")
    })?;
    remaining(deadline)?;
    let engine = capture()?;
    let engine_path = engine.executable().to_path_buf();
    let scratch = tempfile::tempdir().map_err(|error| {
        Failure::failed(
            "local_output_failed",
            format!("cannot create private Solar task scratch: {error}"),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o700)).map_err(
            |error| {
                Failure::failed(
                    "local_output_failed",
                    format!("cannot make Solar task scratch private: {error}"),
                )
            },
        )?;
    }
    let cancel = CancelToken::new();
    let events = Mutex::new(Vec::new());
    let progress = |event: PreparedBatchProgress| {
        events
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(event.event)
    };
    let attempted = Cell::new(false);
    let result = engine.with_verified_executable(|path| {
        let task = BatchEngine::capture(path, scratch.path(), ds_solar_io::compute_parallelism(), &cancel).map_err(process_failure)?;
        let timeout = remaining(deadline)?;
        attempted.set(true);
        task.calculate_with_timeout(&request, &cancel, Some(&progress), timeout).map_err(process_failure)
    }).map_err(|failure| {
        if attempted.get() {
            failure.detail(json!({"phase":"completed_task_integrity_check", "engine_path":engine_path, "out":out, "local_output_committed":false}))
                .remedy("the selected task ran but its output was not accepted; inspect this scope and repair the selected executable before explicitly retrying")
        } else { failure }
    })?;
    let verified = result?;
    let completion = json!({
        "out":out, "batch_id":verified.output.batch.batch_id, "batch_digest":verified.output.batch.batch_digest,
        "run_id":verified.output.batch.run.run_id, "engine":verified.engine,
        "engine_path":engine_path, "lane":inputs.value("lane").unwrap_or("stable"),
        "network":false, "request_sha256":verified.request_sha256,
        "completion_build_manifest_sha256":verified.completion_build_manifest_sha256,
    });
    let failures = verified
        .output
        .cities
        .iter()
        .filter_map(|city| {
            city.outcome
                .as_ref()
                .err()
                .map(|error| ds_solar_project::files::RunFailure {
                    city_id: &city.city_id,
                    error,
                })
        })
        .collect::<Vec<_>>();
    let progress = serde_json::to_value(events.into_inner().unwrap_or_else(|p| p.into_inner()))
        .map_err(|error| {
            completed_failure(
                Failure::failed(
                    "local_output_failed",
                    format!("completed progress could not be encoded: {error}"),
                ),
                &completion,
            )
        })?;
    remaining(deadline).map_err(|error| completed_failure(error, &completion))?;
    ds_solar_project::files::commit_artifact_run(
        Path::new(out),
        &verified.output.batch,
        &failures,
        Some(&progress),
        |root| {
            for artifact in &verified.output.batch.artifacts {
                // The extra task-only daily frame is intentionally outside legacy local output.
                remaining(deadline)
                    .map_err(|error| std::io::Error::other(error.message().to_owned()))?;
                let file = verified
                    .file(&artifact.name)
                    .ok_or_else(|| std::io::Error::other("closed Solar artifact is missing"))?;
                file.copy_to(&root.join(&artifact.name), &cancel)?;
            }
            Ok(())
        },
    )
    .map_err(|error| {
        completed_failure(
            Failure::failed(
                "local_output_failed",
                format!(
                    "Solar calculation completed but local artifact commitment failed: {error:#}"
                ),
            ),
            &completion,
        )
    })?;
    if !failures.is_empty() {
        return Err(Failure::failed("engine_refused", format!("{} of {} cities failed; local output was committed",failures.len(),verified.output.cities.len()))
            .detail(json!({"completion":completion,"failed_city_ids":failures.iter().map(|f|f.city_id).collect::<Vec<_>>(),"failure_details":"inspect failures.json in the completed output"}))
            .remedy("inspect the completed local output and failures.json before explicitly retrying failed cities into a new directory"));
    }
    let batch = read_batch_receipt(
        &PathBuf::from(out).join("batch.json"),
        inputs.value("run-id"),
    )
    .map_err(|error| completed_failure(error, &completion))?;
    let artifacts = batch["artifacts"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .take(MAX_RECEIPT_ARTIFACTS)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let artifact_count = batch["artifacts"].as_array().map_or(0, Vec::len);
    let mut receipt = completion;
    receipt["prepared"] = json!(prepared_path);
    receipt["cities"] = if cities.is_empty() {
        json!("all")
    } else {
        json!(cities)
    };
    receipt["concurrency"] = json!(concurrency_text);
    receipt["charts"] = json!(inputs.switch("charts"));
    receipt["artifact_count"] = json!(artifact_count);
    receipt["artifacts"] = json!(artifacts);
    if artifact_count > MAX_RECEIPT_ARTIFACTS {
        receipt["more"] = json!({"artifacts_omitted":artifact_count-MAX_RECEIPT_ARTIFACTS,"reason":"bounded_batch_inventory","next":"inspect the engine-owned batch.json in --out for the complete local inventory"});
    }
    Ok(receipt)
}

fn read_batch_receipt(path: &Path, expected_run_id: Option<&str>) -> Result<Value, Failure> {
    let file = std::fs::File::open(path).map_err(|_| missing_batch_receipt(path))?;
    let mut body = Vec::new();
    file.take(MAX_BATCH_RECEIPT_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|_| missing_batch_receipt(path))?;
    if body.len() as u64 > MAX_BATCH_RECEIPT_BYTES {
        return Err(Failure::failed(
            "batch_receipt_oversized",
            format!(
                "`{}` exceeds the {MAX_BATCH_RECEIPT_BYTES}-byte batch receipt bound",
                path.display()
            ),
        )
        .remedy("run fewer cities or charts, then retry into a new output directory")
        .detail(json!({
            "max_byte_len": MAX_BATCH_RECEIPT_BYTES,
            "observed_at_least": body.len(),
        })));
    }
    let batch: Value = serde_json::from_slice(&body)
        .map_err(|_| invalid_batch_receipt("batch.json is not a valid JSON document"))?;
    validate_batch_receipt(&batch, expected_run_id)?;
    Ok(batch)
}

fn missing_batch_receipt(path: &Path) -> Failure {
    Failure::failed(
        "batch_receipt_missing",
        format!("solar completed without a readable `{}`", path.display()),
    )
    .remedy("keep --out on a writable local filesystem and retry")
}

fn invalid_batch_receipt(detail: impl Into<String>) -> Failure {
    Failure::failed("batch_receipt_invalid", detail)
        .remedy("rerun into a new output directory; a receipt that still lacks a required member needs a ds-solar engine that writes it")
}

/// The engine's batch receipt is a reply (contract 4): members a newer engine
/// adds are accepted and never read, and its schema revision is provenance.
/// Every member read here stays required and typed; `batch_digest` still binds
/// the whole document as written, unknown members included.
fn validate_batch_receipt(batch: &Value, expected_run_id: Option<&str>) -> Result<(), Failure> {
    if !batch.is_object() {
        return Err(invalid_batch_receipt("batch.json root is not an object"));
    }
    if batch["schema_version"].as_str().map(schema_family) != Some(schema_family(BATCH_SCHEMA)) {
        return Err(invalid_batch_receipt(
            "batch.json does not declare the ds-solar.artifact-batch family",
        ));
    }
    let batch_digest = batch["batch_digest"]
        .as_str()
        .filter(|digest| ds_cli_contract::util::is_sha256_digest(digest, ds_cli_contract::util::HexCase::Lower))
        .ok_or_else(|| invalid_batch_receipt("batch_digest is not a lowercase SHA-256 digest"))?;
    let batch_id = batch["batch_id"]
        .as_str()
        .filter(|batch_id| {
            ds_cli_contract::util::is_hex(batch_id, 32, ds_cli_contract::util::HexCase::Lower)
        })
        .ok_or_else(|| invalid_batch_receipt("batch_id is not 32 lowercase hexadecimal bytes"))?;

    let run = batch["run"]
        .as_object()
        .ok_or_else(|| invalid_batch_receipt("run identity is missing"))?;
    for field in ["project_id", "root", "run_id"] {
        run.get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty() && value.len() <= 512)
            .ok_or_else(|| invalid_batch_receipt(format!("run.{field} is invalid")))?;
    }
    if expected_run_id.is_some_and(|expected| run["run_id"].as_str() != Some(expected)) {
        return Err(invalid_batch_receipt(
            "run.run_id does not match the caller-supplied --run-id",
        ));
    }
    let cities = run["cities"]
        .as_array()
        .filter(|cities| cities.len() <= 200)
        .ok_or_else(|| invalid_batch_receipt("run.cities is not a bounded array"))?;
    let mut city_ids = BTreeSet::new();
    for city in cities {
        let city = city
            .as_object()
            .ok_or_else(|| invalid_batch_receipt("run.cities contains a non-object"))?;
        let city_id = valid_logical_segment(city.get("city_id").and_then(Value::as_str))
            .ok_or_else(|| invalid_batch_receipt("run.cities.city_id is invalid"))?;
        if !city_ids.insert(city_id) {
            return Err(invalid_batch_receipt(
                "run.cities contains a duplicate city_id",
            ));
        }
        for field in ["input_digest", "result_digest"] {
            city.get(field)
                .and_then(Value::as_str)
                .filter(|digest| ds_cli_contract::util::is_sha256_digest(digest, ds_cli_contract::util::HexCase::Lower))
                .ok_or_else(|| invalid_batch_receipt(format!("run.cities.{field} is invalid")))?;
        }
    }

    let artifacts = batch["artifacts"]
        .as_array()
        .filter(|artifacts| artifacts.len() <= MAX_BATCH_ARTIFACTS)
        .ok_or_else(|| invalid_batch_receipt("artifacts is not a bounded array"))?;
    let mut previous_name: Option<&str> = None;
    for artifact in artifacts {
        let artifact = artifact
            .as_object()
            .ok_or_else(|| invalid_batch_receipt("artifacts contains a non-object"))?;
        let name = artifact
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| valid_logical_name(name))
            .ok_or_else(|| invalid_batch_receipt("an artifact has an invalid logical name"))?;
        if previous_name.is_some_and(|previous| previous >= name) {
            return Err(invalid_batch_receipt(
                "artifact names are not unique and strictly sorted",
            ));
        }
        previous_name = Some(name);
        let role = artifact
            .get("role")
            .and_then(Value::as_str)
            .filter(|role| ARTIFACT_ROLES.contains(role))
            .ok_or_else(|| invalid_batch_receipt("an artifact role is not declared by v1"))?;
        artifact
            .get("media_type")
            .and_then(Value::as_str)
            .filter(|media_type| !media_type.trim().is_empty() && media_type.len() <= 256)
            .ok_or_else(|| invalid_batch_receipt("an artifact media_type is invalid"))?;
        artifact
            .get("byte_count")
            .and_then(Value::as_u64)
            .filter(|byte_count| *byte_count > 0)
            .ok_or_else(|| invalid_batch_receipt("an artifact byte_count is invalid"))?;
        artifact
            .get("content_digest")
            .and_then(Value::as_str)
            .filter(|digest| ds_cli_contract::util::is_sha256_digest(digest, ds_cli_contract::util::HexCase::Lower))
            .ok_or_else(|| invalid_batch_receipt("an artifact content_digest is invalid"))?;
        let city_id = match artifact.get("city_id") {
            None | Some(Value::Null) => None,
            Some(city_id) => {
                let city_id = valid_logical_segment(city_id.as_str())
                    .ok_or_else(|| invalid_batch_receipt("an artifact city_id is invalid"))?;
                if !city_ids.contains(city_id) {
                    return Err(invalid_batch_receipt(
                        "an artifact city_id is not declared by run.cities",
                    ));
                }
                if !name.starts_with(&format!("cities/{city_id}/")) {
                    return Err(invalid_batch_receipt(
                        "a city artifact is outside its declared city namespace",
                    ));
                }
                Some(city_id)
            }
        };
        if city_id.is_none() && name.starts_with("cities/") {
            return Err(invalid_batch_receipt(
                "an artifact uses a city namespace without declaring city_id",
            ));
        }
        let requires_city = [
            "report_input",
            "city_result",
            "city_report_model",
            "portfolio_contribution",
            "portfolio_calculation",
            "chart",
        ]
        .contains(&role);
        let requires_portfolio = ["portfolio_run_input", "portfolio_result"].contains(&role);
        if requires_city && city_id.is_none() {
            return Err(invalid_batch_receipt(
                "a city-scoped artifact role has no city_id",
            ));
        }
        if requires_portfolio && city_id.is_some() {
            return Err(invalid_batch_receipt(
                "a portfolio-scoped artifact role unexpectedly declares city_id",
            ));
        }
    }

    batch["state"]["state"]
        .as_str()
        .filter(|state| ["local_ready", "uploading", "published", "failed"].contains(state))
        .ok_or_else(|| invalid_batch_receipt("batch state is invalid"))?;

    // The engine owns canonical key order and the digest recipe. In particular,
    // serde_json's preserve_order feature must never change receipt validity.
    let owner: ds_solar_contracts::SolarArtifactBatch = serde_json::from_value(batch.clone())
        .map_err(|_| invalid_batch_receipt("batch.json violates the owner's typed contract"))?;
    owner.validate().map_err(|error| {
        invalid_batch_receipt(format!("batch_digest or content invalid: {error}"))
    })?;
    if batch_id != &batch_digest[7..39] {
        return Err(invalid_batch_receipt(
            "batch_id does not match the digest-derived batch identity",
        ));
    }
    Ok(())
}

fn valid_logical_segment(value: Option<&str>) -> Option<&str> {
    value.filter(|value| {
        !value.trim().is_empty()
            && value.len() <= 256
            && !matches!(*value, "." | "..")
            && !value.contains(['/', '\\', '\0'])
    })
}

fn valid_logical_name(value: &str) -> bool {
    ds_cli_contract::util::safe_relative(value) && value.len() <= 1_024 && !value.contains('\0')
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "prepared {}\ncities   {}\n",
        data["prepared"].as_str().unwrap_or(""),
        data["cities"],
    );
    if let Some(dir) = data["out"].as_str() {
        out.push_str(&format!("out      {dir}\n"));
    }
    out.push_str(&format!(
        "batch    {}\n",
        data["batch_id"].as_str().unwrap_or("")
    ));
    if let Some(artifacts) = data["artifacts"].as_array() {
        out.push_str(&format!(
            "artifacts {}\n",
            data["artifact_count"]
                .as_u64()
                .unwrap_or(artifacts.len() as u64)
        ));
        for artifact in artifacts {
            out.push_str(&format!("  {}\n", artifact["name"].as_str().unwrap_or("")));
        }
        if let Some(omitted) = data["more"]["artifacts_omitted"].as_u64() {
            out.push_str(&format!("  ... {omitted} more declared artifacts\n"));
        }
    }
    out.push('\n');
    out.push_str(&format!(
        "engine   {} {}\n",
        data["engine"]["name"].as_str().unwrap_or(""),
        data["engine"]["source_sha"].as_str().unwrap_or("")
    ));
    out
}

#[cfg(test)]
#[path = "run_receipt_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "run_tests.rs"]
mod adapter_tests;
