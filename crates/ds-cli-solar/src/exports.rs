//! Export text deliverables from one sealed, paired native Solar batch.
//!
//! The native desktop owns the batch, workspace and every selector.  This
//! adapter never receives a path inside that workspace: it pages a named
//! Markdown or JSON artifact through the closed bridge, then creates the
//! caller-named destination file exactly once.

use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::paired;

const DOCUMENT_READ_OPERATION: &str = "solar.document.read";
const PORTFOLIO_READ_OPERATION: &str = "solar.portfolio.read";
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_EXPORT_BYTES: usize = 32 * 1024 * 1024;
const MAX_EXPORT_PAGES: usize = 1_024;
const REPORT_VARIANTS: &[&str] = &["apd", "draft"];
const PORTFOLIO_ARTIFACTS: &[&str] = &["result", "draft"];

static EXPORT_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "export_path_exists",
        when: "--out already names a file",
        remedy: "choose a new destination; exports never overwrite a local file",
    },
    Refusal {
        code: "export_unwritable",
        when: "the requested destination cannot be created or written",
        remedy: "choose a writable destination whose parent directory exists",
    },
    Refusal {
        code: "desktop_not_paired",
        when: "no DS GridDesign session is running on this machine",
        remedy: "start DS GridDesign, sign in, and retry",
    },
    Refusal {
        code: "desktop_ambiguous",
        when: "more than one DS GridDesign session is running",
        remedy: "name one with --desktop-descriptor <path>",
    },
    Refusal {
        code: "desktop_unreachable",
        when: "the bridge descriptor names a session that does not answer",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_unreadable",
        when: "the paired session's reply could not be read",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_operation_unsupported",
        when: "this DS GridDesign build does not offer the named Solar operation",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "desktop_refused",
        when: "the paired application declined the requested Solar operation",
        remedy: "read the refusal detail, correct the application state, and retry",
    },
    Refusal {
        code: "desktop_contract_mismatch",
        when: "the paired session returned an invalid artifact slice",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "pairing_rejected",
        when: "the descriptor's pairing secret is stale",
        remedy: "restart DS GridDesign to publish a fresh descriptor",
    },
];

pub static REPORT_EXPORT_COMMAND: Command = Command {
    id: "solar.report.export",
    path: &["solar", "report", "export"],
    contract: 1,
    summary: "Export a sealed native Solar APD or parity draft.",
    purpose: "Reads the selected text report in bounded slices from a completed paired native Solar batch and creates one new local file at --out. The desktop validates the run, city, document name and approved workspace on every slice. No workspace path, cache location or credential crosses the CLI boundary.",
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("run-id", "<id>", "Completed native Solar run id.").required(),
        Arg::value("city", "<id>", "Canonical city context in that run.").required(),
        Arg::value(
            "variant",
            "apd|draft",
            "APD or frozen parity draft to export.",
        )
        .default("apd")
        .choices(REPORT_VARIANTS),
        Arg::value(
            "out",
            "<file>",
            "New Markdown file to create; never overwritten.",
        )
        .required(),
        Arg::value(
            "desktop-descriptor",
            "<path>",
            "Use this bridge descriptor instead of discovering one.",
        ),
    ],
    output: "The created Markdown destination, exact byte count and source run/city/variant.",
    examples: &[Example {
        command: "ds solar report export --run-id run-123 --city kigali --variant draft --out ./kigali-draft.md --output json",
        note: "Exports the frozen native draft without exposing its workspace path.",
        runnable: false,
    }],
    refusals: EXPORT_REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static PORTFOLIO_EXPORT_COMMAND: Command = Command {
    id: "solar.portfolio.export",
    path: &["solar", "portfolio", "export"],
    contract: 1,
    summary: "Export a sealed native Solar portfolio result or draft.",
    purpose: "Reads the root-level portfolio artifact from one completed paired native Solar batch and creates one new local file at --out. Portfolio output is first-class: it is validated against the same closed batch as the city reports, never reconstructed by the CLI.",
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("run-id", "<id>", "Completed native Solar run id.").required(),
        Arg::value(
            "artifact",
            "result|draft",
            "Portfolio result JSON or frozen draft Markdown.",
        )
        .default("result")
        .choices(PORTFOLIO_ARTIFACTS),
        Arg::value(
            "out",
            "<file>",
            "New JSON or Markdown file to create; never overwritten.",
        )
        .required(),
        Arg::value(
            "desktop-descriptor",
            "<path>",
            "Use this bridge descriptor instead of discovering one.",
        ),
    ],
    output: "The created portfolio destination, exact byte count and source run/artifact.",
    examples: &[
        Example {
            command: "ds solar portfolio export --run-id run-123 --artifact result --out ./portfolio.json --output json",
            note: "Exports the sealed aggregate portfolio result from the local batch.",
            runnable: false,
        },
        Example {
            command: "ds solar portfolio export --run-id run-123 --artifact draft --out ./portfolio-draft.md --output json",
            note: "Exports the aggregate frozen parity draft.",
            runnable: false,
        },
    ],
    refusals: EXPORT_REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub fn export_report(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let run_id = inputs.require("run-id")?;
    let city = inputs.require("city")?;
    let variant = inputs.value("variant").unwrap_or("apd");
    let bytes = read_all(inputs, DOCUMENT_READ_OPERATION, |offset| {
        let mut arguments = Map::new();
        arguments.insert("run_id".into(), json!(run_id));
        arguments.insert("context".into(), json!(city));
        arguments.insert("document".into(), json!(variant));
        arguments.insert("offset".into(), json!(offset));
        Value::Object(arguments)
    })?;
    let out = inputs.require("out")?;
    write_new(out, &bytes)?;
    Ok(json!({
        "status": "exported",
        "run_id": run_id,
        "context": city,
        "variant": variant,
        "out": out,
        "bytes": bytes.len(),
    }))
}

