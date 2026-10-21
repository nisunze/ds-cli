//! Closed scoped copy and selected-mail inputs. Brain owns copying, privacy,
//! source authority, MIME lineage, attachment adoption and exact replay.
use crate::{ClientError, ErrorKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const MAX_INPUT_BYTES: usize = 64 << 10;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Task,
    ProjectNote,
    PersonalNote,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CopyRequest {
    pub source_kind: DocumentKind,
    pub source_id: String,
    pub source_version: i64,
    pub target_kind: DocumentKind,
    pub target_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclude_attachments: bool,
}
fn invalid() -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, "document_request_invalid")
}
impl CopyRequest {
    pub fn validate(&self) -> Result<(), ClientError> {
        if !crate::personal_notes::note_id_valid(&self.source_id)
            || !crate::personal_notes::note_id_valid(&self.target_id)
            || self.source_version < 0
            || (self.source_kind == self.target_kind && self.source_id == self.target_id)
            || self.attachment_ids.len() > 32
            || (self.exclude_attachments && !self.attachment_ids.is_empty())
            || self
                .attachment_ids
                .iter()
                .any(|id| !crate::personal_notes::note_id_valid(id))
            || self.attachment_ids.iter().collect::<BTreeSet<_>>().len()
                != self.attachment_ids.len()
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn payload(&self, dry_run: bool) -> Value {
        let mut value = serde_json::to_value(self).expect("closed copy request");
        value["dry_run"] = json!(dry_run);
        value
    }
    pub fn check_response(
        &self,
        value: &Value,
        command_id: &str,
        base_revision: i64,
        dry_run: bool,
    ) -> Result<(), ClientError> {
        let unreadable = || {
            ClientError::new(
                ErrorKind::UnreadableResponse,
                "document_response_unreadable",
            )
        };
        let source_kind = serde_json::to_value(&self.source_kind).expect("kind");
        let target_kind = serde_json::to_value(&self.target_kind).expect("kind");
        if value["source_kind"] != source_kind
            || value["target_kind"] != target_kind
            || value["source_id"] != self.source_id
            || value["target_id"] != self.target_id
            || value["source_version"].as_i64() != Some(self.source_version)
            || value["source_retained"] != true
            || !value["applied"].is_boolean()
            || (!dry_run && value["applied"] != true)
        {
            return Err(unreadable());
        }
        let mut seen = BTreeSet::new();
        for field in ["copied_attachment_ids", "excluded_attachment_ids"] {
            let ids = value[field]
                .as_array()
                .filter(|ids| ids.len() <= 32)
                .ok_or_else(unreadable)?;
            for id in ids {
                let id = id
                    .as_str()
                    .filter(|id| crate::personal_notes::note_id_valid(id))
                    .ok_or_else(unreadable)?;
                if !seen.insert(id) {
                    return Err(unreadable());
                }
                if field == "copied_attachment_ids"
                    && !self.attachment_ids.iter().any(|selected| selected == id)
                {
                    return Err(unreadable());
                }
            }
        }
        if seen.len() > 32 {
            return Err(unreadable());
        }
        let copied = value["copied_attachment_ids"].as_array().expect("checked");
        if copied.len() != self.attachment_ids.len() {
            return Err(unreadable());
        }
        if value["applied"] == true {
            let target = match self.target_kind {
                DocumentKind::Task => {
                    if value["task"]["command_id"].as_str() != Some(command_id)
                        || value["task"]["committed_revision"].as_i64()
                            != base_revision.checked_add(1)
                    {
                        return Err(unreadable());
                    }
                    return Ok(());
                }
                DocumentKind::ProjectNote => &value["record"],
                DocumentKind::PersonalNote => &value["note"],
            };
            if target["id"] != self.target_id || target["version"].as_i64() != Some(1) {
                return Err(unreadable());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MailSourcePin {
    pub asset_id: String,
    pub version: i64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MailThreadRequest {
    pub record_id: String,
    pub message_count: usize,
    pub sources: Vec<MailSourcePin>,
}
fn sha256_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl MailThreadRequest {
    pub fn validate(&self) -> Result<(), ClientError> {
        if !crate::personal_notes::note_id_valid(&self.record_id)
            || !(1..=32).contains(&self.sources.len())
            || self.message_count != self.sources.len()
            || self.sources.iter().any(|source| {
                !crate::personal_notes::note_id_valid(&source.asset_id)
                    || source.version < 1
                    || source.generation.is_some_and(|g| g < 1)
                    || !sha256_valid(&source.sha256)
            })
            || self
                .sources
                .iter()
                .map(|s| &s.asset_id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.sources.len()
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn payload(&self, dry_run: bool) -> Value {
        let mut value = serde_json::to_value(self).expect("closed mail source pins");
        value["dry_run"] = json!(dry_run);
        value
    }
    pub fn check_response(&self, value: &Value, dry_run: bool) -> Result<(), ClientError> {
        let unreadable = || {
            ClientError::new(
                ErrorKind::UnreadableResponse,
                "document_response_unreadable",
            )
        };
        let manifest = &value["manifest"];
        let messages = manifest["messages"]
            .as_array()
            .filter(|m| m.len() == self.message_count)
            .ok_or_else(unreadable)?;
        if serde_json::to_vec(manifest)
            .map_err(|_| unreadable())?
            .len()
            > 256 << 10
            || !value["applied"].is_boolean()
            || (!dry_run && value["applied"] != true)
            || manifest["schema"] != 1
            || manifest["message_count"].as_u64() != Some(self.message_count as u64)
            || !manifest["sha256"].as_str().is_some_and(sha256_valid)
        {
            return Err(unreadable());
        }
        let mut seen = BTreeSet::new();
        let mut parts = 0u64;
        for message in messages {
            let source: MailSourcePin =
                serde_json::from_value(message["source"].clone()).map_err(|_| unreadable())?;
            let selected = self
                .sources
                .iter()
                .find(|s| s.asset_id == source.asset_id)
                .ok_or_else(unreadable)?;
            if !seen.insert(source.asset_id.clone())
                || source.version != selected.version
                || source.sha256 != selected.sha256
                || source.generation.is_none_or(|g| g < 1)
                || selected
                    .generation
                    .is_some_and(|g| Some(g) != source.generation)
            {
                return Err(unreadable());
            }
            let members = message["mail"]["parts"].as_array().ok_or_else(unreadable)?;
            if message["mail"]["truncated"].as_bool() != Some(false)
                || message["mail"]["parts_total"].as_u64() != Some(members.len() as u64)
                || members
                    .iter()
                    .any(|p| !p["sha256"].as_str().is_some_and(sha256_valid))
            {
                return Err(unreadable());
            }
            parts = parts
                .checked_add(members.len() as u64)
                .ok_or_else(unreadable)?;
        }
        if manifest["part_count"].as_u64() != Some(parts)
            || (value["applied"] == true && value["record"]["id"] != self.record_id)
        {
            return Err(unreadable());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copy_is_closed_explicit_and_never_changes_source_visibility() {
        let input = json!({"source_kind":"personal_note","source_id":"private","source_version":4,"target_kind":"project_note","target_id":"published","exclude_attachments":true});
        let request: CopyRequest = serde_json::from_value(input.clone()).unwrap();
        request.validate().unwrap();
        assert_eq!(request.payload(true)["dry_run"], true);
        let mut injected = input;
        injected["dry_run"] = json!(false);
        assert!(serde_json::from_value::<CopyRequest>(injected).is_err());
        let result = json!({"source_kind":"personal_note","source_id":"private","source_version":4,"target_kind":"project_note","target_id":"published","source_retained":true,"copied_attachment_ids":[],"excluded_attachment_ids":["a"],"applied":true,"record":{"id":"published","version":1}});
        request
            .check_response(&result, "copy-0001", 0, false)
            .unwrap();
        // Exact applied receipt may also answer a later preview; it performs no new write.
        request
            .check_response(&result, "copy-0001", 0, true)
            .unwrap();
        let mut moved = result;
        moved["source_retained"] = json!(false);
        assert!(request
            .check_response(&moved, "copy-0001", 0, false)
            .is_err());
    }
    #[test]
    fn selected_mail_requires_exact_count_and_no_arbitrary_source_paths() {
        let request = MailThreadRequest {
            record_id: "r".into(),
            message_count: 1,
            sources: vec![MailSourcePin {
                asset_id: "a".into(),
                version: 2,
                sha256: "a".repeat(64),
                generation: Some(3),
            }],
        };
        request.validate().unwrap();
        let mut invalid = request.clone();
        invalid.message_count = 2;
        assert!(invalid.validate().is_err());
        let mut injected = serde_json::to_value(&request).unwrap();
        injected["sources"][0]["url"] = json!("https://invalid.example/mail.eml");
        assert!(serde_json::from_value::<MailThreadRequest>(injected).is_err());
        let result = json!({"applied":true,"record":{"id":"r"},"manifest":{"schema":1,"sha256":"b".repeat(64),"message_count":1,"part_count":1,"messages":[{"source":request.sources[0],"mail":{"truncated":false,"parts_total":1,"parts":[{"sha256":"c".repeat(64)}]}}]}});
        request.check_response(&result, false).unwrap();
        let mut truncated = result;
        truncated["manifest"]["messages"][0]["mail"]["truncated"] = json!(true);
        assert!(request.check_response(&truncated, false).is_err());
    }
}
