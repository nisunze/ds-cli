//! Working-head facts, distinct from deliberate canonical design versions.
//! Hosts supply bounded server reads and local recovery facts. No save, rebase,
//! version increment or overwrite is inferred from a display name.
use serde_json::{json, Value};

#[path = "grid_model_management/saved.rs"]
pub mod saved;

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value[key].as_str().filter(|s| !s.is_empty())
}
fn identifier(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
fn digest(value: &str) -> Option<String> {
    let normalized = value
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or(value.trim())
        .to_ascii_lowercase();
    (normalized.len() == 64 && normalized.bytes().all(|b| b.is_ascii_hexdigit()))
        .then_some(normalized)
}
fn equal_digest(a: &Value, ak: &str, b: &Value, bk: &str) -> bool {
    text(a, ak)
        .and_then(digest)
        .zip(text(b, bk).and_then(digest))
        .is_some_and(|(a, b)| a == b)
}

/// One read projection used by native project-show and the model-management UI.
pub fn project(input: &Value) -> Result<Value, String> {
    let project = text(input, "project").ok_or("model management needs an explicit project")?;
    let model = text(input, "model").ok_or("model management needs an explicit model")?;
    if !identifier(project) || !identifier(model) {
        return Err("invalid project or model identity".into());
    }
    let head = &input["head"];
    if head["model_id"] != model || head.get("project_id").is_some_and(|p| p != project) {
        return Err("model head belongs to another project or model".into());
    }
    let revision = text(head, "head_revision_id").ok_or("model head has no revision")?;
    if !identifier(revision) || text(head, "head_model_digest").and_then(digest).is_none() {
        return Err("model head needs an exact revision and digest".into());
    }
    let named = &input["named_head"];
    let named_version = if named.is_null() {
        Value::Null
    } else {
        if named["project_id"] != project
            || named["object"]["kind"] != "mv_model"
            || named["object"]["id"] != model
        {
            return Err("named version head belongs to another project or model".into());
        }
        if named["source_revision"] != revision
            || !equal_digest(named, "pinned_model_digest", head, "head_model_digest")
        {
            return Err("model head moved while named version state was being read".into());
        }
        let ordinal = named["published_version"]
            .as_u64()
            .ok_or("named version ordinal is missing")?;
        json!({"version":ordinal,"label":if ordinal == 0 {"Unversioned".into()} else {format!("v{ordinal}")}})
    };
    Ok(json!({"project":project,"model_id":model,"working_head":{
        "revision_id":revision,"digest":head["head_model_digest"],
        "revision_count":head["head_version_revision_count"]
    },"named_version":named_version,"content_version":head["head_version"],"mutated":false}))
}

/// An unchanged recovery checkpoint is not evidence that its cloud base is
/// still current. Only an admitted server head can establish that relationship.
pub fn working_copy(input: &Value) -> Value {
    if input["project_selected"] != true || input["lookup"] == "idle" {
        return json!({"kind":"no_project"});
    }
    match input["lookup"].as_str() {
        Some("loading") => return json!({"kind":"loading"}),
        Some("error") => return json!({"kind":"unavailable"}),
        Some("ready") => {}
        _ => return json!({"kind":"unavailable"}),
    }
    let head = &input["head"];
    if head.is_null() {
        return json!({"kind":"not_in_project"});
    }
    let state = |kind: &str| json!({"kind":kind,"head":head});
    if head["lifecycle"] == "temporary" {
        return state("migration_needed");
    }
    if head["lifecycle"] != "promoted" {
        return state("unknown");
    }
    if text(input, "working_copy_digest")
        .and_then(digest)
        .is_none()
        || text(head, "head_model_digest").and_then(digest).is_none()
    {
        return state("unknown");
    }
    if input["working_copy_persisted"] == true
        && equal_digest(input, "working_copy_digest", head, "head_model_digest")
    {
        return state("current");
    }
    if input["read_only"] != true
        && text(input, "base_revision").is_some_and(|base| {
            Some(base) != text(head, "head_revision_id")
                || (text(input, "base_digest").and_then(digest).is_some()
                    && !equal_digest(input, "base_digest", head, "head_model_digest"))
        })
    {
        return json!({"kind":"conflict","head":head,"local_revision":input["local_revision"],
            "local_digest":input["working_copy_digest"],"base_revision":input["base_revision"],
            "base_digest":input["base_digest"],"cloud_revision":head["head_revision_id"],
            "cloud_digest":head["head_model_digest"],"safe_actions":["open_latest","keep_local_copy"],"overwrite_allowed":false});
    }
    json!({"kind":"changed","head":head,"reason":if input["working_copy_persisted"] == true { "digest_mismatch" } else { "uncheckpointed" }})
}

pub fn evaluate(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > 1024 * 1024 {
        return Err("model management facts exceed 1 MiB".into());
    }
    let input: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let result = match input["action"].as_str() {
        Some("project") => project(&input)?,
        Some("working_copy") => working_copy(&input),
        Some("saved_list" | "saved_open") => {
            let request = serde_json::from_value(input)
                .map_err(|_| "saved_model_request_invalid: invalid saved-local facts".to_owned())?;
            serde_json::to_value(saved::evaluate(request).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?
        }
        _ => {
            return Err("choose project, working_copy, saved_list or saved_open model facts".into())
        }
    };
    serde_json::to_string(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn head() -> Value {
        json!({"project_id":"p","model_id":"m","head_revision_id":"r2","head_model_digest":"a".repeat(64),"head_version":7,"head_version_revision_count":4,"lifecycle":"promoted"})
    }
    #[test]
    fn canonical_named_version_does_not_inherit_content_version() {
        let named = json!({"project_id":"p","object":{"kind":"mv_model","id":"m"},"source_revision":"r2","pinned_model_digest":"a".repeat(64),"published_version":0});
        let answer =
            project(&json!({"project":"p","model":"m","head":head(),"named_head":named})).unwrap();
        assert_eq!(answer["content_version"], 7);
        assert_eq!(answer["named_version"]["label"], "Unversioned");
        assert_eq!(answer["mutated"], false);
    }
    #[test]
    fn moved_head_and_foreign_named_version_are_refused() {
        for (project_id, revision) in [("other", "r2"), ("p", "older")] {
            let named = json!({"project_id":project_id,"object":{"kind":"mv_model","id":"m"},"source_revision":revision,"pinned_model_digest":"a".repeat(64),"published_version":1});
            assert!(
                project(&json!({"project":"p","model":"m","head":head(),"named_head":named}))
                    .is_err()
            );
        }
    }
    #[test]
    fn stale_base_preserves_exact_local_and_cloud_heads_without_overwrite() {
        let answer = working_copy(
            &json!({"project_selected":true,"lookup":"ready","head":head(),"working_copy_persisted":true,"working_copy_digest":"b".repeat(64),"base_revision":"r1","base_digest":"c".repeat(64),"local_revision":"rev:local"}),
        );
        assert_eq!(answer["kind"], "conflict");
        assert_eq!(answer["base_revision"], "r1");
        assert_eq!(answer["local_revision"], "rev:local");
        assert_eq!(answer["cloud_revision"], "r2");
        assert_eq!(answer["overwrite_allowed"], false);
        assert_eq!(
            answer["safe_actions"],
            json!(["open_latest", "keep_local_copy"])
        );
    }
    #[test]
    fn old_checkpoint_alone_cannot_claim_sync_and_unknown_lookup_is_closed() {
        let mut input = json!({"project_selected":true,"lookup":"ready","head":head(),"working_copy_persisted":true,"working_copy_digest":"b".repeat(64),"base_revision":"r1"});
        assert_eq!(working_copy(&input)["kind"], "conflict");
        input["working_copy_digest"] = json!(format!("sha256:{}", "A".repeat(64)));
        assert_eq!(working_copy(&input)["kind"], "current");
        input["lookup"] = json!("error");
        assert_eq!(working_copy(&input)["kind"], "unavailable");
    }
    #[test]
    fn shared_native_and_ui_fixtures_preserve_exact_projection() {
        let fixtures: Vec<Value> =
            serde_json::from_str(include_str!("../tests/fixtures/grid_model_management.json"))
                .unwrap();
        for fixture in fixtures {
            let result = evaluate(&serde_json::to_vec(&fixture["input"]).unwrap());
            if fixture["error"] == true {
                assert!(result.is_err(), "{}", fixture["name"]);
            } else {
                assert_eq!(
                    serde_json::from_str::<Value>(&result.unwrap()).unwrap(),
                    fixture["expected"],
                    "{}",
                    fixture["name"]
                );
            }
        }
    }
}
