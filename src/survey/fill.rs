//! Form-fill policy. Hosts retain browser bytes, timers, and IndexedDB I/O.
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

const BINARY_MARKER: &str = "__ds_survey_browser_binary__";

pub(super) fn evaluate(request: &Value) -> Result<Value, String> {
    match request["mode"].as_str() {
        Some("draft_values") => draft_values(request),
        Some("plain_draft_value") => plain_draft_value(request),
        Some("media_plan") => media_plan(request),
        Some("revision") => revision(request),
        Some("draft_admission") => draft_admission(request),
        Some("submission") => submission(request),
        _ => Err("Unknown survey fill operation".into()),
    }
}

fn media_kind(field: &Value) -> Option<&str> {
    match field["type"].as_str()? {
        kind @ ("photo" | "photos" | "video" | "videos" | "audio" | "audios" | "document"
        | "documents") => Some(kind),
        _ => None,
    }
}

fn paths(value: &Value) -> Vec<String> {
    match value {
        Value::String(path) if !path.is_empty() => vec![path.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn binary_marker(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == 1 && object.get(BINARY_MARKER) == Some(&Value::Bool(true))
    })
}

/// Removes only host-marked File/Blob values. Empty arrays and objects are not
/// durable draft values, matching the browser's old structured-clone guard.
fn clone_draft_value(value: &Value) -> Option<Value> {
    if binary_marker(value) {
        return None;
    }
    match value {
        Value::Array(items) => {
            let items: Vec<Value> = items.iter().filter_map(clone_draft_value).collect();
            (!items.is_empty()).then_some(Value::Array(items))
        }
        Value::Object(object) => {
            let mut out = Map::new();
            for (key, nested) in object {
                if let Some(cloned) = clone_draft_value(nested) {
                    out.insert(key.clone(), cloned);
                }
            }
            (!out.is_empty()).then_some(Value::Object(out))
        }
        _ => Some(value.clone()),
    }
}

fn field_kinds(fields: &Value) -> Result<HashMap<String, String>, String> {
    let fields = fields.as_array().ok_or("Form fields must be an array")?;
    if fields.len() > super::MAX_SURVEY_FIELDS {
        return Err("Survey form exceeds 512 fields".into());
    }
    let mut kinds = HashMap::new();
    for field in fields {
        if let (Some(key), Some(kind)) = (field["key"].as_str(), media_kind(field)) {
            kinds.insert(key.to_owned(), kind.to_owned());
        }
    }
    Ok(kinds)
}

fn draft_values(request: &Value) -> Result<Value, String> {
    let kinds = field_kinds(&request["fields"])?;
    let values = request["values"]
        .as_object()
        .ok_or("Survey values must be an object")?;
    let mut out = Map::new();
    for (key, value) in values {
        if let Some(kind) = kinds.get(key) {
            let paths = paths(value);
            if !paths.is_empty() {
                out.insert(
                    key.clone(),
                    if kind.ends_with('s') {
                        json!(paths)
                    } else {
                        json!(paths[0])
                    },
                );
            }
        } else if let Some(cloned) = clone_draft_value(value) {
            out.insert(key.clone(), cloned);
        }
    }
    Ok(Value::Object(out))
}

fn plain_draft_value(request: &Value) -> Result<Value, String> {
    Ok(match clone_draft_value(&request["value"]) {
        Some(value) => json!({"present": true, "value": value}),
        None => json!({"present": false}),
    })
}

fn text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn safe_segment(value: &str, fallback: &str, limit: usize) -> String {
    let mut out = String::new();
    let mut invalid = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-') {
            out.push(character);
            invalid = false;
        } else if !invalid {
            out.push('_');
            invalid = true;
        }
    }
    let trimmed = out.trim_matches(|character| matches!(character, '_' | ' ' | '.' | '-'));
    let result: String = trimmed.chars().take(limit).collect();
    if result.is_empty() {
        fallback.chars().take(limit).collect()
    } else {
        result
    }
}

fn existing_name(path: &str) -> Option<&str> {
    path.rsplit(['/', '\\']).find(|segment| !segment.is_empty())
}

