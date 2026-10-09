//! `ds pls desktop restore` — one fresh native Restore of a backup.
//!
//! The owner is the interim pair proven on the Gisagara rev 3 control and the
//! Nyamagabe exports: `pls-restore-open-interim.ps1` restores into a fresh
//! folder through PLS-CADD's own dialogs, requires every file restored and
//! none skipped, answers only catalogued open prompts and opens the project;
//! `pls-close-interim.ps1` exits without saving and checks the restored tree
//! against the backup's protected members. The entry runs them in that order
//! and keeps both receipts.

use std::time::Duration;

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

use super::bundle::Entry;
use super::run::Invocation;
use super::*;

pub const RECEIPT_SCHEMA: &str = "ds.pls.interim_restore_open.v1";
const TIMEOUT: Duration = Duration::from_secs(2 * 3600);

pub static COMMAND: Command = Command {
    id: "pls.desktop.restore",
    path: &["pls", "desktop", "restore"],
    contract: 1,
    summary: "Restore a .bak natively in PLS-CADD into a new folder and verify it.",
    purpose: "Proves a backup opens where it will be reviewed: PLS-CADD 16.81 restores it through its own Restore dialogs into a new folder, every file restored and none skipped, answers only catalogued open prompts, opens the project, exits without saving, then checks every restored file against the backup, naming each missing path, and the protected members again. A dialog's file count is never taken as success. The restored workspace stays in --into for further work. Use qualify for the two-restore acceptance of a submission.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("bak", "<file.bak>", "The PLS-CADD backup to restore.").required(),
        Arg::value(
            "into",
            "<new folder>",
            "Absent folder off C: that becomes the restored workspace.",
        )
        .required(),
        Arg::value(
            "evidence",
            "<new folder>",
            "Absent folder for the journals; defaults to <into>-evidence.",
        ),
        SHA256_ARG,
        SOURCE_ROOT_ARG,
        PROJECT_FILE_ARG,
    ],
    output: "The receipt path and driver bundle digest, the restored project and folder, the backup's digests, container and member counts, the files present before open, the prompts answered, the post-close full check (every backup file verified, with its digest) and the post-close protected check: files verified and digest against the backup's.",
    examples: &[Example {
        command: r"ds pls desktop restore --bak 'G:\Shared drives\Pro\Working\cap6.bak' --into 'G:\Shared drives\Pro\Working\restore-cap6' --output json",
        note: "Needs the Windows desktop with PLS-CADD 16.81 and no PLS-CADD already open.",
        runnable: false,
    }],
    refusals: &[
        ADAPTERS_NOT_EMBEDDED,
        WINDOWS_ONLY,
        PLS_CADD_NOT_FOUND,
        POWERSHELL_NOT_FOUND,
        SOURCE_NOT_FOUND,
        INVALID_DIGEST,
        BACKUP_DIGEST_MISMATCH,
        INVALID_ARGUMENT,
        OUTPUT_EXISTS,
        OUTPUT_PARENT_MISSING,
        SYSTEM_DRIVE_REFUSED,
        PLS_CADD_RUNNING,
        PLS_CADD_MISMATCH,
        BACKUP_INVALID,
        RESTORED_TREE_MISMATCH,
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
        "native restore",
        "restore backup",
        "open bak",
        "pls-cadd desktop",
    ],
    requires: Requires::Server,
    availability: super::availability,
};

struct Request {
    backup: String,
    sha256: String,
    into: String,
    evidence: String,
    source_root: Option<String>,
    project_file: Option<String>,
}

fn request(inputs: &Inputs) -> Result<Request, Failure> {
    let backup = input_file(inputs.require("bak")?, "bak")?;
    let into = new_folder(inputs.require("into")?, "into")?;
    let evidence_raw = inputs
        .value("evidence")
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-evidence", into.trim_end_matches(['/', '\\'])));
    let evidence = new_folder(&evidence_raw, "evidence")?;
    if evidence == into {
        return Err(
            Failure::invalid(INVALID_ARGUMENT.code, "--evidence must differ from --into")
                .remedy("the restored workspace must hold only what the backup restores"),
        );
    }
    let source_root = optional_text(inputs.value("source-root"), "source-root", false)?;
    let project_file = optional_text(inputs.value("project-file"), "project-file", true)?;
    let sha256 = backup_digest(&backup, inputs.value("sha256"))?;
    Ok(Request {
        backup,
        sha256,
        into,
        evidence,
        source_root,
        project_file,
    })
}

