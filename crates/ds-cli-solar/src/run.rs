//! `ds solar run` — run caller-supplied prepared cities, offline.
//!
//! The engine guarantees this performs no intake and no network call of any
//! kind. Its artifact preparation and run phases are deliberately separate:
//! this adapter exposes the offline run phase only, while product preparation
//! remains at the paired desktop/cache boundary. A caller reading
//! `authority: none` and `network: no` on this contract is reading something
//! the engine actually enforces.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{DS_SOLAR, RUN_TIMEOUT};

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
    contract: 2,
    summary: "Run externally prepared Solar artifacts offline.",
    purpose: "\
Executes caller-supplied prepared Solar cities and writes their results, the \
city batch document and any charts into an output directory. It performs no \
intake, portfolio aggregation or network call of any kind. This is the \
headless city-artifact route; for the paired desktop product lifecycle use \
`ds solar run start` after `ds solar prepare`.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
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
    output: "The v1 city batch id/digest after validating the closed batch manifest, plus at most 500 declared city results, reports and optional charts. `more.artifacts_omitted` reports inventory truncation. This adapter does not verify each artifact's bytes and does not run the separate native portfolio command.",
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
            when: "the packaged `ds-solar` sidecar cannot be found",
            remedy: "reinstall the complete ds release, or set DS_SOLAR_BIN for development",
        },
        Refusal {
            code: "prepared_not_found",
            when: "--prepared does not name a directory",
            remedy: "supply a prepared artifact directory produced by ds-solar",
        },
        Refusal {
            code: "invalid_concurrency",
            when: "--concurrency is not a positive whole number",
            remedy: "pass 1 or more; 1 is strictly serial",
        },
        Refusal {
            code: "engine_refused",
            when: "the engine ran and failed",
            remedy: "read detail.engine for the engine's own message",
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
            remedy: "run fewer cities, or raise concurrency",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    availability,
};

