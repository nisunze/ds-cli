//! The Project Work plan — decided in the kernel, no longer in the browser.
//!
//! `ds work plan` used to be answered by `ds-web/src/lib/desktop/cli-pm.ts`
//! (`readCliWorkPlan`, line 415), which decoded the canonical graph with
//! `project-work/wire.ts` and folded it with `project-work/views.ts`. Every one
//! of those decisions lived inside a running browser session, so the answer
//! existed only where a webview was mounted: a headless server, the CLI on a
//! machine with no window, and MCP could not produce it at all, and any second
//! host that tried would have had to re-derive a dozen invented defaults and
//! would have got at least one of them wrong.
//!
//! This module moves the whole fold — `decodeProjectGraph` plus
//! `projectWorkDashboard`, `projectWorkPhases`, `projectWorkAttention` and
//! `projectWorkRecentlyUpdated` — into the kernel, where the CLI, MCP, the
//! server and the desktop reach the same authority through the same bytes.
//! The defect it closes is a plan that differs by host.
//!
//! Native and WASM hosts share the same projections, hierarchy repairs and
//! validated write planners. Date presence still determines schedule state,
//! and work-day conversion is 480 minutes. Lateness requires a stated finish:
//! unscheduled work cannot slip past a date it never had.
//!
//! **Pure by the crate's contract**: no clock, no IO, no network. `today` is an
//! input, and the caller that has a clock supplies it.
//!
//! Since 2026-09-20 the rest of `ds pm` is decided here too: [`reads`] holds
//! the task and record projections (`task list|read`, `record list|read`) and
//! [`writes`] the validated project commands (`task create|update|assign|
//! respond`) and the fold of the engine's answer. Same doctrine — the browser's
//! rules, translated as they were, reachable without a window.

pub mod attachment_upload;
pub mod authoring_model;
mod browser;
pub(crate) mod command_wire;
pub mod correspondence;
mod hierarchy;
pub mod model_links;
pub mod overview;
pub mod project_notes;
pub mod projections;
pub mod reads;
pub mod task_attachment_detach;
mod wasm_writes;
pub mod wire;
pub mod writes;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use hierarchy::{ordered_rows, Hierarchy, OrderedRow};
pub use projections::{
    effort, proposals, Attention, AttentionKind, CorrespondenceAttentionRecord,
    CorrespondenceBlockedTask, CorrespondenceParty, CorrespondencePlan, Dashboard, Phase, Proposal,
    RecentTask, TaskEffort,
};
pub use wire::{decode_project_graph, CanonicalProjectGraph, ProjectGraphPermissions};

/// The shared request/reply schema for plans, hierarchy and write preparation.
pub const SCHEMA: &str = "ds.project-management/v1";

/// A canonical graph of ten thousand tasks and their residuals, with room to
/// spare. Past this the caller is sending something other than a plan.
const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;

/// The largest attention/recent list one plan returns — `MAX_PLAN_ROWS` in
/// cli-pm.ts:102, kept identical so an over-large `--limit` is refused with
/// the same number the CLI already documents.
const MAX_PLAN_ROWS: i64 = 100;

/// What a plan lists when the caller asks for no particular count
/// (cli-pm.ts:417).
const DEFAULT_PLAN_ROWS: i64 = 10;

#[derive(Deserialize)]
struct Head {
    #[serde(default)]
    schema: String,
    #[serde(default)]
    action: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanRequest {
    #[allow(dead_code)]
    schema: String,
    #[allow(dead_code)]
    action: String,
    /// The calendar day the plan is read against, `YYYY-MM-DD`. The browser
    /// took it from `new Date()`; the kernel has no clock, so the host that
    /// has one states it.
    today: String,
    /// `decodeProjectGraph`'s second argument: the session's selected project,
    /// used only when the payload names none of its own.
    #[serde(default)]
    ds_project: String,
    /// The canonical graph exactly as the engine published it.
    graph: Value,
    #[serde(default = "browser::wire_format")]
    graph_format: String,
    #[serde(default)]
    limit: Option<i64>,
}

/// The vocabulary a plan republishes.
///
/// The engine owns it and publishes it with the graph. It is republished here
/// because it is what makes the write commands usable from `ds capabilities`
/// alone: `--delivery`, `--review` and `--closeout` take their values from it,
/// and a client-side copy of the list is exactly the duplication that once
/// offered `task` for a node that was a milestone.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Vocabulary {
    pub delivery: Vec<String>,
    pub review: Vec<String>,
    pub closeout: Vec<String>,
    pub priorities: Vec<String>,
    pub task_types: Vec<String>,
    pub placements: Vec<String>,
    pub scheduling_modes: Vec<String>,
    /// Task proposals: the admission states and the four bounds
    /// (task-proposals.md). Empty / 0 on a graph from an engine that
    /// predates them — the CLI then refuses nothing locally and the server
    /// still bounds.
    pub admission_states: Vec<String>,
    pub max_estimated_hours: i64,
    pub max_estimated_days: i64,
    pub max_hours_per_entry: i64,
    pub max_hours_log_entries: i64,
    pub max_assignees: i64,
    /// The correspondence vocabularies (`ds pm party`, `ds pm record`,
    /// `ds pm task block`). Each is omitted, not emptied, when the server
    /// declared none — an empty list would tell the CLI the engine accepts
    /// nothing, which is the opposite of the truth on an older server.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub record_categories: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub record_states: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub record_directions: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub response_statuses: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocker_kinds: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocker_cleared_bys: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub party_kinds: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub party_roles: Vec<String>,
}

