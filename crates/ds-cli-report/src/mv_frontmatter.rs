//! Project-owned MV cover and naming pages through the native document renderer.

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
    id: "report.mv-frontmatter",
    path: &["report", "mv-frontmatter"],
    contract: 1,
    summary: "Render selected MV front-matter pages from held project source and explicit facts.",
    purpose: "Pass a typed local request to the Rust reporter. It verifies the project-owned HTML, CSS and logo digests, fills only explicitly supplied facts, and prints selected A3 cover or naming pages with Chromium. The receipt pins inputs and PDFs; the existing MV booklet assembler consumes those PDFs. This command does not publish or edit a project.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "request",
            "<absolute.json>",
            "Typed request from `ds report tasks --task render_mv_frontmatter`; it names held project sources, explicit facts, selected pages and an absent output directory.",
        )
        .required(),
        Arg::value(
            "result",
            "<absolute.json>",
            "Optional absent path to retain the coverage and print receipt, including a refusal receipt.",
        ),
    ],
    output: "Selected A3 PDF paths and digests, held source and input provenance, page audits, and publication state.",
    examples: &[
        Example {
            command: "ds report tasks --task render_mv_frontmatter --output json",
            note: "Discover the installed reporter's request schema before authoring the local request.",
            runnable: false,
        },
        Example {
            command: "ds report mv-frontmatter --request /home/me/reports/frontmatter-request.json --result /home/me/reports/frontmatter-receipt.json --output json",
            note: "Render the explicitly selected pages and retain their receipt; output paths must be fresh.",
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
            when: "a source digest, missing fact, page audit, Chromium render or PDF output fails the reporter's contract",
            remedy: "read the returned refusal receipt, correct the named source or input, and use fresh output paths",
        },
    ],
    reference: Some("docs/reference/report.md"),
    search: &["headless pdf", "cover", "naming convention", "reusable pages"],
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
    let completed = DS_REPORT.call("render-mv-frontmatter", &args, EXPORT_TIMEOUT);
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
            DS_REPORT.failure_from(&completed, "render-mv-frontmatter"),
        ));
    }
    let valid = document.as_ref().is_ok_and(|document| {
        document["schema"] == "ds.mv-frontmatter.render-receipt/v1"
            && document["status"] == "complete"
    });
    if !valid {
        return Err(receipt_detail(Failure::failed(
            "engine_refused",
            "reporter returned no complete MV front-matter receipt",
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
        "ds-report-mv-frontmatter-{}-{nanos}.json",
        std::process::id()
    ))
}

pub fn render(data: &Value) -> String {
    format!(
        "completed — {} selected MV front-matter page(s)\n",
        data["artifacts"].as_array().map_or(0, Vec::len)
    )
}
