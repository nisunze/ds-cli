//! One Project Work attachment upload, decided once for every host.
//!
//! Core (CLI/MCP), the Desktop and the browser run the same steps for one file
//! on one project entity:
//! 1. on a task or a record, survey its attachments for the caller's own
//!    interrupted reservations of this exact file (same name and size,
//!    `uploading` or `failed`);
//! 2. restart the newest `uploading` one, so a retry completes that row
//!    instead of adding another, or reserve a new row;
//! 3. send the bytes to the minted storage session;
//! 4. finalize, accepting only a ready row on this entity;
//! 5. on a task, detach the caller's other reservations of the same file.
//!
//! A host executes the one effect a step names (a governed PM action or one
//! byte upload) and reports what it observed. Each host's adapter lives in its
//! own file beside that host; nothing here knows which host runs it.
//!
//! Pure by the crate's contract: no clock, IO or network.

use super::task_attachment_detach::{reference, Request as Detach};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The largest file Brain reserves: Cloud Storage's own 5 TiB object
/// maximum. There is no product cap: the bytes go straight to storage in
/// resumable chunks, so neither a host nor Brain ever holds the whole file.
pub const MAX_ATTACHMENT_BYTES: u64 = 5 << 40;
const SURVEY_PAGE: u64 = 100;
const MAX_SURVEY_PAGES: u32 = 20;
const MAX_RECLAIM: usize = 16;
/// The most steps one upload takes; a host stops beyond it.
pub const MAX_STEPS: usize = MAX_SURVEY_PAGES as usize + MAX_RECLAIM + 8;

const ENTITY_TYPES: [&str; 5] = ["task", "comment", "residual", "record", "activity"];
const METADATA: [&str; 5] = [
    "artifact_kind",
    "document_number",
    "revision_label",
    "document_state",
    "capture",
];

pub const INPUT_INVALID: &str = "attachment_input_invalid";
pub const ANSWER_UNREADABLE: &str = "attachment_answer_unreadable";
pub const BYTES_INCOMPLETE: &str = "attachment_bytes_incomplete";
pub const UNVERIFIED: &str = "attachment_unverified";
pub const CANCELLED: &str = "attachment_upload_cancelled";

/// The file and the entity it is attached to.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Upload {
    pub project: String,
    pub linked_entity_type: String,
    pub linked_entity_id: String,
    /// The signed-in principal; only its own reservations are restarted or
    /// detached.
    pub principal: String,
    pub name: String,
    pub content_type: String,
    pub size_bytes: u64,
    /// Authored attachment fields sent once with a new reservation.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub metadata: Map<String, Value>,
}

impl Upload {
    fn validate(&self) -> Result<(), &'static str> {
        let media = self.content_type.split(';').next().unwrap_or_default();
        let (kind, subtype) = media.trim().split_once('/').unwrap_or_default();
        if !reference(&self.project)
            || !ENTITY_TYPES.contains(&self.linked_entity_type.as_str())
            || !reference(&self.linked_entity_id)
            || owner(&self.principal).is_empty()
            || self.name.trim().is_empty()
            || self.name.chars().any(char::is_control)
            || kind.is_empty()
            || subtype.is_empty()
            || !(1..=MAX_ATTACHMENT_BYTES).contains(&self.size_bytes)
            || self
                .metadata
                .keys()
                .any(|key| !METADATA.contains(&key.as_str()))
        {
            return Err(
                "an attachment needs one project entity, the signed-in principal, a file name, a type/subtype and 1 byte to 5 TiB",
            );
        }
        Ok(())
    }

    fn task(&self) -> bool {
        self.linked_entity_type == "task"
    }

    /// The entities whose attachments `get_context` reads by id, so a retry
    /// can find and complete the caller's interrupted reservation.
    fn context_key(&self) -> Option<&'static str> {
        match self.linked_entity_type.as_str() {
            "task" => Some("task_id"),
            "record" => Some("record_id"),
            _ => None,
        }
    }
}

/// One of the caller's own earlier reservations of the same file.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Reservation {
    pub id: String,
    pub version: i64,
    pub created_at: i64,
    pub uploading: bool,
}

/// Where the upload stands; the host carries it back unchanged.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "stage", rename_all = "snake_case", deny_unknown_fields)]
pub enum Continuation {
    Survey {
        upload: Upload,
        pages: u32,
        found: Vec<Reservation>,
    },
    Reserve {
        upload: Upload,
        restart: Option<Reservation>,
        reclaim: Vec<Reservation>,
    },
    Send {
        upload: Upload,
        id: String,
        version: i64,
        restarted: bool,
        reclaim: Vec<Reservation>,
    },
    Finalize {
        upload: Upload,
        id: String,
        restarted: bool,
        reclaim: Vec<Reservation>,
    },
    Reclaim {
        upload: Upload,
        item: Value,
        restarted: bool,
        pending: Vec<Reservation>,
        reclaimed: Vec<String>,
        unreclaimed: Vec<String>,
    },
    Cancel,
}