fn invocation(request: &Request) -> Invocation {
    Invocation::new(Entry::Restore, TIMEOUT)
        .value("BackupPath", request.backup.clone())
        .value("ExpectedBackupSha256", request.sha256.clone())
        .value("RestoreDirectory", request.into.clone())
        .value("EvidenceDirectory", request.evidence.clone())
        .optional("SourceRoot", request.source_root.clone())
        .optional("ProjectFileName", request.project_file.clone())
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = request(inputs)?;
    let finished = run::execute(&invocation(&request))?;
    let result = failure::outcome(finished, COMMAND.refusals, &[&request.evidence])?;
    let receipt = result["receipt"].as_str().ok_or_else(|| {
        Failure::failed(RESULT_UNREADABLE.code, "the run named no receipt")
            .remedy(RESULT_UNREADABLE.remedy)
    })?;
    shape(receipt, &read_receipt(receipt)?, &result["close"])
}

/// `restore-open.json` plus the close result, as the answer. The restore's
/// member list and the PLS-CADD log tail stay in the receipt: they are
/// evidence for a reviewer, not something a caller branches on.
fn shape(receipt_path: &str, receipt: &Value, close: &Value) -> Result<Value, Failure> {
    if receipt["schema"] != RECEIPT_SCHEMA {
        return Err(Failure::failed(
            RESULT_UNREADABLE.code,
            format!("the receipt is not {RECEIPT_SCHEMA}"),
        )
        .remedy(RESULT_UNREADABLE.remedy)
        .detail(json!({ "schema": receipt["schema"] })));
    }
    let prompts: Vec<Value> = receipt["open_prompts"]
        .as_array()
        .map(|prompts| {
            prompts
                .iter()
                .map(|prompt| json!({ "name": prompt["name"], "title": prompt["title"] }))
                .collect()
        })
        .unwrap_or_default();
    let mut shaped = provenance(receipt_path);
    shaped["executable"] = receipt["executable"].clone();
    shaped["backup"] = json!({
        "path": receipt["candidate"]["path"],
        "sha256": receipt["candidate"]["sha256"],
        "native_sha256": receipt["candidate"]["native_sha256"],
        "container": receipt["candidate"]["container"],
        "project_file": receipt["candidate"]["project_file"],
        "source_root": receipt["candidate"]["source_root"],
        "counts": receipt["candidate"]["counts"],
    });
    shaped["restore_directory"] = receipt["restore"]["directory"].clone();
    shaped["project"] = receipt["restore"]["project"].clone();
    shaped["files_present_before_open"] = receipt["pre_open_presence"]["verified_files"].clone();
    shaped["open_prompts"] = Value::Array(prompts);
    shaped["repairs"] = json!(receipt["repairs"].as_array().map_or(0, Vec::len));
    shaped["closed"] = close["status"].clone();
    // A restore is reported only when every backup file was verified after
    // close; a Restore dialog's count is not evidence (feedback 1086e058).
    let full = &close["post_close_full"];
    let verified = full["verified_files"].as_u64();
    let expected = full["expected_members"].as_u64();
    if full.get("failed").and_then(Value::as_bool) == Some(true)
        || verified.is_none()
        || verified != expected
    {
        return Err(Failure::failed(
            RESTORED_TREE_MISMATCH.code,
            "the restored folder was not verified file by file against the backup after close",
        )
        .remedy(RESTORED_TREE_MISMATCH.remedy)
        .detail(json!({
            "verified_files": full["verified_files"],
            "expected_files": full["expected_members"],
            "message": full["message"],
            "receipt": receipt_path,
        })));
    }
    shaped["post_close_full"] = json!({
        "verified_files": full["verified_files"],
        "verified_digest": full["verified_digest"],
    });
    shaped["post_close_protected"] = json!({
        "verified_files": close["post_close_protected"]["verified_files"],
        "verified_digest": close["post_close_protected"]["verified_digest"],
        "expected_digest": close["post_close_protected"]["expected_protected"],
    });
    Ok(shaped)
}

pub fn render(data: &Value) -> String {
    format!(
        "PLS-CADD native restore\n  project  {}\n  files    {} present before open · {} verified after close ({} protected)\n  receipt  {}\n",
        data["project"].as_str().unwrap_or(""),
        data["files_present_before_open"],
        data["post_close_full"]["verified_files"],
        data["post_close_protected"]["verified_files"],
        data["receipt"].as_str().unwrap_or(""),
    )
}

#[cfg(all(test, feature = "desktop-adapters"))]
mod tests {
    use super::*;
    use crate::desktop::run::tests::declared_parameters;

