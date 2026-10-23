//! Account-private note controls through the fixed notes owner.
//! This boundary carries selected intent; it never lists projects or prefetches notes.
use crate::project_management::checklist::Edit;
use crate::{authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind};
use serde_json::{json, Value};

mod backup;
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
        limit: usize,
        cursor: Option<String>,
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
        limit: usize,
        cursor: Option<String>,
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
    Backup {
        id: String,
        expected_version: i64,
        command_id: String,
    },
    ReadBackup {
        id: String,
        backup_id: String,
        backup_sha256: String,
    },
    RestoreBackup {
        id: String,
        expected_version: i64,
        backup_id: String,
        backup_sha256: String,
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
            Self::List { limit, cursor, .. } => {
                if !(1..=100).contains(limit)
                    || cursor
                        .as_ref()
                        .is_some_and(|c| c.is_empty() || c.len() > 2048 || c.trim() != c)
                {
                    return Err(invalid());
                }
                None
            }
            Self::Read { id } | Self::ChecklistRead { id } => Some(id),
            Self::Versions { id, limit, cursor } => {
                if !(1..=100).contains(limit)
                    || cursor
                        .as_ref()
                        .is_some_and(|c| c.is_empty() || c.len() > 2048 || c.trim() != c)
                {
                    return Err(invalid());
                }
                Some(id)
            }
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
            Self::Backup {
                id,
                expected_version,
                command_id,
            } => {
                if !(1..i64::MAX).contains(expected_version) || !command_id_valid(command_id) {
                    return Err(invalid());
                }
                Some(id)
            }
            Self::ReadBackup {
                id,
                backup_id,
                backup_sha256,
            } => {
                if !note_id_valid(backup_id)
                    || backup_id != backup_id.trim()
                    || !sha_valid(backup_sha256)
                {
                    return Err(invalid());
                }
                Some(id)
            }
            Self::RestoreBackup {
                id,
                expected_version,
                backup_id,
                backup_sha256,
                command_id,
            } => {
                if !(1..i64::MAX).contains(expected_version)
                    || !command_id_valid(command_id)
                    || !note_id_valid(backup_id)
                    || backup_id != backup_id.trim()
                    || !sha_valid(backup_sha256)
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
                limit,
                cursor,
                include_archived,
                include_trashed,
            } => {
                json!({"action":"list","limit":limit,"cursor":cursor,"include_archived":include_archived,"include_trashed":include_trashed})
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
            Self::Versions { id, limit, cursor } => {
                json!({"action":"versions","id":id,"limit":limit,"cursor":cursor})
            }
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
            Self::Backup {
                id,
                expected_version,
                command_id,
            } => {
                json!({"action":"backup","id":id,"expected_version":expected_version,"command_id":command_id})
            }
            Self::ReadBackup {
                id,
                backup_id,
                backup_sha256,
            } => {
                json!({"action":"read_backup","id":id,"backup_id":backup_id,"backup_sha256":backup_sha256})
            }
            Self::RestoreBackup {
                id,
                expected_version,
                backup_id,
                backup_sha256,
                command_id,
            } => {
                json!({"action":"restore_backup","id":id,"expected_version":expected_version,"backup_id":backup_id,"backup_sha256":backup_sha256,"command_id":command_id})
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
            | Self::Versions { id, .. }
            | Self::ReadVersion { id, .. }
            | Self::RestoreVersion { id, .. }
            | Self::Backup { id, .. }
            | Self::ReadBackup { id, .. }
            | Self::RestoreBackup { id, .. }
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
            limit,
            cursor: _,
            include_archived,
            include_trashed,
        } => {
            let list: MetadataList =
                serde_json::from_value(data.clone()).map_err(|_| unreadable())?;
            if list.notes.len() > list.scanned_on_page
                || list.scanned_on_page > *limit
                || list.limit != *limit
                || list.truncated != !list.complete
                || (list.complete != list.next_cursor.is_none())
                || list
                    .next_cursor
                    .as_ref()
                    .is_some_and(|c| c.is_empty() || c.len() > 2048)
                || (!list.complete && list.scanned_on_page != *limit)
                || (*include_archived
                    && *include_trashed
                    && list.notes.len() != list.scanned_on_page)
            {
                return Err(unreadable());
            }
            let mut ids = std::collections::BTreeSet::new();
            for note in &list.notes {
                if !note_id_valid(&note.id)
                    || note.version < 1
                    || note.title.chars().count() > 300
                    || note.folder.chars().count() > 200
                    || note.tags.len() > 24
                    || note.tags.iter().any(|tag| tag.chars().count() > 64)
                    || note.updated_at.is_empty()
                    || !ids.insert(&note.id)
                    || (!include_archived && note.archived)
                    || (!include_trashed && note.trashed)
                {
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
        Action::Versions { limit, cursor, .. } => {
            validate_versions(data, action.id(), *limit, cursor.is_some())?
        }
        Action::ReadVersion { source_version, .. } => {
            validate_note(&data["note"], Some(action.id()))?;
            if data["note"]["version"].as_i64() != Some(*source_version)
                || !data["sha256"].as_str().is_some_and(sha_valid)
                || data["retained"].as_bool() != Some(true)
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
        Action::Backup { .. } | Action::ReadBackup { .. } | Action::RestoreBackup { .. } => {
            backup::verify(action, data)?
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

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MetadataList {
    notes: Vec<Metadata>,
    limit: usize,
    scanned_on_page: usize,
    #[serde(default)]
    next_cursor: Option<String>,
    complete: bool,
    truncated: bool,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    #[serde(rename = "migration_required")]
    _migration_required: bool,
    id: String,
    version: i64,
    title: String,
    #[serde(default)]
    folder: String,
    tags: Vec<String>,
    #[serde(rename = "pinned")]
    _pinned: bool,
    archived: bool,
    trashed: bool,
    updated_at: String,
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
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionPage {
    baseline_version: i64,
    earlier_history_status: String,
    id: String,
    current_version: i64,
    source_head_version: i64,
    limit: usize,
    versions: Vec<VersionHeader>,
    complete: bool,
    truncated: bool,
    #[serde(default)]
    next_cursor: Option<String>,
    history_chain_complete: bool,
    history_chain_status: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionHeader {
    version: i64,
    sha256: String,
    #[serde(rename = "changed_by")]
    _changed_by: String,
    changed_at: String,
    #[serde(rename = "reason")]
    _reason: String,
    retained: bool,
}
fn validate_versions(
    data: &Value,
    id: &str,
    limit: usize,
    continuation: bool,
) -> Result<(), ClientError> {
    let page: VersionPage = serde_json::from_value(data.clone()).map_err(|_| unreadable())?;
    if page.id != id
        || page.baseline_version < 1
        || page.baseline_version > page.current_version
        || !matches!(
            page.earlier_history_status.as_str(),
            "none" | "not_assessed"
        )
        || (page.earlier_history_status == "none" && page.baseline_version != 1)
        || page.current_version < 1
        || page.source_head_version < 1
        || page.source_head_version > page.current_version
        || page.limit != limit
        || page.versions.len() > limit
        || page.truncated != !page.complete
        || page.complete != page.next_cursor.is_none()
        || page
            .next_cursor
            .as_ref()
            .is_some_and(|c| c.is_empty() || c.len() > 2048)
        || (!page.complete && page.versions.len() != limit)
        || !matches!(
            page.history_chain_status.as_str(),
            "complete" | "incomplete" | "unknown"
        )
        || page.history_chain_complete != (page.history_chain_status == "complete")
    {
        return Err(unreadable());
    }
    let mut previous = page.source_head_version;
    for (index, item) in page.versions.iter().enumerate() {
        if item.version < 1
            || item.version > page.source_head_version
            || (index > 0 && item.version >= previous)
            || !sha_valid(&item.sha256)
            || item.changed_at.is_empty()
            || !item.retained
        {
            return Err(unreadable());
        }
        previous = item.version;
    }
    let observed_incomplete = page.versions.iter().any(|v| !v.retained)
        || (!continuation
            && page
                .versions
                .first()
                .is_none_or(|v| v.version != page.source_head_version))
        || page
            .versions
            .windows(2)
            .any(|pair| pair[1].version != pair[0].version - 1)
        || (page.complete && page.versions.last().is_some_and(|v| v.version != 1));
    let fully_proven = !continuation
        && page.complete
        && page.versions.len() as i64 == page.source_head_version
        && page
            .versions
            .iter()
            .enumerate()
            .all(|(i, v)| v.retained && v.version == page.source_head_version - i as i64);
    if (observed_incomplete && page.history_chain_status != "incomplete")
        || (fully_proven && page.history_chain_status != "complete")
        || (page.history_chain_complete && !fully_proven)
        || (!continuation && !observed_incomplete && page.history_chain_status == "incomplete")
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
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::PersonalNotesAction;

    crate::gateway_compat::gateway_common_accessors! {
        gateway_origin: normal,
        content_type: const,
        gateway_api_key: normal,
        client_id: normal,
        bearer_token: normal,
        canonical_email: normal,
        canonical_uid: normal,
        device_id: normal,
    }
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        MAX_RESPONSE_BYTES
    }
    pub const fn timeout_seconds(&self) -> u64 {
        30
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
    fn adopted_history_enumeration_does_not_invent_missing_earlier_versions() {
        let action = Action::Versions {
            id: "private".into(),
            limit: 50,
            cursor: None,
        };
        let mut data = json!({"id":"private","current_version":7,"baseline_version":1,"earlier_history_status":"none","source_head_version":7,"limit":50,"complete":true,"truncated":false,"history_chain_complete":false,"history_chain_status":"incomplete",
            "versions":[{"version":7,"sha256":"a".repeat(64),"changed_by":"owner","changed_at":"2026-10-13T00:00:00Z","reason":"explicitly retained observed baseline","retained":true}]});
        assert!(decode(&action, response(data.clone())).is_ok());
        data["history_chain_complete"] = json!(true);
        assert!(decode(&action, response(data.clone())).is_err());
        data["history_chain_complete"] = json!(false);
        data["truncated"] = json!(true);
        assert!(decode(&action, response(data.clone())).is_err());
        data["complete"] = json!(false);
        assert!(decode(&action, response(data.clone())).is_err());
        data["complete"] = json!(true);
        data["truncated"] = json!(false);
        data["versions"][0]["version"] = json!(8);
        assert!(decode(&action, response(data)).is_err());
    }
    #[test]
    fn version_pages_do_not_claim_unseen_history_and_keep_head_cutoff() {
        let first = Action::Versions {
            id: "private".into(),
            limit: 1,
            cursor: None,
        };
        let header = json!({"version":7,"sha256":"a".repeat(64),"changed_by":"owner","changed_at":"2026-10-13T00:00:00Z","reason":"update","retained":true});
        let mut data = json!({"id":"private","current_version":8,"baseline_version":1,"earlier_history_status":"none","source_head_version":7,"limit":1,"complete":false,"truncated":true,"next_cursor":"opaque","history_chain_complete":false,"history_chain_status":"unknown","versions":[header]});
        assert!(decode(&first, response(data.clone())).is_ok());
        data["versions"][0]["document_json"] = json!("private body");
        assert!(decode(&first, response(data.clone())).is_err());
        data["versions"][0]
            .as_object_mut()
            .unwrap()
            .remove("document_json");
        data["history_chain_status"] = json!("complete");
        data["history_chain_complete"] = json!(true);
        assert!(decode(&first, response(data.clone())).is_err());
        data["history_chain_status"] = json!("unknown");
        data["history_chain_complete"] = json!(false);
        data["source_head_version"] = json!(9);
        assert!(decode(&first, response(data.clone())).is_err());
        let next = Action::Versions {
            id: "private".into(),
            limit: 1,
            cursor: Some("opaque".into()),
        };
        data["source_head_version"] = json!(7);
        data["versions"][0]["retained"] = json!(false);
        assert!(decode(&next, response(data)).is_err());
        assert!(Action::Versions {
            id: "private".into(),
            limit: 101,
            cursor: None
        }
        .validate()
        .is_err());
        assert!(Action::Versions {
            id: "private".into(),
            limit: 1,
            cursor: Some(" ".into())
        }
        .validate()
        .is_err());
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
            response(json!({"note":note("private",2,false),"sha256":sha,"retained":true}))
        )
        .is_ok());
    }
    #[test]
    fn private_reads_refuse_hidden_uid_and_wrong_attachment_owner() {
        let list = Action::List {
            limit: 50,
            cursor: None,
            include_archived: false,
            include_trashed: false,
        };
        let header = json!({"id":"private","version":1,"title":"Header","migration_required":false,"tags":[],"pinned":false,"archived":false,"trashed":false,"updated_at":"2026-10-13T00:00:00Z"});
        let inventory = json!({"notes":[header],"limit":50,"scanned_on_page":1,"complete":true,"truncated":false});
        assert_eq!(
            decode(&list, response(inventory.clone())).unwrap(),
            inventory
        );
        assert_eq!(list.payload()["action"], "list");
        let mut leaked = inventory.clone();
        leaked["notes"][0]["owner_uid"] = json!("hidden");
        assert!(decode(&list, response(leaked)).is_err());
        for key in [
            "content",
            "checklist",
            "geometry",
            "links",
            "reminder",
            "canvas",
            "attachments",
        ] {
            let mut body = inventory.clone();
            body["notes"][0][key] = json!({});
            assert!(decode(&list, response(body)).is_err());
        }
        let mut partial = inventory.clone();
        partial["complete"] = json!(false);
        partial["truncated"] = json!(true);
        assert!(decode(&list, response(partial)).is_err());
        let mut wrong_count = inventory.clone();
        wrong_count["scanned_on_page"] = json!(0);
        assert!(decode(&list, response(wrong_count)).is_err());
        let mut legacy = inventory.clone();
        legacy.as_object_mut().unwrap().remove("scanned_on_page");
        assert!(decode(&list, response(legacy)).is_err());
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