/// The one effect the host executes next, or the outcome.
#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    /// Post `body` to the governed PM route and answer its `data`.
    Pm {
        body: Value,
        continuation: Continuation,
    },
    /// Send exactly `size_bytes` of the file to this resumable session. The
    /// session is transport authority: spend it here, never keep it. The
    /// reserved row's id names the upload for the host's queue.
    Upload {
        attachment_id: String,
        session_uri: String,
        size_bytes: u64,
        continuation: Continuation,
    },
    /// The verified ready row as Brain answered it (`item`) and as a flat
    /// receipt row (`row`).
    Done {
        item: Value,
        row: Value,
        restarted: bool,
        reclaimed: Vec<String>,
        unreclaimed: Vec<String>,
    },
    /// The last effect failed at the host; surface the host's own error.
    HostFailed,
    Refused {
        code: &'static str,
        message: &'static str,
    },
}

/// What the host observed for the effect it executed.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Observed {
    Pm { data: Value },
    PmFailed,
    Upload { status: u16 },
    UploadFailed { cancelled: bool },
}

pub fn start(upload: Upload) -> Step {
    if let Err(message) = upload.validate() {
        return refused(INPUT_INVALID, message);
    }
    if upload.context_key().is_some() {
        survey(upload, 0, Vec::new(), None)
    } else {
        reserve(upload, None, Vec::new())
    }
}

pub fn resume(continuation: Continuation, observed: Observed) -> Step {
    match (continuation, observed) {
        (
            Continuation::Survey {
                upload,
                pages,
                mut found,
            },
            Observed::Pm { data },
        ) => {
            let context = &data["context"];
            let Some(rows) = context["attachments"].as_array() else {
                return refused(
                    ANSWER_UNREADABLE,
                    "the task context answer has no attachment list",
                );
            };
            found.extend(rows.iter().filter_map(|row| reservation(&upload, row)));
            let next = context["next_cursors"]["pm_attachments"]
                .as_str()
                .filter(|cursor| reference(cursor));
            match next {
                Some(cursor)
                    if context["truncated_by_collection"]["pm_attachments"] == true
                        && pages < MAX_SURVEY_PAGES =>
                {
                    survey(upload, pages, found, Some(cursor.to_owned()))
                }
                _ => plan(upload, found),
            }
        }
        (
            Continuation::Reserve {
                upload,
                restart,
                reclaim,
            },
            Observed::Pm { data },
        ) => {
            let item = &data["item"];
            let (Some(id), Some(version)) = (
                item["id"].as_str().filter(|id| reference(id)),
                item["version"].as_i64().filter(|version| *version >= 1),
            ) else {
                return refused(
                    ANSWER_UNREADABLE,
                    "the attachment reservation has no id or version",
                );
            };
            let fenced = restart
                .as_ref()
                .is_none_or(|row| row.id == id && row.version + 1 == version);
            if !fenced || !on_entity(&upload, item, "uploading") {
                return refused(
                    ANSWER_UNREADABLE,
                    "project management did not reserve this file on this entity",
                );
            }
            let Some(session_uri) = data["session_uri"].as_str().filter(|uri| session(uri)) else {
                return refused(
                    ANSWER_UNREADABLE,
                    "project management did not mint a storage upload session",
                );
            };
            Step::Upload {
                attachment_id: id.to_owned(),
                session_uri: session_uri.to_owned(),
                size_bytes: upload.size_bytes,
                continuation: Continuation::Send {
                    id: id.to_owned(),
                    version,
                    restarted: restart.is_some(),
                    reclaim,
                    upload,
                },
            }
        }
        (
            Continuation::Send {
                upload,
                id,
                version,
                restarted,
                reclaim,
            },
            Observed::Upload { status: 200 | 201 },
        ) => Step::Pm {
            body: json!({"action": "finalize_attachment_upload", "ds_project": upload.project,
                "id": id, "expected_version": version}),
            continuation: Continuation::Finalize {
                upload,
                id,
                restarted,
                reclaim,
            },
        },
        (
            Continuation::Send {
                upload,
                id,
                version,
                ..
            },
            Observed::UploadFailed { cancelled: true },
        ) => cancel(upload, id, version),
        (
            Continuation::Send { upload, .. },
            Observed::Upload { .. } | Observed::UploadFailed { .. },
        ) => incomplete(&upload),
        (
            Continuation::Finalize {
                upload,
                id,
                restarted,
                reclaim,
            },
            Observed::Pm { data },
        ) => {
            let item = &data["item"];
            if item["id"] != id.as_str()
                || item["version"].as_i64().is_none_or(|version| version < 1)
                || !on_entity(&upload, item, "ready")
            {
                return refused(
                    UNVERIFIED,
                    "project management did not verify the uploaded attachment as ready on this entity",
                );
            }
            reclaim_next(
                upload,
                item.clone(),
                restarted,
                reclaim,
                Vec::new(),
                Vec::new(),
            )
        }
        (
            Continuation::Reclaim {
                upload,
                item,
                restarted,
                mut pending,
                mut reclaimed,
                mut unreclaimed,
            },
            observed @ (Observed::Pm { .. } | Observed::PmFailed),
        ) if !pending.is_empty() => {
            let row = pending.remove(0);
            let detached = matches!(&observed, Observed::Pm { data }
                if detach(&upload, &row).verify(&upload.project, data).is_ok());
            if detached {
                reclaimed.push(row.id);
            } else {
                unreclaimed.push(row.id);
            }
            reclaim_next(upload, item, restarted, pending, reclaimed, unreclaimed)
        }
        (Continuation::Cancel, Observed::Pm { .. } | Observed::PmFailed) => {
            refused(CANCELLED, "the attachment upload was cancelled")
        }
        (_, Observed::PmFailed) => Step::HostFailed,
        _ => refused(
            ANSWER_UNREADABLE,
            "the attachment upload observed an effect out of order",
        ),
    }
}

