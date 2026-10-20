//! Personal notes: one account's private notes and their attachments.
//!
//! Brain lists a note's metadata page by page and answers a whole note on
//! `get` and after every change. The page reads both in one shape: a listed
//! note carries neutral content until it is opened. A change names the
//! version it edits; a newer one refuses `NOTE_VERSION_CONFLICT`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, Admitted, Exchange, Plan, Refusal, Request};

const PATH: &str = "/api/v1/notes";
/// Pages one listing follows before it stops; each page is Brain-bounded.
const MAX_PAGES: usize = 20;
/// Positions one note geometry may carry.
const MAX_POSITIONS: usize = 2_000;
pub const INVALID: &str = "notes_answer_invalid";

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
#[wasm_bindgen::prelude::wasm_bindgen(typescript_custom_section)]
const NOTE_GEOMETRY: &'static str = r#"
/** Where a note sits on the map. */
export type NoteGeometry =
    | { type: 'Point'; coordinates: [number, number] }
    | { type: 'LineString'; coordinates: [number, number][] }
    | { type: 'Polygon'; coordinates: [[number, number][]] };
"#;

/// One checklist line.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct NoteChecklistItem {
    pub id: String,
    pub text: String,
    pub checked: bool,
}

/// Where a note sits on the board.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct NoteCanvas {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub z: i64,
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "'sand' | 'rose' | 'mint' | 'sky' | 'lilac' | 'slate'")
    )]
    pub color: String,
}

impl Default for NoteCanvas {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 230.0,
            z: 0,
            color: "sand".into(),
        }
    }
}

/// A reminder Brain fires once.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct NoteReminder {
    pub at: String,
    pub timezone: String,
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "'scheduled' | 'dismissed' | 'fired'")
    )]
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fired_at: Option<String>,
}

/// What a note is linked to in a project.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct NoteLink {
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(
            type = "'project' | 'pm_task' | 'survey_entry' | 'project_record' | 'deliverable_version' | 'ds_object'"
        )
    )]
    pub kind: String,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_version: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attached_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attached_at: Option<String>,
}

/// One note as the page shows it.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct PersonalNote {
    pub id: String,
    pub version: i64,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    pub markdown: String,
    pub checklist: Vec<NoteChecklistItem>,
    pub tags: Vec<String>,
    pub links: Vec<NoteLink>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "NoteGeometry")
    )]
    pub geometry: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reminder: Option<NoteReminder>,
    pub canvas: NoteCanvas,
    pub pinned: bool,
    pub archived: bool,
    pub trashed: bool,
    pub created_at: String,
    pub created_by: String,
    pub updated_at: String,
    pub updated_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trashed_at: Option<String>,
}

/// One attachment of a note.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct NoteAttachment {
    pub id: String,
    pub note_id: String,
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "'uploading' | 'ready' | 'failed' | 'deleted'")
    )]
    pub state: String,
    pub original_name: String,
    pub content_type: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    pub created_at: String,
    pub created_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready_at: Option<String>,
}

/// A note opened with its attachments.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct OpenedNote {
    pub note: PersonalNote,
    pub attachments: Vec<NoteAttachment>,
}

/// A short-lived read of one attachment.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct NoteAttachmentDownload {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    pub expires_at: String,
}

/// A note as Brain writes it.
#[derive(Deserialize)]
struct WireNote {
    id: String,
    #[serde(default)]
    version: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    content: Option<WireContent>,
    #[serde(default)]
    checklist: Option<Vec<NoteChecklistItem>>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    links: Option<Vec<Value>>,
    #[serde(default)]
    geometry: Option<Value>,
    #[serde(default)]
    reminder: Option<NoteReminder>,
    #[serde(default)]
    canvas: Option<NoteCanvas>,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    trashed: bool,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    created_by: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    updated_by: String,
    #[serde(default)]
    trashed_at: Option<String>,
}

#[derive(Deserialize)]
struct WireContent {
    #[serde(default)]
    markdown: String,
}

fn position(value: &Value) -> bool {
    value.as_array().is_some_and(|pair| {
        pair.len() == 2 && pair.iter().all(|n| n.as_f64().is_some_and(f64::is_finite))
    })
}

fn positions(value: &Value, min: usize) -> bool {
    value.as_array().is_some_and(|line| {
        (min..=MAX_POSITIONS).contains(&line.len()) && line.iter().all(position)
    })
}

/// A Point, a LineString or a one-ring Polygon; anything else reads as none.
fn geometry(value: Option<Value>) -> Option<Value> {
    let value = value?;
    let coordinates = value.get("coordinates")?;
    let admitted = match value.get("type")?.as_str()? {
        "Point" => position(coordinates),
        "LineString" => positions(coordinates, 2),
        "Polygon" => coordinates
            .as_array()
            .is_some_and(|rings| rings.len() == 1 && positions(&rings[0], 4)),
        _ => false,
    };
    admitted.then_some(value)
}

