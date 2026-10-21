//! Correspondence, folded for `ds pm party|record|task` (ds-brain
//! `docs/contracts/correspondence.md`).
//!
//! The server answers the contract's bodies — a record view with its
//! attachments, a thread, a bounded page of records or parties, a task with
//! its timeline — and this module turns each into the receipt the CLI
//! documents, on the shapes [`super::reads`] already gives the two record
//! commands. Nothing is re-derived: the response status, the days, the
//! blockers are the engine's numbers; a bound the server reports is carried,
//! never recounted.

use serde::Serialize;
use serde_json::{Map, Value};

use super::reads::{decode_context_record, record_detail, record_row, PageBounds, RecordDetail};
use super::wire::optional_instant;

/// The most timeline entries one read carries; ds-brain bounds every source
/// at 250 and the merged list is cut here, saying so.
pub const MAX_TIMELINE_ENTRIES: usize = 500;
/// The most attachment rows one record view carries.
pub const MAX_ATTACHMENT_ROWS: usize = 200;

fn map_of(value: Option<&Value>) -> &Map<String, Value> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    match value {
        Some(Value::Object(map)) => map,
        _ => EMPTY.get_or_init(Map::new),
    }
}

fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(raw)) => raw.trim().to_owned(),
        _ => String::new(),
    }
}

fn optional_text(value: Option<&Value>) -> Option<String> {
    let text = text(value);
    (!text.is_empty()).then_some(text)
}

fn texts(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| text(Some(item)))
            .filter(|item| !item.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn integer(value: Option<&Value>) -> i64 {
    value.and_then(Value::as_i64).unwrap_or_default()
}

fn rows(value: Option<&Value>) -> &[Value] {
    match value {
        Some(Value::Array(items)) => items,
        _ => &[],
    }
}

// ── attachments ─────────────────────────────────────────────────────────

/// One asset linked to a record, as the assets authority answered it: the
/// catalogue's own row, redacted per caller, reduced to what a terminal
/// needs to open it with `ds assets`.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRow {
    pub asset_id: String,
    pub name: String,
    pub kind: String,
    pub format: String,
    pub bytes: i64,
    pub sensitivity: String,
    pub folder: String,
    /// `false` for an external reference: `read` answers its link.
    pub bytes_held: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
    /// `{number, revision_label, state}` when the asset is a registered document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<Value>,
    /// The parts a mail container lists, by member name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mail_parts: Vec<String>,
}

fn attachment_row(row: &Value) -> Option<AttachmentRow> {
    let asset = map_of(Some(row));
    let asset_id = optional_text(asset.get("asset_id"))?;
    Some(AttachmentRow {
        asset_id,
        name: text(asset.get("name")),
        kind: text(asset.get("kind")),
        format: text(asset.get("format")),
        bytes: integer(asset.get("bytes")),
        sensitivity: text(asset.get("sensitivity")),
        folder: text(asset.get("folder")),
        bytes_held: asset.get("bytes_held") != Some(&Value::Bool(false)),
        external_url: optional_text(map_of(asset.get("external")).get("url")),
        document: asset
            .get("document")
            .filter(|value| value.is_object())
            .cloned(),
        mail_parts: rows(map_of(asset.get("mail")).get("parts"))
            .iter()
            .filter_map(|part| optional_text(map_of(Some(part)).get("path")))
            .collect(),
    })
}

// ── one record, with what the server projected beside it ────────────────

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskRef {
    pub task_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_at: Option<String>,
}

/// `record_create|reply|update|read`'s answer, as `ds pm record read` shows
/// it: the record's detail plus the projections the contract names —
/// `attachments`, `documents`, `blockedTasks`, `resultingTasks`, the thread
/// size — each bounded with its total.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordView {
    pub project: String,
    pub record: RecordDetail,
    pub attachments: Vec<AttachmentRow>,
    pub attachments_total: i64,
    pub documents: Vec<AttachmentRow>,
    pub blocked_tasks: Vec<TaskRef>,
    pub resulting_tasks: Vec<TaskRef>,
    pub thread_total: i64,
    pub thread_truncated: bool,
    pub as_of: String,
}

