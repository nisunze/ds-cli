//! Item-scoped checklist bindings over the already-governed PM owner.
use crate::{LANE_ARG, PROJECT_ARG, TASK_ARG};
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_client_core::project_correspondence::Action;
use ds_client_core::project_management::{
    self,
    checklist::{self, Edit, Promotion},
};
use serde_json::{Value, json};
use std::io::Read;

const ID: Arg = Arg::value(
    "id",
    "<command-id>",
    "Stable retry id; preserve it with the complete intent and base revision after a lost reply.",
)
.required();
const REVISION: Arg = Arg::value(
    "base-revision",
    "<revision>",
    "Reviewed plan revision; preserve it for an exact retry.",
)
.required();
const ITEM: Arg = Arg::value("item", "<item-id>", "Exact stable checklist item id.").required();
const INVALID: Refusal = Refusal {
    code: "invalid_checklist",
    when: "the selected item edit or promotion is outside its closed transport shape or bounds",
    remedy: "read the task checklist, select one stable item and use its admitted edit fields",
};
const INPUT_UNREADABLE: Refusal = Refusal {
    code: "checklist_input_unreadable",
    when: "the input file cannot be read as bounded UTF-8 JSON",
    remedy: "supply a readable JSON file of at most 192 KiB",
};
const READ_UNREADABLE: Refusal = Refusal {
    code: "checklist_response_unreadable",
    when: "the canonical task read omits or contradicts the bounded checklist projection",
    remedy: "retry the task read; report a repeated server projection mismatch",
};

const fn edit_refusals() -> [Refusal; 44] {
    let mut own = [INVALID; 22];
    own[0] = INVALID;
    own[1] = INPUT_UNREADABLE;
    own[2] = crate::INVALID_NUMBER;
    own[3] = super::proposals::INVALID_COMMAND_ID;
    own[4] = super::subdivision::RESPONSE_UNREADABLE;
    let mut index = 0;
    while index < crate::geometry::PROPOSAL_REFUSALS.len() {
        own[5 + index] = crate::geometry::PROPOSAL_REFUSALS[index];
        index += 1;
    }
    crate::write_refusals::<44>(&own)
}