fn nonempty(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.is_empty())
}

impl From<WireNote> for PersonalNote {
    fn from(wire: WireNote) -> Self {
        let links = wire
            .links
            .unwrap_or_default()
            .into_iter()
            .filter_map(|link| NoteLink::deserialize(link).ok())
            .filter(|link| !link.project_id.is_empty())
            .collect();
        Self {
            id: wire.id,
            version: wire.version,
            title: wire.title,
            folder: nonempty(wire.folder),
            markdown: wire
                .content
                .map(|content| content.markdown)
                .unwrap_or_default(),
            checklist: wire.checklist.unwrap_or_default(),
            tags: wire.tags.unwrap_or_default(),
            links,
            geometry: geometry(wire.geometry),
            reminder: wire.reminder.filter(|reminder| !reminder.at.is_empty()),
            canvas: wire.canvas.unwrap_or_default(),
            pinned: wire.pinned,
            archived: wire.archived,
            trashed: wire.trashed,
            created_at: wire.created_at,
            created_by: wire.created_by,
            updated_at: wire.updated_at,
            updated_by: wire.updated_by,
            trashed_at: nonempty(wire.trashed_at),
        }
    }
}

fn invalid(what: &str) -> Refusal {
    Refusal::new(
        INVALID,
        format!("The notes service returned an invalid {what}."),
    )
}

fn note(value: Option<&Value>) -> Result<PersonalNote, Refusal> {
    value
        .and_then(|value| WireNote::deserialize(value).ok())
        .filter(|wire| !wire.id.is_empty())
        .map(PersonalNote::from)
        .ok_or_else(|| invalid("note"))
}

fn attachment(value: Option<&Value>) -> Result<NoteAttachment, Refusal> {
    value
        .and_then(|value| NoteAttachment::deserialize(value).ok())
        .filter(|attachment| !attachment.id.is_empty() && !attachment.note_id.is_empty())
        .ok_or_else(|| invalid("attachment"))
}

fn post(action: &str, mut body: Value) -> Request {
    body["action"] = action.into();
    Request::post(PATH, body)
}

/// A draft, as the page edits it, in the shape Brain stores.
fn wire_draft(draft: &Value) -> Result<Value, Refusal> {
    let text = |name: &str| draft.get(name).and_then(Value::as_str).unwrap_or_default();
    let list = |name: &str| draft.get(name).cloned().unwrap_or_else(|| json!([]));
    if !draft.is_object() {
        return Err(Refusal::new(INVALID, "A note draft is an object."));
    }
    let links: Vec<Value> = draft
        .get("links")
        .and_then(Value::as_array)
        .map(|links| {
            links
                .iter()
                .filter_map(|link| {
                    let field = |name: &str| link.get(name).filter(|value| !value.is_null());
                    let mut wire = Map::new();
                    for (from, to) in [
                        ("kind", "kind"),
                        ("projectId", "project_id"),
                        ("entityId", "entity_id"),
                        ("formSlug", "form_slug"),
                        ("objectType", "object_type"),
                        ("objectRevision", "object_revision"),
                        ("modelVersion", "model_version"),
                        ("modelDigest", "model_digest"),
                        ("label", "label"),
                    ] {
                        if let Some(value) = field(from) {
                            wire.insert(to.into(), value.clone());
                        }
                    }
                    wire.contains_key("kind").then_some(Value::Object(wire))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut body = json!({
        "title": text("title"),
        "content": {"format": "markdown", "markdown": text("markdown")},
        "checklist": list("checklist"),
        "tags": list("tags"),
        "links": links,
        "canvas": draft.get("canvas").cloned().unwrap_or_else(|| json!(NoteCanvas::default())),
        "pinned": draft.get("pinned").and_then(Value::as_bool).unwrap_or(false),
        "archived": draft.get("archived").and_then(Value::as_bool).unwrap_or(false),
    });
    if !text("folder").is_empty() {
        body["folder"] = text("folder").into();
    }
    if let Some(geometry) = draft.get("geometry").filter(|value| !value.is_null()) {
        body["geometry"] = geometry.clone();
    }
    if let Some(reminder) = draft.get("reminder").filter(|value| !value.is_null()) {
        body["reminder"] = json!({"at": reminder.get("at"), "timezone": reminder.get("timezone"),
            "status": reminder.get("status")});
    }
    Ok(body)
}

/// Every note of the account, following Brain's pages.
#[derive(Clone, Debug, PartialEq)]
pub struct NotesList {
    pub include_archived: bool,
    pub include_trashed: bool,
    cursor: Option<String>,
    pages: usize,
    listed: Vec<PersonalNote>,
}

impl NotesList {
    pub fn new(include_archived: bool, include_trashed: bool) -> Self {
        Self {
            include_archived,
            include_trashed,
            cursor: None,
            pages: 0,
            listed: Vec::new(),
        }
    }
}

impl Exchange for NotesList {
    type Output = Vec<PersonalNote>;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut body = json!({"include_archived": self.include_archived,
            "include_trashed": self.include_trashed});
        if let Some(cursor) = &self.cursor {
            body["cursor"] = cursor.clone().into();
        }
        Ok(post("list", body).into())
    }

    fn admit(mut self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let page = data(&body);
        let notes = page
            .get("notes")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("note list"))?;
        self.listed.extend(
            notes
                .iter()
                .filter_map(|value| WireNote::deserialize(value).ok())
                .filter(|wire| !wire.id.is_empty())
                .map(PersonalNote::from),
        );
        self.pages += 1;
        let next = page
            .get("next_cursor")
            .and_then(Value::as_str)
            .filter(|cursor| !cursor.is_empty());
        match next {
            Some(cursor) if self.pages < MAX_PAGES => {
                self.cursor = Some(cursor.to_owned());
                Ok(Admitted::Next(self))
            }
            _ => Ok(Admitted::Done(self.listed)),
        }
    }
}

/// One note with its attachments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotesGet {
    pub id: String,
}

impl Exchange for NotesGet {
    type Output = OpenedNote;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(post("get", json!({"id": self.id})).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let opened = data(&body);
        let attachments = opened
            .get("attachments")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| attachment(Some(item)).ok())
                    .collect()
            })
            .unwrap_or_default();
        Ok(Admitted::Done(OpenedNote {
            note: note(opened.get("note"))?,
            attachments,
        }))
    }
}

