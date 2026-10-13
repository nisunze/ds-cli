//! `ds pm task read` — one work item, with everything hanging off it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{json, Value};

use crate::{LANE_ARG, TASK_ARG};

const TIMELINE_ARG: Arg = Arg::switch(
    "timeline",
    "Also read the task's history in time order: records, attachments, state changes, assignments, blockers set and cleared.",
);

pub static COMMAND: Command = Command {
    id: "pm.task.read",
    path: &["pm", "task", "read"],
    contract: 1,
    summary: "Read one work item with its dependencies and open residuals.",
    purpose: "\
The whole canonical task: schedule, delivery, review and closeout state, who \
holds it, who has been asked to take it, what it depends on, what is still \
outstanding against it, and the records that reference it. Read this before \
any write — the update, assign and respond commands all act on what is here. \
Headless: the named project of the signed-in native credential, no window.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TASK_ARG, TIMELINE_ARG, Arg::switch("offline", "Read this exact retained task/timeline selection without network or freshness checks; captured permissions may have changed."), LANE_ARG, crate::PROJECT_ARG],
    output: "\
`task` with the canonical fields — including `awaitingCorrespondence`, \
`blockedRecordIds` and every `correspondenceBlockers` entry with who set \
it and how it cleared — `dependencies`, `residuals` (open first), \
`episodes`, `records` referencing it, `recordsTruncated`, `recordsNextCursor`, \
`recordTotalKind` (exact or lower_bound), and a `*Total` for each bounded related \
collection; plus the project's `permissions`, graph `revision`, and `link`. \
With --timeline, `timeline.entries` in time order with `total`, `truncated` \
and per-source counts. `retained` reports capture-only permission/source freshness, independent source revisions and known local invalidation. --offline never acquires missing data.",
    examples: &[Example {
        command: "ds pm task read --task T-0007 --output json --project <exact-id>",
        note: "`.data.task.assignmentOpen` tells you whether respond is available.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<25>(&[
        crate::TASK_NOT_FOUND,
        Refusal {code:"pm_offline_acquisition_required",when:"no admitted retained task matches the protected account/project and exact task/timeline selection, or access was denied",remedy:"read this exact task and timeline selection online once under the same account"},
        Refusal {code:"pm_retained_unavailable",when:"the bounded retained task receipts cannot be verified or persisted",remedy:"inspect owner-only local DS data and repeat the exact online task read"},
        Refusal {code:"project_context_changed",when:"authority or the local mutation generation changed during task acquisition",remedy:"repeat the entire intentional task read under the intended account"},
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "subtask",
        "sub-task",
        "deadline",
        "due",
        "date",
        "milestone",
        "wbs",
        "progress",
        "assignee",
        "timeline",
        "history",
        "blocked on",
        "correspondence",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let task_id = inputs.require("task")?;
    let raw = ds_cli_auth::pm_graph_source::read_task(
        inputs.value("lane").unwrap_or("stable"),
        project,
        task_id,
        inputs.switch("timeline"),
        inputs.switch("offline"),
    )?;
    fold_capture(project, task_id, raw)
}
fn fold_capture(project: &str, task_id: &str, raw: Value) -> Result<Value, Failure> {
    let graph = ds_command_kernel::project_management::decode_project_graph(&raw["graph"], project);
    let context = &raw["records_context"];
    let records = ds_command_kernel::project_management::reads::decode_context_records(context);
    let reply = ds_command_kernel::project_management::reads::task_read(&graph, task_id, &records)
        .ok_or_else(|| {
            Failure::invalid(
                "task_not_found",
                "The retained task is absent from its captured plan.",
            )
            .remedy(crate::TASK_NOT_FOUND.remedy)
        })?;
    let mut data = crate::data(&reply)?;
    let truncated = context["truncated_by_collection"]["pm_records"] == true;
    data["recordsTruncated"] = json!(truncated);
    data["recordsNextCursor"] = context["next_cursors"]["pm_records"].clone();
    data["recordTotalKind"] = json!(if truncated { "lower_bound" } else { "exact" });
    if let Some(receipt) = raw.get("timeline_receipt") {
        let timeline =
            ds_command_kernel::project_management::correspondence::timeline(&receipt["timeline"]);
        data["timeline"] = crate::data(&timeline)?;
    }
    data["retained"] = raw["retained"].clone();
    Ok(data)
}

pub fn render(data: &Value) -> String {
    let task = &data["task"];
    let mut out = format!(
        "{} {}\n  {} · review {} · closeout {} · {}% · {}\n",
        task["wbs"].as_str().unwrap_or("—"),
        task["title"].as_str().unwrap_or("?"),
        task["delivery"].as_str().unwrap_or("—"),
        task["review"].as_str().unwrap_or("—"),
        task["closeout"].as_str().unwrap_or("—"),
        task["progress"].as_u64().unwrap_or(0),
        task["responsible"].as_str().unwrap_or("unassigned"),
    );
    if let Some(schedule) = task["start"].as_str() {
        out.push_str(&format!(
            "  {schedule} → {}\n",
            task["finish"].as_str().unwrap_or("—")
        ));
    }
    if let Some(asked) = task["requested"].as_array().filter(|list| !list.is_empty()) {
        let people: Vec<&str> = asked.iter().filter_map(Value::as_str).collect();
        out.push_str(&format!("  asked: {}\n", people.join(", ")));
    }
    for (label, key) in [
        ("depends on", "dependencies"),
        ("residuals", "residuals"),
        ("records", "records"),
    ] {
        let total_key = match key {
            "dependencies" => "dependencyTotal",
            "residuals" => "residualTotal",
            "records" => "recordTotal",
            _ => key,
        };
        let count = data[total_key]
            .as_u64()
            .unwrap_or_else(|| data[key].as_array().map_or(0, Vec::len) as u64);
        if count > 0 || (key == "records" && data["recordTotalKind"] == "lower_bound") {
            let qualifier = if key == "records" && data["recordTotalKind"] == "lower_bound" {
                "at least "
            } else {
                ""
            };
            out.push_str(&format!("  {label}: {qualifier}{count}\n"));
        }
    }
    if data["retained"]["mode"] == "offline_capture" {
        out.push_str("  retained capture ? current source and permissions unknown");
        if data["retained"]["known_invalidated"] == true {
            out.push_str(" ? known local changes");
        }
        out.push('\n');
    }
    if let Some(link) = data["link"].as_str() {
        out.push_str(&format!("  {link}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn captured_task(truncated: bool) -> Value {
        json!({"graph":{"project_id":"project-a","revision":7,"tasks":[{"id":"task-7","data":{"title":"Review"}}]},
            "records_context":{"records":[{"id":"r","data":{"subject":"Evidence","related_task_ids":["task-7"]}}],"truncated_by_collection":{"pm_records":truncated},"next_cursors":if truncated {json!({"pm_records":"boundary"})} else {json!({})}},
            "retained":{"mode":"offline_capture","known_invalidated":true,"source_freshness":"unknown","permission_freshness":"captured_only","source_revisions":{"graph":7,"records_context":null,"timeline":8}}})
    }
    #[test]
    fn task_read_projection_preserves_exact_or_lower_bound_and_refuses_missing_selection() {
        for truncated in [false, true] {
            let reply = fold_capture("project-a", "task-7", captured_task(truncated)).unwrap();
            assert_eq!(reply["recordTotal"], 1);
            assert_eq!(reply["recordsTruncated"], truncated);
            assert_eq!(
                reply["recordTotalKind"],
                if truncated { "lower_bound" } else { "exact" }
            );
            assert_eq!(
                reply["recordsNextCursor"],
                if truncated {
                    json!("boundary")
                } else {
                    Value::Null
                }
            );
            assert_eq!(render(&reply).contains("records: at least 1"), truncated);
        }
        assert_eq!(
            fold_capture("project-a", "missing", captured_task(false))
                .unwrap_err()
                .code(),
            "task_not_found"
        );
    }
    #[test]
    fn task_offline_is_explicit_and_projection_retains_partial_capture_qualifications() {
        assert!(COMMAND.args.iter().any(|arg| arg.name == "offline"));
        assert!(COMMAND
            .refusals
            .iter()
            .any(|r| r.code == "pm_offline_acquisition_required"));
        let mut raw = captured_task(true);
        raw["records_context"]["records"] = json!([]);
        raw["timeline_receipt"] =
            json!({"timeline":{"task_id":"task-7","entries":[],"total":0,"truncated":true}});
        let reply = fold_capture("project-a", "task-7", raw).unwrap();
        assert_eq!(reply["recordsTruncated"], true);
        assert_eq!(reply["recordTotalKind"], "lower_bound");
        assert_eq!(reply["recordsNextCursor"], "boundary");
        assert_eq!(reply["recordTotal"], 0);
        assert!(render(&reply).contains("records: at least 0"));
        assert_eq!(reply["timeline"]["truncated"], true);
        assert_eq!(
            reply["retained"]["source_revisions"]["records_context"],
            Value::Null
        );
        assert!(
            render(&reply).contains("current source and permissions unknown ? known local changes")
        );
    }
}
