//! `ds pls desktop deliver` — a DS-exported backup to the full deliverable set.
//!
//! `pls-deliver-autosag.ps1` owns the chain and proved it end to end on the
//! v19 cap6 export (ds-work `ea67e9b`): a working session that AutoSags,
//! pages, saves, gates on Section Usage and backs up; then every deliverable
//! from a fresh restore of that backup, because a report made in the working
//! session can differ from what a reviewer opening the backup sees. This
//! command validates its flags, runs that chain, and reads its `deliver.json`.

use std::time::Duration;

use ds_cli_contract::args::INVALID_NUMBER;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

use super::bundle::Entry;
use super::run::Invocation;
use super::*;

pub const RECEIPT_SCHEMA: &str = "ds.pls.deliver_autosag.v4";

/// Six reports at their own bound each, plus two restores, AutoSag, the
/// backup and the sheets. A run past this is not making progress.
const BASE_TIMEOUT: Duration = Duration::from_secs(6 * 3600);

pub const INVALID_LABEL: Refusal = Refusal {
    code: "invalid_label",
    when: "--label, or the --bak name it defaults to, is not a plain name",
    remedy: "pass --label with letters, digits, '.', '_' or '-', starting with a letter or digit",
};

pub static COMMAND: Command = Command {
    id: "pls.desktop.deliver",
    path: &["pls", "desktop", "deliver"],
    contract: 1,
    summary: "Turn a DS-exported .bak into the full PLS-CADD deliverable set.",
    purpose: "Runs the proven deliver chain in PLS-CADD 16.81 on the Windows desktop, unattended: a fresh native Restore of the backup, AutoSag of every section through the Section Table, plan & profile paging settings, save, a Section Usage gate, and PLS File > Backup; then, from a fresh Restore of that new backup, the six reports as RTF with their verdict lines, every plan & profile sheet as one PDF (skipped with --no-sheets), and A3 landscape report PDFs made from the RTFs with Word. Everything lands in one new run folder off C:. A dialog the catalogue does not know stops the run; PLS-CADD is then left open for inspection.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "bak",
            "<file.bak>",
            "The DS-exported PLS-CADD backup to deliver.",
        )
        .required(),
        Arg::value(
            "out",
            "<new folder>",
            "Absent run folder off C: that receives every output.",
        )
        .required(),
        SHA256_ARG,
        Arg::value(
            "label",
            "<name>",
            "Name of the delivered backup; defaults to the --bak name.",
        ),
        SOURCE_ROOT_ARG,
        Arg::value(
            "paging-gap",
            "<metres>",
            "Station gap between alignments on the sheets; two decimals at most.",
        )
        .default("100"),
        Arg::switch(
            "no-sheets",
            "Skip the plan & profile sheet PDF; everything else runs.",
        ),
        REPORT_TIMEOUT_ARG,
    ],
    output: "The receipt path and driver bundle digest, the delivered backup's path, sha256 and size, the Section Usage gate after AutoSag, the paging settings read back, each report's RTF, PDF and verdict counts from the fresh restore, and the sheet PDF, or null with --no-sheets.",
    examples: &[Example {
        command: r"ds pls desktop deliver --bak 'G:\Shared drives\Pro\Working\cap6.bak' --out 'G:\Shared drives\Pro\Working\deliver-cap6' --no-sheets --output json",
        note: "Hours on a large model; run it where nothing else uses the desktop.",
        runnable: false,
    }],
    refusals: &[
        WINDOWS_ONLY,
        PLS_CADD_NOT_FOUND,
        POWERSHELL_NOT_FOUND,
        SOURCE_NOT_FOUND,
        INVALID_DIGEST,
        BACKUP_DIGEST_MISMATCH,
        INVALID_LABEL,
        INVALID_NUMBER,
        INVALID_ARGUMENT,
        OUTPUT_EXISTS,
        OUTPUT_PARENT_MISSING,
        SYSTEM_DRIVE_REFUSED,
        PLS_CADD_RUNNING,
        PLS_CADD_MISMATCH,
        BACKUP_INVALID,
        RESTORED_TREE_MISMATCH,
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
        "autosag",
        "deliverables",
        "plan and profile pdf",
        "rtf reports",
        "pls-cadd desktop",
    ],
    requires: Requires::Server,
    availability: super::availability,
};

