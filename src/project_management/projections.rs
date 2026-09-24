//! The four folds the Project Work surface reads the plan through.
//!
//! A port of `ds-web/src/lib/project-work/views.ts`: `projectWorkDashboard`
//! (line 215), `projectWorkPhases` (256), `projectWorkAttention` (296) and
//! `projectWorkRecentlyUpdated` (344). Nothing here fetches, clocks or
//! formats: `today` is an input and every label is the caller's to translate.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::design_dashboard::locale_compare;

use super::hierarchy::wbs_by_task;
use super::wire::{
    AdmissionState, CanonicalProjectGraph, CanonicalProjectTask, Correspondence,
    CorrespondenceCounters, DeliveryState, ResidualStatus, ReviewState, TaskType, DAY_MS,
};

/// Below this a stamp is a fixture or a sentinel, not a wall-clock time —
/// `Date.UTC(2000, 0, 1)` (views.ts:97-98). It is what keeps a row whose
/// `updatedAt` was never populated from reading as "blocked for 20 000 days".
const PLAUSIBLE_STAMP_FLOOR_MS: i64 = 946_684_800_000;
/// Ten years. Past this the gap is a broken stamp, not a stale task
/// (views.ts:122).
const MAX_PLAUSIBLE_DAYS: i64 = 3650;
/// `projectWorkRecentlyUpdated` never returns more than this, whatever it is
/// asked for (views.ts:348).
const MAX_RECENT: i64 = 24;

#[derive(Serialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub total_tasks: usize,
    pub milestones: usize,
    pub primary_complete: usize,
    pub past_planned_finish: usize,
    pub under_review: usize,
    pub open_residuals: usize,
    pub acceptance_blocked: usize,
    pub closeout_blocked: usize,
    /// Every node of the plan — work tasks and milestones alike.
    pub plan_nodes: usize,
    /// Mean of each plan node's own `progress`, 0–1. Unweighted on purpose: a
    /// duration-weighted rollup is a schedule question, not a headline.
    pub overall_progress: f64,
    pub in_progress: usize,
    pub blocked: usize,
    pub paused: usize,
    pub blocked_or_paused: usize,
    /// Distinct tasks carrying at least one open residual — the honest
    /// "across N tasks" denominator for the residual KPI.
    pub residual_task_count: usize,
    /// Task proposals (task-proposals.md): Inbox tasks whose admission is
    /// `requested`, and tasks whose logged hours exceed their estimate.
    pub proposals_pending: usize,
    pub over_estimate: usize,
}

/// One proposal awaiting the PM, as `pm plan` lists it (task-proposals.md
/// §Command surface): who asked, for what, with what estimate.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub task_id: String,
    pub title: String,
    pub requested_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_hours: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_days: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsible: Option<String>,
}

/// The effort and delay facts of one task, derived beside the existing
/// `late` rule (task-proposals.md §Definitions) so there is one lateness
/// rule and one place that reads it. Every number is per task: no rollup.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskEffort {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_hours: Option<f64>,
    pub actual_hours: f64,
    /// `actual − estimated`, present only when an estimate exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate_vs_actual: Option<f64>,
    pub over_estimate: bool,
    /// Signed whole days against the planned finish: `today − finish` for an
    /// open task past its finish, `completed − finish` for a complete one
    /// (negative when early), absent when there is no planned finish or the
    /// task is cancelled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_variance_days: Option<i64>,
    pub late: bool,
}

/// One discipline's rollup — the KIND of work, which is what "phase" means on
/// this surface. Grouping by root task instead made every milestone a phase.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Phase {
    /// The discipline string itself, empty for the unassigned bucket.
    pub id: String,
    pub discipline: String,
    pub unassigned: bool,
    pub total: usize,
    pub complete: usize,
    pub blocked: usize,
    /// 0–1 mean progress across the bucket.
    pub progress: f64,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    Blocked,
    Late,
    /// Logged hours exceed the estimate (task-proposals.md). Ranked with the
    /// slipped finish: both say the plan's numbers were wrong.
    OverEstimate,
    Paused,
    Review,
    Residual,
}

/// A compact, actionable signal derived from the canonical graph. Each item
/// points back to the stable task identity; the caller owns presentation.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    pub task_id: String,
    pub title: String,
    pub kind: AttentionKind,
    /// Display-only position in the plan, joined from the same pass the Table
    /// uses. Never identity — WBS renumbers on reorder.
    pub wbs: String,
    /// Open blockers on the task and below it.
    pub blocker_count: i64,
    /// Whole days past the planned finish; 0 when the task is not late.
    pub days_late: i64,
    /// Hours logged beyond the estimate; 0 when within it or unestimated.
    pub hours_over: f64,
    /// Whole days since the canonical record last changed.
    pub days_since_change: i64,
    /// Open residual items on this task.
    pub residual_count: usize,
}

/// A recent schedule fact, deliberately not a synthetic activity record.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecentTask {
    pub task_id: String,
    pub title: String,
    pub updated_at: i64,
    /// Who the canonical record says touched it last. Absent on a graph
    /// written before the field shipped — degrade, never invent an actor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
}

/// Open means somebody still owes it something (views.ts:100-102).
fn is_open(status: ResidualStatus) -> bool {
    matches!(
        status,
        ResidualStatus::Open | ResidualStatus::InProgress | ResidualStatus::AwaitingResponse
    )
}

