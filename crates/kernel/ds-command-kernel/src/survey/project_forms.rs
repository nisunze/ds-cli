//! Settings changes are planned against a complete backend resolution, online or cached.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[path = "project_forms_management.rs"]
mod management;

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

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

fn revision(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .filter(|value| *value <= MAX_SAFE_INTEGER)
        .or_else(|| {
            value.as_str().and_then(|value| {
                (value == "0"
                    || (!value.starts_with('0') && value.bytes().all(|byte| byte.is_ascii_digit())))
                .then(|| value.parse::<u64>().ok())
                .flatten()
                .filter(|value| *value <= MAX_SAFE_INTEGER)
            })
        })
}

pub fn evaluate(request: &Value) -> Result<Value, String> {
    match request["mode"].as_str() {
        Some("conflict") => conflict(&request["details"]),
        Some("receipt") => receipt(request),
        Some("cleanup") => cleanup(&request["saved"]),
        _ => management::evaluate(request),
    }
}

fn conflict(details: &Value) -> Result<Value, String> {
    let details = details
        .as_object()
        .ok_or("Conflict details must be an object")?;
    let current_revision = revision(
        details
            .get("current_settings_revision")
            .unwrap_or(&Value::Null),
    )
    .or_else(|| revision(details.get("current_version").unwrap_or(&Value::Null)));
    let expected_revision = revision(
        details
            .get("expected_settings_revision")
            .unwrap_or(&Value::Null),
    )
    .or_else(|| revision(details.get("expected_version").unwrap_or(&Value::Null)));
    let current_settings = if current_revision.is_some()
        && details
            .get("current_settings")
            .is_some_and(Value::is_object)
    {
        details["current_settings"].clone()
    } else {
        json!({})
    };
    Ok(json!({
        "form_slug": details.get("form_slug").and_then(Value::as_str).unwrap_or("").trim(),
        "expected_version": expected_revision.unwrap_or(0),
        "current_version": current_revision.unwrap_or(0),
        "current_version_available": current_revision.is_some(),
        "current_aggregate_revision": revision(details.get("current_aggregate_revision").unwrap_or(&Value::Null)).unwrap_or(0),
        "current_settings": current_settings,
        "current_settings_available": current_revision.is_some() && details.get("current_settings").is_some_and(Value::is_object),
        "conflict_type": details.get("conflict_type").and_then(Value::as_str).unwrap_or("settings_revision_mismatch"),
        "recovery_action": details.get("recovery_action").and_then(Value::as_str).unwrap_or("refresh_compare_settings_patch_then_retry"),
    }))
}

fn receipt(request: &Value) -> Result<Value, String> {
    let expected_project = request["expected_project_id"]
        .as_str()
        .ok_or("Project is required")?;
    if request["project_id"].as_str() != Some(expected_project) {
        return Err(format!(
            "Project forms save returned project {}, expected {expected_project}",
            request["project_id"].as_str().unwrap_or("unknown")
        ));
    }
    let requested = request["requested_slugs"]
        .as_array()
        .ok_or("Requested form slugs are missing")?;
    let saved = request["saved"]
        .as_array()
        .ok_or("Saved form rows are missing")?;
    let saved_count = request["saved_count"]
        .as_u64()
        .ok_or("Project forms save response could not be verified; refresh before saving again")?;
    let requested: Vec<&str> = requested
        .iter()
        .map(|value| value.as_str().ok_or("Requested form slug is invalid"))
        .collect::<Result<_, _>>()?;
    let saved_slugs: Vec<&str> = saved
        .iter()
        .map(|row| row["form_slug"].as_str().ok_or("Saved form row is invalid"))
        .collect::<Result<_, _>>()?;
    if saved_count as usize != requested.len()
        || saved_slugs.len() != requested.len()
        || requested
            .iter()
            .any(|slug| saved_slugs.iter().filter(|saved| *saved == slug).count() != 1)
    {
        return Err(
            "Project forms save response could not be verified; refresh before saving again".into(),
        );
    }
    Ok(json!({"authoritative_refresh": request["has_forms"] == true}))
}

fn cleanup(saved: &Value) -> Result<Value, String> {
    let saved = saved.as_array().ok_or("Saved form rows are missing")?;
    let mut removed = Vec::new();
    let mut failed = Vec::new();
    for row in saved {
        let slug = row["form_slug"]
            .as_str()
            .ok_or("Saved form row is invalid")?;
        match row["bq_cleanup_status"].as_str() {
            Some("completed") => removed.push(slug),
            Some("failed") => failed.push(slug),
            Some(_) => {}
            None if row["bq_dropped"] == true => removed.push(slug),
            None if row["enabled"] == false && row["bq_dropped"] == false => failed.push(slug),
            None => {}
        }
    }
    Ok(json!({"removed":removed, "failed":failed}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_refuses_malformed_versions_and_settings_fallback() {
        let result = evaluate(&json!({"mode":"conflict", "details":{"form_slug":" pole ", "current_settings_revision":"07", "current_settings":{"x":true}}})).unwrap();
        assert_eq!(result["form_slug"], "pole");
        assert_eq!(result["current_version_available"], false);
        assert_eq!(result["current_settings"], json!({}));
    }

    #[test]
    fn receipt_and_optional_cleanup_are_verified_together() {
        let receipt = evaluate(&json!({"mode":"receipt", "expected_project_id":"p", "project_id":"p", "requested_slugs":["a"], "saved_count":1, "saved":[{"form_slug":"a"}], "has_forms":false})).unwrap();
        assert_eq!(receipt["authoritative_refresh"], false);
        let cleanup = evaluate(&json!({"mode":"cleanup", "saved":[{"form_slug":"gone", "bq_dropped":true},{"form_slug":"failed", "enabled":false,"bq_dropped":false},{"form_slug":"optional","bq_cleanup_status":"not_configured"}]})).unwrap();
        assert_eq!(cleanup, json!({"removed":["gone"], "failed":["failed"]}));
    }
}
