//! Immutable project-version intentions shared by CLI, Server and presentation hosts.
//! The gateway authorizes and commits; this owner pins the captured source and
//! expected head before any upload. No host may replace a stale expected head.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const MAX_MODEL_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub project: String,
    pub model_id: String,
    pub model_kind: String,
    pub expected_head_revision_id: String,
    pub source_revision_id: String,
    pub manifest_model_id: String,
    pub manifest_model_revision: u64,
    pub model_schema_version: String,
    pub engine_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub digest: String,
    pub object: String,
    pub byte_length: u64,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub intent: Intent,
    pub revision_id: String,
    pub artifact: Artifact,
}

pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        && value != "."
        && value != ".."
}

impl Intent {
    pub fn plan(&self, project: &str, bytes: &[u8]) -> Result<Plan, &'static str> {
        if self.project != project || !identifier(project) || !identifier(&self.model_id) {
            return Err("grid_publication_project_or_model_invalid");
        }
        if !matches!(
            self.model_kind.as_str(),
            "general" | "lv_network" | "mv_line"
        ) || (!self.expected_head_revision_id.is_empty()
            && !identifier(&self.expected_head_revision_id))
            || self.source_revision_id.is_empty()
            || self.source_revision_id.len() > 160
            || self.source_revision_id.chars().any(char::is_control)
            || self.manifest_model_id.is_empty()
            || self.manifest_model_id.len() > 256
            || self.manifest_model_id.trim() != self.manifest_model_id
            || self.manifest_model_id.chars().any(char::is_control)
            || self.manifest_model_revision > i64::MAX as u64
            || bytes.is_empty()
            || bytes.len() > MAX_MODEL_BYTES
        {
            return Err("grid_publication_source_invalid");
        }
        for (value, bound) in [
            (&self.model_schema_version, 160),
            (&self.engine_version, 160),
        ] {
            if value.is_empty()
                || value.len() > bound
                || value.trim() != value
                || value.chars().any(char::is_control)
            {
                return Err("grid_publication_metadata_invalid");
            }
        }
        for (value, bound) in [(&self.display_name, 240), (&self.reason, 2000)] {
            if value.as_ref().is_some_and(|s| {
                s.is_empty() || s.len() > bound || s.trim() != s || s.chars().any(char::is_control)
            }) {
                return Err("grid_publication_metadata_invalid");
            }
        }
        let revision_id: String = self
            .source_revision_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        if !identifier(&revision_id) {
            return Err("grid_publication_revision_invalid");
        }
        let digest = format!("{:x}", Sha256::digest(bytes));
        Ok(Plan {
            intent: self.clone(),
            revision_id,
            artifact: Artifact {
                object: format!("ds-grid/objects/sha256/{}/{}", &digest[..2], digest),
                digest,
                byte_length: bytes.len() as u64,
            },
        })
    }
}

impl Plan {
    pub fn upload_request(&self) -> Value {
        json!({"action":"start_upload", "project_id":self.intent.project,
            "upload":{"model_id":self.intent.model_id,"digest":self.artifact.digest,"byte_length":self.artifact.byte_length}})
    }
    pub fn version_request(&self) -> Value {
        let mut version = json!({
            "model_id":self.intent.model_id,"model_kind":self.intent.model_kind,
            "revision_id":self.revision_id,"expected_head_revision_id":self.intent.expected_head_revision_id,
            "manifest_model_id":self.intent.manifest_model_id,"manifest_model_revision":self.intent.manifest_model_revision,
            "model":self.artifact,"model_schema_version":self.intent.model_schema_version,
            "engine_version":self.intent.engine_version,"validation":{"state":"valid","issue_count":0}
        });
        for (key, value) in [
            ("display_name", &self.intent.display_name),
            ("reason", &self.intent.reason),
        ] {
            if let Some(value) = value {
                version[key] = json!(value);
            }
        }
        json!({"action":"create_version","project_id":self.intent.project,"version":version})
    }
    pub fn matches_version(&self, value: &Value) -> bool {
        value["project_id"] == self.intent.project
            && value["model_id"] == self.intent.model_id
            && value["revision_id"] == self.revision_id
            && value["manifest_model_id"] == self.intent.manifest_model_id
            && value["manifest_model_revision"] == self.intent.manifest_model_revision
            && value["model_kind"] == self.intent.model_kind
            && value["model_schema_version"] == self.intent.model_schema_version
            && value["engine_version"] == self.intent.engine_version
            && value["parent_revision_id"].as_str().unwrap_or("")
                == self.intent.expected_head_revision_id
            && serde_json::from_value::<Artifact>(value["model"].clone())
                .ok()
                .as_ref()
                == Some(&self.artifact)
            && value["version"].as_u64().is_some_and(|v| v > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn intent(project: &str) -> Intent {
        Intent {
            project: project.into(),
            model_id: "model_one".into(),
            model_kind: "mv_line".into(),
            expected_head_revision_id: "rev-old".into(),
            source_revision_id: "rev:new".into(),
            manifest_model_id: "native-model".into(),
            manifest_model_revision: 1,
            model_schema_version: "19".into(),
            engine_version: "ds-grid@1".into(),
            display_name: None,
            reason: Some("Reviewed route".into()),
        }
    }
    #[test]
    fn interleaved_projects_keep_their_bytes_and_expected_heads() {
        let a = intent("project_a").plan("project_a", b"first").unwrap();
        let mut second = intent("project_b");
        second.expected_head_revision_id = "rev-other".into();
        let b = second.plan("project_b", b"second").unwrap();
        assert_ne!(a.artifact.digest, b.artifact.digest);
        assert_eq!(
            a.version_request()["version"]["expected_head_revision_id"],
            "rev-old"
        );
        assert_eq!(b.version_request()["project_id"], "project_b");
        assert!(intent("project_a").plan("project_b", b"first").is_err());
        assert_eq!(a.revision_id, "rev-new");
    }
    #[test]
    fn readback_must_match_project_revision_kind_and_exact_bytes() {
        let plan = intent("p").plan("p", b"model").unwrap();
        let mut v = plan.version_request()["version"].clone();
        v["project_id"] = json!("p");
        v["version"] = json!(1);
        v["parent_revision_id"] = json!("rev-old");
        assert!(plan.matches_version(&v));
        for (key, wrong) in [
            ("project_id", json!("q")),
            ("revision_id", json!("rev-other")),
            ("model_kind", json!("general")),
            ("version", json!(0)),
            ("model", json!({})),
            ("parent_revision_id", json!("unreviewed")),
            ("engine_version", json!("unknown")),
        ] {
            let mut changed = v.clone();
            changed[key] = wrong;
            assert!(!plan.matches_version(&changed));
        }
    }
}
