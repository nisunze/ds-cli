//! Form Factory authoring and project-settings view semantics.
use serde_json::{json, Map, Value};
pub(super) fn evaluate(request: &Value) -> Result<Value, String> {
    match request["mode"].as_str() {
        Some("document") => document(request),
        Some("editor_state") => editor_state(request),
        Some("draft_admission") => draft_admission(request),
        Some("save_result") => save_result(request),
        Some("public") => public(request),
        _ => Err("Unknown form factory operation".into()),
    }
}

fn public(request: &Value) -> Result<Value, String> {
    let form = request["form"]
        .as_object()
        .ok_or("Form must be an object")?;
    let metadata = form.get("metadata").and_then(Value::as_object);
    Ok(json!(
        form.get("is_public") == Some(&Value::Bool(true))
            || form.get("visibility").and_then(Value::as_str) == Some("public")
            || metadata.is_some_and(|metadata| {
                metadata.get("is_public") == Some(&Value::Bool(true))
                    || metadata.get("visibility").and_then(Value::as_str) == Some("public")
                    || metadata.get("access").and_then(Value::as_str) == Some("public")
                    || metadata.get("access_mode").and_then(Value::as_str) == Some("public")
            })
    ))
}

fn editor_state(request: &Value) -> Result<Value, String> {
    let document = document(request)?;
    let snapshot = document_from_value(&request["snapshot"])?;
    Ok(json!({"document": document, "dirty": document != snapshot}))
}

fn document_from_value(value: &Value) -> Result<Value, String> {
    let fields = value["fields"]
        .as_array()
        .ok_or("Form fields must be an array")?;
    if fields.len() > super::MAX_SURVEY_FIELDS {
        return Err("Survey form exceeds 512 fields".into());
    }
    Ok(json!({
        "fields": fields,
        "display_name": value["display_name"].as_str().unwrap_or(""),
        "description": value["description"].as_str().unwrap_or(""),
        "capture_detailed_location": value["capture_detailed_location"] == true,
    }))
}

fn draft_admission(request: &Value) -> Result<Value, String> {
    let form_version = request["form_version"]
        .as_u64()
        .ok_or("Form version is required")?;
    let draft_version = request["draft_version"]
        .as_u64()
        .ok_or("Draft version is required")?;
    Ok(json!({
        "restore": true,
        "stale": draft_version != form_version,
    }))
}

fn save_result(request: &Value) -> Result<Value, String> {
    let saved = document_from_value(&request["saved"])?;
    let current = document_from_value(&request["current"])?;
    let version = request["version"]
        .as_u64()
        .ok_or("Form version is required")?;
    Ok(json!({
        "document": current,
        "snapshot": saved,
        "version": version,
        "draft_action": if current == saved { "clear" } else { "save" },
    }))
}

/// The durable portion of an authoring session. Browser state (expanded cards,
/// loading flags, and IDB transport) stays in the host; this is the exact
/// document whose revision and dirty snapshot are compared and published.
fn document(request: &Value) -> Result<Value, String> {
    let fields = request["fields"]
        .as_array()
        .ok_or("Form fields must be an array")?;
    if fields.len() > super::MAX_SURVEY_FIELDS {
        return Err("Survey form exceeds 512 fields".into());
    }
    Ok(json!({
        "fields": fields,
        "display_name": request["display_name"].as_str().unwrap_or(""),
        "description": request["description"].as_str().unwrap_or(""),
        "capture_detailed_location": request["capture_detailed_location"] == true,
    }))
}