/// The validated request, before it becomes a PowerShell invocation.
struct Request {
    backup: String,
    sha256: String,
    out: String,
    label: String,
    source_root: Option<String>,
    paging_gap: String,
    no_sheets: bool,
    report_timeout: String,
}

fn request(inputs: &Inputs) -> Result<Request, Failure> {
    let backup = input_file(inputs.require("bak")?, "bak")?;
    let out = new_folder(inputs.require("out")?, "out")?;
    let label = match inputs.value("label") {
        Some(label) => checked_label(label)?,
        None => default_label(&backup)?,
    };
    let paging_gap = paging_gap(inputs.value("paging-gap"))?;
    let report_timeout = report_timeout(inputs.value("report-timeout"))?;
    let source_root = optional_text(inputs.value("source-root"), "source-root", false)?;
    let sha256 = backup_digest(&backup, inputs.value("sha256"))?;
    Ok(Request {
        backup,
        sha256,
        out,
        label,
        source_root,
        paging_gap,
        no_sheets: inputs.switch("no-sheets"),
        report_timeout,
    })
}

fn invocation(request: &Request) -> Invocation {
    let report_seconds: u64 = request.report_timeout.parse().unwrap_or(1800);
    Invocation::new(
        Entry::Deliver,
        BASE_TIMEOUT + Duration::from_secs(6 * report_seconds),
    )
    .value("BackupPath", request.backup.clone())
    .value("ExpectedBackupSha256", request.sha256.clone())
    .value("RunDirectory", request.out.clone())
    .value("Label", request.label.clone())
    .optional("SourceRoot", request.source_root.clone())
    .value("AlignmentGap", request.paging_gap.clone())
    .value("ReportTimeoutSeconds", request.report_timeout.clone())
    .switch("NoSheets", request.no_sheets)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = request(inputs)?;
    let finished = run::execute(&invocation(&request))?;
    let result = failure::outcome(finished, COMMAND.refusals, &[&request.out])?;
    let receipt_path = receipt_path(&result)?;
    shape(&receipt_path, &read_receipt(&receipt_path)?)
}

fn receipt_path(result: &Value) -> Result<String, Failure> {
    result["receipt"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| {
            Failure::failed(RESULT_UNREADABLE.code, "the run named no receipt")
                .remedy(RESULT_UNREADABLE.remedy)
        })
}

/// `deliver.json` as the result: its facts, regrouped for a caller, nothing
/// recomputed. A receipt of another schema is refused rather than half-read.
fn shape(receipt_path: &str, receipt: &Value) -> Result<Value, Failure> {
    if receipt["schema"] != RECEIPT_SCHEMA {
        return Err(Failure::failed(
            RESULT_UNREADABLE.code,
            format!("the receipt is not {RECEIPT_SCHEMA}"),
        )
        .remedy(RESULT_UNREADABLE.remedy)
        .detail(json!({ "schema": receipt["schema"] })));
    }
    let session = &receipt["working_session"];
    let fresh = &receipt["deliverables_from_fresh_restore"];
    let mut shaped = provenance(receipt_path);
    shaped["label"] = receipt["label"].clone();
    shaped["source"] = receipt["source"].clone();
    shaped["backup"] = receipt["backup"].clone();
    shaped["gate_section_usage"] = session["gate_section_usage"].clone();
    shaped["autosag"] = session["autosag"].clone();
    shaped["paging"] = session["paging"].clone();
    shaped["restored_files"] = json!({
        "working_session": session["restored_files"],
        "fresh_restore": fresh["restored_files"],
    });
    shaped["reports"] = fresh["reports"].clone();
    shaped["sheets"] = fresh["sheets"].clone();
    Ok(shaped)
}

/// The driver's own pattern is `^[A-Za-z0-9._-]+$`. The first character is
/// held to a letter or digit as well: PowerShell reads a value beginning with
/// `-` as a parameter name.
fn checked_label(raw: &str) -> Result<String, Failure> {
    let plain = raw.len() <= 100
        && raw.starts_with(|c: char| c.is_ascii_alphanumeric())
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !plain {
        return Err(Failure::invalid(
            INVALID_LABEL.code,
            format!("`{raw}` is not a plain delivery name"),
        )
        .remedy(INVALID_LABEL.remedy));
    }
    Ok(raw.to_string())
}

