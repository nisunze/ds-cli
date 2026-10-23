//! Project Work requests: one request of Brain's closed Project Work
//! vocabulary for one project, admitted only when its action is one Brain
//! serves and every member is one Brain's request schema names. Brain owns the
//! graph, its permissions and every write; reads may be sent twice within
//! their deadline, writes never.

use serde_json::{Map, Value};

use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};

const PATH: &str = "/api/v1/pm";
/// Brain reads at most 1 MiB of a request.
const REQUEST_MAX_BYTES: usize = 1 << 20;
/// A project graph of ten thousand tasks with their residuals.
const RESPONSE_CAP: u64 = 32 * 1024 * 1024;
pub const INPUT_INVALID: &str = "project_work_input_invalid";
pub const INVALID: &str = "project_work_answer_invalid";

/// The index of projects with Project Work; it names no project.
pub const PUBLIC_INDEX: &str = "list_public_projects";

/// Every action Brain's Project Work endpoint serves for one project.
pub const ACTIONS: &[&str] = &[
    "project_note_write",
    "project_note_read",
    "project_note_query",
    "project_note_context",
    "project_note_versions",
    "project_note_backup",
    "project_note_retire",
    "project_note_restore",
    "project_note_consolidate",
    "get_graph",
    "get_assignees",
    "get_work_catalog",
    "preview_work_template",
    "apply_work_template",
    "commit_command",
    "commit_batch",
    "deletion_inventory",
    "deletion_read",
    "deletion_restore",
    "get_context",
    "create_work_episode",
    "update_work_episode",
    "create_record",
    "update_record",
    "link_message",
    "create_comment",
    "update_comment",
    "update_attachment",
    "detach_task_attachment",
    "start_attachment_upload",
    "finalize_attachment_upload",
    "restart_attachment_upload",
    "download_attachment_url",
    "download_submission_bundle_url",
    "save_intelligence_artifact",
    "update_intelligence_artifact",
    "party_list",
    "party_create",
    "party_update",
    "record_create",
    "record_reply",
    "record_update",
    "note_checklist_edit",
    "note_checklist_read",
    "copy_document",
    "mail_thread_ingest",
    "record_read",
    "record_list",
    "record_thread",
    "task_block",
    "task_unblock",
    "task_read",
    "task_request_admission",
    "task_admit",
    "task_decline_admission",
    "task_log_hours",
];

/// Actions that change nothing, so a failed link may send them again.
const READS: &[&str] = &[
    PUBLIC_INDEX,
    "project_note_read",
    "project_note_query",
    "project_note_context",
    "project_note_versions",
    "get_graph",
    "get_assignees",
    "get_work_catalog",
    "preview_work_template",
    "deletion_inventory",
    "deletion_read",
    "get_context",
    "party_list",
    "note_checklist_read",
    "record_read",
    "record_list",
    "record_thread",
    "task_read",
];

/// Every member Brain's Project Work request schema names.
const FIELDS: &[&str] = &[
    "authoring_model",
    "project_note",
    "action",
    "ds_project",
    "id",
    "data",
    "command_id",
    "base_revision",
    "command",
    "commands",
    "context",
    "expected_version",
    "task_id",
    "backup_id",
    "item_key",
    "cursor",
    "status",
    "record_id",
    "residual_id",
    "limit",
    "cursors",
    "collections",
    "expires_in_seconds",
    "template_id",
    "subject",
    "preview_digest",
    "party_id",
    "query",
    "role",
    "kind",
    "include_archived",
    "page",
    "thread_id",
    "category",
    "direction",
    "channel",
    "awaiting",
    "overdue",
    "since",
    "reason",
    "timeline",
    "parent_task_id",
    "target_index",
    "note",
    "hours",
];

fn input_invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(INPUT_INVALID, message)
}

/// One Project Work request as Brain reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectWork {
    pub request: Map<String, Value>,
}

/// The export the page calls, and the exchange it runs, for
/// [`crate::capabilities::admit_export`].
pub const EXPORT: &str = "projectWork";
pub const EXCHANGE: &str = "project_work";

/// The capability one committed command performs, by its kind.
pub fn command_capability(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "create_task" => "pm.task.create",
        "assign_task" | "request_assignment" => "pm.task.assign",
        "accept_assignment" | "decline_assignment" => "pm.task.respond",
        "update_task_fields" | "set_delivery_state" | "set_review_state"
        | "set_closeout_state" | "move_task" | "resize_task" => "pm.task.update",
        "set_progress" => "pm.task.progress",
        "delete_task" => "pm.task.delete",
        "add_dependency" | "update_dependency" | "remove_dependency" => "pm.task.dependency",
        "reparent_task" | "reorder_task" => "pm.task.arrange",
        "reopen_task" => "pm.task.reopen",
        "set_calendar" => "pm.calendar.set",
        "add_residual_item" | "update_residual_item" | "resolve_residual_item"
        | "promote_residual_item" => "pm.residual.edit",
        _ => return None,
    })
}