pub fn export_portfolio(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let run_id = inputs.require("run-id")?;
    let artifact = inputs.value("artifact").unwrap_or("result");
    let bytes = read_all(inputs, PORTFOLIO_READ_OPERATION, |offset| {
        json!({
            "run_id": run_id,
            "artifact": artifact,
            "offset": offset,
        })
    })?;
    let out = inputs.require("out")?;
    write_new(out, &bytes)?;
    Ok(json!({
        "status": "exported",
        "run_id": run_id,
        "artifact": artifact,
        "out": out,
        "bytes": bytes.len(),
    }))
}

/// Page a UTF-8 text artifact without ever treating the desktop's workspace as
/// a filesystem the CLI can inspect. The native selector has the matching
/// 32 MiB ceiling; the duplicate limit makes a malformed bridge reply fail
/// before it can fill memory or create a partial destination.
fn read_all(
    inputs: &Inputs,
    operation: &'static str,
    arguments: impl Fn(u64) -> Value,
) -> Result<Vec<u8>, Failure> {
    let mut offset = 0_u64;
    let mut bytes = Vec::new();
    for _ in 0..MAX_EXPORT_PAGES {
        let page = paired::invoke(inputs, operation, arguments(offset), READ_TIMEOUT)?;
        let content = page["content"]
            .as_str()
            .ok_or_else(|| contract_failure(operation))?;
        let next = page["next_offset"]
            .as_u64()
            .ok_or_else(|| contract_failure(operation))?;
        let complete = page["complete"]
            .as_bool()
            .ok_or_else(|| contract_failure(operation))?;
        let page_bytes = content.as_bytes();
        if bytes.len().saturating_add(page_bytes.len()) > MAX_EXPORT_BYTES {
            return Err(Failure::failed(
                "desktop_contract_mismatch",
                "the paired artifact exceeds the 32 MiB export bound",
            )
            .remedy("export a smaller report or inspect the native batch in DS GridDesign"));
        }
        bytes.extend_from_slice(page_bytes);
        if complete {
            if next < offset || next != bytes.len() as u64 {
                return Err(contract_failure(operation));
            }
            return Ok(bytes);
        }
        if next <= offset || next != bytes.len() as u64 {
            return Err(contract_failure(operation));
        }
        offset = next;
    }
    Err(Failure::failed(
        "desktop_contract_mismatch",
        "the paired artifact did not complete within its page bound",
    )
    .remedy("update DS GridDesign and ds to matching releases"))
}

fn contract_failure(operation: &str) -> Failure {
    Failure::unavailable(
        "desktop_contract_mismatch",
        format!("the paired session returned an invalid artifact slice for `{operation}`"),
    )
    .remedy("update DS GridDesign and ds to matching releases")
}

fn write_new(path: &str, bytes: &[u8]) -> Result<(), Failure> {
    let path = Path::new(path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| {
        let code = if error.kind() == std::io::ErrorKind::AlreadyExists {
            "export_path_exists"
        } else {
            "export_unwritable"
        };
        Failure::failed(code, format!("could not create `{}`", path.display()))
            .remedy("choose a new writable destination; exports never overwrite a file")
    })?;
    file.write_all(bytes).map_err(|error| {
        let _ = std::fs::remove_file(path);
        Failure::failed(
            "export_unwritable",
            format!("could not write `{}`", path.display()),
        )
        .remedy("choose a writable destination and retry")
        .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    file.sync_all().map_err(|error| {
        let _ = std::fs::remove_file(path);
        Failure::failed(
            "export_unwritable",
            format!("could not finalize `{}`", path.display()),
        )
        .remedy("choose a writable destination and retry")
        .detail(json!({ "detail": error.kind().to_string() }))
    })
}

pub fn render(data: &Value) -> String {
    format!(
        "{}\nfile     {}\nbytes    {}\n",
        data["status"].as_str().unwrap_or("exported"),
        data["out"].as_str().unwrap_or(""),
        data["bytes"].as_u64().unwrap_or_default(),
    )
}