/// The `--bak` file's own name, with every character the driver refuses
/// replaced by `_`: `Final Huye Gisagara.bak` delivers `Final_Huye_Gisagara.bak`.
fn default_label(backup: &str) -> Result<String, Failure> {
    let leaf = backup.rsplit(['/', '\\']).next().unwrap_or(backup);
    let stem = leaf.rsplit_once('.').map_or(leaf, |(stem, _)| stem);
    let mapped: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    checked_label(&mapped)
}

/// The gap as the driver writes and reads it back: it types the value with
/// two decimals and refuses the run when the readback differs, so a third
/// decimal can never take. Refused here instead, before hours of work.
fn paging_gap(raw: Option<&str>) -> Result<String, Failure> {
    let raw = raw.unwrap_or("100");
    let (whole, fraction) = raw.split_once('.').unwrap_or((raw, ""));
    let digits = |text: &str| text.bytes().all(|byte| byte.is_ascii_digit());
    let valid = !whole.is_empty()
        && whole.len() <= 6
        && digits(whole)
        && fraction.len() <= 2
        && digits(fraction)
        && !(raw.contains('.') && fraction.is_empty());
    if !valid {
        return Err(Failure::invalid(
            "invalid_number",
            format!("`--paging-gap` `{raw}` is not a gap in metres with at most two decimals"),
        )
        .remedy("pass 0 to 999999.99 metres, e.g. 100 or 12.5"));
    }
    Ok(raw.to_string())
}

