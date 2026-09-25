//! `ds pls desktop autosag` — AutoSag every section of a saved project.
//!
//! Step A of the deliver chain, alone: `pls-section-table-autosag.ps1` sets
//! the Section Table's Command To Apply to AutoSag on every row (never menu
//! command 40337, which crashes PLS-CADD 16.81), the chain's own Save, a
//! Section Usage report as the gate that proves AutoSag took, and the chain's
//! own Exit. The project is saved in place; the evidence goes to a new folder.

use std::time::Duration;

use ds_cli_contract::args::INVALID_NUMBER;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

use super::bundle::Entry;
use super::run::Invocation;
use super::*;

pub const RECEIPT_SCHEMA: &str = "ds.pls.desktop_autosag.v1";
const BASE_TIMEOUT: Duration = Duration::from_secs(2 * 3600);

pub static COMMAND: Command = Command {
    id: "pls.desktop.autosag",
    path: &["pls", "desktop", "autosag"],
    contract: 1,
    summary: "AutoSag every section of a saved PLS-CADD project, then gate it.",
    purpose: "Opens a saved project in PLS-CADD 16.81, applies AutoSag to every section through the Section Table exactly as the proven deliver chain does, saves the project in place, runs Section Usage as the gate that shows AutoSag took, and exits. The project's .xyz is its entry point. Use deliver instead for a DS-exported backup: it also pages, backs up and makes the deliverables from a fresh restore.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<project.xyz>",
            "The saved project to AutoSag, by its .xyz.",
        )
        .required(),
        Arg::value(
            "out",
            "<new folder>",
            "Absent folder off C: for the evidence and the gate report.",
        )
        .required(),
        REPORT_TIMEOUT_ARG,
    ],
    output: "The receipt path and driver bundle digest, the project and its sha256 before AutoSag, the Section Table fill that was applied, the watcher outcome after OK, that the project was saved, and the Section Usage gate: its report path and violation counts.",
    examples: &[Example {
        command: r"ds pls desktop autosag --project 'G:\Shared drives\Pro\Working\r1\example.xyz' --out 'G:\Shared drives\Pro\Working\autosag-r1' --output json",
        note: "Saves the project in place; run it on a working copy.",
        runnable: false,
    }],
    refusals: &[
        WINDOWS_ONLY,
        PLS_CADD_NOT_FOUND,
        POWERSHELL_NOT_FOUND,
        SOURCE_NOT_FOUND,
        PROJECT_INVALID,
        INVALID_NUMBER,
        INVALID_ARGUMENT,
        OUTPUT_EXISTS,
        OUTPUT_PARENT_MISSING,
        SYSTEM_DRIVE_REFUSED,
        PLS_CADD_RUNNING,
        PLS_CADD_MISMATCH,
        UNKNOWN_DIALOG,
        DIALOG_STOP,
        PLS_CADD_TIMEOUT,
        DRIVER_FAILED,
        RUN_TIMED_OUT,
        BUNDLE_FAILED,
        RESULT_UNREADABLE,
    ],
    reference: Some("docs/reference/pls.md"),
    search: &[
        "auto sag",
        "section table",
        "sag sections",
        "pls-cadd desktop",
    ],
    requires: Requires::Server,
    availability: super::availability,
};

struct Request {
    project: String,
    out: String,
    report_timeout: String,
}

fn request(inputs: &Inputs) -> Result<Request, Failure> {
    Ok(Request {
        project: project_file(inputs.require("project")?)?,
        out: new_folder(inputs.require("out")?, "out")?,
        report_timeout: report_timeout(inputs.value("report-timeout"))?,
    })
}

fn invocation(request: &Request) -> Invocation {
    let report_seconds: u64 = request.report_timeout.parse().unwrap_or(1800);
    Invocation::new(
        Entry::Autosag,
        BASE_TIMEOUT + Duration::from_secs(report_seconds),
    )
    .value("ProjectPath", request.project.clone())
    .value("RunDirectory", request.out.clone())
    .value("ReportTimeoutSeconds", request.report_timeout.clone())
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = request(inputs)?;
    let finished = run::execute(&invocation(&request))?;
    let result = failure::outcome(finished, COMMAND.refusals, &[&request.out])?;
    let receipt = result["receipt"].as_str().ok_or_else(|| {
        Failure::failed(RESULT_UNREADABLE.code, "the run named no receipt")
            .remedy(RESULT_UNREADABLE.remedy)
    })?;
    shape(receipt, &read_receipt(receipt)?)
}

fn shape(receipt_path: &str, receipt: &Value) -> Result<Value, Failure> {
    if receipt["schema"] != RECEIPT_SCHEMA {
        return Err(Failure::failed(
            RESULT_UNREADABLE.code,
            format!("the receipt is not {RECEIPT_SCHEMA}"),
        )
        .remedy(RESULT_UNREADABLE.remedy)
        .detail(json!({ "schema": receipt["schema"] })));
    }
    let mut shaped = provenance(receipt_path);
    shaped["project"] = receipt["project"].clone();
    shaped["autosag"] = receipt["autosag"].clone();
    shaped["saved"] = receipt["saved"].clone();
    shaped["gate_section_usage"] = receipt["gate_section_usage"].clone();
    Ok(shaped)
}

pub fn render(data: &Value) -> String {
    format!(
        "PLS-CADD AutoSag\n  project  {}\n  gate     {} section violations after AutoSag\n  receipt  {}\n",
        data["project"]["path"].as_str().unwrap_or(""),
        data["gate_section_usage"]["verdict"]["section_violations"],
        data["receipt"].as_str().unwrap_or(""),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::run::tests::declared_parameters;

    #[test]
    fn the_receipt_the_entry_writes_becomes_the_result() {
        let entry = crate::desktop::bundle::text(Entry::Autosag.file()).unwrap();
        for key in [
            "schema = 'ds.pls.desktop_autosag.v1'",
            "project = [ordered]@{ path = $launch.project_path; sha256_before = $launch.project_sha256 }",
            "saved = $true",
            "gate_section_usage = [ordered]@{ report = $g.output; verdict = $gate }",
        ] {
            assert!(entry.contains(key), "the entry no longer writes `{key}`");
        }
        let receipt = json!({
            "schema": RECEIPT_SCHEMA,
            "project": { "path": r"G:\r1\example.xyz", "sha256_before": "ab".repeat(32) },
            "autosag": { "evidence_directory": r"G:\a\ev-autosag", "fill": { "id": 38079, "text": "Copy && Fill Column" }, "watcher": "ready" },
            "saved": true,
            "gate_section_usage": { "report": r"G:\a\gate\section-usage-after-autosag.txt", "verdict": { "section_violations": 0 } },
        });
        let data = shape(r"G:\a\autosag.json", &receipt).unwrap();
        assert_eq!(data["autosag"]["fill"]["id"], 38079);
        assert_eq!(
            data["gate_section_usage"]["verdict"]["section_violations"],
            0
        );
        assert!(render(&data).contains("0 section violations"));
        assert_eq!(
            shape("x", &json!({ "schema": "other" }))
                .unwrap_err()
                .code(),
            "driver_result_unreadable"
        );
    }

    #[test]
    fn every_parameter_passed_is_one_the_entry_declares() {
        let invocation = invocation(&Request {
            project: r"G:\r1\example.xyz".into(),
            out: r"G:\a".into(),
            report_timeout: "1800".into(),
        });
        let declared = declared_parameters(Entry::Autosag);
        for (name, _) in &invocation.params {
            assert!(declared.iter().any(|d| d == name), "{name} is not declared");
        }
        assert_eq!(invocation.params.len(), 3);
    }
}