/// Create a note, edit one at the version it read, or move one to or out of
/// the trash; each answers the note Brain now holds.
#[derive(Clone, Debug, PartialEq)]
pub enum NotesChange {
    Create {
        draft: Value,
    },
    Update {
        id: String,
        expected_version: i64,
        draft: Value,
    },
    Trash {
        id: String,
        expected_version: i64,
        trashed: bool,
    },
}

impl Exchange for NotesChange {
    type Output = PersonalNote;

    fn plan(&self) -> Result<Plan, Refusal> {
        let request = match self {
            NotesChange::Create { draft } => post("create", json!({"note": wire_draft(draft)?})),
            NotesChange::Update {
                id,
                expected_version,
                draft,
            } => post(
                "update",
                json!({"id": id, "expected_version": expected_version, "note": wire_draft(draft)?}),
            ),
            NotesChange::Trash {
                id,
                expected_version,
                trashed,
            } => post(
                if *trashed { "trash" } else { "restore" },
                json!({"id": id, "expected_version": expected_version}),
            ),
        };
        Ok(request.into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        note(data(&body).get("note")).map(Admitted::Done)
    }
}

/// Open an upload session for one attachment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentStart {
    pub note_id: String,
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: u64,
}

impl Exchange for AttachmentStart {
    /// The session URI and the attachment it fills.
    type Output = (String, NoteAttachment);

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({"id": self.note_id, "file_name": self.file_name,
            "content_type": self.content_type, "size_bytes": self.size_bytes});
        Ok(post("start_attachment_upload", body).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let started = data(&body);
        let session_uri = started
            .get("session_uri")
            .and_then(Value::as_str)
            .filter(|uri| !uri.is_empty())
            .ok_or_else(|| invalid("upload session"))?;
        let attachment = attachment(started.get("attachment"))?;
        Ok(Admitted::Done((session_uri.into(), attachment)))
    }
}

/// Verify an uploaded attachment; answers it ready.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentFinalize {
    pub note_id: String,
    pub attachment_id: String,
}

impl Exchange for AttachmentFinalize {
    type Output = NoteAttachment;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({"id": self.note_id, "attachment_id": self.attachment_id});
        Ok(post("finalize_attachment_upload", body).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        attachment(data(&body).get("attachment")).map(Admitted::Done)
    }
}

/// A short-lived read of one attachment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentDownload {
    pub note_id: String,
    pub attachment_id: String,
}

impl Exchange for AttachmentDownload {
    type Output = NoteAttachmentDownload;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({"id": self.note_id, "attachment_id": self.attachment_id});
        Ok(post("download_attachment_url", body).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        NoteAttachmentDownload::deserialize(data(&body))
            .ok()
            .filter(|download| !download.url.is_empty())
            .map(Admitted::Done)
            .ok_or_else(|| invalid("download"))
    }
}

/// Remove one attachment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentRemove {
    pub note_id: String,
    pub attachment_id: String,
}