fn survey(upload: Upload, pages: u32, found: Vec<Reservation>, cursor: Option<String>) -> Step {
    let mut body = json!({"action": "get_context", "ds_project": upload.project,
        "limit": SURVEY_PAGE, "collections": ["pm_attachments"]});
    body[upload.context_key().unwrap_or("task_id")] = json!(upload.linked_entity_id);
    if let Some(cursor) = cursor {
        body["cursors"] = json!({"pm_attachments": cursor});
    }
    Step::Pm {
        body,
        continuation: Continuation::Survey {
            upload,
            pages: pages + 1,
            found,
        },
    }
}

/// The newest `uploading` reservation is completed; on a task every other one
/// is detached once the file is ready. Only a task has a relationship detach,
/// so a record's older reservations are left to ds-brain's own reclaim of the
/// caller's abandoned uploads.
fn plan(upload: Upload, mut found: Vec<Reservation>) -> Step {
    found.sort_by(|a, b| {
        (b.uploading, b.created_at, &b.id).cmp(&(a.uploading, a.created_at, &a.id))
    });
    found.dedup_by(|a, b| a.id == b.id);
    let restart = found.first().filter(|row| row.uploading).cloned();
    let reclaim = if upload.task() {
        found
            .into_iter()
            .skip(usize::from(restart.is_some()))
            .take(MAX_RECLAIM)
            .collect()
    } else {
        Vec::new()
    };
    reserve(upload, restart, reclaim)
}

fn reserve(upload: Upload, restart: Option<Reservation>, reclaim: Vec<Reservation>) -> Step {
    let body = match &restart {
        Some(row) => json!({"action": "restart_attachment_upload", "ds_project": upload.project,
            "id": row.id, "expected_version": row.version}),
        None => {
            let mut data = Map::new();
            data.insert(
                "linked_entity_type".into(),
                json!(upload.linked_entity_type),
            );
            data.insert("linked_entity_id".into(), json!(upload.linked_entity_id));
            data.insert("original_name".into(), json!(upload.name));
            data.insert("content_type".into(), json!(upload.content_type));
            data.insert("size_bytes".into(), json!(upload.size_bytes));
            data.extend(upload.metadata.clone());
            json!({"action": "start_attachment_upload", "ds_project": upload.project, "data": data})
        }
    };
    Step::Pm {
        body,
        continuation: Continuation::Reserve {
            upload,
            restart,
            reclaim,
        },
    }
}

fn reclaim_next(
    upload: Upload,
    item: Value,
    restarted: bool,
    pending: Vec<Reservation>,
    reclaimed: Vec<String>,
    unreclaimed: Vec<String>,
) -> Step {
    let Some(row) = pending.first() else {
        return Step::Done {
            row: receipt_row(&item),
            item,
            restarted,
            reclaimed,
            unreclaimed,
        };
    };
    Step::Pm {
        body: detach(&upload, row).payload(&upload.project),
        continuation: Continuation::Reclaim {
            upload,
            item,
            restarted,
            pending,
            reclaimed,
            unreclaimed,
        },
    }
}

