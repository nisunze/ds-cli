//! Exact, metadata-only task attachment detachment contract.
//! File retention, ownership and issued-evidence protection belong to ds-brain.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub task_id: String,
    pub attachment_id: String,
    pub expected_version: i64,
}

pub(crate) fn reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !matches!(value, "." | "..")
        && !value.contains('/')
        && !value.chars().any(|c| c.is_whitespace() || c.is_control())
}

impl Request {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !reference(&self.task_id)
            || !reference(&self.attachment_id)
            || self.expected_version < 1
            || self.expected_version == i64::MAX
        {
            return Err("detach requires exact project/task/attachment references and a positive incrementable version");
        }
        Ok(())
    }

    pub fn validate_for(&self, project: &str) -> Result<(), &'static str> {
        self.validate()?;
        if !reference(project) {
            return Err("detach requires an exact project reference");
        }
        Ok(())
    }
    pub fn payload(&self, project: &str) -> Value {
        json!({"action":"detach_task_attachment", "ds_project":project,
            "task_id":self.task_id, "id":self.attachment_id, "expected_version":self.expected_version})
    }

    pub fn verify(&self, project: &str, receipt: &Value) -> Result<(), &'static str> {
        self.validate_for(project)?;
        let version = receipt["version"]
            .as_i64()
            .ok_or("detach receipt lacks its version")?;
        let changed = receipt["changed"]
            .as_bool()
            .ok_or("detach receipt lacks its changed flag")?;
        let item = &receipt["item"];
        let prior = item["task_detached_prior_version"].as_i64();
        let next = self.expected_version + 1;
        let version_matches = if changed {
            version == next && prior == Some(self.expected_version)
        } else {
            version == self.expected_version
                || (version == next && prior == Some(self.expected_version))
        };
        if receipt["project_id"].as_str() != Some(project)
            || receipt["task_id"].as_str() != Some(self.task_id.as_str())
            || receipt["attachment_id"].as_str() != Some(self.attachment_id.as_str())
            || receipt["detached"].as_bool() != Some(true)
            || !version_matches
            || item["id"].as_str() != Some(self.attachment_id.as_str())
            || item["version"].as_i64() != Some(version)
            || item["task_detached"].as_bool() != Some(true)
            || prior.is_none_or(|v| v < 1 || v >= version)
            || item["data"]["linked_entity_type"] != "task"
            || item["data"]["linked_entity_id"].as_str() != Some(self.task_id.as_str())
            || item["task_detached_by"].as_str().is_none_or(str::is_empty)
            || item["task_detached_at"].as_str().is_none_or(str::is_empty)
        {
            return Err("detach receipt does not prove the exact project/task/attachment/version relationship");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request {
        Request {
            task_id: "task".into(),
            attachment_id: "file".into(),
            expected_version: 3,
        }
    }
    pub fn receipt() -> Value {
        json!({"project_id":"project","task_id":"task","attachment_id":"file","version":4,"changed":true,"detached":true,
        "item":{"id":"file","version":4,"task_detached":true,"task_detached_prior_version":3,"task_detached_by":"actor@example.com","task_detached_at":"2026-10-13T00:00:00Z","data":{"linked_entity_type":"task","linked_entity_id":"task","state":"ready"}}})
    }
    #[test]
    fn closed_request_and_receipt_match_changed_and_exact_replay() {
        let r = request();
        assert_eq!(
            r.payload("project"),
            json!({"action":"detach_task_attachment","ds_project":"project","task_id":"task","id":"file","expected_version":3})
        );
        r.verify("project", &receipt()).unwrap();
        let mut replay = receipt();
        replay["changed"] = json!(false);
        r.verify("project", &replay).unwrap();
        let current = Request {
            expected_version: 4,
            ..r.clone()
        };
        current.verify("project", &replay).unwrap();
        assert!(serde_json::from_value::<Request>(
            json!({"task_id":"task","attachment_id":"file","expected_version":3,"data":{}})
        )
        .is_err());
    }
    #[test]
    fn foreign_incomplete_or_stale_receipts_never_prove_detachment() {
        for (pointer, value) in [
            ("/project_id", json!("other")),
            ("/task_id", json!("other")),
            ("/attachment_id", json!("other")),
            ("/version", json!(5)),
            ("/detached", json!(false)),
            ("/changed", Value::Null),
            ("/item/version", json!(3)),
            ("/item/id", json!("other")),
            ("/item/task_detached", json!(false)),
            ("/item/data/linked_entity_id", json!("other")),
            ("/item/task_detached_prior_version", json!(2)),
        ] {
            let mut v = receipt();
            *v.pointer_mut(pointer).unwrap() = value;
            assert!(request().verify("project", &v).is_err(), "{pointer}");
        }
    }
    #[test]
    fn invalid_inputs_refuse_before_any_owner_call() {
        for id in ["", ".", "..", "foreign/file", "task space", "task\n"] {
            let mut r = request();
            r.task_id = id.into();
            assert!(r.validate_for("project").is_err());
        }
        for version in [0, -1, i64::MAX] {
            let mut r = request();
            r.expected_version = version;
            assert!(r.validate_for("project").is_err());
        }
        assert!(request().validate_for("foreign/project").is_err());
    }
}