    /// A recorded `restore-open.json` (Nyamagabe v17 cap4, 2026-09-24) with
    /// its paths and licensee anonymised and its member list and log tail
    /// shortened; every field and its PowerShell formatting are as written.
    const RESTORE_OPEN: &str = include_str!("../../tests/fixtures/desktop/restore-open.json");

    fn close() -> Value {
        json!({
            "schema": "ds.pls.interim_close.v1",
            "status": "closed_without_saving",
            "process_id": 22496,
            "frame_title_before": "PLS-CADD - example.xyz - 1 - [Profile View]",
            "post_close_full": {
                "verified_files": 46,
                "verified_digest": "4f".repeat(32),
                "expected_members": 46,
            },
            "post_close_protected": {
                "verified_files": 9,
                "verified_digest": "9f".repeat(32),
                "expected_protected": "9f".repeat(32),
                "members": ["normalized_text=4", "exact=5"],
            },
        })
    }

    #[test]
    fn the_recorded_receipt_and_close_become_the_result() {
        let receipt = parse_document(RESTORE_OPEN.as_bytes()).expect("the recorded receipt parses");
        let data = shape(r"G:\ev\restore-open.json", &receipt, &close()).unwrap();
        assert_eq!(data["backup"]["counts"]["files"], 46);
        assert_eq!(data["backup"]["project_file"], "example.xyz");
        assert_eq!(data["files_present_before_open"], 46);
        assert_eq!(data["executable"]["version"], "Version 16.81");
        assert_eq!(data["repairs"], 0);
        let prompts: Vec<&str> = data["open_prompts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|prompt| prompt["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            prompts,
            [
                "about_pls_cadd_16_81",
                "tip_of_the_day",
                "pp_paging_unable_to_cut",
                "pp_paging_no_progress",
                "pp_paging_no_progress"
            ]
        );
        assert_eq!(data["post_close_full"]["verified_files"], 46);
        assert_eq!(data["post_close_protected"]["verified_files"], 9);
        assert_eq!(data["closed"], "closed_without_saving");
        assert!(
            data.get("log_tail").is_none(),
            "the log tail stays in the receipt"
        );
        assert!(render(&data).contains("46 present before open · 46 verified after close"));
    }

    /// Feedback 1086e058: PLS-CADD's Restore dialog reported 88 files restored
    /// while 10 remained. A dialog count is not success: the result exists
    /// only when the post-close check verified every backup file.
    #[test]
    fn a_restore_without_every_backup_file_verified_after_close_is_refused() {
        let receipt = parse_document(RESTORE_OPEN.as_bytes()).expect("the recorded receipt parses");
        let mut short = close();
        short["post_close_full"]["verified_files"] = json!(10);
        short["post_close_full"]["expected_members"] = json!(88);
        let mut failed = close();
        failed["post_close_full"] = json!({
            "failed": true,
            "message": r"Restored member(s) missing: 78 of 88 backup member(s) are absent below G:\r: cables\acsr 70-12mm2",
        });
        let mut absent = close();
        absent.as_object_mut().unwrap().remove("post_close_full");
        for close in [short, failed, absent] {
            let refusal = shape(r"G:\ev\restore-open.json", &receipt, &close).unwrap_err();
            assert_eq!(refusal.code(), "restored_tree_mismatch");
            assert!(COMMAND.refusals.iter().any(|r| r.code == refusal.code()));
            assert_eq!(
                refusal.detail_value().unwrap()["receipt"],
                r"G:\ev\restore-open.json"
            );
        }
    }

    #[test]
    fn every_parameter_passed_is_one_the_entry_declares() {
        let request = Request {
            backup: r"G:\in\a.bak".into(),
            sha256: "0".repeat(64),
            into: r"G:\r1".into(),
            evidence: r"G:\r1-evidence".into(),
            source_root: Some(r"C:\x".into()),
            project_file: Some("a.xyz".into()),
        };
        let invocation = invocation(&request);
        let declared = declared_parameters(Entry::Restore);
        for (name, _) in &invocation.params {
            assert!(declared.iter().any(|d| d == name), "{name} is not declared");
        }
        assert_eq!(invocation.params.len(), 6);
        // …and the entry hands each to the interim driver under its own name.
        let driver =
            crate::desktop::bundle::pls_cadd_text("interim/pls-restore-open-interim.ps1").unwrap();
        for name in [
            "CandidateBackupPath",
            "ExpectedCandidateBackupSha256",
            "RestoreDirectory",
            "EvidenceDirectory",
            "SourceRoot",
            "ProjectFileName",
            "Execute",
        ] {
            assert!(
                driver.contains(&format!("${name}")),
                "the driver has no ${name}"
            );
        }
    }
}
