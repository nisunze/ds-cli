//! The governed writes `ds pm task create|update|assign|respond` send —
//! decided in the kernel, no longer in a browser window.
//!
//! Until 2026-09-20 the four write commands relayed through the paired desktop
//! to `ds-web/src/lib/desktop/cli-pm.ts` (`createCliWorkTask` line 564,
//! `updateCliWorkTask` 611, `assignCliWorkTask` 691, `respondCliWorkTask`
//! 713), which validated the flags against the current graph, built the
//! project command the engine accepts and committed it through
//! `POST /api/v1/pm`. Every one of those decisions — which permission the
//! write needs, whether the task exists, which vocabulary a state must come
//! from, which command kinds one update becomes, how a direct transfer keeps
//! the former holder — lived inside a running window. This module is those
//! decisions, translated as they were, so a server with no window sends the
//! same command the page would have sent.
//!
//! What this module does NOT do is send anything: it turns validated intent
//! into the wire command (`projectCommandToWire`, `api/pm.ts:225-372`), and
//! folds the engine's answer (`decodeOperationResult`, `wire.ts:280-303`)
//! into the receipt the CLI already documented. The door is
//! `ds-client-core::project_management`; ds-brain remains the authority that
//! checks membership, permission and revision on every commit.
//!
//! Pure by the crate's contract: no clock, no IO, no network, and no minted
//! identifier — a task id the caller did not supply is the CLI's to mint,
//! because only a host has entropy.

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::reads::task_route;
use super::wire::{decode_task, CanonicalProjectGraph, CanonicalProjectTask, TaskType};

/// The engine's bound on the people one assignment request may name, used
/// when the graph predates the field model (`maxAssignees ?? 20`,
/// cli-pm.ts:697).
pub const DEFAULT_MAX_ASSIGNEES: usize = 20;
/// The most changes one saved draft carries when the graph predates the field
/// model (`maxBatchCommands ?? 200`, cli-pm.ts:296).
pub const DEFAULT_MAX_BATCH_COMMANDS: usize = 200;
/// The longest free text one authored field carries (cli-pm.ts:107).
pub const MAX_TEXT_LENGTH: usize = 4_000;
/// The longest title (cli-pm.ts:566).
pub const MAX_TITLE_LENGTH: usize = 400;
/// The longest task id (cli-pm.ts:108).
pub const MAX_ID_LENGTH: usize = 200;
/// The longest discipline name (cli-pm.ts:570).
pub const MAX_DISCIPLINE_LENGTH: usize = 120;

/// Why a write was refused before anything was sent. Each variant is one
/// condition the CLI names with its own code and remedy; the message is the
/// browser's own sentence, kept so the same condition reads the same on
/// every host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The signed-in user may read this plan but not change it.
    NotPermitted(String),
    /// No task carries that id.
    TaskNotFound(String),
    /// A create with an id the plan already holds — the retry case.
    TaskExists(String),
    /// The flags describe a shape the engine has no command for.
    InvalidShape(String),
    /// A value outside the vocabulary the engine published for this project.
    InvalidValue(String),
    /// An update with nothing to change.
    NothingToChange,
    /// More people than one request may name.
    TooManyAssignees { given: usize, max: usize },
}

impl Refusal {
    pub fn message(&self) -> String {
        match self {
            Self::NotPermitted(message)
            | Self::InvalidShape(message)
            | Self::InvalidValue(message) => message.clone(),
            Self::TaskNotFound(id) => format!("No task {id} in this project's plan."),
            Self::TaskExists(id) => format!("Task {id} already exists in this project's plan."),
            Self::NothingToChange => "pm.task.update was given nothing to change.".into(),
            Self::TooManyAssignees { max, .. } => {
                format!("One assignment request names at most {max} people.")
            }
        }
    }
}

fn require_edit_schedule(graph: &CanonicalProjectGraph) -> Result<(), Refusal> {
    if graph.permissions.edit_schedule {
        Ok(())
    } else {
        Err(Refusal::NotPermitted(
            "Project schedule editor access is required to change this plan.".into(),
        ))
    }
}

fn require_answer_access(graph: &CanonicalProjectGraph) -> Result<(), Refusal> {
    if graph.permissions.edit_schedule || graph.permissions.contribute {
        Ok(())
    } else {
        Err(Refusal::NotPermitted(
            "Project contributor access is required to answer an assignment request.".into(),
        ))
    }
}

fn require_task<'a>(
    graph: &'a CanonicalProjectGraph,
    task_id: &str,
) -> Result<&'a CanonicalProjectTask, Refusal> {
    graph
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| Refusal::TaskNotFound(task_id.to_owned()))
}

/// `text(value, label, max)` (cli-pm.ts:118-123): non-empty, trimmed, bounded.
fn required_text(value: &str, label: &str, max: usize) -> Result<String, Refusal> {
    let trimmed = value.trim();
    if trimmed.is_empty() || value.chars().count() > max {
        return Err(Refusal::InvalidValue(format!(
            "{label} must be a non-empty string no longer than {max} characters."
        )));
    }
    Ok(trimmed.to_owned())
}

