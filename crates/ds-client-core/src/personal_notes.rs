//! Account-private note controls through the fixed notes owner.
//! This boundary carries selected intent; it never lists projects or prefetches notes.
use crate::project_management::checklist::Edit;
use crate::{authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind};
use serde_json::{json, Value};

mod draft;
pub use draft::Draft;

pub const MAX_RESPONSE_BYTES: usize = 8 << 20;
pub const MAX_REQUEST_BYTES: usize = 512 << 10;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Create {
        draft: Draft,
    },
    Update {
        id: String,
        expected_version: i64,
        draft: Draft,
    },
    List {
        include_archived: bool,
        include_trashed: bool,
    },
    Read {
        id: String,
    },
    Retire {
        id: String,
        expected_version: i64,
    },
    Restore {
        id: String,
        expected_version: i64,
    },
    Versions {
        id: String,
    },
    ReadVersion {
        id: String,
        source_version: i64,
    },
    RestoreVersion {
        id: String,
        expected_version: i64,
        source_version: i64,
        source_sha256: String,
        command_id: String,
    },
    ChecklistRead {
        id: String,
    },
    ChecklistEdit {
        id: String,
        expected_version: i64,
        command_id: String,
        edit: Edit,
    },
}
pub(crate) fn note_id_valid(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty() && id.len() <= 256 && id != "." && id != ".." && !id.contains(['/', '\\'])
}
pub(crate) fn command_id_valid(id: &str) -> bool {
    (8..=128).contains(&id.len())
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
fn invalid() -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, "notes_request_invalid")
}
impl Action {
    pub fn validate(&self) -> Result<(), ClientError> {
        let id = match self {
            Self::Create { draft } => {
                draft.validate(true)?;
                None
            }
            Self::Update {
                id,
                expected_version,
                draft,
            } => {
                if !(1..i64::MAX).contains(expected_version) {
                    return Err(invalid());
                }
                draft.validate(false)?;
                Some(id)
            }
            Self::List { .. } => None,
            Self::Read { id } | Self::Versions { id } | Self::ChecklistRead { id } => Some(id),
            Self::ReadVersion { id, source_version } => {
                if *source_version < 1 {
                    return Err(invalid());
                }
                Some(id)
            }
            Self::Retire {
                id,
                expected_version,
            }
            | Self::Restore {
                id,
                expected_version,
            } => {
                if !(1..i64::MAX).contains(expected_version) {
                    return Err(invalid());
                }
                Some(id)
            }
            Self::RestoreVersion {
                id,
                expected_version,
                source_version,
                source_sha256,
                command_id,
            } => {
                if !(1..i64::MAX).contains(expected_version)
                    || *source_version < 1
                    || source_version > expected_version
                    || !sha_valid(source_sha256)
                    || !command_id_valid(command_id)
                {
                    return Err(invalid());
                }
                Some(id)
            }
            Self::ChecklistEdit {
                id,
                expected_version,
                command_id,
                edit,
            } => {
                if !(1..i64::MAX).contains(expected_version) || !command_id_valid(command_id) {
                    return Err(invalid());
                }
                edit.validate()?;
                Some(id)
            }
        };
        if id.is_some_and(|id| !note_id_valid(id) || id != id.trim())
            || serde_json::to_vec(&self.payload())
                .map_err(|_| invalid())?
                .len()
                > MAX_REQUEST_BYTES
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn payload(&self) -> Value {
        match self {
            Self::Create { draft } => json!({"action":"create","note":draft}),
            Self::Update {
                id,
                expected_version,
                draft,
            } => {
                json!({"action":"update","id":id,"expected_version":expected_version,"note":draft})
            }
            Self::List {
                include_archived,
                include_trashed,
            } => {
                json!({"action":"list","include_archived":include_archived,"include_trashed":include_trashed})
            }
            Self::Read { id } => json!({"action":"get","id":id}),
            Self::Retire {
                id,
                expected_version,
            } => json!({"action":"trash","id":id,"expected_version":expected_version}),
            Self::Restore {
                id,
                expected_version,
            } => json!({"action":"restore","id":id,"expected_version":expected_version}),
            Self::Versions { id } => json!({"action":"versions","id":id}),
            Self::ReadVersion { id, source_version } => {
                json!({"action":"read_version","id":id,"source_version":source_version})
            }
            Self::RestoreVersion {
                id,
                expected_version,
                source_version,
                source_sha256,
                command_id,
            } => {
                json!({"action":"restore_version","id":id,"expected_version":expected_version,"source_version":source_version,"source_sha256":source_sha256,"command_id":command_id})
            }
            Self::ChecklistRead { id } => json!({"action":"checklist_read","id":id}),
            Self::ChecklistEdit {
                id,
                expected_version,
                command_id,
                edit,
            } => {
                json!({"action":"checklist_edit","id":id,"expected_version":expected_version,"command_id":command_id,"checklist":edit})
            }
        }
    }
    fn id(&self) -> &str {
        match self {
            Self::List { .. } | Self::Create { .. } => "",
            Self::Update { id, .. } => id,
            Self::Read { id }
            | Self::Retire { id, .. }
            | Self::Restore { id, .. }
            | Self::Versions { id }
            | Self::ReadVersion { id, .. }
            | Self::RestoreVersion { id, .. }
            | Self::ChecklistRead { id }
            | Self::ChecklistEdit { id, .. } => id,
        }
    }
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    action: &Action,
) -> Result<Value, ClientError> {
    action.validate()?;
    let response = transport.personal_notes(PersonalNotesCall {
        profile,
        authority: authority.call(),
        action,
    })?;
    decode(action, response)
}
fn unreadable() -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, "notes_response_unreadable")
}
fn decode(action: &Action, response: crate::TransportResponse) -> Result<Value, ClientError> {
    if response.body.len() > MAX_RESPONSE_BYTES {
        return Err(unreadable());
    }
    if response.status != 200 {
        return Err(crate::project_correspondence::refusal(
            response.status,
            &response.body,
        ));
    }
    let envelope: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    let data = envelope
        .get("data")
        .filter(|v| v.is_object())
        .ok_or_else(unreadable)?;
    if envelope["success"] != true {
        return Err(unreadable());
    }
    match action {
        Action::Create { draft } => {
            validate_note(&data["note"], None)?;
            if data["note"]["version"] != 1 {
                return Err(unreadable());
            }
            draft.verify_authored_receipt(&data["note"])?;
        }
        Action::Update {
            expected_version,
            draft,
            ..
        } => {
            validate_note(&data["note"], Some(action.id()))?;
            if data["note"]["version"].as_i64() != Some(expected_version + 1) {
                return Err(unreadable());
            }
            draft.verify_authored_receipt(&data["note"])?;
        }
        Action::List {
            include_archived,
            include_trashed,
        } => {
            let notes = data["notes"].as_array().ok_or_else(unreadable)?;
            if notes.len() > 1000 || data["max_notes"] != 1000 {
                return Err(unreadable());
            }
            let mut ids = std::collections::BTreeSet::new();
            for note in notes {
                validate_note(note, None)?;
                if !ids.insert(note["id"].as_str().unwrap())
                    || (!include_archived && note["archived"] != false)
                    || (!include_trashed && note["trashed"] != false)
                {
                    return Err(unreadable());
                }
            }
            // The legacy owner bounded acquisition before filtering. Absence of
            // enumeration metadata must never become a completeness claim.
            if data.get("complete").is_some() || data.get("truncated").is_some() {
                let complete = data["complete"].as_bool().ok_or_else(unreadable)?;
                if data["truncated"].as_bool() != Some(!complete) {
                    return Err(unreadable());
                }
            }
        }
        Action::Read { .. } => {
            validate_note(&data["note"], Some(action.id()))?;
            let attachments = data["attachments"].as_array().ok_or_else(unreadable)?;
            if attachments.len() > 32 {
                return Err(unreadable());
            }
            let mut ids = std::collections::BTreeSet::new();
            for attachment in attachments {
                let id = attachment["id"]
                    .as_str()
                    .filter(|id| note_id_valid(id))
                    .ok_or_else(unreadable)?;
                if !ids.insert(id)
                    || attachment["note_id"].as_str() != Some(action.id())
                    || !matches!(
                        attachment["state"].as_str(),
                        Some("ready" | "uploading" | "failed")
                    )
                {
                    return Err(unreadable());
                }
            }
            crate::project_management::checklist::projection(data)?;
        }
        Action::Retire {
            expected_version, ..
        }
        | Action::Restore {
            expected_version, ..
        } => {
            validate_note(&data["note"], Some(action.id()))?;
            let version = data["note"]["version"].as_i64().unwrap();
            if (version != *expected_version && version != expected_version + 1)
                || data["note"]["trashed"].as_bool()
                    != Some(matches!(action, Action::Retire { .. }))
            {
                return Err(unreadable());
            }
        }
        Action::Versions { .. } => validate_versions(data, action.id())?,
        Action::ReadVersion { source_version, .. } => {
            validate_note(&data["note"], Some(action.id()))?;
            if data["note"]["version"].as_i64() != Some(*source_version)
                || !data["sha256"].as_str().is_some_and(sha_valid)
                || data["retained"].as_bool().is_none()
            {
                return Err(unreadable());
            }
        }
        Action::RestoreVersion {
            expected_version,
            source_version,
            source_sha256,
            ..
        } => {
            validate_note(&data["note"], Some(action.id()))?;
            if data["note"]["version"].as_i64() != Some(expected_version + 1)
                || data["note"]["trashed"] != false
                || data["source_version"].as_i64() != Some(*source_version)
                || data["source_sha256"].as_str() != Some(source_sha256)
                || data["preserved_fields"]
                    != json!(["identity", "creation_attribution", "archived", "reminder"])
            {
                return Err(unreadable());
            }
        }
        Action::ChecklistRead { .. } => {
            if data["id"].as_str() != Some(action.id())
                || data["version"].as_i64().is_none_or(|v| v < 1)
            {
                return Err(unreadable());
            }
            crate::project_management::checklist::projection(data)?;
        }
        Action::ChecklistEdit {
            expected_version, ..
        } => {
            if data["note"]["id"].as_str() != Some(action.id())
                || data["note"]["version"].as_i64() != Some(expected_version + 1)
            {
                return Err(unreadable());
            }
        }
    }
    Ok(data.clone())
}