fn media_plan(request: &Value) -> Result<Value, String> {
    let fields = request["fields"]
        .as_array()
        .ok_or("Form fields must be an array")?;
    if fields.len() > super::MAX_SURVEY_FIELDS {
        return Err("Survey form exceeds 512 fields".into());
    }
    let candidates = request["media"]
        .as_object()
        .ok_or("Media candidates must be an object")?;
    let project = request["project_id"]
        .as_str()
        .ok_or("Project is required")?;
    let form = request["form_slug"]
        .as_str()
        .ok_or("Form slug is required")?;
    let document = request["document_id"]
        .as_str()
        .ok_or("Document id is required")?;
    let now = request["now_ms"].as_u64().unwrap_or(0);
    let mut used_names = HashSet::new();
    let mut field_plans = Vec::new();
    let mut uploads = Vec::new();

    for field in fields {
        let Some(kind) = media_kind(field) else {
            continue;
        };
        let key = field["key"].as_str().ok_or("Survey field has no key")?;
        let candidate = &candidates[key];
        let mut field_paths = paths(&candidate["existing_paths"]);
        for path in &field_paths {
            if let Some(name) = existing_name(path) {
                used_names.insert(name.to_owned());
            }
        }
        let files = candidate["files"].as_array().cloned().unwrap_or_default();
        let field_name = safe_segment(key, "media", 40);
        for (index, file) in files.iter().enumerate() {
            let token = file["token"]
                .as_str()
                .filter(|token| !token.is_empty())
                .ok_or("Media token missing")?;
            let identity = file["identity"]
                .as_str()
                .filter(|identity| !identity.is_empty())
                .ok_or("Media identity missing")?;
            let raw_name = file["name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{key}_{now}_{index}"));
            let dot = raw_name.rfind('.').filter(|dot| *dot > 0);
            let (raw_stem, raw_ext) = dot
                .map(|dot| (&raw_name[..dot], &raw_name[dot + 1..]))
                .unwrap_or((&raw_name[..], ""));
            let stem = safe_segment(raw_stem, &field_name, 80);
            let extension = safe_segment(&raw_ext.to_ascii_lowercase(), "bin", 12);
            let file_name = format!("{field_name}-{identity}-{stem}.{extension}");
            if !used_names.insert(file_name.clone()) {
                return Err("Media object identity must be unique within an entry".into());
            }
            let path = format!("{project}/forms/{form}/{document}/{file_name}");
            field_paths.push(path.clone());
            uploads.push(json!({
                "token": token,
                "path": path,
                "photo": matches!(kind, "photo" | "photos"),
            }));
        }
        let present = !field_paths.is_empty();
        field_plans.push(if present {
            json!({
                "key": key,
                "present": true,
                "value": if kind.ends_with('s') { json!(field_paths) } else { json!(field_paths[0]) },
            })
        } else {
            json!({"key": key, "present": false})
        });
    }
    Ok(json!({"fields": field_plans, "uploads": uploads}))
}

fn revision(request: &Value) -> Result<Value, String> {
    Ok(json!(format!(
        "{}:{}",
        text(&request["version"]),
        text(&request["entry_schema_version"]),
    )))
}

fn draft_admission(request: &Value) -> Result<Value, String> {
    let current = request["form_revision"]
        .as_str()
        .ok_or("Form revision is required")?;
    let draft = request["draft_revision"].as_str();
    Ok(json!({
        "restore": draft == Some(current),
        "incompatible": draft != Some(current),
    }))
}

