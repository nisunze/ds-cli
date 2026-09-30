//! `ds report voltage-drop-combined` — one local A3 overview from the
//! reporter's typed, roster-pinned voltage-drop task.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, ExitClass, Inputs};
use serde_json::{Value, json};

use crate::{DS_REPORT, EXPORT_TIMEOUT};

pub static COMMAND: Command = Command {
    id: "report.voltage-drop-combined",
    path: &["report", "voltage-drop-combined"],
    contract: 1,
    summary: "Print an A3 voltage-drop overview for an exact transformer roster.",
    purpose: "Pass a typed local request to the Rust reporter. It verifies one voltage-drop result per transformer, keeps reserved and held designs out of calculated coverage, prints with one headless Chromium process, and returns a source- and PDF-digest receipt. This command does not publish or edit a project.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "request",
            "<absolute.json>",
            "Typed request from `ds report tasks --task render_voltage_drop_combined`; it names the exact roster, local result directory, any evidence-pinned hold, and an absent output PDF.",
        )
        .required(),
        Arg::value(
            "result",
            "<absolute.json>",
            "Optional absent path to retain the coverage and print receipt, including a refusal receipt.",
        ),
    ],
    output: "Local A3 PDF path, SHA-256, page count, per-source digests, calculated/reserved/held counts, blockers, and publication state.",
    examples: &[
        Example {
            command: "ds report tasks --task render_voltage_drop_combined --output json",
            note: "Discover the installed reporter's request schema before authoring the local request.",
            runnable: false,
        },
        Example {
            command: "ds report voltage-drop-combined --request /tmp/vd-overview-request.json --result /tmp/vd-overview-receipt.json --output json",
            note: "Print the exact roster once and retain its receipt; neither output path may exist.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "reporter_engine_missing",
            when: "the matching local ds-report process is unavailable",
            remedy: "install the matching DS reporter or set DS_REPORT_BIN to its built binary",
        },
        Refusal {
            code: "path_not_absolute",
            when: "--request or --result is a relative path",
            remedy: "supply absolute paths for the request and optional receipt",
        },
        Refusal {
            code: "request_not_found",
            when: "the typed request file does not exist",
            remedy: "supply the exact local request path",
        },
        Refusal {
            code: "result_exists",
            when: "the requested receipt path already exists",
            remedy: "choose a fresh receipt path",
        },
        Refusal {
            code: "engine_refused",
            when: "the roster, source results, pinned hold evidence, Chromium layout, or PDF output fails the reporter's contract",
            remedy: "read the returned refusal receipt, correct the named source or hold, and run with fresh output paths",
        },
    ],
    reference: Some("docs/reference/report.md"),
    search: &["voltage drop", "combined", "A3", "overview", "headless PDF"],
    requires: Requires::Server,
    availability,
};

fn availability() -> Availability {
    DS_REPORT.availability()
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = absolute_path(inputs.require("request")?, "request")?;
    if !request.is_file() {
        return Err(Failure::invalid(
            "request_not_found",
            format!("cannot read `{}`", request.display()),
        )
        .remedy("supply the exact local request path"));
    }
    let (result, keep) = match inputs.value("result") {
        Some(path) => (absolute_path(path, "result")?, true),
        None => (scratch_result(), false),
    };
    if result.symlink_metadata().is_ok() {
        return Err(Failure::new(
            ExitClass::Conflict,
            "result_exists",
            format!("`{}` already exists", result.display()),
        )
        .remedy("choose a fresh receipt path"));
    }
    let args = vec![
        OsString::from("--request"),
        request.into(),
        OsString::from("--result"),
        result.clone().into(),
    ];
    let completed = DS_REPORT.call("render-voltage-drop-combined", &args, EXPORT_TIMEOUT);
    // Read even after a process-level error: the owner may have written a
    // refusal before the process timed out or could no longer be waited on.
    let document = std::fs::read(&result)
        .map_err(|error| format!("read receipt: {error}"))
        .and_then(|body| {
            serde_json::from_slice::<Value>(&body)
                .map_err(|error| format!("decode receipt: {error}"))
        });
    // A malformed/unreadable receipt is evidence too. Keep its bytes unless
    // the entire document can be conveyed in the answer.
    let retained = keep || document.is_err() || std::fs::remove_file(&result).is_err();
    let receipt_detail = |failure: Failure| {
        let mut detail = failure.detail_value().cloned().unwrap_or_else(|| json!({}));
        detail["receipt"] = json!(document.as_ref().ok());
        if retained {
            detail["result_path"] = json!(result);
        }
        if let Err(error) = &document {
            detail["receipt_error"] = json!(error);
        }
        failure.detail(detail)
    };
    let completed = completed.map_err(receipt_detail)?;
    if !completed.succeeded() {
        return Err(receipt_detail(
            DS_REPORT.failure_from(&completed, "render-voltage-drop-combined"),
        ));
    }
    let valid = document.as_ref().is_ok_and(|document| {
        document["schema"] == "ds.voltage-drop-combined.render-receipt/v1"
            && document["status"] == "complete"
    });
    if !valid {
        return Err(receipt_detail(Failure::failed(
            "engine_refused",
            "reporter returned no complete combined voltage-drop receipt",
        )
        .remedy("inspect detail.receipt or result_path and the reporter version; use fresh output paths")
        .detail(json!({"engine": completed.stderr}))));
    }
    let mut document = document.expect("the complete receipt was checked above");
    if retained {
        document["result_path"] = json!(result.display().to_string());
    }
    Ok(document)
}

fn absolute_path(raw: &str, flag: &str) -> Result<PathBuf, Failure> {
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(Failure::invalid(
            "path_not_absolute",
            format!("--{flag} requires an absolute path"),
        )
        .remedy("supply absolute paths for the request and optional receipt"));
    }
    Ok(path.to_path_buf())
}

fn scratch_result() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "ds-report-voltage-drop-combined-{}-{nanos}.json",
        std::process::id()
    ))
}

pub fn render(data: &Value) -> String {
    format!(
        "completed — {} transformers ({} calculated, {} reserved, {} held), {} A3 page(s)\n{}\n",
        data["coverage"]["roster"],
        data["coverage"]["calculated"],
        data["coverage"]["reserved"],
        data["coverage"]["held"],
        data["pages"],
        data["output"].as_str().unwrap_or("")
    )
}