/// The `work.plan.read` answer, field for field as `readCliWorkPlan` produced
/// it — the names are camelCase because that answer is the contract a CLI and
/// an MCP client already read.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanReply {
    pub schema: &'static str,
    pub hierarchy: Hierarchy,
    pub project: String,
    pub revision: i64,
    pub today: String,
    pub dashboard: Dashboard,
    pub phases: Vec<Phase>,
    pub phase_total: usize,
    pub attention: Vec<Attention>,
    pub attention_total: usize,
    /// Proposals awaiting the PM, oldest first (task-proposals.md).
    pub proposals: Vec<Proposal>,
    pub proposals_total: usize,
    pub recent: Vec<RecentTask>,
    /// Every plan node, not the length of `recent` — what the browser reported
    /// so a capped list can say what it was capped from (cli-pm.ts:432).
    pub recent_total: usize,
    pub permissions: ProjectGraphPermissions,
    /// `null` on a graph written before the backend published the field model.
    /// Absent is not the same answer as empty, and neither is invented here.
    pub vocabulary: Option<Vocabulary>,
    /// The correspondence counters and the attention rows grouped by party.
    /// `null` when the server published no projection — an older ds-brain —
    /// which is not the same answer as "nothing is awaited".
    pub correspondence: Option<CorrespondencePlan>,
}

/// `YYYY-MM-DD`, shaped. The browser never checked because it minted the value
/// itself; the kernel takes it from a host and refuses a malformed one by
/// name, because every date comparison in the fold is a string comparison and
/// a malformed day would quietly mis-sort the whole plan.
fn valid_today(today: &str) -> Result<(), String> {
    let bytes = today.as_bytes();
    let shaped = bytes.len() == 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit);
    if shaped {
        Ok(())
    } else {
        Err(format!(
            "Project management refuses today `{today}`: a plan is read against a YYYY-MM-DD day."
        ))
    }
}

fn plan(request: PlanRequest) -> Result<PlanReply, String> {
    valid_today(&request.today)?;
    let limit = match request.limit {
        None => DEFAULT_PLAN_ROWS,
        Some(value) if (1..=MAX_PLAN_ROWS).contains(&value) => value,
        Some(value) => {
            return Err(format!(
                "Project management refuses limit `{value}`: a plan lists 1 through {MAX_PLAN_ROWS} rows."
            ));
        }
    };
    let graph = browser::graph(&request.graph, &request.ds_project, &request.graph_format)?;
    let day = request.today;

    let all_phases = projections::phases(&graph);
    let all_attention = projections::attention(&graph, &day);
    let all_proposals = projections::proposals(&graph);
    let (phase_total, attention_total, proposals_total) =
        (all_phases.len(), all_attention.len(), all_proposals.len());
    let bounded = limit as usize;

    Ok(PlanReply {
        schema: SCHEMA,
        hierarchy: hierarchy::hierarchy(&graph.tasks),
        project: graph.project_id.clone(),
        revision: graph.revision,
        dashboard: projections::dashboard(&graph, &day),
        phases: all_phases.into_iter().take(bounded).collect(),
        phase_total,
        attention: all_attention.into_iter().take(bounded).collect(),
        attention_total,
        proposals: all_proposals.into_iter().take(bounded).collect(),
        proposals_total,
        // The browser asked for `Math.min(limit, 24)`; the projection bounds
        // itself at 24 as well, so the two agree by construction.
        recent: projections::recently_updated(&graph, limit.min(24)),
        recent_total: graph.tasks.len(),
        permissions: graph.permissions,
        vocabulary: graph.field_model.as_ref().map(|model| Vocabulary {
            delivery: model.delivery_states.clone(),
            review: model.review_states.clone(),
            closeout: model.closeout_states.clone(),
            priorities: model.priorities.clone(),
            task_types: model.task_types.clone(),
            placements: model.placements.clone(),
            scheduling_modes: model.scheduling_modes.clone(),
            admission_states: model.admission_states.clone(),
            max_estimated_hours: model.max_estimated_hours,
            max_estimated_days: model.max_estimated_days,
            max_hours_per_entry: model.max_hours_per_entry,
            max_hours_log_entries: model.max_hours_log_entries,
            max_assignees: model.max_assignees,
            channels: model.channels.clone(),
            record_categories: model.record_categories.clone(),
            record_states: model.record_states.clone(),
            record_directions: model.record_directions.clone(),
            response_statuses: model.response_statuses.clone(),
            blocker_kinds: model.blocker_kinds.clone(),
            blocker_cleared_bys: model.blocker_cleared_bys.clone(),
            party_kinds: model.party_kinds.clone(),
            party_roles: model.party_roles.clone(),
        }),
        correspondence: projections::correspondence(&graph, limit),
        today: day,
    })
}

