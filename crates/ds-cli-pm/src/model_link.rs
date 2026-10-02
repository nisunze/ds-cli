//! `ds pm model-link add|remove|list` — a task, milestone or project note
//! linked to one exact DS Grid model version.
//!
//! A catalog version stays open until a bump supersedes it, so the link pins
//! one immutable catalog revision and carries its version and `.dsgrid`
//! digest (ds-brain `docs/contracts/work-links-and-spatial-anchors.md` §DS Grid
//! model versions). The pin is built from the catalog's own revision record —
//! `--version N` alone resolves to the newest revision of version N as it
//! stands now — and ds-brain checks it again in the Project Work write. The
//! kernel (`project_management::model_links`) owns the link identity, the
//! whole-array change and the read projections; this adapter reads and
//! submits governed actions for the explicitly named project.
//!
//! Not the design-version `vN` marker: linking freezes nothing. Task geometry
//! links (`ds pm task geometry`) are separate and untouched.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use ds_command_kernel::project_management::model_links::{self, ModelPin, VersionLookup};
use ds_command_kernel::project_management::writes::{self, UpdateTask};
use serde_json::{Map, Value, json};

use crate::LANE_ARG;

/// Catalog pages one `--version` lookup reads before it asks for `--revision`.
const MAX_VERSION_PAGES: usize = 20;
const VERSION_PAGE: u16 = 100;

const TASK: Arg = Arg::value(
    "task",
    "<task-id>",
    "The task or milestone, by the id `ds pm task list` reports. Use this or --note.",
);
const NOTE: Arg = Arg::value(
    "note",
    "<record-id>",
    "The project note (a record of category project_note). Use this or --task.",
);
const MODEL: Arg = Arg::value(
    "model",
    "<model-id>",
    "Exact catalog model id from `ds dsgrid project list`, never a display name.",
);
const REVISION: Arg = Arg::value(
    "revision",
    "<revision-id>",
    "Exact immutable catalog revision from `ds dsgrid project versions`.",
);
const VERSION: Arg = Arg::value(
    "version",
    "<n>",
    "Catalog version. Alone: pin its newest revision now. With --revision: must agree.",
);
const LABEL: Arg = Arg::value(
    "label",
    "<text>",
    "Display label kept beside the link (at most 200 characters); not an identity.",
);

pub const SOURCE_INVALID: Refusal = Refusal {
    code: "model_link_source_invalid",
    when: "neither or both of --task and --note were given, or the record is not a project note",
    remedy: "name exactly one existing task or milestone (--task) or project note (--note)",
};
pub const LINK_INVALID: Refusal = Refusal {
    code: "model_link_invalid",
    when: "the model or revision id is not an exact catalog id, neither --revision nor --version was given, the label is over its bound, or the item already holds 128 links",
    remedy: "use the opaque ids from `ds dsgrid project list|versions`; name --revision or --version",
};
pub const VERSION_NOT_FOUND: Refusal = Refusal {
    code: "model_version_not_found",
    when: "the project's DS Grid catalog has no such model, revision or version (a --version lookup reads at most 2,000 revisions)",
    remedy: "list the model's revisions with `ds dsgrid project versions --model <id>` and name --revision",
};
pub const VERSION_MISMATCH: Refusal = Refusal {
    code: "model_version_mismatch",
    when: "--version disagrees with the catalog version of --revision",
    remedy: "drop --version, or name a revision of that version from `ds dsgrid project versions`",
};

