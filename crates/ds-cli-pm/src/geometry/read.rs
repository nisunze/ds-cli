//! `ds pm task geometry read` — where a task is, and which objects say so.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_management::writes;
use serde_json::{Value, json};

use crate::{LANE_ARG, TASK_ARG};

pub static COMMAND: Command = Command {
    id: "pm.task.geometry.read",
    path: &["pm", "task", "geometry", "read"],
    contract: 1,
    summary: "Read a task's geometry and the DS object links it came from.",
    purpose: "\
The task's top-level WGS84 geometry (a point, a line or a polygon) — the \
same value the plan and the map's tasks layer paint from — with every link \
the task carries and, for each ds_object link, the reconciliation state \
ds-brain reports for that subject. Headless: it reads the canonical graph, \
nothing else.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TASK_ARG, LANE_ARG, crate::PROJECT_ARG],
    output: "\
`taskId`, `title`, `geometry` (or null), `positions`, `links` (all), \
`objectLinks` (the ds_object ones with `subject_state`), the plan `revision` \
and `link`.",
    examples: &[Example {
        command: "ds pm task geometry read --task T4 --output json --project <exact-id>",
        note: "`.data.geometry` is the GeoJSON the map draws; `.data.objectLinks[]` names the structures.",
        runnable: false,
    }],
    refusals: &crate::read_refusals::<17>(&[crate::TASK_NOT_FOUND]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "task geometry",
        "where",
        "task location",
        "structures",
        "map",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let task_id = inputs.require("task")?.to_owned();
    let lane = inputs.value("lane").unwrap_or("stable");
    let read = crate::graph(lane, inputs.require("project")?)?;
    let task = read
        .raw_task(&task_id)
        .ok_or_else(|| crate::refused(writes::Refusal::TaskNotFound(task_id.clone())))?;
    let geometry = task.get("geometry").cloned().unwrap_or(Value::Null);
    let positions = match geometry["type"].as_str() {
        Some("Point") => 1,
        Some("LineString") => geometry["coordinates"].as_array().map_or(0, Vec::len),
        Some("Polygon") => geometry["coordinates"][0].as_array().map_or(0, Vec::len),
        _ => 0,
    };
    let links = crate::geometry::existing_links(task);
    let states = read.raw["subject_states"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let object_links: Vec<Value> = links
        .iter()
        .filter(|link| link["kind"] == "ds_object")
        .map(|link| {
            let state = states.iter().find(|state| {
                state["object_type"] == link["object_type"]
                    && state["object_id"] == link["entity_id"]
                    && (state["observed_revision"] == link["object_revision"]
                        || state["observed_revision"].is_null())
            });
            json!({
                "object_type": link["object_type"],
                "entity_id": link["entity_id"],
                "object_revision": link["object_revision"],
                "label": link["label"],
                "attached_by": link["attached_by"],
                "attached_at": link["attached_at"],
                "subject_state": state.map(|s| s["state"].clone()).unwrap_or(Value::Null),
            })
        })
        .collect();
    Ok(json!({
        "project": read.project_id,
        "revision": read.graph.revision,
        "taskId": task_id,
        "title": task["data"]["title"],
        "geometry": geometry,
        "positions": positions,
        "links": links,
        "objectLinks": object_links,
        "link": ds_command_kernel::project_management::reads::task_route(&read.project_id, &task_id),
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} · {} · revision {}\n",
        data["taskId"].as_str().unwrap_or("?"),
        data["title"].as_str().unwrap_or("?"),
        data["revision"].as_u64().unwrap_or(0),
    );
    match data["geometry"]["type"].as_str() {
        Some(kind) => out.push_str(&format!(
            "  geometry   {kind} · {} positions\n",
            data["positions"]
        )),
        None => out.push_str("  geometry   none\n"),
    }
    let objects = data["objectLinks"].as_array().map_or(0, Vec::len);
    let all = data["links"].as_array().map_or(0, Vec::len);
    out.push_str(&format!(
        "  links      {all} · {objects} DS object link{}\n",
        if objects == 1 { "" } else { "s" }
    ));
    for link in data["objectLinks"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<18} {:<20} {}{}\n",
            link["object_type"].as_str().unwrap_or("?"),
            crate::truncate(link["label"].as_str().unwrap_or("?"), 20),
            link["entity_id"].as_str().unwrap_or("?"),
            link["subject_state"]
                .as_str()
                .map(|s| format!(" · {s}"))
                .unwrap_or_default(),
        ));
    }
    if let Some(link) = data["link"].as_str() {
        out.push_str(&format!("  {link}\n"));
    }
    out
}
