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
//! **Translated, not improved.** The browser's rules are reproduced as they
//! were, including the ones nothing on the wire declares: the schedule state
//! inferred from date presence, the 480-minute working day, closeout inferred
//! from delivery, the legacy `done`/`doing` status words, and a residual that
//! stays review-required until somebody classifies it. Each carries the
//! `wire.ts` or `views.ts` line it came from. Where the browser is arguably
//! wrong — an unscheduled task counting as past its planned finish because an
//! empty date string sorts before every real one — the kernel is wrong in
//! exactly the same way, on purpose, and says so at the site.
//!
//! **Pure by the crate's contract**: no clock, no IO, no network. `today` is an
//! input, and the caller that has a clock supplies it.
//!
//! Since 2026-09-20 the rest of `ds pm` is decided here too: [`reads`] holds
//! the task and record projections (`task list|read`, `record list|read`) and
//! [`writes`] the validated project commands (`task create|update|assign|
//! respond`) and the fold of the engine's answer. Same doctrine — the browser's
//! rules, translated as they were, reachable without a window.

mod hierarchy;
pub mod projections;
pub mod reads;
pub mod wire;
pub mod writes;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use hierarchy::{ordered_rows, OrderedRow};
pub use projections::{
    effort, proposals, Attention, AttentionKind, Dashboard, Phase, Proposal, RecentTask, TaskEffort,
};
pub use wire::{decode_project_graph, CanonicalProjectGraph, ProjectGraphPermissions};

/// The request/reply schema of this module's one action.
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
}

/// The `work.plan.read` answer, field for field as `readCliWorkPlan` produced
/// it — the names are camelCase because that answer is the contract a CLI and
/// an MCP client already read.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanReply {
    pub schema: &'static str,
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
    let graph = decode_project_graph(&request.graph, &request.ds_project);
    let day = request.today;

    let all_phases = projections::phases(&graph);
    let all_attention = projections::attention(&graph, &day);
    let all_proposals = projections::proposals(&graph);
    let (phase_total, attention_total, proposals_total) =
        (all_phases.len(), all_attention.len(), all_proposals.len());
    let bounded = limit as usize;

    Ok(PlanReply {
        schema: SCHEMA,
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
        }),
        today: day,
    })
}

