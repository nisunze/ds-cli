//! `ds pm task geometry set` — give an existing task the geometry of the DS
//! objects it is about.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_management::writes::{self, UpdateTask};
use serde_json::{Map, Value, json};

use crate::geometry::{BUFFER_ARG, DRY_RUN_ARG, FROM_ARG, PACKAGE_ARG, PROPOSAL_REFUSALS};
use crate::{LANE_ARG, TASK_ARG};

pub static COMMAND: Command = Command {
    id: "pm.task.geometry.set",
    path: &["pm", "task", "geometry", "set"],
    contract: 1,
    summary: "Set a task's geometry from DS Grid structures or an alignment.",
    purpose: "\
Where the work is, from the objects a comment names: structures 74, 76, 77 \
of a DS Grid model become an area around them (a polygon: their convex hull, \
buffered), one structure a point, an alignment or a span range a line. The \
kernel resolves the typed references deterministically — nothing parses prose \
— and the task gets the geometry plus one ds_object link per structure, in \
one revision. --dry-run answers the proposal without writing; --yes writes it.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TASK_ARG,
        FROM_ARG,
        PACKAGE_ARG,
        BUFFER_ARG,
        DRY_RUN_ARG,
        LANE_ARG,
    ],
    output: "\
`proposal` — the `geometry`, the shaping `rule`, the resolved `objects`, the \
new `links` and the model `sources` — plus, when written, `taskId`, \
`committedRevision`, `warnings` and `link`; `dry_run: true` when not.",
    examples: &[
        Example {
            command: "ds pm task geometry set --task T4 --from dsgrid:local-<id>:structure:74,76,77 --dry-run --output json",
            note: "The proposal: a polygon around the three structures and three links. Nothing is written.",
            runnable: false,
        },
        Example {
            command: "ds pm task geometry set --task T4 --from dsgrid:local-<id>:alignment:aln-1:74..77 --yes",
            note: "A line along the alignment between structures 74 and 77.",
            runnable: false,
        },
    ],
    refusals: &crate::write_refusals::<39>(&PROPOSAL_REFUSALS),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "task geometry",
        "where is the task",
        "attach structures to task",
        "area",
        "swamp",
        "structures",
        "alignment",
        "map",
        "location",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let task_id = inputs.require("task")?.to_owned();
    crate::geometry::check(inputs, inputs.repeated("from"))?;
    let lane = inputs.value("lane").unwrap_or("stable");
    let read = crate::graph(lane)?;
    let task = read
        .raw_task(&task_id)
        .ok_or_else(|| crate::refused(writes::Refusal::TaskNotFound(task_id.clone())))?;
    let existing = crate::geometry::existing_links(task);
    let proposal = crate::geometry::propose(inputs, inputs.repeated("from"), &read, &existing)?;
    let proposal = crate::geometry::proposal_json(&proposal);

    if inputs.switch("dry-run") {
        return Ok(json!({
            "project": read.project_id,
            "taskId": task_id,
            "dry_run": true,
            "revision": read.graph.revision,
            "proposal": proposal,
            "committedRevision": Value::Null,
        }));
    }

    let mut links: Vec<Value> = existing.iter().map(crate::geometry::wire_link).collect();
    links.extend(proposal["links"].as_array().into_iter().flatten().cloned());
    let update = UpdateTask {
        task: task_id.clone(),
        geometry: Some(proposal["geometry"].clone()),
        links: Some(links),
        ..UpdateTask::default()
    };
    let (prepared, kinds) = writes::update_task(&read.graph, &update).map_err(crate::refused)?;
    let result = crate::commit_batch(lane, &prepared)?;
    let mut extra = Map::new();
    extra.insert("commands".into(), json!(kinds));
    extra.insert("dry_run".into(), json!(false));
    extra.insert("proposal".into(), proposal);
    Ok(writes::write_outcome(
        &read.project_id,
        &task_id,
        &result,
        extra,
    ))
}

pub fn render(data: &Value) -> String {
    let mut out = if data["dry_run"] == true {
        format!(
            "proposal for {} in {} · revision {} · nothing written\n",
            data["taskId"].as_str().unwrap_or("?"),
            data["project"].as_str().unwrap_or("?"),
            data["revision"].as_u64().unwrap_or(0),
        )
    } else {
        format!(
            "geometry set on {} in {} · revision {}\n",
            data["taskId"].as_str().unwrap_or("?"),
            data["project"].as_str().unwrap_or("?"),
            data["committedRevision"].as_u64().unwrap_or(0),
        )
    };
    out.push_str(&crate::geometry::render_proposal(&data["proposal"]));
    out.push_str(&crate::task::warnings(data));
    if let Some(link) = data["link"].as_str() {
        out.push_str(&format!("  {link}\n"));
    }
    out
}
