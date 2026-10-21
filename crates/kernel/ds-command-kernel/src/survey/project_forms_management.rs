//! Project-form drafts and unavailable bindings. The page supplies observations;
//! this owner decides which edits survive and which changes may be published.
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}
fn rows<'a>(value: &'a Value, label: &str) -> Result<&'a [Value], String> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{label} must be an array"))
}
fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}

pub(super) fn evaluate(request: &Value) -> Result<Value, String> {
    match text(&request["mode"]) {
        "draft_scope" => {
            let owner = text(&request["owner"]);
            let project = text(&request["project"]);
            Ok(
                if owner.is_empty() || owner.trim() != owner || project.trim().is_empty() {
                    Value::Null
                } else {
                    json!(serde_json::to_string(&(owner, project)).map_err(|e| e.to_string())?)
                },
            )
        }
        "refresh_confirmations" => {
            let project = text(&request["project"]);
            let mut result = Map::new();
            for binding in rows(&request["orphans"], "Unavailable forms")? {
                let slug = text(&binding["form_slug"]).trim();
                if binding["project_id"] == project
                    && !slug.is_empty()
                    && !text(&binding["reason"]).trim().is_empty()
                    && binding["enabled"] == true
                {
                    result.insert(slug.into(), json!({"projectId":project,"source":"refresh"}));
                }
            }
            Ok(result.into())
        }
        "editor_confirmation" => Ok(json!(
            !text(&request["slug"]).is_empty()
                && request["editor"]["form"]["slug"] == request["slug"]
                && request["editor"]["form"]["enabled"] == true
                && request["editor"]["unavailable"] == true
                && request["editor"]["read_only"] == true
        )),
        "orphan_views" => orphan_views(request),
        "stage_draft" => stage(request),
        "working_draft" => working_draft(request),
        "clear_settings" => clear_settings(request),
        "conflict_slug" => conflict_slug(request),
        "recover_conflict" => recover_conflict(request),
        "remove_submitted" => {
            let mut next = object(&request["source"], "Drafts")?.clone();
            for (slug, sent) in object(&request["submitted"], "Submitted drafts")? {
                if next.get(slug) == Some(sent) {
                    next.remove(slug);
                }
            }
            Ok(next.into())
        }
        "publish_plan" => publish(request),
        _ => Err("Unknown project forms operation".into()),
    }
}

fn slugs(forms: &Value) -> Result<BTreeSet<&str>, String> {
    Ok(rows(forms, "Forms")?
        .iter()
        .filter_map(|row| row["slug"].as_str())
        .collect())
}

fn orphan_views(request: &Value) -> Result<Value, String> {
    let project = text(&request["project"]);
    let forms = slugs(&request["forms"])?;
    let mut by_slug = BTreeMap::<String, Value>::new();
    for binding in rows(&request["orphans"], "Unavailable forms")? {
        let slug = text(&binding["form_slug"]).trim();
        if slug.is_empty() || forms.contains(slug) {
            continue;
        }
        let reason = text(&binding["reason"]).trim();
        let verified =
            binding["project_id"] == project && binding["enabled"] == true && !reason.is_empty();
        let mut candidate = object(binding, "Unavailable form")?.clone();
        candidate.insert("form_slug".into(), slug.into());
        candidate.insert(
            "reason".into(),
            if reason.is_empty() {
                "unverified_backend_binding"
            } else {
                reason
            }
            .into(),
        );
        candidate.insert("reportedByBackend".into(), verified.into());
        candidate.insert("confirmedForWrite".into(), false.into());
        if by_slug
            .get(slug)
            .is_none_or(|prior| prior["reportedByBackend"] != true && verified)
        {
            by_slug.insert(slug.into(), candidate.into());
        }
    }
    for slug in object(&request["staged"], "Drafts")?.keys() {
        if slug.is_empty() || forms.contains(slug.as_str()) || by_slug.contains_key(slug) {
            continue;
        }
        by_slug.insert(slug.clone(), json!({"project_id":project,"form_slug":slug,
            "reason":"preserved_local_draft_without_binding","reportedByBackend":false,"confirmedForWrite":false}));
    }
    let confirmations = object(&request["confirmations"], "Confirmations")?;
    for (slug, binding) in &mut by_slug {
        let confirmed = confirmations
            .get(slug)
            .is_some_and(|value| value["projectId"] == project);
        binding["confirmedForWrite"] = confirmed.into();
        if confirmed {
            binding["enabled"] = true.into();
        }
    }
    Ok(Value::Array(by_slug.into_values().collect()))
}