/// Fold one record view. `None` when the server's `item` is not a record.
pub fn record_view(project: &str, view: &Value) -> Option<RecordView> {
    let view = map_of(Some(view));
    let entry = decode_context_record(view.get("item")?)?;
    let attachments: Vec<AttachmentRow> = rows(view.get("attachments"))
        .iter()
        .filter_map(attachment_row)
        .take(MAX_ATTACHMENT_ROWS)
        .collect();
    let documents: Vec<AttachmentRow> = rows(view.get("documents"))
        .iter()
        .filter_map(attachment_row)
        .take(MAX_ATTACHMENT_ROWS)
        .collect();
    let task_ref = |row: &Value| -> Option<TaskRef> {
        let task = map_of(Some(row));
        Some(TaskRef {
            task_id: optional_text(task.get("task_id"))?,
            title: text(task.get("title")),
            set_by: optional_text(task.get("set_by")),
            set_at: optional_instant(task.get("set_at")),
        })
    };
    Some(RecordView {
        project: project.to_owned(),
        record: record_detail(&entry),
        attachments_total: integer(view.get("attachments_total")).max(attachments.len() as i64),
        attachments,
        documents,
        blocked_tasks: rows(view.get("blocked_tasks"))
            .iter()
            .filter_map(task_ref)
            .collect(),
        resulting_tasks: rows(view.get("resulting_tasks"))
            .iter()
            .filter_map(task_ref)
            .collect(),
        thread_total: integer(view.get("thread_total")),
        thread_truncated: view.get("thread_truncated") == Some(&Value::Bool(true)),
        as_of: text(view.get("as_of")),
    })
}

// ── a thread ────────────────────────────────────────────────────────────

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadView {
    pub project: String,
    pub thread_id: String,
    pub records: Vec<RecordView>,
    /// Every record of the thread, readable or not; the rows are only the
    /// readable ones.
    pub total: i64,
    pub truncated: bool,
    pub as_of: String,
}

/// Fold `record_thread`: the readable records in `happened_at` order, each
/// with its own projections.
pub fn thread_view(project: &str, thread: &Value) -> ThreadView {
    let thread = map_of(Some(thread));
    let records: Vec<RecordView> = rows(thread.get("records"))
        .iter()
        .filter_map(|view| record_view(project, view))
        .collect();
    ThreadView {
        project: project.to_owned(),
        thread_id: optional_text(thread.get("thread_id")).unwrap_or_else(|| {
            records
                .first()
                .and_then(|view| view.record.row.thread_id.clone())
                .unwrap_or_default()
        }),
        total: integer(thread.get("total")).max(records.len() as i64),
        truncated: thread.get("truncated") == Some(&Value::Bool(true)),
        as_of: text(thread.get("as_of")),
        records,
    }
}

// ── a server page of records ────────────────────────────────────────────

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordPage {
    pub project: String,
    #[serde(flatten)]
    pub bounds: PageBounds,
    /// The server scanned its cap and stopped: the total counts what was
    /// scanned, not what the project holds.
    pub truncated: bool,
    pub scanned: i64,
    pub as_of: String,
    pub records: Vec<super::reads::RecordRow>,
}

/// Fold one `record_list` page. The server's page is 1-based; the CLI's is
/// 0-based like every other `ds pm` list, so the bounds are restated here.
pub fn record_page(project: &str, page: &Value) -> RecordPage {
    let page = map_of(Some(page));
    let records: Vec<super::reads::RecordRow> = rows(page.get("records"))
        .iter()
        .filter_map(decode_context_record)
        .map(|entry| record_row(&entry))
        .collect();
    let total = integer(page.get("total")).max(0) as usize;
    let page_size = integer(page.get("limit")).max(1) as usize;
    let page_index = (integer(page.get("page")).max(1) - 1) as usize;
    let page_count = total.div_ceil(page_size).max(1);
    let from = if records.is_empty() {
        0
    } else {
        page_index * page_size + 1
    };
    RecordPage {
        project: project.to_owned(),
        bounds: PageBounds {
            total,
            page: page_index,
            page_size,
            page_count,
            from,
            to: if records.is_empty() {
                0
            } else {
                from + records.len() - 1
            },
        },
        truncated: page.get("truncated") == Some(&Value::Bool(true)),
        scanned: integer(page.get("scanned")),
        as_of: text(page.get("as_of")),
        records,
    }
}