/// The capability one request performs, by its action and, where one action
/// serves several, the members that tell them apart. A commit is admitted
/// command by command ([`command_capability`]).
pub fn capability(request: &Map<String, Value>) -> Option<&'static str> {
    let action = request
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let member = |key: &str| request.get(key).is_some_and(|value| !value.is_null());
    let entity = request
        .get("data")
        .and_then(|data| data.get("linked_entity_type"))
        .and_then(Value::as_str);
    Some(match action {
        PUBLIC_INDEX => "pm.project.list",
        "get_graph" => "pm.plan",
        "get_context" if member("record_id") => "pm.record.read",
        "get_context" if member("task_id") || member("residual_id") => "pm.task.read",
        "get_context" => "pm.overview",
        "get_assignees" => "pm.member.list",
        "get_work_catalog" => "pm.template.list",
        "preview_work_template" => "pm.template.preview",
        "apply_work_template" => "pm.template.apply",
        "deletion_inventory" => "pm.deletion.inventory",
        "deletion_read" => "pm.deletion.read",
        "deletion_restore" => "pm.deletion.restore",
        "create_record" | "record_create" => "pm.record.create",
        "update_record" | "record_update" => "pm.record.update",
        "record_read" => "pm.record.read",
        "record_list" => "pm.record.list",
        "record_reply" => "pm.record.reply",
        "record_thread" => "pm.record.thread",
        "start_attachment_upload" | "restart_attachment_upload" | "finalize_attachment_upload"
            if entity == Some("record") =>
        {
            "pm.record.attach"
        }
        "start_attachment_upload" | "restart_attachment_upload" | "finalize_attachment_upload" => {
            "pm.task.attach"
        }
        "detach_task_attachment" => "pm.task.detach",
        "update_attachment" => "pm.attachment.update",
        "download_attachment_url" => "pm.attachment.download",
        "save_intelligence_artifact" | "update_intelligence_artifact" => "pm.intelligence.save",
        "create_comment" | "update_comment" => "pm.task.comment",
        "create_work_episode" | "update_work_episode" | "task_log_hours" => "pm.task.log-hours",
        "task_block" => "pm.task.block",
        "task_unblock" => "pm.task.unblock",
        "task_read" => "pm.task.read",
        "task_request_admission" => "pm.task.request-admission",
        "task_admit" => "pm.task.admit",
        "task_decline_admission" => "pm.task.decline",
        "party_list" => "pm.party.list",
        "party_create" => "pm.party.create",
        "party_update" => "pm.party.update",
        "copy_document" => "pm.document.copy",
        "mail_thread_ingest" => "pm.mail-thread.ingest",
        "note_checklist_edit" => "pm.note.checklist.edit",
        "note_checklist_read" => "pm.note.checklist.read",
        "project_note_write" => "pm.note.update",
        "project_note_read" => "pm.note.read",
        "project_note_query" => "pm.note.query",
        "project_note_context" => "pm.note.context",
        "project_note_versions" => "pm.note.versions",
        "project_note_backup" => "pm.note.backup",
        "project_note_retire" => "pm.note.retire",
        "project_note_restore" => "pm.note.restore",
        "project_note_consolidate" => "pm.note.consolidate",
        _ => return None,
    })
}

impl ProjectWork {
    fn action(&self) -> &str {
        self.request
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    /// Admit this request from the page: its capability's row, and for a
    /// commit every command's, must name [`EXPORT`] and be supported on this
    /// build's platform. Answers the capabilities admitted.
    pub fn admit_in_page(&self, platform: &str) -> Result<Vec<&'static str>, Refusal> {
        let action = self.action();
        let admit = |id: &'static str| {
            crate::capabilities::admit_export_on(platform, id, EXPORT, EXCHANGE)
                .map(|capability| capability.id)
                .map_err(|refused| Refusal::new(refused.code, refused.message))
        };
        let commands: Vec<&Value> = match action {
            "commit_command" => self.request.get("command").into_iter().collect(),
            "commit_batch" => self
                .request
                .get("commands")
                .and_then(Value::as_array)
                .map(|commands| commands.iter().collect())
                .unwrap_or_default(),
            _ => {
                let id = capability(&self.request).ok_or_else(|| {
                    input_invalid(format!("`{action}` is not a Project Work action the page runs."))
                })?;
                return admit(id).map(|id| vec![id]);
            }
        };
        if commands.is_empty() {
            return Err(input_invalid("A Project Work commit names its commands."));
        }
        let mut admitted = Vec::new();
        for command in commands {
            let kind = command
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let id = command_capability(kind).ok_or_else(|| {
                input_invalid(format!("`{kind}` is not a Project Work command the page runs."))
            })?;
            let id = admit(id)?;
            if !admitted.contains(&id) {
                admitted.push(id);
            }
        }
        Ok(admitted)
    }
}

