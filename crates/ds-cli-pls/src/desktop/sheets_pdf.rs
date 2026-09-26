//! `ds pls desktop sheets-pdf` — every plan & profile sheet to one PDF.
//!
//! The deliver chain's sheet step alone: open the project, bring up its
//! Sheets View as the chain does, and save every sheet with PLS-CADD's own
//! exporter (`pls-save-sheets-pdf.ps1`, which writes each sheet at its page
//! size; the Microsoft Print to PDF route produced Letter pages and is not
//! used). Exit without saving.

use std::time::Duration;

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

use super::bundle::Entry;
use super::run::Invocation;
use super::*;

pub const RECEIPT_SCHEMA: &str = "ds.pls.desktop_sheets_pdf.v1";
const TIMEOUT: Duration = Duration::from_secs(3 * 3600);

pub static COMMAND: Command = Command {
    id: "pls.desktop.sheets-pdf",
    path: &["pls", "desktop", "sheets-pdf"],
    contract: 1,
    summary: "Save every plan & profile sheet of a PLS-CADD project to one PDF.",
    purpose: "Opens a saved project in PLS-CADD 16.81, brings up its Sheets View, and saves every plan & profile sheet into one PDF with PLS-CADD's own exporter, each sheet at its configured page size, then exits without saving. The sheets are paged as the project's own sheet settings say; deliver sets the multi-alignment paging first. The project's .xyz is its entry point.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<project.xyz>",
            "The saved project whose sheets to save, by its .xyz.",
        )
        .required(),
        Arg::value(
            "out",
            "<new folder>",
            "Absent folder off C: that receives the PDF and the journal.",
        )
        .required(),
    ],
    output: "The receipt path and driver bundle digest, the project and its sha256, and the PDF's path, size and the seconds PLS-CADD took to write it.",
    examples: &[Example {
        command: r"ds pls desktop sheets-pdf --project 'G:\Shared drives\Pro\Working\r2\example.xyz' --out 'G:\Shared drives\Pro\Working\sheets-r2' --output json",
        note: "Writes <out>\\pdf\\Plan and Profile.pdf.",
        runnable: false,
    }],
    refusals: &[
        WINDOWS_ONLY,
        PLS_CADD_NOT_FOUND,
        POWERSHELL_NOT_FOUND,
        SOURCE_NOT_FOUND,
        PROJECT_INVALID,
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
        "plan profile",
        "sheets view",
        "print sheets",
        "pls-cadd desktop",
    ],
    requires: Requires::Server,
    availability: super::availability,
};

struct Request {
    project: String,
    out: String,
}

fn request(inputs: &Inputs) -> Result<Request, Failure> {
    Ok(Request {
        project: project_file(inputs.require("project")?)?,
        out: new_folder(inputs.require("out")?, "out")?,
    })
}

fn invocation(request: &Request) -> Invocation {
    Invocation::new(Entry::SheetsPdf, TIMEOUT)
        .value("ProjectPath", request.project.clone())
        .value("RunDirectory", request.out.clone())
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
    shaped["sheets"] = receipt["sheets"].clone();
    Ok(shaped)
}

pub fn render(data: &Value) -> String {
    format!(
        "PLS-CADD plan & profile sheets\n  pdf      {} ({} bytes)\n  receipt  {}\n",
        data["sheets"]["pdf"].as_str().unwrap_or(""),
        data["sheets"]["bytes"],
        data["receipt"].as_str().unwrap_or(""),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::bundle;
    use crate::desktop::run::tests::declared_parameters;

    /// The Sheets View step is a hand copy of the deliver chain's; the
    /// commands it posts must be the chain's.
    #[test]
    fn the_sheets_view_step_is_the_deliver_chain_s() {
        let chain = bundle::text("pls-deliver-autosag.ps1").unwrap();
        let lib = bundle::text("ds-desktop-lib.ps1").unwrap();
        for line in [
            "-CommandId 61504 -Post | Out-Null; Start-Sleep -Milliseconds 1500",
            "-CommandId 40075 -Post | Out-Null   # Window > New Window > Sheets View",
            "if ($w.outcome -ne 'ready' -or (Title) -notmatch '\\[Sheets View\\]$') { throw \"no Sheets View (frame '$(Title)', watch $($w.outcome))\" }",
        ] {
            assert!(chain.contains(line), "the chain does not do `{line}`");
            assert!(
                lib.contains(line),
                "the entry plumbing does not do `{line}`"
            );
        }
    }

    #[test]
    fn the_receipt_the_entry_writes_becomes_the_result() {
        assert!(
            bundle::text(Entry::SheetsPdf.file())
                .unwrap()
                .contains("schema = 'ds.pls.desktop_sheets_pdf.v1'")
        );
        let receipt = json!({
            "schema": RECEIPT_SCHEMA,
            "project": { "path": r"G:\r2\example.xyz", "sha256": "cd".repeat(32) },
            "sheets": { "pdf": r"G:\s\pdf\Plan and Profile.pdf", "bytes": 48_912_004, "seconds": 412 },
        });
        let data = shape(r"G:\s\sheets.json", &receipt).unwrap();
        assert_eq!(data["sheets"]["seconds"], 412);
        assert!(render(&data).contains("48912004 bytes"));
    }

    #[test]
    fn every_parameter_passed_is_one_the_entry_declares() {
        let invocation = invocation(&Request {
            project: r"G:\r2\example.xyz".into(),
            out: r"G:\s".into(),
        });
        let declared = declared_parameters(Entry::SheetsPdf);
        for (name, _) in &invocation.params {
            assert!(declared.iter().any(|d| d == name), "{name} is not declared");
        }
    }
}
