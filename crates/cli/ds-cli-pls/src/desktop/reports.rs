//! `ds pls desktop reports` — five canonical submission reports of a saved project.
//!
//! Part B of the deliver chain without its restore: open the project, save
//! reports as RTF through `pls-report-any.ps1` and read their verdict
//! lines with the chain's own `Verdict`, exit without saving, then make A4
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
    contract: 2,
    summary: "Save five canonical PLS-CADD reports as RTF and A4 PDF, or RTF only.",
    purpose: "Opens a saved project in PLS-CADD 16.81 and saves Section Usage, Structure Usage, Terrain Clearances for every feature code, Summary and Section Sag-Tension as RTF with violation counts, then exits without saving. Microsoft Word, or installed LibreOffice when Word is absent, converts each report to A4 landscape PDF. A3 paper and the supplementary Wind & Weight Span report require explicit customization. --rtf-only skips Word and PDF conversion; the default remains RTF plus PDF. --attach-pid uses an existing pinned process only when its unique frame proves the current project's exact full path, and leaves that session open instead of exiting. Staking is an Excel deliverable, not a printed report. The project's .xyz is its entry point; no model or engineering setting is changed.",
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
            "Absent folder off C: that receives the RTFs, selected PDFs and journals.",
        )
        .required(),
        Arg::switch(
            "rtf-only",
            "Save native RTF reports and verdict counts without Microsoft Word or PDFs.",
        ),
        ATTACH_PID_ARG,
        REPORT_TIMEOUT_ARG,
        Arg::value(
            "pdf-paper",
            "<paper>",
            "Report PDF paper; landscape orientation.",
        )
        .choices(&["A4", "A3"])
        .default("A4"),
        Arg::switch(
            "include-wind-weight-span",
            "Also produce the supplementary Wind & Weight Span report.",
        ),
    ],
    output: "The receipt path and driver bundle digest, the project and its sha256, report_format and session lifecycle metadata, and per report its RTF path (PDF path only when selected), sizes, RTF sha256 and verdict counts: section and structure violations, structure warnings, and spans with and without clearance violations.",
    examples: &[Example {
        command: r"ds pls desktop reports --project 'G:\Shared drives\Pro\Working\r2\example.xyz' --out 'G:\Shared drives\Pro\Working\reports-r2' --output json",
        note: "Terrain Clearances computes for minutes on a large model; --report-timeout bounds each report.",
        runnable: false,
    }],
    refusals: &[
        ADAPTERS_NOT_EMBEDDED,
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
        ATTACH_REFUSED,
        WORD_NOT_FOUND,
        REPORT_CONVERTER_NOT_FOUND,
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
    pdf_paper: String,
    include_wind_weight_span: bool,
    rtf_only: bool,
    attach_pid: Option<String>,
}

