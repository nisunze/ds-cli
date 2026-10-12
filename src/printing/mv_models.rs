//! One logical MV model per authoritative project head. Held worker provenance
//! may identify an exact working copy; labels and open-model membership never
//! select project data. No acquisition, focus, save or mutation lives here.
use crate::grid_held_models::{self, Inventory};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const SCHEMA: &str = "ds.mv-model-catalogue/v1";
pub const MAX_MODELS: usize = 4096;
pub const INVALID: &str = "mv_model_catalogue_invalid";
pub const STALE: &str = "mv_model_source_stale";
pub fn scene_revision(bytes: &[u8]) -> Result<String, &'static str> {
    #[derive(Deserialize)]
    struct Revision {
        model_revision: String,
    }
    if bytes.len() > 256 * 1024 * 1024 {
        return Err(INVALID);
    }
    let revision: Revision = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    if !id(&revision.model_revision) {
        return Err(INVALID);
    }
    Ok(revision.model_revision)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBinding {
    pub project_id: String,
    pub model_id: String,
    pub revision_id: String,
    pub content_sha256: String,
    pub version: u64,
    pub worker_id: String,
    pub worker_model_id: String,
    pub worker_revision: String,
    pub history_pin: String,
    pub coordinate_system: String,
    pub read_only: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Origin {
    #[serde(rename = "dsProject")]
    project: String,
    #[serde(rename = "modelId")]
    model: String,
    #[serde(rename = "revisionId")]
    revision: String,
    version: u64,
    digest: String,
}
fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
fn digest(value: &str) -> bool {
    super::preview_progress::digest(value.strip_prefix("sha256:").unwrap_or(value))
}
fn bare(value: &str) -> &str {
    value.strip_prefix("sha256:").unwrap_or(value)
}

pub fn project(
    project: &str,
    account: &str,
    listing: &Value,
    inventory: Option<Inventory>,
) -> Result<Value, &'static str> {
    if !id(project) || !id(account) || listing["project"] != project || listing["more"] != false {
        return Err(INVALID);
    }
    let heads = listing["models"]
        .as_array()
        .filter(|rows| rows.len() <= MAX_MODELS)
        .ok_or(INVALID)?;
    let inventory = match inventory {
        Some(inventory) => {
            let inventory =
                grid_held_models::admit(serde_json::to_value(inventory).map_err(|_| INVALID)?)
                    .map_err(|_| STALE)?;
            if inventory.context.project.as_deref() != Some(project)
                || inventory.context.account.as_deref() != Some(account)
            {
                return Err(STALE);
            }
            Some(inventory)
        }
        None => None,
    };
    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    for head in heads {
        let model = head["model_id"]
            .as_str()
            .filter(|value| id(value))
            .ok_or(INVALID)?;
        if !seen.insert(model) {
            return Err(INVALID);
        }
        if head["model_kind"] != "mv_line"
            || head["lifecycle"] != "promoted"
            || head["state"] == "deleted"
        {
            continue;
        }
        let revision = head["head_revision_id"]
            .as_str()
            .filter(|v| id(v))
            .ok_or(INVALID)?;
        let sha = head["head_model_digest"]
            .as_str()
            .filter(|v| digest(v))
            .ok_or(INVALID)?;
        let version = head["head_version"]
            .as_u64()
            .filter(|v| *v > 0)
            .ok_or(INVALID)?;
        let label = head["display_name"]
            .as_str()
            .filter(|v| id(v))
            .unwrap_or(model);
        let mut candidates = Vec::new();
        if let Some(inventory) = &inventory {
            for row in &inventory.rows {
                let Some(origin) = row
                    .project_binding
                    .as_ref()
                    .and_then(|v| serde_json::from_value::<Origin>(v.clone()).ok())
                else {
                    continue;
                };
                if origin.project != project
                    || origin.model != model
                    || origin.revision != revision
                    || bare(&origin.digest) != bare(sha)
                    || origin.version != version
                {
                    continue;
                }
                if (row.read_only
                    && head["manifest_model_id"]
                        .as_str()
                        .is_some_and(|expected| expected != row.native.manifest_model_id))
                    || row.saving
                    || row.pending_operations != 0
                {
                    continue;
                }
                let source = SourceBinding {
                    project_id: project.into(),
                    model_id: model.into(),
                    revision_id: revision.into(),
                    content_sha256: bare(sha).into(),
                    version,
                    worker_id: row.model_id.clone(),
                    worker_model_id: row.native.manifest_model_id.clone(),
                    worker_revision: row.native.revision.clone(),
                    history_pin: row.native.history.history_pin.clone(),
                    coordinate_system: row.native.coordinate_system.clone(),
                    read_only: row.read_only,
                };
                candidates.push(json!({"worker_id":row.model_id,"name":row.name,"persisted":row.persisted,"source":source}));
            }
        }
        let state = match candidates.len() {
            0 => "load_needed",
            1 => "held",
            _ => "choose_working_copy",
        };
        let source = if candidates.len() == 1 {
            candidates[0]["source"].clone()
        } else {
            Value::Null
        };
        models.push(json!({"model_id":model,"name":label,"head_revision_id":revision,"head_model_digest":bare(sha),"head_version":version,"state":state,"source":source,"candidates":candidates}));
    }
    Ok(json!({"schema":SCHEMA,"project_id":project,"models":models,"mutated":false}))
}

