//! Three-way reconciliation of offline project-settings drafts.
use serde_json::{json, Map, Value};
pub(super) fn evaluate(r: &Value) -> Result<Value, String> {
    let mut fresh = r["cached"]
        .as_object()
        .ok_or("Drafts must be an object")?
        .clone();
    let slug = r["slug"].as_str().unwrap_or("").trim();
    let revision = r["revision"].as_f64().filter(|v| *v >= 0.).map(f64::trunc);
    let current = fresh.get(slug).cloned();
    let simple = r["mode"] == "revision";
    if slug.is_empty() || revision.is_none() || current.is_none() {
        return Ok(if simple {
            json!({"fresh":fresh,"rebased":false})
        } else {
            json!({"fresh":fresh,"rebased":false,"canAutoRetry":false,"conflictKeys":[],"resolvedKeys":[]})
        });
    }
    let mut next = current.unwrap();
    let revision = json!(revision.unwrap() as u64);
    if simple {
        let changed = next["editorVersion"] != revision;
        next["editorVersion"] = revision;
        fresh.insert(slug.into(), next);
        return Ok(json!({"fresh":fresh,"rebased":changed}));
    }
    if r["mode"] != "conflict" {
        return Err("Unknown draft operation".into());
    }
    let latest = r["latest"]
        .as_object()
        .ok_or("Latest settings must be an object")?;
    let mut remaining = Map::new();
    let mut conflicts = Vec::new();
    let mut resolved = Vec::new();
    if let Some(patch) = next["settings"].as_object() {
        for (key, value) in patch {
            if latest.get(key) == Some(value) {
                resolved.push(key.clone());
                continue;
            }
            remaining.insert(key.clone(), value.clone());
            if !next["baseSettings"]
                .as_object()
                .is_some_and(|base| base.get(key) == latest.get(key))
            {
                conflicts.push(key.clone());
            }
        }
    }
    next["editorVersion"] = revision;
    next["updatedAt"] = r["now"].clone();
    let object = next.as_object_mut().ok_or("Draft must be an object")?;
    if remaining.is_empty() {
        for key in ["settings", "baseSettings", "conflictKeys"] {
            object.remove(key);
        }
    } else {
        object.insert("settings".into(), Value::Object(remaining));
        object.insert("baseSettings".into(), Value::Object(latest.clone()));
        if conflicts.is_empty() {
            object.remove("conflictKeys");
        } else {
            object.insert("conflictKeys".into(), json!(conflicts));
        }
    }
    if !object.contains_key("enabled") && !object.contains_key("settings") {
        fresh.remove(slug);
    } else {
        fresh.insert(slug.into(), next);
    }
    Ok(
        json!({"fresh":fresh,"rebased":true,"canAutoRetry":conflicts.is_empty(),"conflictKeys":conflicts,"resolvedKeys":resolved}),
    )
}