pub static ADD: Command = Command {
    id: "pm.model-link.add",
    path: &["pm", "model-link", "add"],
    contract: 1,
    summary: "Link a task, milestone or note to an exact DS Grid model version.",
    purpose: "\
Pins one project model at one exact catalog revision on a task, milestone or \
project note, with that revision's catalog version and .dsgrid digest read \
from the catalog. --version N alone pins the newest revision of version N as \
it stands now; a later save in that version does not move the link. ds-brain \
re-checks the pin in the Project Work write (model in this project and not \
retired, revision present, version and digest equal). The same revision \
already linked is a no-op that keeps its first attribution. Other links, and \
task geometry, are untouched. Linking freezes nothing. Headless.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TASK,
        NOTE,
        MODEL.required(),
        REVISION,
        VERSION,
        LABEL,
        LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "`project`, `source` {kind, id}, the `pin` (model_id, revision_id, model_version, model_digest), `changed`, the item's model `links` after the write, and the committed graph revision or note version.",
    examples: &[Example {
        command: "ds pm model-link add --task T4 --model <model-id> --version 3 --yes --output json --project <exact-id>",
        note: "Pins version 3's newest revision; read .data.pin.revision_id for the exact pin.",
        runnable: false,
    }],
    refusals: &crate::write_refusals::<29>(&[
        SOURCE_INVALID,
        LINK_INVALID,
        VERSION_NOT_FOUND,
        VERSION_MISMATCH,
        crate::RECORD_NOT_FOUND,
        crate::BOUND_EXCEEDED,
        crate::PROJECT_NOT_VISIBLE,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "model version",
        "model revision",
        "link model",
        "model link",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static REMOVE: Command = Command {
    id: "pm.model-link.remove",
    path: &["pm", "model-link", "remove"],
    contract: 1,
    summary: "Remove one exact model-revision link from a task or note.",
    purpose: "\
Removes only the link to --model at --revision from one task, milestone or \
project note. Other revisions of the same model, other links and task \
geometry stay. Works after the model was retired. No such link is a no-op. \
Headless.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TASK,
        NOTE,
        MODEL.required(),
        REVISION.required(),
        LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "`project`, `source`, `model_id`, `revision_id`, `changed`, the item's model `links` after the write, and the committed graph revision or note version.",
    examples: &[],
    refusals: &crate::write_refusals::<26>(&[
        SOURCE_INVALID,
        LINK_INVALID,
        crate::RECORD_NOT_FOUND,
        crate::PROJECT_NOT_VISIBLE,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &["unlink model", "model link"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static LIST: Command = Command {
    id: "pm.model-link.list",
    path: &["pm", "model-link", "list"],
    contract: 1,
    summary: "Read an item's model-version links, or the work linking a model.",
    purpose: "\
With --task or --note: the DS Grid model links that item carries, each with \
its exact revision, catalog version, digest and who attached it when. With \
--model (optionally --revision): every live task, milestone and project note \
in the project that links that model, at that revision when named; notes are \
read from the bounded context page and `notes_truncated` says when it was cut. \
A display label is never matched. Read-only, headless.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TASK, NOTE, MODEL, REVISION, LANE_ARG, crate::PROJECT_ARG],
    output: "Per item: `source` and `links`. Per model: `model_id`, `revision_id`, `tasks`, `notes` and `notes_truncated`.",
    examples: &[Example {
        command: "ds pm model-link list --model <model-id> --output json --project <exact-id>",
        note: "Every task, milestone and note that links any revision of the model.",
        runnable: false,
    }],
    refusals: &crate::read_refusals::<20>(&[
        SOURCE_INVALID,
        LINK_INVALID,
        crate::RECORD_NOT_FOUND,
        crate::TASK_NOT_FOUND,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &["model links", "model work", "model version"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

enum Source {
    Task(String),
    Note(String),
}

fn source(inputs: &Inputs, required: bool) -> Result<Option<Source>, Failure> {
    let task = inputs
        .value("task")
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let note = inputs
        .value("note")
        .map(str::trim)
        .filter(|v| !v.is_empty());
    match (task, note) {
        (Some(id), None) => Ok(Some(Source::Task(id.into()))),
        (None, Some(id)) => Ok(Some(Source::Note(id.into()))),
        (None, None) if !required => Ok(None),
        _ => Err(source_invalid("name exactly one --task or --note")),
    }
}

fn source_invalid(message: &str) -> Failure {
    Failure::invalid(SOURCE_INVALID.code, message).remedy(SOURCE_INVALID.remedy)
}

fn link_invalid(message: &str) -> Failure {
    Failure::invalid(LINK_INVALID.code, message).remedy(LINK_INVALID.remedy)
}

fn not_found(message: String) -> Failure {
    Failure::invalid(VERSION_NOT_FOUND.code, message)
        .remedy(VERSION_NOT_FOUND.remedy)
        .next("ds dsgrid project versions --model <model-id>")
}

/// A catalog read that found nothing is this family's `model_version_not_found`;
/// every other failure (signed out, not permitted, unavailable) keeps its own
/// code and remedy.
fn catalog_failure(failure: Failure, message: String) -> Failure {
    if failure.code() == "grid_model_not_found" {
        not_found(message)
    } else {
        failure
    }
}

fn catalog_id<'a>(inputs: &'a Inputs, flag: &str) -> Result<Option<&'a str>, Failure> {
    match inputs.value(flag).map(str::trim) {
        None => Ok(None),
        Some(id) if model_links::valid_catalog_id(id) => Ok(Some(id)),
        Some(_) => Err(link_invalid(&format!(
            "--{flag} must be an exact catalog id (lowercase letters, digits, '-' or '_')"
        ))),
    }
}

/// The pin, from the catalog's own revision record.
fn resolve_pin(inputs: &Inputs, lane: &str, project: &str) -> Result<ModelPin, Failure> {
    let model = catalog_id(inputs, "model")?.ok_or_else(|| link_invalid("--model is required"))?;
    let version = match inputs.value("version") {
        None => None,
        Some(raw) => Some(
            raw.trim()
                .parse::<u64>()
                .ok()
                .filter(|v| *v > 0)
                .ok_or_else(|| link_invalid("--version must be a positive catalog version"))?,
        ),
    };
    let revision = match (catalog_id(inputs, "revision")?, version) {
        (Some(revision), _) => revision.to_owned(),
        (None, Some(version)) => newest_revision_of(lane, project, model, version)?,
        (None, None) => return Err(link_invalid("name --revision or --version")),
    };
    let shown = ds_cli_auth::grid_models_for_project(
        lane,
        project,
        &ds_cli_auth::GridModelsCommand::ShowVersion {
            model: model.into(),
            revision: revision.clone(),
        },
    )
    .map_err(|failure| {
        catalog_failure(
            failure,
            format!("model {model} has no catalog revision {revision} in this project"),
        )
    })?;
    let pin = ModelPin::from_revision(project, model, &revision, &shown.data["revision"])
        .map_err(|why| not_found(why.into()))?;
    if let Some(version) = version.filter(|v| *v != pin.version) {
        return Err(Failure::invalid(
            VERSION_MISMATCH.code,
            format!(
                "revision {} of model {} is version {}, not {version}",
                pin.revision_id, pin.model_id, pin.version
            ),
        )
        .detail(json!({"model_version": pin.version, "revision_id": pin.revision_id}))
        .remedy(VERSION_MISMATCH.remedy));
    }
    Ok(pin)
}

/// The newest revision of `version`, from newest-first catalog pages.
fn newest_revision_of(
    lane: &str,
    project: &str,
    model: &str,
    version: u64,
) -> Result<String, Failure> {
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_VERSION_PAGES {
        let page = ds_cli_auth::grid_models_for_project(
            lane,
            project,
            &ds_cli_auth::GridModelsCommand::ListVersions {
                model: model.into(),
                limit: VERSION_PAGE,
                cursor: cursor.clone(),
            },
        )
        .map_err(|failure| {
            catalog_failure(
                failure,
                format!("model {model} is not in this project's catalog"),
            )
        })?;
        let groups = page.data["versions"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        match model_links::latest_revision_of_version(&groups, version) {
            VersionLookup::Found(revision) => return Ok(revision),
            VersionLookup::Absent => break,
            VersionLookup::NotYet => {}
        }
        match page.data["next_cursor"].as_str().filter(|c| !c.is_empty()) {
            Some(next) if page.data["more"] == true => cursor = Some(next.into()),
            _ => break,
        }
    }
    Err(not_found(format!(
        "model {model} has no catalog version {version} within its newest {} revisions",
        MAX_VERSION_PAGES * usize::from(VERSION_PAGE)
    )))
}

/// One project note's record: its `item`, refused unless it is a live
/// project note.
fn read_note(lane: &str, project: &str, id: &str) -> Result<Value, Failure> {
    let report = crate::correspondence(
        lane,
        project,
        &Action::RecordRead {
            record_id: id.into(),
        },
    )?;
    let item = report.into_result()["item"].clone();
    if item["id"] != id || item["is_deleted"] == true {
        return Err(Failure::invalid(
            crate::RECORD_NOT_FOUND.code,
            format!("record {id} was not found"),
        )
        .remedy(crate::RECORD_NOT_FOUND.remedy));
    }
    if item["data"]["category"] != "project_note" {
        return Err(source_invalid(
            "only a project note carries model links; this record is not one",
        ));
    }
    Ok(item)
}

fn note_link_array(item: &Value) -> Vec<Value> {
    model_links::note_links(item)
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// Apply one link change to the named source and report it.
fn change(
    inputs: &Inputs,
    lane: &str,
    project: &str,
    apply: impl Fn(&[Value]) -> Result<model_links::LinkChange, Failure>,
    mut out: Map<String, Value>,
) -> Result<Value, Failure> {
    let source = source(inputs, true)?.expect("required source");
    out.insert("project".into(), json!(project));
    match source {
        Source::Task(task_id) => {
            let read = crate::graph(lane, project)?;
            let task = read
                .raw_task(&task_id)
                .ok_or_else(|| crate::refused(writes::Refusal::TaskNotFound(task_id.clone())))?;
            let current = task["links"].as_array().cloned().unwrap_or_default();
            let changed = apply(&current)?;
            out.insert("source".into(), json!({"kind": "task", "id": task_id}));
            out.insert("changed".into(), json!(changed.changed));
            out.insert(
                "links".into(),
                json!(model_links::project_links(&changed.links, &read.project_id)),
            );
            if !changed.changed {
                out.insert("committedRevision".into(), json!(read.graph.revision));
                return Ok(Value::Object(out));
            }
            let update = UpdateTask {
                task: task_id.clone(),
                links: Some(changed.links),
                ..UpdateTask::default()
            };
            let (prepared, kinds) =
                writes::update_task(&read.graph, &update).map_err(crate::refused)?;
            let result = crate::commit_batch(lane, project, &prepared)?;
            out.insert("commands".into(), json!(kinds));
            Ok(writes::write_outcome(
                &read.project_id,
                &task_id,
                &result,
                out,
            ))
        }
        Source::Note(note_id) => {
            let item = read_note(lane, project, &note_id)?;
            let changed = apply(&note_link_array(&item))?;
            out.insert(
                "source".into(),
                json!({"kind": "project_note", "id": note_id}),
            );
            out.insert("changed".into(), json!(changed.changed));
            if !changed.changed {
                out.insert(
                    "links".into(),
                    json!(model_links::project_links(&changed.links, project)),
                );
                out.insert("noteVersion".into(), item["version"].clone());
                return Ok(Value::Object(out));
            }
            let expected_version = item["version"].as_i64().unwrap_or(1).max(1);
            let mut data = Map::new();
            data.insert("links".into(), json!(changed.links));
            let report = crate::correspondence(
                lane,
                project,
                &Action::RecordUpdate {
                    id: note_id.clone(),
                    expected_version,
                    data,
                },
            )?;
            let saved = report.into_result()["item"].clone();
            out.insert(
                "links".into(),
                json!(model_links::project_links(
                    &note_link_array(&saved),
                    project
                )),
            );
            out.insert("noteVersion".into(), saved["version"].clone());
            Ok(Value::Object(out))
        }
    }
}

pub fn add(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.value("lane").unwrap_or("stable");
    let project = inputs.require("project")?;
    source(inputs, true)?;
    let pin = resolve_pin(inputs, lane, project)?;
    let label = inputs.value("label");
    let mut out = Map::new();
    out.insert(
        "pin".into(),
        json!({"model_id": pin.model_id, "revision_id": pin.revision_id,
            "model_version": pin.version, "model_digest": pin.digest}),
    );
    change(
        inputs,
        lane,
        project,
        |current| model_links::add(current, &pin, label).map_err(link_invalid),
        out,
    )
}

pub fn remove(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.value("lane").unwrap_or("stable");
    let project = inputs.require("project")?;
    source(inputs, true)?;
    let model = catalog_id(inputs, "model")?.ok_or_else(|| link_invalid("--model is required"))?;
    let revision =
        catalog_id(inputs, "revision")?.ok_or_else(|| link_invalid("--revision is required"))?;
    let mut out = Map::new();
    out.insert("model_id".into(), json!(model));
    out.insert("revision_id".into(), json!(revision));
    change(
        inputs,
        lane,
        project,
        |current| Ok(model_links::remove(current, project, model, revision)),
        out,
    )
}

pub fn list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.value("lane").unwrap_or("stable");
    let project = inputs.require("project")?;
    let model = catalog_id(inputs, "model")?;
    let revision = catalog_id(inputs, "revision")?;
    match (source(inputs, false)?, model) {
        (Some(_), Some(_)) | (None, None) => Err(source_invalid(
            "name --task or --note to read one item, or --model to find the work that links it",
        )),
        (Some(Source::Task(task_id)), None) => {
            let read = crate::graph(lane, project)?;
            let task = read
                .raw_task(&task_id)
                .ok_or_else(|| crate::refused(writes::Refusal::TaskNotFound(task_id.clone())))?;
            let links = task["links"].as_array().cloned().unwrap_or_default();
            Ok(json!({
                "project": read.project_id,
                "source": {"kind": "task", "id": task_id, "title": task["data"]["title"]},
                "links": model_links::project_links(&links, &read.project_id),
                "revision": read.graph.revision,
            }))
        }
        (Some(Source::Note(note_id)), None) => {
            let item = read_note(lane, project, &note_id)?;
            Ok(json!({
                "project": project,
                "source": {"kind": "project_note", "id": note_id, "title": item["data"]["subject"]},
                "links": model_links::project_links(&note_link_array(&item), project),
                "noteVersion": item["version"],
            }))
        }
        (None, Some(model)) => {
            let read = crate::graph(lane, project)?;
            let tasks = model_links::task_references(&read.raw, &read.project_id, model, revision);
            let context = ds_cli_auth::project_management_for_project(
                lane,
                project,
                &ds_client_core::project_management::Command::Context {
                    limit: Some(crate::MAX_CONTEXT_ROWS),
                },
            )?
            .into_result();
            let records = context["records"].as_array().cloned().unwrap_or_default();
            let notes = model_links::note_references(&records, &read.project_id, model, revision);
            let truncated = context["truncated_by_collection"]["records"] == true
                || context["next_cursors"]["records"].is_string();
            Ok(json!({
                "project": read.project_id,
                "model_id": model,
                "revision_id": revision,
                "tasks": tasks,
                "notes": notes,
                "notes_truncated": truncated,
                "revision": read.graph.revision,
            }))
        }
    }
}

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    if let Some(source) = data["source"].as_object() {
        out.push_str(&format!(
            "{} {} in {}{}\n",
            source.get("kind").and_then(Value::as_str).unwrap_or("?"),
            source.get("id").and_then(Value::as_str).unwrap_or("?"),
            data["project"].as_str().unwrap_or("?"),
            match data["changed"].as_bool() {
                Some(true) => " · changed",
                Some(false) => " · unchanged",
                None => "",
            },
        ));
        for link in data["links"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "  {} @ {} · v{} · {}\n",
                link["model_id"].as_str().unwrap_or("?"),
                link["revision_id"].as_str().unwrap_or("(model-wide)"),
                link["model_version"]
                    .as_u64()
                    .map_or("?".into(), |v| v.to_string()),
                link["label"].as_str().unwrap_or(""),
            ));
        }
        return out;
    }
    out.push_str(&format!(
        "work linking {}{} in {}\n",
        data["model_id"].as_str().unwrap_or("?"),
        data["revision_id"]
            .as_str()
            .map(|r| format!(" @ {r}"))
            .unwrap_or_default(),
        data["project"].as_str().unwrap_or("?"),
    ));
    for row in data["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(data["notes"].as_array().into_iter().flatten())
    {
        out.push_str(&format!(
            "  {} {} · {} · @ {}\n",
            row["source_kind"].as_str().unwrap_or("?"),
            row["source_id"].as_str().unwrap_or("?"),
            row["title"].as_str().unwrap_or(""),
            row["revision_id"].as_str().unwrap_or("(model-wide)"),
        ));
    }
    if data["notes_truncated"] == true {
        out.push_str(
            "  notes page was cut; more project notes exist than one context read holds\n",
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_name_one_family_and_their_refusal_sets_hold() {
        for command in [&ADD, &REMOVE, &LIST] {
            assert_eq!(command.path[..2], ["pm", "model-link"]);
            assert!(
                command
                    .refusals
                    .iter()
                    .any(|r| r.code == SOURCE_INVALID.code)
            );
        }
        assert!(ADD.refusals.iter().any(|r| r.code == VERSION_MISMATCH.code));
        assert_eq!(REMOVE.effect, Effect::GlobalWrite);
        assert_eq!(LIST.effect, Effect::ReadOnly);
    }

    #[test]
    fn render_reads_both_shapes() {
        let item = json!({"project":"p","source":{"kind":"task","id":"T4"},"changed":true,
            "links":[{"model_id":"model-a","revision_id":"rev-2","model_version":3,"label":"Feeder"}]});
        assert!(render(&item).contains("model-a @ rev-2 · v3 · Feeder"));
        let by_model = json!({"project":"p","model_id":"model-a","revision_id":null,
            "tasks":[{"source_kind":"milestone","source_id":"M1","title":"Issue","revision_id":"rev-2"}],
            "notes":[],"notes_truncated":true});
        let text = render(&by_model);
        assert!(text.contains("milestone M1 · Issue · @ rev-2"));
        assert!(text.contains("notes page was cut"));
    }
}
