//! `ds report bundle` — package digest-pinned report artifacts.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DS_REPORT, EXPORT_TIMEOUT};

pub static COMMAND: Command = Command {
    id: "report.bundle",
    path: &["report", "bundle"],
    contract: 1,
    summary: "Package transformer and combined reports into one verified ZIP.",
    purpose: "Calls the reporter's closed local bundle task. Every source is digest-pinned, manifest.json is embedded, no network call is made, and an existing output is never overwritten.",
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "request",
            "<path>",
            "Typed request from `ds report tasks --task export_compounded_report`.",
        )
        .required(),
        Arg::value(
            "result",
            "<path>",
            "Keep the reporter result document here; must not exist.",
        ),
    ],
    output: "The reporter receipt: ZIP path, byte count, SHA-256, and entry count.",
    examples: &[Example {
        command: "ds report bundle --request ./bundle.json --output json",
        note: "Create the ZIP named by the typed request.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "reporter_engine_missing",
            when: "`ds-report` is unavailable",
            remedy: "set DS_REPORT_BIN to a built ds-report",
        },
        Refusal {
            code: "request_not_found",
            when: "the request file is missing",
            remedy: "check --request",
        },
        Refusal {
            code: "result_exists",
            when: "the result path exists",
            remedy: "choose a new result path",
        },
        Refusal {
            code: "engine_refused",
            when: "the reporter rejects an input, digest, path, or output",
            remedy: "read detail.engine and correct the typed request",
        },
    ],
    reference: Some("docs/reference/report.md"),
    availability,
};

fn availability() -> Availability {
    DS_REPORT.availability()
}

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
    let completed = DS_REPORT.call("export-compounded-report", &args, EXPORT_TIMEOUT)?;
    let mut document = std::fs::read(&result)
        .ok()
        .and_then(|body| serde_json::from_slice::<Value>(&body).ok());
    if !keep {
        let _ = std::fs::remove_file(&result);
    }
    if !completed.succeeded() {
        return Err(DS_REPORT.failure_from(&completed, "export-compounded-report"));
    }
    let Some(mut document) = document.take() else {
        return Err(
            Failure::failed("engine_refused", "reporter returned no bundle receipt")
                .detail(json!({"engine": completed.stderr})),
        );
    };
    if keep {
        document["result_path"] = json!(result.display().to_string());
    }
    Ok(document)
}

fn scratch_result() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "ds-report-bundle-{}-{nanos}.json",
        std::process::id()
    ))
}

pub fn render(data: &Value) -> String {
    format!(
        "completed — {} entries, {} bytes\n{}\n",
        data["entries"],
        data["bytes"],
        data["output"].as_str().unwrap_or("")
    )
}
