//! Settings changes are planned against a complete backend resolution, online or cached.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFormChange {
    pub form_slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<u64>,
}
pub fn validate_changes(changes: &[ProjectFormChange]) -> Result<(), String> {
    let mut seen = HashSet::new();
    if changes.is_empty() || changes.len() > 32 {
        return Err("Settings batch must contain 1 through 32 forms".into());
    }
    for change in changes {
        if change.form_slug.is_empty()
            || change.form_slug.trim() != change.form_slug
            || change.form_slug.len() > 160
            || !seen.insert(&change.form_slug)
        {
            return Err("Settings batch contains an invalid or duplicate form slug".into());
        }
        if change.enabled.is_none() && change.settings.is_none() {
            return Err("Settings change requires enabled or settings".into());
        }
        if change.settings.is_some() != change.expected_version.is_some()
            || change
                .expected_version
                .is_some_and(|v| v > 9_007_199_254_740_991)
        {
            return Err("Settings edits require their exact settings revision".into());
        }
    }
    Ok(())
}
pub fn plan(
    project: &str,
    payload: &Value,
    changes: &[ProjectFormChange],
) -> Result<Value, String> {
    validate_changes(changes)?;
    if payload["project_id"] != project {
        return Err("Resolved forms belong to another project".into());
    }
    let forms = payload["forms"]
        .as_array()
        .ok_or("Resolved form catalogue is missing")?;
    let forms: HashMap<_, _> = forms
        .iter()
        .filter_map(|f| f["slug"].as_str().map(|s| (s, f)))
        .collect();
    let empty = Vec::new();
    let orphans: HashMap<_, _> = payload["orphaned_project_forms"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|f| f["form_slug"].as_str().map(|s| (s, f)))
        .collect();
    let mut planned = Vec::new();
    for change in changes {
        let mut row = serde_json::to_value(change).map_err(|_| "Invalid settings change")?;
        let mut reasons = Vec::<String>::new();
        let form = forms.get(change.form_slug.as_str());
        let orphan = orphans.get(change.form_slug.as_str());
        if form.is_none() && orphan.is_none() {
            reasons.push("form is not present in the authoritative project-form catalogue".into());
        }
        if let Some(orphan) = orphan {
            row["unavailable_reason"] = orphan["reason"].clone();
            if change.enabled != Some(false) || change.settings.is_some() {
                reasons.push("the master form is unavailable; only an enable-only false cleanup is permitted".into());
            }
        }
        if let Some(settings) = &change.settings {
            let editor = &payload["settings_editors"][&change.form_slug];
            if editor["form"]["slug"] != change.form_slug
                || !editor["sections"].is_array()
                || editor["version"].as_u64().is_none()
            {
                reasons.push("authoritative settings editor is missing or malformed".into());
            } else {
                row["current_version"] = editor["version"].clone();
                if editor["read_only"] == true || editor["unavailable"] == true {
                    reasons.push("form settings are read-only or unavailable".into());
                }
                if editor["version"].as_u64() != change.expected_version {
                    reasons.push(format!(
                        "expected settings revision {} but current revision is {}",
                        change.expected_version.unwrap(),
                        editor["version"]
                    ));
                }
                let keys: HashSet<&str> = editor["sections"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|s| s["fields"].as_array())
                    .flatten()
                    .filter_map(|f| f["key"].as_str())
                    .collect();
                let unknown: Vec<_> = settings
                    .keys()
                    .filter(|k| !keys.contains(k.as_str()))
                    .cloned()
                    .collect();
                if !unknown.is_empty() {
                    reasons.push(format!(
                        "settings keys are not in the backend editor: {}",
                        unknown.join(", ")
                    ));
                }
            }
        }
        row["ready"] = json!(reasons.is_empty());
        row["reasons"] = json!(reasons);
        planned.push(row);
    }
    Ok(json!({"project":project,"ready":planned.iter().all(|r|r["ready"]==true),"changes":planned}))
}
