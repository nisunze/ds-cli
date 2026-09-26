//! `ds pls desktop reports` — the six deliverable reports of a saved project.
//!
//! Part B of the deliver chain without its restore: open the project, save
//! the six reports as RTF through `pls-report-any.ps1` and read their verdict
//! lines with the chain's own `Verdict`, exit without saving, then make A3
//! landscape PDFs from the RTFs with Word (`pls-rtf-to-pdf.ps1`). Nothing is
//! written to the project.

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

pub const RECEIPT_SCHEMA: &str = "ds.pls.desktop_reports.v1";
const BASE_TIMEOUT: Duration = Duration::from_secs(2 * 3600);

pub static COMMAND: Command = Command {
    id: "pls.desktop.reports",
    path: &["pls", "desktop", "reports"],
    contract: 1,
    summary: "Save a saved project's six PLS-CADD reports as RTF and A3 PDF.",
    purpose: "Opens a saved project in PLS-CADD 16.81 and saves the deliverable reports as RTF, in the deliver chain's order: Section Usage, Structure Usage, Terrain Clearances for every feature code, Wind & Weight Span, Summary and Sag-Tension, reading the violation lines from each; exits without saving; then makes an A3 landscape PDF of every RTF with Microsoft Word. Nothing is written to the project. The project's .xyz is its entry point.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<project.xyz>",
            "The saved project to report on, by its .xyz.",
        )
        .required(),
        Arg::value(
            "out",
            "<new folder>",
            "Absent folder off C: that receives the RTFs, PDFs and journals.",
        )
        .required(),
        REPORT_TIMEOUT_ARG,
    ],
    output: "The receipt path and driver bundle digest, the project and its sha256, and per report its RTF and PDF paths, sizes, RTF sha256 and verdict counts: section and structure violations, structure warnings, and spans with and without clearance violations.",
    examples: &[Example {
        command: r"ds pls desktop reports --project 'G:\Shared drives\Pro\Working\r2\example.xyz' --out 'G:\Shared drives\Pro\Working\reports-r2' --output json",
        note: "Terrain Clearances computes for minutes on a large model; --report-timeout bounds each report.",
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
        WORD_NOT_FOUND,
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
        "structure usage",
        "section usage",
        "sag tension",
        "rtf pdf",
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
        Entry::Reports,
        BASE_TIMEOUT + Duration::from_secs(6 * report_seconds),
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
    shaped["reports"] = receipt["reports"].clone();
    Ok(shaped)
}

pub fn render(data: &Value) -> String {
    let mut text = format!(
        "PLS-CADD reports\n  project  {}\n",
        data["project"]["path"].as_str().unwrap_or("")
    );
    if let Some(reports) = data["reports"].as_object() {
        for (name, report) in reports {
            text.push_str(&format!(
                "  {name}: {}\n",
                report["pdf"]
                    .as_str()
                    .or(report["rtf"].as_str())
                    .unwrap_or("")
            ));
        }
    }
    text.push_str(&format!(
        "  receipt  {}\n",
        data["receipt"].as_str().unwrap_or("")
    ));
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::bundle;
    use crate::desktop::run::tests::declared_parameters;

    /// The report list is a hand copy of the deliver chain's part B, in the
    /// shared entry plumbing. Every id and document title must be the chain's.
    #[test]
    fn the_six_reports_are_the_deliver_chain_s_six() {
        let chain = bundle::text("pls-deliver-autosag.ps1").unwrap();
        let lib = bundle::text("ds-desktop-lib.ps1").unwrap();
        let reports = [
            "@{ k = 'Section Usage'; id = 40015; p = 'Section Usage Report'; all = $false }",
            "@{ k = 'Structure Usage'; id = 40014; p = 'Structure Usage Report'; all = $false }",
            "@{ k = 'Terrain Clearances'; id = 40016; p = 'Terrain Clearances by Span'; all = $true }",
            "@{ k = 'Wind & Weight Span'; id = 40020; p = 'Wind & Weight Span'; all = $false }",
            "@{ k = 'Summary'; id = 40019; p = 'Summary Report'; all = $false }",
            "@{ k = 'Sag Tension'; id = 40403; p = 'Sag-Tension Report'; all = $false }",
        ];
        for report in reports {
            assert!(chain.contains(report), "the chain does not run {report}");
            assert!(
                lib.contains(report),
                "the entry plumbing does not run {report}"
            );
        }
        assert_eq!(lib.matches("@{ k = '").count(), reports.len());
    }

    #[test]
    fn the_receipt_the_entry_writes_becomes_the_result() {
        let entry = bundle::text(Entry::Reports.file()).unwrap();
        assert!(entry.contains("schema = 'ds.pls.desktop_reports.v1'"));
        assert!(
            entry.contains("Assert-DsWord"),
            "Word is checked before PLS-CADD starts"
        );
        let receipt = json!({
            "schema": RECEIPT_SCHEMA,
            "project": { "path": r"G:\r2\example.xyz", "sha256": "cd".repeat(32) },
            "reports": {
                "Structure Usage": {
                    "rtf": r"G:\o\reports\Structure Usage.rtf", "bytes": 10, "sha256": "ef".repeat(32),
                    "verdict": { "structure_violations": 3 },
                    "pdf": r"G:\o\reports\Structure Usage.pdf", "pdf_bytes": 4
                }
            }
        });
        let data = shape(r"G:\o\reports.json", &receipt).unwrap();
        assert_eq!(
            data["reports"]["Structure Usage"]["verdict"]["structure_violations"],
            3
        );
        assert!(render(&data).contains(r"Structure Usage: G:\o\reports\Structure Usage.pdf"));
    }

    #[test]
    fn every_parameter_passed_is_one_the_entry_declares() {
        let invocation = invocation(&Request {
            project: r"G:\r2\example.xyz".into(),
            out: r"G:\o".into(),
            report_timeout: "600".into(),
        });
        let declared = declared_parameters(Entry::Reports);
        for (name, _) in &invocation.params {
            assert!(declared.iter().any(|d| d == name), "{name} is not declared");
        }
    }
}
