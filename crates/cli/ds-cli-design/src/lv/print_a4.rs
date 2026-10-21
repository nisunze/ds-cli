//! `design lv voltage-drop --print-a4`: the read-only A4 of one exported
//! transformer. The Kernel decides the Reporter request from the export's own
//! sealed capture (`report_export::voltage_drop_a4`); this adapter reads that
//! capture, preflights the lane's Reporter before anything is calculated,
//! runs `report.export --task voltage-drop` and verifies the PDF. Nothing is
//! read from a server, saved or published.

use std::path::{Path, PathBuf};

use ds_cli_contract::spec::Refusal;
use ds_cli_contract::{Failure, Inputs, parse};
use ds_command_kernel::report_export::{InputReceipt, voltage_drop_a4};
use ds_network_contracts::lv_processing::NativeLvJobV1;
use serde_json::{Value, json};

use super::artifact::{ArtifactContract, ensure_absent, printing_receipt_path};
use super::process::bounded_read;

const INPUT_INVALID: Refusal = Refusal {
    code: "vd_print_input_invalid",
    when: "--print-a4 meets a scenario flag, several jobs or no sealed --project-config capture",
    remedy: "print one project-export --project-config request, its .printing-receipt.json beside it",
};
const PRINT_FAILED: Refusal = Refusal {
    code: "vd_print_failed",
    when: "the result was written; the A4 was refused or unverified",
    remedy: "act on detail.print_error; only calculated transformers print on A4",
};
pub(super) const REFUSALS: &[Refusal] = &[
    INPUT_INVALID,
    Refusal {
        code: "vd_print_output_exists",
        when: "--print-a4 names an existing path",
        remedy: "choose an absent PDF path",
    },
    PRINT_FAILED,
    Refusal {
        code: "reporter_engine_missing",
        when: "--print-a4 finds no Reporter for --lane",
        remedy: "install Reporter for that lane",
    },
    Refusal {
        code: "unknown_task",
        when: "the Reporter lacks render_voltage_drop_result",
        remedy: "install a Reporter advertising it",
    },
    super::project_run::BROWSER_MISSING,
];

const PDF: ArtifactContract = ArtifactContract {
    name: "A4 PDF",
    stage_tag: "ds-lv-voltage-drop-a4",
    exists_code: "vd_print_output_exists",
    write_code: "vd_print_input_invalid",
    exists_remedy: "Choose an absent PDF path; prints are never overwritten.",
    write_remedy: "Choose a PDF path in a writable directory.",
};

/// One admitted print, decided before the voltage drop is calculated.
pub(super) struct Print {
    capture: voltage_drop_a4::Capture,
    network_config: Value,
    transformer: String,
    pdf: PathBuf,
    lane: String,
    reporter: ds_cli_exec::CapturedExternal,
}

type Preflight = fn(&str) -> Result<Option<ds_cli_exec::CapturedExternal>, Failure>;

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid(INPUT_INVALID.code, message).remedy(INPUT_INVALID.remedy)
}

/// Admit `--print-a4`, or `None` without it. Local checks precede the
/// Reporter preflight, which precedes any calculation.
pub(super) fn admit(
    inputs: &Inputs,
    input: &Path,
    jobs: &[NativeLvJobV1],
) -> Result<Option<Print>, Failure> {
    admit_with(inputs, input, jobs, super::project_run::print_preflight)
}

pub(super) fn admit_with(
    inputs: &Inputs,
    input: &Path,
    jobs: &[NativeLvJobV1],
    preflight: Preflight,
) -> Result<Option<Print>, Failure> {
    let Some(pdf) = inputs.value("print-a4") else {
        return Ok(None);
    };
    if inputs.value("year").is_some()
        || inputs.value("outlook").is_some()
        || inputs.switch("no-outlook")
        || !inputs.repeated("load").is_empty()
        || !inputs.repeated("set").is_empty()
    {
        return Err(invalid(
            "--print-a4 prints the project's own settings; a scenario run is not a project print.",
        ));
    }
    let [job] = jobs else {
        return Err(invalid(
            "--print-a4 prints exactly one exported transformer.",
        ));
    };
    let network_config = Value::Object(job.config_dfs.clone());
    let receipt_path = printing_receipt_path(input);
    let bytes = bounded_read(&receipt_path).map_err(|_| {
        invalid(format!(
            "The sealed printing receipt {} is not beside --input.",
            receipt_path.display()
        ))
    })?;
    let receipt: InputReceipt =
        serde_json::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
    let capture = voltage_drop_a4::capture(&network_config, &receipt).map_err(invalid)?;
    let pdf = absolute_new(Path::new(pdf))?;
    let lane = inputs.require("lane")?.to_owned();
    let reporter = preflight(&lane)?
        .ok_or_else(|| invalid("The Reporter preflight returned no captured Reporter."))?;
    Ok(Some(Print {
        capture,
        network_config,
        transformer: job.transformer_name.clone(),
        pdf,
        lane,
        reporter,
    }))
}