fn clamp_progress(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn in_review(state: ReviewState) -> bool {
    matches!(
        state,
        ReviewState::Submitted | ReviewState::UnderReview | ReviewState::ChangesRequested
    )
}

/// Days since the Unix epoch for a `YYYY-MM-DD` calendar date, or `None` when
/// `Date.parse("<date>T00:00:00Z")` would have returned `NaN` — which includes
/// a day the month does not have.
fn epoch_days(date: &str) -> Option<i64> {
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i64 = date.get(0..4)?.parse().ok()?;
    let month: i64 = date.get(5..7)?.parse().ok()?;
    let day: i64 = date.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    // Howard Hinnant's `days_from_civil`, the inverse of the crate's day key.
    let shifted_year = if month <= 2 { year - 1 } else { year };
    let era = shifted_year.div_euclid(400);
    let yoe = shifted_year - era * 400;
    let month_part = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * month_part + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.rem_euclid(4) == 0
            && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0) =>
        {
            29
        }
        2 => 28,
        _ => 0,
    }
}

/// Signed whole days from one calendar date to another; `None` when either
/// does not parse. The one place a variance may be negative.
fn signed_dayspan(from_date: &str, to_date: &str) -> Option<i64> {
    Some(epoch_days(to_date)? - epoch_days(from_date)?)
}

/// `effort(task, today)`: the one fold that reads a task's estimate, hours
/// and planned finish together. `late` here IS the attention rule below —
/// the same predicate, so a task cannot be late on the plan and on time on
/// its own row.
pub fn effort(task: &CanonicalProjectTask, today: &str) -> TaskEffort {
    let resolved = matches!(
        task.delivery_state,
        DeliveryState::Complete | DeliveryState::Cancelled
    );
    let late = !resolved && !task.finish_date.is_empty() && task.finish_date.as_str() < today;
    let estimate_vs_actual = task
        .estimated_hours
        .map(|estimated| task.actual_hours - estimated);
    let schedule_variance_days =
        if task.finish_date.is_empty() || task.delivery_state == DeliveryState::Cancelled {
            None
        } else if task.delivery_state == DeliveryState::Complete {
            task.primary_completed_at
                .as_deref()
                .map(|stamp| stamp.chars().take(10).collect::<String>())
                .and_then(|completed| signed_dayspan(&task.finish_date, &completed))
        } else if late {
            signed_dayspan(&task.finish_date, today)
        } else {
            Some(0)
        };
    TaskEffort {
        estimated_hours: task.estimated_hours,
        actual_hours: task.actual_hours,
        estimate_vs_actual,
        over_estimate: estimate_vs_actual.is_some_and(|delta| delta > 0.0),
        schedule_variance_days,
        late,
    }
}

/// Every Inbox task whose admission is `requested`, oldest request first so
/// the PM answers in the order people asked.
pub fn proposals(graph: &CanonicalProjectGraph) -> Vec<Proposal> {
    let mut rows: Vec<(Option<String>, Proposal)> = graph
        .tasks
        .iter()
        .filter_map(|task| {
            let admission = task.admission.as_ref()?;
            if admission.state != AdmissionState::Requested {
                return None;
            }
            Some((
                admission.requested_at.clone(),
                Proposal {
                    task_id: task.id.clone(),
                    title: task.title.clone(),
                    requested_by: admission.requested_by.clone(),
                    requested_at: admission.requested_at.clone(),
                    note: admission.note.clone(),
                    estimated_hours: task.estimated_hours,
                    estimated_days: task.estimated_days,
                    responsible: task.responsible_email.clone(),
                },
            ))
        })
        .collect();
    rows.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| locale_compare(&left.1.task_id, &right.1.task_id))
    });
    rows.into_iter().map(|(_, row)| row).collect()
}

/// `dayspan` (views.ts:110-115): whole days between two calendar dates, or 0
/// if either is unparseable — and never negative.
fn dayspan(from_date: &str, to_date: &str) -> i64 {
    match (epoch_days(from_date), epoch_days(to_date)) {
        (Some(from), Some(to)) => (to - from).max(0),
        _ => 0,
    }
}

/// `daysSince` (views.ts:118-123): days between a millisecond stamp and a
/// calendar date, reported only while both the stamp and the gap are
/// plausible. An implausible one answers 0, never a number that reads as fact.
fn days_since(stamp_ms: i64, today: &str) -> i64 {
    let Some(today_days) = epoch_days(today) else {
        return 0;
    };
    if stamp_ms < PLAUSIBLE_STAMP_FLOOR_MS {
        return 0;
    }
    let days = (today_days * DAY_MS - stamp_ms).div_euclid(DAY_MS);
    if days > 0 && days < MAX_PLAUSIBLE_DAYS {
        days
    } else {
        0
    }
}

