//! `ds pm task read` — one work item, with everything hanging off it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use serde_json::{Value, json};

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
    args: &[TASK_ARG, TIMELINE_ARG, LANE_ARG, crate::PROJECT_ARG],
    output: "\
`task` with the canonical fields — including `awaitingCorrespondence`, \
`blockedRecordIds` and every `correspondenceBlockers` entry with who set \
it and how it cleared — `dependencies`, `residuals` (open first), \
`episodes`, `records` referencing it, `recordsTruncated`, `recordsNextCursor`, \
`recordTotalKind` (exact or lower_bound), and a `*Total` for each bounded related \
collection; plus the project's `permissions`, graph `revision`, and `link`. \
With --timeline, `timeline.entries` in time order with `total`, `truncated` \
and per-source counts.",
    examples: &[Example {
        command: "ds pm task read --task T-0007 --output json --project <exact-id>",
        note: "`.data.task.assignmentOpen` tells you whether respond is available.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<22>(&[crate::TASK_NOT_FOUND]),
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
    let task_id = inputs.require("task")?;
    let lane = inputs.value("lane").unwrap_or("stable");
    let read = crate::graph(lane, inputs.require("project")?)?;
    read_selected(
        &read,
        task_id,
        || crate::records(lane, inputs.require("project")?, task_id),
        |data| {
            if inputs.switch("timeline") {
                let report = crate::correspondence(
                    lane,
                    inputs.require("project")?,
                    &Action::TaskRead {
                        task_id: task_id.to_owned(),
                        timeline: true,
                    },
                )?;
                same_reader(&read.uid, report.identity().uid())?;
                let timeline = ds_command_kernel::project_management::correspondence::timeline(
                    &report.into_result()["timeline"],
                );
                data["timeline"] = crate::data(&timeline)?;
            }
            Ok(())
        },
    )
}

/// The acquisition runs only after the graph proves a selected task exists.
fn read_selected(
    read: &crate::Graph,
    task_id: &str,
    acquire_records: impl FnOnce() -> Result<crate::TaskRecords, Failure>,
    add_timeline: impl FnOnce(&mut Value) -> Result<(), Failure>,
) -> Result<Value, Failure> {
    if !read.graph.tasks.iter().any(|task| task.id == task_id) {
        return Err(Failure::invalid(
            crate::TASK_NOT_FOUND.code,
            format!("No task {task_id} in this project's plan."),
        )
        .detail(json!({ "task": task_id, "project": read.project_id }))
        .remedy(crate::TASK_NOT_FOUND.remedy)
        .next("ds pm task list"));
    }
    let records = acquire_records()?;
    same_reader(&read.uid, &records.uid)?;
    let reply = ds_command_kernel::project_management::reads::task_read(
        &read.graph,
        task_id,
        &records.rows,
    )
    .expect("selected task was checked against this unchanged graph");
    let mut data = crate::data(&reply)?;
    data["recordsTruncated"] = json!(records.truncated);
    data["recordsNextCursor"] = json!(records.next_cursor);
    data["recordTotalKind"] = json!(if records.truncated {
        "lower_bound"
    } else {
        "exact"
    });
    add_timeline(&mut data)?;
    Ok(data)
}

/// Separately captured reads cannot be composed across an account change.
fn same_reader(expected: &str, observed: &str) -> Result<(), Failure> {
    if expected != observed {
        return Err(Failure::conflict(
            "project_context_stale",
            "The account changed between task reads.",
        )
        .remedy("retry the entire task read under the intended account"));
    }
    Ok(())
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
    if let Some(link) = data["link"].as_str() {
        out.push_str(&format!("  {link}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn held_graph() -> crate::Graph {
        let raw = json!({"project_id":"project-a","revision":7,"tasks":[{"id":"task-7","data":{"title":"Review"}}]});
        crate::Graph {
            lane: "stable",
            project_id: "project-a".into(),
            uid: "uid".into(),
            graph: ds_command_kernel::project_management::decode_project_graph(&raw, "project-a"),
            raw,
        }
    }

    #[test]
    fn task_records_missing_selection_consumes_no_context_or_timeline() {
        let reads = Cell::new(0);
        let failure = read_selected(
            &held_graph(),
            "missing",
            || {
                reads.set(reads.get() + 1);
                panic!("unused context")
            },
            |_| panic!("unused timeline"),
        )
        .unwrap_err();
        assert_eq!(failure.code(), "task_not_found");
        assert_eq!(reads.get(), 0);
    }

    #[test]
    fn task_records_partial_empty_scan_is_visible_and_failed_context_does_not_read_timeline() {
        let partial = read_selected(
            &held_graph(),
            "task-7",
            || {
                Ok(crate::TaskRecords {
                    uid: "uid".into(),
                    rows: vec![],
                    truncated: true,
                    next_cursor: Some("scan-boundary".into()),
                })
            },
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(partial["recordTotal"], 0);
        assert_eq!(partial["recordTotalKind"], "lower_bound");
        assert!(render(&partial).contains("records: at least 0"));
        let failure = read_selected(
            &held_graph(),
            "task-7",
            || Err(Failure::unauthorized("auth_rejected", "Denied")),
            |_| panic!("denied context cannot read timeline"),
        )
        .unwrap_err();
        assert_eq!(failure.code(), "auth_rejected");
    }

    #[test]
    fn task_records_account_change_refuses_before_projection_or_timeline() {
        let failure = read_selected(
            &held_graph(),
            "task-7",
            || {
                Ok(crate::TaskRecords {
                    uid: "another-user".into(),
                    rows: vec![],
                    truncated: false,
                    next_cursor: None,
                })
            },
            |_| panic!("changed account cannot read timeline"),
        )
        .unwrap_err();
        assert_eq!(failure.code(), "project_context_stale");
        assert!(same_reader("uid", "uid").is_ok());
        assert_eq!(
            same_reader("uid", "another-user").unwrap_err().code(),
            "project_context_stale"
        );
        assert!(
            COMMAND
                .refusals
                .iter()
                .any(|refusal| refusal.code == "project_context_stale")
        );
    }

    #[test]
    fn task_records_selected_once_reports_exact_or_lower_bound_without_invented_total() {
        for truncated in [false, true] {
            let reads = Cell::new(0);
            let result=read_selected(&held_graph(),"task-7",|| {
                reads.set(reads.get()+1);
                Ok(crate::TaskRecords {uid:"uid".into(),rows:ds_command_kernel::project_management::reads::decode_context_records(&json!({"records":[{"id":"record-1","data":{"subject":"Review evidence","related_task_ids":["task-7"]}}]})),truncated,next_cursor:truncated.then(||"record-1".into())})
            }, |_| Ok(())).unwrap();
            assert_eq!(reads.get(), 1);
            assert_eq!(result["recordTotal"], 1);
            assert_eq!(result["recordsTruncated"], truncated);
            assert_eq!(
                result["recordTotalKind"],
                if truncated { "lower_bound" } else { "exact" }
            );
            assert_eq!(
                result["recordsNextCursor"],
                if truncated {
                    json!("record-1")
                } else {
                    Value::Null
                }
            );
            assert_eq!(render(&result).contains("records: at least 1"), truncated);
        }
    }
}
