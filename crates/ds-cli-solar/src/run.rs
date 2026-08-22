//! `ds solar run` — run prepared cities, offline.
//!
//! The engine guarantees this performs no intake and no network call of any
//! kind. That guarantee is the reason `prepare` and `run` are separate
//! commands here rather than phases of one: a caller reading
//! `authority: none` and `network: no` on this contract is reading something
//! the engine actually enforces.

use std::ffi::OsString;
use std::path::Path;

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
    summary: "Run prepared solar cities. Never touches the network.",
    purpose: "\
Executes a prepared solar batch and writes results, the batch document and any \
charts into an output directory. It performs no intake and no network call of \
any kind — it accepts only inputs `ds solar prepare` already committed. A long \
run is compute, never a stalled request.",
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "prepared",
            "<dir>",
            "Directory of prepared inputs, as written by prepare.",
        )
        .required(),
        Arg::value(
            "out",
            "<dir>",
            "Directory to write results, the batch and charts into.",
        ),
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
    output: "\
The output directory, the cities selected, and the engine's own summary lines. \
The results themselves are documents in --out; they are not inlined.",
    examples: &[
        Example {
            command: "ds solar run --prepared ./prepared --out ./results --output json",
            note: "Every prepared city, serially written into ./results.",
            runnable: false,
        },
        Example {
            command: "ds solar run --prepared ./prepared --city kigali --concurrency 1 --output json",
            note: "One city, strictly serial.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "solar_engine_missing",
            when: "`ds-solar` cannot be found; it is not a bundled sidecar",
            remedy: "set DS_SOLAR_BIN to a built ds-solar; see docs/reference/solar.md",
        },
        Refusal {
            code: "prepared_not_found",
            when: "--prepared does not name a directory",
            remedy: "run `ds solar prepare --out <dir>` first",
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
        .remedy("run `ds solar prepare --out <dir>` first")
        .next("ds solar prepare --help"));
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
    if let Some(out) = inputs.value("out") {
        args.push(OsString::from("--out"));
        args.push(OsString::from(out));
    }
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

    Ok(json!({
        "prepared": prepared,
        "out": inputs.value("out"),
        "cities": if cities.is_empty() { json!("all") } else { json!(cities) },
        "concurrency": concurrency,
        "charts": inputs.switch("charts"),
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
    out.push('\n');
    for line in data["engine"].as_array().into_iter().flatten() {
        out.push_str(&format!("{}\n", line.as_str().unwrap_or("")));
    }
    out
}
