//! The canonical Project Work graph, read off the wire.
//!
//! A faithful port of `ds-web/src/lib/project-work/wire.ts` (`decodeProjectGraph`)
//! and the domain types of `domain.ts`. Every default the browser invented is
//! carried over with the line it came from, because the kernel is the authority
//! now and a silently different default would be a silently different plan.
//!
//! Two members of `CanonicalProjectTask` are deliberately NOT ported here:
//! `geometry` and `links`. They are decoded by `$lib/work-context/geometry` and
//! `$lib/work-context/link` — a different module with its own contract — and no
//! projection in this crate reads either. Porting half of them would be worse
//! than porting neither.
//!
//! `decodeProjectGraph` also returns `workEpisodes`, `records`, `attachments`
//! and `intelligenceArtifacts` as empty lists unconditionally (wire.ts:35-39):
//! the canonical graph payload does not carry them, they arrive from their own
//! bounded collections. The ported graph therefore has no such members at all
//! rather than four members that are always empty.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::design_dashboard::day_key;

pub(crate) const DAY_MS: i64 = 86_400_000;

// ── the vocabulary ──────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    #[default]
    Task,
    Milestone,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskPlacement {
    #[default]
    Wbs,
    Inbox,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskScheduleState {
    Full,
    StartOnly,
    FinishOnly,
    #[default]
    Unscheduled,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    #[default]
    NotStarted,
    InProgress,
    Paused,
    Blocked,
    SubstantiallyComplete,
    Complete,
    Cancelled,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    #[default]
    NotRequired,
    NotSubmitted,
    Submitted,
    UnderReview,
    ChangesRequested,
    Accepted,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseoutState {
    #[default]
    Open,
    ResidualOpen,
    Closed,
    Waived,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SchedulingMode {
    #[default]
    Manual,
    Auto,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DependencyType {
    #[default]
    FS,
    SS,
    FF,
    SF,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentAction {
    #[default]
    Assigned,
    Reassigned,
    Requested,
    Withdrawn,
    Accepted,
    Declined,
    Revoked,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResidualKind {
    Clarification,
    MinorCorrection,
    ReviewResponse,
    MissingInformation,
    SignatureOrApproval,
    CloseoutDocument,
    Monitoring,
    ClientResponse,
    InternalFollowUp,
    #[default]
    Other,
    Blocker,
    /// Legacy only: the backend never declared these two and the UI no longer
    /// offers them. They stay readable so rows written before the scope
    /// classification took over still decode as themselves (wire.ts:370-373).
    PotentialAdditionalScope,
    ApprovedAdditionalScope,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResidualStatus {
    #[default]
    Open,
    InProgress,
    AwaitingResponse,
    Resolved,
    Waived,
    Promoted,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScopeClassification {
    WithinOriginalScope,
    PotentialChange,
    ApprovedChange,
    #[default]
    NotApplicable,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResidualWorkClassification {
    BoundedResidual,
    #[default]
    ScopeReviewRequired,
    MaterialRework,
    ControlsSchedule,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubjectStateKind {
    #[default]
    Current,
    Changed,
    Available,
    Missing,
    Unsupported,
}

// ── the graph ───────────────────────────────────────────────────────────

/// Sunday through Saturday, matching JavaScript/Go weekday indexes.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCalendar {
    pub working_weekdays: Vec<bool>,
    pub holidays: Vec<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalProjectTask {
    pub id: String,
    /// The field names the schedule accepts a write for, straight off the
    /// document's own `data` map. Never derived here: a client that decided
    /// this for itself would be re-implementing the server's own rule.
    pub authored_fields: Vec<String>,
    pub discipline: String,
    pub proposed_assignee_emails: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposed_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_requested_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_requested_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_accepted_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_accepted_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_last_action: Option<AssignmentAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_last_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_last_by: Option<String>,
    pub parent_task_id: Option<String>,
    pub sort_key: String,
    pub title: String,
    pub description: String,
    #[serde(rename = "type")]
    pub task_type: TaskType,
    pub placement: TaskPlacement,
    pub schedule_state: TaskScheduleState,
    pub delivery_state: DeliveryState,
    pub review_state: ReviewState,
    pub closeout_state: CloseoutState,
    pub progress: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    pub start_date: String,
    pub finish_date: String,
    pub commitment_start_date: String,
    pub commitment_finish_date: String,
    pub duration_minutes: i64,
    pub scheduling_mode: SchedulingMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsible_email: Option<String>,
    pub collaborator_emails: Vec<String>,
    pub assignee_emails: Vec<String>,
    pub priority: Priority,
    pub open_blocker_count: i64,
    pub descendant_blocker_count: i64,
    pub unscheduled_descendant_count: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<TaskBlocker>,
    pub awaiting_correspondence_count: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_record_ids: Vec<String>,
    /// Task proposals (ds-brain docs/contracts/task-proposals.md). The
    /// estimate is the proposer's statement, absent when never stated; the
    /// log is what assignees declared; `actual_hours` is the server's sum;
    /// `admission` is where the proposal stands with the PM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_hours: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_days: Option<i64>,
    pub actual_hours: f64,
    pub hours_log: Vec<HoursEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admission: Option<Admission>,
    pub version: i64,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
}

/// One appended statement of hours worked, as the server stored it.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HoursEntry {
    pub id: String,
    pub by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    pub hours: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionState {
    Requested,
    Admitted,
    Declined,
}

/// Where a proposed task stands with the PM; absent on a task nobody asked
/// about.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Admission {
    pub state: AdmissionState,
    pub requested_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskDependency {
    pub id: String,
    pub from_task_id: String,
    pub to_task_id: String,
    #[serde(rename = "type")]
    pub dependency_type: DependencyType,
    pub lag_minutes: i64,
    pub version: i64,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskResidualItem {
    pub id: String,
    pub task_id: String,
    pub title: String,
    pub description: String,
    pub kind: ResidualKind,
    pub status: ResidualStatus,
    pub scope_classification: ScopeClassification,
    pub work_classification: ResidualWorkClassification,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsible_email: Option<String>,
    pub collaborator_emails: Vec<String>,
    /// Compatibility alias for pre-canonical residual projections, set to the
    /// responsible email whenever there is one (wire.ts:267).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsible_user_id: Option<String>,
    pub assignee_emails: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    pub blocks_acceptance: bool,
    pub blocks_closeout: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_record_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promoted_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<String>,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSubjectState {
    pub object_type: String,
    pub object_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_revision: Option<String>,
    pub state: SubjectStateKind,
    pub checked_at: String,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSubjectWork {
    pub object_type: String,
    pub object_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub task_ids: Vec<String>,
    pub active_count: i64,
    pub closed_count: i64,
    pub blocked_count: i64,
    pub awaiting_review_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_updated_at: Option<i64>,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGraphPermissions {
    pub view: bool,
    pub comment: bool,
    pub contribute: bool,
    pub edit_schedule: bool,
    pub admin: bool,
}

/// The vocabulary of a task, as stated by the engine that enforces it. Nothing
/// is defaulted: a missing list means the graph predates the field model, and
/// the caller falls back explicitly rather than this inventing a vocabulary of
/// its own (wire.ts:77-79).
#[derive(Serialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFieldModel {
    pub delivery_states: Vec<String>,
    pub review_states: Vec<String>,
    pub closeout_states: Vec<String>,
    pub priorities: Vec<String>,
    pub task_types: Vec<String>,
    pub scheduling_modes: Vec<String>,
    pub placements: Vec<String>,
    pub dependency_types: Vec<String>,
    pub residual_kinds: Vec<String>,
    pub residual_statuses: Vec<String>,
    pub scope_classifications: Vec<String>,
    pub work_classifications: Vec<String>,
    /// Task proposals: the admission vocabulary and the four bounds, so a
    /// client refuses an out-of-range number before the round trip.
    pub admission_states: Vec<String>,
    pub max_estimated_hours: i64,
    pub max_estimated_days: i64,
    pub max_hours_per_entry: i64,
    pub max_hours_log_entries: i64,
    pub max_assignees: i64,
    pub max_batch_commands: i64,
    /// The correspondence vocabularies (ds-brain `docs/contracts/correspondence.md`).
    /// Empty on a graph from a server that predates them: absent is read as
    /// empty here and republished as absent by the plan, never as a list
    /// claiming the engine accepts nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub record_categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub record_states: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub record_directions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_statuses: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocker_kinds: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocker_cleared_bys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub party_kinds: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub party_roles: Vec<String>,
}

/// One correspondence blocker on a task (`data.blockers[]`): the record the
/// task waits on, who set it, and — once settled — how it cleared.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskBlocker {
    pub kind: String,
    pub record_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleared_at: Option<String>,
    /// `response`, `waiver` or `manual`, from the engine's vocabulary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleared_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clear_reason: Option<String>,
}

impl TaskBlocker {
    /// A blocker still holds the task while nothing has cleared it.
    pub fn is_open(&self) -> bool {
        self.cleared_at.is_none() && self.cleared_by.is_none()
    }
}

/// One awaiting record as `get_graph.correspondence.records[]` carries it.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrespondenceRecord {
    pub record_id: String,
    pub thread_id: String,
    pub subject: String,
    pub category: String,
    pub channel: String,
    pub direction: String,
    pub happened_at: String,
    /// `outstanding` or `overdue` — the projection lists awaiting rows only.
    pub response_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_due_date: Option<String>,
    pub days_overdue: i64,
    pub days_until_due: i64,
    pub due_within_window: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_owner_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_owner_party_id: Option<String>,
    /// The party the row is grouped under: the owing party, else the first
    /// external party when the project owes the answer.
    pub party_id: String,
    pub party_name: String,
    pub external_party_ids: Vec<String>,
    pub blocked_task_ids: Vec<String>,
    pub sensitivity: String,
}

/// One task the projection reports as blocked on correspondence.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrespondenceBlockedTask {
    pub task_id: String,
    pub title: String,
    pub record_ids: Vec<String>,
    pub party_ids: Vec<String>,
}

/// The three dashboard counters the contract names; readable rows only.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CorrespondenceCounters {
    pub records_outstanding: i64,
    pub records_overdue: i64,
    pub tasks_awaiting_correspondence: i64,
}

/// `get_graph.correspondence`: the awaiting records and the tasks they hold,
/// redacted by the caller's class authority, not part of the graph revision.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Correspondence {
    pub as_of: String,
    pub due_window_days: i64,
    pub records: Vec<CorrespondenceRecord>,
    pub records_total: i64,
    pub truncated: bool,
    pub blocked_tasks: Vec<CorrespondenceBlockedTask>,
    pub counters: CorrespondenceCounters,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalProjectGraph {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_model: Option<ProjectFieldModel>,
    pub project_id: String,
    pub revision: i64,
    pub schedule_revision: i64,
    pub calendar: ProjectCalendar,
    pub tasks: Vec<CanonicalProjectTask>,
    pub dependencies: Vec<TaskDependency>,
    pub residual_items: Vec<TaskResidualItem>,
    pub subject_states: Vec<ProjectSubjectState>,
    pub subject_work: Vec<ProjectSubjectWork>,
    pub permissions: ProjectGraphPermissions,
    /// Present only when the server published the projection. Absent on a
    /// graph from before the correspondence contract, which is not the same
    /// answer as "nothing is awaited".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correspondence: Option<Correspondence>,
}

// ── the readers ─────────────────────────────────────────────────────────

/// `record(value)` — an object that is not an array, else the empty object.
fn record(value: Option<&Value>) -> &Map<String, Value> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    match value {
        Some(Value::Object(map)) => map,
        _ => EMPTY.get_or_init(Map::new),
    }
}

/// `records(value)` — every array member read as an object, empty ones dropped.
fn records(value: Option<&Value>) -> Vec<&Map<String, Value>> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| record(Some(item)))
            .filter(|item| !item.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// `text(value)` — a trimmed string, else empty.
fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(raw)) => raw.trim().to_string(),
        _ => String::new(),
    }
}

fn non_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// `texts(value)` — array members read as text, empty ones dropped.
fn texts(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| text(Some(item)))
            .filter(|item| !item.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// `a ?? b ?? c` — the first member that is neither absent nor null. A present
/// empty string wins over the next key, exactly as the browser's `??` does.
fn coalesce<'a>(raw: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|key| raw.get(*key).filter(|value| !value.is_null()))
}

/// `integer(value)` — a finite number truncated toward zero, else 0.
fn integer(value: Option<&Value>) -> i64 {
    optional_integer(value).unwrap_or(0)
}

/// `optionalInteger(value)` — a finite number truncated toward zero, else null.
fn optional_integer(value: Option<&Value>) -> Option<i64> {
    let number = match value {
        Some(Value::Number(number)) => number,
        _ => return None,
    };
    if let Some(exact) = number.as_i64() {
        return Some(exact);
    }
    number
        .as_f64()
        .filter(|candidate| candidate.is_finite())
        .map(|candidate| candidate.trunc() as i64)
}

/// `finite(value, fallback)` — the number itself, undivided and unclamped.
fn finite(value: Option<&Value>, fallback: f64) -> f64 {
    match value {
        Some(Value::Number(number)) => number
            .as_f64()
            .filter(|candidate| candidate.is_finite())
            .unwrap_or(fallback),
        _ => fallback,
    }
}

/// `scheduleDate(value)` — the first ten characters of the trimmed text when
/// they read `YYYY-MM-DD`, else empty. Calendar validity is NOT checked here,
/// because the browser did not check it either: the projections compare these
/// as strings and only `dayspan` ever parses one.
pub(crate) fn schedule_date(value: Option<&Value>) -> String {
    let candidate: String = text(value).chars().take(10).collect();
    let bytes = candidate.as_bytes();
    let shaped = bytes.len() == 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit);
    if shaped {
        candidate
    } else {
        String::new()
    }
}

/// JavaScript's `\s`: everything Rust calls whitespace, plus the BOM, which
/// `String::trim` leaves alone but the email pattern rejects.
fn js_space(character: char) -> bool {
    character.is_whitespace() || character == '\u{feff}'
}

/// `normalizedEmail(value)` — `/^[^\s@]+@[^\s@]+\.[^\s@]+$/` over the
/// lower-cased trimmed text, else empty. The pattern is greedy, so the dot it
/// anchors on is the LAST one in the domain: a domain may not begin or end
/// with a dot, and must carry at least one.
fn normalized_email(value: Option<&Value>) -> String {
    let candidate = text(value).to_lowercase();
    let mut parts = candidate.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return String::new();
    };
    let valid = !local.is_empty()
        && !local.contains(js_space)
        && !domain.contains(js_space)
        && domain
            .rfind('.')
            .is_some_and(|dot| dot > 0 && dot + 1 < domain.len());
    if valid {
        candidate
    } else {
        String::new()
    }
}

/// `normalizedEmails(value)` — valid addresses in wire order, first win.
fn normalized_emails(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    let mut emails: Vec<String> = Vec::new();
    for item in items {
        let email = normalized_email(Some(item));
        if email.is_empty() || emails.contains(&email) {
            continue;
        }
        emails.push(email);
    }
    emails
}

fn metadata_updated_at(raw: &Map<String, Value>) -> i64 {
    integer(record(raw.get("metadata")).get("updated_at"))
}

fn metadata_created_at(raw: &Map<String, Value>) -> i64 {
    integer(record(raw.get("metadata")).get("created_at"))
}

/// `new Date(ms).toISOString()`.
///
/// The browser THROWS for a stamp outside the representable `Date` range and
/// switches to the expanded `±YYYYYY` year form outside years 0000–9999. The
/// kernel cannot throw out of a projection, so it omits the field instead —
/// the one place this port answers differently from the browser, and it is
/// reachable only from a wire stamp that is already nonsense.
fn iso_from_ms(ms: i64) -> Option<String> {
    let day = day_key(ms, 0);
    if day.len() != 10 || !day.as_bytes()[..4].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let within = ms.rem_euclid(DAY_MS);
    let (hours, minutes) = (within / 3_600_000, (within / 60_000) % 60);
    let (seconds, millis) = ((within / 1_000) % 60, within % 1_000);
    Some(format!(
        "{day}T{hours:02}:{minutes:02}:{seconds:02}.{millis:03}Z"
    ))
}

/// `optionalInstant(value, key)` — a finite number becomes its ISO string, a
/// string is taken as it stands, anything else is absent.
pub(crate) fn optional_instant(value: Option<&Value>) -> Option<String> {
    if let Some(Value::Number(_)) = value {
        return optional_integer(value).and_then(iso_from_ms);
    }
    non_empty(text(value))
}

// ── the graph reader ────────────────────────────────────────────────────

/// `decodeProjectGraph(value, projectId)`. `fallback_project` is the second
/// argument the browser passes: the session's selected project, used only when
/// the payload names none of its own.
pub fn decode_project_graph(value: &Value, fallback_project: &str) -> CanonicalProjectGraph {
    decode_graph(value, fallback_project, false)
}

/// Canonical drafts can temporarily have an empty title while being edited;
/// keep their task positions. The write planners still require a valid title.
pub(super) fn decode_canonical_graph(
    value: &Value,
    fallback_project: &str,
) -> CanonicalProjectGraph {
    decode_graph(value, fallback_project, true)
}

fn decode_graph(value: &Value, fallback_project: &str, draft: bool) -> CanonicalProjectGraph {
    let raw = record(Some(value));
    CanonicalProjectGraph {
        field_model: decode_field_model(raw.get("field_model")),
        project_id: non_empty(text(raw.get("project_id")))
            .unwrap_or_else(|| fallback_project.to_string()),
        revision: integer(coalesce(raw, &["revision", "graph_revision"])),
        schedule_revision: integer(raw.get("schedule_revision")),
        calendar: decode_project_calendar(raw.get("calendar")),
        tasks: records(raw.get("tasks"))
            .into_iter()
            .filter_map(|task| decode_task_fields(task, draft))
            .collect(),
        dependencies: records(raw.get("dependencies"))
            .into_iter()
            .filter_map(decode_dependency)
            .collect(),
        residual_items: records(coalesce(raw, &["residual_items", "residuals"]))
            .into_iter()
            .filter_map(decode_residual_item)
            .collect(),
        subject_states: records(raw.get("subject_states"))
            .into_iter()
            .filter_map(decode_subject_state)
            .collect(),
        subject_work: records(raw.get("subject_work"))
            .into_iter()
            .filter_map(decode_subject_work)
            .collect(),
        permissions: decode_permissions(raw.get("permissions")),
        correspondence: decode_correspondence(raw.get("correspondence")),
    }
}

/// `get_graph.correspondence`, when the server published one. A row without
/// a record id is dropped rather than defaulted: it could not be acted on.
fn decode_correspondence(value: Option<&Value>) -> Option<Correspondence> {
    let raw = record(value);
    if raw.is_empty() {
        return None;
    }
    let counters = record(raw.get("counters"));
    Some(Correspondence {
        as_of: schedule_date(raw.get("as_of")),
        due_window_days: integer(raw.get("due_window_days")).max(0),
        records: records(raw.get("records"))
            .into_iter()
            .filter_map(decode_correspondence_record)
            .collect(),
        records_total: integer(raw.get("records_total")).max(0),
        truncated: raw.get("truncated") == Some(&Value::Bool(true)),
        blocked_tasks: records(raw.get("blocked_tasks"))
            .into_iter()
            .filter_map(|row| {
                Some(CorrespondenceBlockedTask {
                    task_id: non_empty(text(row.get("task_id")))?,
                    title: text(row.get("title")),
                    record_ids: texts(row.get("record_ids")),
                    party_ids: texts(row.get("party_ids")),
                })
            })
            .collect(),
        counters: CorrespondenceCounters {
            records_outstanding: integer(counters.get("recordsOutstanding")).max(0),
            records_overdue: integer(counters.get("recordsOverdue")).max(0),
            tasks_awaiting_correspondence: integer(counters.get("tasksAwaitingCorrespondence"))
                .max(0),
        },
    })
}

fn decode_correspondence_record(raw: &Map<String, Value>) -> Option<CorrespondenceRecord> {
    let record_id = non_empty(text(raw.get("record_id")))?;
    Some(CorrespondenceRecord {
        thread_id: non_empty(text(raw.get("thread_id"))).unwrap_or_else(|| record_id.clone()),
        record_id,
        subject: text(raw.get("subject")),
        category: text(raw.get("category")),
        channel: text(raw.get("channel")),
        direction: text(raw.get("direction")),
        happened_at: text(raw.get("happened_at")),
        response_status: text(raw.get("response_status")),
        response_due_date: non_empty(schedule_date(raw.get("response_due_date"))),
        days_overdue: integer(raw.get("days_overdue")).max(0),
        days_until_due: integer(raw.get("days_until_due")).max(0),
        due_within_window: raw.get("due_within_window") == Some(&Value::Bool(true)),
        response_owner_id: non_empty(text(raw.get("response_owner_id"))),
        response_owner_party_id: non_empty(text(raw.get("response_owner_party_id"))),
        party_id: text(raw.get("party_id")),
        party_name: text(raw.get("party_name")),
        external_party_ids: texts(raw.get("external_party_ids")),
        blocked_task_ids: texts(raw.get("blocked_task_ids")),
        sensitivity: non_empty(text(raw.get("sensitivity"))).unwrap_or_else(|| "open".into()),
    })
}

/// `data.blockers[]` on a task: rows without a record id are dropped.
fn decode_blockers(value: Option<&Value>) -> Vec<TaskBlocker> {
    records(value)
        .into_iter()
        .filter_map(|row| {
            Some(TaskBlocker {
                kind: non_empty(text(row.get("kind")))
                    .unwrap_or_else(|| "awaiting_correspondence".into()),
                record_id: non_empty(text(row.get("record_id")))?,
                set_by: non_empty(text(row.get("set_by"))),
                set_at: optional_instant(row.get("set_at")),
                cleared_at: optional_instant(row.get("cleared_at")),
                cleared_by: non_empty(text(row.get("cleared_by"))),
                clear_reason: non_empty(text(row.get("clear_reason"))),
            })
        })
        .collect()
}

/// Present only when the backend declared a non-empty `field_model`
/// (wire.ts:71-73). Absent means the graph predates it.
fn decode_field_model(value: Option<&Value>) -> Option<ProjectFieldModel> {
    let raw = record(value);
    if raw.is_empty() {
        return None;
    }
    Some(ProjectFieldModel {
        delivery_states: texts(raw.get("delivery_states")),
        review_states: texts(raw.get("review_states")),
        closeout_states: texts(raw.get("closeout_states")),
        priorities: texts(raw.get("priorities")),
        task_types: texts(raw.get("task_types")),
        scheduling_modes: texts(raw.get("scheduling_modes")),
        placements: texts(raw.get("placements")),
        dependency_types: texts(raw.get("dependency_types")),
        residual_kinds: texts(raw.get("residual_kinds")),
        residual_statuses: texts(raw.get("residual_statuses")),
        scope_classifications: texts(raw.get("scope_classifications")),
        work_classifications: texts(raw.get("work_classifications")),
        admission_states: texts(raw.get("admission_states")),
        max_estimated_hours: integer(raw.get("max_estimated_hours")),
        max_estimated_days: integer(raw.get("max_estimated_days")),
        max_hours_per_entry: integer(raw.get("max_hours_per_entry")),
        max_hours_log_entries: integer(raw.get("max_hours_log_entries")),
        max_assignees: integer(raw.get("max_assignees")),
        max_batch_commands: integer(raw.get("max_batch_commands")),
        channels: texts(raw.get("channels")),
        record_categories: texts(raw.get("record_categories")),
        record_states: texts(raw.get("record_states")),
        record_directions: texts(raw.get("record_directions")),
        response_statuses: texts(raw.get("response_statuses")),
        blocker_kinds: texts(raw.get("blocker_kinds")),
        blocker_cleared_bys: texts(raw.get("blocker_cleared_bys")),
        party_kinds: texts(raw.get("party_kinds")),
        party_roles: texts(raw.get("party_roles")),
    })
}

/// The proposal fields off the merged task map. A log is what the row
/// carries; the sum is the server's `computed.actual_hours` when it is there
/// and the log's own sum when a row predates the field — the same number
/// either way, so an older row never reads as zero hours.
fn decode_hours_log(value: Option<&Value>) -> Vec<HoursEntry> {
    records(value)
        .into_iter()
        .filter_map(|entry| {
            let id = non_empty(text(entry.get("id")))?;
            Some(HoursEntry {
                id,
                by: text(entry.get("by")),
                at: optional_instant(entry.get("at")),
                hours: finite(entry.get("hours"), 0.0).max(0.0),
                note: non_empty(text(entry.get("note"))),
            })
        })
        .collect()
}

fn decode_admission(value: Option<&Value>) -> Option<Admission> {
    let raw = record(value);
    let state = match text(raw.get("state")).as_str() {
        "requested" => AdmissionState::Requested,
        "admitted" => AdmissionState::Admitted,
        "declined" => AdmissionState::Declined,
        _ => return None,
    };
    Some(Admission {
        state,
        requested_by: text(raw.get("requested_by")),
        requested_at: optional_instant(raw.get("requested_at")),
        note: non_empty(text(raw.get("note"))),
        decided_by: non_empty(text(raw.get("decided_by"))),
        decided_at: optional_instant(raw.get("decided_at")),
        reason: non_empty(text(raw.get("reason"))),
    })
}

fn optional_finite(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(number)) => number.as_f64().filter(|candidate| candidate.is_finite()),
        _ => None,
    }
}

/// INVENTED DEFAULT (wire.ts:111): a calendar that does not author exactly
/// seven booleans gets Monday-to-Friday, Sunday first. Holidays may arrive as
/// a list or as a map whose `true` entries are the dates (wire.ts:107-109).
fn decode_project_calendar(value: Option<&Value>) -> ProjectCalendar {
    let raw = record(value);
    let authored = match raw.get("working_weekdays") {
        Some(Value::Array(items)) if items.len() == 7 => items
            .iter()
            .map(|item| item.as_bool())
            .collect::<Option<Vec<bool>>>(),
        _ => None,
    };
    let holidays = match raw.get("holidays") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| schedule_date(Some(item)))
            .collect::<Vec<String>>(),
        other => record(other)
            .iter()
            .filter(|(_, enabled)| *enabled == &Value::Bool(true))
            .map(|(date, _)| schedule_date(Some(&Value::String(date.clone()))))
            .collect(),
    };
    ProjectCalendar {
        working_weekdays: authored
            .unwrap_or_else(|| vec![false, true, true, true, true, true, false]),
        holidays: holidays
            .into_iter()
            .filter(|date| !date.is_empty())
            .collect(),
    }
}

