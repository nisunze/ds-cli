//! Project Work entries authored while the project could not be reached,
//! held in the outbox's `project_work` queue per project and replayed in the
//! order they were authored. Creates only: a create carries its whole task,
//! client id included, so replaying it against a later revision is well
//! defined. A create's command is built against the freshest graph at its
//! first send and kept there, so a retry after a lost answer sends the same
//! command; a definitive revision refusal proves it did not commit and
//! rebuilds it once on a fresh graph. Brain stays the only write authority:
//! the page's graph is a projection its answers overwrite.
//!
//! A write the page submitted online is held on the same line, exactly as
//! sent, until its answer is known: one per project, so a second write waits
//! for the person to retry the first (`PM_RETRY_REQUIRED`). The page clears
//! it on a definitive answer; one whose answer was lost is sent again as it
//! was by the next drain.

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::project_work::{keep_graph, ProjectWork};
use super::retained::Edge;
use super::{Refusal, NETWORK_ERROR, NETWORK_OFFLINE, REQUEST_TIMEOUT};
use crate::durable_store::{Call, Request};
use crate::outbox::{Entry, Lane, Operation, Queue, Settlement, EXISTS, LEASE_LOST};
use crate::project_management::{command_wire, wire};

const QUEUE: Queue = Queue::ProjectWork;
const PREFIX: &str = "project-work:";
/// The row id of a project's submitted write.
const SUBMITTED: &str = "project-work-submitted:";
pub const INVALID: &str = "project_work_outbox_invalid";
/// A different write is held for the project, its answer unknown.
pub const RETRY_REQUIRED: &str = "PM_RETRY_REQUIRED";
/// Creates one drain sends at most.
const MAX_SENDS: usize = 200;
const LEASE_MS: u64 = 120_000;
/// Answers that decided nothing: the create stays queued.
const UNDECIDED: [&str; 7] = [
    NETWORK_OFFLINE,
    NETWORK_ERROR,
    REQUEST_TIMEOUT,
    "PM_PROJECT_EVENT_PENDING",
    "PM_NOTICE_DELIVERY_PENDING",
    "PM_RETRY_REQUIRED",
    "PM_WRITE_OUTCOME_UNKNOWN",
];

fn invalid(message: &str) -> Refusal {
    Refusal::new(INVALID, message)
}

async fn call(
    edge: &impl Edge,
    owner: Option<&str>,
    operation: Operation,
) -> Result<Value, Refusal> {
    edge.store(Call {
        request: Request::Outbox { operation },
        expect_owner: owner.map(str::to_owned),
    })
    .await
}

/// Queue a task create for `project`. `envelope` is the command a first send
/// already froze, if one did; a create already queued under its command id
/// is the same create.
pub async fn enqueue(
    edge: &impl Edge,
    project: &str,
    command_id: &str,
    task: Value,
    envelope: Option<Value>,
) -> Result<(), Refusal> {
    if task
        .get("id")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
        || command_id.is_empty()
    {
        return Err(invalid(
            "A queued Project Work create names its task and command.",
        ));
    }
    let mut payload = json!({"command_id": command_id, "task": task});
    if let Some(envelope) = envelope {
        payload["envelope"] = envelope;
    }
    let entry = Entry {
        project: Some(project.to_owned()),
        id: format!("{PREFIX}{command_id}"),
        // One line per project: each create is built on what the one before it committed.
        record: Some(project.to_owned()),
        size: None,
        meta: Some(json!({"title": payload["task"]["title"]})),
        payload,
    };
    let operation = Operation::Enqueue {
        queue: QUEUE,
        rows: vec![entry],
        now: edge.now_ms(),
    };
    match call(edge, None, operation).await {
        Err(refusal) if refusal.code == EXISTS => Ok(()),
        other => other.map(drop),
    }
}

