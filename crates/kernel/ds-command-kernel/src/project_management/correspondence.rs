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

/// One Project Work attachment on the record itself (`pm.record.attach`), in
/// whatever state its upload reached: only `ready` is a delivered file, and
/// `uploading` is an interrupted upload that attaching the same file resumes.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordAttachment {
    pub id: String,
    pub version: i64,
    pub name: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// The `sys:pm_attachment:<id>` that `ds assets read` and `preview` open.
    pub asset_id: String,
}

/// Brain's canonical envelope: top-level id/version, authored fields in `data`.
fn record_attachment(row: &Value) -> Option<RecordAttachment> {
    let envelope = map_of(Some(row));
    let id = optional_text(envelope.get("id"))?;
    let data = map_of(envelope.get("data"));
    Some(RecordAttachment {
        asset_id: format!("sys:pm_attachment:{id}"),
        id,
        version: integer(envelope.get("version")),
        name: text(data.get("original_name")),
        content_type: text(data.get("content_type")),
        size_bytes: integer(data.get("size_bytes")),
        state: text(data.get("state")),
        created_by: optional_text(map_of(envelope.get("metadata")).get("created_by")),
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
    /// Files uploaded onto the record itself, with their upload state.
    pub pm_attachments: Vec<RecordAttachment>,
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
        pm_attachments: rows(view.get("pm_attachments"))
            .iter()
            .filter_map(record_attachment)
            .take(MAX_ATTACHMENT_ROWS)
            .collect(),
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
#[path = "correspondence_tests.rs"]
mod tests;
