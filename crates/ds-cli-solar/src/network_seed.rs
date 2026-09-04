//! `ds solar seed network-plan` — local reporter → Solar projection.
//!
//! This command is intentionally declarative. It names the reporter's fixed
//! task and returns that task's document; all validation, grouping, and
//! calculation remain in the shared Rust command kernel used by WASM.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "solar.seed.network-plan",
    path: &["solar", "seed", "network-plan"],
    contract: 1,
    summary: "Plan Solar city network documents from tagged MV lengths and local LV rooms.",
    purpose: "Calls the reporter's closed local task, which delegates unchanged request bytes to the same Rust command-kernel operation used by the UI. MV supplies alignment lengths only; LV rooms supply transformers, customers and LV lengths; MV structure counts are refused.",
    chapter: Chapter::Solar,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "request",
            "<path>",
            "Typed JSON from `ds report tasks --task plan_solar_network_seed`.",
        )
        .required(),
        Arg::value(
            "result",
            "<path>",
            "Keep the immutable plan here; must not already exist.",
        ),
    ],
    output: "The deterministic local seed plan, source receipt, city summaries, and complete Solar city documents; mutated is false.",
    examples: &[Example {
        command: "ds solar seed network-plan --request ./ader-loc7-network-seed.json --result ./ader-loc7-network-plan.json --output json",
        note: "Plan offline writes without Firestore; publish the returned documents separately.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "reporter_engine_missing",
            when: "the packaged ds-report process is unavailable",
            remedy: "install the complete ds release, or set DS_REPORT_BIN for development",
        },
        Refusal {
            code: "request_not_found",
            when: "the typed request file does not exist",
            remedy: "supply one file matching the discovered reporter task schema",
        },
        Refusal {
            code: "result_exists",
            when: "the requested result path already exists",
            remedy: "choose a new result path; plans are never overwritten",
        },
        Refusal {
            code: "engine_refused",
            when: "the shared Rust kernel rejects a fact, tag binding, or base document",
            remedy: "read detail.engine, correct the source JSON or granular tags, and rerun",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    availability: || ds_cli_report::DS_REPORT.availability(),
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = PathBuf::from(inputs.require("request")?);
    if !request.is_file() {
        return Err(Failure::invalid(
            "request_not_found",
            format!("cannot read `{}`", request.display()),
        ));
    }
    let (result, keep) = match inputs.value("result") {
        Some(path) => (PathBuf::from(path), true),
        None => (scratch_result(), false),
    };
    if result.symlink_metadata().is_ok() {
        return Err(Failure::invalid(
            "result_exists",
            format!("`{}` already exists", result.display()),
        ));
    }
    let args = vec![
        OsString::from("--request"),
        request.into(),
        OsString::from("--result"),
        result.clone().into(),
    ];
    let completed = ds_cli_report::DS_REPORT.call(
        "plan-solar-network-seed",
        &args,
        ds_cli_report::EXPORT_TIMEOUT,
    )?;
    let mut document = std::fs::read(&result)
        .ok()
        .and_then(|body| serde_json::from_slice::<Value>(&body).ok());
    if !keep {
        let _ = std::fs::remove_file(&result);
    }
    if !completed.succeeded() {
        return Err(ds_cli_report::DS_REPORT.failure_from(&completed, "plan-solar-network-seed"));
    }
    let Some(mut document) = document.take() else {
        return Err(Failure::failed(
            "engine_refused",
            "reporter returned no Solar network seed plan",
        )
        .detail(json!({"engine": completed.stderr})));
    };
    if document.get("mutated").and_then(Value::as_bool) != Some(false) {
        return Err(Failure::failed(
            "engine_refused",
            "reporter returned a document that is not a read-only seed plan",
        ));
    }
    if keep {
        document["result_path"] = json!(result.display().to_string());
    }
    Ok(document)
}

fn scratch_result() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "ds-solar-network-seed-{}-{nanos}.json",
        std::process::id()
    ))
}

pub fn render(data: &Value) -> String {
    format!(
        "planned {} Solar city document(s) from {} MV alignment(s) and {} LV transformer room(s)\n",
        data["writes"].as_array().map(Vec::len).unwrap_or(0),
        data["source"]["mv_alignment_count"].as_u64().unwrap_or(0),
        data["source"]["lv_transformer_count"].as_u64().unwrap_or(0),
    )
}