fn stage(request: &Value) -> Result<Value, String> {
    let mut staged = object(&request["staged"], "Drafts")?.clone();
    let slug = text(&request["slug"]);
    if slug.is_empty() {
        return Err("Choose a form to stage".into());
    }
    let patch = object(&request["patch"], "Draft change")?;
    let revision = super::revision(&request["revision"]).ok_or("Invalid settings revision")?;
    let current = staged
        .get(slug)
        .cloned()
        .unwrap_or_else(|| json!({"updatedAt":request["now"]}));
    let current = object(&current, "Draft")?;
    let staged_revision = current.get("editorVersion").and_then(super::revision);
    let editor_revision = if current.contains_key("settings") {
        staged_revision
    } else {
        Some(staged_revision.unwrap_or(revision).max(revision))
    };
    let mut next = current.clone();
    next.extend(patch.clone());
    if patch
        .get("settings")
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty)
    {
        next.remove("settings");
    }
    next.insert("updatedAt".into(), request["now"].clone());
    match editor_revision {
        Some(revision) => {
            next.insert("editorVersion".into(), revision.into());
        }
        None => {
            next.remove("editorVersion");
        }
    }
    if patch.contains_key("settings") && !patch.contains_key("conflictKeys") {
        next.remove("conflictKeys");
    }
    if !next.contains_key("settings") {
        next.remove("baseSettings");
        next.remove("conflictKeys");
    }
    if next.get("enabled") == request.get("original_enabled") {
        next.remove("enabled");
    }
    if next.contains_key("enabled") || next.contains_key("settings") {
        staged.insert(slug.into(), next.into());
    } else {
        staged.remove(slug);
    }
    Ok(staged.into())
}

fn working_draft(request: &Value) -> Result<Value, String> {
    let mut staged = object(&request["staged"], "Drafts")?.clone();
    let working = &request["working"];
    if working["valid"] != true || !working["settings"].is_object() {
        return Ok(staged.into());
    }
    let slug = text(&working["formSlug"]);
    if slug.is_empty() {
        return Err("Choose a form to stage".into());
    }
    let mut next = staged
        .get(slug)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for key in ["settings", "baseSettings", "editorVersion"] {
        if let Some(value) = working.get(key) {
            next.insert(key.into(), value.clone());
        }
    }
    next.insert("updatedAt".into(), request["now"].clone());
    next.remove("conflictKeys");
    staged.insert(slug.into(), next.into());
    Ok(staged.into())
}

fn clear_settings(request: &Value) -> Result<Value, String> {
    let mut staged = object(&request["staged"], "Drafts")?.clone();
    let slug = text(&request["slug"]);
    if let Some(current) = staged.get(slug).cloned() {
        let mut next = object(&current, "Draft")?.clone();
        if next.contains_key("settings") {
            for key in ["settings", "baseSettings", "conflictKeys"] {
                next.remove(key);
            }
            next.insert("updatedAt".into(), request["now"].clone());
            if next.contains_key("enabled") {
                staged.insert(slug.into(), next.into());
            } else {
                staged.remove(slug);
            }
        }
    }
    Ok(staged.into())
}

fn conflict_slug(request: &Value) -> Result<Value, String> {
    let staged = object(&request["staged"], "Drafts")?;
    let advertised = text(&request["slug"]).trim();
    if !advertised.is_empty() {
        return Ok(advertised.into());
    }
    let mut candidates = staged.iter().filter(|(_, row)| !row["settings"].is_null());
    Ok(match (candidates.next(), candidates.next()) {
        (Some((slug, _)), None) => slug.clone().into(),
        _ => "".into(),
    })
}

