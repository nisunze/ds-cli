//! Bounded Project Work overview. Geometry is neither required nor a filter.
//! Graph and context are separate authority reads, never an atomic snapshot.
use super::{browser, ordered_rows, reads, wire};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const MAX_ROWS: i64 = 100;
pub const DEFAULT_ROWS: i64 = 10;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Options {
    #[serde(default = "default_rows")]
    pub limit: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub records_cursor: Option<String>,
}
fn default_rows() -> i64 {
    DEFAULT_ROWS
}
impl Default for Options {
    fn default() -> Self {
        Self {
            limit: DEFAULT_ROWS,
            records_cursor: None,
        }
    }
}
impl Options {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=MAX_ROWS).contains(&self.limit)
            || self.records_cursor.as_deref().is_some_and(|s| {
                s.is_empty()
                    || s.len() > 256
                    || matches!(s, "." | "..")
                    || s.chars()
                        .any(|c| c.is_whitespace() || c.is_control() || c == '/')
            })
        {
            return Err(
                "Overview requires 1..100 rows and an optional exact records cursor.".into(),
            );
        }
        Ok(())
    }
    pub fn context_payload(&self, project: &str) -> Value {
        let mut payload = json!({"action":"get_context","ds_project":project,"limit":MAX_ROWS,"collections":["pm_records"]});
        if let Some(cursor) = &self.records_cursor {
            payload["cursors"] = json!({"pm_records":cursor});
        }
        payload
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub schema: String,
    pub action: String,
    pub ds_project: String,
    pub graph: Value,
    pub context: Value,
    #[serde(default = "wire_format")]
    pub graph_format: String,
    #[serde(default)]
    pub options: Options,
}
fn wire_format() -> String {
    "wire".into()
}
#[derive(Serialize, Debug)]
pub struct List<T> {
    pub count: usize,
    pub count_kind: &'static str,
    pub returned: usize,
    pub truncated: bool,
    pub rows: Vec<T>,
}
fn list<T>(mut rows: Vec<T>, limit: usize, incomplete: bool) -> List<T> {
    let count = rows.len();
    rows.truncate(limit);
    List {
        count,
        count_kind: if incomplete { "lower_bound" } else { "exact" },
        returned: rows.len(),
        truncated: incomplete || count > rows.len(),
        rows,
    }
}
#[derive(Serialize, Debug)]
pub struct Record {
    #[serde(flatten)]
    pub row: reads::RecordRow,
    pub version: i64,
    pub revision_label: Option<String>,
    pub document_state: Option<String>,
    pub related_task_ids: Vec<String>,
}
#[derive(Serialize, Debug)]
pub struct Overview {
    pub schema: &'static str,
    pub project_id: String,
    pub graph_revision: i64,
    pub schedule_revision: i64,
    pub read_consistency: &'static str,
    pub context_graph_revision: Option<i64>,
    pub tasks: List<reads::TaskRow>,
    pub milestones: List<reads::TaskRow>,
    pub records: List<Record>,
    pub project_notes: List<Record>,
    pub context_returned_records: usize,
    pub context_records_cursor: Option<String>,
    pub truncated_by_collection: BTreeMap<String, bool>,
    pub next_cursors: BTreeMap<String, String>,
}
pub fn validate_graph(graph: &Value, project: &str, format: &str) -> Result<(), String> {
    let (project_key, schedule_key, residual_key) = if format == "canonical" {
        ("projectId", "scheduleRevision", "residualItems")
    } else {
        ("project_id", "schedule_revision", "residual_items")
    };
    if project.trim().is_empty()
        || graph.get(project_key).and_then(Value::as_str) != Some(project)
        || !graph["tasks"].is_array()
        || !graph["dependencies"].is_array()
        || !graph[residual_key].is_array()
        || graph["revision"].as_i64().is_none_or(|v| v < 0)
        || graph[schedule_key].as_i64().is_none_or(|v| v < 0)
    {
        return Err(
            "Overview graph does not prove the exact project, collections and revisions.".into(),
        );
    }
    Ok(())
}
pub fn fold(input: Input) -> Result<Overview, String> {
    input.options.validate()?;
    if input.schema != super::SCHEMA
        || input.action != "overview"
        || input.ds_project.trim().is_empty()
    {
        return Err("Invalid project overview request.".into());
    }
    validate_graph(&input.graph, &input.ds_project, &input.graph_format)?;
    if input.context["project_id"].as_str() != Some(input.ds_project.as_str())
        || input
            .context
            .get("task_id")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
        || input
            .context
            .get("record_id")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
        || input
            .context
            .get("residual_id")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
    {
        return Err("Overview requires an exact, project-wide graph and context; denied or foreign responses cannot become empty work.".into());
    }
    let loaded = input.context["loaded_collections"]
        .as_array()
        .ok_or("Overview context has no loaded-collection evidence.")?;
    let allowed = [
        "pm_work_episodes",
        "pm_records",
        "pm_comments",
        "pm_attachments",
        "pm_activities",
        "pm_intelligence_artifacts",
    ];
    let mut seen = std::collections::BTreeSet::new();
    for collection in loaded {
        let name = collection.as_str().ok_or("Invalid loaded collection.")?;
        if !allowed.contains(&name) || !seen.insert(name) {
            return Err("Invalid loaded collection evidence.".into());
        }
    }
    if !seen.contains("pm_records") {
        return Err("Overview records were not acquired.".into());
    }
    let raw_records = input.context["records"]
        .as_array()
        .ok_or("Overview context has no records collection.")?;
    if raw_records.len() > MAX_ROWS as usize {
        return Err("Overview context exceeds its record read bound.".into());
    }
    let flags = input.context["truncated_by_collection"]
        .as_object()
        .ok_or("Overview context has no completeness flags.")?;
    let records_truncated = flags
        .get("pm_records")
        .and_then(Value::as_bool)
        .ok_or("Overview context has no record completeness flag.")?;
    let cursor_values = input.context["next_cursors"]
        .as_object()
        .ok_or("Overview context has no cursor manifest.")?;
    let collections = [
        "pm_work_episodes",
        "pm_records",
        "pm_comments",
        "pm_attachments",
        "pm_activities",
        "pm_intelligence_artifacts",
    ];
    let mut truncated = BTreeMap::new();
    let mut cursors = BTreeMap::new();
    for key in collections {
        if !seen.contains(key) && (flags.contains_key(key) || cursor_values.contains_key(key)) {
            return Err(
                "Unqueried overview collection cannot claim completeness or a cursor.".into(),
            );
        }
        if let Some(flag) = flags.get(key) {
            truncated.insert(
                key.into(),
                flag.as_bool().ok_or("Invalid context completeness flag.")?,
            );
        }
        if let Some(cursor) = cursor_values.get(key) {
            cursors.insert(
                key.into(),
                cursor.as_str().ok_or("Invalid context cursor.")?.to_owned(),
            );
        }
    }
    let context_revision = input
        .context
        .get("graph_revision")
        .map(|v| {
            v.as_i64()
                .filter(|v| *v >= 0)
                .ok_or("Invalid context graph revision.")
        })
        .transpose()?;
    let graph = browser::graph(&input.graph, &input.ds_project, &input.graph_format)?;
    let expected_tasks = input.graph["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["is_deleted"] != true)
        .count();
    if graph.tasks.len() != expected_tasks {
        return Err("Overview graph contains unreadable task rows.".into());
    }
    let by_id: BTreeMap<_, _> = graph.tasks.iter().map(|t| (t.id.as_str(), t)).collect();
    let mut tasks = Vec::new();
    let mut milestones = Vec::new();
    for ordered in ordered_rows(&graph.tasks) {
        let task = by_id
            .get(ordered.id.as_str())
            .ok_or("Overview task identity disappeared.")?;
        let row = reads::task_row(&ordered.wbs, task);
        if matches!(task.task_type, wire::TaskType::Milestone) {
            milestones.push(row)
        } else {
            tasks.push(row)
        }
    }
    let mut records = Vec::new();
    let mut notes = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for raw in raw_records {
        if raw["is_deleted"] == true {
            continue;
        }
        if !raw["data"].is_object() {
            return Err("Overview context contains an unreadable record envelope.".into());
        }
        let record = reads::decode_context_record(raw)
            .ok_or("Overview context contains an unreadable record identity.")?;
        if record.version < 1 || !seen.insert(record.id.clone()) {
            return Err("Overview record version or identity is invalid.".into());
        }
        let row = Record {
            row: reads::record_row(&record),
            version: record.version,
            revision_label: record.revision_label,
            document_state: record.document_state,
            related_task_ids: record.related_task_ids,
        };
        if record.category == "project_note" {
            notes.push(row)
        } else {
            records.push(row)
        }
    }
    let context_count = records.len() + notes.len();
    let incomplete = records_truncated
        || input.options.records_cursor.is_some()
        || cursors.get("pm_records").is_some_and(|s| !s.is_empty());
    let consistency = match context_revision {
        None => "separate_reads_unversioned_context",
        Some(r) if r == graph.revision => "separate_reads_matching_graph_revision",
        Some(_) => "separate_reads_different_graph_revisions",
    };
    let limit = input.options.limit as usize;
    Ok(Overview {
        schema: "ds.project-overview/v1",
        project_id: input.ds_project,
        graph_revision: graph.revision,
        schedule_revision: graph.schedule_revision,
        read_consistency: consistency,
        context_graph_revision: context_revision,
        tasks: list(tasks, limit, false),
        milestones: list(milestones, limit, false),
        records: list(records, limit, incomplete),
        project_notes: list(notes, limit, incomplete),
        context_returned_records: context_count,
        context_records_cursor: input.options.records_cursor,
        truncated_by_collection: truncated,
        next_cursors: cursors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn input() -> Input {
        serde_json::from_value(json!({"schema":super::super::SCHEMA,"action":"overview","ds_project":"p","options":{"limit":1},
  "graph":{"project_id":"p","revision":9,"schedule_revision":4,"tasks":[{"id":"inbox","data":{"title":"Non-geometric inbox","placement":"inbox","type":"task"}},{"id":"gate","data":{"title":"Milestone","type":"milestone"}}],"dependencies":[],"residual_items":[]},
  "context":{"project_id":"p","loaded_collections":["pm_records"],"records":[{"id":"record","version":2,"data":{"category":"instruction","subject":"Record","revision_label":"R3"}},{"id":"note","version":7,"data":{"category":"project_note","subject":"Non-geometric project note","body":"project note body"}}],"truncated_by_collection":{"pm_records":false},"next_cursors":{},"personal_notes":[{"body":"PRIVATE_SECRET"}]}})).unwrap()
    }
    #[test]
    fn record_only_selection_is_fixed_and_unqueried_totals_are_refused() {
        let mut i = input();
        assert_eq!(
            i.options.context_payload("p")["collections"],
            json!(["pm_records"])
        );
        i.options.records_cursor = Some("next-record".into());
        assert_eq!(
            i.options.context_payload("p")["cursors"],
            json!({"pm_records":"next-record"})
        );
        i.context["truncated_by_collection"]["pm_comments"] = json!(false);
        assert!(fold(i).is_err());
        for loaded in [
            json!([]),
            json!(["private_notes"]),
            json!(["pm_records", "pm_records"]),
        ] {
            let mut i = input();
            i.context["loaded_collections"] = loaded;
            assert!(fold(i).is_err());
        }
    }

    #[test]
    fn includes_every_kind_without_geometry_and_keeps_source_versions() {
        let o = fold(input()).unwrap();
        assert_eq!(o.tasks.count, 1);
        assert_eq!(o.milestones.count, 1);
        assert_eq!(o.records.count, 1);
        assert_eq!(o.project_notes.count, 1);
        assert_eq!(o.project_notes.rows[0].version, 7);
        assert_eq!(o.records.rows[0].revision_label.as_deref(), Some("R3"));
        assert_eq!(o.graph_revision, 9);
        assert_eq!(o.read_consistency, "separate_reads_unversioned_context");
        assert!(!serde_json::to_string(&o)
            .unwrap()
            .contains("PRIVATE_SECRET"));
    }
    #[test]
    fn truncated_and_cursor_pages_do_not_claim_full_totals() {
        let mut i = input();
        i.context["truncated_by_collection"]["pm_records"] = json!(true);
        i.context["next_cursors"]["pm_records"] = json!("next");
        let o = fold(i).unwrap();
        assert_eq!(o.records.count_kind, "lower_bound");
        assert!(o.project_notes.truncated);
        let mut i = input();
        i.options.records_cursor = Some("last-page".into());
        assert_eq!(fold(i).unwrap().project_notes.count_kind, "lower_bound");
    }
    #[test]
    fn differing_graph_revisions_remain_explicit() {
        let mut i = input();
        i.context["graph_revision"] = json!(10);
        let o = fold(i).unwrap();
        assert_eq!(o.context_graph_revision, Some(10));
        assert_eq!(
            o.read_consistency,
            "separate_reads_different_graph_revisions"
        );
    }
    #[test]
    fn denied_foreign_missing_and_nonobject_inputs_do_not_become_empty() {
        for pointer in [
            "/graph",
            "/context",
            "/graph/tasks",
            "/context/records",
            "/context/loaded_collections",
            "/context/truncated_by_collection",
        ] {
            let mut i = input();
            if pointer.starts_with("/graph") {
                *i.graph
                    .pointer_mut(pointer.strip_prefix("/graph").unwrap())
                    .unwrap() = Value::Null;
            } else {
                *i.context
                    .pointer_mut(pointer.strip_prefix("/context").unwrap())
                    .unwrap() = Value::Null;
            }
            assert!(fold(i).is_err(), "{pointer}");
        }
        let mut i = input();
        i.context["project_id"] = json!("foreign");
        assert!(fold(i).is_err());
        let mut i = input();
        i.graph["project_id"] = json!("foreign");
        assert!(fold(i).is_err());
    }
}
