//! `ds solar run` — run caller-supplied prepared cities, offline.
//!
//! The engine guarantees this performs no intake and no network call of any
//! kind. Its artifact preparation and run phases are deliberately separate:
//! this adapter exposes the offline run phase only, while product preparation
//! remains at the paired desktop/cache boundary. A caller reading
//! `authority: none` and `network: no` on this contract is reading something
//! the engine actually enforces.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DS_SOLAR, RUN_TIMEOUT};

pub static COMMAND: Command = Command {
    id: "solar.run",
    path: &["solar", "run"],
    contract: 1,
    summary: "Run externally prepared Solar artifacts offline.",
    purpose: "\
Executes a caller-supplied prepared Solar batch and writes results, the batch \
document and any charts into an output directory. It performs no intake and no \
network call of any kind. This is the headless artifact route; for the paired \
desktop product lifecycle use `ds solar run start` after `ds solar prepare`.",
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
    output: "The verified batch identity and bounded artifact inventory, including city APDs and the portfolio result and draft.",
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
            remedy: "reinstall DS GridDesign, or set DS_SOLAR_BIN for development",
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
    let batch: Value = std::fs::read(&batch_path)
        .ok()
        .and_then(|body| serde_json::from_slice(&body).ok())
        .ok_or_else(|| {
            Failure::failed(
                "batch_receipt_missing",
                format!(
                    "solar completed without a readable `{}`",
                    batch_path.display()
                ),
            )
        })?;
    let artifacts = batch["artifacts"]
        .as_array()
        .map(|items| items.iter().take(500).cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let artifact_count = batch["artifacts"].as_array().map_or(0, Vec::len);
    Ok(json!({
        "prepared": prepared,
        "out": out,
        "cities": if cities.is_empty() { json!("all") } else { json!(cities) },
        "concurrency": concurrency,
        "charts": inputs.switch("charts"),
        "batch_id": batch["batch_id"],
        "batch_digest": batch["batch_digest"],
        "artifact_count": artifact_count,
        "artifacts": artifacts,
        "artifacts_truncated": artifact_count > 500,
        "engine": summarize(&completed.stdout),
    }))
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
        out.push_str(&format!("artifacts {}\n", artifacts.len()));
        for artifact in artifacts {
            out.push_str(&format!("  {}\n", artifact["name"].as_str().unwrap_or("")));
        }
    }
    out.push('\n');
    for line in data["engine"].as_array().into_iter().flatten() {
        out.push_str(&format!("{}\n", line.as_str().unwrap_or("")));
    }
    out
}