/// `projectWorkDashboard` (views.ts:215-243).
pub fn dashboard(graph: &CanonicalProjectGraph, today: &str) -> Dashboard {
    let tasks: Vec<&CanonicalProjectTask> = graph
        .tasks
        .iter()
        .filter(|task| task.task_type == TaskType::Task)
        .collect();
    let open_residuals: Vec<_> = graph
        .residual_items
        .iter()
        .filter(|item| is_open(item.status))
        .collect();
    let progress_sum: f64 = graph
        .tasks
        .iter()
        .map(|task| clamp_progress(task.progress))
        .sum();
    let count = |state: DeliveryState| {
        graph
            .tasks
            .iter()
            .filter(|task| task.delivery_state == state)
            .count()
    };
    let blocked = count(DeliveryState::Blocked);
    let paused = count(DeliveryState::Paused);
    let plan_nodes = graph.tasks.len();
    Dashboard {
        total_tasks: tasks.len(),
        milestones: plan_nodes - tasks.len(),
        // These three count WORK TASKS only, while `inProgress`, `blocked` and
        // `paused` below count every plan node. The browser drew them from the
        // two different piles and the kernel keeps both piles as they were.
        primary_complete: tasks
            .iter()
            .filter(|task| task.delivery_state == DeliveryState::Complete)
            .count(),
        // The finish date is compared AS A STRING, so the empty string an
        // unscheduled task carries is less than every date. Late therefore
        // requires a finish date to exist, exactly as `attention` requires it:
        // a task cannot have slipped past a date it has not got.
        past_planned_finish: tasks
            .iter()
            .filter(|task| {
                !task.finish_date.is_empty()
                    && task.finish_date.as_str() < today
                    && task.delivery_state != DeliveryState::Complete
                    && task.delivery_state != DeliveryState::Cancelled
            })
            .count(),
        under_review: tasks
            .iter()
            .filter(|task| in_review(task.review_state))
            .count(),
        open_residuals: open_residuals.len(),
        acceptance_blocked: open_residuals
            .iter()
            .filter(|item| item.blocks_acceptance)
            .count(),
        closeout_blocked: open_residuals
            .iter()
            .filter(|item| item.blocks_closeout)
            .count(),
        plan_nodes,
        overall_progress: if plan_nodes > 0 {
            progress_sum / plan_nodes as f64
        } else {
            0.0
        },
        in_progress: count(DeliveryState::InProgress),
        blocked,
        paused,
        blocked_or_paused: blocked + paused,
        residual_task_count: open_residuals
            .iter()
            .map(|item| item.task_id.as_str())
            .collect::<BTreeSet<&str>>()
            .len(),
        proposals_pending: graph
            .tasks
            .iter()
            .filter(|task| {
                task.admission
                    .as_ref()
                    .is_some_and(|admission| admission.state == AdmissionState::Requested)
            })
            .count(),
        over_estimate: graph
            .tasks
            .iter()
            .filter(|task| {
                task.delivery_state != DeliveryState::Cancelled && effort(task, today).over_estimate
            })
            .count(),
    }
}

/// `projectWorkPhases` (views.ts:256-285).
///
/// Progress by phase, where a phase is the task's **discipline** — the kind of
/// work, which is the project's own vocabulary and the only field that answers
/// "how is survey doing versus design". One pass over the task list, no
/// per-bucket rescan. Nothing is dropped: the caller decides how many buckets
/// to paint and says how many it did not.
pub fn phases(graph: &CanonicalProjectGraph) -> Vec<Phase> {
    struct Bucket {
        sum: f64,
        total: usize,
        complete: usize,
        blocked: usize,
    }
    let mut buckets: BTreeMap<&str, Bucket> = BTreeMap::new();
    for task in &graph.tasks {
        let bucket = buckets.entry(task.discipline.trim()).or_insert(Bucket {
            sum: 0.0,
            total: 0,
            complete: 0,
            blocked: 0,
        });
        bucket.total += 1;
        bucket.sum += clamp_progress(task.progress);
        // Complete counts a full progress bar as well as the declared state,
        // and reads the RAW progress, not the clamped one.
        if task.delivery_state == DeliveryState::Complete || task.progress >= 1.0 {
            bucket.complete += 1;
        }
        if task.delivery_state == DeliveryState::Blocked {
            bucket.blocked += 1;
        }
    }
    let mut phases: Vec<Phase> = buckets
        .into_iter()
        .map(|(id, bucket)| Phase {
            id: id.to_string(),
            discipline: id.to_string(),
            unassigned: id.is_empty(),
            total: bucket.total,
            complete: bucket.complete,
            blocked: bucket.blocked,
            progress: if bucket.total > 0 {
                bucket.sum / bucket.total as f64
            } else {
                0.0
            },
        })
        .collect();
    // Biggest buckets first so a capped list shows the work that matters; the
    // nameless bucket always sits last, whatever its size.
    phases.sort_by(|left, right| {
        u8::from(left.unassigned)
            .cmp(&u8::from(right.unassigned))
            .then_with(|| right.total.cmp(&left.total))
            .then_with(|| locale_compare(&left.discipline, &right.discipline))
    });
    phases
}