// ── parties ─────────────────────────────────────────────────────────────

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartyRow {
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organisation_party_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub emails: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub phones: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub archived: bool,
    pub version: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// One party envelope (`{id, data, version, metadata}`) as a row.
pub fn party_row(envelope: &Value) -> Option<PartyRow> {
    let outer = map_of(Some(envelope));
    let data = map_of(outer.get("data"));
    let metadata = map_of(outer.get("metadata"));
    Some(PartyRow {
        id: optional_text(outer.get("id")).or_else(|| optional_text(data.get("id")))?,
        kind: text(data.get("kind")),
        name: text(data.get("name")),
        role: optional_text(data.get("role")),
        organisation_party_id: optional_text(data.get("organisation_party_id")),
        emails: texts(data.get("emails")),
        phones: texts(data.get("phones")),
        notes: optional_text(data.get("notes")),
        archived: data.get("archived") == Some(&Value::Bool(true)),
        version: integer(outer.get("version")),
        updated_at: optional_instant(metadata.get("updated_at")),
    })
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartyPage {
    pub project: String,
    #[serde(flatten)]
    pub bounds: PageBounds,
    pub truncated: bool,
    pub scanned: i64,
    pub parties: Vec<PartyRow>,
}

/// Fold one `party_list` page (1-based on the server, 0-based here).
pub fn party_page(project: &str, page: &Value) -> PartyPage {
    let page = map_of(Some(page));
    let parties: Vec<PartyRow> = rows(page.get("parties"))
        .iter()
        .filter_map(party_row)
        .collect();
    let total = integer(page.get("total")).max(0) as usize;
    let page_size = integer(page.get("limit")).max(1) as usize;
    let page_index = (integer(page.get("page")).max(1) - 1) as usize;
    let from = if parties.is_empty() {
        0
    } else {
        page_index * page_size + 1
    };
    PartyPage {
        project: project.to_owned(),
        bounds: PageBounds {
            total,
            page: page_index,
            page_size,
            page_count: total.div_ceil(page_size).max(1),
            from,
            to: if parties.is_empty() {
                0
            } else {
                from + parties.len() - 1
            },
        },
        truncated: page.get("truncated") == Some(&Value::Bool(true)),
        scanned: integer(page.get("scanned")),
        parties,
    }
}

// ── a task's timeline ───────────────────────────────────────────────────

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEntry {
    pub at: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    pub task_id: String,
    pub entries: Vec<TimelineEntry>,
    pub total: i64,
    pub truncated: bool,
    /// How many rows each source contributed, as the server counted them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Value>,
}

/// Fold `task_read --timeline`'s `timeline`: records, attachments, assets,
/// state changes, assignments, commands and blockers in time order.
pub fn timeline(value: &Value) -> Option<Timeline> {
    let raw = map_of(Some(value));
    if raw.is_empty() {
        return None;
    }
    let all: Vec<TimelineEntry> = rows(raw.get("entries"))
        .iter()
        .filter_map(|row| {
            let entry = map_of(Some(row));
            Some(TimelineEntry {
                at: optional_instant(entry.get("at")).unwrap_or_default(),
                kind: optional_text(entry.get("kind"))?,
                entity_id: optional_text(entry.get("entity_id")),
                summary: text(entry.get("summary")),
                actor: optional_text(entry.get("actor")),
                detail: entry
                    .get("detail")
                    .filter(|detail| detail.is_object() && !map_of(Some(detail)).is_empty())
                    .cloned(),
            })
        })
        .collect();
    let total = integer(raw.get("total")).max(all.len() as i64);
    let cut = all.len() > MAX_TIMELINE_ENTRIES;
    Some(Timeline {
        task_id: text(raw.get("task_id")),
        entries: all.into_iter().take(MAX_TIMELINE_ENTRIES).collect(),
        total,
        truncated: raw.get("truncated") == Some(&Value::Bool(true)) || cut,
        sources: raw
            .get("sources")
            .filter(|value| value.is_object())
            .cloned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(id: &str, extra: Value) -> Value {
        let mut data = json!({
            "category": "request_for_information", "channel": "email", "direction": "outbound",
            "subject": "Questions on spans", "happened_at": "2026-09-20T09:00:00Z", "thread_id": "R1",
            "response_required": true, "response_owner_party_id": "p_tbea", "response_due_date": "2026-09-24",
            "external_party_ids": ["p_tbea"], "source_asset_id": "a_mail", "sensitivity": "internal",
            "resulting_task_ids": ["T0"],
        });
        for (key, value) in extra.as_object().expect("extra") {
            data[key] = value.clone();
        }
        json!({"id": id, "version": 3, "data": data,
               "computed": {"response_status": "outstanding", "days_until_due": 4, "days_overdue": 0}})
    }

    #[test]
    fn a_record_view_carries_the_record_and_what_the_server_projected_beside_it() {
        let view = record_view("p", &json!({
            "item": record("R5", json!({})),
            "attachments": [
                {"asset_id": "a_mail", "name": "review.eml", "kind": "mail", "format": "eml", "bytes": 1805,
                 "sensitivity": "internal", "folder": "correspondence", "bytes_held": true,
                 "mail": {"parts": [{"path": "part-1-1.txt"}, {"path": "picture.png"}]}},
                {"asset_id": "a_drive", "name": "package", "kind": "pack", "format": "unknown", "bytes": 18033672,
                 "sensitivity": "confidential", "folder": "correspondence", "bytes_held": false,
                 "external": {"url": "https://drive.google.com/x", "provider": "google_drive"}},
                {"name": "no id"},
            ],
            "attachments_total": 2,
            "documents": [{"asset_id": "a_gtp", "name": "GTP.pdf", "kind": "doc", "format": "pdf", "bytes": 10,
                           "sensitivity": "internal", "folder": "", "document": {"number": "GTP-001", "revision_label": "B", "state": "issued"}}],
            "blocked_tasks": [{"task_id": "T2b", "title": "Replace spans", "set_by": "lead@ds.rw", "set_at": 1_789_689_600_000i64}],
            "resulting_tasks": [{"task_id": "T0", "title": "Address review"}],
            "thread_total": 4, "thread_truncated": false, "as_of": "2026-09-20",
        }))
        .expect("view");
        assert_eq!(view.record.row.id, "R5");
        assert_eq!(
            view.record.row.response_status.as_deref(),
            Some("outstanding")
        );
        assert_eq!(
            view.record.row.response_owner_party_id.as_deref(),
            Some("p_tbea")
        );
        assert_eq!(view.record.days_until_due, 4);
        assert_eq!(view.record.source_asset_id.as_deref(), Some("a_mail"));
        assert_eq!(view.record.resulting_task_ids, vec!["T0"]);
        assert_eq!(
            view.attachments.len(),
            2,
            "a row without an id is not an attachment"
        );
        assert_eq!(view.attachments_total, 2);
        assert_eq!(
            view.attachments[0].mail_parts,
            vec!["part-1-1.txt", "picture.png"]
        );
        assert!(!view.attachments[1].bytes_held);
        assert_eq!(
            view.attachments[1].external_url.as_deref(),
            Some("https://drive.google.com/x")
        );
        assert_eq!(
            view.documents[0].document.as_ref().unwrap()["number"],
            "GTP-001"
        );
        assert_eq!(
            view.blocked_tasks[0].set_at.as_deref(),
            Some("2026-09-18T00:00:00.000Z")
        );
        assert_eq!(view.resulting_tasks[0].task_id, "T0");
        assert_eq!(view.thread_total, 4);
        assert!(
            record_view("p", &json!({"item": {"data": {}}})).is_none(),
            "an idless item is no view"
        );
    }

    #[test]
    fn a_thread_lists_its_readable_records_and_counts_them_all() {
        let thread = thread_view(
            "p",
            &json!({
                "thread_id": "R1", "total": 3, "truncated": false, "as_of": "2026-09-20",
                "records": [
                    {"item": record("R1", json!({"direction": "inbound"})), "attachments": []},
                    {"item": record("R5", json!({}))},
                ],
            }),
        );
        assert_eq!(thread.thread_id, "R1");
        assert_eq!(thread.records.len(), 2);
        assert_eq!(
            thread.total, 3,
            "a record the caller may not read still counts"
        );
        assert_eq!(thread.records[1].record.row.id, "R5");
    }

    #[test]
    fn a_server_page_is_restated_in_the_clis_zero_based_bounds() {
        let page = record_page(
            "p",
            &json!({
                "records": [record("R5", json!({})), record("R2", json!({}))],
                "total": 7, "page": 2, "limit": 2, "scanned": 7, "truncated": false, "as_of": "2026-09-20",
            }),
        );
        assert_eq!(page.bounds.total, 7);
        assert_eq!(
            page.bounds.page, 1,
            "the server's page 2 is the CLI's page 1"
        );
        assert_eq!(page.bounds.page_count, 4);
        assert_eq!((page.bounds.from, page.bounds.to), (3, 4));
        assert_eq!(page.records[0].id, "R5");
        assert_eq!(page.records[0].party_ids, vec!["p_tbea"]);
        let empty = record_page(
            "p",
            &json!({"records": [], "total": 0, "page": 1, "limit": 50}),
        );
        assert_eq!(
            (empty.bounds.from, empty.bounds.to, empty.bounds.page_count),
            (0, 0, 1)
        );
    }

    #[test]
    fn parties_read_their_envelope_and_page_the_same_way() {
        let page = party_page(
            "p",
            &json!({
                "parties": [
                    {"id": "p_tbea", "version": 1, "data": {"kind": "organisation", "name": "TBEA Co., Ltd", "role": "contractor", "emails": ["a@example.com"]},
                     "metadata": {"updated_at": 1_789_689_600_000i64}},
                    {"id": "p_zhang", "version": 2, "data": {"kind": "person", "name": "Zhang", "organisation_party_id": "p_tbea", "archived": true}},
                    {"data": {"name": "no id"}},
                ],
                "total": 2, "page": 1, "limit": 50, "scanned": 2, "truncated": false,
            }),
        );
        assert_eq!(page.parties.len(), 2);
        assert_eq!(page.parties[0].role.as_deref(), Some("contractor"));
        assert_eq!(
            page.parties[0].updated_at.as_deref(),
            Some("2026-09-18T00:00:00.000Z")
        );
        assert!(page.parties[1].archived);
        assert_eq!(
            page.parties[1].organisation_party_id.as_deref(),
            Some("p_tbea")
        );
        assert_eq!(page.bounds.total, 2);
    }

    #[test]
    fn a_timeline_keeps_the_servers_order_and_its_bound() {
        let folded = timeline(&json!({
            "task_id": "T2b",
            "entries": [
                {"at": "2026-09-20T09:00:00Z", "kind": "blocker_set", "entity_id": "R5", "summary": "blocked on R5", "actor": "lead@ds.rw", "detail": {"record_id": "R5"}},
                {"at": 1_789_689_600_000i64, "kind": "created", "summary": "created", "detail": {}},
                {"summary": "no kind"},
            ],
            "total": 2, "truncated": false,
            "sources": {"records": 1, "blockers": 1},
        }))
        .expect("timeline");
        assert_eq!(folded.entries.len(), 2);
        assert_eq!(folded.entries[0].kind, "blocker_set");
        assert_eq!(
            folded.entries[0].detail.as_ref().unwrap()["record_id"],
            "R5"
        );
        assert_eq!(folded.entries[1].at, "2026-09-18T00:00:00.000Z");
        assert!(
            folded.entries[1].detail.is_none(),
            "an empty detail is omitted"
        );
        assert_eq!(folded.sources.as_ref().unwrap()["blockers"], 1);
        assert!(timeline(&Value::Null).is_none());
    }
}