fn decode_permissions(value: Option<&Value>) -> ProjectGraphPermissions {
    let raw = record(value);
    let flag = |key: &str| raw.get(key) == Some(&Value::Bool(true));
    ProjectGraphPermissions {
        view: flag("view"),
        comment: flag("comment"),
        contribute: flag("contribute"),
        edit_schedule: flag("edit_schedule"),
        admin: flag("admin"),
    }
}

fn decode_subject_state(raw: &Map<String, Value>) -> Option<ProjectSubjectState> {
    let object_type = non_empty(text(raw.get("object_type")))?;
    let object_id = non_empty(text(raw.get("object_id")))?;
    // An unknown state is dropped, not defaulted: a freshness verdict nobody
    // declared is not a verdict (wire.ts:44).
    let state = match text(raw.get("state")).as_str() {
        "current" => SubjectStateKind::Current,
        "changed" => SubjectStateKind::Changed,
        "available" => SubjectStateKind::Available,
        "missing" => SubjectStateKind::Missing,
        "unsupported" => SubjectStateKind::Unsupported,
        _ => return None,
    };
    Some(ProjectSubjectState {
        object_type,
        object_id,
        observed_revision: non_empty(text(raw.get("observed_revision"))),
        current_revision: non_empty(text(raw.get("current_revision"))),
        state,
        checked_at: text(raw.get("checked_at")),
    })
}