pub(super) fn default_values(fields: &Value) -> Result<Value, String> {
    let fields = fields.as_array().ok_or("Form fields must be an array")?;
    if fields.len() > super::MAX_SURVEY_FIELDS {
        return Err("Survey form exceeds 512 fields".into());
    }
    let mut values = Map::new();
    for field in fields {
        if let Some(value) = field
            .get("defaultValue")
            .filter(|v| !v.is_null() && v.as_str() != Some(""))
        {
            values.insert(
                field["key"].as_str().ok_or("Field key missing")?.into(),
                value.clone(),
            );
        }
    }
    Ok(Value::Object(values))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_keeps_only_publishable_authoring_state() {
        let document = evaluate(&json!({
            "mode": "document",
            "fields": [{"key": "material"}],
            "display_name": "Pole",
            "description": "As built",
            "capture_detailed_location": true,
            "expandedFieldId": "ignored",
        }))
        .unwrap();
        assert_eq!(
            document,
            json!({
                "fields": [{"key": "material"}],
                "display_name": "Pole",
                "description": "As built",
                "capture_detailed_location": true,
            })
        );
    }

    #[test]
    fn editor_decisions_keep_a_stale_draft_and_save_post_request_edits() {
        let current = json!({"fields": [], "display_name":"Pole", "description":"changed", "capture_detailed_location":false});
        let saved = json!({"fields": [], "display_name":"Pole", "description":"saved", "capture_detailed_location":false});
        let admission =
            evaluate(&json!({"mode":"draft_admission", "form_version":8, "draft_version":7}))
                .unwrap();
        assert_eq!(admission, json!({"restore":true, "stale":true}));
        let result =
            evaluate(&json!({"mode":"save_result", "saved":saved, "current":current, "version":8}))
                .unwrap();
        assert_eq!(result["draft_action"], "save");
        assert_eq!(result["snapshot"]["description"], "saved");
    }

    #[test]
    fn public_visibility_uses_the_backend_compatibility_mirrors() {
        assert_eq!(
            evaluate(&json!({"mode":"public", "form":{"metadata":{"access_mode":"public"}}}))
                .unwrap(),
            json!(true)
        );
        assert_eq!(
            evaluate(&json!({"mode":"public", "form":{"visibility":"private"}})).unwrap(),
            json!(false)
        );
    }
}
pub(super) fn create_field(request: &Value) -> Result<Value, String> {
    let kind = request["kind"].as_str().ok_or("Field type missing")?;
    let id = request["id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 100)
        .ok_or("Field identity missing")?;
    let metadata = &request["metadata"];
    if metadata["type"] != kind || !metadata["label"].is_string() {
        return Err("Field type is unavailable in the cached backend registry".into());
    }
    let mut field = json!({"id":id,"key":format!("{kind}_{id}"),"label":metadata["label"],"type":kind,"required":false});
    match kind {
        "dropdown" | "radio" | "multiselect" | "radioGrid" | "likertScale" | "ranking" => {
            field["options"] = json!(["Option 1", "Option 2"]);
            if kind == "radio" {
                field["radioLayout"] = json!("vertical");
            }
        }
        "rating" => field["options"] = json!(["1", "2", "3", "4", "5"]),
        "photos" | "documents" | "videos" | "audios" => {
            field["minCount"] = json!(0);
            field["maxCount"] = json!(5);
        }
        "compound" => field["children"] = json!([]),
        "kv" => {
            field["kvUsePredefinedKeys"] = json!(false);
            field["kvPredefinedKeys"] = json!([]);
        }
        "separator" => {
            field["separatorThickness"] = json!(1);
            field["separatorColor"] = json!("#e2e8f0");
        }
        _ => {}
    }
    Ok(field)
}
fn truth(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(n) => n.as_f64() != Some(0.),
        Value::String(s) => !matches!(s.trim().to_lowercase().as_str(), "" | "false" | "0"),
        _ => true,
    }
}
fn text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}
pub(super) fn condition(raw: &Value, values: &Value) -> bool {
    let raw = raw.as_str().unwrap_or("").trim();
    if raw.is_empty() {
        return true;
    }
    if let Some((key, expected)) = raw.split_once('=') {
        return text(&values[key]) == expected;
    }
    truth(&values[raw])
}
fn visible(field: &Value, state: &Value, values: &Value) -> bool {
    if state["applicable"] == false {
        return false;
    }
    if field["show_when"].as_str().is_some_and(|s| !s.is_empty())
        && !condition(&field["show_when"], values)
    {
        return false;
    }
    if field["hide_when"].as_str().is_some_and(|s| !s.is_empty())
        && condition(&field["hide_when"], values)
    {
        return false;
    }
    if field["show_when"].as_str().unwrap_or("").is_empty()
        && field["hide_when"].as_str().unwrap_or("").is_empty()
        && state["visible"] == false
    {
        return false;
    }
    true
}
pub(super) fn settings(request: &Value) -> Result<Value, String> {
    let field = &request["field"];
    let state = &request["state"];
    let values = &request["values"];
    match request["mode"].as_str() {
        Some("condition") => Ok(json!(condition(&request["condition"], values))),
        Some("visible") => Ok(json!(visible(field, state, values))),
        Some("required") => Ok(json!(if field["required_when"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        {
            condition(&field["required_when"], values)
        } else {
            state["required"] == true
        })),
        Some("state") => {
            let mut states = Map::new();
            for section in request["sections"]
                .as_array()
                .ok_or("Settings sections missing")?
            {
                for field in section["fields"]
                    .as_array()
                    .ok_or("Settings fields missing")?
                {
                    let key = field["key"].as_str().ok_or("Settings key missing")?;
                    let state = &request["fieldState"][key];
                    let required = if field["required_when"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty())
                    {
                        condition(&field["required_when"], values)
                    } else {
                        state["required"] == true
                    };
                    states.insert(
                        key.into(),
                        json!({"visible":visible(field,state,values),"required":required}),
                    );
                }
            }
            Ok(Value::Object(states))
        }
        Some("payload") => {
            let mut payload = Map::new();
            let sections = request["sections"]
                .as_array()
                .ok_or("Settings sections missing")?;
            for section in sections {
                for field in section["fields"]
                    .as_array()
                    .ok_or("Settings fields missing")?
                {
                    let key = field["key"].as_str().ok_or("Settings key missing")?;
                    if visible(field, &request["fieldState"][key], values)
                        && values.get(key).is_some()
                        && values[key] != request["persisted"][key]
                    {
                        payload.insert(key.into(), values[key].clone());
                    }
                }
            }
            Ok(Value::Object(payload))
        }
        _ => Err("Unknown settings operation".into()),
    }
}
