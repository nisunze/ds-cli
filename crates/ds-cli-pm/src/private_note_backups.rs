//! Thin private backup controls over the same explicit notes command owner.
use crate::private_notes::{ID, NOTE, VERSION, declaration};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Command, Effect, Refusal},
};
use ds_client_core::personal_notes::Action;
use serde_json::{Value, json};
const BACKUP_ID: Arg = Arg::value(
    "backup",
    "<backup-id>",
    "Exact verified backup id returned by notes backup.",
)
.required();
const SHA: Arg = Arg::value(
    "backup-sha256",
    "<sha256>",
    "Exact lowercase SHA256 of that verified portable backup.",
)
.required();
const OUT: Arg = Arg::value(
    "out",
    "<absolute.json>",
    "Optional new absolute .json output; exact archive bytes, no overwrite. Without this, returns the checked descriptor only.",
);
pub static BACKUP: Command = declaration(
    "notes.backup",
    &["notes", "backup"],
    "Capture one verified private note backup.",
    "With --yes, captures the selected current note and every available revision against --version and stable --id. Independently stored-byte verified, at most 400 snapshots and 512 KiB; larger archives refuse completely. No source retirement, attachment bytes or fabricated older history. Exact retries reuse the backup.",
    Effect::GlobalWrite,
    &[NOTE, VERSION, ID, crate::LANE_ARG],
);
const EXPORT_REFUSALS: [Refusal; 28] = {
    let output_exists = Refusal {
        code: "notes_output_exists",
        when: "the explicit backup output already exists or appears during acquisition",
        remedy: "choose a new absolute .json path; existing files are preserved",
    };
    let mut out = [output_exists; 28];
    let mut i = 0;
    while i < crate::checklist_notes::PRIVATE_REFUSALS.len() {
        out[i] = crate::checklist_notes::PRIVATE_REFUSALS[i];
        i += 1;
    }
    out
};
pub static READ: Command = {
    let mut command = declaration(
        "notes.backup.read",
        &["notes", "backup", "read"],
        "Read or export one exact verified private backup.",
        "Select the backup id and exact SHA256 returned by notes backup. The native owner measures archive bytes and verifies scope, history and descriptor before output. Optional --out writes a new private .json file without reformatting or overwrite; absent --out returns only the descriptor, never a large body dump. No upload or account migration.",
        Effect::LocalFileWrite,
        &[NOTE, BACKUP_ID, SHA, OUT, crate::LANE_ARG],
    );
    command.refusals = &EXPORT_REFUSALS;
    command
};
pub static RESTORE: Command = declaration(
    "notes.backup.restore",
    &["notes", "backup", "restore"],
    "Restore a selected verified private backup as a fresh revision.",
    "With --yes, restores exact verified backup content against the current --version and stable --id. Keeps current identity, creation attribution, archive and operational reminder, clears trash, and recovers only genuinely retained missing history after collision checks. Does not schedule reminders, recreate a missing note or accept arbitrary uploaded JSON. Exact replay returns the original receipt.",
    Effect::GlobalWrite,
    &[NOTE, VERSION, BACKUP_ID, SHA, ID, crate::LANE_ARG],
);
fn invalid(message: impl std::fmt::Display) -> Failure {
    Failure::invalid("notes_request_invalid", message.to_string())
        .remedy("Use one private note, its reviewed current version and an exact verified backup reference.")
}
fn version(i: &Inputs) -> Result<i64, Failure> {
    i.require("version")?.parse::<i64>().map_err(invalid)
}
fn action(i: &Inputs, command: &str) -> Result<Action, Failure> {
    let id = i.require("note")?.to_owned();
    let action = match command {
        "notes.backup" => Action::Backup {
            id,
            expected_version: version(i)?,
            command_id: i.require("id")?.into(),
        },
        "notes.backup.read" => Action::ReadBackup {
            id,
            backup_id: i.require("backup")?.into(),
            backup_sha256: i.require("backup-sha256")?.into(),
        },
        "notes.backup.restore" => Action::RestoreBackup {
            id,
            expected_version: version(i)?,
            backup_id: i.require("backup")?.into(),
            backup_sha256: i.require("backup-sha256")?.into(),
            command_id: i.require("id")?.into(),
        },
        _ => unreachable!("closed backup command"),
    };
    action.validate().map_err(invalid)?;
    Ok(action)
}
pub fn backup(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let action = action(i, "notes.backup")?;
    ds_cli_auth::personal_notes::execute(i.value("lane").unwrap_or("stable"), &action)
}
pub fn restore(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let action = action(i, "notes.backup.restore")?;
    ds_cli_auth::personal_notes::execute(i.value("lane").unwrap_or("stable"), &action)
}
pub fn read(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let action = action(i, "notes.backup.read")?;
    let out = i
        .value("out")
        .map(ds_cli_auth::personal_notes::backup_export::admit)
        .transpose()?;
    let lane = i.value("lane").unwrap_or("stable");
    let fence = ds_cli_auth::personal_notes::backup_export::Captured::capture(lane)?;
    let mut data = ds_cli_auth::personal_notes::execute(lane, &action)?;
    fence.verify()?;
    let bytes = data["archive_json"]
        .as_str()
        .expect("core verified archive bytes")
        .as_bytes();
    if let Some(path) = &out {
        fence.verify()?;
        ds_cli_auth::personal_notes::backup_export::write(path, bytes)?;
    }
    data.as_object_mut()
        .expect("core verified backup object")
        .remove("archive_json");
    data["output"] = json!(out);
    Ok(data)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declared_backup_controls_share_owner_and_honest_effects() {
        assert_eq!(BACKUP.effect, Effect::GlobalWrite);
        assert_eq!(RESTORE.effect, Effect::GlobalWrite);
        assert_eq!(READ.effect, Effect::LocalFileWrite);
        assert!(
            READ.refusals
                .iter()
                .any(|r| r.code == "notes_output_exists")
        );
        for command in [&BACKUP, &READ, &RESTORE] {
            assert!(
                command
                    .args
                    .iter()
                    .all(|a| !["project", "owner", "upload"].contains(&a.name))
            );
        }
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    fn inputs(command: &Command, args: &[&str]) -> Inputs {
        ds_cli_contract::args::parse(
            command,
            &args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn exact_backup_reference_maps_and_writes_require_confirmation() {
        let sha = "a".repeat(64);
        let i = inputs(
            &RESTORE,
            &[
                "--note",
                "private",
                "--version",
                "8",
                "--backup",
                "pb_test",
                "--backup-sha256",
                &sha,
                "--id",
                "restore-command",
            ],
        );
        assert_eq!(
            action(&i, RESTORE.id).unwrap().payload(),
            json!({"action":"restore_backup","id":"private","expected_version":8,"backup_id":"pb_test","backup_sha256":sha,"command_id":"restore-command"})
        );
        assert!(RESTORE.confirmation_required_for(&i));
        let i = inputs(
            &BACKUP,
            &[
                "--note",
                "private",
                "--version",
                "0",
                "--id",
                "backup-command",
            ],
        );
        assert_eq!(
            backup(
                &i,
                &Context {
                    confirmed: true,
                    output: ds_cli_contract::Output::resolve(
                        ds_cli_contract::Format::Json,
                        false,
                        true
                    )
                }
            )
            .unwrap_err()
            .code(),
            "notes_request_invalid"
        );
    }
    #[test]
    fn invalid_export_path_refuses_before_authentication() {
        let sha = "a".repeat(64);
        let i = inputs(
            &READ,
            &[
                "--note",
                "private",
                "--backup",
                "pb_test",
                "--backup-sha256",
                &sha,
                "--out",
                "relative.json",
            ],
        );
        assert_eq!(
            read(
                &i,
                &Context {
                    confirmed: true,
                    output: ds_cli_contract::Output::resolve(
                        ds_cli_contract::Format::Json,
                        false,
                        true
                    )
                }
            )
            .unwrap_err()
            .code(),
            "notes_request_invalid"
        );
    }
}
