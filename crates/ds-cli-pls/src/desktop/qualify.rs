//! `ds pls desktop qualify` — the two-restore native acceptance of a backup.
//!
//! `pls-backup-restore-qualify.ps1` owns it: Restore and open, PLS File >
//! Backup of the untouched project, close without saving, Full and Protected
//! checks of the first tree; then Restore that fresh PLS backup, close, and
//! check the second tree against the fresh backup and against the candidate.
//! It is the native Restore/reopen gate `ds pls backup-create` leaves open.
//! This command lays out its four targets in one new run folder and reads its
//! `manifest.json`.

use std::time::Duration;

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

use super::bundle::Entry;
use super::run::Invocation;
use super::*;

pub const RECEIPT_SCHEMA: &str = "ds.pls.backup_restore_qualification.v1";
const TIMEOUT: Duration = Duration::from_secs(4 * 3600);

pub static COMMAND: Command = Command {
    id: "pls.desktop.qualify",
    path: &["pls", "desktop", "qualify"],
    contract: 1,
    summary: "Qualify a .bak by two native PLS-CADD restores and a PLS backup.",
    purpose: "The native Restore/reopen acceptance a submission backup needs, and that backup-create cannot give itself: PLS-CADD 16.81 restores the backup into a new folder and opens it, makes its own File > Backup of the untouched project, closes without saving and verifies every restored file; then restores that fresh backup, closes, and verifies the second tree against both backups. No save is authorised at any point. On an unexpected dialog the qualifier leaves PLS-CADD open for the operator rather than closing it blind.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "bak",
            "<file.bak>",
            "The candidate PLS-CADD backup to qualify.",
        )
        .required(),
        Arg::value(
            "out",
            "<new folder>",
            "Absent run folder off C: for both restores, the PLS backup and the evidence.",
        )
        .required(),
        SHA256_ARG,
        SOURCE_ROOT_ARG,
        PROJECT_FILE_ARG,
    ],
    output: "The receipt path and driver bundle digest, the status, the candidate's digests, project and counts, both restores' verification verdicts, the fresh PLS backup's path and digest and whether its protected content equals the candidate's, the repair count, and the SAPS caveat.",
    examples: &[Example {
        command: r"ds pls desktop qualify --bak 'G:\Shared drives\Pro\Working\submission.bak' --out 'G:\Shared drives\Pro\Working\qualify-submission' --output json",
        note: "Run after `ds pls backup-create`; its receipt asks for exactly this gate.",
        runnable: false,
    }],
    refusals: &[
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
        "restore reopen",
        "submission acceptance",
        "restore twice",
        "pls-cadd desktop",
    ],
    requires: Requires::Server,
    availability: super::availability,
};

struct Request {
    backup: String,
    sha256: String,
    out: String,
    source_root: Option<String>,
    project_file: Option<String>,
}

fn request(inputs: &Inputs) -> Result<Request, Failure> {
    let backup = input_file(inputs.require("bak")?, "bak")?;
    let out = new_folder(inputs.require("out")?, "out")?;
    let source_root = optional_text(inputs.value("source-root"), "source-root", false)?;
    let project_file = optional_text(inputs.value("project-file"), "project-file", true)?;
    let sha256 = backup_digest(&backup, inputs.value("sha256"))?;
    Ok(Request {
        backup,
        sha256,
        out,
        source_root,
        project_file,
    })
}

fn invocation(request: &Request) -> Invocation {
    Invocation::new(Entry::Qualify, TIMEOUT)
        .value("BackupPath", request.backup.clone())
        .value("ExpectedBackupSha256", request.sha256.clone())
        .value("RunDirectory", request.out.clone())
        .optional("SourceRoot", request.source_root.clone())
        .optional("ProjectFileName", request.project_file.clone())
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
    let candidate = &receipt["candidate"];
    let fresh = &receipt["fresh_pls_backup"];
    let mut shaped = provenance(receipt_path);
    shaped["status"] = receipt["status"].clone();
    shaped["executable"] = receipt["executable"].clone();
    shaped["candidate"] = json!({
        "path": candidate["path"],
        "sha256": candidate["container_sha256"],
        "native_sha256": candidate["native_sha256"],
        "project_file": candidate["project_file"],
        "counts": candidate["counts"],
    });
    shaped["first_restore"] = receipt["first_restore"].clone();
    shaped["fresh_pls_backup"] = json!({
        "path": fresh["path"],
        "sha256": fresh["container_sha256"],
        "protected_equal_to_candidate": fresh["protected_equal_to_candidate"],
        "save_before_backup_authorized": fresh["save_before_backup_authorized"],
    });
    shaped["second_restore"] = receipt["second_restore"].clone();
    shaped["repairs"] = receipt["repairs"]["count"].clone();
    shaped["known_dialogs"] = receipt["warnings"]["known_dialog_count"].clone();
    shaped["caveat"] = json!({
        "code": receipt["caveat"]["code"],
        "shipment_grade": receipt["caveat"]["shipment_grade"],
        "text": receipt["caveat"]["text"],
    });
    Ok(shaped)
}