/// The absent PDF as an absolute path in an existing directory: the Reporter
/// and the receipt check name the same file.
fn absolute_new(pdf: &Path) -> Result<PathBuf, Failure> {
    ensure_absent(pdf, &PDF)?;
    let name = pdf
        .file_name()
        .ok_or_else(|| invalid("--print-a4 names no file."))?;
    let parent = pdf
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = std::fs::canonicalize(parent)
        .map_err(|error| invalid(format!("The --print-a4 directory is unavailable: {error}")))?;
    Ok(parent.join(name))
}

fn text(path: &Path) -> Result<&str, Failure> {
    path.to_str()
        .ok_or_else(|| invalid("--print-a4 and --out paths must be UTF-8."))
}

/// Render the written result through the preflighted Reporter and verify
/// the PDF against its receipt. Errors keep their own code and remedy.
pub(super) fn print(print: &Print, result: &Path, result_sha256: &str) -> Result<Value, Failure> {
    let source = std::fs::canonicalize(result).map_err(|error| invalid(error.to_string()))?;
    let request = voltage_drop_a4::request(
        &print.capture,
        &print.network_config,
        &print.transformer,
        text(&source)?,
        result_sha256,
        text(&print.pdf)?,
    );
    let scratch = tempfile::Builder::new()
        .prefix(".ds-vd-a4-")
        .tempdir_in(print.pdf.parent().unwrap_or(Path::new(".")))
        .map_err(|error| invalid(error.to_string()))?;
    let request_path = scratch.path().join("request.json");
    std::fs::write(
        &request_path,
        serde_json::to_vec(&request).map_err(|error| invalid(error.to_string()))?,
    )
    .map_err(|error| invalid(error.to_string()))?;
    let tokens = [
        "--lane",
        &print.lane,
        "--task",
        "voltage-drop",
        "--request",
        text(&request_path)?,
    ]
    .map(str::to_owned);
    let printed = ds_cli_report::export::run_captured(
        &parse(&ds_cli_report::export::COMMAND, &tokens)?,
        &print.reporter,
    )?;
    ds_cli_report::export::verify_voltage_drop_pdf(
        &printed,
        &print.pdf,
        result_sha256,
        &print.transformer,
        &print.capture.project_label,
    )
    .map_err(|message| {
        Failure::failed("reporter_pdf_unverified", message)
            .remedy("print again to an absent --print-a4 path")
    })?;
    Ok(json!({
        "pdf": print.pdf,
        "sha256": printed["output_sha256"],
        "bytes": printed["bytes"],
        "pages": printed["pages"],
        "transformer": print.transformer,
        "project": print.capture.project,
        "project_label": print.capture.project_label,
        "report_format": print.capture.report_format,
        "source_sha256": result_sha256,
        "publication": "nothing_published",
    }))
}

/// A print failure after the result was written: the result stays valid.
pub(super) fn failed(error: Failure, written: Value) -> Failure {
    Failure::failed(
        PRINT_FAILED.code,
        "The voltage-drop result was written; its A4 was not printed.",
    )
    .remedy(PRINT_FAILED.remedy)
    .detail(json!({"result": written, "printed": false, "print_error": {
        "code": error.code(), "message": error.message(),
        "remedy": error.remedy_text(), "detail": error.detail_value()}}))
}

#[cfg(test)]
#[path = "print_a4_tests.rs"]
mod tests;
