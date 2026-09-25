//! Exact DS Grid model and governance-version links in Project Work.
//! The kernel owns link identity/projection; this adapter only reads and
//! submits governed PM actions under the explicitly supplied project.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::design_attachments::{
    Command as AttachmentCommand, Object as AttachmentObject,
};
use ds_client_core::design_versions::Command as VersionCommand;
use ds_client_core::project_management::Command as PMCommand;
use ds_command_kernel::project_management::model_links::ModelTarget;
use ds_command_kernel::project_management::{model_links, writes};
use serde_json::{Map, Value, json};

const MODEL: Arg = Arg::value(
    "model",
    "<model-id>",
    "Exact project model ID from dsgrid project list.",
)
.required();
const VERSION: Arg = Arg::value(
    "version",
    "<vN>",
    "Exact governance marker; omit for a model-wide reference.",
);
const TASK: Arg = Arg::value("task", "<task-id>", "Exact task or milestone ID.");
const NOTE: Arg = Arg::value("note", "<record-id>", "Exact project-note record ID.");
const CURSOR: Arg = Arg::value(
    "cursor",
    "<record-id>",
    "Next records cursor from the previous references page.",
);
const LABEL: Arg = Arg::value("label", "<text>", "Optional label shown beside the link.");
const TARGET_INVALID: Refusal = Refusal {
    code: "model_reference_invalid",
    when: "project/model identity or governance vN is invalid, or the link bound is exceeded",
    remedy: "use the exact project and opaque model ID; use an assigned vN from design version list",
};
const SOURCE_INVALID: Refusal = Refusal {
    code: "model_link_source_invalid",
    when: "neither or both --task and --note were supplied, or the selected source is absent or not a project note",
    remedy: "name exactly one existing task, milestone, or project-note record in this project",
};
const VERSION_UNAVAILABLE: Refusal = Refusal {
    code: "model_version_unavailable",
    when: "the exact governance vN could not be read in the named project",
    remedy: "inspect design version list for this exact model ID and check project access",
};
const ATTACHMENTS_UNAVAILABLE: Refusal = Refusal {
    code: "model_attachments_unavailable",
    when: "attachments for the version's pinned content revision could not be read",
    remedy: "read design version metadata and list attachments on its source_revision separately",
};
const CONTEXT_UNREADABLE: Refusal = Refusal {
    code: "model_context_unreadable",
    when: "the project-note page is missing its expected records or cursor",
    remedy: "retry the named project; report a persistent PM context contract mismatch",
};

