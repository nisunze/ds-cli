//! Browser command intents enter the same Rust wire authority as native PM.
//! Geometry is opaque GeoJSON; outgoing model revision pins are kept whole.
use super::browser::snake_fields;
use serde_json::{json, Map, Value};

fn object(value: &Value) -> Result<&Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| "Project command must be an object.".into())
}
fn pick(value: &Value, keys: &[(&str, &str)]) -> Value {
    let mut result = Map::new();
    for (source, target) in keys {
        if let Some(value) = value.get(*source) {
            result.insert((*target).into(), value.clone());
        }
    }
    Value::Object(result)
}
fn days(value: &Value) -> Result<Value, String> {
    let minutes = value
        .as_i64()
        .ok_or("dependency lag must be expressed in whole 8-hour working days.")?;
    if minutes % 480 != 0 {
        return Err("dependency lag must be expressed in whole 8-hour working days.".into());
    }
    Ok(json!(minutes / 480))
}
fn task_payload(task: &Value) -> Value {
    let mut payload = pick(
        task,
        &[
            ("id", "id"),
            ("title", "title"),
            ("description", "description"),
            ("type", "type"),
            ("geometry", "geometry"),
            ("startDate", "start"),
            ("finishDate", "finish"),
            ("schedulingMode", "scheduling_mode"),
            ("priority", "priority"),
            ("collaboratorEmails", "collaborator_emails"),
        ],
    );
    payload["parent_id"] = task
        .get("parentTaskId")
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or(json!(""));
    payload["responsible_email"] = task.get("responsibleEmail").cloned().unwrap_or(json!(""));
    payload["placement"] = task.get("placement").cloned().unwrap_or(json!("wbs"));
    payload["links"] = snake_fields(task.get("links").unwrap_or(&json!([])));
    let nonempty = |name| {
        task.get(name)
            .and_then(Value::as_str)
            .is_some_and(|v| !v.is_empty())
    };
    payload["schedule_state"] = task.get("scheduleState").cloned().unwrap_or_else(|| {
        json!(match (nonempty("startDate"), nonempty("finishDate")) {
            (true, true) => "full",
            (true, false) => "start_only",
            (false, true) => "finish_only",
            _ => "unscheduled",
        })
    });
    if nonempty("discipline") {
        payload["discipline"] = task["discipline"].clone();
    }
    payload
}

