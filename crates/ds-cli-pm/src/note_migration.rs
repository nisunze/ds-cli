//! Thin adapters for deliberate shared-note representation adoption.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Command, Effect},
};
use ds_command_kernel::project_management::project_notes::{self as owner, migration};
use serde_json::Value;
use std::io::Read;
const NOTE: Arg = Arg::value(
    "note",
    "<note-id>",
    "Exact same-project current plain note.",
)
.required();
const INPUT:Arg=Arg::value("input","<reviewed-plan.json>","Closed exact plan returned by prepare, at most 8 KiB; retain source/update-time/version pins unchanged.").required();
const ID: Arg = Arg::value(
    "id",
    "<operation-id>",
    "Stable exact-replay operation identity.",
)
.required();
pub static PREPARE: Command = super::note_memory::descriptor(
    "pm.note.canonicalize.prepare",
    &["pm", "note", "canonicalize", "prepare"],
    "Review a plain shared note for explicit canonical adoption.",
    "Reads one literal note and current immutable history guard. Returns reviewed plan plus untouched source document. No inferred decisions, project-wide scope, invented past versions or automatic application.",
    Effect::ReadOnly,
    &[NOTE, crate::PROJECT_ARG, crate::LANE_ARG],
);
pub static APPLY: Command = super::note_memory::descriptor(
    "pm.note.canonicalize.apply",
    &["pm", "note", "canonicalize", "apply"],
    "Apply an exactly reviewed shared-note representation adoption.",
    "With --yes and stable operation ID, preserves authored version, original author/timestamps, literal body, geometry and unknown stored fields. Records representation adoption separately and keeps original immutable history bytes/SHA. Source/version/update-time changes refuse; exact retries replay. Empty observation claims never imply consensus or complete context.",
    Effect::GlobalWrite,
    &[INPUT, ID, crate::PROJECT_ARG, crate::LANE_ARG],
);
fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("project_note_invalid", e.to_string()).remedy(
        "Review prepare's exact source pins; never interpret plain text as accepted claims.",
    )
}
fn parse(raw: &[u8]) -> Result<migration::Plan, Failure> {
    if raw.len() > 8 << 10 {
        return Err(invalid("migration plan exceeds 8 KiB"));
    };
    let p: migration::Plan = serde_json::from_slice(raw).map_err(invalid)?;
    p.validate().map_err(invalid)?;
    Ok(p)
}
pub fn prepare(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    super::note_memory::ask(
        i,
        owner::Command::PrepareMigration(migration::Prepare {
            id: i.require("note")?.into(),
        }),
    )
}
pub fn apply(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let mut raw = Vec::new();
    std::fs::File::open(i.require("input")?)
        .map_err(invalid)?
        .take((8 << 10) + 1)
        .read_to_end(&mut raw)
        .map_err(invalid)?;
    super::note_memory::ask(
        i,
        owner::Command::ApplyMigration(migration::Apply {
            operation_id: i.require("id")?.into(),
            plan: parse(&raw)?,
        }),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_closed_plan_required() {
        assert!(parse(&vec![b' '; (8 << 10) + 1]).is_err());
        assert!(parse(br#"{"owner_uid":"spoof"}"#).is_err());
        assert_eq!(PREPARE.effect, Effect::ReadOnly);
        assert_eq!(APPLY.effect, Effect::GlobalWrite);
    }
}