/// Evaluate one request. `action: "plan"` folds a canonical graph into the
/// Project Work plan: the dashboard, the phases, the attention list, the
/// recently updated tasks, the permissions and the vocabulary.
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
        "plan" => {
            let request: PlanRequest = serde_json::from_slice(input)
                .map_err(|error| format!("Invalid project management request: {error}"))?;
            serde_json::to_string(&plan(request)?).map_err(|error| error.to_string())
        }
        other => Err(format!("Unsupported project management action `{other}`.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(request: Value) -> Result<Value, String> {
        evaluate(&serde_json::to_vec(&request).expect("request serialises"))
            .map(|reply| serde_json::from_str(&reply).expect("reply parses"))
    }

    /// 2026-09-18T00:00:00Z. Every stamp in the fixture is stated as an offset
    /// from this one day, so a reader can check the day counts by eye.
    const TODAY_MS: i64 = 1_789_689_600_000;
    const DAY: i64 = 86_400_000;

    /// One small but complete project: a finished survey, a blocked design
    /// with two children, a milestone, an unfiled inbox task carrying two open
    /// residuals, a deleted row and a nameless row. It exercises all four
    /// projections at once, which is the point — the plan is one fold and its
    /// parts have to agree with each other.
    fn fixture() -> Value {
        json!({
            "project_id": "ds-plan-fixture",
            "revision": 42,
            "schedule_revision": 7,
            "permissions": {"view": true, "comment": true, "contribute": true, "edit_schedule": true},
            "field_model": {
                "delivery_states": ["not_started", "in_progress", "complete"],
                "review_states": ["not_required", "under_review"],
                "closeout_states": ["open", "closed"],
                "priorities": ["low", "normal", "high", "critical"],
                "task_types": ["task", "milestone"],
                "scheduling_modes": ["manual", "auto"],
                "placements": ["wbs", "inbox"],
                "dependency_types": ["FS", "SS"],
                "residual_kinds": ["blocker", "other"],
                "residual_statuses": ["open", "resolved"],
                "scope_classifications": ["within_original_scope"],
                "work_classifications": ["bounded_residual", "scope_review_required"],
                "max_assignees": 5,
                "max_batch_commands": 200
            },
            "tasks": [
                {"id": "survey-1",
                 "data": {"title": "Route survey", "discipline": "Survey", "sort_key": "0100",
                          "start_date": "2026-09-01", "finish_date": "2026-09-10",
                          "delivery_state": "complete"},
                 "metadata": {"updated_at": TODAY_MS - 20 * DAY, "updated_by": "ops@ds.rw"}},
                {"id": "design-1",
                 "data": {"title": "LV design", "discipline": "Design", "sort_key": "0200",
                          "start_date": "2026-09-05", "finish_date": "2026-09-15", "status": "doing"},
                 "computed": {"open_blocker_count": 2, "descendant_blocker_count": 1},
                 "metadata": {"updated_at": TODAY_MS - 3 * DAY, "updated_by": "lead@ds.rw"}},
                {"id": "design-1a",
                 "data": {"title": "Pole schedule", "discipline": "Design", "sort_key": "0210",
                          "parent_task_id": "design-1", "finish_date": "2026-09-12",
                          "delivery_state": "paused", "progress": 0.5},
                 "metadata": {"updated_at": TODAY_MS - DAY}},
                {"id": "review-1",
                 "data": {"title": "Client review", "discipline": "Design", "sort_key": "0220",
                          "parent_task_id": "design-1", "start_date": "2026-09-16",
                          "finish_date": "2026-09-30", "review_state": "under_review"},
                 "metadata": {"updated_at": TODAY_MS}},
                {"id": "handover",
                 "data": {"title": "Handover", "discipline": "Design", "sort_key": "0300",
                          "type": "milestone", "finish_date": "2026-09-30"},
                 "metadata": {"updated_at": TODAY_MS - 5 * DAY}},
                {"id": "inbox-1",
                 "data": {"title": "Unfiled query", "sort_key": "0400", "placement": "inbox"},
                 "metadata": {"updated_at": 1}},
                {"id": "gone", "data": {"title": "Removed"}, "is_deleted": true},
                {"id": "nameless", "data": {"description": "no title"}}
            ],
            "residual_items": [
                {"id": "r1", "task_id": "inbox-1", "title": "Await client drawing",
                 "status": "awaiting_response", "blocks_acceptance": true, "blocks_closeout": true},
                {"id": "r2", "task_id": "design-1a", "title": "Signature", "status": "resolved"},
                {"id": "r3", "task_id": "inbox-1", "title": "Second query", "kind": "blocker"}
            ]
        })
    }

    fn plan_of(limit: Option<i64>) -> Value {
        let mut request = json!({
            "schema": SCHEMA, "action": "plan", "today": "2026-09-18",
            "ds_project": "ds-session-project", "graph": fixture(),
        });
        if let Some(limit) = limit {
            request["limit"] = json!(limit);
        }
        run(request).expect("plan")
    }

    #[test]
    fn the_schema_and_the_action_are_each_refused_by_name() {
        // A host that reaches the wrong module, or asks for a verb this one
        // does not have, must be told WHICH of the two it got wrong.
        let error = run(json!({"schema": "ds.other/v1", "action": "plan"})).unwrap_err();
        assert!(
            error.starts_with("Unsupported project management schema `ds.other/v1`"),
            "{error}"
        );
        let error = run(json!({"schema": SCHEMA, "action": "sweep"})).unwrap_err();
        assert!(
            error.starts_with("Unsupported project management action `sweep`"),
            "{error}"
        );
        let error = run(json!({"schema": SCHEMA})).unwrap_err();
        assert!(
            error.starts_with("Unsupported project management action ``"),
            "{error}"
        );
        assert!(evaluate(b"not json")
            .unwrap_err()
            .starts_with("Invalid project management request"));
        assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1])
            .unwrap_err()
            .starts_with("Project management request exceeds its bound"));
    }

    #[test]
    fn an_unknown_request_member_is_a_contract_break() {
        // The browser ignored what it did not recognise. The kernel cannot:
        // a member nobody reads is a caller believing something is happening.
        let error = run(json!({
            "schema": SCHEMA, "action": "plan", "today": "2026-09-18",
            "graph": {}, "page": 2,
        }))
        .unwrap_err();
        assert!(
            error.starts_with("Invalid project management request"),
            "{error}"
        );
    }

    #[test]
    fn a_malformed_day_and_an_out_of_range_limit_are_each_refused_by_name() {
        // Every date comparison in the fold is a STRING comparison, so a day
        // that is not `YYYY-MM-DD` would quietly mis-sort the whole plan
        // rather than fail. The kernel takes `today` from a host, so it checks.
        let error = run(json!({
            "schema": SCHEMA, "action": "plan", "today": "18/09/2026", "graph": {},
        }))
        .unwrap_err();
        assert!(
            error.starts_with("Project management refuses today `18/09/2026`"),
            "{error}"
        );
        for limit in [0, 101] {
            let error = run(json!({
                "schema": SCHEMA, "action": "plan", "today": "2026-09-18",
                "graph": {}, "limit": limit,
            }))
            .unwrap_err();
            assert!(
                error.starts_with(&format!("Project management refuses limit `{limit}`")),
                "{error}"
            );
        }
    }

    #[test]
    fn the_plan_folds_one_graph_into_four_agreeing_projections() {
        // The frozen fixture. Each number below is checkable against the
        // fixture by hand, and the four projections have to add up against one
        // another — that is what makes this one fold rather than four reports.
        let plan = plan_of(None);
        assert_eq!(plan["schema"], SCHEMA);
        assert_eq!(plan["project"], "ds-plan-fixture");
        assert_eq!(plan["revision"], 42);
        assert_eq!(plan["today"], "2026-09-18");

        let dashboard = &plan["dashboard"];
        // Six live nodes: the deleted row and the nameless row never arrive.
        assert_eq!(dashboard["planNodes"], 6);
        assert_eq!(dashboard["totalTasks"], 5);
        assert_eq!(dashboard["milestones"], 1);
        assert_eq!(dashboard["primaryComplete"], 1);
        assert_eq!(dashboard["underReview"], 1);
        assert_eq!(dashboard["inProgress"], 1);
        assert_eq!(dashboard["blocked"], 0);
        assert_eq!(dashboard["paused"], 1);
        assert_eq!(dashboard["blockedOrPaused"], 1);
        // 1 (complete, undeclared) + 0.5 (declared) over six plan nodes.
        assert_eq!(dashboard["overallProgress"], 0.25);
        assert_eq!(dashboard["openResiduals"], 2);
        assert_eq!(dashboard["acceptanceBlocked"], 1);
        assert_eq!(dashboard["closeoutBlocked"], 1);
        assert_eq!(dashboard["residualTaskCount"], 1);

        // Biggest discipline first, the nameless bucket last, whatever its size.
        assert_eq!(plan["phaseTotal"], 3);
        let phases: Vec<(&str, u64)> = plan["phases"]
            .as_array()
            .expect("phases")
            .iter()
            .map(|phase| {
                (
                    phase["discipline"].as_str().unwrap_or_default(),
                    phase["total"].as_u64().unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(phases, vec![("Design", 4), ("Survey", 1), ("", 1)]);
        assert_eq!(plan["phases"][1]["progress"], 1.0);
        assert!(plan["phases"][2]["unassigned"].as_bool().unwrap_or(false));

        // One row per task that needs a decision, worst reason first, each
        // carrying the WBS the Table shows and the magnitude to act on.
        assert_eq!(plan["attentionTotal"], 4);
        assert_eq!(
            plan["attention"],
            json!([
                {"taskId": "design-1", "title": "LV design", "kind": "blocked", "wbs": "2",
                 "blockerCount": 3, "daysLate": 3, "hoursOver": 0.0, "daysSinceChange": 3, "residualCount": 0},
                {"taskId": "design-1a", "title": "Pole schedule", "kind": "late", "wbs": "2.1",
                 "blockerCount": 0, "daysLate": 6, "hoursOver": 0.0, "daysSinceChange": 1, "residualCount": 0},
                {"taskId": "review-1", "title": "Client review", "kind": "review", "wbs": "2.2",
                 "blockerCount": 0, "daysLate": 0, "hoursOver": 0.0, "daysSinceChange": 0, "residualCount": 0},
                {"taskId": "inbox-1", "title": "Unfiled query", "kind": "residual", "wbs": "Inbox 1",
                 "blockerCount": 0, "daysLate": 0, "hoursOver": 0.0, "daysSinceChange": 0, "residualCount": 2},
            ])
        );
        // The fixture proposes nothing and logs nothing: the proposal rows
        // and counters are present and empty, never absent.
        assert_eq!(plan["proposalsTotal"], 0);
        assert_eq!(plan["proposals"], json!([]));
        assert_eq!(dashboard["proposalsPending"], 0);
        assert_eq!(dashboard["overEstimate"], 0);
        assert_eq!(plan["vocabulary"]["admissionStates"], json!([]));

        // Recent is the canonical update stamp, newest first, and never an
        // actor the graph did not name.
        assert_eq!(plan["recentTotal"], 6);
        let recent: Vec<&str> = plan["recent"]
            .as_array()
            .expect("recent")
            .iter()
            .map(|row| row["taskId"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            recent,
            vec![
                "review-1",
                "design-1a",
                "design-1",
                "handover",
                "survey-1",
                "inbox-1"
            ]
        );
        assert_eq!(plan["recent"][2]["updatedBy"], "lead@ds.rw");
        assert!(plan["recent"][0].get("updatedBy").is_none());

        assert_eq!(plan["permissions"]["editSchedule"], true);
        assert_eq!(plan["permissions"]["admin"], false);
        assert_eq!(plan["vocabulary"]["maxAssignees"], 5);
        assert_eq!(plan["vocabulary"]["placements"], json!(["wbs", "inbox"]));
        // The plan republishes only the eight vocabularies the write commands
        // take their values from; the residual lists are not among them.
        assert!(plan["vocabulary"].get("residualKinds").is_none());
    }

    #[test]
    fn a_capped_list_still_reports_how_much_it_was_capped_from() {
        // A limit bounds what is painted, never what is counted — otherwise
        // an operator reading "1 needs attention" would act on a lie.
        let plan = plan_of(Some(1));
        assert_eq!(plan["phases"].as_array().map(Vec::len), Some(1));
        assert_eq!(plan["phaseTotal"], 3);
        assert_eq!(plan["attention"].as_array().map(Vec::len), Some(1));
        assert_eq!(plan["attentionTotal"], 4);
        assert_eq!(plan["recent"].as_array().map(Vec::len), Some(1));
        assert_eq!(plan["recentTotal"], 6);
    }

    #[test]
    fn a_graph_with_no_field_model_publishes_no_vocabulary_rather_than_an_empty_one() {
        // Absent and empty are different answers. An empty list would tell the
        // CLI the engine accepts nothing, which is the opposite of the truth.
        let plan = run(json!({
            "schema": SCHEMA, "action": "plan", "today": "2026-09-18",
            "ds_project": "ds-session-project", "graph": {"revision": 3},
        }))
        .expect("plan");
        assert_eq!(plan["vocabulary"], Value::Null);
        // And a graph that names no project of its own is the host's.
        assert_eq!(plan["project"], "ds-session-project");
        assert_eq!(plan["dashboard"]["planNodes"], 0);
        assert_eq!(plan["dashboard"]["overallProgress"], 0.0);
    }
}