/// `optionalText` (cli-pm.ts:125-131): absent stays absent, present is
/// trimmed and bounded (and may be empty).
fn optional_text(value: Option<&str>, label: &str, max: usize) -> Result<Option<String>, Refusal> {
    match value {
        None => Ok(None),
        Some(raw) if raw.chars().count() > max => Err(Refusal::InvalidValue(format!(
            "{label} must be a string no longer than {max} characters."
        ))),
        Some(raw) => Ok(Some(raw.trim().to_owned())),
    }
}

/// `held(value, allowed, label)` (cli-pm.ts:630-635): a value the engine
/// published a vocabulary for must be in it. A graph that predates the field
/// model publishes none, and then nothing is checked here — the engine still
/// refuses what it does not accept.
fn held(value: Option<&str>, allowed: Option<&[String]>, label: &str) -> Result<(), Refusal> {
    match (value, allowed) {
        (Some(value), Some(allowed)) if !allowed.iter().any(|entry| entry == value) => Err(
            Refusal::InvalidValue(format!("{label} must be one of {}.", allowed.join(", "))),
        ),
        _ => Ok(()),
    }
}

/// A validated command with the revision it was authored against: what one
/// `commit_command` sends.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedCommand {
    pub project: String,
    pub base_revision: i64,
    /// The wire command, `{kind, <kind>: {…}}` in the engine's snake_case.
    pub command: Value,
}

/// Several validated commands as one saved draft: what one `commit_batch`
/// sends. One base revision, applied whole or refused whole.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreparedBatch {
    pub project: String,
    pub base_revision: i64,
    pub commands: Vec<Value>,
}

// ── create ──────────────────────────────────────────────────────────────

/// `--kind parent|child|inbox|milestone` (cli-pm.ts:567).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateKind {
    Parent,
    Child,
    Inbox,
    Milestone,
}

impl CreateKind {
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "parent" => Some(Self::Parent),
            "child" => Some(Self::Child),
            "inbox" => Some(Self::Inbox),
            "milestone" => Some(Self::Milestone),
            _ => None,
        }
    }
    pub const fn token(self) -> &'static str {
        match self {
            Self::Parent => "parent",
            Self::Child => "child",
            Self::Inbox => "inbox",
            Self::Milestone => "milestone",
        }
    }
}

/// What `ds pm task create` was given. Dates are already `YYYY-MM-DD`; the
/// CLI checks the calendar before the kernel sees them, as the adapter did.
#[derive(Debug, Clone)]
pub struct CreateTask {
    /// The id to mint — the caller's, or one the host minted for this call.
    pub id: String,
    pub title: String,
    pub kind: CreateKind,
    pub parent: Option<String>,
    pub description: Option<String>,
    pub discipline: Option<String>,
    pub start_date: Option<String>,
    pub finish_date: Option<String>,
    /// The task's one optional top-level WGS84 geometry
    /// (work-links-and-spatial-anchors.md), already shaped — by
    /// `task_geometry::propose` when it comes from DS objects. Sent as the
    /// engine's `geometry`; `None` sends none.
    pub geometry: Option<Value>,
    /// The task's outgoing `links[]`, as the engine's wire links. Empty is
    /// what the Plan sheet sends for a new item.
    pub links: Vec<Value>,
}

