//! The bounded reads `ds pm task list|read` and `ds pm record list|read`
//! answer — decided in the kernel, no longer in a browser window.
//!
//! Until 2026-09-20 these four commands relayed through the paired desktop to
//! `ds-web/src/lib/desktop/cli-pm.ts` (`listCliWorkTasks` line 456,
//! `readCliWorkTask` 493, `listCliWorkRecords` 733, `readCliWorkRecord` 749),
//! so a server with no window could not list a plan's tasks at all. This
//! module is those four folds, translated as they were: the same row shapes,
//! the same bounds, the same filters, the same sort. The graph is the server's
//! answer (`get_graph`); the records are the server's answer (`get_context`,
//! which the browser never asked for — `decodeProjectGraph` hard-coded
//! `records: []`, so `ds pm record list` answered nothing however many records
//! a project held); what they MEAN is decided here once, for the CLI, MCP, the
//! Server and the page alike.
//!
//! Pure by the crate's contract: no clock, no IO, no network.

use serde::Serialize;
use serde_json::{Map, Value};

use super::hierarchy::ordered_rows;
use super::wire::{
    optional_instant, CanonicalProjectGraph, CanonicalProjectTask, ProjectGraphPermissions,
    ResidualStatus,
};

/// The largest page any Project Work projection returns (cli-pm.ts:98) — the
/// same number `ds pm` documents, so an over-large `--limit` is refused once.
pub const MAX_PAGE_SIZE: i64 = 250;
/// The page a caller gets without asking (cli-pm.ts:380).
pub const DEFAULT_PAGE_SIZE: i64 = 50;
/// Detail reads have no paging cursor. Each related collection reports its
/// full count and carries at most this many rows (cli-pm.ts:104).
pub const MAX_RELATED_ROWS: usize = 250;
/// How much of a record body one read carries (cli-pm.ts:100).
pub const MAX_RECORD_BODY_CHARS: usize = 8_000;
/// How much of a task description one read carries (cli-pm.ts:105).
pub const MAX_TEXT_LENGTH: usize = 4_000;

// ── task rows ───────────────────────────────────────────────────────────

/// One task as the Table lists it (`taskRow`, cli-pm.ts:318-338).
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskRow {
    pub wbs: String,
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub task_type: String,
    pub delivery: String,
    pub review: String,
    pub closeout: String,
    /// Whole percent, 0 through 100.
    pub progress: u64,
    pub start: String,
    pub finish: String,
    pub responsible: String,
    pub discipline: String,
    pub priority: String,
    pub blockers: i64,
    /// Open correspondence blockers — records this task waits on — counted
    /// by the engine (`computed.awaiting_correspondence_count`).
    pub awaiting_correspondence: i64,
    /// The records those open blockers name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocked_record_ids: Vec<String>,
    /// True while an assignment request is waiting for an answer.
    pub assignment_open: bool,
    pub updated_at: i64,
}

/// The page bounds every list carries (`boundedPage`, views.ts:157-172).
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PageBounds {
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
    pub page_count: usize,
    pub from: usize,
    pub to: usize,
}

fn enum_token<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn percent(progress: f64) -> u64 {
    (progress.clamp(0.0, 1.0) * 100.0).round() as u64
}

fn task_row(wbs: &str, task: &CanonicalProjectTask) -> TaskRow {
    TaskRow {
        wbs: wbs.to_owned(),
        id: task.id.clone(),
        title: task.title.clone(),
        task_type: enum_token(&task.task_type),
        delivery: enum_token(&task.delivery_state),
        review: enum_token(&task.review_state),
        closeout: enum_token(&task.closeout_state),
        progress: percent(task.progress),
        start: task.start_date.clone(),
        finish: task.finish_date.clone(),
        responsible: task.responsible_email.clone().unwrap_or_default(),
        discipline: task.discipline.clone(),
        priority: enum_token(&task.priority),
        blockers: task.open_blocker_count + task.descendant_blocker_count,
        awaiting_correspondence: task.awaiting_correspondence_count,
        blocked_record_ids: task.blocked_record_ids.clone(),
        assignment_open: !task.proposed_assignee_emails.is_empty(),
        updated_at: task.updated_at,
    }
}

/// `projectWorkTableRows` (views.ts:190-201): every task in plan order with
/// its WBS label. A task the hierarchy walk did not reach — there is none
/// today, the walk re-roots every orphan — would be absent here as it was in
/// the browser.
fn table_rows(graph: &CanonicalProjectGraph) -> Vec<(String, &CanonicalProjectTask)> {
    ordered_rows(&graph.tasks)
        .into_iter()
        .filter_map(|row| {
            graph
                .tasks
                .iter()
                .find(|task| task.id == row.id)
                .map(|task| (row.wbs, task))
        })
        .collect()
}

