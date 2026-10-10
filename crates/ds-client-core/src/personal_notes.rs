//! Account-private note reads and item edits through the fixed notes owner.
//! This boundary carries selected intent; it never lists projects or prefetches notes.
use crate::project_management::checklist::Edit;
use crate::{authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind};
use serde_json::{json, Value};

pub const MAX_RESPONSE_BYTES: usize = 8 << 20;
pub const MAX_REQUEST_BYTES: usize = 512 << 10;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
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
            Self::ChecklistRead { id } => id,
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
                id
            }
        };
        if !note_id_valid(id)
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
            Self::ChecklistRead { id } | Self::ChecklistEdit { id, .. } => id.trim(),
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
    let (id, version) = match action {
        Action::ChecklistRead { .. } => (&data["id"], &data["version"]),
        Action::ChecklistEdit { .. } => (&data["note"]["id"], &data["note"]["version"]),
    };
    if envelope["success"] != true
        || id.as_str() != Some(action.id())
        || version.as_i64().is_none_or(|v| v < 1)
    {
        return Err(unreadable());
    }
    match action {
        Action::ChecklistRead { .. } => {
            crate::project_management::checklist::projection(data)?;
        }
        Action::ChecklistEdit {
            expected_version, ..
        } => {
            if version.as_i64() != Some(expected_version + 1) {
                return Err(unreadable());
            }
        }
    }
    Ok(data.clone())
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