pub static READ: Command = Command {
    id: "pm.task.checklist.read",
    path: &["pm", "task", "checklist", "read"],
    contract: 1,
    summary: "Read one task's checklist, completion and item locations.",
    purpose: "Read the canonical checklist, including promoted-child completion and item-scoped locations/assets. The server owns status and the multi-location overlay; this read does not derive another checklist. Default output is at most 50 concise items; --details includes authored bodies, answers, links and geometry. Use the plan revision for a later edit.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TASK_ARG,
        Arg::value(
            "limit",
            "<1-200>",
            "Maximum item rows; total and more remain explicit.",
        )
        .default("50"),
        Arg::switch(
            "details",
            "Include full selected item bodies, answers, links, assets and geometry.",
        ),
        LANE_ARG,
        PROJECT_ARG,
    ],
    output: "Project, taskId, revision, kind, items, total, completed and more. --details includes the server's canonical item values and overlay.",
    examples: &[],
    refusals: &crate::correspondence_refusals::<24>(&[
        crate::TASK_NOT_FOUND,
        crate::INVALID_NUMBER,
        READ_UNREADABLE,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &["issues", "answered"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static EDIT: Command = Command {
    id: "pm.task.checklist.edit",
    path: &["pm", "task", "checklist", "edit"],
    contract: 1,
    summary: "Add, edit, answer, move or remove one task checklist item atomically.",
    purpose: "Apply one closed JSON edit to one stable checklist item through the existing project command. The input contains action add|edit|check|uncheck|move|remove and item_id; optional checklist_kind, text, body, answer, geometry, clear_geometry, links, attachment_ids and zero-based index. Omitted fields stay unchanged. attachment_ids reference assets already held by this task; no upload, ownership transfer or project-wide overwrite occurs. Optional --geometry-from resolves DS objects through the existing native geometry owner and scopes its geometry/links to this item. Preserve --id, --base-revision and the exact authored input for retries. Permissions, list state, attachment authority and promotion fences are checked by the server.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TASK_ARG,
        Arg::value(
            "input",
            "<edit.json>",
            "Closed item edit JSON file, at most 192 KiB; task_id comes only from --task.",
        )
        .required(),
        ID,
        REVISION,
        crate::geometry::GEOMETRY_FROM_ARG,
        crate::geometry::PACKAGE_ARG,
        crate::geometry::BUFFER_ARG,
        LANE_ARG,
        PROJECT_ARG,
    ],
    output: "Project, taskId, itemId, commandId, baseRevision, committedRevision and warnings; the receipt must match the exact submitted id and revision.",
    examples: &[],
    refusals: &edit_refusals(),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "issue",
        "check",
        "uncheck",
        "item geometry",
        "item attachment",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static PROMOTE: Command = Command {
    id: "pm.task.checklist.promote",
    path: &["pm", "task", "checklist", "promote"],
    contract: 1,
    summary: "Promote one checklist item with its evidence and location.",
    purpose: "Promote one stable checklist item through one revision-pinned project command. The server creates the explicit child id and links the original item to it atomically, preserving item body, geometry, links and task-held asset references. Promoted completion follows the child; the original checklist is not rewritten as a second task list. Optional recipients use the existing project membership and assignment-request authority. Exact retries reuse --id, --base-revision, --child and recipients.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TASK_ARG,
        ITEM,
        Arg::value(
            "child",
            "<task-id>",
            "Stable explicit id of the child to create; preserve it for replay.",
        )
        .required(),
        Arg {
            name: "request",
            kind: ArgKind::Repeated,
            value: "<email>",
            required: false,
            default: None,
            choices: &[],
            summary: "Ask an active project member to take the child; repeat up to 20.",
        },
        ID,
        REVISION,
        LANE_ARG,
        PROJECT_ARG,
    ],
    output: "Project, child taskId, parentTaskId, itemId, commandId, baseRevision, committedRevision and warnings.",
    examples: &[],
    refusals: &crate::write_refusals::<27>(&[
        INVALID,
        crate::INVALID_NUMBER,
        crate::INVALID_EMAIL,
        super::proposals::INVALID_COMMAND_ID,
        super::subdivision::RESPONSE_UNREADABLE,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &["issue", "subtask", "child task"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn invalid(error: impl std::fmt::Display) -> Failure {
    Failure::invalid(INVALID.code, error.to_string()).remedy(INVALID.remedy)
}
fn envelope(inputs: &Inputs) -> Result<(String, i64), Failure> {
    Ok((
        super::proposals::command_id(inputs, "checklist")?,
        crate::integer(
            inputs.require("base-revision")?,
            "base-revision",
            0,
            i64::MAX - 1,
        )?,
    ))
}
fn decode_edit(raw: &[u8]) -> Result<Edit, Failure> {
    if raw.len() > checklist::MAX_EDIT_BYTES {
        return Err(invalid("checklist edit exceeds 192 KiB"));
    }
    let edit: Edit = serde_json::from_slice(raw).map_err(invalid)?;
    edit.validate().map_err(invalid)?;
    Ok(edit)
}
fn read_edit(path: &str) -> Result<Edit, Failure> {
    let file = std::fs::File::open(path).map_err(|error| {
        Failure::invalid(INPUT_UNREADABLE.code, error.to_string()).remedy(INPUT_UNREADABLE.remedy)
    })?;
    let mut raw = Vec::new();
    file.take((checklist::MAX_EDIT_BYTES + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|error| {
            Failure::invalid(INPUT_UNREADABLE.code, error.to_string())
                .remedy(INPUT_UNREADABLE.remedy)
        })?;
    decode_edit(&raw)
}
fn read_projection(
    raw: &Value,
    project: &str,
    task: &str,
    limit: usize,
    details: bool,
) -> Result<Value, Failure> {
    if raw["revision"].as_i64().is_none_or(|revision| revision < 0) {
        return Err(Failure::unavailable(
            READ_UNREADABLE.code,
            "task read is missing the reviewed graph revision",
        )
        .remedy(READ_UNREADABLE.remedy));
    }
    let projection = checklist::projection(raw).map_err(|error| {
        Failure::unavailable(READ_UNREADABLE.code, error.to_string()).remedy(READ_UNREADABLE.remedy)
    })?;
    let items = projection["items"]
        .as_array()
        .expect("validated projection");
    let selected:Vec<Value>=items.iter().take(limit).map(|item|if details{item.clone()}else{json!({"id":item["id"],"text":item["text"],"state":item["state"],"promoted_to":item["promoted_to"],"promoted_missing":item["promoted_missing"],"has_body":item["body"].as_str().is_some_and(|body|!body.is_empty()),"has_answer":item["answer"].as_str().is_some_and(|answer|!answer.is_empty()),"has_geometry":item["geometry"].is_object(),"attachment_ids":item["attachment_ids"]})}).collect();
    let mut result = json!({"project":project,"taskId":task,"revision":raw["revision"],"kind":projection["kind"],"items":selected,"total":projection["total"],"completed":projection["completed"],"more":items.len()>limit});
    if details {
        result["overlay"] = projection["overlay"].clone()
    }
    Ok(result)
}
pub fn read(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let task = inputs.require("task")?;
    let limit = crate::integer(inputs.value("limit").unwrap_or("50"), "limit", 1, 200)? as usize;
    let report = crate::correspondence(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
        &Action::TaskRead {
            task_id: task.to_owned(),
            timeline: false,
        },
    )?;
    let project = report.project_id().to_owned();
    read_projection(
        &report.into_result(),
        &project,
        task,
        limit,
        inputs.switch("details"),
    )
}

pub fn edit(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let (id, revision) = envelope(inputs)?;
    let mut edit = read_edit(inputs.require("input")?)?;
    let task = inputs.require("task")?;
    let refs = inputs.repeated("geometry-from");
    if !refs.is_empty() {
        if !matches!(
            edit.action,
            checklist::EditAction::Add | checklist::EditAction::Edit
        ) {
            return Err(invalid("geometry-from belongs only to add or edit"));
        }
        if edit.geometry.is_some() || edit.clear_geometry {
            return Err(invalid(
                "geometry-from cannot accompany geometry or clear_geometry in the edit",
            ));
        }
        crate::geometry::check(inputs, refs)?;
        let report = crate::correspondence(
            inputs.value("lane").unwrap_or("stable"),
            inputs.require("project")?,
            &Action::TaskRead {
                task_id: task.to_owned(),
                timeline: false,
            },
        )?;
        let scope = ds_command_kernel::local_models::Scope {
            lane: report.lane().to_owned(),
            uid: report.identity().uid().to_owned(),
        };
        let project = report.project_id().to_owned();
        let raw = report.into_result();
        if raw["revision"].as_i64() != Some(revision) {
            return Err(Failure::conflict(
                crate::CONFLICT.code,
                "plan revision moved before checklist geometry proposal",
            )
            .remedy(crate::CONFLICT.remedy));
        }
        let existing = edit
            .links
            .clone()
            .unwrap_or_else(|| item_links(&raw["task"], &edit.item_id));
        let proposal = crate::geometry::propose_scoped(inputs, refs, &project, &scope, &existing)?;
        let proposal = crate::geometry::proposal_json(&proposal);
        edit.geometry = Some(proposal["geometry"].clone());
        let mut links: Vec<Value> = existing.iter().map(crate::geometry::wire_link).collect();
        links.extend(proposal["links"].as_array().into_iter().flatten().cloned());
        edit.links = Some(links);
    }
    let command = edit.command(task, &id, revision).map_err(invalid)?;
    let mut outcome = super::subdivision::send(inputs, &command, task, &id, revision)?;
    outcome["itemId"] = json!(edit.item_id);
    Ok(outcome)
}
fn item_links(task: &Value, item: &str) -> Vec<Value> {
    let items = task
        .get("checklist")
        .or_else(|| task.get("data").and_then(|data| data.get("checklist")));
    items
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|row| row["id"].as_str() == Some(item))
        .and_then(|row| row["links"].as_array())
        .cloned()
        .unwrap_or_default()
}
fn promotion(
    inputs: &Inputs,
) -> Result<(project_management::Command, Promotion, String, i64), Failure> {
    let (id, revision) = envelope(inputs)?;
    let mut recipients = Vec::new();
    for value in inputs.repeated("request") {
        let email = crate::email(value, "request")?;
        if !recipients.contains(&email) {
            recipients.push(email)
        }
    }
    let promotion = Promotion {
        item_id: inputs.require("item")?.to_owned(),
        child_id: inputs.require("child")?.to_owned(),
        request_emails: recipients,
    };
    let command = promotion
        .command(inputs.require("task")?, &id, revision)
        .map_err(invalid)?;
    Ok((command, promotion, id, revision))
}
pub fn promote(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let (command, promotion, id, revision) = promotion(inputs)?;
    let mut outcome =
        super::subdivision::send(inputs, &command, &promotion.child_id, &id, revision)?;
    outcome["parentTaskId"] = json!(inputs.require("task")?);
    outcome["itemId"] = json!(promotion.item_id);
    Ok(outcome)
}
pub fn render(data: &Value) -> String {
    if data.get("items").is_some() {
        let mut out = format!(
            "{} · {} / {} {} items complete · revision {}\n",
            data["taskId"].as_str().unwrap_or("?"),
            data["completed"],
            data["total"],
            data["kind"].as_str().unwrap_or("checklist"),
            data["revision"]
        );
        for item in data["items"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "  {} · {} · {}\n",
                item["id"].as_str().unwrap_or("?"),
                item["state"].as_str().unwrap_or("?"),
                item["text"].as_str().unwrap_or("")
            ))
        }
        if data["more"] == true {
            out.push_str("  more items: raise --limit (up to 200)\n")
        };
        out
    } else {
        super::subdivision::render(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concise_read_preserves_server_completion_and_explicit_truncation() {
        let raw = json!({"revision":7,"checklist":{"kind":"issues","items":[{"id":"i1","text":"first","state":"answered","body":"details","geometry":{"type":"Point","coordinates":[30,-2]}},{"id":"i2","text":"second","state":"open"}],"total":2,"completed":1,"overlay":{"type":"MultiPoint","coordinates":[[30,-2]]}}});
        let result = read_projection(&raw, "p", "t", 1, false).unwrap();
        assert_eq!(result["completed"], 1);
        assert_eq!(result["more"], true);
        assert_eq!(result["items"][0]["state"], "answered");
        assert!(result["items"][0].get("body").is_none());
        assert_eq!(result["items"][0]["has_geometry"], true);
        let detailed = read_projection(&raw, "p", "t", 200, true).unwrap();
        assert_eq!(detailed["items"], raw["checklist"]["items"]);
        assert_eq!(detailed["overlay"], raw["checklist"]["overlay"]);
        assert_eq!(detailed["more"], false);
    }
    #[test]
    fn edit_json_is_closed_and_empty_arrays_remain_explicit() {
        let edit = decode_edit(
            br#"{"action":"edit","item_id":"i","attachment_ids":[],"links":[],"body":""}"#,
        )
        .unwrap();
        assert_eq!(edit.attachment_ids, Some(vec![]));
        assert_eq!(edit.body, Some(String::new()));
        assert!(decode_edit(br#"{"action":"edit","item_id":"i","task_id":"another"}"#).is_err());
        assert!(decode_edit(br#"{"action":"edit","item_id":"i","checked":true}"#).is_err());
    }
    #[test]
    fn promotion_has_exact_replay_identity_and_one_existing_command() {
        let inputs = ds_cli_contract::args::parse(
            &PROMOTE,
            &[
                "--project",
                "p",
                "--task",
                "parent",
                "--item",
                "i",
                "--child",
                "child",
                "--id",
                "retry-0001",
                "--base-revision",
                "7",
                "--request",
                "FIELD@example.com",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        )
        .unwrap();
        let (command, promoted, id, revision) = promotion(&inputs).unwrap();
        assert_eq!(id, "retry-0001");
        assert_eq!(revision, 7);
        assert_eq!(promoted.request_emails, vec!["field@example.com"]);
        assert_eq!(command, promotion(&inputs).unwrap().0);
        let project_management::Command::Commit { command, .. } = command else {
            panic!()
        };
        assert_eq!(command["kind"], "promote_checklist_item");
        assert_eq!(command["promote_checklist_item"]["task_id"], "parent");
        assert_eq!(command["promote_checklist_item"]["item_id"], "i");
    }
    #[test]
    fn geometry_uses_only_selected_items_links() {
        let row = json!({"links":[{"kind":"other","target_id":"task-link"}],"data":{"checklist":[{"id":"i1","links":[{"kind":"ds_object","target_id":"first"}]},{"id":"i2","links":[{"kind":"ds_object","target_id":"second"}]}]}});
        assert_eq!(
            item_links(&row, "i2"),
            vec![json!({"kind":"ds_object","target_id":"second"})]
        );
    }
}
