//! Present one complete, governed parcel-sector count as an XLSX workbook.
//! ds-network-reporter owns the workbook; this command only passes its typed
//! request and returns its machine-readable receipt.

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DS_REPORT, EXPORT_TIMEOUT};

const SOURCE_MISSING: Refusal = Refusal {
    code: "spatial_receipt_missing",
    when: "The direct spatial execution receipt is absent or unreadable.",
    remedy: "Run `ds data spatial execute --result count --group-by sector` and pass its fresh JSON output.",
};
const OUTPUT_EXISTS: Refusal = Refusal {
    code: "output_exists",
    when: "The workbook output path already exists.",
    remedy: "Choose a new .xlsx path in the project's authoritative delivery folder.",
};
const REQUEST_FAILED: Refusal = Refusal {
    code: "request_write_failed",
    when: "The typed local reporter request cannot be written.",
    remedy: "Check temporary storage and retry with the same source receipt.",
};
const ENGINE_REFUSED: Refusal = Refusal {
    code: "engine_refused",
    when: "The reporter refused the source receipt or did not return a workbook receipt.",
    remedy: "Check that the spatial result is a complete, distinct-UPI sector count for the same project.",
};

pub static COMMAND: Command = Command {
    id: "report.spatial.workbook",
    path: &["report", "spatial", "workbook"],
    contract: 1,
    summary: "Create an Excel parcel count by sector from a spatial result.",
    purpose: "Turns one complete `data.spatial.execute` distinct-UPI sector-count JSON receipt into a two-sheet XLSX with counts and query provenance. The reporter verifies the source, project, authority version, plan hash and count semantics. This is local presentation of a prior authorized read; it performs no BigQuery query and reads no active project.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<exact-id>",
            "Must equal the project_id in the spatial execution receipt.",
        )
        .required(),
        Arg::value(
            "project-name",
            "<label>",
            "Optional human-readable project name on the workbook.",
        ),
        Arg::value(
            "receipt",
            "<spatial-count.json>",
            "Direct JSON file from `ds data spatial execute`.",
        )
        .required(),
        Arg::value(
            "out",
            "<new.xlsx>",
            "Fresh workbook path in the project's authoritative folder.",
        )
        .required(),
    ],
    output: "Workbook path, SHA-256, distinct UPI overall count, sector count, unassigned count, dataset and authority versions, and plan hash.",
    examples: &[Example {
        command: "ds report spatial workbook --project <id> --receipt ./parcel-sector-counts.json --out ./parcel-sector-counts.xlsx --output json",
        note: "Present one complete governed parcel-sector count.",
        runnable: false,
    }],
    refusals: &[
        SOURCE_MISSING,
        OUTPUT_EXISTS,
        REQUEST_FAILED,
        ENGINE_REFUSED,
    ],
    reference: Some("docs/reference/report.md"),
    search: &[
        "spatial", "parcels", "upi", "sector", "bigquery", "excel", "xlsx",
    ],
    requires: Requires::Server,
    availability,
};

fn availability() -> Availability {
    DS_REPORT.availability()
}

fn absolute(path: &str) -> Result<PathBuf, Failure> {
    let path = Path::new(path);
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| {
                Failure::invalid(REQUEST_FAILED.code, error.to_string())
                    .remedy(REQUEST_FAILED.remedy)
            })
    }
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let receipt = absolute(inputs.require("receipt")?)?;
    let out = absolute(inputs.require("out")?)?;
    if !receipt.is_file() {
        return Err(Failure::invalid(
            SOURCE_MISSING.code,
            format!("missing spatial receipt: {}", receipt.display()),
        )
        .remedy(SOURCE_MISSING.remedy));
    }
    if out.symlink_metadata().is_ok() {
        return Err(Failure::invalid(
            OUTPUT_EXISTS.code,
            format!("workbook already exists: {}", out.display()),
        )
        .remedy(OUTPUT_EXISTS.remedy));
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let temporary = std::env::temp_dir();
    let request_path = temporary.join(format!(
        "ds-spatial-workbook-request-{}-{nonce}.json",
        std::process::id()
    ));
    let result_path = temporary.join(format!(
        "ds-spatial-workbook-result-{}-{nonce}.json",
        std::process::id()
    ));
    let request = json!({
        "schema": "ds-spatial-sector-counts-workbook/v1",
        "project_id": project,
        "project_name": inputs.value("project-name"),
        "receipt_path": receipt,
        "out_xlsx": out,
    });
    let bytes = serde_json::to_vec(&request)
        .map_err(|error| Failure::internal(REQUEST_FAILED.code, error.to_string()))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&request_path)
        .map_err(|error| {
            Failure::invalid(REQUEST_FAILED.code, error.to_string()).remedy(REQUEST_FAILED.remedy)
        })?;
    if let Err(error) = file.write_all(&bytes) {
        let _ = std::fs::remove_file(&request_path);
        return Err(
            Failure::invalid(REQUEST_FAILED.code, error.to_string()).remedy(REQUEST_FAILED.remedy)
        );
    }
    drop(file);
    let args = [
        OsString::from("--request"),
        request_path.clone().into(),
        OsString::from("--result"),
        result_path.clone().into(),
    ];
    let completed = DS_REPORT.call("export-spatial-sector-counts", &args, EXPORT_TIMEOUT);
    let _ = std::fs::remove_file(&request_path);
    let completed = completed?;
    let document = std::fs::read(&result_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let _ = std::fs::remove_file(&result_path);
    if !completed.succeeded() {
        return Err(DS_REPORT.failure_from(&completed, "export-spatial-sector-counts"));
    }
    let document = document.ok_or_else(|| {
        Failure::failed(ENGINE_REFUSED.code, "reporter returned no workbook receipt")
            .remedy(ENGINE_REFUSED.remedy)
    })?;
    if document["schema"] != "ds-spatial-sector-counts-workbook/v1"
        || document["project_id"] != project
        || document["status"] != "completed"
        || !document["output_sha256"].is_string()
    {
        return Err(Failure::failed(
            ENGINE_REFUSED.code,
            "reporter returned an invalid workbook receipt",
        )
        .remedy(ENGINE_REFUSED.remedy));
    }
    Ok(document)
}

pub fn render(value: &Value) -> String {
    format!(
        "{} distinct UPI in {} sectors\n{}\n",
        value["overall_count"],
        value["sector_count"],
        value["out_xlsx"].as_str().unwrap_or("?")
    )
}
