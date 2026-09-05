//! Form Factory authoring and project-settings view semantics.
use serde_json::{json, Map, Value};
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
