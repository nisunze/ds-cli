//! Closed safe promotion wire; source backup and identity policy stay server-owned.
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Promotion {
    pub version_id: String,
    pub revision_id: String,
    pub expected_catalog_revision: String,
    pub expected_source_revision: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_record_ids: Vec<String>,
    pub reason: String,
    pub idempotency_key: String,
}
fn identity(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.trim() == s
        && !s
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/' | '\\'))
}
fn sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn instant(s: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
}
impl Promotion {
    pub fn validate(&self) -> Result<(), String> {
        if !super::design_version_catalog::version_id(&self.version_id)
            || !super::design_version_catalog::revision_id(&self.revision_id)
            || !sha(&self.expected_catalog_revision)
            || !instant(&self.expected_source_revision).is_some_and(|t| t.unix_timestamp() > 0)
            || self.reason.trim().is_empty()
            || self.reason.trim() != self.reason
            || self.reason.chars().count() > 280
            || self.reason.chars().any(char::is_control)
            || self.idempotency_key.trim().is_empty()
            || self.idempotency_key.trim() != self.idempotency_key
            || self.idempotency_key.chars().count() > 128
            || self.idempotency_key.chars().any(char::is_control)
        {
            return Err(
                "promotion needs exact immutable IDs, catalog/source fences and bounded reason/key"
                    .into(),
            );
        }
        for ids in [&self.task_ids, &self.source_record_ids] {
            let unique: std::collections::BTreeSet<_> = ids.iter().collect();
            if ids.len() > 32 || unique.len() != ids.len() || ids.iter().any(|s| !identity(s)) {
                return Err(
                    "promotion PM references must be unique exact IDs, at most32 per type".into(),
                );
            }
        }
        Ok(())
    }
    pub fn verify_response(&self, value: &Value) -> Result<(), String> {
        let backup = &value["backup"];
        if value["version_id"] != self.version_id
            || value["revision_id"] != self.revision_id
            || !value["change_id"].as_str().is_some_and(identity)
            || !value["working_content_digest"].as_str().is_some_and(sha)
            || !value["working_source_version"]
                .as_u64()
                .is_some_and(|n| n > 0)
            || !backup["backup_id"].as_str().is_some_and(identity)
            || !backup["snapshot_digest"].as_str().is_some_and(sha)
            || !backup["source_version"].as_u64().is_some_and(|n| {
                n < i64::MAX as u64 && value["working_source_version"].as_u64() == Some(n + 1)
            })
            || !backup["created_by"].as_str().is_some_and(identity)
            || !backup["created_at"]
                .as_str()
                .and_then(instant)
                .is_some_and(|t| t.unix_timestamp() > 0)
            || backup["source_revision"].as_str().and_then(instant)
                != instant(&self.expected_source_revision)
        {
            return Err("promotion receipt lacks the exact target, fenced original backup or tracked change".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    pub fn request() -> Promotion {
        Promotion {
            version_id: "v0".into(),
            revision_id: "v0".into(),
            expected_catalog_revision: "a".repeat(64),
            expected_source_revision: "2026-10-13T12:00:00Z".into(),
            task_ids: vec!["task1".into()],
            source_record_ids: vec!["record1".into()],
            reason: "Restore reviewed immutable design".into(),
            idempotency_key: "promote1".into(),
        }
    }
    #[test]
    fn exact_fences_and_bounded_pm_refs_are_required() {
        let mut p = request();
        p.validate().unwrap();
        p.task_ids.push("task1".into());
        assert!(p.validate().is_err());
        p = request();
        p.revision_id = "0".into();
        assert!(p.validate().is_err());
        p = request();
        p.expected_source_revision = "0".into();
        assert!(p.validate().is_err());
        assert!(serde_json::from_value::<Promotion>(json!({"gdfs":{}})).is_err());
    }
    #[test]
    fn target_and_actual_original_backup_must_be_acknowledged() {
        let p = request();
        let good = json!({"version_id":"v0","revision_id":"v0","change_id":"change1","working_source_version":6,"working_content_digest":"b".repeat(64),"backup":{"backup_id":"backup1","snapshot_digest":"c".repeat(64),"source_revision":"2026-10-13T12:00:00+00:00","source_version":5,"created_at":"2026-10-13T12:01:00Z","created_by":"actor1"}});
        p.verify_response(&good).unwrap();
        let mut initial = good.clone();
        initial["backup"]["source_version"] = json!(0);
        initial["working_source_version"] = json!(1);
        p.verify_response(&initial).unwrap();
        initial["working_source_version"] = json!(2);
        assert!(p.verify_response(&initial).is_err());
        for (pointer, replacement) in [
            ("/backup/source_revision", json!("2026-10-13T11:00:00Z")),
            ("/revision_id", json!("v1")),
            ("/backup/snapshot_digest", json!("bad")),
            ("/change_id", json!("")),
        ] {
            let mut bad = good.clone();
            *bad.pointer_mut(pointer).unwrap() = replacement;
            assert!(p.verify_response(&bad).is_err(), "{pointer}");
        }
    }
}

/// Captured consuming read fences for one explicit UI promotion. Not authority
/// to write: the server still checks runtime identity, backup and PM trace.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub project: String,
    pub transformer: String,
    pub version_id: String,
    pub revision_id: String,
    pub expected_catalog_revision: String,
    pub expected_source_revision: String,
}
pub fn eligible(project: &str, transformer: &str, catalog: &Value) -> Result<Value, String> {
    let command = super::design_version_catalog::Command::Catalog {
        object_id: transformer.into(),
    };
    command.validate(project)?;
    command.verify_response(project, &serde_json::json!({"catalog":catalog}))?;
    let ids = catalog["versions"]
        .as_array()
        .ok_or("catalog versions absent")?
        .iter()
        .filter(|entry| {
            catalog["baseline_ready"] == true
                && entry["snapshot_scope"] == "runtime_design"
                && entry["playback_available"] == true
        })
        .map(|entry| entry["version_id"].clone())
        .collect::<Vec<_>>();
    Ok(serde_json::json!({"eligible_versions":ids}))
}
pub fn context_read_plan(
    project: &str,
    transformer: &str,
    catalog: &Value,
    version_id: &str,
    revision_id: &str,
) -> Result<Value, String> {
    let allowed = eligible(project, transformer, catalog)?;
    if !allowed["eligible_versions"]
        .as_array()
        .is_some_and(|ids| ids.iter().any(|id| id == version_id))
        || !catalog["versions"].as_array().is_some_and(|entries| {
            entries
                .iter()
                .any(|e| e["version_id"] == version_id && e["head_revision_id"] == revision_id)
        })
    {
        return Err("promotion requires the exact selected playable runtime revision; archived layouts remain read-only".into());
    }
    Ok(
        serde_json::json!({"action":"get_head","project_id":project,"object":{"kind":"lv_transformer","id":transformer}}),
    )
}
pub fn capture_context(
    project: &str,
    transformer: &str,
    catalog: &Value,
    version_id: &str,
    revision_id: &str,
    response: &Value,
) -> Result<Context, String> {
    context_read_plan(project, transformer, catalog, version_id, revision_id)?;
    let head = &response["head"];
    if head["project_id"] != project
        || head["object"] != serde_json::json!({"kind":"lv_transformer","id":transformer})
    {
        return Err("promotion head read differs from the selected project/transformer".into());
    }
    let context = Context {
        project: project.into(),
        transformer: transformer.into(),
        version_id: version_id.into(),
        revision_id: revision_id.into(),
        expected_catalog_revision: catalog["catalog_revision"]
            .as_str()
            .ok_or("catalog fence absent")?
            .into(),
        expected_source_revision: head["source_revision"]
            .as_str()
            .ok_or("current source fence absent")?
            .into(),
    };
    context.promotion(project, transformer, "Prepare promotion", "prepare-only")?;
    Ok(context)
}
impl Context {
    pub fn promotion(
        &self,
        project: &str,
        transformer: &str,
        reason: &str,
        idempotency_key: &str,
    ) -> Result<Promotion, String> {
        if self.project != project || self.transformer != transformer {
            return Err("promotion context owner changed".into());
        }
        let promotion = Promotion {
            version_id: self.version_id.clone(),
            revision_id: self.revision_id.clone(),
            expected_catalog_revision: self.expected_catalog_revision.clone(),
            expected_source_revision: self.expected_source_revision.clone(),
            task_ids: vec![],
            source_record_ids: vec![],
            reason: reason.into(),
            idempotency_key: idempotency_key.into(),
        };
        promotion.validate()?;
        Ok(promotion)
    }
}
#[cfg(test)]
mod ui_context_tests {
    use super::*;
    use serde_json::json;
    fn catalog() -> Value {
        json!({"schema":"ds.lv-version-catalog/v1","project_id":"p","object":{"kind":"lv_transformer","id":"t"},"catalog_revision":"a".repeat(64),"baseline_ready":true,"versions":[{"version_id":"v0","display_ordinal":0,"head_revision_id":"v0","revision_count":1,"playback_available":true,"snapshot_scope":"runtime_design"}]})
    }
    #[test]
    fn archived_unavailable_or_rebound_revisions_never_start_a_head_read() {
        for (key, value) in [
            ("snapshot_scope", json!("exported_design")),
            ("playback_available", json!(false)),
        ] {
            let mut c = catalog();
            c["versions"][0][key] = value;
            assert!(context_read_plan("p", "t", &c, "v0", "v0").is_err());
        }
        let mut c = catalog();
        c["baseline_ready"] = json!(false);
        assert!(context_read_plan("p", "t", &c, "v0", "v0").is_err());
        assert!(context_read_plan("p", "t", &catalog(), "v0", "v1").is_err());
        assert!(context_read_plan("other", "t", &catalog(), "v0", "v0").is_err());
    }
    #[test]
    fn exact_current_source_and_catalog_fences_survive_the_native_plan() {
        let head = json!({"head":{"project_id":"p","object":{"kind":"lv_transformer","id":"t"},"source_revision":"2026-10-13T12:00:00Z"}});
        let c = capture_context("p", "t", &catalog(), "v0", "v0", &head).unwrap();
        let p = c
            .promotion("p", "t", "Restore checked version", "operation1")
            .unwrap();
        assert_eq!(p.expected_catalog_revision, "a".repeat(64));
        assert_eq!(p.expected_source_revision, "2026-10-13T12:00:00Z");
        assert!(c.promotion("other", "t", "reason", "key").is_err());
        assert!(c.promotion("p", "t", " ", "key").is_err());
        let mut wrong = head;
        wrong["head"]["source_revision"] = json!("unknown");
        assert!(capture_context("p", "t", &catalog(), "v0", "v0", &wrong).is_err());
    }
}