pub fn render(data: &Value) -> String {
    let mut text = format!(
        "PLS-CADD deliverables · {}\n  backup   {} ({} bytes)\n  gate     {} section violations after AutoSag\n",
        data["label"].as_str().unwrap_or(""),
        data["backup"]["path"].as_str().unwrap_or(""),
        data["backup"]["bytes"],
        data["gate_section_usage"]["section_violations"],
    );
    if let Some(reports) = data["reports"].as_object() {
        for (name, report) in reports {
            text.push_str(&format!(
                "  report   {name}: {}\n",
                report["pdf"]
                    .as_str()
                    .or(report["rtf"].as_str())
                    .unwrap_or("")
            ));
        }
    }
    match data["sheets"]["pdf"].as_str() {
        Some(pdf) => text.push_str(&format!("  sheets   {pdf}\n")),
        None => text.push_str("  sheets   skipped\n"),
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
    use crate::desktop::run::tests::declared_parameters;

    /// A `deliver.json` in the exact shape `pls-deliver-autosag.ps1` writes
    /// (Windows PowerShell 5.1 `ConvertTo-Json -Depth 8`: CRLF, the wide
    /// two-space colon gap, `\u0026` for `&`), with the counts of the proven
    /// v19 cap6 run: gate 0, 3 structure NG, 0 of 666 clearance spans. The
    /// paths are anonymised; no deliver.json from that run is kept on the
    /// server, so this sample is written from the script, not captured.
    const DELIVER_JSON: &str = include_str!("../../tests/fixtures/desktop/deliver.json");

    fn shaped() -> Value {
        let receipt: Value = parse_document(DELIVER_JSON.as_bytes()).expect("the sample parses");
        shape(r"G:\Working\deliver-cap6\deliver.json", &receipt).expect("v4 is read")
    }

    #[test]
    fn the_receipt_becomes_the_result_without_recomputing_anything() {
        let data = shaped();
        assert_eq!(data["label"], "cap6");
        assert_eq!(data["backup"]["bytes"], 5_322_311);
        assert_eq!(data["gate_section_usage"]["section_violations"], 0);
        assert_eq!(data["paging"]["alignment_gap_m"], 100.0);
        assert_eq!(data["paging"]["page_start_rounding"], "do_not_round");
        assert_eq!(data["restored_files"]["fresh_restore"], 44);
        let reports = data["reports"].as_object().unwrap();
        assert_eq!(reports.len(), 6);
        assert_eq!(
            reports["Structure Usage"]["verdict"]["structure_violations"],
            3
        );
        assert_eq!(
            reports["Terrain Clearances"]["verdict"]["clearance_spans_ok"],
            666
        );
        assert_eq!(
            reports["Wind & Weight Span"]["pdf"],
            r"G:\Working\deliver-cap6\reports\Wind & Weight Span.pdf"
        );
        assert_eq!(data["sheets"]["bytes"], 48_912_004);
        assert_eq!(data["receipt"], r"G:\Working\deliver-cap6\deliver.json");
        assert!(data["drivers"].as_str().unwrap().starts_with("sha256:"));
        let text = render(&data);
        assert!(text.contains("0 section violations after AutoSag"));
        assert!(text.contains("Plan and Profile.pdf"));
    }

    #[test]
    fn a_run_without_sheets_reports_them_skipped() {
        let mut receipt: Value = parse_document(DELIVER_JSON.as_bytes()).unwrap();
        receipt["deliverables_from_fresh_restore"]["sheets"] = Value::Null;
        let data = shape("r", &receipt).unwrap();
        assert!(data["sheets"].is_null());
        assert!(render(&data).contains("sheets   skipped"));
    }

    #[test]
    fn a_receipt_of_another_schema_is_refused() {
        let mut receipt: Value = parse_document(DELIVER_JSON.as_bytes()).unwrap();
        receipt["schema"] = json!("ds.pls.deliver_autosag.v3");
        assert_eq!(
            shape("r", &receipt).unwrap_err().code(),
            "driver_result_unreadable"
        );
    }

    #[test]
    fn labels_are_plain_names_and_default_from_the_backup() {
        assert_eq!(
            default_label(r"G:\in\Final Huye Gisagara.bak").unwrap(),
            "Final_Huye_Gisagara"
        );
        assert_eq!(default_label("/in/cap6.v19.bak").unwrap(), "cap6.v19");
        assert_eq!(checked_label("Rev-A2_final").unwrap(), "Rev-A2_final");
        for bad in ["-x", ".hidden", "a b", "", "é"] {
            assert_eq!(
                checked_label(bad).unwrap_err().code(),
                "invalid_label",
                "{bad}"
            );
        }
        assert_eq!(
            default_label(r"G:\in\ .bak").unwrap_err().code(),
            "invalid_label"
        );
    }

    #[test]
    fn the_paging_gap_must_survive_the_driver_s_two_decimal_readback() {
        assert_eq!(paging_gap(None).unwrap(), "100");
        for good in ["0", "1", "12.5", "100.25", "999999.99"] {
            assert_eq!(paging_gap(Some(good)).unwrap(), good);
        }
        for bad in [
            "100.125", "-1", "1e3", "abc", "", "12.", ".5", "1000000", "1,5",
        ] {
            assert_eq!(
                paging_gap(Some(bad)).unwrap_err().code(),
                "invalid_number",
                "{bad}"
            );
        }
    }

    #[test]
    fn every_parameter_passed_is_one_the_entry_declares() {
        let request = Request {
            backup: r"G:\in\cap6.bak".into(),
            sha256: "0".repeat(64),
            out: r"G:\out\run".into(),
            label: "cap6".into(),
            source_root: Some(r"C:\nyamagabe".into()),
            paging_gap: "100".into(),
            no_sheets: true,
            report_timeout: "1800".into(),
        };
        let invocation = invocation(&request);
        assert_eq!(invocation.entry, Entry::Deliver);
        let declared = declared_parameters(Entry::Deliver);
        for (name, _) in &invocation.params {
            assert!(declared.iter().any(|d| d == name), "{name} is not declared");
        }
        assert_eq!(invocation.params.len(), 8, "every flag reaches the driver");
        // The entry passes each one on to the chain under the chain's own name.
        let entry = crate::desktop::bundle::text(Entry::Deliver.file()).unwrap();
        let chain = crate::desktop::bundle::text("pls-deliver-autosag.ps1").unwrap();
        for name in [
            "BackupPath",
            "ExpectedBackupSha256",
            "RunDirectory",
            "Label",
            "SourceRoot",
            "AlignmentGap",
            "ReportTimeoutSeconds",
            "NoSheets",
        ] {
            assert!(
                chain.contains(&format!("${name}")),
                "the chain has no ${name}"
            );
            assert!(
                entry.contains(&format!("{name} = ")) || entry.contains(&format!("$a.{name} = ")),
                "the entry does not pass {name}"
            );
        }
    }
}
