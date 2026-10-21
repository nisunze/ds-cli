//! Bounded WASM entry to the native write planners and browser command codec.
use super::{browser, command_wire, projections, writes, SCHEMA};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    action: String,
    #[serde(default)]
    ds_project: String,
    #[serde(default = "browser::wire_format")]
    graph_format: String,
    #[serde(default)]
    graph: Value,
    #[serde(default)]
    input: Value,
    #[serde(default)]
    command: Value,
    #[serde(default)]
    now: i64,
}

pub(super) fn evaluate(bytes: &[u8]) -> Result<String, String> {
    let request: Request = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    debug_assert_eq!(request.schema, SCHEMA);
    if request.action == "command.encode" {
        return serde_json::to_string(&command_wire::encode(&request.command)?)
            .map_err(|e| e.to_string());
    }
    if request.action == "task.owner-write" {
        return serde_json::to_string(&owner_write(request.input)?).map_err(|e| e.to_string());
    }
    let graph = browser::graph(&request.graph, &request.ds_project, &request.graph_format)?;
    if !request.ds_project.is_empty() && graph.project_id != request.ds_project {
        return Err("Project command scope does not match the captured graph.".into());
    }
    let result = match request.action.as_str() {
        "task.create" => {
            let mut input: writes::CreateTask =
                serde_json::from_value(request.input.clone()).map_err(|e| e.to_string())?;
            // Native planners take wire links; the draft preserves the browser
            // links verbatim, including catalog revision, digest and attribution.
            input.links = input.links.iter().map(browser::snake_fields).collect();
            let prepared = writes::create_task(&graph, &input).map_err(|e| e.message())?;
            let payload = &prepared.command["create_task"];
            let start = payload["start"].as_str().unwrap_or_default();
            let finish = payload["finish"].as_str().unwrap_or_default();
            let parent = payload["parent_id"].as_str().filter(|v| !v.is_empty());
            let siblings = graph
                .tasks
                .iter()
                .filter(|task| task.parent_task_id.as_deref() == parent)
                .count();
            let duration = if input.kind == writes::CreateKind::Milestone
                || start.is_empty()
                || finish.is_empty()
            {
                0
            } else {
                (projections::dayspan(start, finish) + 1) * 480
            };
            let mut task = json!({
                "id": payload["id"], "parentTaskId": parent, "sortKey": format!("{:08}", siblings + 1),
                "title": payload["title"], "description": payload["description"],
                "links": request.input.get("links").cloned().unwrap_or(json!([])),
                "type": payload["type"], "placement": payload["placement"], "scheduleState": payload["schedule_state"],
                "deliveryState": "not_started", "reviewState": "not_required", "closeoutState": "open", "progress": 0,
                "startDate": start, "finishDate": finish, "commitmentStartDate": start, "commitmentFinishDate": finish,
                "durationMinutes": duration, "schedulingMode": "auto", "collaboratorEmails": [], "assigneeEmails": [],
                "priority": "normal", "openBlockerCount": 0, "descendantBlockerCount": 0,
                "unscheduledDescendantCount": 0, "version": 0, "updatedAt": request.now,
            });
            for key in ["geometry", "discipline"] {
                if let Some(value) = payload.get(key) {
                    task[key] = value.clone();
                }
            }
            json!({"prepared": prepared, "draftCommand": {"kind": "create_task", "task": task}})
        }
        "task.update" => {
            let mut input: writes::UpdateTask =
                serde_json::from_value(request.input).map_err(|e| e.to_string())?;
            input.links = input
                .links
                .map(|links| links.iter().map(browser::snake_fields).collect());
            let (prepared, kinds) = writes::update_task(&graph, &input).map_err(|e| e.message())?;
            json!({"prepared": prepared, "kinds": kinds})
        }
        "task.assign" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                task: String,
                mode: String,
                #[serde(default)]
                emails: Vec<String>,
                #[serde(default)]
                owner: String,
            }
            let input: Input = serde_json::from_value(request.input).map_err(|e| e.to_string())?;
            let assignment = match input.mode.as_str() {
                "request" => writes::Assignment::Request(input.emails),
                "owner" => writes::Assignment::Owner(input.owner),
                other => return Err(format!("Unsupported assignment mode `{other}`.")),
            };
            let (prepared, mode) =
                writes::assign_task(&graph, &input.task, &assignment).map_err(|e| e.message())?;
            json!({"prepared": prepared, "mode": mode})
        }
        "task.respond" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                task: String,
                response: String,
            }
            let input: Input = serde_json::from_value(request.input).map_err(|e| e.to_string())?;
            let response = writes::Response::parse(&input.response)
                .ok_or("Response must be accept or decline.")?;
            json!({"prepared": writes::respond_task(&graph, &input.task, response).map_err(|e| e.message())?})
        }
        "task.sort-key" => {
            let parent = request.input.get("parentTaskId").and_then(Value::as_str);
            let siblings = graph
                .tasks
                .iter()
                .filter(|task| task.parent_task_id.as_deref() == parent)
                .count();
            json!(format!("{:08}", siblings + 1))
        }
        other => {
            return Err(format!(
                "Unsupported project management write action `{other}`."
            ))
        }
    };
    serde_json::to_string(&result).map_err(|e| e.to_string())
}