/// `boundedPage` (views.ts:157-172), over an already-filtered list.
fn page<T: Clone>(items: &[T], requested_page: i64, page_size: i64) -> (Vec<T>, PageBounds) {
    let page_size = page_size.clamp(1, MAX_PAGE_SIZE) as usize;
    let page_count = items.len().div_ceil(page_size).max(1);
    let page = (requested_page.max(0) as usize).min(page_count - 1);
    let start = page * page_size;
    let end = items.len().min(start + page_size);
    let slice = if start < items.len() {
        items[start..end].to_vec()
    } else {
        Vec::new()
    };
    (
        slice,
        PageBounds {
            total: items.len(),
            page,
            page_size,
            page_count,
            from: if items.is_empty() { 0 } else { start + 1 },
            to: end,
        },
    )
}

/// `--placement any|wbs|inbox` (cli-pm.ts:463).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlacementFilter {
    #[default]
    Any,
    Wbs,
    Inbox,
}

/// What `ds pm task list` filters on. Every member optional; an absent one
/// matches everything.
#[derive(Debug, Clone, Default)]
pub struct TaskListFilter {
    /// Matches WBS, id, title, description, the responsible person or a
    /// collaborator, case-insensitively.
    pub query: String,
    pub state: Option<String>,
    pub assignee: Option<String>,
    pub discipline: Option<String>,
    pub placement: PlacementFilter,
    /// Only tasks openly blocked on correspondence — the engine's own
    /// `computed.awaiting_correspondence_count > 0` (correspondence.md
    /// `task_list --blocked-on-correspondence`: a graph filter, no action).
    pub blocked_on_correspondence: bool,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskListReply {
    pub project: String,
    pub revision: i64,
    #[serde(flatten)]
    pub bounds: PageBounds,
    pub tasks: Vec<TaskRow>,
}

/// `listCliWorkTasks` (cli-pm.ts:456-491) less the session: the same
/// text filter as the Table's search box (`filterProjectWorkTableRows`,
/// views.ts:174-188), then the state, discipline, holder and placement
/// narrowings, then one bounded page.
pub fn task_list(graph: &CanonicalProjectGraph, filter: &TaskListFilter) -> TaskListReply {
    let needle = filter.query.trim().to_lowercase();
    let assignee = filter.assignee.as_ref().map(|email| email.to_lowercase());
    let holds = |task: &CanonicalProjectTask, email: &str| {
        task.responsible_email
            .as_ref()
            .is_some_and(|holder| holder.to_lowercase() == email)
            || task
                .collaborator_emails
                .iter()
                .any(|entry| entry.to_lowercase() == email)
            || task
                .assignee_emails
                .iter()
                .any(|entry| entry.to_lowercase() == email)
            || task
                .proposed_assignee_emails
                .iter()
                .any(|entry| entry.to_lowercase() == email)
    };
    let matched: Vec<TaskRow> = table_rows(graph)
        .into_iter()
        .filter(|(wbs, task)| {
            if needle.is_empty() {
                return true;
            }
            let mut haystack: Vec<String> = vec![
                wbs.clone(),
                task.id.clone(),
                task.title.clone(),
                task.description.clone(),
            ];
            haystack.extend(task.responsible_email.iter().cloned());
            haystack.extend(task.collaborator_emails.iter().cloned());
            haystack
                .iter()
                .any(|value| value.to_lowercase().contains(&needle))
        })
        .filter(|(_, task)| {
            filter
                .state
                .as_ref()
                .is_none_or(|state| enum_token(&task.delivery_state) == *state)
        })
        .filter(|(_, task)| {
            filter
                .discipline
                .as_ref()
                .is_none_or(|discipline| task.discipline == *discipline)
        })
        .filter(|(_, task)| assignee.as_ref().is_none_or(|email| holds(task, email)))
        .filter(|(_, task)| match filter.placement {
            PlacementFilter::Any => true,
            PlacementFilter::Wbs => enum_token(&task.placement) == "wbs",
            PlacementFilter::Inbox => enum_token(&task.placement) == "inbox",
        })
        .filter(|(_, task)| {
            !filter.blocked_on_correspondence || task.awaiting_correspondence_count > 0
        })
        .map(|(wbs, task)| task_row(&wbs, task))
        .collect();
    let (tasks, bounds) = page(&matched, filter.page, filter.page_size);
    TaskListReply {
        project: graph.project_id.clone(),
        revision: graph.revision,
        bounds,
        tasks,
    }
}

// ── one task ────────────────────────────────────────────────────────────

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    #[serde(flatten)]
    pub row: TaskRow,
    pub description: String,
    pub description_truncated: bool,
    pub collaborators: Vec<String>,
    /// Who has been asked to take the work and has not yet answered.
    pub requested: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_assignment_action: Option<String>,
    pub scheduling_mode: String,
    pub schedule_state: String,
    pub placement: String,
    pub parent_task_id: Option<String>,
    pub version: i64,
    /// Every correspondence blocker the task carries, open or cleared, as
    /// the engine wrote it (`data.blockers[]`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub correspondence_blockers: Vec<super::wire::TaskBlocker>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DependencyRow {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub dependency_type: String,
    pub lag_minutes: i64,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResidualRow {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub blocks_acceptance: bool,
    pub blocks_closeout: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordRef {
    pub id: String,
    pub category: String,
    pub subject: String,
    pub happened_at: String,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskReadReply {
    pub project: String,
    pub revision: i64,
    pub task: TaskDetail,
    pub dependencies: Vec<DependencyRow>,
    pub dependency_total: usize,
    pub residuals: Vec<ResidualRow>,
    pub residual_total: usize,
    /// Work episodes are not on the canonical graph and the browser never
    /// read them from the context either (wire.ts:35-39): always empty, and
    /// the total says so honestly rather than inventing rows.
    pub episodes: Vec<Value>,
    pub episode_total: usize,
    pub records: Vec<RecordRef>,
    pub record_total: usize,
    pub permissions: ProjectGraphPermissions,
    pub link: String,
}

/// `projectWorkTaskRoute` (deep-link.ts:9-24) with no action: the Gantt view
/// with the task selected.
pub fn task_route(project: &str, task: &str) -> String {
    format!(
        "/projects/{}/work?view=gantt&task={}",
        url_component(project),
        form_component(task)
    )
}

/// `encodeURIComponent`, for the path segment — spelled the way the browser
/// spelled it so a link pasted from `ds` opens the same page as one copied
/// from the app.
fn url_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `URLSearchParams`' application/x-www-form-urlencoded serializer, for the
/// query value: a space is `+`, and only `*-._` and alphanumerics pass.
fn form_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A description cut on a character boundary, as `slice(0, MAX)` cuts it.
fn bounded_text(text: &str, max: usize) -> (String, bool) {
    let count = text.chars().count();
    if count <= max {
        (text.to_owned(), false)
    } else {
        (text.chars().take(max).collect(), true)
    }
}

/// `readCliWorkTask` (cli-pm.ts:493-562) less the session. `records` are the
/// context records the caller fetched — the browser had none, so its
/// `records` was always empty; here they are the ones that name the task.
/// `None` when no task carries that id.
pub fn task_read(
    graph: &CanonicalProjectGraph,
    task_id: &str,
    records: &[ContextRecord],
) -> Option<TaskReadReply> {
    let task = graph.tasks.iter().find(|task| task.id == task_id)?;
    let wbs = table_rows(graph)
        .into_iter()
        .find(|(_, candidate)| candidate.id == task_id)
        .map(|(wbs, _)| wbs)
        .unwrap_or_default();
    let (description, description_truncated) = bounded_text(&task.description, MAX_TEXT_LENGTH);

    let dependencies: Vec<DependencyRow> = graph
        .dependencies
        .iter()
        .filter(|edge| edge.from_task_id == task_id || edge.to_task_id == task_id)
        .map(|edge| DependencyRow {
            id: edge.id.clone(),
            from: edge.from_task_id.clone(),
            to: edge.to_task_id.clone(),
            dependency_type: enum_token(&edge.dependency_type),
            lag_minutes: edge.lag_minutes,
        })
        .collect();
    let mut residuals: Vec<&super::wire::TaskResidualItem> = graph
        .residual_items
        .iter()
        .filter(|item| item.task_id == task_id)
        .collect();
    // `openFirst`: open rows before the rest, otherwise wire order. A stable
    // sort, as `Array.prototype.sort` is.
    residuals.sort_by_key(|item| u8::from(item.status != ResidualStatus::Open));
    let residuals: Vec<ResidualRow> = residuals
        .into_iter()
        .map(|item| ResidualRow {
            id: item.id.clone(),
            title: item.title.clone(),
            kind: enum_token(&item.kind),
            status: enum_token(&item.status),
            blocks_acceptance: item.blocks_acceptance,
            blocks_closeout: item.blocks_closeout,
            due_date: item.due_date.clone(),
        })
        .collect();
    let related: Vec<RecordRef> = records
        .iter()
        .filter(|entry| entry.related_task_ids.iter().any(|id| id == task_id))
        .map(|entry| RecordRef {
            id: entry.id.clone(),
            category: entry.category.clone(),
            subject: entry.subject.clone(),
            happened_at: entry.happened_at.clone(),
        })
        .collect();

    Some(TaskReadReply {
        project: graph.project_id.clone(),
        revision: graph.revision,
        task: TaskDetail {
            row: task_row(&wbs, task),
            description,
            description_truncated,
            collaborators: task.collaborator_emails.clone(),
            requested: task.proposed_assignee_emails.clone(),
            last_assignment_action: task.assignment_last_action.as_ref().map(enum_token),
            scheduling_mode: enum_token(&task.scheduling_mode),
            schedule_state: enum_token(&task.schedule_state),
            placement: enum_token(&task.placement),
            parent_task_id: task.parent_task_id.clone(),
            version: task.version,
            correspondence_blockers: task.blockers.clone(),
        },
        dependency_total: dependencies.len(),
        dependencies: dependencies.into_iter().take(MAX_RELATED_ROWS).collect(),
        residual_total: residuals.len(),
        residuals: residuals.into_iter().take(MAX_RELATED_ROWS).collect(),
        episodes: Vec::new(),
        episode_total: 0,
        record_total: related.len(),
        records: related.into_iter().take(MAX_RELATED_ROWS).collect(),
        permissions: graph.permissions,
        link: task_route(&graph.project_id, task_id),
    })
}

// ── records ─────────────────────────────────────────────────────────────

/// One record of the project's context, as `ds pm record` reads it.
///
/// A translation of the members of `decodeRecord` (`api/pm-context.ts:369-410`)
/// that the two record commands project, plus the correspondence facts the
/// server now publishes beside them (`thread_id`, `channel`, `computed
/// .response_status`; ds-brain `docs/contracts/correspondence.md`). Read
/// leniently on purpose: a row the browser's strict decoder would have thrown
/// on — an unknown category, a missing metadata stamp — is still a record an
/// operator asked to see, so an unreadable member reads as empty rather than
/// hiding the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRecord {
    pub id: String,
    pub category: String,
    pub state: String,
    pub direction: String,
    pub channel: Option<String>,
    pub subject: String,
    pub body: String,
    pub happened_at: String,
    pub response_required: bool,
    pub response_due_date: Option<String>,
    pub response_status: Option<String>,
    pub thread_id: Option<String>,
    pub reply_to_record_id: Option<String>,
    pub reference_number: Option<String>,
    pub affects_scope: bool,
    pub affects_schedule: bool,
    pub affects_quality: bool,
    pub affects_cost: bool,
    pub related_task_ids: Vec<String>,
    pub related_residual_item_ids: Vec<String>,
    pub related_record_ids: Vec<String>,
    pub revision_label: Option<String>,
    pub document_state: Option<String>,
    pub source_conversation_id: Option<String>,
    pub source_message_id: Option<String>,
    pub source_artifact_id: Option<String>,
    pub version: i64,
    // The correspondence contract's additions (ds-brain
    // `docs/contracts/correspondence.md` §Record): who owes the answer, the
    // parties, what the record was authored against, and the derivation.
    pub response_owner_id: Option<String>,
    pub response_owner_party_id: Option<String>,
    pub external_party_ids: Vec<String>,
    pub external_participant_names: Vec<String>,
    pub source_asset_id: Option<String>,
    pub meeting_note_asset_id: Option<String>,
    pub document_ids: Vec<String>,
    pub resulting_task_ids: Vec<String>,
    pub waived: bool,
    pub waived_reason: Option<String>,
    /// `open` when the server named none: every record authored before the
    /// contract reads as `open`.
    pub sensitivity: String,
    pub responded_by_record_id: Option<String>,
    pub responded_at: Option<String>,
    pub days_overdue: i64,
    pub days_until_due: i64,
}

fn record_map(value: Option<&Value>) -> &Map<String, Value> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    match value {
        Some(Value::Object(map)) => map,
        _ => EMPTY.get_or_init(Map::new),
    }
}

fn text_of(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(raw)) => raw.trim().to_owned(),
        _ => String::new(),
    }
}