/// `createCliWorkTask` (cli-pm.ts:564-609) + `createTaskCommand`
/// (gantt-workspace.ts:248-296) + the `create_task` arm of
/// `projectCommandToWire` (api/pm.ts:229-250), as one function.
pub fn create_task(
    graph: &CanonicalProjectGraph,
    input: &CreateTask,
) -> Result<PreparedCommand, Refusal> {
    let title = required_text(&input.title, "title", MAX_TITLE_LENGTH)?;
    let parent = optional_text(input.parent.as_deref(), "parent", MAX_ID_LENGTH)?
        .filter(|id| !id.is_empty());
    let description = optional_text(input.description.as_deref(), "description", MAX_TEXT_LENGTH)?;
    let discipline = optional_text(
        input.discipline.as_deref(),
        "discipline",
        MAX_DISCIPLINE_LENGTH,
    )?
    .filter(|name| !name.is_empty());
    let task_id = required_text(&input.id, "id", MAX_ID_LENGTH)?;
    let start = input.start_date.as_deref().filter(|date| !date.is_empty());
    let finish = input.finish_date.as_deref().filter(|date| !date.is_empty());

    match input.kind {
        CreateKind::Child if parent.is_none() => {
            return Err(Refusal::InvalidShape(
                "A child task requires parent.".into(),
            ));
        }
        CreateKind::Parent | CreateKind::Inbox if parent.is_some() => {
            return Err(Refusal::InvalidShape(format!(
                "{} tasks cannot name a parent.",
                input.kind.token()
            )));
        }
        CreateKind::Milestone if start.is_some() && finish.is_some() => {
            return Err(Refusal::InvalidShape(
                "A milestone takes one date, not both startDate and finishDate.".into(),
            ));
        }
        _ => {}
    }

    require_edit_schedule(graph)?;
    if graph.tasks.iter().any(|task| task.id == task_id) {
        // The retry case, answered honestly: the id already landed, so this
        // is the same create arriving twice rather than a new work item.
        return Err(Refusal::TaskExists(task_id));
    }
    let parent_task_id = match input.kind {
        CreateKind::Parent | CreateKind::Inbox => None,
        CreateKind::Child | CreateKind::Milestone => parent,
    };
    if let Some(parent) = parent_task_id.as_deref() {
        require_task(graph, parent)?;
    }

    // `createTaskCommand`: a milestone collapses to its one date.
    let milestone = input.kind == CreateKind::Milestone;
    let milestone_date = start.or(finish).unwrap_or_default();
    let start_date = if milestone {
        milestone_date
    } else {
        start.unwrap_or_default()
    };
    let finish_date = if milestone {
        milestone_date
    } else {
        finish.unwrap_or_default()
    };
    let schedule_state = match (!start_date.is_empty(), !finish_date.is_empty()) {
        (true, true) => "full",
        (true, false) => "start_only",
        (false, true) => "finish_only",
        (false, false) => "unscheduled",
    };

    let mut payload = json!({
        "id": task_id,
        "parent_id": parent_task_id.clone().unwrap_or_default(),
        "title": title,
        "description": description.unwrap_or_default(),
        "links": input.links,
        "type": if milestone { "milestone" } else { "task" },
        "placement": if input.kind == CreateKind::Inbox { "inbox" } else { "wbs" },
        "schedule_state": schedule_state,
        "start": start_date,
        "finish": finish_date,
        "scheduling_mode": "auto",
        "priority": "normal",
        "responsible_email": "",
        "collaborator_emails": [],
    });
    if let Some(discipline) = discipline {
        payload["discipline"] = json!(discipline);
    }
    if let Some(geometry) = &input.geometry {
        payload["geometry"] = geometry.clone();
    }
    Ok(PreparedCommand {
        project: graph.project_id.clone(),
        base_revision: graph.revision,
        command: json!({ "kind": "create_task", "create_task": payload }),
    })
}

// ── update ──────────────────────────────────────────────────────────────

/// What `ds pm task update` was given. Every member optional; an absent one
/// is a field left alone, never cleared.
#[derive(Debug, Clone, Default)]
pub struct UpdateTask {
    pub task: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub discipline: Option<String>,
    pub priority: Option<String>,
    pub task_type: Option<String>,
    pub placement: Option<String>,
    pub scheduling_mode: Option<String>,
    pub delivery: Option<String>,
    pub review: Option<String>,
    pub closeout: Option<String>,
    /// Whole percent, 0 through 100.
    pub progress: Option<u64>,
    pub start_date: Option<String>,
    pub finish_date: Option<String>,
    /// Set the task's one top-level geometry (`update_task_fields.geometry`);
    /// `None` leaves it alone.
    pub geometry: Option<Value>,
    /// Remove the task's geometry (`update_task_fields.clear_geometry`).
    pub clear_geometry: bool,
    /// Replace the task's whole `links[]` — the engine keeps the first
    /// attachment of a link that was already there. `None` leaves them alone.
    pub links: Option<Vec<Value>>,
}