fn decode_subject_work(raw: &Map<String, Value>) -> Option<ProjectSubjectWork> {
    let object_type = non_empty(text(raw.get("object_type")))?;
    let object_id = non_empty(text(raw.get("object_id")))?;
    let latest = integer(raw.get("latest_updated_at"));
    Some(ProjectSubjectWork {
        object_type,
        object_id,
        label: non_empty(text(raw.get("label"))),
        task_ids: texts(raw.get("task_ids")),
        active_count: integer(raw.get("active_count")).max(0),
        closed_count: integer(raw.get("closed_count")).max(0),
        blocked_count: integer(raw.get("blocked_count")).max(0),
        awaiting_review_count: integer(raw.get("awaiting_review_count")).max(0),
        latest_updated_at: (latest > 0).then_some(latest),
    })
}

/// The alias table `taskAuthoredFieldName` (wire.ts:202-211): the wire's
/// snake_case name as the properties dialog knows it. A name outside the table
/// is reported as it arrived.
fn task_authored_field_name(field: &str) -> &str {
    match field {
        "parent_task_id" => "parentTaskId",
        "sort_key" => "sortKey",
        "review_state" => "reviewState",
        "closeout_state" => "closeoutState",
        "start_date" => "startDate",
        "finish_date" => "finishDate",
        "commitment_start_date" => "commitmentStartDate",
        "commitment_finish_date" => "commitmentFinishDate",
        "scheduling_mode" => "schedulingMode",
        "responsible_email" => "responsibleEmail",
        "collaborator_emails" => "collaboratorEmails",
        "proposed_assignee_emails" => "proposedAssigneeEmails",
        "proposed_by" => "proposedBy",
        "proposed_at" => "proposedAt",
        "delivery_state" => "deliveryState",
        other => other,
    }
}

