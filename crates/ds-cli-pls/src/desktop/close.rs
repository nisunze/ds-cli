//! Receipt-pinned recovery of a qualifier-owned stopped native session.
use super::{bundle::Entry, run::Invocation, *};
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::Value;
use std::time::Duration;

pub static COMMAND: Command = Command {
    id: "pls.desktop.close",
    path: &["pls", "desktop", "close"],
    contract: 1,
    summary: "Close a qualifier-owned stopped PLS-CADD session without saving.",
    purpose: "Recover a session left open only after a completed native Backup and an offline protected-content mismatch. The failed run's receipt and journal must prove the same pinned executable, PID, start-time interval, window handle and project leaf. The existing characterized Exit/No-save transition closes that owned session; unknown dialogs, another project, reused PID, missing evidence or an uncompleted native operation refuse. No kill, save, model change or generic native click is available.",
    chapter: Chapter::PlsCadd,
    effect: Effect::ArtifactWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "run",
            "<failed-run>",
            "Existing qualification run with its failure receipt and journal.",
        )
        .required(),
        Arg::value(
            "out",
            "<new-folder>",
            "Absent evidence folder off C: for recovery.",
        )
        .required(),
    ],
    output: "Status closed_without_saving, proved process id, source failed run, recovery receipt and embedded driver digest.",
    examples: &[],
    refusals: &[
        Refusal {
            code: "confirmation_required",
            when: "--yes was not supplied",
            remedy: "review the failed run and authorize closing its owned session without saving",
        },
        ADAPTERS_NOT_EMBEDDED,
        WINDOWS_ONLY,
        PLS_CADD_NOT_FOUND,
        POWERSHELL_NOT_FOUND,
        SOURCE_NOT_FOUND,
        INVALID_ARGUMENT,
        OUTPUT_EXISTS,
        OUTPUT_PARENT_MISSING,
        SYSTEM_DRIVE_REFUSED,
        ATTACH_REFUSED,
        UNKNOWN_DIALOG,
        PLS_CADD_TIMEOUT,
        DRIVER_FAILED,
        RUN_TIMED_OUT,
        BUNDLE_FAILED,
        RESULT_UNREADABLE,
    ],
    reference: Some("docs/reference/pls.md"),
    search: &[
        "close stopped session",
        "recover qualifier",
        "exit without saving",
        "native cleanup",
    ],
    requires: Requires::Server,
    availability: super::availability,
};
pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let root = absolute(inputs.require("run")?)?;
    let failure = input_file(
        &root.join("evidence/failure.json").display().to_string(),
        "run",
    )?;
    let out = new_folder(inputs.require("out")?, "out")?;
    let invocation = Invocation::new(Entry::Close, Duration::from_secs(180))
        .value("FailurePath", failure)
        .value("RunDirectory", out.clone());
    let finished = run::execute(&invocation)?;
    let result = failure::outcome(finished, COMMAND.refusals, &[&out])?;
    let receipt = result["receipt"].as_str().ok_or_else(|| {
        Failure::failed(RESULT_UNREADABLE.code, "recovery named no receipt")
            .remedy(RESULT_UNREADABLE.remedy)
    })?;
    let body = read_receipt(receipt)?;
    if body["schema"] != "ds.pls.stopped_session_closed.v1"
        || body["status"] != "closed_without_saving"
    {
        return Err(Failure::failed(
            RESULT_UNREADABLE.code,
            "recovery did not prove no-save exit",
        )
        .remedy(RESULT_UNREADABLE.remedy));
    }
    let mut shaped = provenance(receipt);
    shaped["status"] = body["status"].clone();
    shaped["process_id"] = body["process_id"].clone();
    shaped["source_run"] = body["source_run"].clone();
    Ok(shaped)
}
pub fn render(data: &Value) -> String {
    format!(
        "PLS-CADD recovery: {} (PID {})\\n",
        data["status"], data["process_id"]
    )
}
#[cfg(all(test, feature = "desktop-adapters"))]
mod tests {
    use super::*;
    use crate::desktop::run::tests::declared_parameters;
    #[test]
    fn close_has_only_receipt_and_evidence_parameters() {
        let declared = declared_parameters(Entry::Close);
        for field in ["FailurePath", "RunDirectory"] {
            assert!(declared.iter().any(|p| p == field));
        }
        assert_eq!(COMMAND.args.len(), 2);
        assert!(COMMAND.requires_confirmation());
        assert!(
            COMMAND
                .refusals
                .iter()
                .any(|r| r.code == "confirmation_required")
        );
    }
}