impl Exchange for AttachmentRemove {
    type Output = ();

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({"id": self.note_id, "attachment_id": self.attachment_id});
        Ok(post("remove_attachment", body).into())
    }

    fn admit(self, _status: u16, _body: Value) -> Result<Admitted<Self>, Refusal> {
        Ok(Admitted::Done(()))
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use serde_json::Value;
    use wasm_bindgen::prelude::*;

    use super::{
        AttachmentDownload, AttachmentFinalize, AttachmentRemove, AttachmentStart, NotesChange,
        NotesGet, NotesList,
    };
    use crate::gateway_exchange::browser::{rejection, run, typed, GatewayHost};
    use crate::transfer::browser::exports::{send, UploadSource};

    fn draft(value: JsValue) -> Result<Value, JsValue> {
        tsify::serde_wasm_bindgen::from_value(value)
            .map_err(|error| js_sys::Error::new(&error.to_string()).into())
    }

    /// Every note of the account; a listed note carries neutral content
    /// until it is opened.
    #[wasm_bindgen(js_name = notesList, unchecked_return_type = "PersonalNote[]")]
    pub async fn notes_list(
        host: GatewayHost,
        include_archived: bool,
        include_trashed: bool,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, NotesList::new(include_archived, include_trashed)).await?)
    }

    /// One note with its attachments.
    #[wasm_bindgen(js_name = notesGet, unchecked_return_type = "OpenedNote")]
    pub async fn notes_get(host: GatewayHost, id: String) -> Result<JsValue, JsValue> {
        typed(&run(&host, NotesGet { id }).await?)
    }

    /// Create one note from a draft.
    #[wasm_bindgen(js_name = notesCreate, unchecked_return_type = "PersonalNote")]
    pub async fn notes_create(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "object")] note: JsValue,
    ) -> Result<JsValue, JsValue> {
        let call = NotesChange::Create {
            draft: draft(note)?,
        };
        typed(&run(&host, call).await?)
    }

    /// Edit one note at the version the page read.
    #[wasm_bindgen(js_name = notesUpdate, unchecked_return_type = "PersonalNote")]
    pub async fn notes_update(
        host: GatewayHost,
        id: String,
        expected_version: f64,
        #[wasm_bindgen(unchecked_param_type = "object")] note: JsValue,
    ) -> Result<JsValue, JsValue> {
        let call = NotesChange::Update {
            id,
            expected_version: expected_version as i64,
            draft: draft(note)?,
        };
        typed(&run(&host, call).await?)
    }

    /// Move one note to the trash, or out of it.
    #[wasm_bindgen(js_name = notesTrash, unchecked_return_type = "PersonalNote")]
    pub async fn notes_trash(
        host: GatewayHost,
        id: String,
        expected_version: f64,
        trashed: bool,
    ) -> Result<JsValue, JsValue> {
        let call = NotesChange::Trash {
            id,
            expected_version: expected_version as i64,
            trashed,
        };
        typed(&run(&host, call).await?)
    }

    /// Upload one attachment and answer it verified.
    #[wasm_bindgen(js_name = notesAttach, unchecked_return_type = "NoteAttachment")]
    pub async fn notes_attach(
        host: GatewayHost,
        note_id: String,
        source: UploadSource,
        file_name: String,
        content_type: String,
    ) -> Result<JsValue, JsValue> {
        let start = AttachmentStart {
            note_id: note_id.clone(),
            file_name,
            content_type,
            size_bytes: source.size().max(0.0) as u64,
        };
        let (session_uri, attachment) = run(&host, start).await?;
        send(&host, &session_uri, source, None, None, false, None, None)
            .await
            .finished()
            .map_err(|refusal| rejection(&refusal))?;
        let finalize = AttachmentFinalize {
            note_id,
            attachment_id: attachment.id,
        };
        typed(&run(&host, finalize).await?)
    }

    /// A short-lived read of one attachment.
    #[wasm_bindgen(js_name = notesAttachmentDownload, unchecked_return_type = "NoteAttachmentDownload")]
    pub async fn notes_attachment_download(
        host: GatewayHost,
        note_id: String,
        attachment_id: String,
    ) -> Result<JsValue, JsValue> {
        let call = AttachmentDownload {
            note_id,
            attachment_id,
        };
        typed(&run(&host, call).await?)
    }

    /// Remove one attachment.
    #[wasm_bindgen(js_name = notesAttachmentRemove)]
    pub async fn notes_attachment_remove(
        host: GatewayHost,
        note_id: String,
        attachment_id: String,
    ) -> Result<(), JsValue> {
        let call = AttachmentRemove {
            note_id,
            attachment_id,
        };
        run(&host, call).await
    }
}

#[cfg(test)]
#[path = "notes_tests.rs"]
mod tests;
