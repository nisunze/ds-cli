//! `ds pm task geometry clear` — remove a task's geometry and the object
//! links that traced it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_management::writes::{self, UpdateTask};
use serde_json::{Map, Value, json};

use crate::{LANE_ARG, TASK_ARG};

pub static COMMAND: Command = Command {
    id: "pm.task.geometry.clear",
    path: &["pm", "task", "geometry", "clear"],
    contract: 1,
    summary: "Clear a task's geometry and the DS Grid object links behind it.",
    purpose: "\
Removes the task's top-level geometry and every dsgrid_structure or \
dsgrid_alignment link — the trace of a geometry that no longer exists. Links \
a person attached by hand (a survey entry, a record, a transformer) stay. One \
revision; a task with nothing to clear is answered, not refused.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TASK_ARG, LANE_ARG],
    output: "\
`taskId`, `committedRevision`, `warnings`, `link`, and `cleared` — whether a \
geometry was removed and how many object links went with it.",
    examples: &[Example {
        command: "ds pm task geometry clear --task T4 --yes",
        note: "Without --yes dispatch refuses before anything is sent.",
        runnable: false,
    }],
    refusals: &crate::write_refusals::<22>(&[]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "task geometry",
        "remove geometry",
        "unlink structures",
        "map",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let task_id = inputs.require("task")?.to_owned();
    let lane = inputs.value("lane").unwrap_or("stable");
    let read = crate::graph(lane)?;
    let task = read
        .raw_task(&task_id)
        .ok_or_else(|| crate::refused(writes::Refusal::TaskNotFound(task_id.clone())))?;
    let had_geometry = task.get("geometry").is_some_and(|g| !g.is_null());
    let existing = crate::geometry::existing_links(task);
    let kept: Vec<Value> = existing
        .iter()
        .filter(|link| !crate::geometry::is_geometry_link(link))
        .map(crate::geometry::wire_link)
        .collect();
    let dropped = existing.len() - kept.len();
    if !had_geometry && dropped == 0 {
        return Ok(json!({
            "project": read.project_id,
            "taskId": task_id,
            "applied": false,
            "committedRevision": read.graph.revision,
            "warnings": [],
            "link": ds_command_kernel::project_management::reads::task_route(&read.project_id, &task_id),
            "cleared": { "geometry": false, "links": 0 },
        }));
    }
    let update = UpdateTask {
        task: task_id.clone(),
        clear_geometry: true,
        links: Some(kept),
        ..UpdateTask::default()
    };
    let (prepared, kinds) = writes::update_task(&read.graph, &update).map_err(crate::refused)?;
    let result = crate::commit_batch(lane, &prepared)?;
    let mut extra = Map::new();
    extra.insert("commands".into(), json!(kinds));
    extra.insert(
        "cleared".into(),
        json!({ "geometry": had_geometry, "links": dropped }),
    );
    Ok(writes::write_outcome(
        &read.project_id,
        &task_id,
        &result,
        extra,
    ))
}

pub fn render(data: &Value) -> String {
    let cleared = &data["cleared"];
    let mut out = if data["applied"] == false {
        format!(
            "nothing to clear on {} in {} · revision {}\n",
            data["taskId"].as_str().unwrap_or("?"),
            data["project"].as_str().unwrap_or("?"),
            data["committedRevision"].as_u64().unwrap_or(0),
        )
    } else {
        format!(
            "cleared {} in {} · revision {} · geometry {} · {} object link{} removed\n",
            data["taskId"].as_str().unwrap_or("?"),
            data["project"].as_str().unwrap_or("?"),
            data["committedRevision"].as_u64().unwrap_or(0),
            if cleared["geometry"] == true {
                "removed"
            } else {
                "none"
            },
            cleared["links"].as_u64().unwrap_or(0),
            if cleared["links"].as_u64() == Some(1) {
                ""
            } else {
                "s"
            },
        )
    };
    out.push_str(&crate::task::warnings(data));
    out
}