fn recover_conflict(request: &Value) -> Result<Value, String> {
    let mut staged = object(&request["staged"], "Drafts")?.clone();
    let slug = text(&request["slug"]);
    let editor = request.get("editor").filter(|value| !value.is_null());
    let conflict = &request["conflict"];
    let revision = if let Some(editor) = editor {
        super::revision(&editor["version"])
    } else if conflict["currentVersionAvailable"] == true {
        super::revision(&conflict["currentVersion"])
    } else {
        None
    };
    let can_compare = editor.is_some() || conflict["currentSettingsAvailable"] == true;
    let latest = editor
        .map(|editor| &editor["current_settings"])
        .unwrap_or(&conflict["currentSettings"]);
    let keys;
    if let Some(revision) = revision.filter(|_| can_compare) {
        let reconciled =
            super::super::draft::evaluate(&json!({"mode":"conflict", "cached":staged,
            "slug":slug, "revision":revision, "latest":latest, "now":request["now"]}))?;
        staged = object(&reconciled["fresh"], "Reconciled drafts")?.clone();
        keys = reconciled["conflictKeys"].clone();
    } else {
        if let Some(revision) = revision {
            let rebased =
                super::super::draft::evaluate(&json!({"mode":"revision", "cached":staged,
                "slug":slug, "revision":revision}))?;
            staged = object(&rebased["fresh"], "Rebased drafts")?.clone();
        }
        keys = json!(staged
            .get(slug)
            .and_then(|row| row["settings"].as_object())
            .map(|settings| settings.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default());
        if keys.as_array().is_some_and(|keys| !keys.is_empty()) {
            if let Some(row) = staged.get_mut(slug) {
                row["conflictKeys"] = keys.clone();
            }
        }
    }
    let editor_patch =
        if editor.is_none() && conflict["currentSettingsAvailable"] == true && revision.is_some() {
            json!({"current_settings":latest,"version":revision})
        } else {
            Value::Null
        };
    Ok(json!({"fresh":staged,"conflictKeys":keys,"editorPatch":editor_patch}))
}

fn publish(request: &Value) -> Result<Value, String> {
    let forms = slugs(&request["forms"])?;
    let confirmed: BTreeSet<&str> = rows(&request["orphans"], "Unavailable forms")?
        .iter()
        .filter(|row| row["confirmedForWrite"] == true)
        .filter_map(|row| row["form_slug"].as_str())
        .collect();
    let mut changes = Vec::new();
    let mut blocked = Vec::new();
    let mut submitted = Map::new();
    for (slug, change) in object(&request["staged"], "Drafts")? {
        let change_object = object(change, "Draft change")?;
        if !forms.contains(slug.as_str()) {
            if change_object.contains_key("settings")
                || !confirmed.contains(slug.as_str())
                || change["enabled"] != false
            {
                blocked.push(slug.clone());
                continue;
            }
            changes.push(json!({"form_slug":slug,"enabled":false}));
        } else {
            let mut row = Map::from_iter([("form_slug".into(), slug.clone().into())]);
            if let Some(enabled) = change_object.get("enabled") {
                row.insert("enabled".into(), enabled.clone());
            }
            if let Some(settings) = change_object.get("settings") {
                let Some(revision) = super::revision(&change["editorVersion"]) else {
                    return Ok(
                        json!({"failure":{"code":"project_forms_revision_missing","formSlug":slug}}),
                    );
                };
                row.insert("settings".into(), settings.clone());
                row.insert("expected_version".into(), revision.into());
            }
            changes.push(row.into());
        }
        submitted.insert(slug.clone(), change.clone());
    }
    Ok(json!({"changes":changes,"blockedSlugs":blocked,"submitted":submitted}))
}

#[cfg(test)]
#[path = "project_forms_management_tests.rs"]
mod tests;
