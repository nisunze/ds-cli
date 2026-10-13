//! Explicit small-data canonical migration through the private owner.
use crate::private_notes::{ID, NOTE, declaration};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Command, Effect},
};
use ds_client_core::personal_notes::{Action, MigrationPlan};
use serde_json::Value;
use std::io::Read;

const INPUT:Arg=Arg::value("input","<reviewed-plan.json>","Exact reviewed plan from notes canonicalize prepare; closed JSON at most 8 KiB. No source data edits or identity inputs.").required();
pub static PREPARE: Command = declaration(
    "notes.canonicalize.prepare",
    &["notes", "canonicalize", "prepare"],
    "Prepare an exact private-note canonical migration without writing.",
    "Reads only the selected stored note and its current immutable snapshot guard. Returns an explicit reviewable plan with raw source SHA256, Firestore update time, unchanged authored version and target SHA256. Old geometry/missing identity is decoded only here; no read-time healing, guessed past versions or automatic application. Capture the returned plan as JSON, review it, then use notes canonicalize apply.",
    Effect::ReadOnly,
    &[NOTE, crate::LANE_ARG],
);
pub static APPLY: Command = declaration(
    "notes.canonicalize.apply",
    &["notes", "canonicalize", "apply"],
    "Apply one exactly reviewed private-note canonical migration.",
    "With --yes and a stable --id, sends the closed reviewed plan to the existing notes owner. Source digest/update-time/version/identity changes refuse and require a fresh review. Preserves actual authored version, creation/update author and timestamps, reminder, geometry values and evidence links; retains only the observed current baseline if missing. Original earlier retained history stays unchanged and earlier history not assessed is explicit. No eager account migration or background hook; retries replay the exact receipt.",
    Effect::GlobalWrite,
    &[INPUT, ID, crate::LANE_ARG],
);
fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("notes_request_invalid",e.to_string()).remedy("Review the exact plan from notes canonicalize prepare and retain its source pins unchanged.")
}
fn parse_plan(raw: &[u8]) -> Result<MigrationPlan, Failure> {
    if raw.len() > 8 << 10 {
        return Err(invalid("migration plan exceeds 8 KiB"));
    }
    let plan: MigrationPlan = serde_json::from_slice(raw).map_err(invalid)?;
    plan.validate().map_err(invalid)?;
    Ok(plan)
}
fn action(i: &Inputs, command: &str) -> Result<Action, Failure> {
    let action = match command {
        "notes.canonicalize.prepare" => Action::PrepareMigration {
            id: i.require("note")?.into(),
        },
        "notes.canonicalize.apply" => {
            let path = i.require("input")?;
            let mut reader = std::fs::File::open(path)
                .map_err(invalid)?
                .take((8 << 10) + 1);
            let mut raw = Vec::new();
            reader.read_to_end(&mut raw).map_err(invalid)?;
            Action::ApplyMigration {
                plan: parse_plan(&raw)?,
                command_id: i.require("id")?.into(),
            }
        }
        _ => return Err(invalid("unknown canonical migration command")),
    };
    action.validate().map_err(invalid)?;
    Ok(action)
}
pub fn prepare(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ds_cli_auth::personal_notes::execute(
        i.value("lane").unwrap_or("stable"),
        &action(i, "notes.canonicalize.prepare")?,
    )
}
pub fn apply(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    ds_cli_auth::personal_notes::execute(
        i.value("lane").unwrap_or("stable"),
        &action(i, "notes.canonicalize.apply")?,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn migration_plan_cannot_carry_owner_fields_or_be_unbounded() {
        let mut plan = json!({"schema":"ds.personal-note-canonicalize/v1","id":"one","expected_version":7,"source_sha256":"a".repeat(64),"source_update_time":"2026-10-13T10:15:00Z","target_sha256":"b".repeat(64),"baseline_version":7,"earlier_history":"not_assessed","changes":["history_baseline","retain_observed_current"]});
        assert!(parse_plan(&serde_json::to_vec(&plan).unwrap()).is_ok());
        plan["owner_uid"] = json!("forged");
        assert!(parse_plan(&serde_json::to_vec(&plan).unwrap()).is_err());
        assert!(parse_plan(&vec![b' '; (8 << 10) + 1]).is_err());
    }
    #[test]
    fn preparation_is_read_only_and_apply_has_explicit_write_admission() {
        assert_eq!(PREPARE.effect, Effect::ReadOnly);
        assert_eq!(APPLY.effect, Effect::GlobalWrite);
        let i =
            ds_cli_contract::args::parse(&PREPARE, &["--note".into(), "selected".into()]).unwrap();
        assert_eq!(
            action(&i, "notes.canonicalize.prepare").unwrap().payload(),
            json!({"action":"canonicalize_prepare","id":"selected"})
        );
    }
}
