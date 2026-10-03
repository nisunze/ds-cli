//! Bounded immutable print-document revision contract, shared by all hosts.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
pub const MAX_REVISIONS: usize = 200;
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    List {
        reference: String,
    },
    Read {
        reference: String,
        revision: String,
    },
    Compare {
        reference: String,
        revision: String,
    },
    Restore {
        reference: String,
        revision: String,
        expected_head: String,
    },
}
#[derive(Debug, Clone)]
pub struct Request {
    pub project: String,
    pub command: Command,
}
fn digest(raw: &str) -> bool {
    raw.len() == 64 && raw.bytes().all(|c| c.is_ascii_hexdigit())
}
impl Command {
    pub fn reference(&self) -> &str {
        match self {
            Self::List { reference }
            | Self::Read { reference, .. }
            | Self::Compare { reference, .. }
            | Self::Restore { reference, .. } => reference,
        }
    }
    pub fn wire(&self) -> &'static str {
        match self {
            Self::List { .. } => "list_versions",
            Self::Read { .. } => "read_version",
            Self::Compare { .. } => "compare_version",
            Self::Restore { .. } => "restore_version",
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let reference = self.reference();
        if !reference.ends_with("_print")
            || reference.len() > 500
            || reference.trim() != reference
            || reference.chars().any(char::is_control)
        {
            return Err("revision commands need one exact governed _print ref".into());
        }
        match self {
            Self::List { .. } => {}
            Self::Read { revision, .. } | Self::Compare { revision, .. } => {
                if !digest(revision) {
                    return Err("revision must be a 64-character hexadecimal id".into());
                }
            }
            Self::Restore {
                revision,
                expected_head,
                ..
            } => {
                if !digest(revision) || !digest(expected_head) {
                    return Err(
                        "restore needs exact revision and expected head ids from the version list"
                            .into(),
                    );
                }
            }
        }
        Ok(())
    }
}
pub fn request_body(request: &Request) -> Result<Value, String> {
    request.command.validate()?;
    if request.project.is_empty()
        || request.project.len() > 200
        || request.project.trim() != request.project
        || request.project.chars().any(char::is_control)
    {
        return Err("history needs one bounded explicit project".into());
    }
    let mut data = json!({"style_ref":request.command.reference(),"project_id":request.project});
    match &request.command {
        Command::List { .. } => {}
        Command::Read { revision, .. } | Command::Compare { revision, .. } => {
            data["revision_id"] = json!(revision);
        }
        Command::Restore {
            revision,
            expected_head,
            ..
        } => {
            data["revision_id"] = json!(revision);
            data["expected_head"] = json!(expected_head);
        }
    }
    Ok(json!({"action":request.command.wire(),"data":data}))
}
pub fn admit(command: &Command, response: &Value) -> Result<Value, String> {
    if response["success"] != true || response["data"]["style_ref"] != command.reference() {
        return Err("print revision receipt does not match the requested style".into());
    }
    let data = &response["data"];
    match command {
        Command::List { .. } => {
            let rows = data["revisions"]
                .as_array()
                .ok_or("revision list is missing")?;
            if rows.len() > MAX_REVISIONS {
                return Err("revision list exceeds its bound".into());
            }
            if !data["more"].is_null() && !data["more"].is_boolean() {
                return Err("revision list continuation flag is unreadable".into());
            }
            if !data["head_revision"].as_str().is_some_and(digest) {
                return Err("head revision id is missing".into());
            }
            for row in rows {
                if !row["revision_id"].as_str().is_some_and(digest)
                    || row["author"].as_str().is_none()
                    || row["created_at"].as_str().is_none()
                {
                    return Err("revision metadata is incomplete".into());
                }
            }
        }
        Command::Read { revision, .. } => {
            if data["revision"]["revision_id"] != revision.as_str() {
                return Err("exact revision receipt mismatches".into());
            }
            crate::style_validation::validate(&data["style_data"])?;
        }
        Command::Compare { revision, .. } => {
            if data["revision_id"] != revision.as_str()
                || data["equal"].as_bool().is_none()
                || data["changes"].as_array().is_none()
            {
                return Err("revision comparison is incomplete".into());
            }
        }
        Command::Restore { revision, .. } => {
            if data["restored_from"] != revision.as_str()
                || data["updated"] != true
                || !data["head_revision"].as_str().is_some_and(digest)
            {
                return Err("restore receipt mismatches".into());
            }
            crate::style_validation::validate(&data["style_data"])?;
        }
    }
    Ok(data.clone())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_bounded_history_keeps_exact_reads_available() {
        let command = Command::List {
            reference: "gt/rivers_print".into(),
        };
        let row = json!({"revision_id":"a".repeat(64),"author":"owner","created_at":"2026-10-02T00:00:00Z"});
        let mut receipt = json!({"success":true,"data":{"style_ref":"gt/rivers_print","head_revision":"b".repeat(64),"more":true,"revisions":vec![row.clone(); MAX_REVISIONS]}});
        assert_eq!(admit(&command, &receipt).unwrap()["more"], true);
        receipt["data"]["revisions"]
            .as_array_mut()
            .unwrap()
            .push(row);
        assert!(admit(&command, &receipt).is_err());
        receipt["data"]["revisions"].as_array_mut().unwrap().pop();
        receipt["data"]["more"] = json!("yes");
        assert!(admit(&command, &receipt).is_err());
    }
    #[test]
    fn restore_is_fenced_and_receipt_is_exact() {
        let command = Command::Restore {
            reference: "gt/rivers_print".into(),
            revision: "a".repeat(64),
            expected_head: "b".repeat(64),
        };
        let request = Request {
            project: "gisagara".into(),
            command: command.clone(),
        };
        let body = request_body(&request).unwrap();
        assert_eq!(body["action"], "restore_version");
        assert_eq!(body["data"]["expected_head"], "b".repeat(64));
        assert!(admit(
            &command,
            &json!({"success":true,"data":{"style_ref":"other_print"}})
        )
        .is_err());
        assert!(request_body(&Request {
            project: "p".into(),
            command: Command::Restore {
                reference: "gt/rivers_print".into(),
                revision: "a".repeat(64),
                expected_head: "".into()
            }
        })
        .is_err());
    }
}