fn request(inputs: &Inputs) -> Result<Request, Failure> {
    Ok(Request {
        project: project_file(inputs.require("project")?)?,
        out: new_folder(inputs.require("out")?, "out")?,
        report_timeout: report_timeout(inputs.value("report-timeout"))?,
        pdf_paper: inputs.require("pdf-paper")?.into(),
        include_wind_weight_span: inputs.switch("include-wind-weight-span"),
        rtf_only: inputs.switch("rtf-only"),
        attach_pid: attach_pid(inputs.value("attach-pid"))?,
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
    .value("PdfPaper", request.pdf_paper.clone())
    .switch("IncludeWindWeightSpan", request.include_wind_weight_span)
    .switch("RtfOnly", request.rtf_only)
    .optional("AttachProcessId", request.attach_pid.clone())
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
    if let Some(paper) = receipt.get("pdf_paper") {
        shaped["pdf_paper"] = paper.clone();
        shaped["pdf_orientation"] = receipt["pdf_orientation"].clone();
    }
    for key in ["report_format", "session"] {
        if let Some(value) = receipt.get(key) {
            shaped[key] = value.clone();
        }
    }
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

#[cfg(all(test, feature = "desktop-adapters"))]
mod tests {
    use super::*;
    use crate::desktop::bundle;
    use crate::desktop::run::tests::declared_parameters;

    /// The report list is a hand copy of the deliver chain's part B, in the
    /// shared entry plumbing. Every id and document title must be the chain's.
    #[test]
    fn the_six_reports_are_the_deliver_chain_s_six() {
        let chain = bundle::pls_cadd_text("pls-deliver-autosag.ps1").unwrap();
        let lib = bundle::pls_cadd_text("ds-desktop-lib.ps1").unwrap();
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
            entry.contains("Assert-DsReportConverter"),
            "the selected PDF path retains its converter preflight"
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
    fn declared_defaults_and_customization_reach_the_native_entry() {
        let tokens = |parts: &[&str]| parts.iter().map(|p| p.to_string()).collect::<Vec<_>>();
        let defaults = ds_cli_contract::args::parse(
            &COMMAND,
            &tokens(&["--project", "G:\\model.xyz", "--out", "G:\\report-set"]),
        )
        .unwrap();
        assert_eq!(defaults.value("pdf-paper"), Some("A4"));
        assert!(!defaults.switch("include-wind-weight-span"));
        let custom = ds_cli_contract::args::parse(
            &COMMAND,
            &tokens(&[
                "--project",
                "G:\\model.xyz",
                "--out",
                "G:\\report-set",
                "--pdf-paper",
                "A3",
                "--include-wind-weight-span",
            ]),
        )
        .unwrap();
        assert_eq!(custom.value("pdf-paper"), Some("A3"));
        assert!(custom.switch("include-wind-weight-span"));
        let invocation = invocation(&Request {
            project: "G:\\model.xyz".into(),
            out: "G:\\report-set".into(),
            report_timeout: "600".into(),
            pdf_paper: custom.require("pdf-paper").unwrap().into(),
            include_wind_weight_span: custom.switch("include-wind-weight-span"),
            rtf_only: custom.switch("rtf-only"),
            attach_pid: None,
        });
        assert!(invocation.params.contains(&("PdfPaper", Some("A3".into()))));
        assert!(invocation.params.contains(&("IncludeWindWeightSpan", None)));
        assert!(
            ds_cli_contract::args::parse(
                &COMMAND,
                &tokens(&[
                    "--project",
                    "G:\\model.xyz",
                    "--out",
                    "G:\\report-set",
                    "--pdf-paper",
                    "A2",
                ])
            )
            .is_err()
        );
    }

    #[test]
    fn every_parameter_passed_is_one_the_entry_declares() {
        let invocation = invocation(&Request {
            project: r"G:\r2\example.xyz".into(),
            out: r"G:\o".into(),
            report_timeout: "600".into(),
            pdf_paper: "A4".into(),
            include_wind_weight_span: true,
            rtf_only: true,
            attach_pid: Some("4242".into()),
        });
        let declared = declared_parameters(Entry::Reports);
        for (name, _) in &invocation.params {
            assert!(declared.iter().any(|d| d == name), "{name} is not declared");
        }
    }

    // The request fixture lives in the Unix temp directory; Windows temp is
    // normally on C:, which the delivery folder contract deliberately refuses.
    #[cfg(unix)]
    #[test]
    fn explicit_report_modes_route_from_declared_flags_without_changing_defaults() {
        let root =
            std::env::temp_dir().join(format!("ds-cli-pls-reports-flags-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let project = root.join("example.xyz");
        std::fs::write(&project, "fixture").unwrap();
        let out = root.join("new-reports");
        let tokens = vec![
            "--project".into(),
            project.display().to_string(),
            "--out".into(),
            out.display().to_string(),
        ];
        let inputs = ds_cli_contract::args::parse(&COMMAND, &tokens).unwrap();
        let default = invocation(&request(&inputs).unwrap());
        // ProjectPath, RunDirectory, ReportTimeoutSeconds and the A4 default.
        assert_eq!(default.params.len(), 4);
        assert!(default.params.contains(&("PdfPaper", Some("A4".into()))));
        assert!(
            default
                .params
                .iter()
                .all(|(name, _)| *name != "RtfOnly" && *name != "AttachProcessId")
        );

        let mut explicit = tokens;
        explicit.extend(["--rtf-only".into(), "--attach-pid".into(), "4242".into()]);
        let inputs = ds_cli_contract::args::parse(&COMMAND, &explicit).unwrap();
        let selected = invocation(&request(&inputs).unwrap());
        assert!(selected.params.contains(&("RtfOnly", None)));
        assert!(
            selected
                .params
                .contains(&("AttachProcessId", Some("4242".into())))
        );
        assert_eq!(selected.params.len(), default.params.len() + 2);
        assert_eq!(selected.timeout, default.timeout);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rtf_only_receipt_preserves_verdicts_and_attached_session() {
        let receipt = json!({
            "schema": RECEIPT_SCHEMA,
            "project": { "path": "example.xyz" },
            "report_format": "rtf_only",
            "session": {
                "mode": "attached", "process_id": 4242,
                "main_window_handle": 1234, "project_left_open": true
            },
            "reports": {
                "Structure Usage": {
                    "rtf": "Structure Usage.rtf", "bytes": 10,
                    "verdict": { "structure_violations": 3, "structure_warnings": 2 }
                }
            }
        });
        let data = shape("reports.json", &receipt).unwrap();
        assert_eq!(data["report_format"], receipt["report_format"]);
        assert_eq!(data["session"], receipt["session"]);
        assert_eq!(data["reports"], receipt["reports"]);
        assert!(data["reports"]["Structure Usage"].get("pdf").is_none());
        assert!(render(&data).contains("Structure Usage: Structure Usage.rtf"));
    }
}