/// The creates queued for `project`, in the order authored.
pub async fn held(edge: &impl Edge, project: &str) -> Result<Vec<Value>, Refusal> {
    let operation = Operation::List {
        queue: QUEUE,
        project: Some(project.to_owned()),
    };
    Ok(call(edge, None, operation).await?["rows"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

/// The write the page submitted for `project` whose answer is not known yet.
pub async fn submitted(edge: &impl Edge, project: &str) -> Result<Option<Value>, Refusal> {
    let id = format!("{SUBMITTED}{project}");
    Ok(held(edge, project)
        .await?
        .into_iter()
        .find(|row| row["id"] == id.as_str())
        .map(|row| row["payload"]["submitted"].clone()))
}

/// Hold `attempt` (the page's submitted write, as it sends it) before it is
/// sent. The same attempt held already is admitted again; a different one
/// refuses until the person retries the held one.
pub async fn admit_submitted(
    edge: &impl Edge,
    project: &str,
    attempt: Value,
) -> Result<(), Refusal> {
    if !attempt.is_object() {
        return Err(invalid("A submitted Project Work write is an object."));
    }
    let entry = Entry {
        project: Some(project.to_owned()),
        id: format!("{SUBMITTED}{project}"),
        record: Some(project.to_owned()),
        size: None,
        meta: None,
        payload: json!({"submitted": attempt}),
    };
    let operation = Operation::Enqueue {
        queue: QUEUE,
        rows: vec![entry],
        now: edge.now_ms(),
    };
    match call(edge, None, operation).await {
        Err(refusal) if refusal.code == EXISTS => match submitted(edge, project).await? {
            Some(held) if held == attempt => Ok(()),
            _ => Err(Refusal {
                outcome_unknown: true,
                ..Refusal::new(
                    RETRY_REQUIRED,
                    "Retry the earlier request before submitting another change.",
                )
            }),
        },
        other => other.map(drop),
    }
}

/// Forget `attempt` once its answer is known. Another attempt, or one a
/// drain is sending now, stays.
pub async fn clear_submitted(
    edge: &impl Edge,
    project: &str,
    attempt: &Value,
) -> Result<bool, Refusal> {
    if submitted(edge, project).await?.as_ref() != Some(attempt) {
        return Ok(false);
    }
    let operation = Operation::Discard {
        queue: QUEUE,
        id: format!("{SUBMITTED}{project}"),
        now: edge.now_ms(),
    };
    match call(edge, None, operation).await {
        Err(refusal) if refusal.code == crate::outbox::LEASED => Ok(false),
        other => Ok(other?["removed"].as_u64() > Some(0)),
    }
}

fn queued_tasks(rows: &[Value]) -> Vec<Value> {
    rows.iter()
        .filter(|row| row["status"] != "failed")
        .filter_map(|row| {
            row["payload"]["task"]
                .as_object()
                .map(|task| Value::Object(task.clone()))
        })
        .filter(|task| task["id"].is_string())
        .collect()
}

fn tasks_mut(graph: &mut Value) -> &mut Vec<Value> {
    if !graph["tasks"].is_array() {
        graph["tasks"] = json!([]);
    }
    graph["tasks"].as_array_mut().expect("an array")
}

/// The graph minus the queued overlay: what the server is believed to hold.
fn without_tasks(graph: &Value, tasks: &[Value]) -> Value {
    let mut base = graph.clone();
    tasks_mut(&mut base).retain(|held| !tasks.iter().any(|task| task["id"] == held["id"]));
    base
}

/// The overlay applied again, skipping what the server already returned.
fn with_tasks(graph: &Value, tasks: &[Value]) -> Value {
    let mut next = graph.clone();
    let held = tasks_mut(&mut next);
    for task in tasks {
        if !held.iter().any(|present| present["id"] == task["id"]) {
            held.push(task.clone());
        }
    }
    next
}

/// The overlay applied to `graph`: every queued create of the project,
/// whatever the server answered meanwhile.
pub async fn overlay(edge: &impl Edge, project: &str, graph: &Value) -> Result<Value, Refusal> {
    Ok(with_tasks(
        graph,
        &queued_tasks(&held(edge, project).await?),
    ))
}

/// A create command on `base`, keeping the context a first send named.
fn envelope(base: &Value, project: &str, command_id: &str, task: &Value, context: &Value) -> Value {
    let mut envelope = json!({
        "commandId": command_id,
        "projectId": project,
        "baseRevision": base["revision"].as_i64().unwrap_or(0),
        "command": {"kind": "create_task", "task": task},
    });
    if context.as_object().is_some_and(|context| {
        context
            .values()
            .any(|value| !value.is_null() && value != "")
    }) {
        envelope["context"] = context.clone();
    }
    envelope
}

/// A key and every nested key in snake case, as Brain reads a context.
fn snake(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(snake).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    let name = key.chars().fold(String::new(), |mut name, c| {
                        if c.is_ascii_uppercase() {
                            name.push('_');
                        }
                        name.push(c.to_ascii_lowercase());
                        name
                    });
                    (name, snake(value))
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// What one commit decided.
enum Commit {
    Accepted(Value),
    Stale,
    Undecided(String),
    Refused(String),
}

/// Brain's commit_command for one envelope.
fn command_request(project: &str, envelope: &Value) -> Result<Map<String, Value>, String> {
    let mut request = Map::new();
    request.insert("action".into(), json!("commit_command"));
    request.insert("ds_project".into(), json!(project));
    request.insert("command_id".into(), envelope["commandId"].clone());
    request.insert(
        "base_revision".into(),
        json!(envelope["baseRevision"].as_i64().unwrap_or(0)),
    );
    request.insert(
        "command".into(),
        command_wire::encode(&envelope["command"])?,
    );
    if envelope["context"].is_object() {
        request.insert("context".into(), snake(&envelope["context"]));
    }
    Ok(request)
}

/// Brain's commit_batch for one submitted batch.
fn batch_request(project: &str, attempt: &Value) -> Result<Map<String, Value>, String> {
    let commands = attempt["commands"]
        .as_array()
        .filter(|commands| !commands.is_empty())
        .ok_or("A saved batch carries at least one command.")?
        .iter()
        .map(command_wire::encode)
        .collect::<Result<Vec<_>, _>>()?;
    let mut request = Map::new();
    request.insert("action".into(), json!("commit_batch"));
    request.insert("ds_project".into(), json!(project));
    request.insert("command_id".into(), attempt["commandId"].clone());
    request.insert(
        "base_revision".into(),
        json!(attempt["baseRevision"].as_i64().unwrap_or(0)),
    );
    request.insert("commands".into(), Value::Array(commands));
    Ok(request)
}

/// Send one commit and judge its answer against the command it sent.
async fn commit(edge: &impl Edge, request: Result<Map<String, Value>, String>) -> Commit {
    let request = match request {
        Ok(request) => request,
        Err(message) => return Commit::Refused(message),
    };
    let base_revision = request["base_revision"].as_i64().unwrap_or(0);
    let command_id = request["command_id"].clone();
    match edge.fetch(ProjectWork { request }).await {
        Ok(answer) => {
            let result = wire_result(&answer);
            let decoded = crate::project_management::writes::decode_operation_result(&answer);
            if !decoded.applied || !decoded.violations.is_empty() {
                let first = decoded.violations.first();
                return Commit::Refused(
                    first.map_or("The project change was not applied.".into(), |issue| {
                        issue.message.clone()
                    }),
                );
            }
            if Some(decoded.command_id.as_str()) != command_id.as_str() {
                return Commit::Refused(
                    "The project change response did not match the submitted command.".into(),
                );
            }
            if decoded.committed_revision != base_revision + 1 {
                return Commit::Refused(
                    "The project change response contained an invalid revision transition.".into(),
                );
            }
            Commit::Accepted(result.clone())
        }
        Err(refusal) if refusal.code == "PM_REVISION_CONFLICT" => Commit::Stale,
        Err(refusal)
            if UNDECIDED.contains(&refusal.code.as_str())
                || refusal.outcome_unknown
                || refusal.status.is_some_and(|status| status >= 500) =>
        {
            Commit::Undecided(refusal.message)
        }
        Err(refusal) => Commit::Refused(refusal.message),
    }
}

fn wire_result(answer: &Value) -> &Value {
    answer
        .get("result")
        .filter(|result| result.is_object())
        .unwrap_or(answer)
}

fn integer(raw: &Value, keys: &[&str]) -> i64 {
    keys.iter()
        .find_map(|key| {
            raw.get(*key).and_then(|value| {
                value
                    .as_i64()
                    .or_else(|| value.as_f64().map(|f| f.trunc() as i64))
            })
        })
        .unwrap_or_default()
}

/// Merge `patches` into the rows of `field` by id, dropping `deleted`.
fn merge(graph: &mut Value, field: &str, patches: Vec<Value>, deleted: &Value) {
    let deleted: Vec<&str> = deleted
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut patches: Vec<Value> = patches
        .into_iter()
        .filter(|patch| {
            patch["id"]
                .as_str()
                .is_some_and(|id| !deleted.contains(&id))
        })
        .collect();
    let mut merged = Vec::new();
    for row in graph[field].as_array().cloned().unwrap_or_default() {
        if row["id"].as_str().is_some_and(|id| deleted.contains(&id)) {
            continue;
        }
        match patches.iter().position(|patch| patch["id"] == row["id"]) {
            Some(at) => merged.push(patches.remove(at)),
            None => merged.push(row),
        }
    }
    merged.extend(patches);
    graph[field] = Value::Array(merged);
}

fn decoded<T: Serialize>(
    raw: &Value,
    field: &str,
    decode: impl Fn(&Map<String, Value>) -> Option<T>,
) -> Vec<Value> {
    raw[field]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter_map(decode)
        .filter_map(|row| serde_json::to_value(row).ok())
        .collect()
}

/// The graph after one accepted command (`reconcileProjectGraph`): a receipt
/// older than the graph changes nothing.
fn reconcile(graph: &Value, raw: &Value) -> Value {
    let committed = integer(raw, &["committed_revision", "next_graph_revision"]);
    if committed < graph["revision"].as_i64().unwrap_or(0) {
        return graph.clone();
    }
    let mut next = graph.clone();
    next["revision"] = json!(committed);
    let schedule = integer(raw, &["schedule_revision", "next_schedule_revision"]);
    next["scheduleRevision"] = json!(schedule.max(graph["scheduleRevision"].as_i64().unwrap_or(0)));
    if raw["calendar"]
        .as_object()
        .is_some_and(|calendar| !calendar.is_empty())
    {
        if let Ok(calendar) =
            serde_json::to_value(wire::decode_project_calendar(raw.get("calendar")))
        {
            next["calendar"] = calendar;
        }
    }
    merge(
        &mut next,
        "tasks",
        decoded(raw, "task_patches", wire::decode_task),
        &raw["deleted_task_ids"],
    );
    merge(
        &mut next,
        "dependencies",
        decoded(raw, "dependency_patches", wire::decode_dependency),
        &raw["deleted_dependency_ids"],
    );
    merge(
        &mut next,
        "residualItems",
        decoded(raw, "residual_patches", wire::decode_residual_item),
        &raw["deleted_residual_ids"],
    );
    next
}

/// The project's graph read again, after a revision refusal.
async fn reload(edge: &impl Edge, project: &str) -> Option<Value> {
    let mut request = Map::new();
    request.insert("action".into(), json!("get_graph"));
    request.insert("ds_project".into(), json!(project));
    let answer = edge.fetch(ProjectWork { request }).await.ok()?;
    let graph = answer
        .get("graph")
        .filter(|graph| graph["project_id"] == project)?;
    serde_json::to_value(wire::decode_project_graph(graph, project)).ok()
}

/// What one drain did, for the page to show.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Drained {
    /// The graph to show: what the server holds, with what is still queued.
    pub graph: Value,
    /// Creates and held writes the server accepted.
    pub drained: u64,
    /// Creates still queued because nothing was decided.
    pub deferred: u64,
    /// Creates and held writes the server refused: `id`, `title` (empty for a
    /// held write) and `message`, for the person to act on. They leave the line.
    pub failed: Vec<Value>,
}

/// Replay every queued create and held write of `project` on `graph`, oldest
/// first, until the line is empty or an answer decides nothing. `lease` is a fresh host
/// nonce naming this drain.
pub async fn drain(
    edge: &impl Edge,
    project: &str,
    graph: Value,
    lease: &str,
) -> Result<Drained, Refusal> {
    let queued = queued_tasks(&held(edge, project).await?);
    let mut base = without_tasks(&graph, &queued);
    let mut drained = Drained::default();
    let mut owner: Option<String> = None;
    let mut sent_ids: Vec<Value> = Vec::new();
    for _ in 0..MAX_SENDS {
        let claim = Operation::Claim {
            queue: QUEUE,
            lane: Lane::Small,
            project: Some(project.to_owned()),
            lease: lease.into(),
            lease_ms: LEASE_MS,
            now: edge.now_ms(),
        };
        let answer = call(edge, owner.as_deref(), claim).await?;
        if owner.is_none() {
            owner = answer["owner"].as_str().map(str::to_owned);
        }
        let row = &answer["claimed"];
        if row.is_null() {
            break;
        }
        let id = row["id"].as_str().unwrap_or_default().to_owned();
        let task = row["payload"]["task"].clone();
        let command_id = row["payload"]["command_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let settle = |settlement| Operation::Settle {
            queue: QUEUE,
            id: id.clone(),
            lease: lease.into(),
            settlement,
            now: edge.now_ms(),
        };
        let submitted = &row["payload"]["submitted"];
        let decided = if submitted.is_object() {
            // A write the page submitted is sent again exactly as it was.
            let request = if submitted["kind"] == "batch" {
                batch_request(project, submitted)
            } else {
                command_request(project, &submitted["envelope"])
            };
            match commit(edge, request).await {
                Commit::Stale => Commit::Refused(
                    "The project changed before this write arrived; it was not applied.".into(),
                ),
                decided => decided,
            }
        } else if !task["id"].is_string() || command_id.is_empty() {
            // A row no drain can read would block the queue behind it.
            call(edge, owner.as_deref(), settle(Settlement::Sent)).await?;
            drained.failed.push(json!({"id": id, "title": "", "message": "A queued project entry could not be read and was discarded."}));
            continue;
        } else {
            let mut sent = match row["payload"]
                .get("envelope")
                .filter(|envelope| envelope.is_object())
            {
                Some(frozen) => frozen.clone(),
                None => envelope(&base, project, &command_id, &task, &Value::Null),
            };
            let freeze = |envelope: &Value| Operation::Revise {
                queue: QUEUE,
                id: id.clone(),
                lease: lease.into(),
                payload: Some(
                    json!({"command_id": command_id, "task": task, "envelope": envelope}),
                ),
                meta: None,
                lease_ms: None,
                now: edge.now_ms(),
            };
            call(edge, owner.as_deref(), freeze(&sent)).await?;
            let mut decided = commit(edge, command_request(project, &sent)).await;
            if matches!(decided, Commit::Stale) {
                // A definitive revision refusal proves this create did not commit.
                if let Some(fresh) = reload(edge, project).await {
                    base = without_tasks(&fresh, &queued);
                    sent = envelope(&base, project, &command_id, &task, &sent["context"]);
                    call(edge, owner.as_deref(), freeze(&sent)).await?;
                    decided = commit(edge, command_request(project, &sent)).await;
                }
            }
            decided
        };
        let (settlement, stop) = match decided {
            Commit::Accepted(raw) => {
                // A replayed receipt can precede the head: keep the newer row.
                base = if integer(&raw, &["committed_revision", "next_graph_revision"])
                    < base["revision"].as_i64().unwrap_or(0)
                {
                    let newer: Vec<Value> = graph["tasks"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|held| held["id"] == task["id"])
                        .cloned()
                        .collect();
                    with_tasks(&base, &newer)
                } else {
                    reconcile(&base, &raw)
                };
                drained.drained += 1;
                sent_ids.push(task["id"].clone());
                (Settlement::Sent, false)
            }
            Commit::Stale => (
                Settlement::Deferred {
                    error: Some("PM_REVISION_CONFLICT".into()),
                    next_attempt_at: None,
                },
                true,
            ),
            Commit::Undecided(message) => (
                Settlement::Deferred {
                    error: Some(message),
                    next_attempt_at: None,
                },
                true,
            ),
            Commit::Refused(message) => {
                // The server decided; the person enters it again from its title.
                let title = task["title"].as_str().unwrap_or_default();
                drained
                    .failed
                    .push(json!({"id": id, "title": title, "message": message}));
                sent_ids.push(task["id"].clone());
                (Settlement::Sent, false)
            }
        };
        match call(edge, owner.as_deref(), settle(settlement)).await {
            Err(refusal) if refusal.code == LEASE_LOST => {}
            other => {
                other?;
            }
        }
        if stop {
            break;
        }
    }
    let still: Vec<Value> = queued
        .into_iter()
        .filter(|task| !sent_ids.contains(&task["id"]))
        .collect();
    drained.deferred = still.len() as u64;
    drained.graph = with_tasks(&base, &still);
    if drained.drained > 0 || !drained.failed.is_empty() {
        // A store that cannot keep it never fails the drain.
        let _ = keep_graph(edge, project, &drained.graph).await;
    }
    Ok(drained)
}

/// The typed browser exports Svelte calls.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use serde_json::Value;
    use wasm_bindgen::prelude::*;

    fn value(input: JsValue) -> Result<Value, JsValue> {
        tsify::serde_wasm_bindgen::from_value(input)
            .map_err(|error| js_sys::Error::new(&error.to_string()).into())
    }

    #[wasm_bindgen(typescript_custom_section)]
    const TYPES: &'static str = r#"
/** One Project Work drain: the graph to show, creates accepted, creates still queued, and creates the server refused. */
export interface ProjectWorkOutboxDrained { graph: unknown; drained: number; deferred: number; failed: { id: string; title: string; message: string }[] }
"#;

    /// Queue a task create for the project.
    #[wasm_bindgen(js_name = projectWorkOutboxEnqueue)]
    pub async fn enqueue(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        command_id: String,
        #[wasm_bindgen(unchecked_param_type = "unknown")] task: JsValue,
        #[wasm_bindgen(unchecked_param_type = "unknown | undefined")] envelope: JsValue,
    ) -> Result<(), JsValue> {
        let envelope = (!envelope.is_undefined() && !envelope.is_null())
            .then(|| value(envelope))
            .transpose()?;
        super::enqueue(
            &BrowserEdge::new(host, store),
            &project,
            &command_id,
            value(task)?,
            envelope,
        )
        .await
        .map_err(|refusal| rejection(&refusal))
    }

    /// The creates queued for the project, in the order authored.
    #[wasm_bindgen(js_name = projectWorkOutboxHeld, unchecked_return_type = "Record<string, unknown>[]")]
    pub async fn held(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let held = super::held(&BrowserEdge::new(host, store), &project).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// `graph` with every create still queued for the project on it.
    #[wasm_bindgen(js_name = projectWorkOutboxOverlay, unchecked_return_type = "unknown")]
    pub async fn overlay(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "unknown")] graph: JsValue,
    ) -> Result<JsValue, JsValue> {
        let overlaid =
            super::overlay(&BrowserEdge::new(host, store), &project, &value(graph)?).await;
        typed(&overlaid.map_err(|refusal| rejection(&refusal))?)
    }

    /// Replay the project's queued creates on `graph`; `lease` is a fresh nonce.
    #[wasm_bindgen(js_name = projectWorkOutboxDrain, unchecked_return_type = "ProjectWorkOutboxDrained")]
    pub async fn drain(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "unknown")] graph: JsValue,
        lease: String,
    ) -> Result<JsValue, JsValue> {
        let drained = super::drain(
            &BrowserEdge::new(host, store),
            &project,
            value(graph)?,
            &lease,
        )
        .await;
        typed(&drained.map_err(|refusal| rejection(&refusal))?)
    }

    /// The write submitted for the project whose answer is not known yet.
    #[wasm_bindgen(js_name = projectWorkSubmittedHeld, unchecked_return_type = "unknown")]
    pub async fn submitted(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let held = super::submitted(&BrowserEdge::new(host, store), &project).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// Hold a write before it is sent; a different held one refuses `PM_RETRY_REQUIRED`.
    #[wasm_bindgen(js_name = projectWorkSubmittedAdmit)]
    pub async fn admit_submitted(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "unknown")] attempt: JsValue,
    ) -> Result<(), JsValue> {
        super::admit_submitted(&BrowserEdge::new(host, store), &project, value(attempt)?)
            .await
            .map_err(|refusal| rejection(&refusal))
    }

    /// Forget a held write once its answer is known.
    #[wasm_bindgen(js_name = projectWorkSubmittedClear)]
    pub async fn clear_submitted(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "unknown")] attempt: JsValue,
    ) -> Result<bool, JsValue> {
        super::clear_submitted(&BrowserEdge::new(host, store), &project, &value(attempt)?)
            .await
            .map_err(|refusal| rejection(&refusal))
    }
}

#[cfg(test)]
#[path = "project_work_outbox_tests.rs"]
mod tests;