fn submission(request: &Value) -> Result<Value, String> {
    let revision = request["form_revision"]
        .as_str()
        .filter(|revision| !revision.is_empty())
        .ok_or("Form revision is required")?;
    let field_result = super::fields::evaluate(&json!({
        "fields": request["fields"],
        "values": request["values"],
    }))?;
    let location = &request["location"];
    let detailed_location = if request["entry_document_schema"]["capture_detailed_location"] == true
        && location.is_object()
    {
        let mut detail = Map::new();
        for (source, target) in [
            ("latitude", "device_lat"),
            ("longitude", "device_lng"),
            ("accuracy", "accuracy_m"),
            ("altitude", "altitude_m"),
            ("speed", "speed_mps"),
        ] {
            if !location[source].is_null() {
                detail.insert(target.into(), location[source].clone());
            }
        }
        Value::Object(detail)
    } else {
        Value::Null
    };
    Ok(json!({
        "form_revision": revision,
        "visible_keys": field_result["visible_keys"],
        "errors": field_result["errors"],
        "detailed_location": detailed_location,
        "connectivity": request["connectivity"],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_values_drops_browser_bytes_and_keeps_only_resolved_media_paths() {
        let result = evaluate(&json!({
            "mode": "draft_values",
            "fields": [{"key":"photo", "type":"photo"}, {"key":"photos", "type":"photos"}],
            "values": {
                "photo": [{BINARY_MARKER: true}],
                "photos": ["existing/path.jpg", {BINARY_MARKER: true}],
                "nested": {"binary": {BINARY_MARKER: true}, "value": "kept"}
            }
        }))
        .unwrap();
        assert_eq!(
            result,
            json!({"photos":["existing/path.jpg"], "nested":{"value":"kept"}})
        );
    }

    #[test]
    fn plain_draft_value_keeps_null_but_removes_binary_only_objects() {
        assert_eq!(
            evaluate(&json!({"mode":"plain_draft_value", "value": null})).unwrap(),
            json!({"present":true, "value":null})
        );
        assert_eq!(
            evaluate(&json!({"mode":"plain_draft_value", "value": {BINARY_MARKER:true}})).unwrap(),
            json!({"present":false})
        );
    }

    #[test]
    fn media_plan_normalizes_names_and_omits_empty_fields() {
        let result = evaluate(&json!({
            "mode": "media_plan",
            "project_id": "project", "form_slug": "poles", "document_id": "doc", "now_ms": 8,
            "fields": [{"key":"photo", "type":"photo"}, {"key":"photos", "type":"photos"}],
            "media": {
                "photo": {"existing_paths": [], "files": [{"token":"one", "identity":"id-1", "name":"../Pole photo.JPG"}]},
                "photos": {"existing_paths": [], "files": []}
            }
        })).unwrap();
        assert_eq!(
            result["fields"],
            json!([
                {"key":"photo", "present":true, "value":"project/forms/poles/doc/photo-id-1-Pole_photo.jpg"},
                {"key":"photos", "present":false}
            ])
        );
        assert_eq!(result["uploads"][0]["photo"], true);
    }

    #[test]
    fn revision_and_draft_admission_are_exact() {
        let revision =
            evaluate(&json!({"mode":"revision", "version":2, "entry_schema_version":"v2"}))
                .unwrap();
        assert_eq!(revision, json!("2:v2"));
        assert_eq!(
            evaluate(
                &json!({"mode":"draft_admission", "form_revision":"2:v2", "draft_revision":"2:v2"})
            )
            .unwrap()["restore"],
            true
        );
        assert_eq!(
            evaluate(
                &json!({"mode":"draft_admission", "form_revision":"2:v2", "draft_revision":"1:v1"})
            )
            .unwrap()["incompatible"],
            true
        );
    }

    #[test]
    fn submission_uses_one_pinned_form_revision_for_visibility_validation_and_location() {
        let result = evaluate(&json!({
            "mode":"submission", "form_revision":"2:v2",
            "fields":[
                {"key":"count","type":"number","label":"Count","required":true,"validation":{"min":1}},
                {"key":"hidden","type":"text","visibility":{"conditions":[{"field":"show","operator":"equals","value":"yes"}]}}
            ],
            "values":{"count":0,"show":"no","hidden":"not submitted"},
            "entry_document_schema":{"capture_detailed_location":true},
            "location":{"latitude":-1.2,"longitude":30.1,"accuracy":4},
            "connectivity":{"precedentDocId":"node-1"}
        })).unwrap();
        assert_eq!(result["form_revision"], "2:v2");
        assert_eq!(result["visible_keys"], json!(["count"]));
        assert_eq!(result["errors"]["count"], "Minimum is 1");
        assert_eq!(result["detailed_location"]["device_lat"], -1.2);
        assert_eq!(result["connectivity"]["precedentDocId"], "node-1");
    }
}