/// `updateCliWorkTask` (cli-pm.ts:611-689): every flag becomes one project
/// command, and they travel as one saved draft. The `kind` of each command is
/// reported beside the batch so the receipt can name what the flags became.
pub fn update_task(
    graph: &CanonicalProjectGraph,
    input: &UpdateTask,
) -> Result<(PreparedBatch, Vec<String>), Refusal> {
    let task_id = required_text(&input.task, "task", MAX_ID_LENGTH)?;
    let field = |value: Option<&str>, label: &str| -> Result<Option<String>, Refusal> {
        value
            .map(|raw| required_text(raw, &format!("fields.{label}"), MAX_TEXT_LENGTH))
            .transpose()
    };
    let title = field(input.title.as_deref(), "title")?;
    let description = field(input.description.as_deref(), "description")?;
    let discipline = field(input.discipline.as_deref(), "discipline")?;
    let priority = field(input.priority.as_deref(), "priority")?;
    let task_type = field(input.task_type.as_deref(), "type")?;
    let placement = field(input.placement.as_deref(), "placement")?;
    let scheduling_mode = field(input.scheduling_mode.as_deref(), "schedulingMode")?;
    let delivery = optional_text(input.delivery.as_deref(), "delivery", 64)?;
    let review = optional_text(input.review.as_deref(), "review", 64)?;
    let closeout = optional_text(input.closeout.as_deref(), "closeout", 64)?;
    if input.progress.is_some_and(|percent| percent > 100) {
        return Err(Refusal::InvalidValue(
            "progress must be a whole number from 0 through 100.".into(),
        ));
    }
    let start = input.start_date.as_deref().filter(|date| !date.is_empty());
    let finish = input.finish_date.as_deref().filter(|date| !date.is_empty());

    require_edit_schedule(graph)?;
    let task = require_task(graph, &task_id)?;

    let model = graph.field_model.as_ref();
    held(
        delivery.as_deref(),
        model.map(|m| m.delivery_states.as_slice()),
        "delivery",
    )?;
    held(
        review.as_deref(),
        model.map(|m| m.review_states.as_slice()),
        "review",
    )?;
    held(
        closeout.as_deref(),
        model.map(|m| m.closeout_states.as_slice()),
        "closeout",
    )?;
    held(
        priority.as_deref(),
        model.map(|m| m.priorities.as_slice()),
        "fields.priority",
    )?;
    held(
        task_type.as_deref(),
        model.map(|m| m.task_types.as_slice()),
        "fields.type",
    )?;
    held(
        placement.as_deref(),
        model.map(|m| m.placements.as_slice()),
        "fields.placement",
    )?;
    held(
        scheduling_mode.as_deref(),
        model.map(|m| m.scheduling_modes.as_slice()),
        "fields.schedulingMode",
    )?;
    // The type this task will HAVE after the patch, not the one it has now.
    let becomes_milestone = match task_type.as_deref() {
        Some(kind) => kind == "milestone",
        None => task.task_type == TaskType::Milestone,
    };
    if becomes_milestone && start.is_some() && finish.is_some() && start != finish {
        return Err(Refusal::InvalidShape(
            "A milestone cannot move to two different dates in one update.".into(),
        ));
    }

    let mut commands: Vec<Value> = Vec::new();
    let mut kinds: Vec<String> = Vec::new();
    let mut fields = Map::new();
    for (key, value) in [
        ("title", &title),
        ("description", &description),
        ("discipline", &discipline),
        ("priority", &priority),
        ("type", &task_type),
        ("placement", &placement),
        ("scheduling_mode", &scheduling_mode),
    ] {
        if let Some(value) = value {
            fields.insert(key.into(), json!(value));
        }
    }
    // Geometry and links ride the same `update_task_fields` command as the
    // authored fields: one command, one revision, exactly as the map's
    // drawing lifecycle commits them.
    if input.clear_geometry {
        fields.insert("clear_geometry".into(), json!(true));
    } else if let Some(geometry) = &input.geometry {
        fields.insert("geometry".into(), geometry.clone());
    }
    if let Some(links) = &input.links {
        fields.insert("links".into(), json!(links));
    }
    if !fields.is_empty() {
        fields.insert("task_id".into(), json!(task_id));
        commands.push(json!({ "kind": "update_task_fields", "update_task_fields": fields }));
        kinds.push("update_task_fields".into());
    }
    for (kind, state) in [
        ("set_delivery_state", &delivery),
        ("set_review_state", &review),
        ("set_closeout_state", &closeout),
    ] {
        if let Some(state) = state.as_deref().filter(|state| !state.is_empty()) {
            commands.push(json!({ "kind": kind, kind: { "task_id": task_id, "state": state } }));
            kinds.push(kind.into());
        }
    }
    if let Some(percent) = input.progress {
        commands.push(json!({
            "kind": "set_progress",
            "set_progress": { "task_id": task_id, "progress": percent as f64 / 100.0 },
        }));
        kinds.push("set_progress".into());
    }
    if start.is_some() || finish.is_some() {
        // A call that turns a task into a milestone and dates it in the same
        // draft would otherwise send `resize_task` for a node the engine is
        // about to treat as zero-duration, and be refused whole.
        if becomes_milestone {
            commands.push(json!({
                "kind": "move_task",
                "move_task": { "task_id": task_id, "start": start.or(finish).unwrap_or_default() },
            }));
            kinds.push("move_task".into());
        } else {
            let mut resize = json!({ "task_id": task_id });
            if let Some(start) = start {
                resize["start"] = json!(start);
            }
            if let Some(finish) = finish {
                resize["finish"] = json!(finish);
            }
            commands.push(json!({ "kind": "resize_task", "resize_task": resize }));
            kinds.push("resize_task".into());
        }
    }
    if commands.is_empty() {
        return Err(Refusal::NothingToChange);
    }
    let bound = model
        .map(|m| m.max_batch_commands)
        .filter(|bound| *bound > 0)
        .map_or(DEFAULT_MAX_BATCH_COMMANDS, |bound| bound as usize);
    if commands.len() > bound {
        return Err(Refusal::InvalidShape(format!(
            "One saved draft carries at most {bound} changes; this one carries {}.",
            commands.len()
        )));
    }
    Ok((
        PreparedBatch {
            project: graph.project_id.clone(),
            base_revision: graph.revision,
            commands,
        },
        kinds,
    ))
}

// ── assign ──────────────────────────────────────────────────────────────

/// `--request … | --owner … | --withdraw` (cli-pm.ts:691-711). Emails are
/// already normalised by the CLI, which refuses a malformed one by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assignment {
    /// Ask these people; the first to accept holds it. An empty list
    /// withdraws the open request — a real intent the engine implements.
    Request(Vec<String>),
    /// Transfer accountability directly; the former holder stays a
    /// collaborator.
    Owner(String),
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentMode {
    Request,
    Owner,
    Withdraw,
}