/// INVENTED DEFAULT (wire.ts:331-340): `done` and `doing` are the two legacy
/// `status` values the schedule wrote before delivery states existed. Anything
/// else that is not a declared delivery state reads as `not_started`.
fn decode_delivery_state(value: Option<&Value>, legacy: Option<&Value>) -> DeliveryState {
    match text(value).as_str() {
        "not_started" => return DeliveryState::NotStarted,
        "in_progress" => return DeliveryState::InProgress,
        "paused" => return DeliveryState::Paused,
        "blocked" => return DeliveryState::Blocked,
        "substantially_complete" => return DeliveryState::SubstantiallyComplete,
        "complete" => return DeliveryState::Complete,
        "cancelled" => return DeliveryState::Cancelled,
        _ => {}
    }
    match legacy {
        Some(Value::String(raw)) if raw == "done" => DeliveryState::Complete,
        Some(Value::String(raw)) if raw == "doing" => DeliveryState::InProgress,
        _ => DeliveryState::NotStarted,
    }
}

fn decode_review_state(value: Option<&Value>) -> ReviewState {
    match text(value).as_str() {
        "not_submitted" => ReviewState::NotSubmitted,
        "submitted" => ReviewState::Submitted,
        "under_review" => ReviewState::UnderReview,
        "changes_requested" => ReviewState::ChangesRequested,
        "accepted" => ReviewState::Accepted,
        // `not_required` and anything undeclared alike (wire.ts:342-348).
        _ => ReviewState::NotRequired,
    }
}

/// INVENTED DEFAULT (wire.ts:350-354): a row that never wrote a closeout state
/// is closed when delivery is complete and open otherwise. Closeout is
/// therefore INFERRED FROM DELIVERY, which is why a complete task with open
/// residuals still reads `closed` here.
fn decode_closeout_state(value: Option<&Value>, delivery: DeliveryState) -> CloseoutState {
    match text(value).as_str() {
        "open" => CloseoutState::Open,
        "residual_open" => CloseoutState::ResidualOpen,
        "closed" => CloseoutState::Closed,
        "waived" => CloseoutState::Waived,
        _ if delivery == DeliveryState::Complete => CloseoutState::Closed,
        _ => CloseoutState::Open,
    }
}

fn decode_assignment_action(value: Option<&Value>) -> Option<AssignmentAction> {
    // An unknown stored value invents no lifecycle action (wire.ts:194-200).
    match text(value).as_str() {
        "assigned" => Some(AssignmentAction::Assigned),
        "reassigned" => Some(AssignmentAction::Reassigned),
        "requested" => Some(AssignmentAction::Requested),
        "withdrawn" => Some(AssignmentAction::Withdrawn),
        "accepted" => Some(AssignmentAction::Accepted),
        "declined" => Some(AssignmentAction::Declined),
        "revoked" => Some(AssignmentAction::Revoked),
        _ => None,
    }
}

struct Assignment {
    responsible_email: Option<String>,
    collaborator_emails: Vec<String>,
    assignee_emails: Vec<String>,
}

/// `decodeTaskAssignment` (wire.ts:213-233). The responsible holder falls back
/// to the FIRST projected assignee, and collaborators to the rest of that
/// projection — so a row that only ever carried `assignee_emails` still reads
/// as somebody's work.
fn decode_task_assignment(raw: &Map<String, Value>) -> Assignment {
    let projected = normalized_emails(raw.get("assignee_emails"));
    let explicit = normalized_email(coalesce(raw, &["responsible_email", "responsible_user_id"]));
    let responsible = non_empty(explicit).or_else(|| projected.first().cloned());
    let explicit_collaborators = match raw.get("collaborator_emails") {
        Some(Value::Array(_)) => normalized_emails(raw.get("collaborator_emails")),
        _ => projected
            .iter()
            .filter(|email| Some(*email) != responsible.as_ref())
            .cloned()
            .collect(),
    };
    let collaborators: Vec<String> = explicit_collaborators
        .into_iter()
        .filter(|email| Some(email) != responsible.as_ref())
        .collect();
    let mut all: Vec<Value> = Vec::new();
    if let Some(email) = responsible.as_ref() {
        all.push(Value::String(email.clone()));
    }
    all.extend(
        collaborators
            .iter()
            .map(|email| Value::String(email.clone())),
    );
    let assignee_emails = normalized_emails(Some(&Value::Array(all)));
    Assignment {
        responsible_email: responsible,
        collaborator_emails: collaborators,
        assignee_emails,
    }
}

/// `decodeTask(envelope)` (wire.ts:116-192).
///
/// A task arrives as three top maps. `data` is what a command authored and is
/// therefore the whole of what may be edited; `computed` is what the schedule
/// derived and is read-only everywhere. Reading them apart is what lets the
/// properties dialog decide editability from the document instead of from a
/// hand-maintained list of field names.
pub fn decode_task(envelope: &Map<String, Value>) -> Option<CanonicalProjectTask> {
    decode_task_fields(envelope, false)
}