/// A cancelled task upload leaves the task; elsewhere the row is marked
/// failed, as the browser always did.
fn cancel(upload: Upload, id: String, version: i64) -> Step {
    let body = if upload.task() {
        let row = Reservation {
            id,
            version,
            created_at: 0,
            uploading: true,
        };
        detach(&upload, &row).payload(&upload.project)
    } else {
        json!({"action": "update_attachment", "ds_project": upload.project, "id": id,
            "expected_version": version, "data": {"state": "failed"}})
    };
    Step::Pm {
        body,
        continuation: Continuation::Cancel,
    }
}

fn incomplete(upload: &Upload) -> Step {
    refused(
        BYTES_INCOMPLETE,
        if upload.context_key().is_some() {
            "the attachment bytes did not reach storage; the reserved row stays uploading and attaching the same file to this task or record again completes it"
        } else {
            "the attachment bytes did not reach storage; the reserved row stays uploading until it is reclaimed"
        },
    )
}

fn detach(upload: &Upload, row: &Reservation) -> Detach {
    Detach {
        task_id: upload.linked_entity_id.clone(),
        attachment_id: row.id.clone(),
        expected_version: row.version,
    }
}

/// The caller's own live reservation of the same file on this entity.
fn reservation(upload: &Upload, row: &Value) -> Option<Reservation> {
    let data = &row["data"];
    let state = data["state"].as_str()?;
    let mine = row["metadata"]["created_by"]
        .as_str()
        .is_some_and(|by| owner(by) == owner(&upload.principal));
    let same_file = data["original_name"]
        .as_str()
        .is_some_and(|name| name.trim() == upload.name.trim())
        && data["size_bytes"].as_u64() == Some(upload.size_bytes);
    let live = row["is_deleted"] == false && row["task_detached"] != true;
    let on_entity = data["linked_entity_type"] == upload.linked_entity_type.as_str()
        && data["linked_entity_id"] == upload.linked_entity_id.as_str();
    if !(mine && same_file && live && on_entity && matches!(state, "uploading" | "failed")) {
        return None;
    }
    Some(Reservation {
        id: row["id"].as_str().filter(|id| reference(id))?.to_owned(),
        version: row["version"].as_i64().filter(|version| *version >= 1)?,
        created_at: row["metadata"]["created_at"].as_i64().unwrap_or_default(),
        uploading: state == "uploading",
    })
}

/// Brain's canonical envelope: top-level id/version, authored fields in `data`.
fn on_entity(upload: &Upload, item: &Value, state: &str) -> bool {
    let data = &item["data"];
    item["is_deleted"] == false
        && data["state"] == state
        && data["linked_entity_type"] == upload.linked_entity_type.as_str()
        && data["linked_entity_id"] == upload.linked_entity_id.as_str()
        && data["original_name"]
            .as_str()
            .is_some_and(|name| name.trim() == upload.name.trim())
        && data["size_bytes"].as_u64() == Some(upload.size_bytes)
}

fn receipt_row(item: &Value) -> Value {
    let data = &item["data"];
    let mut row = json!({"id": item["id"], "version": item["version"]});
    for key in [
        "linked_entity_type",
        "linked_entity_id",
        "original_name",
        "content_type",
        "size_bytes",
        "state",
        "checksum",
        "generation",
    ] {
        if let Some(value) = data.get(key) {
            row[key] = value.clone();
        }
    }
    row
}

/// A Google Cloud Storage JSON-API resumable session, the only kind Brain mints.
fn session(uri: &str) -> bool {
    let Ok(url) = url::Url::parse(uri) else {
        return false;
    };
    uri.len() <= 8192
        && url.scheme() == "https"
        && url.host_str() == Some("storage.googleapis.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && url.path().starts_with("/upload/storage/v1/b/")
        && url.path().ends_with("/o")
        && url
            .query_pairs()
            .any(|(key, value)| key == "upload_id" && !value.is_empty())
}

fn owner(email: &str) -> String {
    email.trim().to_lowercase()
}

fn refused(code: &'static str, message: &'static str) -> Step {
    Step::Refused { code, message }
}

#[derive(Deserialize)]
#[serde(tag = "action", deny_unknown_fields)]
enum Call {
    #[serde(rename = "attachment.start")]
    Start {
        #[allow(dead_code)]
        schema: String,
        upload: Upload,
    },
    #[serde(rename = "attachment.resume")]
    Resume {
        #[allow(dead_code)]
        schema: String,
        continuation: Box<Continuation>,
        observed: Observed,
    },
}

/// The browser entry, reached through `projectManagement`.
pub(super) fn evaluate(input: &[u8]) -> Result<String, String> {
    let step = match serde_json::from_slice(input).map_err(|error| error.to_string())? {
        Call::Start { upload, .. } => start(upload),
        Call::Resume {
            continuation,
            observed,
            ..
        } => resume(*continuation, observed),
    };
    serde_json::to_string(&step).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "attachment_upload_tests.rs"]
mod tests;