fn sha_valid(sha: &str) -> bool {
    sha.len() == 64
        && sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn validate_note(note: &Value, id: Option<&str>) -> Result<(), ClientError> {
    let actual = note["id"]
        .as_str()
        .filter(|id| note_id_valid(id))
        .ok_or_else(unreadable)?;
    if id.is_some_and(|id| id != actual)
        || note["version"].as_i64().is_none_or(|v| v < 1)
        || note["archived"].as_bool().is_none()
        || note["trashed"].as_bool().is_none()
        || note.get("owner_uid").is_some()
        || note.get("OwnerUID").is_some()
    {
        return Err(unreadable());
    }
    Ok(())
}
fn validate_versions(data: &Value, id: &str) -> Result<(), ClientError> {
    let current = data["current_version"]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or_else(unreadable)?;
    let complete = data["complete"].as_bool().ok_or_else(unreadable)?;
    let chain = data["history_chain_complete"]
        .as_bool()
        .ok_or_else(unreadable)?;
    let versions = data["versions"].as_array().ok_or_else(unreadable)?;
    if data["id"].as_str() != Some(id)
        || data["truncated"].as_bool() != Some(!complete)
        || versions.len() > 100
    {
        return Err(unreadable());
    }
    let mut ids = std::collections::BTreeSet::new();
    for item in versions {
        let version = item["version"]
            .as_i64()
            .filter(|v| *v > 0 && *v <= current)
            .ok_or_else(unreadable)?;
        if !ids.insert(version)
            || !item["sha256"].as_str().is_some_and(sha_valid)
            || item["retained"].as_bool().is_none()
            || item["changed_by"].as_str().is_none()
            || item["changed_at"].as_str().is_none()
            || item["reason"].as_str().is_none()
        {
            return Err(unreadable());
        }
    }
    if chain
        && (!complete
            || versions.len() as i64 != current
            || versions.iter().any(|v| v["retained"] != true)
            || ids.first() != Some(&1)
            || ids.last() != Some(&current))
    {
        return Err(unreadable());
    }
    Ok(())
}

pub struct PersonalNotesCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) authority: crate::transport::CallAuthority<'a>,
    pub(crate) action: &'a Action,
}
impl PersonalNotesCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        "/api/v1/notes"
    }
    pub const fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        MAX_RESPONSE_BYTES
    }
    pub const fn timeout_seconds(&self) -> u64 {
        30
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub fn client_id(&self) -> &'static str {
        self.profile.client_id()
    }
    pub fn bearer_token(&self) -> &str {
        self.authority.bearer_token
    }
    pub fn canonical_email(&self) -> &str {
        self.authority.canonical_email
    }
    pub fn canonical_uid(&self) -> &str {
        self.authority.canonical_uid
    }
    pub fn device_id(&self) -> Option<&str> {
        self.authority.device_id
    }
    pub fn body(&self) -> crate::SecretRequestBody {
        crate::SecretRequestBody::new(
            serde_json::to_vec(&self.action.payload()).expect("closed note request"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(data: Value) -> crate::TransportResponse {
        crate::TransportResponse::new(
            200,
            serde_json::to_vec(&json!({"success":true,"data":data})).unwrap(),
        )
    }
    fn note(id: &str, version: i64, trashed: bool) -> Value {
        json!({"id":id,"version":version,"archived":false,"trashed":trashed})
    }
    #[test]
    fn authored_receipts_fence_draft_id_version_and_content() {
        let draft: Draft=serde_json::from_value(json!({"title":"  Private  ","content":{"markdown":"Reviewed body"},"tags":["Field","field"]})).unwrap();
        let mut authored = note("new-note", 1, false);
        authored["title"] = json!("Private");
        authored["content"] = json!({"format":"markdown","markdown":"Reviewed body"});
        authored["tags"] = json!(["field"]);
        authored["pinned"] = json!(false);
        authored["checklist"] = json!([]);
        authored["canvas"] = json!({"x":0,"y":0,"width":300,"height":240,"z":0,"color":"sand"});
        let create = Action::Create {
            draft: draft.clone(),
        };
        create.validate().unwrap();
        assert_eq!(create.payload()["action"], "create");
        assert!(create.payload().get("id").is_none());
        assert!(decode(&create, response(json!({"note":authored.clone()}))).is_ok());
        authored["content"]["markdown"] = json!("Another draft");
        assert!(decode(&create, response(json!({"note":authored.clone()}))).is_err());
        authored["content"]["markdown"] = json!("Reviewed body");
        authored["version"] = json!(2);
        assert!(decode(&create, response(json!({"note":authored.clone()}))).is_err());
        let update = Action::Update {
            id: "new-note".into(),
            expected_version: 1,
            draft,
        };
        update.validate().unwrap();
        assert!(decode(&update, response(json!({"note":authored.clone()}))).is_ok());
        authored["id"] = json!("another-note");
        assert!(decode(&update, response(json!({"note":authored}))).is_err());
    }
    #[test]
    fn private_controls_admit_only_closed_bounded_intent() {
        let selected = Action::ReadVersion {
            id: "private".into(),
            source_version: 2,
        };
        selected.validate().unwrap();
        assert_eq!(
            selected.payload(),
            json!({"action":"read_version","id":"private","source_version":2})
        );
        for action in [
            Action::Read {
                id: "../other".into(),
            },
            Action::Read {
                id: " private ".into(),
            },
            Action::Retire {
                id: "private".into(),
                expected_version: i64::MAX,
            },
            Action::ReadVersion {
                id: "private".into(),
                source_version: 0,
            },
            Action::RestoreVersion {
                id: "private".into(),
                expected_version: 3,
                source_version: 4,
                source_sha256: "a".repeat(64),
                command_id: "restore-001".into(),
            },
            Action::RestoreVersion {
                id: "private".into(),
                expected_version: 3,
                source_version: 1,
                source_sha256: "A".repeat(64),
                command_id: "restore-001".into(),
            },
        ] {
            assert!(action.validate().is_err());
        }
    }
    #[test]
    fn retirement_receipt_fences_identity_version_and_trash_without_archive_claim() {
        let action = Action::Retire {
            id: "private".into(),
            expected_version: 4,
        };
        assert!(decode(&action, response(json!({"note":note("private",5,true)}))).is_ok());
        assert!(decode(&action, response(json!({"note":note("private",4,true)}))).is_ok());
        for invalid in [
            note("other", 5, true),
            note("private", 5, false),
            note("private", 6, true),
        ] {
            assert!(decode(&action, response(json!({"note":invalid}))).is_err());
        }
        let restore = Action::Restore {
            id: "private".into(),
            expected_version: 5,
        };
        let mut archived = note("private", 6, false);
        archived["archived"] = json!(true);
        assert!(decode(&restore, response(json!({"note":archived}))).is_ok());
    }
    #[test]
    fn legacy_history_enumeration_is_not_a_full_recoverable_chain() {
        let action = Action::Versions {
            id: "private".into(),
        };
        let mut data = json!({"id":"private","current_version":7,"complete":true,"truncated":false,"history_chain_complete":false,
            "versions":[{"version":7,"sha256":"a".repeat(64),"changed_by":"owner","changed_at":"2026-10-13T00:00:00Z","reason":"observed_current","retained":false}]});
        assert!(decode(&action, response(data.clone())).is_ok());
        data["history_chain_complete"] = json!(true);
        assert!(decode(&action, response(data.clone())).is_err());
        data["history_chain_complete"] = json!(false);
        data["truncated"] = json!(true);
        assert!(decode(&action, response(data.clone())).is_err());
        data["complete"] = json!(false);
        assert!(decode(&action, response(data.clone())).is_ok());
        data["versions"][0]["version"] = json!(8);
        assert!(decode(&action, response(data)).is_err());
    }
    #[test]
    fn historic_restore_receipt_matches_selected_content_and_preserved_current_controls() {
        let sha = "a".repeat(64);
        let action = Action::RestoreVersion {
            id: "private".into(),
            expected_version: 8,
            source_version: 2,
            source_sha256: sha.clone(),
            command_id: "restore-001".into(),
        };
        let mut data = json!({"note":note("private",9,false),"source_version":2,"source_sha256":sha,"preserved_fields":["identity","creation_attribution","archived","reminder"]});
        assert!(decode(&action, response(data.clone())).is_ok());
        data["source_sha256"] = json!("b".repeat(64));
        assert!(decode(&action, response(data.clone())).is_err());
        data["source_sha256"] = json!(sha);
        data["note"]["version"] = json!(8);
        assert!(decode(&action, response(data.clone())).is_err());
        data["note"]["version"] = json!(9);
        data["preserved_fields"] = json!(["identity"]);
        assert!(decode(&action, response(data)).is_err());
        let read = Action::ReadVersion {
            id: "private".into(),
            source_version: 2,
        };
        assert!(decode(
            &read,
            response(json!({"note":note("private",2,false),"sha256":sha,"retained":false}))
        )
        .is_ok());
    }
    #[test]
    fn private_reads_refuse_hidden_uid_and_wrong_attachment_owner() {
        let list = Action::List {
            include_archived: false,
            include_trashed: false,
        };
        let legacy = json!({"notes":[note("private",1,false)],"max_notes":1000});
        assert_eq!(decode(&list, response(legacy.clone())).unwrap(), legacy);
        let mut leaked = note("private", 1, false);
        leaked["owner_uid"] = json!("hidden");
        assert!(decode(&list, response(json!({"notes":[leaked],"max_notes":1000}))).is_err());
        let read = Action::Read {
            id: "private".into(),
        };
        let mut data = json!({"note":note("private",1,false),"attachments":[{"id":"a","note_id":"private","state":"uploading"}],
            "checklist":{"kind":"checklist","items":[],"total":0,"completed":0}});
        assert!(decode(&read, response(data.clone())).is_ok());
        data["attachments"][0]["note_id"] = json!("other");
        assert!(decode(&read, response(data)).is_err());
        assert!(decode(&read, crate::TransportResponse::new(403, b"{}".to_vec())).is_err());
    }
    #[test]
    fn private_item_edit_has_no_project_identity_or_overwrite() {
        let edit: Edit =
            serde_json::from_value(json!({"action":"check","item_id":"i","answer":"verified"}))
                .unwrap();
        let action = Action::ChecklistEdit {
            id: "n".into(),
            expected_version: 7,
            command_id: "note-edit-001".into(),
            edit,
        };
        action.validate().unwrap();
        assert_eq!(
            action.payload(),
            json!({"action":"checklist_edit","id":"n","expected_version":7,"command_id":"note-edit-001","checklist":{"action":"check","item_id":"i","answer":"verified"}})
        );
        assert!(decode(&action, response(json!({"note":{"id":"n","version":8}}))).is_ok());
        assert!(decode(
            &action,
            response(json!({"note":{"id":"other","version":8}}))
        )
        .is_err());
        assert!(decode(&action, response(json!({"note":{"id":"n","version":9}}))).is_err());
    }
    #[test]
    fn private_read_preserves_owner_projection_and_refuses_wrong_route() {
        let action = Action::ChecklistRead { id: "n".into() };
        let data = json!({"id":"n","version":4,"checklist":{"kind":"issues","items":[{"id":"i","state":"answered"}],"total":1,"completed":1}});
        assert_eq!(decode(&action, response(data.clone())).unwrap(), data);
        assert!(decode(&action, response(json!({"id":"n","version":4}))).is_err());
        assert!(!note_id_valid("a/b"));
        assert!(!note_id_valid("a\\b"));
        assert!(note_id_valid("legacy note"));
        assert!(!command_id_valid("short"));
        assert!(!command_id_valid("bad.id-001"));
    }
}