fn availability() -> Availability {
    DS_SOLAR.availability()
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let prepared = inputs.require("prepared")?;
    if !Path::new(prepared).is_dir() {
        return Err(Failure::invalid(
            "prepared_not_found",
            format!("`{prepared}` is not a directory of prepared inputs"),
        )
        .remedy("supply a prepared artifact directory produced by ds-solar")
        .next("ds solar run --help"));
    }

    let concurrency = inputs.value("concurrency").unwrap_or("10");
    if concurrency.parse::<usize>().map(|n| n == 0).unwrap_or(true) {
        return Err(Failure::invalid(
            "invalid_concurrency",
            "--concurrency must be a positive whole number",
        )
        .remedy("pass 1 or more; 1 is strictly serial"));
    }

    let cities = inputs.repeated("city");

    let mut args: Vec<OsString> = vec![
        OsString::from("--prepared"),
        OsString::from(prepared),
        OsString::from("--concurrency"),
        OsString::from(concurrency),
    ];
    let out = inputs.require("out")?;
    args.push(OsString::from("--out"));
    args.push(OsString::from(out));
    if let Some(run_id) = inputs.value("run-id") {
        args.push(OsString::from("--run-id"));
        args.push(OsString::from(run_id));
    }
    for city in cities {
        args.push(OsString::from("--city"));
        args.push(OsString::from(city));
    }
    if inputs.switch("charts") {
        args.push(OsString::from("--charts"));
    }

    let completed = DS_SOLAR.call("run", &args, RUN_TIMEOUT)?;
    if !completed.succeeded() {
        return Err(DS_SOLAR.failure_from(&completed, "run"));
    }

    let batch_path = PathBuf::from(out).join("batch.json");
    let batch = read_batch_receipt(&batch_path, inputs.value("run-id"))?;
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
    let mut receipt = json!({
        "prepared": prepared,
        "out": out,
        "cities": if cities.is_empty() { json!("all") } else { json!(cities) },
        "concurrency": concurrency,
        "charts": inputs.switch("charts"),
        "batch_id": batch["batch_id"],
        "batch_digest": batch["batch_digest"],
        "run_id": batch["run"]["run_id"],
        "artifact_count": artifact_count,
        "artifacts": artifacts,
        "engine": summarize(&completed.stdout),
    });
    if artifact_count > MAX_RECEIPT_ARTIFACTS {
        receipt["more"] = json!({
            "artifacts_omitted": artifact_count - MAX_RECEIPT_ARTIFACTS,
            "reason": "bounded_batch_inventory",
            "next": "inspect the engine-owned batch.json in --out for the complete local inventory",
        });
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
        .remedy("install matching ds and ds-solar releases, then rerun into a new output directory")
}

fn validate_batch_receipt(batch: &Value, expected_run_id: Option<&str>) -> Result<(), Failure> {
    let root = batch
        .as_object()
        .ok_or_else(|| invalid_batch_receipt("batch.json root is not an object"))?;
    let allowed_root = [
        "schema_version",
        "batch_id",
        "run",
        "artifacts",
        "state",
        "batch_digest",
    ];
    if root.keys().any(|key| !allowed_root.contains(&key.as_str())) {
        return Err(invalid_batch_receipt(
            "batch.json contains a field outside the v1 batch contract",
        ));
    }
    if batch["schema_version"].as_str() != Some(BATCH_SCHEMA) {
        return Err(invalid_batch_receipt(
            "batch.json does not declare ds-solar.artifact-batch/v1",
        ));
    }
    let batch_digest = batch["batch_digest"]
        .as_str()
        .filter(|digest| crate::exports::valid_sha256_digest(digest))
        .ok_or_else(|| invalid_batch_receipt("batch_digest is not a lowercase SHA-256 digest"))?;
    let batch_id = batch["batch_id"]
        .as_str()
        .filter(|batch_id| {
            batch_id.len() == 32
                && batch_id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| invalid_batch_receipt("batch_id is not 32 lowercase hexadecimal bytes"))?;

    let run = batch["run"]
        .as_object()
        .ok_or_else(|| invalid_batch_receipt("run identity is missing"))?;
    let run_fields = ["project_id", "root", "run_id", "cities"];
    if run.keys().any(|key| !run_fields.contains(&key.as_str())) {
        return Err(invalid_batch_receipt(
            "run identity contains a field outside the v1 batch contract",
        ));
    }
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
    let city_fields = ["city_id", "input_digest", "result_digest"];
    let mut city_ids = BTreeSet::new();
    for city in cities {
        let city = city
            .as_object()
            .ok_or_else(|| invalid_batch_receipt("run.cities contains a non-object"))?;
        if city.keys().any(|key| !city_fields.contains(&key.as_str())) {
            return Err(invalid_batch_receipt(
                "a run city contains a field outside the v1 batch contract",
            ));
        }
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
                .filter(|digest| crate::exports::valid_sha256_digest(digest))
                .ok_or_else(|| invalid_batch_receipt(format!("run.cities.{field} is invalid")))?;
        }
    }

    let artifacts = batch["artifacts"]
        .as_array()
        .filter(|artifacts| artifacts.len() <= MAX_BATCH_ARTIFACTS)
        .ok_or_else(|| invalid_batch_receipt("artifacts is not a bounded array"))?;
    let artifact_fields = [
        "name",
        "role",
        "media_type",
        "byte_count",
        "content_digest",
        "city_id",
    ];
    let mut previous_name: Option<&str> = None;
    for artifact in artifacts {
        let artifact = artifact
            .as_object()
            .ok_or_else(|| invalid_batch_receipt("artifacts contains a non-object"))?;
        if artifact
            .keys()
            .any(|key| !artifact_fields.contains(&key.as_str()))
        {
            return Err(invalid_batch_receipt(
                "an artifact contains a field outside the v1 batch contract",
            ));
        }
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
            .filter(|digest| crate::exports::valid_sha256_digest(digest))
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

    let content = json!({
        "schema_version": BATCH_SCHEMA,
        "run": batch["run"],
        "artifacts": batch["artifacts"],
    });
    let canonical_bytes = serde_json::to_vec(&content)
        .map_err(|_| invalid_batch_receipt("batch content cannot be canonicalised"))?;
    let computed_digest = format!("sha256:{:x}", Sha256::digest(&canonical_bytes));
    if computed_digest != batch_digest {
        return Err(invalid_batch_receipt(
            "batch_digest does not match the closed batch content",
        ));
    }
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
    !value.is_empty()
        && value.len() <= 1_024
        && !value.starts_with(['/', '\\'])
        && !value.contains(['\\', '\0'])
        && !value
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
}

/// The engine's own summary, bounded. The results are documents in `--out`;
/// this is the receipt, not the answer.
fn summarize(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(20)
        .map(|line| line.chars().take(200).collect::<String>())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
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
    for line in data["engine"].as_array().into_iter().flatten() {
        out.push_str(&format!("{}\n", line.as_str().unwrap_or("")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn closed_batch() -> Value {
        let digest = |byte: char| format!("sha256:{}", byte.to_string().repeat(64));
        let run = json!({
            "project_id": "project-1",
            "root": "solar",
            "run_id": "run-1",
            "cities": [{
                "city_id": "city-1",
                "input_digest": digest('a'),
                "result_digest": digest('b'),
            }],
        });
        let artifacts = json!([{
            "name": "cities/city-1/result.json",
            "role": "city_result",
            "media_type": "application/json",
            "byte_count": 42,
            "content_digest": digest('c'),
            "city_id": "city-1",
        }]);
        let content = json!({
            "schema_version": BATCH_SCHEMA,
            "run": run,
            "artifacts": artifacts,
        });
        let digest = format!(
            "sha256:{:x}",
            Sha256::digest(serde_json::to_vec(&content).expect("serialize closed batch content"))
        );
        json!({
            "schema_version": BATCH_SCHEMA,
            "batch_id": &digest[7..39],
            "run": content["run"],
            "artifacts": content["artifacts"],
            "state": {"state": "local_ready"},
            "batch_digest": digest,
        })
    }

    #[test]
    fn closed_batch_manifest_identity_is_recomputed_before_projection() {
        let mut batch = closed_batch();
        validate_batch_receipt(&batch, Some("run-1")).expect("closed receipt is valid");

        batch["artifacts"][0]["byte_count"] = json!(43);
        let failure =
            validate_batch_receipt(&batch, Some("run-1")).expect_err("tampering must fail");
        assert_eq!(failure.code(), "batch_receipt_invalid");
        assert!(failure.message().contains("batch_digest"));
    }

    #[test]
    fn oversized_batch_manifest_is_bounded_at_max_plus_one() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ds-cli-solar-oversized-batch-{}-{unique}.json",
            std::process::id()
        ));
        let file = std::fs::File::create(&path).expect("create sparse batch fixture");
        file.set_len(MAX_BATCH_RECEIPT_BYTES + 2)
            .expect("extend sparse batch fixture");
        drop(file);

        let failure = read_batch_receipt(&path, None).expect_err("oversized receipt must fail");
        assert_eq!(failure.code(), "batch_receipt_oversized");
        assert_eq!(
            failure.detail_value().expect("bounded detail")["observed_at_least"],
            MAX_BATCH_RECEIPT_BYTES + 1
        );
        let _ = std::fs::remove_file(path);
    }
}