/// `assignCliWorkTask` + `directOwnerAssignment` (task-assignment.ts:10-28).
pub fn assign_task(
    graph: &CanonicalProjectGraph,
    task_id: &str,
    assignment: &Assignment,
) -> Result<(PreparedCommand, AssignmentMode), Refusal> {
    let task_id = required_text(task_id, "task", MAX_ID_LENGTH)?;
    require_edit_schedule(graph)?;
    let task = require_task(graph, &task_id)?;
    let max_assignees = graph
        .field_model
        .as_ref()
        .map(|model| model.max_assignees)
        .filter(|max| *max > 0)
        .map_or(DEFAULT_MAX_ASSIGNEES, |max| max as usize);

    let (command, mode) = match assignment {
        Assignment::Request(people) => {
            let mut emails: Vec<String> = Vec::with_capacity(people.len());
            for email in people {
                let email = email.trim().to_lowercase();
                if !email.is_empty() && !emails.contains(&email) {
                    emails.push(email);
                }
            }
            if emails.len() > max_assignees {
                return Err(Refusal::TooManyAssignees {
                    given: emails.len(),
                    max: max_assignees,
                });
            }
            let mode = if emails.is_empty() {
                AssignmentMode::Withdraw
            } else {
                AssignmentMode::Request
            };
            (
                json!({
                    "kind": "request_assignment",
                    "request_assignment": { "task_id": task_id, "emails": emails },
                }),
                mode,
            )
        }
        Assignment::Owner(email) => {
            let responsible = email.trim().to_lowercase();
            if responsible.is_empty() {
                return Err(Refusal::InvalidValue(
                    "owner must be a project member email address.".into(),
                ));
            }
            let mut collaborators: Vec<String> = Vec::new();
            if let Some(former) = task
                .responsible_email
                .as_ref()
                .filter(|former| **former != responsible)
            {
                collaborators.push(former.clone());
            }
            for candidate in &task.collaborator_emails {
                if *candidate != responsible && !collaborators.contains(candidate) {
                    collaborators.push(candidate.clone());
                }
            }
            (
                json!({
                    "kind": "assign_task",
                    "assign_task": {
                        "task_id": task_id,
                        "responsible_email": responsible,
                        "collaborator_emails": collaborators,
                    },
                }),
                AssignmentMode::Owner,
            )
        }
    };
    Ok((
        PreparedCommand {
            project: graph.project_id.clone(),
            base_revision: graph.revision,
            command,
        },
        mode,
    ))
}

// ── respond ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    Accept,
    Decline,
}

impl Response {
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "accept" => Some(Self::Accept),
            "decline" => Some(Self::Decline),
            _ => None,
        }
    }
    pub const fn token(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Decline => "decline",
        }
    }
}

/// `respondCliWorkTask` (cli-pm.ts:713-731). Answering a request that names
/// you is not editing the plan, so a contributor reaches this; the engine
/// independently verifies the responder is on the proposed list.
pub fn respond_task(
    graph: &CanonicalProjectGraph,
    task_id: &str,
    response: Response,
) -> Result<PreparedCommand, Refusal> {
    let task_id = required_text(task_id, "task", MAX_ID_LENGTH)?;
    require_answer_access(graph)?;
    require_task(graph, &task_id)?;
    let kind = match response {
        Response::Accept => "accept_assignment",
        Response::Decline => "decline_assignment",
    };
    Ok(PreparedCommand {
        project: graph.project_id.clone(),
        base_revision: graph.revision,
        command: json!({ "kind": kind, kind: { "task_id": task_id } }),
    })
}

// ── the engine's answer ─────────────────────────────────────────────────

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub code: String,
    pub message: String,
}

/// `decodeOperationResult` (wire.ts:280-303), the members a receipt reads.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationResult {
    pub applied: bool,
    pub command_id: String,
    pub committed_revision: i64,
    pub task_patches: Vec<CanonicalProjectTask>,
    pub warnings: Vec<Issue>,
    pub violations: Vec<Issue>,
}

fn issues(value: Option<&Value>) -> Vec<Issue> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .map(|raw| Issue {
                code: raw["code"]
                    .as_str()
                    .map(str::trim)
                    .filter(|code| !code.is_empty())
                    .unwrap_or("PROJECT_WORK_INVALID")
                    .to_owned(),
                message: raw["message"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The `result` of a `commit_command` / `commit_batch` answer, or the answer
/// itself when the server did not wrap it (`response.result ?? response`).
pub fn decode_operation_result(value: &Value) -> OperationResult {
    let raw = value
        .get("result")
        .filter(|r| r.is_object())
        .unwrap_or(value);
    let integer = |keys: &[&str]| -> i64 {
        keys.iter()
            .find_map(|key| raw.get(*key))
            .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f.trunc() as i64)))
            .unwrap_or_default()
    };
    OperationResult {
        applied: raw["applied"] == Value::Bool(true),
        command_id: raw["command_id"].as_str().unwrap_or_default().to_owned(),
        committed_revision: integer(&["committed_revision", "next_graph_revision"]),
        task_patches: match raw.get("task_patches") {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| item.as_object().and_then(decode_task))
                .collect(),
            _ => Vec::new(),
        },
        warnings: issues(raw.get("warnings")),
        violations: issues(raw.get("violations")),
    }
}

