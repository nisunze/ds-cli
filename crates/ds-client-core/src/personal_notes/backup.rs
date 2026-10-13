//! Private backup receipts: Brain owns capture/restore; native checks selected
//! scope and exact portable bytes. Revision hashes remain server-only UID pins.
use super::{Action, note_id_valid, sha_valid, unreadable, validate_note};
use crate::ClientError;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const MAX_ARCHIVE_BYTES: usize = 512 << 10;
pub const MAX_VERSIONS: usize = 400;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Info {
    id: String,
    note_id: String,
    source_version: i64,
    sha256: String,
    verified: bool,
    json_bytes: usize,
    history_count: usize,
    history_chain_complete: bool,
    max_archive_bytes: usize,
    max_restore_versions: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    id: String,
    backup: Info,
    command_id: String,
    replayed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct View {
    id: String,
    backup: Info,
    archive_json: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Restored {
    note: Value,
    backup_id: String,
    backup_sha256: String,
    source_version: i64,
    preserved_fields: Vec<String>,
    command_id: String,
    replayed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    schema: String,
    id: String,
    source_version: i64,
    current: Value,
    history: Vec<Revision>,
    history_chain_complete: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    version: i64,
    sha256: String,
    #[serde(rename = "changed_by")]
    _changed_by: String,
    changed_at: String,
    #[serde(rename = "reason")]
    _reason: String,
    retained: bool,
    note: Value,
}
fn decoded<T: serde::de::DeserializeOwned>(data: &Value) -> Result<T, ClientError> {
    serde_json::from_value(data.clone()).map_err(|_| unreadable())
}
fn valid_info(info: &Info, id: &str) -> Result<(), ClientError> {
    if !note_id_valid(&info.id)
        || info.id != info.id.trim()
        || info.note_id != id
        || info.source_version < 1
        || !sha_valid(&info.sha256)
        || !info.verified
        || !(1..=MAX_ARCHIVE_BYTES).contains(&info.json_bytes)
        || !(1..=MAX_VERSIONS).contains(&info.history_count)
        || info.history_count as i64 > info.source_version
        || info.max_archive_bytes != MAX_ARCHIVE_BYTES
        || info.max_restore_versions != MAX_VERSIONS
        || (info.history_chain_complete && info.history_count as i64 != info.source_version)
    {
        return Err(unreadable());
    }
    Ok(())
}
fn public_note(note: &Value, id: &str, version: i64) -> Result<(), ClientError> {
    validate_note(note, Some(id))?;
    const PUBLIC_FIELDS: &[&str] = &[
        "id",
        "version",
        "title",
        "folder",
        "content",
        "checklist",
        "checklist_kind",
        "tags",
        "links",
        "geometry",
        "reminder",
        "canvas",
        "pinned",
        "archived",
        "trashed",
        "created_at",
        "created_by",
        "updated_at",
        "updated_by",
        "trashed_at",
    ];
    if note.as_object().is_none_or(|fields| {
        fields
            .keys()
            .any(|key| !PUBLIC_FIELDS.contains(&key.as_str()))
    }) {
        return Err(unreadable());
    }
    if note["version"].as_i64() != Some(version) || hidden_uid(note) {
        return Err(unreadable());
    }
    Ok(())
}
fn hidden_uid(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields
            .iter()
            .any(|(k, v)| matches!(k.as_str(), "owner_uid" | "OwnerUID") || hidden_uid(v)),
        Value::Array(values) => values.iter().any(hidden_uid),
        _ => false,
    }
}
fn authored(mut note: Value) -> Value {
    if let Some(reminder) = note.get_mut("reminder").and_then(Value::as_object_mut) {
        reminder.remove("status");
        reminder.remove("fired_at");
    }
    note
}
fn verify_archive(view: &View) -> Result<(), ClientError> {
    let raw = view.archive_json.as_bytes();
    if raw.len() != view.backup.json_bytes
        || raw.len() > MAX_ARCHIVE_BYTES
        || format!("{:x}", Sha256::digest(raw)) != view.backup.sha256
    {
        return Err(unreadable());
    }
    let a: Archive = serde_json::from_str(&view.archive_json).map_err(|_| unreadable())?;
    if a.schema != "ds.personal-note-backup/v1"
        || a.id != view.id
        || a.source_version != view.backup.source_version
        || a.history.len() != view.backup.history_count
        || a.history_chain_complete != view.backup.history_chain_complete
    {
        return Err(unreadable());
    }
    public_note(&a.current, &a.id, a.source_version)?;
    let mut previous = 0;
    let mut found = false;
    for v in &a.history {
        if v.version <= previous
            || v.version > a.source_version
            || !sha_valid(&v.sha256)
            || !v.retained
            || v.changed_at.is_empty()
        {
            return Err(unreadable());
        }
        public_note(&v.note, &a.id, v.version)?;
        if v.version == a.source_version {
            found = authored(v.note.clone()) == authored(a.current.clone());
        }
        previous = v.version;
    }
    let chain = a.history.len() as i64 == a.source_version
        && a.history
            .iter()
            .enumerate()
            .all(|(i, v)| v.retained && v.version == i as i64 + 1);
    if !found || chain != a.history_chain_complete {
        return Err(unreadable());
    }
    Ok(())
}
pub(super) fn verify(action: &Action, data: &Value) -> Result<(), ClientError> {
    match action {
        Action::Backup {
            id,
            expected_version,
            command_id,
        } => {
            let receipt: Capture = decoded(data)?;
            valid_info(&receipt.backup, id)?;
            let expected_id = format!(
                "pb_{}",
                &format!("{:x}", Sha256::digest(command_id.as_bytes()))[..32]
            );
            if receipt.id != *id
                || receipt.backup.id != expected_id
                || receipt.backup.source_version != *expected_version
                || receipt.command_id != *command_id
            {
                return Err(unreadable());
            }
            let _ = receipt.replayed;
        }
        Action::ReadBackup {
            id,
            backup_id,
            backup_sha256,
        } => {
            let view: View = decoded(data)?;
            valid_info(&view.backup, id)?;
            if view.id != *id
                || view.backup.id != *backup_id
                || view.backup.sha256 != *backup_sha256
            {
                return Err(unreadable());
            }
            verify_archive(&view)?;
        }
        Action::RestoreBackup {
            id,
            expected_version,
            backup_id,
            backup_sha256,
            command_id,
        } => {
            let receipt: Restored = decoded(data)?;
            public_note(&receipt.note, id, expected_version + 1)?;
            if receipt.note["trashed"] != false
                || receipt.backup_id != *backup_id
                || receipt.backup_sha256 != *backup_sha256
                || receipt.command_id != *command_id
                || !(1..=*expected_version).contains(&receipt.source_version)
                || serde_json::to_value(receipt.preserved_fields).unwrap_or(Value::Null)
                    != json!(["identity", "creation_attribution", "archived", "reminder"])
            {
                return Err(unreadable());
            }
            let _ = receipt.replayed;
        }
        _ => return Err(unreadable()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn note(version: i64) -> Value {
        json!({"id":"private","version":version,"archived":false,"trashed":false,"content":{"format":"markdown","markdown":"Exact body"}})
    }
    fn view() -> (Action, Value) {
        let archive = json!({"schema":"ds.personal-note-backup/v1","id":"private","source_version":2,
            "current":note(2),"history":[{"version":2,"sha256":"a".repeat(64),"changed_by":"owner","changed_at":"2026-10-13T00:00:00Z","reason":"explicitly retained observed baseline","retained":true,"note":note(2)}],"history_chain_complete":false});
        let raw = serde_json::to_string(&archive).unwrap();
        let sha = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let data = json!({"id":"private","backup":{"id":"pb_test","note_id":"private","source_version":2,"sha256":sha,"verified":true,"json_bytes":raw.len(),"history_count":1,"history_chain_complete":false,"max_archive_bytes":MAX_ARCHIVE_BYTES,"max_restore_versions":MAX_VERSIONS},"archive_json":raw});
        (
            Action::ReadBackup {
                id: "private".into(),
                backup_id: "pb_test".into(),
                backup_sha256: sha,
            },
            data,
        )
    }
    fn repin(data: &mut Value) {
        let raw = data["archive_json"].as_str().unwrap();
        let sha = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let bytes = raw.len();
        data["backup"]["sha256"] = json!(sha);
        data["backup"]["json_bytes"] = json!(bytes);
    }
    #[test]
    fn exact_archive_hash_scope_and_descriptor_are_required() {
        let (action, data) = view();
        assert!(verify(&action, &data).is_ok());
        for mutate in [0, 1, 2, 3, 4, 5] {
            let mut wrong = data.clone();
            match mutate {
                0 => {
                    wrong["archive_json"] =
                        json!(wrong["archive_json"].as_str().unwrap().to_owned() + " ")
                }
                1 => wrong["backup"]["verified"] = json!(false),
                2 => wrong["backup"]["note_id"] = json!("other"),
                3 => wrong["backup"]["history_count"] = json!(2),
                4 => wrong["backup"]["max_archive_bytes"] = json!(MAX_ARCHIVE_BYTES + 1),
                _ => wrong["extra"] = json!("foreign"),
            }
            assert!(verify(&action, &wrong).is_err());
        }
    }
    #[test]
    fn measured_archive_still_refuses_hidden_uid_order_and_false_retention() {
        let (_, data) = view();
        for mutate in [0, 1, 2, 3] {
            let mut wrong = data.clone();
            let mut archive: Value =
                serde_json::from_str(wrong["archive_json"].as_str().unwrap()).unwrap();
            match mutate {
                0 => archive["current"]["nested"] = json!({"owner_uid":"secret"}),
                1 => archive["history"][0]["note"]["id"] = json!("other"),
                2 => archive["history_chain_complete"] = json!(true),
                _ => archive["history"][0]["note"]["content"]["markdown"] = json!("Different body"),
            }
            wrong["archive_json"] = json!(serde_json::to_string(&archive).unwrap());
            repin(&mut wrong);
            let action = Action::ReadBackup {
                id: "private".into(),
                backup_id: "pb_test".into(),
                backup_sha256: wrong["backup"]["sha256"].as_str().unwrap().into(),
            };
            assert!(verify(&action, &wrong).is_err());
        }
    }
    #[test]
    fn capture_checks_command_derived_identity_and_closed_replay() {
        let (_, mut data) = view();
        data.as_object_mut().unwrap().remove("archive_json");
        let command = "backup-command-1";
        data["backup"]["id"] = json!(format!(
            "pb_{}",
            &format!("{:x}", Sha256::digest(command.as_bytes()))[..32]
        ));
        data["command_id"] = json!(command);
        data["replayed"] = json!(true);
        let action = Action::Backup {
            id: "private".into(),
            expected_version: 2,
            command_id: command.into(),
        };
        assert!(verify(&action, &data).is_ok());
        data["replayed"] = json!("true");
        assert!(verify(&action, &data).is_err());
        data["replayed"] = json!(false);
        data["command_id"] = json!("different");
        assert!(verify(&action, &data).is_err());
    }
    #[test]
    fn restore_checks_cas_source_state_and_exact_replay_reference() {
        let sha = "a".repeat(64);
        let action = Action::RestoreBackup {
            id: "private".into(),
            expected_version: 8,
            backup_id: "pb_test".into(),
            backup_sha256: sha.clone(),
            command_id: "restore-command-1".into(),
        };
        let mut data = json!({"note":note(9),"backup_id":"pb_test","backup_sha256":sha,"source_version":2,"preserved_fields":["identity","creation_attribution","archived","reminder"],"command_id":"restore-command-1","replayed":false});
        assert!(verify(&action, &data).is_ok());
        data["note"]["trashed"] = json!(true);
        assert!(verify(&action, &data).is_err());
        data["note"]["trashed"] = json!(false);
        data["source_version"] = json!(9);
        assert!(verify(&action, &data).is_err());
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    #[test]
    fn malformed_backup_intent_refuses_before_transport() {
        for action in [
            Action::Backup {
                id: "private".into(),
                expected_version: 0,
                command_id: "backup-command".into(),
            },
            Action::Backup {
                id: "../private".into(),
                expected_version: 1,
                command_id: "backup-command".into(),
            },
            Action::ReadBackup {
                id: "private".into(),
                backup_id: "../backup".into(),
                backup_sha256: "a".repeat(64),
            },
            Action::ReadBackup {
                id: "private".into(),
                backup_id: "pb_test".into(),
                backup_sha256: "A".repeat(64),
            },
            Action::RestoreBackup {
                id: "private".into(),
                expected_version: i64::MAX,
                backup_id: "pb_test".into(),
                backup_sha256: "a".repeat(64),
                command_id: "restore-command".into(),
            },
        ] {
            assert!(action.validate().is_err());
        }
    }
}