pub fn render(data: &Value) -> String {
    format!(
        "PLS-CADD two-restore qualification · {}\n  candidate  {}\n  fresh PLS backup protected-equal: {}\n  caveat     {}\n  receipt    {}\n",
        data["status"].as_str().unwrap_or(""),
        data["candidate"]["path"].as_str().unwrap_or(""),
        data["fresh_pls_backup"]["protected_equal_to_candidate"],
        data["caveat"]["code"].as_str().unwrap_or(""),
        data["receipt"].as_str().unwrap_or(""),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::run::tests::declared_parameters;

    /// A `manifest.json` written field for field from the qualifier's own
    /// `$manifest` block, in PowerShell 5.1's JSON layout. No successful
    /// qualification manifest is kept on the server to record; the qualifier's
    /// recorded failure record is exercised in `failure.rs`.
    const MANIFEST: &str = include_str!("../../tests/fixtures/desktop/manifest.json");

    #[test]
    fn the_manifest_becomes_the_result() {
        let receipt = parse_document(MANIFEST.as_bytes()).unwrap();
        let data = shape(r"G:\q\evidence\manifest.json", &receipt).unwrap();
        assert_eq!(data["status"], "qualified_backup_roundtrip_local_only");
        assert_eq!(data["candidate"]["project_file"], "example.xyz");
        assert_eq!(data["candidate"]["counts"]["files"], 46);
        assert_eq!(
            data["fresh_pls_backup"]["protected_equal_to_candidate"],
            true
        );
        assert_eq!(
            data["fresh_pls_backup"]["save_before_backup_authorized"],
            false
        );
        assert_eq!(
            data["second_restore"]["full_tree_verified_against_fresh_backup"],
            true
        );
        assert_eq!(data["repairs"], 0);
        assert_eq!(data["caveat"]["code"], "saps_unlicensed_pls_16_81");
        assert_eq!(data["caveat"]["shipment_grade"], false);
        assert!(render(&data).contains("protected-equal: true"));
    }

    /// Every key read above is one the qualifier writes. The sample is
    /// written from the script, so this is the check that it still is.
    #[test]
    fn every_manifest_key_read_is_written_by_the_qualifier() {
        let qualifier = crate::desktop::bundle::text("pls-backup-restore-qualify.ps1").unwrap();
        for key in [
            "schema = 'ds.pls.backup_restore_qualification.v1'",
            "status = 'qualified_backup_roundtrip_local_only'",
            "container_sha256 = $actualCandidateDigest",
            "native_sha256 = $candidateInventory.native_backup_sha256",
            "project_file = $candidateInventory.project_file",
            "first_restore = [ordered]@{",
            "fresh_pls_backup = [ordered]@{",
            "protected_equal_to_candidate = $protectedComparison.equal",
            "save_before_backup_authorized = [bool] $AuthorizeSaveBeforeBackup",
            "second_restore = [ordered]@{",
            "count = $repairEvidence.Count",
            "known_dialog_count = $promptEvidence.Count",
            "code = 'saps_unlicensed_pls_16_81'",
            "shipment_grade = $false",
        ] {
            assert!(
                qualifier.contains(key),
                "the qualifier no longer writes `{key}`"
            );
        }
    }

    #[test]
    fn every_parameter_passed_is_one_the_entry_declares() {
        let request = Request {
            backup: r"G:\in\a.bak".into(),
            sha256: "0".repeat(64),
            out: r"G:\q".into(),
            source_root: Some(r"C:\x".into()),
            project_file: Some("a.xyz".into()),
        };
        let invocation = invocation(&request);
        let declared = declared_parameters(Entry::Qualify);
        for (name, _) in &invocation.params {
            assert!(declared.iter().any(|d| d == name), "{name} is not declared");
        }
        assert_eq!(invocation.params.len(), 5);
    }
}