pub static REFERENCES: Command = Command {
    id: "pm.model.references",
    path: &["pm", "model", "references"],
    contract: 1,
    summary: "Read PM tasks, milestones and notes linked to one exact model or vN.",
    purpose: "Read source-owned references to an exact project model ID and optional governance vN; a model-wide link is distinct from every vN link. Every read shows current project MV business authority from transformers/mv_data. A vN read separates its MV authority pin, source .dsgrid content revision, direct marker attachment refs and attachments indexed on the source content revision. Project notes are paged at 100; follow nextCursor while more is true. No display-name matching and no mutation.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[crate::PROJECT_ARG, crate::LANE_ARG, MODEL, VERSION, CURSOR],
    output: "Exact target, current project MV authority, version grounding facts, direct marker refs and separate source-content attachments when vN is supplied; task/milestone references, one note page, more and nextCursor.",
    examples: &[],
    refusals: &crate::read_refusals::<20>(&[
        TARGET_INVALID,
        VERSION_UNAVAILABLE,
        ATTACHMENTS_UNAVAILABLE,
        CONTEXT_UNREADABLE,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "model references",
        "version references",
        "project note model",
        "milestone model",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static LINKS: Command = Command {
    id: "pm.model.links",
    path: &["pm", "model", "links"],
    contract: 1,
    summary: "Read exact model and vN links on a task, milestone or project note.",
    purpose: "Read the source-owned DS Grid references on one exact PM item in the explicitly named project. The model ID and optional governance vN are shown separately; a display label is not used as an identity.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[crate::PROJECT_ARG, crate::LANE_ARG, TASK, NOTE],
    output: "Selected PM item and its exact model/vN links, including server-owned attachment actor and time.",
    examples: &[],
    refusals: &crate::read_refusals::<18>(&[SOURCE_INVALID, CONTEXT_UNREADABLE]),
    reference: Some("docs/reference/pm.md"),
    search: &["model links", "task model", "project note model"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static ADD: Command = Command {
    id: "pm.model.link.add",
    path: &["pm", "model", "link", "add"],
    contract: 1,
    summary: "Attach an exact DS Grid model or governance vN to PM work.",
    purpose: "Add one source-owned model reference to an exact task, milestone or project note in the named project. The server validates target existence and project scope, stamps actor/time, fences concurrent edits and records the action. An existing reference is a no-op. Linking an open vN does not freeze it.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        crate::LANE_ARG,
        MODEL,
        VERSION,
        TASK,
        NOTE,
        LABEL,
    ],
    output: "Exact target and PM source, whether changed, committed graph or note revision, and governed result.",
    examples: &[],
    refusals: &crate::write_refusals::<26>(&[
        TARGET_INVALID,
        SOURCE_INVALID,
        VERSION_UNAVAILABLE,
        CONTEXT_UNREADABLE,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "link model",
        "link model version",
        "attach model to task",
        "attach model to note",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static REMOVE: Command = Command {
    id: "pm.model.link.remove",
    path: &["pm", "model", "link", "remove"],
    contract: 1,
    summary: "Remove one exact model or vN link from PM work.",
    purpose: "Remove only the named model-wide or governance-vN reference from one task, milestone or project note. Other links, versions and attachments remain. The server fences concurrent changes and records who removed it.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        crate::LANE_ARG,
        MODEL,
        VERSION,
        TASK,
        NOTE,
    ],
    output: "Exact target and PM source, whether changed, committed graph or note revision, and governed result.",
    examples: &[],
    refusals: &crate::write_refusals::<25>(&[TARGET_INVALID, SOURCE_INVALID, CONTEXT_UNREADABLE]),
    reference: Some("docs/reference/pm.md"),
    search: &["unlink model", "remove model version reference"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn target(i: &Inputs) -> Result<ModelTarget, Failure> {
    let target = ModelTarget {
        project_id: i.require("project")?.to_owned(),
        model_id: i.require("model")?.to_owned(),
        version_id: i.value("version").map(str::to_owned),
    };
    target
        .validate()
        .map_err(|why| Failure::invalid(TARGET_INVALID.code, why).remedy(TARGET_INVALID.remedy))?;
    Ok(target)
}
fn source(i: &Inputs) -> Result<(&'static str, String), Failure> {
    match (i.value("task"), i.value("note")) {
        (Some(id), None) if !id.is_empty() => Ok(("task", id.to_owned())),
        (None, Some(id)) if !id.is_empty() => Ok(("note", id.to_owned())),
        _ => Err(
            Failure::invalid(SOURCE_INVALID.code, "name exactly one --task or --note")
                .remedy(SOURCE_INVALID.remedy),
        ),
    }
}
fn version(i: &Inputs, target: &ModelTarget) -> Result<Option<Value>, Failure> {
    let Some(id) = &target.version_id else {
        return Ok(None);
    };
    let data = ds_cli_auth::design_versions_for_project(
        i.require("lane")?,
        &target.project_id,
        &VersionCommand::Object {
            kind: "mv_model".into(),
            command: Box::new(VersionCommand::Read {
                transformer: target.model_id.clone(),
                version: id.clone(),
            }),
        },
    )
    .map_err(|error| {
        Failure::failed(VERSION_UNAVAILABLE.code, error.to_string())
            .remedy(VERSION_UNAVAILABLE.remedy)
    })?;
    if data["project"] != target.project_id
        || data["version"]["object"]["id"] != target.model_id
        || data["version"]["version_id"] != *id
    {
        return Err(Failure::failed(
            VERSION_UNAVAILABLE.code,
            "version authority returned a different target",
        )
        .remedy(VERSION_UNAVAILABLE.remedy));
    }
    Ok(Some(data["version"].clone()))
}
fn mv_authority(i: &Inputs, target: &ModelTarget) -> Result<Value, Failure> {
    let data = ds_cli_auth::design_versions_for_project(
        i.require("lane")?,
        &target.project_id,
        &VersionCommand::Object {
            kind: "mv_model".into(),
            command: Box::new(VersionCommand::Status {
                transformer: target.model_id.clone(),
            }),
        },
    );
    mv_authority_readback(data, target)
}
fn mv_authority_readback(
    data: Result<Value, Failure>,
    target: &ModelTarget,
) -> Result<Value, Failure> {
    let data = match data {
        Ok(value) => value,
        Err(error) if error.code() == "transformer_not_found" => {
            // A tombstoned catalog head cannot answer get_head, but retained
            // governance vN and PM links remain readable. Unknown is not false.
            return Ok(
                json!({"status":"unavailable","present":null,"revision":null,
                "source":"transformers/mv_data","reason":"model_head_unavailable"}),
            );
        }
        Err(error) => {
            return Err(Failure::failed(VERSION_UNAVAILABLE.code, error.to_string())
                .remedy("read design version status for this exact model and project"));
        }
    };
    let head = &data["head"];
    if data["project"] != target.project_id
        || data["object"]["kind"] != "mv_model"
        || data["object"]["id"] != target.model_id
        || head["project_id"] != target.project_id
        || head["object"]["kind"] != "mv_model"
        || head["object"]["id"] != target.model_id
        || !head["mv_authority_present"].is_boolean()
        || (head["mv_authority_present"] == true
            && !head["mv_authority_revision"]
                .as_str()
                .is_some_and(|s| !s.is_empty()))
    {
        return Err(Failure::failed(
            VERSION_UNAVAILABLE.code,
            "MV authority status returned another project or model",
        )
        .remedy(VERSION_UNAVAILABLE.remedy));
    }
    Ok(json!({
        "status":"available",
        "present":head["mv_authority_present"],
        "revision":head["mv_authority_revision"],
        "source":"transformers/mv_data",
    }))
}
fn context(i: &Inputs, command: PMCommand) -> Result<Value, Failure> {
    let report = ds_cli_auth::project_management_for_project(
        i.require("lane")?,
        i.require("project")?,
        &command,
    )?;
    Ok(report.into_result())
}
fn note(i: &Inputs, id: &str) -> Result<Value, Failure> {
    let page = context(
        i,
        PMCommand::ProjectNoteContext {
            note_id: id.to_owned(),
        },
    )?;
    if page["project_id"] != i.require("project")? {
        return Err(Failure::failed(
            CONTEXT_UNREADABLE.code,
            "project-note response changed project",
        )
        .remedy(CONTEXT_UNREADABLE.remedy));
    }
    page["records"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| {
            row["id"] == id
                && row["is_deleted"] != true
                && row["data"]["category"] == "project_note"
        })
        .cloned()
        .ok_or_else(|| {
            Failure::invalid(
                SOURCE_INVALID.code,
                "no readable project note with that exact record ID was found",
            )
            .remedy(SOURCE_INVALID.remedy)
        })
}

pub fn references(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let target = target(i)?;
    let current_mv_authority = mv_authority(i, &target)?;
    let version = version(i, &target)?;
    let marker_attachment_refs = version.as_ref().map(|v| v["attachment_refs"].clone());
    let source_revision_attachments = if let Some(row) = &version {
        if let Some(revision) = row["source_revision"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            Some(
                ds_cli_auth::design_attachments_for_project(
                    i.require("lane")?,
                    &target.project_id,
                    &AttachmentCommand::List {
                        object: AttachmentObject {
                            kind: "mv_model".into(),
                            id: target.model_id.clone(),
                            version: Some(revision.into()),
                        },
                        archived: true,
                    },
                )
                .map_err(|error| {
                    Failure::failed(ATTACHMENTS_UNAVAILABLE.code, error.to_string())
                        .remedy(ATTACHMENTS_UNAVAILABLE.remedy)
                })?,
            )
        } else {
            None
        }
    } else {
        None
    };
    let graph = crate::graph(i.require("lane")?, &target.project_id)?;
    let page = match i.value("cursor") {
        Some(cursor) => context(
            i,
            PMCommand::RecordContextPage {
                cursor: cursor.into(),
                limit: 100,
            },
        )?,
        None => context(i, PMCommand::Context { limit: Some(100) })?,
    };
    if page["project_id"] != target.project_id || !page["records"].is_array() {
        return Err(Failure::failed(
            CONTEXT_UNREADABLE.code,
            "PM records page changed project or shape",
        )
        .remedy(CONTEXT_UNREADABLE.remedy));
    }
    let records = page["records"].as_array().cloned().unwrap_or_default();
    let tasks = model_links::task_references(&graph.raw, &target);
    let notes = model_links::note_references(&records, &target);
    let next = page["next_cursors"]["records"].as_str().map(str::to_owned);
    let more = page["truncated_by_collection"]["records"] == true;
    if more && next.is_none() {
        return Err(Failure::failed(
            CONTEXT_UNREADABLE.code,
            "PM records page was truncated without a cursor",
        )
        .remedy(CONTEXT_UNREADABLE.remedy));
    }
    let source_content_revision = version
        .as_ref()
        .and_then(|v| v["source_revision"].as_str())
        .map(str::to_owned);
    Ok(json!({
        "project": target.project_id, "modelId": target.model_id, "versionId": target.version_id,
        "governanceVersion": version, "currentMvAuthority": current_mv_authority,
        "markerAttachmentRefs": marker_attachment_refs,
        "sourceContentRevision": source_content_revision,
        "sourceRevisionAttachments": source_revision_attachments, "tasksAndMilestones": tasks, "projectNotes": notes,
        "more": more, "nextCursor": next, "graphRevision": graph.graph.revision,
    }))
}
pub fn links(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let (kind, id) = source(i)?;
    let project = i.require("project")?;
    let raw = if kind == "task" {
        let graph = crate::graph(i.require("lane")?, project)?;
        graph.raw_task(&id).cloned().ok_or_else(|| {
            Failure::invalid(SOURCE_INVALID.code, "task or milestone was not found")
                .remedy(SOURCE_INVALID.remedy)
        })?
    } else {
        note(i, &id)?
    };
    let raw_links = if kind == "task" {
        &raw["links"]
    } else {
        &raw["data"]["links"]
    };
    let selected = model_links::source_model_links(
        &raw_links.as_array().cloned().unwrap_or_default(),
        project,
    );
    Ok(json!({"project":project,"sourceKind":kind,"sourceId":id,"links":selected}))
}
fn change(i: &Inputs, add: bool) -> Result<Value, Failure> {
    let target = target(i)?;
    let (kind, id) = source(i)?;
    if add {
        version(i, &target)?;
    }
    if kind == "task" {
        let graph = crate::graph(i.require("lane")?, &target.project_id)?;
        let task = graph.raw_task(&id).ok_or_else(|| {
            Failure::invalid(SOURCE_INVALID.code, "task or milestone was not found")
                .remedy(SOURCE_INVALID.remedy)
        })?;
        let current = task["links"].as_array().cloned().unwrap_or_default();
        let changed =
            model_links::change(&current, &target, add, i.value("label")).map_err(|why| {
                Failure::invalid(TARGET_INVALID.code, why).remedy(TARGET_INVALID.remedy)
            })?;
        if !changed.changed {
            return Ok(
                json!({"project":target.project_id,"sourceKind":"task","sourceId":id,
                "modelId":target.model_id,"versionId":target.version_id,"changed":false,
                "graphRevision":graph.graph.revision}),
            );
        }
        let update = writes::UpdateTask {
            task: id.clone(),
            links: Some(changed.links),
            ..writes::UpdateTask::default()
        };
        let (batch, _) = writes::update_task(&graph.graph, &update).map_err(crate::refused)?;
        let result = crate::commit_batch(i.require("lane")?, &target.project_id, &batch)?;
        let outcome = writes::write_outcome(&target.project_id, &id, &result, Map::new());
        Ok(
            json!({"project":target.project_id,"sourceKind":"task","sourceId":id,
            "modelId":target.model_id,"versionId":target.version_id,"changed":true,
            "result":outcome}),
        )
    } else {
        let row = note(i, &id)?;
        let current = row["data"]["links"].as_array().cloned().unwrap_or_default();
        let changed =
            model_links::change(&current, &target, add, i.value("label")).map_err(|why| {
                Failure::invalid(TARGET_INVALID.code, why).remedy(TARGET_INVALID.remedy)
            })?;
        if !changed.changed {
            return Ok(
                json!({"project":target.project_id,"sourceKind":"project_note","sourceId":id,
                "modelId":target.model_id,"versionId":target.version_id,"changed":false,
                "noteVersion":row["data"]["version"]}),
            );
        }
        let expected = row["data"]["version"].as_i64().ok_or_else(|| {
            Failure::failed(CONTEXT_UNREADABLE.code, "project note has no version fence")
                .remedy(CONTEXT_UNREADABLE.remedy)
        })?;
        let saved = context(
            i,
            PMCommand::UpdateProjectNoteLinks {
                note_id: id.clone(),
                expected_version: expected,
                links: changed.links,
            },
        )?;
        Ok(
            json!({"project":target.project_id,"sourceKind":"project_note","sourceId":id,
            "modelId":target.model_id,"versionId":target.version_id,"changed":true,
            "noteVersion":saved["item"]["data"]["version"],"result":saved}),
        )
    }
}
pub fn add(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    change(i, true)
}
pub fn remove(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    change(i, false)
}
pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

#[cfg(test)]
mod model_authority_tests {
    use super::*;
    #[test]
    fn tombstoned_model_head_does_not_hide_historical_pm_references() {
        let target = ModelTarget {
            project_id: "project-a".into(),
            model_id: "model-7".into(),
            version_id: Some("v3".into()),
        };
        let unavailable = mv_authority_readback(
            Err(Failure::invalid(
                "transformer_not_found",
                "retained marker has no active catalog head",
            )),
            &target,
        )
        .unwrap();
        assert_eq!(unavailable["status"], "unavailable");
        assert!(unavailable["present"].is_null());
        assert!(unavailable["revision"].is_null());
        let foreign = json!({"project":"project-b","object":{"kind":"mv_model","id":"model-7"},
            "head":{"project_id":"project-b","object":{"kind":"mv_model","id":"model-7"},
                "mv_authority_present":true,"mv_authority_revision":"2026-09-25T10:00:00Z"}});
        assert!(mv_authority_readback(Ok(foreign), &target).is_err());
        let current = json!({"project":"project-a","object":{"kind":"mv_model","id":"model-7"},
            "head":{"project_id":"project-a","object":{"kind":"mv_model","id":"model-7"},
                "mv_authority_present":true,"mv_authority_revision":"2026-09-25T10:00:00Z",
                "source_revision":"artifact-9"}});
        let result = mv_authority_readback(Ok(current), &target).unwrap();
        assert_eq!(result["revision"], "2026-09-25T10:00:00Z");
        assert!(result.get("source_revision").is_none());
    }
}