fn optional_text_of(value: Option<&Value>) -> Option<String> {
    let text = text_of(value);
    (!text.is_empty()).then_some(text)
}

fn texts_of(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| text_of(Some(item)))
            .filter(|item| !item.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// `liveEntity` + `decodeRecord`: the envelope's `data` map with the
/// envelope's own `id`, `version` and `computed` beside it. A deleted or
/// idless row is not a record.
pub fn decode_context_record(envelope: &Value) -> Option<ContextRecord> {
    let outer = record_map(Some(envelope));
    if outer.get("is_deleted") == Some(&Value::Bool(true)) {
        return None;
    }
    let data = record_map(outer.get("data"));
    let computed = record_map(outer.get("computed"));
    let id = optional_text_of(outer.get("id")).or_else(|| optional_text_of(data.get("id")))?;
    let flag = |key: &str| data.get(key) == Some(&Value::Bool(true));
    Some(ContextRecord {
        id,
        category: text_of(data.get("category")),
        state: text_of(data.get("state")),
        direction: text_of(data.get("direction")),
        channel: optional_text_of(data.get("channel")),
        subject: text_of(data.get("subject")),
        body: match data.get("body") {
            Some(Value::String(raw)) => raw.clone(),
            _ => String::new(),
        },
        happened_at: optional_instant(data.get("happened_at")).unwrap_or_default(),
        response_required: flag("response_required"),
        response_due_date: optional_text_of(data.get("response_due_date")),
        response_status: optional_text_of(computed.get("response_status")),
        thread_id: optional_text_of(data.get("thread_id")),
        reply_to_record_id: optional_text_of(data.get("reply_to_record_id")),
        reference_number: optional_text_of(data.get("reference_number")),
        affects_scope: flag("affects_scope"),
        affects_schedule: flag("affects_schedule"),
        affects_quality: flag("affects_quality"),
        affects_cost: flag("affects_cost"),
        related_task_ids: texts_of(data.get("related_task_ids")),
        related_residual_item_ids: texts_of(data.get("related_residual_item_ids")),
        related_record_ids: texts_of(data.get("related_record_ids")),
        revision_label: optional_text_of(data.get("revision_label")),
        document_state: optional_text_of(data.get("document_state")),
        source_conversation_id: optional_text_of(data.get("source_conversation_id")),
        source_message_id: optional_text_of(data.get("source_message_id")),
        source_artifact_id: optional_text_of(data.get("source_artifact_id")),
        version: outer
            .get("version")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        response_owner_id: optional_text_of(data.get("response_owner_id")),
        response_owner_party_id: optional_text_of(data.get("response_owner_party_id")),
        external_party_ids: texts_of(data.get("external_party_ids")),
        external_participant_names: texts_of(data.get("external_participant_names")),
        source_asset_id: optional_text_of(data.get("source_asset_id")),
        meeting_note_asset_id: optional_text_of(data.get("meeting_note_asset_id")),
        document_ids: texts_of(data.get("document_ids")),
        resulting_task_ids: texts_of(data.get("resulting_task_ids")),
        waived: flag("waived"),
        waived_reason: optional_text_of(data.get("waived_reason")),
        sensitivity: optional_text_of(data.get("sensitivity")).unwrap_or_else(|| "open".into()),
        responded_by_record_id: optional_text_of(computed.get("responded_by_record_id")),
        responded_at: optional_instant(computed.get("responded_at")),
        days_overdue: computed
            .get("days_overdue")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .max(0),
        days_until_due: computed
            .get("days_until_due")
            .and_then(Value::as_i64)
            .unwrap_or_default()
            .max(0),
    })
}

/// Every readable record of one `get_context` answer.
pub fn decode_context_records(context: &Value) -> Vec<ContextRecord> {
    match context.get("records") {
        Some(Value::Array(items)) => items.iter().filter_map(decode_context_record).collect(),
        _ => Vec::new(),
    }
}

/// One record as the list shows it (`recordRow`, cli-pm.ts:340-354).
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordRow {
    pub id: String,
    pub category: String,
    pub state: String,
    pub direction: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    pub subject: String,
    pub happened_at: String,
    pub response_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_due_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub related_tasks: usize,
    /// The project user who owes the answer (an account uid), when one does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_owner_id: Option<String>,
    /// The party that owes the answer, when one does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_owner_party_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub party_ids: Vec<String>,
    pub sensitivity: String,
}

/// One record as every list of this module shows it.
pub fn record_row(entry: &ContextRecord) -> RecordRow {
    RecordRow {
        id: entry.id.clone(),
        category: entry.category.clone(),
        state: entry.state.clone(),
        direction: entry.direction.clone(),
        channel: entry.channel.clone(),
        subject: entry.subject.clone(),
        happened_at: entry.happened_at.clone(),
        response_required: entry.response_required,
        response_due_date: entry.response_due_date.clone(),
        response_status: entry.response_status.clone(),
        thread_id: entry.thread_id.clone(),
        related_tasks: entry.related_task_ids.len(),
        response_owner_id: entry.response_owner_id.clone(),
        response_owner_party_id: entry.response_owner_party_id.clone(),
        party_ids: entry.external_party_ids.clone(),
        sensitivity: entry.sensitivity.clone(),
    }
}

#[derive(Debug, Clone, Default)]
pub struct RecordListFilter {
    pub query: String,
    pub category: Option<String>,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordListReply {
    pub project: String,
    #[serde(flatten)]
    pub bounds: PageBounds,
    /// True when the server cut the context page this list was folded from;
    /// the total then counts what was fetched, not what the project holds.
    pub truncated: bool,
    pub records: Vec<RecordRow>,
}

/// `filterProjectRecords` (views.ts:366-376): the search-box match, newest
/// first, ties by id.
fn filtered_records<'a>(records: &'a [ContextRecord], query: &str) -> Vec<&'a ContextRecord> {
    let needle = query.trim().to_lowercase();
    let mut matched: Vec<&ContextRecord> = records
        .iter()
        .filter(|entry| {
            if needle.is_empty() {
                return true;
            }
            let mut haystack: Vec<&str> = vec![
                &entry.subject,
                &entry.body,
                &entry.category,
                &entry.direction,
            ];
            haystack.extend(entry.source_conversation_id.as_deref());
            haystack.extend(entry.source_message_id.as_deref());
            haystack.extend(entry.source_artifact_id.as_deref());
            haystack
                .iter()
                .any(|value| value.to_lowercase().contains(&needle))
        })
        .collect();
    matched.sort_by(|left, right| {
        right
            .happened_at
            .cmp(&left.happened_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    matched
}

/// `listCliWorkRecords` (cli-pm.ts:733-747) less the session.
pub fn record_list(
    project: &str,
    records: &[ContextRecord],
    truncated: bool,
    filter: &RecordListFilter,
) -> RecordListReply {
    let matched: Vec<RecordRow> = filtered_records(records, &filter.query)
        .into_iter()
        .filter(|entry| {
            filter
                .category
                .as_ref()
                .is_none_or(|category| entry.category == *category)
        })
        .map(record_row)
        .collect();
    let (rows, bounds) = page(&matched, filter.page, filter.page_size);
    RecordListReply {
        project: project.to_owned(),
        bounds,
        truncated,
        records: rows,
    }
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordAffects {
    pub scope: bool,
    pub schedule: bool,
    pub quality: bool,
    pub cost: bool,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordDetail {
    #[serde(flatten)]
    pub row: RecordRow,
    pub body: String,
    pub body_truncated: bool,
    pub affects: RecordAffects,
    pub related_task_ids: Vec<String>,
    pub related_task_total: usize,
    pub related_tasks_truncated: bool,
    pub related_residual_item_ids: Vec<String>,
    pub related_residual_item_total: usize,
    pub related_residual_items_truncated: bool,
    pub related_record_ids: Vec<String>,
    pub related_record_total: usize,
    pub related_records_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_state: Option<String>,
    pub version: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_asset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meeting_note_asset_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub document_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub external_participant_names: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resulting_task_ids: Vec<String>,
    pub waived: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waived_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responded_by_record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responded_at: Option<String>,
    pub days_overdue: i64,
    pub days_until_due: i64,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordReadReply {
    pub project: String,
    pub record: RecordDetail,
}

/// `readCliWorkRecord` (cli-pm.ts:749-786) less the session. `None` when no
/// fetched record carries that id.
pub fn record_read(
    project: &str,
    records: &[ContextRecord],
    record_id: &str,
) -> Option<RecordReadReply> {
    let entry = records.iter().find(|entry| entry.id == record_id)?;
    let (body, body_truncated) = bounded_text(&entry.body, MAX_RECORD_BODY_CHARS);
    let bounded = |ids: &[String]| -> (Vec<String>, usize, bool) {
        (
            ids.iter().take(MAX_RELATED_ROWS).cloned().collect(),
            ids.len(),
            ids.len() > MAX_RELATED_ROWS,
        )
    };
    let (related_task_ids, related_task_total, related_tasks_truncated) =
        bounded(&entry.related_task_ids);
    let (related_residual_item_ids, related_residual_item_total, related_residual_items_truncated) =
        bounded(&entry.related_residual_item_ids);
    let (related_record_ids, related_record_total, related_records_truncated) =
        bounded(&entry.related_record_ids);
    Some(RecordReadReply {
        project: project.to_owned(),
        record: RecordDetail {
            row: record_row(entry),
            body,
            body_truncated,
            affects: RecordAffects {
                scope: entry.affects_scope,
                schedule: entry.affects_schedule,
                quality: entry.affects_quality,
                cost: entry.affects_cost,
            },
            related_task_ids,
            related_task_total,
            related_tasks_truncated,
            related_residual_item_ids,
            related_residual_item_total,
            related_residual_items_truncated,
            related_record_ids,
            related_record_total,
            related_records_truncated,
            reply_to_record_id: entry.reply_to_record_id.clone(),
            reference_number: entry.reference_number.clone(),
            revision_label: entry.revision_label.clone(),
            document_state: entry.document_state.clone(),
            version: entry.version,
            source_asset_id: entry.source_asset_id.clone(),
            meeting_note_asset_id: entry.meeting_note_asset_id.clone(),
            document_ids: entry.document_ids.clone(),
            external_participant_names: entry.external_participant_names.clone(),
            resulting_task_ids: entry.resulting_task_ids.clone(),
            waived: entry.waived,
            waived_reason: entry.waived_reason.clone(),
            responded_by_record_id: entry.responded_by_record_id.clone(),
            responded_at: entry.responded_at.clone(),
            days_overdue: entry.days_overdue,
            days_until_due: entry.days_until_due,
        },
    })
}

/// The detail of one record from its own entry: what `record_read` answers
/// for a fetched context, reusable by the correspondence views that carry
/// the record beside its attachments.
pub fn record_detail(entry: &ContextRecord) -> RecordDetail {
    record_read("", std::slice::from_ref(entry), &entry.id)
        .expect("the entry names itself")
        .record
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_management::wire::decode_project_graph;
    use serde_json::json;

    fn task(id: &str, parent: Option<&str>, sort_key: &str, extra: Value) -> Value {
        let mut data = json!({
            "title": format!("Task {id}"), "sort_key": sort_key, "parent_task_id": parent,
            "delivery_state": "in_progress", "progress": 0.25,
            "responsible_email": "lead@ds.rw", "collaborator_emails": ["field@ds.rw"],
        });
        if let Value::Object(more) = extra {
            for (key, value) in more {
                data[key] = value;
            }
        }
        json!({"id": id, "data": data, "metadata": {"updated_at": 5}})
    }

    fn graph() -> CanonicalProjectGraph {
        decode_project_graph(
            &json!({
                "project_id": "p", "revision": 9,
                "permissions": {"view": true, "edit_schedule": true},
                "tasks": [
                    task("b", None, "0200", json!({})),
                    task("a", None, "0100", json!({"discipline": "Design"})),
                    task("a1", Some("a"), "0100", json!({"proposed_assignee_emails": ["pilot@ds.rw"], "placement": "inbox"})),
                    task("a2", Some("a"), "0200", json!({"description": "x".repeat(5000), "delivery_state": "blocked"})),
                ],
                "dependencies": [{"id": "d1", "predecessor_id": "a1", "successor_id": "a2", "type": "FS", "lag_minutes": 480}],
                "residual_items": [
                    {"id": "r1", "task_id": "a2", "title": "closed one", "status": "resolved"},
                    {"id": "r2", "task_id": "a2", "title": "open one", "status": "open", "blocks_acceptance": true},
                ],
            }),
            "p",
        )
    }

    #[test]
    fn the_list_is_in_plan_order_with_wbs_labels_and_pages_honestly() {
        let reply = task_list(
            &graph(),
            &TaskListFilter {
                page_size: 2,
                ..TaskListFilter::default()
            },
        );
        assert_eq!(reply.bounds.total, 4);
        assert_eq!(
            (reply.bounds.page_count, reply.bounds.from, reply.bounds.to),
            (2, 1, 2)
        );
        let labels: Vec<(&str, &str)> = reply
            .tasks
            .iter()
            .map(|row| (row.wbs.as_str(), row.id.as_str()))
            .collect();
        assert_eq!(labels, vec![("1", "a"), ("1.1", "a1")]);
        assert!(reply.tasks[1].assignment_open);
        assert_eq!(reply.tasks[0].progress, 25);

        let second = task_list(
            &graph(),
            &TaskListFilter {
                page_size: 2,
                page: 1,
                ..TaskListFilter::default()
            },
        );
        let ids: Vec<&str> = second.tasks.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, vec!["a2", "b"]);
        assert_eq!(second.bounds.from, 3);
    }

    #[test]
    fn every_filter_narrows_the_same_rows_the_table_would() {
        let g = graph();
        let ids = |filter: TaskListFilter| -> Vec<String> {
            task_list(&g, &filter)
                .tasks
                .into_iter()
                .map(|row| row.id)
                .collect()
        };
        assert_eq!(
            ids(TaskListFilter {
                query: "TASK A2".into(),
                ..Default::default()
            }),
            vec!["a2"]
        );
        assert_eq!(
            ids(TaskListFilter {
                state: Some("blocked".into()),
                ..Default::default()
            }),
            vec!["a2"]
        );
        assert_eq!(
            ids(TaskListFilter {
                discipline: Some("Design".into()),
                ..Default::default()
            }),
            vec!["a"]
        );
        assert_eq!(
            ids(TaskListFilter {
                assignee: Some("PILOT@ds.rw".into()),
                ..Default::default()
            }),
            vec!["a1"]
        );
        assert_eq!(
            ids(TaskListFilter {
                placement: PlacementFilter::Inbox,
                ..Default::default()
            }),
            vec!["a1"]
        );
        // A WBS label is searchable too, as the Table's box searches it.
        assert_eq!(
            ids(TaskListFilter {
                query: "1.2".into(),
                ..Default::default()
            }),
            vec!["a2"]
        );
    }

    #[test]
    fn one_task_carries_its_edges_residuals_open_first_and_a_bounded_description() {
        let records = vec![ContextRecord {
            related_task_ids: vec!["a2".into()],
            ..sample_record("R1", "2026-09-20T10:00:00Z")
        }];
        let reply = task_read(&graph(), "a2", &records).expect("a2 exists");
        assert_eq!(reply.task.row.wbs, "1.2");
        assert!(reply.task.description_truncated);
        assert_eq!(reply.task.description.len(), MAX_TEXT_LENGTH);
        assert_eq!(reply.dependency_total, 1);
        assert_eq!(reply.dependencies[0].from, "a1");
        let statuses: Vec<&str> = reply
            .residuals
            .iter()
            .map(|row| row.status.as_str())
            .collect();
        assert_eq!(statuses, vec!["open", "resolved"]);
        assert_eq!(reply.record_total, 1);
        assert_eq!(reply.records[0].id, "R1");
        assert_eq!(reply.link, "/projects/p/work?view=gantt&task=a2");
        assert!(task_read(&graph(), "nope", &[]).is_none());
    }

    fn sample_record(id: &str, happened_at: &str) -> ContextRecord {
        ContextRecord {
            id: id.into(),
            category: "review".into(),
            state: "open".into(),
            direction: "inbound".into(),
            channel: Some("email".into()),
            subject: format!("Subject {id}"),
            body: "b".repeat(9_000),
            happened_at: happened_at.into(),
            response_required: true,
            response_due_date: Some("2026-09-25".into()),
            response_status: Some("outstanding".into()),
            thread_id: Some(id.into()),
            reply_to_record_id: None,
            reference_number: None,
            affects_scope: true,
            affects_schedule: false,
            affects_quality: false,
            affects_cost: true,
            related_task_ids: vec![],
            related_residual_item_ids: vec![],
            related_record_ids: vec![],
            revision_label: None,
            document_state: None,
            source_conversation_id: None,
            source_message_id: None,
            source_artifact_id: None,
            version: 3,
            response_owner_id: None,
            response_owner_party_id: None,
            external_party_ids: vec![],
            external_participant_names: vec![],
            source_asset_id: None,
            meeting_note_asset_id: None,
            document_ids: vec![],
            resulting_task_ids: vec![],
            waived: false,
            waived_reason: None,
            sensitivity: "open".into(),
            responded_by_record_id: None,
            responded_at: None,
            days_overdue: 0,
            days_until_due: 0,
        }
    }

    #[test]
    fn records_decode_from_the_context_envelope_and_list_newest_first() {
        let context = json!({"records": [
            {"id": "R1", "version": 2, "is_deleted": false,
             "data": {"category": "review", "state": "open", "direction": "inbound", "subject": "First",
                      "happened_at": "2026-09-14T09:00:00Z", "response_required": true,
                      "related_task_ids": ["t1", "t2"], "thread_id": "R1"},
             "computed": {"response_status": "overdue"}},
            {"id": "R2", "version": 1, "is_deleted": false,
             "data": {"category": "instruction", "direction": "inbound", "subject": "Second",
                      "happened_at": 1789689600000i64}},
            {"id": "gone", "is_deleted": true, "data": {"subject": "deleted"}},
            {"data": {"subject": "no id"}},
        ]});
        let records = decode_context_records(&context);
        assert_eq!(records.len(), 2);
        let reply = record_list(
            "p",
            &records,
            false,
            &RecordListFilter {
                page_size: 50,
                ..Default::default()
            },
        );
        let ids: Vec<&str> = reply.records.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, vec!["R2", "R1"], "newest first");
        assert_eq!(reply.records[0].happened_at, "2026-09-18T00:00:00.000Z");
        assert_eq!(reply.records[1].related_tasks, 2);
        assert_eq!(reply.records[1].response_status.as_deref(), Some("overdue"));

        let only = record_list(
            "p",
            &records,
            false,
            &RecordListFilter {
                category: Some("instruction".into()),
                page_size: 50,
                ..Default::default()
            },
        );
        assert_eq!(only.bounds.total, 1);
        let searched = record_list(
            "p",
            &records,
            false,
            &RecordListFilter {
                query: "first".into(),
                page_size: 50,
                ..Default::default()
            },
        );
        assert_eq!(searched.records[0].id, "R1");
    }

    #[test]
    fn one_record_carries_a_bounded_body_and_its_relations() {
        let records = vec![sample_record("R1", "2026-09-20T10:00:00Z")];
        let reply = record_read("p", &records, "R1").expect("R1 exists");
        assert!(reply.record.body_truncated);
        assert_eq!(reply.record.body.len(), MAX_RECORD_BODY_CHARS);
        assert!(reply.record.affects.scope && reply.record.affects.cost);
        assert!(!reply.record.affects.schedule);
        assert_eq!(reply.record.version, 3);
        assert!(record_read("p", &records, "R9").is_none());
    }

    #[test]
    fn the_task_route_is_the_deep_link_the_app_opens() {
        assert_eq!(
            task_route("arjgpydw_nyam", "nyam shaker/t1"),
            "/projects/arjgpydw_nyam/work?view=gantt&task=nyam+shaker%2Ft1"
        );
    }
}