/// A preview consumes only the exact selected project head and captured worker
/// revision. It cannot silently borrow a different project, revision or copy.
pub fn admit_context(project: &str, source: &SourceBinding) -> Result<(), &'static str> {
    if source.project_id != project
        || !id(&source.model_id)
        || !id(&source.revision_id)
        || !super::preview_progress::digest(&source.content_sha256)
        || source.version == 0
        || !id(&source.worker_id)
        || !id(&source.worker_model_id)
        || !id(&source.worker_revision)
        || !id(&source.history_pin)
        || !id(&source.coordinate_system)
    {
        return Err(STALE);
    }
    Ok(())
}

pub fn admit_head(
    project: &str,
    account: &str,
    listing: &Value,
    source: &SourceBinding,
    revision: &str,
) -> Result<(), &'static str> {
    admit_context(project, source)?;
    if source.worker_revision != revision {
        return Err(STALE);
    }
    let catalogue = self::project(project, account, listing, None)?;
    if !catalogue["models"].as_array().is_some_and(|rows| {
        rows.iter().any(|row| {
            row["model_id"] == source.model_id
                && row["head_revision_id"] == source.revision_id
                && row["head_model_digest"] == source.content_sha256
                && row["head_version"] == source.version
        })
    }) {
        return Err(STALE);
    }
    Ok(())
}

