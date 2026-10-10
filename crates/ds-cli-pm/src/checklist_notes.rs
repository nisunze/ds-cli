//! Selected personal/project note checklist controls. The owner supplies state.
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Domain, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_client_core::{personal_notes, project_correspondence::Action};
use serde_json::{Value, json};
use std::io::Read;

const NOTE: Arg = Arg::value(
    "note",
    "<note-id>",
    "One selected note id; no catalogue is read.",
)
.required();
const INPUT: Arg = Arg::value(
    "input",
    "<json-file>",
    "One closed item edit JSON, at most 192 KiB.",
)
.required();
const ID: Arg = Arg::value(
    "id",
    "<command-id>",
    "Stable 8-128 character command id; retain exact intent on retry.",
)
.required();
const VERSION: Arg = Arg::value(
    "version",
    "<version>",
    "Positive note version reviewed; retain it on exact replay.",
)
.required();
const LIMIT: Arg = Arg::value(
    "limit",
    "<count>",
    "Items to return (1-200); total and more remain explicit.",
)
.default("50");
const DETAILS: Arg = Arg::switch(
    "details",
    "Include item body, answer, geometry, links and canonical overlay.",
);
pub const INVALID: Refusal = Refusal {
    code: "note_checklist_invalid",
    when: "the note edit JSON, id or reviewed version is outside the owner contract",
    remedy: "use one stable note, an explicit command id and version, and the closed item edit fields",
};
pub const UNREADABLE: Refusal = Refusal {
    code: "note_checklist_unreadable",
    when: "the selected file or canonical checklist receipt cannot be read",
    remedy: "use a UTF-8 edit file within 192 KiB; update client and owner together if a receipt remains invalid",
};
const PROJECT_REFUSALS: &[Refusal] = &crate::correspondence_refusals::<25>(&[
    INVALID,
    UNREADABLE,
    crate::INVALID_NUMBER,
    crate::task::proposals::INVALID_COMMAND_ID,
]);
const PRIVATE_REFUSALS: [Refusal; 27] = {
    let mut out = [INVALID; 27];
    let base = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals;
    let own = ds_cli_auth::personal_notes::OWN_REFUSALS;
    assert!(base.len() + own.len() + 5 == 27);
    let mut i = 0;
    while i < base.len() {
        out[i] = base[i];
        i += 1;
    }
    let mut j = 0;
    while j < own.len() {
        out[i + j] = own[j];
        j += 1;
    }
    out[i + j] = INVALID;
    out[i + j + 1] = UNREADABLE;
    out[i + j + 2] = crate::INVALID_NUMBER;
    out[i + j + 3] = crate::task::proposals::INVALID_COMMAND_ID;
    out[i + j + 4] = crate::CONFIRMATION_REQUIRED;
    out
};
const fn descriptor(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    purpose: &'static str,
    effect: Effect,
    authority: Authority,
    args: &'static [Arg],
    refusals: &'static [Refusal],
) -> Command {
    Command {
        id,
        path,
        summary,
        purpose,
        effect,
        authority,
        args,
        refusals,
        contract: 1,
        chapter: Chapter::Project,
        execution: Execution::Sync,
        output: "Selected note id and version, canonical checklist state or exact item edit receipt; no unrelated collections.",
        examples: &[],
        reference: Some("docs/reference/pm.md"),
        search: &["note", "checklist", "issues", "item", "answer"],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    }
}
pub static PRIVATE_READ: Command = descriptor(
    "notes.checklist.read",
    &["notes", "checklist", "read"],
    "Read one account-private note checklist.",
    "Reads only the selected note through the account-private owner. No project, attachment, graph or thread catalogue is acquired. Canonical states and geometry remain the owner's projection.",
    Effect::ReadOnly,
    Authority::HeadlessUser,
    &[NOTE, LIMIT, DETAILS, crate::LANE_ARG],
    &PRIVATE_REFUSALS,
);
pub static PRIVATE_EDIT: Command = descriptor(
    "notes.checklist.edit",
    &["notes", "checklist", "edit"],
    "Edit one account-private note checklist item.",
    "Sends one selected add/edit/check/uncheck/move/remove intent against the reviewed note version. Exact retries reuse id, version and payload. Existing note body, other items and evidence remain unchanged. Project model pins cannot be adopted by private notes.",
    Effect::GlobalWrite,
    Authority::HeadlessUser,
    &[NOTE, INPUT, ID, VERSION, crate::LANE_ARG],
    &PRIVATE_REFUSALS,
);
pub static PROJECT_READ: Command = descriptor(
    "pm.note.checklist.read",
    &["pm", "note", "checklist", "read"],
    "Read one project note checklist.",
    "Reads only the selected project note and its owner-computed checklist projection. Does not acquire a thread, graph or assets; project membership is revalidated by the owner.",
    Effect::ReadOnly,
    Authority::HeadlessProject,
    &[NOTE, LIMIT, DETAILS, crate::LANE_ARG, crate::PROJECT_ARG],
    PROJECT_REFUSALS,
);
pub static PROJECT_EDIT: Command = descriptor(
    "pm.note.checklist.edit",
    &["pm", "note", "checklist", "edit"],
    "Edit one project note checklist item.",
    "Sends one closed item edit through the existing Project Work owner with an explicit note version and command id. No read precedes the write. Geometry and links stay item-scoped; referenced assets must already be note-held. The server owns authority and exact replay.",
    Effect::GlobalWrite,
    Authority::HeadlessProject,
    &[
        NOTE,
        INPUT,
        ID,
        VERSION,
        crate::LANE_ARG,
        crate::PROJECT_ARG,
    ],
    PROJECT_REFUSALS,
);
pub static DOMAIN: Domain = Domain {
    id: "notes",
    summary: "Private notes.",
    commands: &[&PRIVATE_READ, &PRIVATE_EDIT],
};