impl Exchange for ProjectWork {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        let action = self.action();
        if action != PUBLIC_INDEX && !ACTIONS.contains(&action) {
            return Err(input_invalid(format!(
                "`{action}` is not a Project Work action."
            )));
        }
        if let Some(unknown) = self
            .request
            .keys()
            .find(|key| !FIELDS.contains(&key.as_str()))
        {
            return Err(input_invalid(format!(
                "A Project Work request has no member `{unknown}`."
            )));
        }
        let project = self.request.get("ds_project").and_then(Value::as_str);
        match (action == PUBLIC_INDEX, project) {
            (true, None) => {}
            (false, Some(project)) => {
                project_reference(project)?;
            }
            (true, Some(_)) => {
                return Err(input_invalid("The public index names no project."));
            }
            (false, None) => {
                return Err(input_invalid("A Project Work request names its project."))
            }
        }
        // An absent member reaches here as `null` from the page; Brain reads it as absent.
        let body = Value::Object(
            self.request
                .iter()
                .filter(|(_, value)| !value.is_null())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        if serde_json::to_vec(&body).map_or(true, |bytes| bytes.len() > REQUEST_MAX_BYTES) {
            return Err(input_invalid("A Project Work request is at most 1 MiB."));
        }
        let request = Request::post(PATH, body).response_cap(RESPONSE_CAP);
        Ok(Plan {
            request,
            retries: u32::from(READS.contains(&action)),
        })
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        if answer.get("signed_out") == Some(&Value::Bool(true))
            || answer.get("cancelled") == Some(&Value::Bool(true))
        {
            return Err(Refusal::new(
                INVALID,
                "The Project Work answer was cancelled. Sign in and retry.",
            ));
        }
        Ok(Admitted::Done(answer.clone()))
    }

    fn project(&self) -> Option<&str> {
        self.request.get("ds_project").and_then(Value::as_str)
    }
}

/// The project's Project Work graph this device holds, as the page last kept
/// it, with no transport. Another account's graph reads as absent.
#[cfg(feature = "durable-store")]
pub async fn held_graph(
    edge: &impl super::retained::Edge,
    project: &str,
) -> Result<Option<Value>, Refusal> {
    project_reference(project)?;
    let kind = crate::retained_reads::Kind::ProjectWorkGraph;
    let observed = super::retained::read::<Value>(edge, kind, Some(project), "").await?;
    Ok(observed.held.map(|held| held.value))
}

/// Keep the graph the page holds for the next offline open, under the
/// account the store observes now.
#[cfg(feature = "durable-store")]
pub async fn keep_graph(
    edge: &impl super::retained::Edge,
    project: &str,
    graph: &Value,
) -> Result<(), Refusal> {
    project_reference(project)?;
    let kind = crate::retained_reads::Kind::ProjectWorkGraph;
    let observed = super::retained::read::<Value>(edge, kind, Some(project), "").await?;
    let owner = observed.owner.as_deref();
    super::retained::write(edge, owner, kind, Some(project), "", graph, None).await
}

/// The typed browser export Svelte calls; it runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{held_graph, keep_graph, ProjectWork};
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, run, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use serde_json::{Map, Value};
    use wasm_bindgen::prelude::*;

    /// One Project Work request; answers Brain's `data` as served.
    #[wasm_bindgen(js_name = projectWork, unchecked_return_type = "Record<string, unknown>")]
    pub async fn project_work(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] request: JsValue,
    ) -> Result<JsValue, JsValue> {
        let request: Map<String, Value> = tsify::serde_wasm_bindgen::from_value(request)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        let call = ProjectWork { request };
        call.admit_in_page(crate::capabilities::PLATFORM)
            .map_err(|refusal| rejection(&refusal))?;
        typed(&run(&host, call).await?)
    }
    /// The project's graph this device holds, at once and offline.
    #[wasm_bindgen(js_name = projectWorkGraphHeld, unchecked_return_type = "unknown")]
    pub async fn project_work_graph_held(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        typed(
            &held_graph(&edge, &project)
                .await
                .map_err(|refusal| rejection(&refusal))?,
        )
    }

    /// Keep the graph the page holds for the next offline open.
    #[wasm_bindgen(js_name = projectWorkGraphKeep)]
    pub async fn project_work_graph_keep(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "unknown")] graph: JsValue,
    ) -> Result<(), JsValue> {
        let graph: Value = tsify::serde_wasm_bindgen::from_value(graph)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        let edge = BrowserEdge::new(host, store);
        keep_graph(&edge, &project, &graph)
            .await
            .map_err(|refusal| rejection(&refusal))
    }
}

#[cfg(test)]
#[path = "project_work_tests.rs"]
mod tests;