/// `projectWorkAttention` (views.ts:296-340).
///
/// Surface the first decision-worthy state for each task without inventing an
/// activity or a second issue model. The order is intentional: a blocker beats
/// a slipped finish, that beats work parked on purpose, and any of them beats
/// a review state or an open residual for the same work item.
///
/// Each row also carries its magnitude — days, blockers, residuals — because
/// "Blocked" alone does not tell an operator whether to act today.
pub fn attention(graph: &CanonicalProjectGraph, today: &str) -> Vec<Attention> {
    let wbs = wbs_by_task(&graph.tasks);
    let mut residuals_by_task: BTreeMap<&str, usize> = BTreeMap::new();
    for item in &graph.residual_items {
        if !is_open(item.status) {
            continue;
        }
        *residuals_by_task.entry(item.task_id.as_str()).or_insert(0) += 1;
    }
    let mut items: Vec<Attention> = graph
        .tasks
        .iter()
        .filter_map(|task| {
            let resolved = matches!(
                task.delivery_state,
                DeliveryState::Complete | DeliveryState::Cancelled
            );
            let blocker_count = task.open_blocker_count + task.descendant_blocker_count;
            // Unlike the dashboard, `late` requires a finish date to exist:
            // an unscheduled task cannot have slipped past a date it has not.
            // The predicate is `effort`'s, so the row and the plan agree.
            let effort = effort(task, today);
            let late = effort.late;
            let over_estimate = !resolved && effort.over_estimate;
            let residual_count = residuals_by_task
                .get(task.id.as_str())
                .copied()
                .unwrap_or(0);
            let kind = if task.delivery_state == DeliveryState::Blocked || blocker_count > 0 {
                AttentionKind::Blocked
            } else if late {
                AttentionKind::Late
            } else if over_estimate {
                AttentionKind::OverEstimate
            } else if task.delivery_state == DeliveryState::Paused {
                AttentionKind::Paused
            } else if in_review(task.review_state) {
                AttentionKind::Review
            } else if residual_count > 0 {
                AttentionKind::Residual
            } else {
                return None;
            };
            Some(Attention {
                task_id: task.id.clone(),
                title: task.title.clone(),
                kind,
                wbs: wbs.get(&task.id).cloned().unwrap_or_default(),
                blocker_count,
                // The slip is still measured even when a blocker outranks it
                // as the REASON, so the row can show either without a second
                // pass.
                days_late: if late {
                    dayspan(&task.finish_date, today)
                } else {
                    0
                },
                hours_over: effort.estimate_vs_actual.unwrap_or(0.0).max(0.0),
                days_since_change: days_since(task.updated_at, today),
                residual_count,
            })
        })
        .collect();
    items.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| locale_compare(&left.title, &right.title))
            .then_with(|| locale_compare(&left.task_id, &right.task_id))
    });
    items
}

/// One awaiting record as the plan's correspondence attention shows it.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrespondenceAttentionRecord {
    pub record_id: String,
    pub thread_id: String,
    pub subject: String,
    pub category: String,
    pub channel: String,
    pub direction: String,
    /// `overdue` or `outstanding`.
    pub response_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_due_date: Option<String>,
    pub days_overdue: i64,
    pub days_until_due: i64,
    /// `party` when the counterparty owes the answer, `project` when we do.
    pub owed_by: &'static str,
    pub blocked_task_ids: Vec<String>,
    pub sensitivity: String,
}

/// One task the plan reports as blocked on correspondence, under the party
/// whose answer it waits for.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrespondenceBlockedTask {
    pub task_id: String,
    pub title: String,
    pub record_ids: Vec<String>,
}

/// The attention rows for one party: what is overdue, what is due within the
/// window, and which tasks wait on it. Bounded per party; the totals say
/// what was cut.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrespondenceParty {
    pub party_id: String,
    pub party_name: String,
    pub overdue: usize,
    pub due_within_window: usize,
    pub records: Vec<CorrespondenceAttentionRecord>,
    pub record_total: usize,
    pub blocked_tasks: Vec<CorrespondenceBlockedTask>,
    pub blocked_task_total: usize,
}

/// `ds pm plan`'s correspondence section: the three dashboard counters and
/// the attention rows grouped by party (contract §Plan attention).
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrespondencePlan {
    pub as_of: String,
    pub due_window_days: i64,
    pub counters: CorrespondenceCounters,
    pub parties: Vec<CorrespondenceParty>,
    pub party_total: usize,
    /// Every awaiting record the server counted, listed or not.
    pub records_total: i64,
    /// The server capped its own listing; some awaiting rows are not here.
    pub truncated: bool,
}