fn invalid(error: impl std::fmt::Display) -> Failure {
    Failure::invalid(INVALID.code, error.to_string()).remedy(INVALID.remedy)
}
pub(crate) fn bounded_json<T: serde::de::DeserializeOwned>(
    path: &str,
    max: usize,
) -> Result<T, Failure> {
    let mut raw = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| Failure::invalid(UNREADABLE.code, e.to_string()).remedy(UNREADABLE.remedy))?
        .take((max + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|e| Failure::invalid(UNREADABLE.code, e.to_string()).remedy(UNREADABLE.remedy))?;
    if raw.len() > max {
        return Err(invalid("selected input exceeds its byte bound"));
    }
    serde_json::from_slice(&raw).map_err(invalid)
}
fn project_action(inputs: &Inputs, edit: bool) -> Result<Action, Failure> {
    let id = inputs.require("note")?.to_owned();
    let action = if edit {
        Action::NoteChecklistEdit {
            id,
            expected_version: crate::integer(
                inputs.require("version")?,
                "version",
                1,
                i64::MAX - 1,
            )?,
            command_id: crate::task::proposals::command_id(inputs, "note-item")?,
            edit: bounded_json(
                inputs.require("input")?,
                ds_client_core::project_management::checklist::MAX_EDIT_BYTES,
            )?,
        }
    } else {
        Action::NoteChecklistRead { id }
    };
    action.validate().map_err(invalid)?;
    Ok(action)
}
fn private_action(inputs: &Inputs, edit: bool) -> Result<personal_notes::Action, Failure> {
    Ok(match project_action(inputs, edit)? {
        Action::NoteChecklistRead { id } => personal_notes::Action::ChecklistRead { id },
        Action::NoteChecklistEdit {
            id,
            expected_version,
            command_id,
            edit,
        } => personal_notes::Action::ChecklistEdit {
            id,
            expected_version,
            command_id,
            edit,
        },
        _ => unreachable!(),
    })
}
fn projection(raw: &Value, limit: usize, details: bool) -> Result<Value, Failure> {
    let view = ds_client_core::project_management::checklist::projection(raw).map_err(|e| {
        Failure::unavailable(UNREADABLE.code, e.to_string()).remedy(UNREADABLE.remedy)
    })?;
    let items = view["items"].as_array().expect("canonical projection");
    let selected:Vec<_>=items.iter().take(limit).map(|item|if details{item.clone()}else{json!({"id":item["id"],"text":item["text"],"state":item["state"],"has_body":item["body"].as_str().is_some_and(|s|!s.is_empty()),"has_answer":item["answer"].as_str().is_some_and(|s|!s.is_empty()),"has_geometry":item["geometry"].is_object(),"attachment_ids":item["attachment_ids"]})}).collect();
    let mut result = json!({"noteId":raw["id"],"version":raw["version"],"kind":view["kind"],"total":view["total"],"completed":view["completed"],"items":selected,"more":items.len()>limit});
    if details {
        result["overlay"] = view["overlay"].clone();
    }
    Ok(result)
}
fn read(inputs: &Inputs, private: bool) -> Result<Value, Failure> {
    let limit = crate::integer(inputs.value("limit").unwrap_or("50"), "limit", 1, 200)? as usize;
    let lane = inputs.value("lane").unwrap_or("stable");
    let raw = if private {
        ds_cli_auth::personal_notes::execute(lane, &private_action(inputs, false)?)?
    } else {
        crate::correspondence(
            lane,
            inputs.require("project")?,
            &project_action(inputs, false)?,
        )?
        .into_result()
    };
    let mut result = projection(&raw, limit, inputs.switch("details"))?;
    if !private {
        result["project"] = json!(inputs.require("project")?);
    }
    Ok(result)
}
fn edit(inputs: &Inputs, private: bool) -> Result<Value, Failure> {
    let lane = inputs.value("lane").unwrap_or("stable");
    let raw = if private {
        ds_cli_auth::personal_notes::execute(lane, &private_action(inputs, true)?)?
    } else {
        crate::correspondence(
            lane,
            inputs.require("project")?,
            &project_action(inputs, true)?,
        )?
        .into_result()
    };
    let note = if private { &raw["note"] } else { &raw["item"] };
    let mut result =
        json!({"noteId":note["id"],"version":note["version"],"commandId":inputs.require("id")?});
    if !private {
        result["project"] = json!(inputs.require("project")?);
    }
    Ok(result)
}
pub fn private_read(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    read(i, true)
}
pub fn private_edit(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    edit(i, true)
}
pub fn project_read(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    read(i, false)
}
pub fn project_edit(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    edit(i, false)
}
pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn note_read_uses_canonical_states_and_explicit_bound() {
        let raw = json!({"id":"n","version":5,"checklist":{"kind":"issues","items":[{"id":"i","text":"issue","state":"answered","body":"private detail"},{"id":"j","state":"open"}],"total":2,"completed":1,"overlay":{"type":"FeatureCollection","features":[]}}});
        let result = projection(&raw, 1, false).unwrap();
        assert_eq!(result["items"][0]["state"], "answered");
        assert!(result["items"][0].get("body").is_none());
        assert_eq!(result["more"], true);
        assert_eq!(result["total"], 2);
        assert_eq!(
            projection(&raw, 200, true).unwrap()["overlay"],
            raw["checklist"]["overlay"]
        );
    }
}