fn decode_task_fields(envelope: &Map<String, Value>, draft: bool) -> Option<CanonicalProjectTask> {
    let mut raw = record(envelope.get("data")).clone();
    for (key, value) in record(envelope.get("computed")) {
        raw.insert(key.clone(), value.clone());
    }
    for key in ["id", "metadata", "is_deleted"] {
        match envelope.get(key) {
            Some(value) => {
                raw.insert(key.to_string(), value.clone());
            }
            None => {
                raw.remove(key);
            }
        }
    }
    let authored_fields = record(envelope.get("data"))
        .keys()
        .map(|field| task_authored_field_name(field).to_string())
        .collect();

    let id = non_empty(text(raw.get("id")))?;
    let title = text(raw.get("title"));
    if !draft && title.is_empty() {
        return None;
    }
    if raw.get("is_deleted") == Some(&Value::Bool(true)) {
        return None;
    }

    // Only the exact word is a milestone; everything else is a task.
    let task_type = if text(raw.get("type")) == "milestone" {
        TaskType::Milestone
    } else {
        TaskType::Task
    };
    let delivery_state = decode_delivery_state(raw.get("delivery_state"), raw.get("status"));
    let finish_date = schedule_date(coalesce(
        &raw,
        &["finish_date", "finish_at", "finish", "due_date"],
    ));
    let start_date = schedule_date(coalesce(&raw, &["start_date", "start_at", "start"]));
    // INVENTED DEFAULT (wire.ts:132-134): the schedule state is INFERRED FROM
    // DATE PRESENCE. Nothing on the wire declares it.
    let schedule_state = match (start_date.is_empty(), finish_date.is_empty()) {
        (false, false) => TaskScheduleState::Full,
        (false, true) => TaskScheduleState::StartOnly,
        (true, false) => TaskScheduleState::FinishOnly,
        (true, true) => TaskScheduleState::Unscheduled,
    };
    // A task with no progress of its own is 1 when it is complete and 0
    // otherwise (wire.ts:135), then clamped into 0–1 (wire.ts:169).
    let progress = finite(
        raw.get("progress"),
        if delivery_state == DeliveryState::Complete {
            1.0
        } else {
            0.0
        },
    )
    .clamp(0.0, 1.0);
    let assignment = decode_task_assignment(&raw);

    Some(CanonicalProjectTask {
        id: id.clone(),
        authored_fields,
        discipline: text(raw.get("discipline")),
        proposed_assignee_emails: match raw.get("proposed_assignee_emails") {
            // Note this list is NOT normalised as an email list: the browser
            // takes each member's trimmed text and drops the empty ones.
            Some(Value::Array(_)) => texts(raw.get("proposed_assignee_emails")),
            _ => Vec::new(),
        },
        proposed_at: optional_instant(raw.get("proposed_at")),
        proposed_by: non_empty(text(raw.get("proposed_by"))),
        assignment_requested_at: optional_instant(raw.get("assignment_requested_at")),
        assignment_requested_by: non_empty(text(raw.get("assignment_requested_by"))),
        assignment_accepted_at: optional_instant(raw.get("assignment_accepted_at")),
        assignment_accepted_by: non_empty(text(raw.get("assignment_accepted_by"))),
        assignment_last_action: decode_assignment_action(raw.get("assignment_last_action")),
        assignment_last_at: optional_instant(raw.get("assignment_last_at")),
        assignment_last_by: non_empty(text(raw.get("assignment_last_by"))),
        parent_task_id: non_empty(text(coalesce(&raw, &["parent_task_id", "parent_id"]))),
        // INVENTED DEFAULT (wire.ts:158): a row with no sort key is ordered by
        // when it was last touched, tie-broken by id — so sibling order is
        // still total on a graph that never authored one.
        sort_key: non_empty(text(raw.get("sort_key")))
            .unwrap_or_else(|| format!("{}:{id}", metadata_updated_at(&raw))),
        title,
        description: text(raw.get("description")),
        task_type,
        // Only the exact word is the inbox; everything else is the WBS.
        placement: if text(raw.get("placement")) == "inbox" {
            TaskPlacement::Inbox
        } else {
            TaskPlacement::Wbs
        },
        schedule_state,
        delivery_state,
        review_state: decode_review_state(raw.get("review_state")),
        closeout_state: decode_closeout_state(raw.get("closeout_state"), delivery_state),
        progress,
        primary_completed_at: non_empty(text(raw.get("primary_completed_at"))),
        accepted_at: non_empty(text(raw.get("accepted_at"))),
        closed_at: non_empty(text(raw.get("closed_at"))),
        start_date,
        finish_date,
        commitment_start_date: schedule_date(raw.get("commitment_start_date")),
        commitment_finish_date: schedule_date(raw.get("commitment_finish_date")),
        // INVENTED DEFAULT (wire.ts:177-179): 480 minutes — one eight-hour
        // working day — is the duration of a fully scheduled task that never
        // declared one. A milestone or a partially scheduled task has none.
        duration_minutes: if task_type == TaskType::Milestone
            || schedule_state != TaskScheduleState::Full
        {
            0
        } else {
            optional_integer(raw.get("duration_minutes"))
                .unwrap_or(480)
                .max(0)
        },
        // Only the exact word is automatic; everything else is manual.
        scheduling_mode: if text(raw.get("scheduling_mode")) == "auto" {
            SchedulingMode::Auto
        } else {
            SchedulingMode::Manual
        },
        responsible_email: assignment.responsible_email,
        collaborator_emails: assignment.collaborator_emails,
        assignee_emails: assignment.assignee_emails,
        priority: match text(raw.get("priority")).as_str() {
            "low" => Priority::Low,
            "high" => Priority::High,
            "critical" => Priority::Critical,
            // `normal` and anything undeclared alike (wire.ts:360-362).
            _ => Priority::Normal,
        },
        open_blocker_count: integer(raw.get("open_blocker_count")).max(0),
        descendant_blocker_count: integer(raw.get("descendant_blocker_count")).max(0),
        unscheduled_descendant_count: integer(raw.get("unscheduled_descendant_count")).max(0),
        blockers: decode_blockers(raw.get("blockers")),
        awaiting_correspondence_count: integer(raw.get("awaiting_correspondence_count")).max(0),
        blocked_record_ids: texts(raw.get("blocked_record_ids")),
        estimated_hours: optional_finite(raw.get("estimated_hours")).filter(|hours| *hours >= 0.0),
        estimated_days: optional_integer(raw.get("estimated_days")).filter(|days| *days >= 0),
        actual_hours: {
            let log = decode_hours_log(raw.get("hours_log"));
            optional_finite(raw.get("actual_hours"))
                .unwrap_or_else(|| log.iter().map(|entry| entry.hours).sum())
                .max(0.0)
        },
        hours_log: decode_hours_log(raw.get("hours_log")),
        admission: decode_admission(raw.get("admission")),
        version: integer(raw.get("version")).max(0),
        created_at: metadata_created_at(&raw),
        created_by: non_empty(text(record(raw.get("metadata")).get("created_by"))),
        updated_at: metadata_updated_at(&raw),
        updated_by: non_empty(text(record(raw.get("metadata")).get("updated_by"))),
    })
}

/// `decodeDependency` (wire.ts:235-248).
pub fn decode_dependency(raw: &Map<String, Value>) -> Option<TaskDependency> {
    let id = non_empty(text(raw.get("id")))?;
    let from_task_id = non_empty(text(coalesce(raw, &["from_task_id", "predecessor_id"])))?;
    let to_task_id = non_empty(text(coalesce(raw, &["to_task_id", "successor_id"])))?;
    if raw.get("is_deleted") == Some(&Value::Bool(true)) {
        return None;
    }
    Some(TaskDependency {
        id,
        from_task_id,
        to_task_id,
        dependency_type: match text(raw.get("type")).as_str() {
            "SS" => DependencyType::SS,
            "FF" => DependencyType::FF,
            "SF" => DependencyType::SF,
            // Finish-to-start is the shape of every dependency nobody typed.
            _ => DependencyType::FS,
        },
        // A lag authored in DAYS is eight working hours each (wire.ts:245) —
        // the same 480 minutes a default task duration is worth.
        lag_minutes: optional_integer(raw.get("lag_minutes"))
            .unwrap_or_else(|| integer(raw.get("lag_days")) * 480),
        version: integer(raw.get("version")).max(0),
    })
}