pub fn admit_source(
    project: &str,
    catalogue: &Value,
    source: &SourceBinding,
    worker_revision: &str,
) -> Result<(), &'static str> {
    admit_context(project, source)?;
    if catalogue["schema"] != SCHEMA
        || catalogue["project_id"] != project
        || source.project_id != project
        || source.worker_revision != worker_revision
    {
        return Err(STALE);
    }
    let rows = catalogue["models"].as_array().ok_or(INVALID)?;
    let row = rows
        .iter()
        .find(|row| row["model_id"] == source.model_id)
        .ok_or(STALE)?;
    if row["head_revision_id"] != source.revision_id
        || row["head_model_digest"] != source.content_sha256
        || row["head_version"] != source.version
        || !row["candidates"].as_array().is_some_and(|candidates| {
            candidates.iter().any(|candidate| {
                candidate["source"] == serde_json::to_value(source).unwrap_or(Value::Null)
            })
        })
    {
        return Err(STALE);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn head(model: &str) -> Value {
        json!({"model_id":model,"display_name":"Copied original name","model_kind":"mv_line","lifecycle":"promoted","state":"active","head_revision_id":"head","head_version":1,"head_model_digest":"a".repeat(64),"manifest_model_id":model})
    }
    fn inventory() -> Inventory {
        let context = json!({"account":"owner","project":"template","generation":1});
        let rows = (0..11).map(|n| { let model=format!("model-{}",n%3); json!({"model_id":format!("local-{n}"),"name":"Opened copy","selected":n==0,"read_only":false,"persisted":false,"host_revision":"rev:dirty","workspace_owner":"mv-design","project_binding":{"dsProject":if n<3 {"template"} else {"other"},"modelId":model,"revisionId":"head","version":1,"digest":"a".repeat(64)},"saving":false,"pending_operations":0,"physical_worker_active":false,"native":{"manifest_model_id":model,"revision":"rev:dirty","model_revision":7,"coordinate_system":"EPSG:32736","history":{"model_revision":"rev:dirty","initial_revision":"rev:base","undo_depth":4,"redo_depth":2,"history_pin":"pin"}}}) }).collect::<Vec<_>>();
        grid_held_models::admit(json!({"captured_context":context,"context":context,"total":rows.len(),"rows":rows,"mutated":false})).unwrap()
    }
    fn listing() -> Value {
        json!({"project":"template","models":(0..3).map(|n| head(&format!("model-{n}"))).collect::<Vec<_>>(),"more":false})
    }
    #[test]
    fn three_project_heads_remain_three_choices_despite_eleven_dirty_open_copies() {
        let held = inventory();
        let before = serde_json::to_value(&held).unwrap();
        let result = project("template", "owner", &listing(), Some(held.clone())).unwrap();
        assert_eq!(result["models"].as_array().unwrap().len(), 3);
        for row in result["models"].as_array().unwrap() {
            assert_eq!(row["state"], "held");
            assert_eq!(row["candidates"].as_array().unwrap().len(), 1);
            assert_eq!(row["source"]["project_id"], "template");
        }
        assert_eq!(serde_json::to_value(held).unwrap(), before);
        let source: SourceBinding =
            serde_json::from_value(result["models"][0]["source"].clone()).unwrap();
        assert!(admit_source("template", &result, &source, "rev:dirty").is_ok());
        assert!(admit_head("template", "owner", &listing(), &source, "rev:dirty").is_ok());
        assert!(admit_head("foreign", "owner", &listing(), &source, "rev:dirty").is_err());
        assert!(admit_head("template", "owner", &listing(), &source, "rev:changed").is_err());
        assert_eq!(
            scene_revision(br#"{"model_revision":"rev:dirty","large_data":[1,2,3]}"#).unwrap(),
            "rev:dirty"
        );
        assert!(admit_source("other", &result, &source, "rev:dirty").is_err());
        assert!(admit_source("template", &result, &source, "rev:new").is_err());
    }
    #[test]
    fn exact_copy_ambiguity_requires_selection_and_old_or_unbound_copies_do_not_select_a_head() {
        let mut held = inventory();
        let mut duplicate = held.rows[0].clone();
        duplicate.model_id = "another-copy".into();
        duplicate.selected = false;
        held.rows.push(duplicate);
        held.total += 1;
        let result = project("template", "owner", &listing(), Some(held.clone())).unwrap();
        assert_eq!(result["models"][0]["state"], "choose_working_copy");
        assert!(result["models"][0]["source"].is_null());
        for row in &mut held.rows {
            row.project_binding.as_mut().unwrap()["revisionId"] = json!("old-head");
        }
        let result = project("template", "owner", &listing(), Some(held)).unwrap();
        assert!(result["models"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["state"] == "load_needed"));
    }
    #[test]
    fn crossed_accounts_incomplete_catalogues_and_duplicate_heads_refuse() {
        assert!(project("template", "peer", &listing(), Some(inventory())).is_err());
        let mut value = listing();
        value["more"] = json!(true);
        assert!(project("template", "owner", &value, None).is_err());
        let mut value = listing();
        value["models"]
            .as_array_mut()
            .unwrap()
            .push(head("model-0"));
        assert!(project("template", "owner", &value, None).is_err());
        let mut value = listing();
        value["models"][0]["head_model_digest"] = json!("missing");
        assert!(project("template", "owner", &value, None).is_err());
    }
}
