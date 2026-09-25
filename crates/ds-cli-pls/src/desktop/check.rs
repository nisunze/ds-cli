//! `ds pls desktop check` — is this desktop ready for the drivers?
//!
//! Reads only. The executable, its digest and its version are judged by the
//! same pinned profile and version test the restore drivers apply, so `check`
//! cannot pass a PLS-CADD a run would then refuse. The Classic interface and
//! the Project Wizard switch have no characterised setting key yet; they are
//! listed for the operator to confirm, with the PLS_CADD.INI lines that
//! mention them as evidence, rather than guessed.

use std::time::Duration;

use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::Value;

use super::bundle::Entry;
use super::run::Invocation;
use super::*;

const TIMEOUT: Duration = Duration::from_secs(300);

pub static COMMAND: Command = Command {
    id: "pls.desktop.check",
    path: &["pls", "desktop", "check"],
    contract: 1,
    summary: "Check this desktop can run the PLS-CADD 16.81 drivers.",
    purpose: "Reads, and changes nothing: whether PLS-CADD is installed where the drivers expect it, whether its digest and version are the pinned 16.81 build, whether it is already running, whether Word is registered for report PDFs, and the Windows PowerShell version. Lists what the operator must confirm by eye, the Classic interface and the Project Wizard switched off, with the PLS_CADD.INI lines that mention them. Run it before a long desktop run.",
    chapter: Chapter::PlsCadd,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "ready and its blockers, the executable's path, sha256 against the pinned sha256 and version against 16.81, running PLS-CADD process ids, whether Word is registered, the PowerShell version, and the settings the operator confirms with their INI evidence.",
    examples: &[Example {
        command: "ds pls desktop check",
        note: "Off Windows this refuses windows_only.",
        runnable: true,
    }],
    refusals: &[
        WINDOWS_ONLY,
        PLS_CADD_NOT_FOUND,
        POWERSHELL_NOT_FOUND,
        DRIVER_FAILED,
        RUN_TIMED_OUT,
        BUNDLE_FAILED,
        RESULT_UNREADABLE,
    ],
    reference: Some("docs/reference/pls.md"),
    search: &[
        "pls-cadd installed",
        "pls-cadd version",
        "desktop readiness",
    ],
    requires: Requires::Server,
    availability: super::availability,
};

pub fn run(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let finished = run::execute(&Invocation::new(Entry::Check, TIMEOUT))?;
    let mut result = failure::outcome(finished, COMMAND.refusals, &[])?;
    if !result.is_object() {
        return Err(
            Failure::failed(RESULT_UNREADABLE.code, "the check returned no report")
                .remedy(RESULT_UNREADABLE.remedy),
        );
    }
    result["drivers"] = Value::String(bundle::digest());
    Ok(result)
}

pub fn render(data: &Value) -> String {
    let blockers: Vec<&str> = data["blockers"]
        .as_array()
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut text = if data["ready"] == true {
        "PLS-CADD desktop ready\n".to_string()
    } else {
        format!("PLS-CADD desktop not ready: {}\n", blockers.join(", "))
    };
    text.push_str(&format!(
        "  executable  {} · version {} · pinned digest {}\n",
        data["executable"]["path"].as_str().unwrap_or(""),
        data["executable"]["file_version"].as_str().unwrap_or("?"),
        if data["executable"]["matches_pin"] == true {
            "matches"
        } else {
            "differs"
        },
    ));
    if let Some(confirm) = data["operator_confirms"].as_array() {
        for item in confirm.iter().filter_map(Value::as_str) {
            text.push_str(&format!("  confirm     {item}\n"));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The blockers the check can name are refusal codes the run verbs
    /// document, so a caller can match them to the refusal they prevent.
    #[test]
    fn the_blockers_the_check_names_are_the_run_verbs_refusal_codes() {
        let entry = bundle::text(Entry::Check.file()).unwrap();
        let documented: Vec<&str> = super::super::deliver::COMMAND
            .refusals
            .iter()
            .map(|refusal| refusal.code)
            .collect();
        let mut named = 0;
        for (at, _) in entry.match_indices("$blockers += '") {
            let rest = &entry[at + "$blockers += '".len()..];
            let code = &rest[..rest.find('\'').unwrap()];
            named += 1;
            if code == "powershell_not_5_1" {
                continue; // only check can see it; a run would fail on its own
            }
            assert!(
                documented.contains(&code),
                "`{code}` is not a deliver refusal"
            );
        }
        assert_eq!(named, 5);
        assert!(entry.contains(
            "Test-PlsExecutableVersion $fileVersion ([string] $DsProfile.ProductVersion)"
        ));
    }

    #[test]
    fn a_report_renders_its_blockers_and_what_to_confirm() {
        let data = serde_json::json!({
            "ready": false,
            "blockers": ["pls_cadd_running"],
            "executable": { "path": PLS_CADD_EXECUTABLE, "file_version": "Version 16.81", "matches_pin": true },
            "operator_confirms": ["PLS-CADD opens with the Classic interface"],
        });
        let text = render(&data);
        assert!(text.starts_with("PLS-CADD desktop not ready: pls_cadd_running"));
        assert!(text.contains("pinned digest matches"));
        assert!(text.contains("confirm     PLS-CADD opens with the Classic interface"));
    }
}