/// `decodeResidualItem` (wire.ts:250-278).
pub fn decode_residual_item(raw: &Map<String, Value>) -> Option<TaskResidualItem> {
    let id = non_empty(text(raw.get("id")))?;
    let task_id = non_empty(text(raw.get("task_id")))?;
    let title = non_empty(text(raw.get("title")))?;
    if raw.get("is_deleted") == Some(&Value::Bool(true)) {
        return None;
    }
    let due_date = schedule_date(raw.get("due_date"));
    let assignment = decode_task_assignment(raw);
    Some(TaskResidualItem {
        id,
        task_id,
        title,
        description: text(raw.get("description")),
        kind: match text(raw.get("kind")).as_str() {
            "clarification" => ResidualKind::Clarification,
            "minor_correction" => ResidualKind::MinorCorrection,
            "review_response" => ResidualKind::ReviewResponse,
            "missing_information" => ResidualKind::MissingInformation,
            "signature_or_approval" => ResidualKind::SignatureOrApproval,
            "closeout_document" => ResidualKind::CloseoutDocument,
            "monitoring" => ResidualKind::Monitoring,
            "client_response" => ResidualKind::ClientResponse,
            "internal_follow_up" => ResidualKind::InternalFollowUp,
            "blocker" => ResidualKind::Blocker,
            "potential_additional_scope" => ResidualKind::PotentialAdditionalScope,
            "approved_additional_scope" => ResidualKind::ApprovedAdditionalScope,
            // `other` and anything undeclared alike (wire.ts:364-376).
            _ => ResidualKind::Other,
        },
        status: match text(raw.get("status")).as_str() {
            "in_progress" => ResidualStatus::InProgress,
            "awaiting_response" => ResidualStatus::AwaitingResponse,
            "resolved" => ResidualStatus::Resolved,
            "waived" => ResidualStatus::Waived,
            "promoted" => ResidualStatus::Promoted,
            // An undeclared status is OPEN, so nothing is closed by silence
            // (wire.ts:378-381).
            _ => ResidualStatus::Open,
        },
        scope_classification: match text(coalesce(raw, &["scope_classification", "scope"])).as_str()
        {
            "within_original_scope" => ScopeClassification::WithinOriginalScope,
            "potential_change" => ScopeClassification::PotentialChange,
            "approved_change" => ScopeClassification::ApprovedChange,
            // `not_applicable` and anything undeclared alike (wire.ts:383-388).
            _ => ScopeClassification::NotApplicable,
        },
        // INVENTED DEFAULT (wire.ts:390-399): legacy rows are not silently
        // certified as bounded. They remain visibly review-required until a
        // user makes the explicit classification decision.
        work_classification: match text(raw.get("work_classification")).as_str() {
            "bounded_residual" => ResidualWorkClassification::BoundedResidual,
            "material_rework" => ResidualWorkClassification::MaterialRework,
            "controls_schedule" => ResidualWorkClassification::ControlsSchedule,
            _ => ResidualWorkClassification::ScopeReviewRequired,
        },
        responsible_user_id: assignment.responsible_email.clone(),
        responsible_email: assignment.responsible_email,
        collaborator_emails: assignment.collaborator_emails,
        assignee_emails: assignment.assignee_emails,
        due_date: non_empty(due_date),
        blocks_acceptance: raw.get("blocks_acceptance") == Some(&Value::Bool(true)),
        blocks_closeout: raw.get("blocks_closeout") == Some(&Value::Bool(true)),
        source_record_id: non_empty(text(raw.get("source_record_id"))),
        promoted_task_id: non_empty(text(raw.get("promoted_task_id"))),
        resolved_at: optional_instant(raw.get("resolved_at")),
        resolved_by: non_empty(text(raw.get("resolved_by"))),
        updated_at: metadata_updated_at(raw),
        updated_by: non_empty(text(record(raw.get("metadata")).get("updated_by"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn one_task(envelope: Value) -> CanonicalProjectTask {
        decode_task(envelope.as_object().expect("envelope is an object")).expect("task decodes")
    }

    fn task_with(data: Value) -> CanonicalProjectTask {
        let mut envelope = json!({"id": "t", "data": {"title": "T"}});
        for (key, value) in data.as_object().expect("data is an object") {
            envelope["data"][key] = value.clone();
        }
        one_task(envelope)
    }

    fn one_residual(raw: Value) -> TaskResidualItem {
        decode_residual_item(raw.as_object().expect("residual is an object"))
            .expect("residual decodes")
    }

    #[test]
    fn a_schedule_state_is_inferred_from_which_dates_the_row_carries() {
        // INVENTED DEFAULT (wire.ts:132-134). Nothing on the wire declares a
        // schedule state; it exists only because two date fields do. Every
        // Gantt bar and the whole duration rule below hang off this inference,
        // so a kernel that derived it differently would draw a different plan.
        assert_eq!(
            task_with(json!({"start_date": "2026-09-01", "finish_date": "2026-09-10"}))
                .schedule_state,
            TaskScheduleState::Full
        );
        assert_eq!(
            task_with(json!({"start_date": "2026-09-01"})).schedule_state,
            TaskScheduleState::StartOnly
        );
        assert_eq!(
            task_with(json!({"finish_date": "2026-09-10"})).schedule_state,
            TaskScheduleState::FinishOnly
        );
        assert_eq!(
            task_with(json!({})).schedule_state,
            TaskScheduleState::Unscheduled
        );
        // The aliases the browser accepts for the same two dates, in order.
        assert_eq!(
            task_with(json!({"finish_at": "2026-09-11"})).finish_date,
            "2026-09-11"
        );
        assert_eq!(
            task_with(json!({"due_date": "2026-09-12"})).finish_date,
            "2026-09-12"
        );
        assert_eq!(
            task_with(json!({"start": "2026-09-02"})).start_date,
            "2026-09-02"
        );
        // A timestamp is read down to its calendar day; anything else is no date.
        assert_eq!(
            task_with(json!({"finish_date": "2026-09-10T17:30:00Z"})).finish_date,
            "2026-09-10"
        );
        assert_eq!(
            task_with(json!({"finish_date": "10/09/2026"})).finish_date,
            ""
        );
    }

    #[test]
    fn a_fully_scheduled_task_that_declares_no_duration_is_one_eight_hour_day() {
        // INVENTED DEFAULT (wire.ts:177-179). 480 minutes is the working day
        // the whole schedule is denominated in — the same 480 a dependency lag
        // authored in days is worth. A milestone and a partially scheduled
        // task have NO duration at all, which is why neither can be resized.
        let scheduled = json!({"start_date": "2026-09-01", "finish_date": "2026-09-10"});
        assert_eq!(task_with(scheduled.clone()).duration_minutes, 480);
        let mut declared = scheduled.clone();
        declared["duration_minutes"] = json!(960);
        assert_eq!(task_with(declared).duration_minutes, 960);
        let mut negative = scheduled.clone();
        negative["duration_minutes"] = json!(-30);
        assert_eq!(task_with(negative).duration_minutes, 0, "never below zero");
        let mut milestone = scheduled.clone();
        milestone["type"] = json!("milestone");
        assert_eq!(task_with(milestone).duration_minutes, 0);
        assert_eq!(
            task_with(json!({"start_date": "2026-09-01", "duration_minutes": 960}))
                .duration_minutes,
            0,
            "a task that is not fully scheduled carries no duration"
        );
    }

    #[test]
    fn a_closeout_state_is_inferred_from_delivery_when_the_row_declares_none() {
        // INVENTED DEFAULT (wire.ts:350-354). Closeout follows delivery, so a
        // complete task reads `closed` even while it still carries open
        // residuals. That is the browser's answer and the kernel keeps it: the
        // residual counts are the honest signal, not this field.
        assert_eq!(
            task_with(json!({"delivery_state": "complete"})).closeout_state,
            CloseoutState::Closed
        );
        assert_eq!(
            task_with(json!({"delivery_state": "in_progress"})).closeout_state,
            CloseoutState::Open
        );
        assert_eq!(
            task_with(json!({"delivery_state": "complete", "closeout_state": "residual_open"}))
                .closeout_state,
            CloseoutState::ResidualOpen,
            "a declared state always wins over the inference"
        );
    }

    #[test]
    fn the_legacy_done_and_doing_status_words_still_decode() {
        // INVENTED DEFAULT (wire.ts:331-340). `status` is what the schedule
        // wrote before delivery states existed. Dropping the mapping would
        // silently move every pre-migration task to `not_started` — a whole
        // project reading as untouched.
        assert_eq!(
            task_with(json!({"status": "done"})).delivery_state,
            DeliveryState::Complete
        );
        assert_eq!(
            task_with(json!({"status": "doing"})).delivery_state,
            DeliveryState::InProgress
        );
        assert_eq!(
            task_with(json!({"status": "anything else"})).delivery_state,
            DeliveryState::NotStarted
        );
        assert_eq!(
            task_with(json!({"delivery_state": "paused", "status": "done"})).delivery_state,
            DeliveryState::Paused,
            "a declared delivery state is never overruled by the legacy word"
        );
    }

    #[test]
    fn a_residual_nobody_classified_stays_review_required() {
        // INVENTED DEFAULT (wire.ts:390-399). Legacy rows are not silently
        // certified as bounded — they stay visibly review-required until a
        // user makes the explicit decision. Defaulting to `bounded_residual`
        // would quietly clear work that may be a change of scope.
        let base = json!({"id": "r", "task_id": "t", "title": "Query"});
        assert_eq!(
            one_residual(base.clone()).work_classification,
            ResidualWorkClassification::ScopeReviewRequired
        );
        let mut classified = base.clone();
        classified["work_classification"] = json!("bounded_residual");
        assert_eq!(
            one_residual(classified).work_classification,
            ResidualWorkClassification::BoundedResidual
        );
        // Its siblings default differently and each on purpose: an unclassified
        // status is OPEN so nothing closes by silence, a kind is `other`, and a
        // scope classification is `not_applicable`.
        let residual = one_residual(base.clone());
        assert_eq!(residual.status, ResidualStatus::Open);
        assert_eq!(residual.kind, ResidualKind::Other);
        assert_eq!(
            residual.scope_classification,
            ScopeClassification::NotApplicable
        );
        // The two kinds the backend never declared stay readable, so a row
        // written before the scope classification still decodes as itself.
        let mut legacy = base.clone();
        legacy["kind"] = json!("potential_additional_scope");
        assert_eq!(
            one_residual(legacy).kind,
            ResidualKind::PotentialAdditionalScope
        );
        let mut aliased = base;
        aliased["scope"] = json!("approved_change");
        assert_eq!(
            one_residual(aliased).scope_classification,
            ScopeClassification::ApprovedChange,
            "`scope` is the older name of `scope_classification`"
        );
    }

    #[test]
    fn a_week_nobody_declared_works_monday_to_friday() {
        // INVENTED DEFAULT (wire.ts:111). Sunday first, matching the weekday
        // indexes JavaScript and Go both use — a calendar shifted by one would
        // schedule every auto task onto the wrong days.
        let calendar = decode_project_calendar(None);
        assert_eq!(
            calendar.working_weekdays,
            vec![false, true, true, true, true, true, false]
        );
        // Seven booleans, or the default: six of them is a malformed week, not
        // a short one.
        let short = json!({"working_weekdays": [true, true, true]});
        assert_eq!(
            decode_project_calendar(Some(&short)).working_weekdays,
            vec![false, true, true, true, true, true, false]
        );
        // Holidays arrive as a list, or as a map whose `true` entries are the
        // dates. Anything that is not a calendar day is dropped.
        let listed = json!({"holidays": ["2026-07-04", "not a day", "2026-12-25"]});
        assert_eq!(
            decode_project_calendar(Some(&listed)).holidays,
            vec!["2026-07-04", "2026-12-25"]
        );
        let mapped = json!({"holidays": {"2026-07-04": true, "2026-12-25": false}});
        assert_eq!(
            decode_project_calendar(Some(&mapped)).holidays,
            vec!["2026-07-04"]
        );
    }

    #[test]
    fn a_row_with_no_sort_key_is_ordered_by_when_it_was_last_touched() {
        // INVENTED DEFAULT (wire.ts:158). Sibling order has to be TOTAL or the
        // WBS renumbers between two reads of the same graph; `updatedAt:id` is
        // what makes it total on a graph that never authored a sort key.
        let task = one_task(json!({
            "id": "t", "data": {"title": "T"}, "metadata": {"updated_at": 1700},
        }));
        assert_eq!(task.sort_key, "1700:t");
        assert_eq!(task_with(json!({"sort_key": "0100"})).sort_key, "0100");
    }

    #[test]
    fn progress_follows_the_delivery_state_and_is_clamped_into_its_range() {
        // INVENTED DEFAULT (wire.ts:135, 169). A complete task with no
        // progress of its own is 1, everything else is 0, and a value outside
        // 0–1 is clamped rather than refused — so the dashboard mean is always
        // a fraction.
        assert_eq!(
            task_with(json!({"delivery_state": "complete"})).progress,
            1.0
        );
        assert_eq!(
            task_with(json!({"delivery_state": "blocked"})).progress,
            0.0
        );
        assert_eq!(task_with(json!({"progress": 0.25})).progress, 0.25);
        assert_eq!(task_with(json!({"progress": 4})).progress, 1.0);
        assert_eq!(task_with(json!({"progress": -4})).progress, 0.0);
        assert_eq!(
            task_with(json!({"delivery_state": "complete", "progress": 0.1})).progress,
            0.1,
            "a declared progress is never overruled by the state"
        );
    }

    #[test]
    fn the_responsible_holder_falls_back_to_the_first_projected_assignee() {
        // INVENTED DEFAULT (wire.ts:213-233). A row that only ever carried
        // `assignee_emails` still reads as somebody's work, and the rest of
        // that list becomes the collaborators. Addresses are lower-cased,
        // de-duplicated and dropped when they are not addresses at all.
        let task = task_with(json!({
            "assignee_emails": ["Lead@DS.rw", "lead@ds.rw", "crew@ds.rw", "not an address"],
        }));
        assert_eq!(task.responsible_email.as_deref(), Some("lead@ds.rw"));
        assert_eq!(task.collaborator_emails, vec!["crew@ds.rw"]);
        assert_eq!(task.assignee_emails, vec!["lead@ds.rw", "crew@ds.rw"]);
        // An explicit holder wins, and never doubles as their own collaborator.
        let explicit = task_with(json!({
            "responsible_email": "boss@ds.rw",
            "collaborator_emails": ["boss@ds.rw", "crew@ds.rw"],
            "assignee_emails": ["lead@ds.rw"],
        }));
        assert_eq!(explicit.responsible_email.as_deref(), Some("boss@ds.rw"));
        assert_eq!(explicit.collaborator_emails, vec!["crew@ds.rw"]);
        // `responsible_user_id` is the older name of the same field.
        assert_eq!(
            task_with(json!({"responsible_user_id": "old@ds.rw"}))
                .responsible_email
                .as_deref(),
            Some("old@ds.rw")
        );
        // A residual carries the deprecated alias alongside the real field, so
        // a pre-canonical projection still reads it.
        let residual = one_residual(json!({
            "id": "r", "task_id": "t", "title": "Query", "responsible_email": "me@ds.rw",
        }));
        assert_eq!(residual.responsible_user_id.as_deref(), Some("me@ds.rw"));
    }

    #[test]
    fn a_dependency_lag_authored_in_days_is_eight_working_hours_each() {
        // INVENTED DEFAULT (wire.ts:245). The same 480 minutes a default task
        // duration is worth — a lag read as calendar days would slide every
        // successor across a weekend.
        let lagged = json!({"id": "d", "from_task_id": "a", "to_task_id": "b", "lag_days": 2});
        let dependency =
            decode_dependency(lagged.as_object().expect("object")).expect("dependency decodes");
        assert_eq!(dependency.lag_minutes, 960);
        assert_eq!(dependency.dependency_type, DependencyType::FS);
        let minutes = json!({
            "id": "d", "predecessor_id": "a", "successor_id": "b",
            "lag_minutes": 15, "lag_days": 2, "type": "SS",
        });
        let dependency =
            decode_dependency(minutes.as_object().expect("object")).expect("dependency decodes");
        assert_eq!(dependency.lag_minutes, 15, "declared minutes win");
        assert_eq!(dependency.dependency_type, DependencyType::SS);
    }

    #[test]
    fn a_deleted_or_nameless_row_never_reaches_the_graph() {
        // The graph is what the plan counts, so a tombstone that survived the
        // decode would be counted as work.
        assert!(decode_task(
            json!({"id": "t", "data": {"title": "T"}, "is_deleted": true})
                .as_object()
                .expect("object")
        )
        .is_none());
        assert!(
            decode_task(json!({"id": "t", "data": {}}).as_object().expect("object")).is_none(),
            "a row with no title is not a task"
        );
        assert!(
            decode_task(json!({"data": {"title": "T"}}).as_object().expect("object")).is_none(),
            "a row with no id has no identity"
        );
    }

    #[test]
    fn what_the_command_authored_is_read_apart_from_what_the_schedule_derived() {
        // `data` is the whole of what may be edited and `computed` is
        // read-only everywhere. Reading them apart is what lets the properties
        // dialog decide editability from the document instead of from a
        // hand-maintained list of field names — and `computed` wins the merge.
        let task = one_task(json!({
            "id": "t",
            "data": {"title": "T", "delivery_state": "in_progress", "start_date": "2026-09-01"},
            "computed": {"delivery_state": "blocked", "open_blocker_count": 2},
            "metadata": {"created_at": 10, "created_by": "maker@ds.rw", "updated_at": 20, "updated_by": "editor@ds.rw"},
        }));
        assert_eq!(task.delivery_state, DeliveryState::Blocked);
        assert_eq!(task.open_blocker_count, 2);
        assert_eq!(
            task.authored_fields,
            vec!["title", "deliveryState", "startDate"],
            "authored names are reported as the properties dialog knows them"
        );
        assert_eq!(task.created_at, 10);
        assert_eq!(task.created_by.as_deref(), Some("maker@ds.rw"));
        assert_eq!(task.updated_by.as_deref(), Some("editor@ds.rw"));
        // An unknown stored value invents no lifecycle action (wire.ts:194-200).
        assert_eq!(
            one_task(json!({
                "id": "t", "data": {"title": "T"},
                "computed": {"assignment_last_action": "teleported"},
            }))
            .assignment_last_action,
            None
        );
        assert_eq!(
            one_task(json!({
                "id": "t", "data": {"title": "T"},
                "computed": {"assignment_last_action": "accepted"},
            }))
            .assignment_last_action,
            Some(AssignmentAction::Accepted)
        );
    }

    #[test]
    fn a_field_model_defaults_nothing_and_is_absent_when_the_backend_declared_none() {
        // The frontend keeps no copy of the vocabulary. A missing list means
        // the graph predates the field model and the caller falls back
        // explicitly — inventing one here is the duplication that once offered
        // `task` for a node that WAS a milestone.
        let declared = decode_project_graph(
            &json!({"project_id": "p", "field_model": {"max_assignees": 5}}),
            "p",
        );
        let model = declared.field_model.expect("a declared field model");
        assert_eq!(model.max_assignees, 5);
        assert!(model.residual_kinds.is_empty());
        assert!(model.work_classifications.is_empty());
        let silent = decode_project_graph(&json!({"project_id": "p"}), "p");
        assert!(silent.field_model.is_none());
    }

    #[test]
    fn the_graph_reads_its_aliases_and_drops_what_it_cannot_trust() {
        // `residuals` and `graph_revision` are the older names, and an
        // unknown subject state is DROPPED rather than defaulted: a freshness
        // verdict nobody declared is not a verdict.
        let graph = decode_project_graph(
            &json!({
                "graph_revision": 9,
                "residuals": [{"id": "r", "task_id": "t", "title": "Query"}],
                "subject_states": [
                    {"object_type": "design", "object_id": "d1", "state": "changed", "checked_at": "2026-09-18T00:00:00Z"},
                    {"object_type": "design", "object_id": "d2", "state": "teleported"},
                    {"object_type": "design", "state": "current"},
                ],
                "subject_work": [{"object_type": "design", "object_id": "d1", "active_count": -2, "latest_updated_at": 0}],
                "permissions": {"view": true, "admin": "yes"},
            }),
            "session-project",
        );
        assert_eq!(graph.project_id, "session-project");
        assert_eq!(graph.revision, 9);
        assert_eq!(graph.residual_items.len(), 1);
        assert_eq!(graph.subject_states.len(), 1);
        assert_eq!(graph.subject_states[0].state, SubjectStateKind::Changed);
        assert_eq!(
            graph.subject_work[0].active_count, 0,
            "counts never go negative"
        );
        assert_eq!(graph.subject_work[0].latest_updated_at, None);
        // Only a literal `true` is a permission — a truthy string is not.
        assert!(graph.permissions.view);
        assert!(!graph.permissions.admin);
    }

    #[test]
    fn a_millisecond_stamp_becomes_the_instant_the_browser_would_have_written() {
        // `optionalInstant` turns a number into `new Date(ms).toISOString()`
        // and leaves a string alone. A stamp the browser could not represent
        // would have THROWN there; here it is simply absent, which is the one
        // place this port answers differently and is reachable only from a
        // wire value that is already nonsense.
        assert_eq!(
            task_with(json!({"proposed_at": 1_789_689_600_000i64}))
                .proposed_at
                .as_deref(),
            Some("2026-09-18T00:00:00.000Z")
        );
        assert_eq!(
            task_with(json!({"proposed_at": "2026-09-18T09:30:00Z"}))
                .proposed_at
                .as_deref(),
            Some("2026-09-18T09:30:00Z")
        );
        assert_eq!(
            task_with(json!({"proposed_at": i64::MAX})).proposed_at,
            None
        );
        assert_eq!(task_with(json!({})).proposed_at, None);
    }

    #[test]
    fn a_task_reads_its_correspondence_blockers_apart_from_the_schedule_counts() {
        // `data.blockers[]` is authored by `task_block`; the two counts are
        // the engine's own under `computed`. Neither is re-derived here: a
        // client that counted open blockers for itself would disagree with
        // the plan the moment the engine's rule moved.
        let task = one_task(json!({
            "id": "T2b",
            "data": {
                "title": "Replace spans",
                "blockers": [
                    {"kind": "awaiting_correspondence", "record_id": "R5",
                     "set_by": "lead@ds.rw", "set_at": 1_789_689_600_000i64},
                    {"kind": "awaiting_correspondence", "record_id": "R2",
                     "cleared_at": "2026-09-19T08:00:00Z", "cleared_by": "response"},
                    {"kind": "awaiting_correspondence"},
                ],
            },
            "computed": {
                "open_blocker_count": 1,
                "awaiting_correspondence_count": 1,
                "blocked_record_ids": ["R5"],
            },
        }));
        assert_eq!(
            task.blockers.len(),
            2,
            "a blocker naming no record is dropped"
        );
        assert!(task.blockers[0].is_open());
        assert_eq!(
            task.blockers[0].set_at.as_deref(),
            Some("2026-09-18T00:00:00.000Z")
        );
        assert!(!task.blockers[1].is_open());
        assert_eq!(task.blockers[1].cleared_by.as_deref(), Some("response"));
        assert_eq!(task.awaiting_correspondence_count, 1);
        assert_eq!(task.blocked_record_ids, vec!["R5"]);
        // The plain task carries none, and says so by omission.
        let plain = task_with(json!({}));
        assert!(plain.blockers.is_empty());
        assert_eq!(plain.awaiting_correspondence_count, 0);
    }

    #[test]
    fn the_correspondence_projection_and_its_vocabularies_are_read_and_never_invented() {
        let graph = decode_project_graph(
            &json!({
                "project_id": "p",
                "field_model": {
                    "max_assignees": 5,
                    "channels": ["email", "letter", "system"],
                    "response_statuses": ["none", "outstanding", "overdue", "responded", "waived"],
                    "party_roles": ["client", "consultant"],
                },
                "correspondence": {
                    "as_of": "2026-09-20", "due_window_days": 7,
                    "records": [
                        {"record_id": "R5", "thread_id": "R1", "subject": "Questions", "category": "request_for_information",
                         "channel": "email", "direction": "outbound", "happened_at": "2026-09-20T09:00:00Z",
                         "response_status": "outstanding", "response_due_date": "2026-09-24",
                         "days_overdue": 0, "days_until_due": 4, "due_within_window": true,
                         "response_owner_party_id": "p_tbea", "party_id": "p_tbea", "party_name": "TBEA",
                         "external_party_ids": ["p_shaker", "p_tbea"], "blocked_task_ids": ["T2b"], "sensitivity": "confidential"},
                        {"subject": "a row with no record id"},
                    ],
                    "records_total": 2, "truncated": false,
                    "blocked_tasks": [{"task_id": "T2b", "title": "Replace spans", "record_ids": ["R5"], "party_ids": ["p_tbea"]}],
                    "counters": {"recordsOutstanding": 1, "recordsOverdue": 0, "tasksAwaitingCorrespondence": 1},
                },
            }),
            "p",
        );
        let model = graph.field_model.expect("field model");
        assert_eq!(model.channels, vec!["email", "letter", "system"]);
        assert_eq!(model.response_statuses.len(), 5);
        assert!(model.record_categories.is_empty(), "undeclared stays empty");
        let correspondence = graph.correspondence.expect("projection");
        assert_eq!(correspondence.as_of, "2026-09-20");
        assert_eq!(correspondence.due_window_days, 7);
        assert_eq!(
            correspondence.records.len(),
            1,
            "a row without a record id is dropped"
        );
        let row = &correspondence.records[0];
        assert_eq!(row.thread_id, "R1");
        assert_eq!(row.response_due_date.as_deref(), Some("2026-09-24"));
        assert!(row.due_within_window);
        assert_eq!(row.party_name, "TBEA");
        assert_eq!(row.sensitivity, "confidential");
        assert_eq!(correspondence.records_total, 2);
        assert_eq!(correspondence.blocked_tasks[0].task_id, "T2b");
        assert_eq!(correspondence.counters.records_outstanding, 1);
        assert_eq!(correspondence.counters.tasks_awaiting_correspondence, 1);
        // A graph that never published the projection has none, not an empty one.
        let silent = decode_project_graph(&json!({"project_id": "p"}), "p");
        assert!(silent.correspondence.is_none());
    }
}