/// The plan's correspondence fold: the server's awaiting rows grouped under
/// the party the answer is owed to, overdue parties first, then the soonest
/// due. Only rows that have earned attention are listed — `overdue`, or
/// `outstanding` and due within the window — and every blocked task; the
/// counters count what the server counted. `None` when the graph carries no
/// projection at all, which a caller must not read as "nothing awaited".
pub fn correspondence(graph: &CanonicalProjectGraph, limit: i64) -> Option<CorrespondencePlan> {
    let projection: &Correspondence = graph.correspondence.as_ref()?;
    let bounded = limit.clamp(1, i64::MAX) as usize;
    struct Bucket {
        name: String,
        records: Vec<CorrespondenceAttentionRecord>,
        tasks: Vec<CorrespondenceBlockedTask>,
    }
    let mut buckets: BTreeMap<String, Bucket> = BTreeMap::new();
    for row in &projection.records {
        let attention = row.response_status == "overdue" || row.due_within_window;
        if !attention {
            continue;
        }
        let bucket = buckets
            .entry(row.party_id.clone())
            .or_insert_with(|| Bucket {
                name: row.party_name.clone(),
                records: Vec::new(),
                tasks: Vec::new(),
            });
        if bucket.name.is_empty() {
            bucket.name = row.party_name.clone();
        }
        bucket.records.push(CorrespondenceAttentionRecord {
            record_id: row.record_id.clone(),
            thread_id: row.thread_id.clone(),
            subject: row.subject.clone(),
            category: row.category.clone(),
            channel: row.channel.clone(),
            direction: row.direction.clone(),
            response_status: row.response_status.clone(),
            response_due_date: row.response_due_date.clone(),
            days_overdue: row.days_overdue,
            days_until_due: row.days_until_due,
            owed_by: if row.response_owner_party_id.is_some() {
                "party"
            } else {
                "project"
            },
            blocked_task_ids: row.blocked_task_ids.clone(),
            sensitivity: row.sensitivity.clone(),
        });
    }
    for task in &projection.blocked_tasks {
        // A blocked task sits under every party it waits on; a task whose
        // record named no party sits under the nameless bucket.
        let parties: Vec<String> = if task.party_ids.is_empty() {
            vec![String::new()]
        } else {
            task.party_ids.clone()
        };
        for party in parties {
            let bucket = buckets.entry(party).or_insert_with(|| Bucket {
                name: String::new(),
                records: Vec::new(),
                tasks: Vec::new(),
            });
            bucket.tasks.push(CorrespondenceBlockedTask {
                task_id: task.task_id.clone(),
                title: task.title.clone(),
                record_ids: task.record_ids.clone(),
            });
        }
    }
    let mut parties: Vec<CorrespondenceParty> = buckets
        .into_iter()
        .map(|(party_id, mut bucket)| {
            // Overdue first, longest overdue first; then soonest due.
            bucket.records.sort_by(|left, right| {
                let overdue =
                    |row: &CorrespondenceAttentionRecord| row.response_status == "overdue";
                overdue(right)
                    .cmp(&overdue(left))
                    .then_with(|| right.days_overdue.cmp(&left.days_overdue))
                    .then_with(|| left.days_until_due.cmp(&right.days_until_due))
                    .then_with(|| locale_compare(&left.record_id, &right.record_id))
            });
            bucket
                .tasks
                .sort_by(|left, right| locale_compare(&left.task_id, &right.task_id));
            let overdue = bucket
                .records
                .iter()
                .filter(|row| row.response_status == "overdue")
                .count();
            let record_total = bucket.records.len();
            let blocked_task_total = bucket.tasks.len();
            CorrespondenceParty {
                party_id,
                party_name: bucket.name,
                overdue,
                due_within_window: record_total - overdue,
                records: bucket.records.into_iter().take(bounded).collect(),
                record_total,
                blocked_tasks: bucket.tasks.into_iter().take(bounded).collect(),
                blocked_task_total,
            }
        })
        .collect();
    parties.sort_by(|left, right| {
        right
            .overdue
            .cmp(&left.overdue)
            .then_with(|| soonest(left).cmp(&soonest(right)))
            .then_with(|| right.blocked_task_total.cmp(&left.blocked_task_total))
            .then_with(|| locale_compare(&left.party_name, &right.party_name))
            .then_with(|| locale_compare(&left.party_id, &right.party_id))
    });
    let party_total = parties.len();
    Some(CorrespondencePlan {
        as_of: projection.as_of.clone(),
        due_window_days: projection.due_window_days,
        counters: projection.counters,
        parties: parties.into_iter().take(bounded).collect(),
        party_total,
        records_total: projection.records_total,
        truncated: projection.truncated,
    })
}

/// The soonest non-overdue due day among a party's listed rows.
fn soonest(party: &CorrespondenceParty) -> i64 {
    party
        .records
        .iter()
        .filter(|row| row.response_status != "overdue")
        .map(|row| row.days_until_due)
        .min()
        .unwrap_or(i64::MAX)
}