/// `writeOutcome` (cli-pm.ts:302-316): the receipt every write returns, with
/// the members each command adds beside it.
pub fn write_outcome(
    project: &str,
    task_id: &str,
    result: &OperationResult,
    extra: Map<String, Value>,
) -> Value {
    let mut outcome = json!({
        "project": project,
        "taskId": task_id,
        "applied": result.applied,
        "committedRevision": result.committed_revision,
        "warnings": result.warnings,
        "link": task_route(project, task_id),
    });
    for (key, value) in extra {
        outcome[key] = value;
    }
    outcome
}

/// Who holds the task and who is still asked after a write, read from the
/// engine's own patch of it — or from the task as it was, when the patch did
/// not touch it.
pub fn assignment_after(
    result: &OperationResult,
    task_id: &str,
    before: Option<&CanonicalProjectTask>,
) -> (String, Vec<String>) {
    let after = result.task_patches.iter().find(|patch| patch.id == task_id);
    let responsible = after
        .and_then(|patch| patch.responsible_email.clone())
        .or_else(|| before.and_then(|task| task.responsible_email.clone()))
        .unwrap_or_default();
    let requested = after
        .map(|patch| patch.proposed_assignee_emails.clone())
        .unwrap_or_default();
    (responsible, requested)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_management::wire::decode_project_graph;

    fn graph(edit_schedule: bool) -> CanonicalProjectGraph {
        decode_project_graph(
            &json!({
                "project_id": "p", "revision": 4,
                "permissions": {"view": true, "contribute": true, "edit_schedule": edit_schedule},
                "field_model": {
                    "delivery_states": ["not_started", "in_progress", "complete"],
                    "review_states": ["not_required", "under_review"],
                    "closeout_states": ["open", "closed"],
                    "priorities": ["low", "normal", "high"],
                    "task_types": ["task", "milestone"],
                    "scheduling_modes": ["manual", "auto"],
                    "placements": ["wbs", "inbox"],
                    "max_assignees": 3,
                    "max_batch_commands": 200
                },
                "tasks": [
                    {"id": "t1", "data": {"title": "Root", "responsible_email": "lead@ds.rw",
                                          "collaborator_emails": ["field@ds.rw"]}},
                    {"id": "m1", "data": {"title": "Handover", "type": "milestone", "parent_task_id": "t1"}},
                ],
            }),
            "p",
        )
    }

    fn create(
        kind: CreateKind,
        parent: Option<&str>,
        start: Option<&str>,
        finish: Option<&str>,
    ) -> CreateTask {
        CreateTask {
            id: "new".into(),
            title: "  Stake MV route  ".into(),
            kind,
            parent: parent.map(str::to_owned),
            description: Some("what done looks like".into()),
            discipline: Some("Design".into()),
            start_date: start.map(str::to_owned),
            finish_date: finish.map(str::to_owned),
            geometry: None,
            links: Vec::new(),
        }
    }

    /// A create that carries WHERE: the geometry `task_geometry::propose`
    /// shaped and one `ds_object` link per object, in the same command —
    /// what `ds pm task create --geometry-from` sends.
    #[test]
    fn a_create_carries_its_geometry_and_links_in_the_one_command() {
        let mut input = create(CreateKind::Inbox, None, None, None);
        input.geometry = Some(json!({ "type": "Point", "coordinates": [29.61, -2.34] }));
        input.links = vec![json!({
            "kind": "ds_object", "project_id": "p", "object_type": "dsgrid_structure",
            "entity_id": "m-1:s-74", "object_revision": "dsgrid:r3:abc", "label": "Structure 74",
        })];
        let payload =
            create_task(&graph(true), &input).expect("valid").command["create_task"].clone();
        assert_eq!(
            payload["geometry"],
            json!({ "type": "Point", "coordinates": [29.61, -2.34] })
        );
        assert_eq!(payload["links"][0]["entity_id"], "m-1:s-74");
        // And without them the payload is exactly what it was.
        let plain = create_task(&graph(true), &create(CreateKind::Inbox, None, None, None))
            .expect("valid")
            .command["create_task"]
            .clone();
        assert!(plain.get("geometry").is_none());
        assert_eq!(plain["links"], json!([]));
    }

    /// `ds pm task geometry set|clear`: geometry and links travel in the same
    /// `update_task_fields` command as any authored field — one revision.
    #[test]
    fn an_update_sets_or_clears_geometry_and_replaces_links_in_update_task_fields() {
        let set = UpdateTask {
            task: "t1".into(),
            geometry: Some(json!({ "type": "Point", "coordinates": [29.61, -2.34] })),
            links: Some(vec![
                json!({ "kind": "pm_task", "project_id": "p", "entity_id": "m1" }),
            ]),
            ..UpdateTask::default()
        };
        let (batch, kinds) = update_task(&graph(true), &set).expect("valid");
        assert_eq!(kinds, vec!["update_task_fields"]);
        let fields = &batch.commands[0]["update_task_fields"];
        assert_eq!(fields["task_id"], "t1");
        assert_eq!(fields["geometry"]["type"], "Point");
        assert_eq!(fields["links"][0]["entity_id"], "m1");
        assert!(fields.get("clear_geometry").is_none());

        let clear = UpdateTask {
            task: "t1".into(),
            clear_geometry: true,
            links: Some(Vec::new()),
            title: Some("Renamed".into()),
            ..UpdateTask::default()
        };
        let (batch, kinds) = update_task(&graph(true), &clear).expect("valid");
        assert_eq!(kinds, vec!["update_task_fields"]);
        let fields = &batch.commands[0]["update_task_fields"];
        assert_eq!(fields["clear_geometry"], true);
        assert_eq!(fields["links"], json!([]));
        assert_eq!(fields["title"], "Renamed");
        assert!(fields.get("geometry").is_none());
    }

    #[test]
    fn a_create_is_the_command_the_plan_sheet_would_have_sent() {
        let prepared = create_task(
            &graph(true),
            &create(CreateKind::Child, Some("t1"), Some("2026-09-01"), None),
        )
        .expect("valid");
        assert_eq!(prepared.base_revision, 4);
        let payload = &prepared.command["create_task"];
        assert_eq!(prepared.command["kind"], "create_task");
        assert_eq!(payload["title"], "Stake MV route");
        assert_eq!(payload["parent_id"], "t1");
        assert_eq!(payload["schedule_state"], "start_only");
        assert_eq!(payload["type"], "task");
        assert_eq!(payload["placement"], "wbs");
        assert_eq!(payload["discipline"], "Design");
        assert_eq!(payload["scheduling_mode"], "auto");
        assert_eq!(payload["finish"], "");

        let milestone = create_task(
            &graph(true),
            &create(CreateKind::Milestone, Some("t1"), None, Some("2026-09-30")),
        )
        .expect("valid")
        .command;
        assert_eq!(milestone["create_task"]["start"], "2026-09-30");
        assert_eq!(milestone["create_task"]["finish"], "2026-09-30");
        assert_eq!(milestone["create_task"]["type"], "milestone");

        let inbox = create_task(&graph(true), &create(CreateKind::Inbox, None, None, None))
            .expect("valid")
            .command;
        assert_eq!(inbox["create_task"]["placement"], "inbox");
        assert_eq!(inbox["create_task"]["schedule_state"], "unscheduled");
    }

    #[test]
    fn a_create_refuses_the_shapes_the_engine_has_no_command_for() {
        let g = graph(true);
        assert!(matches!(
            create_task(&g, &create(CreateKind::Child, None, None, None)),
            Err(Refusal::InvalidShape(_))
        ));
        assert!(matches!(
            create_task(&g, &create(CreateKind::Parent, Some("t1"), None, None)),
            Err(Refusal::InvalidShape(_))
        ));
        assert!(matches!(
            create_task(
                &g,
                &create(
                    CreateKind::Milestone,
                    None,
                    Some("2026-09-01"),
                    Some("2026-09-02")
                )
            ),
            Err(Refusal::InvalidShape(_))
        ));
        assert!(matches!(
            create_task(&g, &create(CreateKind::Child, Some("nope"), None, None)),
            Err(Refusal::TaskNotFound(id)) if id == "nope"
        ));
        let mut retry = create(CreateKind::Parent, None, None, None);
        retry.id = "t1".into();
        assert!(matches!(
            create_task(&g, &retry),
            Err(Refusal::TaskExists(_))
        ));
        assert!(matches!(
            create_task(&graph(false), &create(CreateKind::Parent, None, None, None)),
            Err(Refusal::NotPermitted(_))
        ));
    }

    #[test]
    fn an_update_folds_its_flags_into_one_draft_of_named_commands() {
        let (batch, kinds) = update_task(
            &graph(true),
            &UpdateTask {
                task: "t1".into(),
                title: Some("Renamed".into()),
                delivery: Some("in_progress".into()),
                progress: Some(40),
                start_date: Some("2026-09-02".into()),
                ..Default::default()
            },
        )
        .expect("valid");
        assert_eq!(
            kinds,
            vec![
                "update_task_fields",
                "set_delivery_state",
                "set_progress",
                "resize_task"
            ]
        );
        assert_eq!(batch.commands[0]["update_task_fields"]["title"], "Renamed");
        assert_eq!(
            batch.commands[1]["set_delivery_state"]["state"],
            "in_progress"
        );
        assert_eq!(batch.commands[2]["set_progress"]["progress"], 0.4);
        assert_eq!(batch.commands[3]["resize_task"]["start"], "2026-09-02");
        assert!(batch.commands[3]["resize_task"].get("finish").is_none());

        // A milestone — or a task becoming one — moves, it does not resize.
        let (batch, kinds) = update_task(
            &graph(true),
            &UpdateTask {
                task: "t1".into(),
                task_type: Some("milestone".into()),
                finish_date: Some("2026-09-30".into()),
                ..Default::default()
            },
        )
        .expect("valid");
        assert_eq!(kinds, vec!["update_task_fields", "move_task"]);
        assert_eq!(batch.commands[1]["move_task"]["start"], "2026-09-30");
    }

    #[test]
    fn an_update_is_held_to_the_vocabulary_the_engine_published() {
        let g = graph(true);
        let refused = update_task(
            &g,
            &UpdateTask {
                task: "t1".into(),
                delivery: Some("done".into()),
                ..Default::default()
            },
        )
        .expect_err("outside the vocabulary");
        assert!(
            matches!(refused, Refusal::InvalidValue(ref m) if m.contains("delivery must be one of"))
        );
        assert!(matches!(
            update_task(
                &g,
                &UpdateTask {
                    task: "t1".into(),
                    ..Default::default()
                }
            ),
            Err(Refusal::NothingToChange)
        ));
        assert!(matches!(
            update_task(
                &g,
                &UpdateTask {
                    task: "m1".into(),
                    start_date: Some("2026-09-01".into()),
                    finish_date: Some("2026-09-02".into()),
                    ..Default::default()
                }
            ),
            Err(Refusal::InvalidShape(_))
        ));
    }

    #[test]
    fn an_assignment_is_a_request_a_transfer_or_a_withdrawal() {
        let g = graph(true);
        let (prepared, mode) = assign_task(
            &g,
            "t1",
            &Assignment::Request(vec!["Pilot@ds.rw".into(), "pilot@ds.rw".into()]),
        )
        .expect("valid");
        assert_eq!(mode, AssignmentMode::Request);
        assert_eq!(
            prepared.command["request_assignment"]["emails"],
            json!(["pilot@ds.rw"])
        );

        let (prepared, mode) = assign_task(&g, "t1", &Assignment::Request(vec![])).expect("valid");
        assert_eq!(mode, AssignmentMode::Withdraw);
        assert_eq!(prepared.command["request_assignment"]["emails"], json!([]));

        // The former holder stays a collaborator; the rest of the crew stays.
        let (prepared, mode) =
            assign_task(&g, "t1", &Assignment::Owner("new@ds.rw".into())).expect("valid");
        assert_eq!(mode, AssignmentMode::Owner);
        assert_eq!(
            prepared.command["assign_task"]["responsible_email"],
            "new@ds.rw"
        );
        assert_eq!(
            prepared.command["assign_task"]["collaborator_emails"],
            json!(["lead@ds.rw", "field@ds.rw"])
        );

        assert!(matches!(
            assign_task(
                &g,
                "t1",
                &Assignment::Request(vec![
                    "a@x.rw".into(),
                    "b@x.rw".into(),
                    "c@x.rw".into(),
                    "d@x.rw".into()
                ])
            ),
            Err(Refusal::TooManyAssignees { given: 4, max: 3 })
        ));
        assert!(matches!(
            assign_task(
                &graph(false),
                "t1",
                &Assignment::Request(vec!["a@x.rw".into()])
            ),
            Err(Refusal::NotPermitted(_))
        ));
    }

    #[test]
    fn a_response_needs_only_contributor_access_and_names_its_command() {
        let g = graph(false);
        let prepared = respond_task(&g, "t1", Response::Accept).expect("a contributor may answer");
        assert_eq!(prepared.command["kind"], "accept_assignment");
        assert_eq!(prepared.command["accept_assignment"]["task_id"], "t1");
        assert!(matches!(
            respond_task(&g, "zz", Response::Decline),
            Err(Refusal::TaskNotFound(_))
        ));
    }

    #[test]
    fn the_engines_answer_folds_into_the_documented_receipt() {
        let answer = json!({"result": {
            "applied": true, "command_id": "c1", "committed_revision": 5,
            "task_patches": [{"id": "t1", "data": {"title": "Root", "responsible_email": "new@ds.rw",
                              "proposed_assignee_emails": ["pilot@ds.rw"]}}],
            "warnings": [{"code": "W1", "message": "a dependant moved"}],
        }});
        let result = decode_operation_result(&answer);
        assert!(result.applied);
        assert_eq!(result.committed_revision, 5);
        let (responsible, requested) = assignment_after(&result, "t1", None);
        assert_eq!(responsible, "new@ds.rw");
        assert_eq!(requested, vec!["pilot@ds.rw"]);
        let outcome = write_outcome("p", "t1", &result, Map::new());
        assert_eq!(outcome["committedRevision"], 5);
        assert_eq!(outcome["warnings"][0]["code"], "W1");
        assert_eq!(outcome["link"], "/projects/p/work?view=gantt&task=t1");

        // Unwrapped, with the legacy revision key, and a violation.
        let bare = decode_operation_result(&json!({
            "applied": false, "next_graph_revision": 4,
            "violations": [{"message": "task \"T-1\" already exists"}],
        }));
        assert!(!bare.applied);
        assert_eq!(bare.violations[0].code, "PROJECT_WORK_INVALID");
    }
}