/// Encode every domain ProjectCommand, including saved draft gestures. No IO:
/// the server independently authorizes and applies the revision-pinned write.
pub fn encode(command: &Value) -> Result<Value, String> {
    object(command)?;
    let kind = command
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("Project command needs kind.")?;
    let task_id = || pick(command, &[("taskId", "task_id")]);
    let mut payload = match kind {
        "create_task" => task_payload(&command["task"]),
        "delete_task" | "accept_assignment" | "decline_assignment" => task_id(),
        "update_task_fields" => {
            let mut payload = task_id();
            let fields = pick(
                &command["fields"],
                &[
                    ("title", "title"),
                    ("description", "description"),
                    ("discipline", "discipline"),
                    ("geometry", "geometry"),
                    ("type", "type"),
                    ("schedulingMode", "scheduling_mode"),
                    ("priority", "priority"),
                    ("placement", "placement"),
                ],
            );
            payload
                .as_object_mut()
                .unwrap()
                .extend(object(&fields)?.clone());
            if let Some(links) = command["fields"].get("links") {
                payload["links"] = snake_fields(links);
            }
            payload
        }
        "move_task" => pick(command, &[("taskId", "task_id"), ("startDate", "start")]),
        "resize_task" => pick(
            command,
            &[
                ("taskId", "task_id"),
                ("startDate", "start"),
                ("finishDate", "finish"),
            ],
        ),
        "reorder_task" => pick(
            command,
            &[("taskId", "task_id"), ("targetIndex", "target_index")],
        ),
        "reparent_task" => {
            let mut payload = pick(
                command,
                &[("taskId", "task_id"), ("targetIndex", "target_index")],
            );
            payload["new_parent_id"] = command
                .get("parentTaskId")
                .filter(|v| !v.is_null())
                .cloned()
                .unwrap_or(json!(""));
            payload
        }
        "add_dependency" => {
            let mut payload = pick(
                &command["dependency"],
                &[
                    ("id", "id"),
                    ("fromTaskId", "predecessor_id"),
                    ("toTaskId", "successor_id"),
                    ("type", "type"),
                ],
            );
            payload["lag_days"] = days(&command["dependency"]["lagMinutes"])?;
            payload
        }
        "update_dependency" => {
            let mut payload = pick(command, &[("dependencyId", "id"), ("type", "type")]);
            payload["lag_days"] = days(&command["lagMinutes"])?;
            payload
        }
        "remove_dependency" => pick(command, &[("dependencyId", "id")]),
        "set_progress" => pick(command, &[("taskId", "task_id"), ("progress", "progress")]),
        "set_delivery_state" | "set_review_state" | "set_closeout_state" => {
            pick(command, &[("taskId", "task_id"), ("state", "state")])
        }
        "assign_task" => {
            let mut payload = pick(
                command,
                &[
                    ("taskId", "task_id"),
                    ("collaboratorEmails", "collaborator_emails"),
                ],
            );
            payload["responsible_email"] = command
                .get("responsibleEmail")
                .cloned()
                .unwrap_or(json!(""));
            payload
        }
        "request_assignment" => pick(command, &[("taskId", "task_id"), ("emails", "emails")]),
        "set_calendar" => snake_fields(&command["calendar"]),
        "create_residual" => {
            let mut payload = pick(
                &command["residual"],
                &[
                    ("id", "id"),
                    ("taskId", "task_id"),
                    ("title", "title"),
                    ("description", "description"),
                    ("kind", "kind"),
                    ("scopeClassification", "scope_classification"),
                    ("workClassification", "work_classification"),
                    ("dueDate", "due_date"),
                    ("collaboratorEmails", "collaborator_emails"),
                    ("blocksAcceptance", "blocks_acceptance"),
                    ("blocksCloseout", "blocks_closeout"),
                    ("sourceRecordId", "source_record_id"),
                ],
            );
            payload["responsible_email"] = command["residual"]
                .get("responsibleEmail")
                .cloned()
                .unwrap_or(json!(""));
            payload
        }
        "update_residual" => {
            let mut payload = pick(
                &command["fields"],
                &[
                    ("title", "title"),
                    ("description", "description"),
                    ("kind", "kind"),
                    ("scopeClassification", "scope_classification"),
                    ("workClassification", "work_classification"),
                    ("dueDate", "due_date"),
                    ("responsibleEmail", "responsible_email"),
                    ("collaboratorEmails", "collaborator_emails"),
                    ("blocksAcceptance", "blocks_acceptance"),
                    ("blocksCloseout", "blocks_closeout"),
                ],
            );
            payload["id"] = command["residualId"].clone();
            payload
        }
        "resolve_residual" => pick(command, &[("residualId", "id"), ("status", "status")]),
        "promote_residual" => {
            let mut payload = pick(
                &command["task"],
                &[
                    ("id", "new_task_id"),
                    ("title", "title"),
                    ("type", "type"),
                    ("startDate", "start"),
                    ("finishDate", "finish"),
                ],
            );
            payload["parent_id"] = command["task"]
                .get("parentTaskId")
                .filter(|v| !v.is_null())
                .cloned()
                .unwrap_or(json!(""));
            payload["residual_id"] = command["residualId"].clone();
            payload
        }
        "reopen_task" => pick(command, &[("taskId", "task_id"), ("progress", "progress")]),
        other => return Err(format!("Unsupported project command: {other}")),
    };
    if matches!(kind, "create_task" | "promote_residual") {
        if let Some(index) = command.get("targetIndex") {
            payload["target_index"] = index.clone();
        }
    }
    let wire_kind = match kind {
        "create_residual" => "add_residual_item",
        "update_residual" => "update_residual_item",
        "resolve_residual" => "resolve_residual_item",
        "promote_residual" => "promote_residual_item",
        _ => kind,
    };
    Ok(json!({"kind": wire_kind, wire_kind: payload}))
}