/// `projectWorkRecentlyUpdated` (views.ts:344-358).
///
/// Recent means the canonical task's own update stamp — never a mock audit
/// feed. A bounded projection keeps the dashboard cheap on large projects.
pub fn recently_updated(graph: &CanonicalProjectGraph, limit: i64) -> Vec<RecentTask> {
    let bounded = limit.clamp(0, MAX_RECENT) as usize;
    let mut tasks: Vec<&CanonicalProjectTask> = graph.tasks.iter().collect();
    tasks.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| locale_compare(&left.title, &right.title))
            .then_with(|| locale_compare(&left.id, &right.id))
    });
    tasks
        .into_iter()
        .take(bounded)
        .map(|task| RecentTask {
            task_id: task.id.clone(),
            title: task.title.clone(),
            updated_at: task.updated_at,
            updated_by: task.updated_by.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_management::wire::decode_project_graph;
    use serde_json::{json, Value};

    /// The browser's own `project-work-views.test.ts` fixture, restated as the
    /// wire the kernel reads: three tasks in a chain, each carrying the same
    /// sort key, and one open residual on the middle one. Its expectations are
    /// the oracle this port is measured against.
    fn task(id: &str, parent: Option<&str>) -> Value {
        json!({
            "id": id,
            "data": {
                "title": id, "sort_key": "1", "parent_task_id": parent,
                "delivery_state": "in_progress", "progress": 0.5,
                "start_date": "2026-08-01", "finish_date": "2026-08-02",
            },
            "metadata": {"updated_at": 1},
        })
    }

    fn graph_of(tasks: Value) -> CanonicalProjectGraph {
        decode_project_graph(
            &json!({
                "project_id": "p", "revision": 1, "tasks": tasks,
                "residual_items": [{
                    "id": "r", "task_id": "child", "title": "Signature",
                    "kind": "signature_or_approval", "status": "open",
                    "blocks_acceptance": true, "blocks_closeout": true,
                }],
            }),
            "p",
        )
    }

    fn base_graph() -> CanonicalProjectGraph {
        graph_of(json!([
            task("grandchild", Some("child")),
            task("root", None),
            task("child", Some("root")),
        ]))
    }

    /// Merge `patch` into one task's `data`, the way the browser's test
    /// spreads over a fixture task.
    fn patched(id: &str, parent: Option<&str>, patch: Value) -> Value {
        let mut envelope = task(id, parent);
        for (key, value) in patch.as_object().expect("patch is an object") {
            envelope["data"][key] = value.clone();
        }
        envelope
    }

    #[test]
    fn the_dashboard_counts_work_tasks_and_plan_nodes_from_two_different_piles() {
        // Pinned because the two piles look like one and are not: complete,
        // past-finish and under-review count WORK TASKS, while the progress
        // mean and the state counts run over every plan node, milestones
        // included. The browser drew them that way and the KPIs only add up
        // against the Plan if the kernel keeps both piles.
        let graph = graph_of(json!([
            patched(
                "grandchild",
                Some("child"),
                json!({"progress": 1, "delivery_state": "complete"})
            ),
            patched(
                "root",
                None,
                json!({"progress": 0.5, "delivery_state": "blocked"})
            ),
            patched(
                "child",
                Some("root"),
                json!({"progress": 0, "delivery_state": "paused", "type": "milestone"})
            ),
        ]));
        let answer = dashboard(&graph, "2026-08-10");
        assert_eq!(answer.plan_nodes, 3);
        assert_eq!(answer.total_tasks, 2);
        assert_eq!(answer.milestones, 1);
        assert_eq!(answer.overall_progress, 0.5);
        assert_eq!(answer.in_progress, 0);
        assert_eq!(answer.blocked, 1);
        assert_eq!(answer.paused, 1);
        assert_eq!(answer.blocked_or_paused, 2);
        assert_eq!(answer.open_residuals, 1);
        assert_eq!(answer.residual_task_count, 1);
    }

    #[test]
    fn an_unscheduled_task_is_not_past_a_planned_finish_it_has_not_got() {
        // The dashboard compares the finish date AS A STRING, and an
        // unscheduled task carries the empty string, which is less than every
        // real date — so it used to read as late. `projectWorkAttention`
        // guarded that case and the dashboard did not, so one payload could
        // say `pastPlannedFinish: 1` and `attention: []`. The guarded reading
        // is the true one: both projections now mean the same thing by late.
        let graph = graph_of(json!([
            patched(
                "grandchild",
                Some("child"),
                json!({"finish_date": "", "start_date": ""})
            ),
            task("root", None),
            task("child", Some("root")),
        ]));
        assert_eq!(
            dashboard(&graph, "2026-08-10").past_planned_finish,
            2,
            "the two scheduled tasks are late; the unscheduled one is not"
        );
        assert!(
            !attention(&graph, "2026-08-10")
                .iter()
                .any(|item| item.task_id == "grandchild"),
            "the unscheduled task raises nothing on the attention list"
        );
    }

    #[test]
    fn phases_roll_up_by_discipline_and_keep_the_nameless_bucket_last() {
        // A phase is the KIND of work, not a root task — grouping by root made
        // every milestone a phase. The nameless bucket sorts last whatever its
        // size, so a capped list never spends its rows on unassigned work.
        let graph = graph_of(json!([
            patched(
                "grandchild",
                Some("child"),
                json!({"discipline": "Survey", "progress": 1, "delivery_state": "complete"})
            ),
            patched(
                "root",
                None,
                json!({"discipline": "Survey", "progress": 0.5, "delivery_state": "blocked"})
            ),
            patched(
                "child",
                Some("root"),
                json!({"discipline": "  ", "progress": 0.25})
            ),
        ]));
        let buckets = phases(&graph);
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].discipline, "Survey");
        assert!(!buckets[0].unassigned);
        assert_eq!(
            (buckets[0].total, buckets[0].complete, buckets[0].blocked),
            (2, 1, 1)
        );
        assert_eq!(buckets[0].progress, 0.75);
        assert_eq!(buckets[1].discipline, "");
        assert!(buckets[1].unassigned);
        assert_eq!(buckets[1].progress, 0.25);
    }

    #[test]
    fn every_attention_row_carries_its_magnitude_and_its_display_wbs() {
        // A blocker outranks a slipped finish as the REASON, but the slip is
        // still measured, and a residual that did not win still says how many
        // — so the row can be rendered either way without a second pass.
        let graph = graph_of(json!([
            patched(
                "grandchild",
                Some("child"),
                json!({"delivery_state": "blocked", "open_blocker_count": 2, "descendant_blocker_count": 1})
            ),
            patched(
                "root",
                None,
                json!({"finish_date": "2026-08-12", "delivery_state": "paused"})
            ),
            patched("child", Some("root"), json!({"finish_date": "2026-08-04"})),
        ]));
        let rows = attention(&graph, "2026-08-10");
        assert_eq!(
            rows,
            vec![
                Attention {
                    task_id: "grandchild".into(),
                    title: "grandchild".into(),
                    kind: AttentionKind::Blocked,
                    wbs: "1.1.1".into(),
                    blocker_count: 3,
                    days_late: 8,
                    hours_over: 0.0,
                    days_since_change: 0,
                    residual_count: 0,
                },
                Attention {
                    task_id: "child".into(),
                    title: "child".into(),
                    kind: AttentionKind::Late,
                    wbs: "1.1".into(),
                    blocker_count: 0,
                    days_late: 6,
                    hours_over: 0.0,
                    days_since_change: 0,
                    residual_count: 1,
                },
                Attention {
                    task_id: "root".into(),
                    title: "root".into(),
                    kind: AttentionKind::Paused,
                    wbs: "1".into(),
                    blocker_count: 0,
                    days_late: 0,
                    hours_over: 0.0,
                    days_since_change: 0,
                    residual_count: 0,
                },
            ]
        );
    }

    #[test]
    fn effort_reads_the_estimate_the_hours_and_the_one_lateness_rule() {
        // task-proposals.md: `late` is the attention rule; the variance is its
        // signed magnitude; a complete task measures against its completion
        // day; no finish means no variance; hours compare only when estimated.
        let graph = graph_of(json!([
            patched(
                "root",
                None,
                json!({"finish_date": "2026-08-04", "estimated_hours": 12, "estimated_days": 3,
                       "hours_log": [{"id": "e1", "by": "c@x", "hours": 4}, {"id": "e2", "by": "c@x", "hours": 10.5}],
                       "actual_hours": 14.5})
            ),
            patched(
                "child",
                Some("root"),
                json!({"finish_date": "2026-08-12", "delivery_state": "complete",
                       "primary_completed_at": "2026-08-09T15:00:00Z", "estimated_hours": 8,
                       "hours_log": [{"id": "e3", "by": "c@x", "hours": 6}]})
            ),
            patched(
                "grandchild",
                Some("child"),
                json!({"finish_date": "", "start_date": "",
                       "hours_log": [{"id": "e4", "by": "c@x", "hours": 2}]})
            ),
        ]));
        let by_id = |id: &str| graph.tasks.iter().find(|task| task.id == id).expect("task");
        let root = effort(by_id("root"), "2026-08-10");
        assert!(root.late);
        assert_eq!(root.schedule_variance_days, Some(6));
        assert_eq!(root.estimate_vs_actual, Some(2.5));
        assert!(root.over_estimate);
        assert_eq!(root.actual_hours, 14.5);
        let child = effort(by_id("child"), "2026-08-10");
        assert!(!child.late, "a complete task is never late");
        assert_eq!(
            child.schedule_variance_days,
            Some(-3),
            "finished three days early"
        );
        assert_eq!(child.estimate_vs_actual, Some(-2.0));
        assert!(!child.over_estimate);
        let grandchild = effort(by_id("grandchild"), "2026-08-10");
        assert_eq!(
            grandchild.schedule_variance_days, None,
            "no planned finish, no variance"
        );
        assert_eq!(
            grandchild.estimate_vs_actual, None,
            "no estimate, no comparison"
        );
        assert_eq!(
            grandchild.actual_hours, 2.0,
            "an older row without the server sum still sums its log"
        );
        assert!(!grandchild.over_estimate);
        // The attention list ranks the over-estimate after the slipped finish
        // and carries the magnitude; the dashboard counts it.
        let rows = attention(&graph, "2026-08-10");
        let root_row = rows
            .iter()
            .find(|row| row.task_id == "root")
            .expect("root row");
        assert_eq!(
            root_row.kind,
            AttentionKind::Late,
            "late outranks over-estimate"
        );
        assert_eq!(root_row.hours_over, 2.5);
        // The fixture's one open residual sits on `child`; that is the only
        // reason a complete task reaches the list — never late, never over.
        let child_row = rows
            .iter()
            .find(|row| row.task_id == "child")
            .expect("child row");
        assert_eq!(child_row.kind, AttentionKind::Residual);
        assert_eq!((child_row.days_late, child_row.hours_over), (0, 0.0));
        assert_eq!(dashboard(&graph, "2026-08-10").over_estimate, 1);
        let on_time = graph_of(json!([patched(
            "root",
            None,
            json!({"finish_date": "2026-08-30", "estimated_hours": 1, "actual_hours": 3})
        )]));
        let rows = attention(&on_time, "2026-08-10");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, AttentionKind::OverEstimate);
        assert_eq!(rows[0].hours_over, 2.0);
    }

    #[test]
    fn proposals_are_the_requested_inbox_tasks_oldest_first() {
        let graph = graph_of(json!([
            patched(
                "root",
                None,
                json!({"placement": "inbox", "estimated_hours": 12, "estimated_days": 3, "responsible_email": "c@x.rw",
                       "admission": {"state": "requested", "requested_by": "c@x.rw", "requested_at": "2026-08-02T09:00:00Z", "note": "site visit"}})
            ),
            patched(
                "child",
                None,
                json!({"placement": "inbox",
                       "admission": {"state": "requested", "requested_by": "d@x", "requested_at": "2026-08-01T09:00:00Z"}})
            ),
            patched(
                "grandchild",
                None,
                json!({"placement": "inbox",
                       "admission": {"state": "declined", "requested_by": "d@x", "requested_at": "2026-08-01T09:00:00Z", "reason": "no"}})
            ),
        ]));
        let rows = proposals(&graph);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].task_id, "child", "the older request comes first");
        assert_eq!(rows[1].task_id, "root");
        assert_eq!(rows[1].estimated_hours, Some(12.0));
        assert_eq!(rows[1].estimated_days, Some(3));
        assert_eq!(rows[1].note.as_deref(), Some("site visit"));
        assert_eq!(rows[1].responsible.as_deref(), Some("c@x.rw"));
        assert_eq!(rows[1].requested_by, "c@x.rw");
        assert_eq!(dashboard(&graph, "2026-08-10").proposals_pending, 2);
        let declined = graph
            .tasks
            .iter()
            .find(|task| task.id == "grandchild")
            .expect("declined");
        assert_eq!(
            declined.admission.as_ref().map(|a| a.state),
            Some(AdmissionState::Declined)
        );
        assert_eq!(
            declined
                .admission
                .as_ref()
                .and_then(|a| a.reason.clone())
                .as_deref(),
            Some("no")
        );
    }

    #[test]
    fn a_quiet_task_still_reaches_the_list_through_its_review_or_its_residual() {
        // The old reason set dropped both entirely: work parked on purpose and
        // work whose only open question is a residual are exactly the two
        // states that go unnoticed for weeks.
        let graph = graph_of(json!([
            patched(
                "grandchild",
                Some("child"),
                json!({"finish_date": "2026-08-20"})
            ),
            patched(
                "root",
                None,
                json!({"finish_date": "2026-08-20", "delivery_state": "paused"})
            ),
            patched("child", Some("root"), json!({"finish_date": "2026-08-20"})),
        ]));
        assert_eq!(
            attention(&graph, "2026-08-10")
                .iter()
                .map(|row| (row.kind, row.task_id.as_str(), row.residual_count))
                .collect::<Vec<_>>(),
            vec![
                (AttentionKind::Paused, "root", 0),
                (AttentionKind::Residual, "child", 1),
            ]
        );
    }

    #[test]
    fn days_since_a_change_is_reported_only_for_a_plausible_stamp() {
        // A fixture stamp (`updated_at: 1`) must never become "blocked for
        // 20 000 days" — a number an operator would act on.
        let mut tasks = vec![
            patched(
                "grandchild",
                Some("child"),
                json!({"delivery_state": "blocked"}),
            ),
            patched("root", None, json!({"delivery_state": "blocked"})),
            patched("child", Some("root"), json!({"delivery_state": "blocked"})),
        ];
        // 2026-08-04 and 2026-08-10 as UTC midnights.
        tasks[1]["metadata"] = json!({"updated_at": 1_785_801_600_000i64});
        tasks[2]["metadata"] = json!({"updated_at": 1_786_320_000_000i64});
        let graph = graph_of(Value::Array(tasks));
        assert_eq!(
            attention(&graph, "2026-08-10")
                .iter()
                .map(|row| (row.task_id.as_str(), row.days_since_change))
                .collect::<Vec<_>>(),
            vec![("child", 0), ("grandchild", 0), ("root", 6)]
        );
    }

    #[test]
    fn recent_is_the_canonical_update_stamp_and_never_an_invented_actor() {
        // Recent means the task's own stamp — not a synthetic activity feed —
        // and a graph written before `updated_by` shipped degrades to no actor
        // rather than to a guessed one.
        let mut tasks = vec![
            task("grandchild", Some("child")),
            task("root", None),
            task("child", Some("root")),
        ];
        tasks[0]["metadata"] = json!({"updated_at": 40});
        tasks[1]["metadata"] = json!({"updated_at": 80, "updated_by": "surveyor@example.com"});
        tasks[2]["metadata"] = json!({"updated_at": 60});
        let graph = graph_of(Value::Array(tasks));
        let rows = recently_updated(&graph, 2);
        assert_eq!(
            rows.iter()
                .map(|row| (row.task_id.as_str(), row.updated_at))
                .collect::<Vec<_>>(),
            vec![("root", 80), ("child", 60)]
        );
        assert_eq!(rows[0].updated_by.as_deref(), Some("surveyor@example.com"));
        assert_eq!(rows[1].updated_by, None);
        // Twenty-four is the ceiling whatever the caller asks for
        // (views.ts:348), and zero is a legal, empty answer.
        assert_eq!(recently_updated(&base_graph(), 500).len(), 3);
        assert!(recently_updated(&base_graph(), 0).is_empty());
    }

    #[test]
    fn a_day_the_calendar_does_not_have_measures_nothing() {
        // `Date.parse` answers NaN for it and the browser's `dayspan` folds
        // that to 0. Measuring it as a real gap would invent a slip.
        assert_eq!(dayspan("2026-02-30", "2026-03-01"), 0);
        assert_eq!(dayspan("2026-02-28", "2026-03-01"), 1);
        assert_eq!(dayspan("2024-02-29", "2024-03-01"), 1);
        // Never negative: a finish in the future is not a negative slip.
        assert_eq!(dayspan("2026-09-30", "2026-09-18"), 0);
    }
}