/// Evaluate one request. `action: "plan"` folds a canonical graph into the
/// Project Work plan: the dashboard, the phases, the attention list, the
/// recently updated tasks, the permissions and the vocabulary.
/// `task.list`: the Table and Board's filter controls, answered by the same
/// rule as `ds pm task list` ([`reads::task_list_ids`]). Choice tokens are the
/// CLI's own (`--placement`, `--geometry`); an unknown one is refused.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskListRequest {
    #[allow(dead_code)]
    schema: String,
    #[allow(dead_code)]
    action: String,
    #[serde(default)]
    ds_project: String,
    graph: Value,
    #[serde(default = "browser::wire_format")]
    graph_format: String,
    #[serde(default)]
    query: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    discipline: Option<String>,
    #[serde(default)]
    placement: Option<String>,
    #[serde(default)]
    blocked_on_correspondence: bool,
    #[serde(default)]
    geometry: Option<String>,
}

#[derive(Serialize)]
struct TaskListIdsReply {
    schema: &'static str,
    project: String,
    revision: i64,
    total: usize,
    ids: Vec<String>,
}

fn task_list_ids(request: TaskListRequest) -> Result<TaskListIdsReply, String> {
    use reads::{GeometryFilter, PlacementFilter, TaskListFilter};
    let placement = match request.placement.as_deref() {
        None | Some("any") => PlacementFilter::Any,
        Some("wbs") => PlacementFilter::Wbs,
        Some("inbox") => PlacementFilter::Inbox,
        Some(other) => {
            return Err(format!(
                "Unsupported placement `{other}`: any, wbs or inbox."
            ))
        }
    };
    let geometry = match request.geometry.as_deref() {
        None | Some("any") => GeometryFilter::Any,
        Some("with") => GeometryFilter::With,
        Some("without") => GeometryFilter::Without,
        Some(other) => {
            return Err(format!(
                "Unsupported geometry `{other}`: any, with or without."
            ))
        }
    };
    let graph = browser::graph(&request.graph, &request.ds_project, &request.graph_format)?;
    let ids = reads::task_list_ids(
        &graph,
        &TaskListFilter {
            query: request.query,
            state: request.state,
            assignee: request.assignee,
            discipline: request.discipline,
            placement,
            blocked_on_correspondence: request.blocked_on_correspondence,
            geometry,
            ..TaskListFilter::default()
        },
    );
    Ok(TaskListIdsReply {
        schema: SCHEMA,
        project: graph.project_id.clone(),
        revision: graph.revision,
        total: ids.len(),
        ids,
    })
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("Project management request exceeds its bound.".into());
    }
    let head: Head = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid project management request: {error}"))?;
    if head.schema != SCHEMA {
        return Err(format!(
            "Unsupported project management schema `{}`.",
            head.schema
        ));
    }
    match head.action.as_str() {
        "overview" => {
            let request: overview::Input = serde_json::from_slice(input)
                .map_err(|error| format!("Invalid project overview request: {error}"))?;
            serde_json::to_string(&overview::fold(request)?).map_err(|error| error.to_string())
        }
        "plan" => {
            let request: PlanRequest = serde_json::from_slice(input)
                .map_err(|error| format!("Invalid project management request: {error}"))?;
            serde_json::to_string(&plan(request)?).map_err(|error| error.to_string())
        }
        "hierarchy" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Request {
                schema: String,
                action: String,
                tasks: Vec<hierarchy::TimelineTask>,
            }
            let request: Request = serde_json::from_slice(input).map_err(|e| e.to_string())?;
            let _ = (&request.schema, &request.action);
            serde_json::to_string(&hierarchy::timeline_hierarchy(&request.tasks))
                .map_err(|e| e.to_string())
        }
        "task.list" => {
            let request: TaskListRequest = serde_json::from_slice(input)
                .map_err(|error| format!("Invalid project management request: {error}"))?;
            serde_json::to_string(&task_list_ids(request)?).map_err(|error| error.to_string())
        }
        "task.create" | "task.update" | "task.assign" | "task.respond" | "task.sort-key"
        | "task.owner-write" | "command.encode" => wasm_writes::evaluate(input),
        "attachment.start" | "attachment.resume" => attachment_upload::evaluate(input),
        other => Err(format!("Unsupported project management action `{other}`.")),
    }
}

#[cfg(test)]
#[path = "project_management_tests.rs"]
mod tests;