/// Receipt identity is host-owned; the payload shape and deterministic child
/// identity are kernel-owned. ds-brain checks the actor's owner action flags.
fn owner_write(input: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Intent {
        kind: String,
        project_id: String,
        command_id: String,
        base_revision: i64,
        task_id: String,
        title: Option<String>,
        description: Option<String>,
        requested_emails: Option<Vec<String>>,
        progress: Option<f64>,
    }
    let intent: Intent = serde_json::from_value(input).map_err(|e| e.to_string())?;
    if intent.project_id.trim().is_empty()
        || intent.command_id.trim().is_empty()
        || intent.task_id.trim().is_empty()
    {
        return Err("An owner write requires exact project, command and task identities.".into());
    }
    let mut payload =
        json!({"command_id": intent.command_id, "base_revision": intent.base_revision});
    let action = match intent.kind.as_str() {
        "progress" => {
            payload["command"] = command_wire::encode(&json!({
                "kind": "set_progress", "taskId": intent.task_id,
                "progress": intent.progress.ok_or("A progress write requires progress.")?,
            }))?;
            "commit_command"
        }
        "subdivide" => {
            let child_id = format!("child-{}", intent.command_id);
            payload["commands"] = json!([
                {"kind": "create_task", "create_task": {
                    "id": child_id, "parent_id": intent.task_id,
                    "title": intent.title.ok_or("A subdivision requires title.")?,
                    "description": intent.description.unwrap_or_default(),
                    "type": "task", "placement": "wbs", "schedule_state": "unscheduled", "start": "",
                }},
                {"kind": "request_assignment", "request_assignment": {
                    "task_id": child_id, "emails": intent.requested_emails.unwrap_or_default(),
                }},
            ]);
            "commit_batch"
        }
        other => return Err(format!("Unsupported task owner write `{other}`.")),
    };
    Ok(json!({"project": intent.project_id, "action": action, "payload": payload}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_subdivision_and_progress_use_the_same_exact_receipt_identity() {
        let base = json!({"projectId": "p", "commandId": "request-1", "baseRevision": 7, "taskId": "parent"});
        let mut subdivide = base.clone();
        subdivide["kind"] = json!("subdivide");
        subdivide["title"] = json!("Inspect");
        subdivide["description"] = json!("");
        subdivide["requestedEmails"] = json!(["worker@example.com"]);
        let request = json!({"schema": SCHEMA, "action": "task.owner-write", "input": subdivide});
        let reply: Value = serde_json::from_str(
            &super::super::evaluate(&serde_json::to_vec(&request).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(reply["action"], "commit_batch");
        assert_eq!(reply["project"], "p");
        assert_eq!(reply["payload"]["command_id"], "request-1");
        assert_eq!(
            reply["payload"]["commands"][0]["create_task"]["id"],
            "child-request-1"
        );
        assert_eq!(
            reply["payload"]["commands"][1]["request_assignment"]["task_id"],
            "child-request-1"
        );
        let mut progress = base;
        progress["kind"] = json!("progress");
        progress["progress"] = json!(0.5);
        let reply = owner_write(progress).unwrap();
        assert_eq!(reply["action"], "commit_command");
        assert_eq!(
            reply["payload"]["command"]["set_progress"],
            json!({"task_id": "parent", "progress": 0.5})
        );
    }
}
