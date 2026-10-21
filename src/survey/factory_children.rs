//! Compound child authoring, governed by the held backend type registry.
//! The window forwards events; this owner admits defaults, types, values and
//! order, then answers a patch for the existing Form Editor document owner.
use serde_json::{json, Value};
use std::collections::BTreeSet;

const INVALID: &str = "form_factory_children_invalid";
const MAX: usize = super::super::MAX_SURVEY_FIELDS;

fn flag(value: &Value, canonical: &str, alias: &str) -> bool {
    value.get(canonical).or_else(|| value.get(alias)) == Some(&Value::Bool(true))
}
fn kind(value: &Value) -> &str {
    value["key"].as_str().unwrap_or("")
}
fn label(value: &Value) -> &str {
    value
        .get("display_name")
        .or_else(|| value.get("displayName"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| kind(value))
}
fn registry(request: &Value) -> Result<Vec<&Value>, String> {
    let rows = request["registry"]
        .as_array()
        .filter(|rows| rows.len() <= MAX)
        .ok_or("The held field-type registry is unavailable or exceeds its bound.")?;
    let mut seen = BTreeSet::new();
    for row in rows {
        let key = kind(row);
        if key.is_empty()
            || key.len() > 100
            || key.chars().any(char::is_control)
            || !seen.insert(key)
        {
            return Err(
                "The held field-type registry contains an invalid or duplicate type.".into(),
            );
        }
    }
    Ok(rows
        .iter()
        .filter(|row| flag(row, "allowed_as_child", "allowedAsChild"))
        .collect())
}
fn admitted<'a>(registry: &[&'a Value], selected: &str) -> Result<&'a Value, String> {
    registry.iter().copied().find(|row|kind(row)==selected).ok_or_else(||format!("Field type `{selected}` is unavailable for compound children in the held backend registry."))
}
fn index(intent: &Value, key: &str, length: usize) -> Result<usize, String> {
    intent[key]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n < length)
        .ok_or_else(|| {
            "The child field order changed; use the current child fields and retry.".into()
        })
}
fn created(registry: &[&Value], selected: &str, id: &str) -> Result<Value, String> {
    let metadata = admitted(registry, selected)?;
    super::create_field(
        &json!({"kind":selected,"id":id,"metadata":{"type":selected,"label":label(metadata)}}),
    )
}
fn nonce(value: &Value) -> Result<String, String> {
    let raw = value
        .as_str()
        .ok_or("A fresh child identity is required.")?;
    let id: String = raw
        .chars()
        .filter(|c| *c != '-')
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("A fresh child identity is required.".into());
    }
    Ok(format!("f_{id}"))
}
fn list(text: &str, separator: char) -> Vec<&str> {
    text.split(separator)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect()
}
fn view(field: &Value, children: &[Value], registry: &[&Value], patch: Value) -> Value {
    let rows:Vec<Value>=children.iter().enumerate().map(|(index,child)|{
        let metadata=registry.iter().find(|row|kind(row)==child["type"].as_str().unwrap_or(""));
        json!({"field":child,"can_move_up":index>0,"can_move_down":index+1<children.len(),
            "allowed":metadata.is_some(),"show_options":metadata.is_some_and(|row|flag(row,"needs_options","needsOptions"))})
    }).collect();
    let choices: Vec<Value> = registry
        .iter()
        .map(|row| json!({"value":kind(row),"label":label(row)}))
        .collect();
    json!({"is_kv":field["type"]=="kv","rows":rows,"types":choices,"can_add":field["type"]=="compound"&&!registry.is_empty()&&children.len()<MAX,
        "kv_keys_text":field["kvPredefinedKeys"].as_array().map(|keys|keys.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")).unwrap_or_default(),
        "patch":patch,"refusal":null})
}
fn apply(request: &Value) -> Result<Value, String> {
    let field = request["field"]
        .as_object()
        .ok_or("The child editor needs its current parent field.")?;
    let parent = field.get("type").and_then(Value::as_str).unwrap_or("");
    if !matches!(parent, "compound" | "kv") {
        return Err("The child editor belongs to a compound or key-value field.".into());
    }
    let registry = registry(request)?;
    let mut children = match field.get("children") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .filter(|rows| rows.len() <= MAX)
            .cloned()
            .ok_or("A compound field holds at most 512 children.")?,
    };
    if children.iter().any(|child| !child.is_object()) {
        return Err("A held child field is not an object.".into());
    }
    let intent = &request["intent"];
    let action = intent["action"].as_str().unwrap_or("view");
    if action == "view" {
        return Ok(view(&request["field"], &children, &registry, Value::Null));
    }
    if parent == "kv" {
        let patch = match action {
            "kv_toggle" => {
                json!({"kvUsePredefinedKeys":intent["value"].as_bool().ok_or("Choose whether keys are predefined.")?})
            }
            "kv_keys" => {
                json!({"kvPredefinedKeys":list(intent["text"].as_str().ok_or("Predefined keys must be text.")?,'\n')})
            }
            _ => return Err("Key-value fields do not author compound children.".into()),
        };
        return Ok(view(&request["field"], &children, &registry, patch));
    }
    match action {
        "add" => {
            if children.len() >= MAX {
                return Err("A compound field holds at most 512 children.".into());
            }
            let selected = registry
                .first()
                .map(|row| kind(row))
                .ok_or("No child type is available in the held backend registry.")?;
            let id = nonce(&intent["nonce"])?;
            if children.iter().any(|child| child["id"] == id) {
                return Err("The new child identity is already present.".into());
            }
            children.push(created(&registry, selected, &id)?);
        }
        "remove" => {
            let selected = index(intent, "index", children.len())?;
            children.remove(selected);
        }
        "move" => {
            let from = index(intent, "from", children.len())?;
            let to = index(intent, "to", children.len())?;
            let child = children.remove(from);
            children.insert(to, child);
        }
        "update" => {
            let selected = index(intent, "index", children.len())?;
            let patch = intent["patch"]
                .as_object()
                .ok_or("A child edit needs its changed fields.")?;
            if patch
                .keys()
                .any(|key| !matches!(key.as_str(), "label" | "key" | "type" | "required"))
            {
                return Err("Unknown child field or a change to its stable identity.".into());
            }
            let child = children[selected].as_object_mut().unwrap();
            for (key, value) in patch {
                let value = match key.as_str() {
                    "required" => json!(value
                        .as_bool()
                        .ok_or("A child requirement is a checkbox.")?),
                    "key" => json!(value
                        .as_str()
                        .ok_or("A child key is text.")?
                        .chars()
                        .map(|c| if c.is_ascii_alphanumeric() || c == '_' {
                            c.to_ascii_lowercase()
                        } else {
                            '_'
                        })
                        .collect::<String>()),
                    "type" => {
                        let selected = value.as_str().ok_or("A child type must be named.")?;
                        admitted(&registry, selected)?;
                        json!(selected)
                    }
                    _ => json!(value.as_str().ok_or("A child label is text.")?),
                };
                child.insert(key.clone(), value);
            }
            if patch.contains_key("type") {
                let defaults = created(
                    &registry,
                    child["type"].as_str().unwrap(),
                    child["id"]
                        .as_str()
                        .ok_or("The child has no stable identity.")?,
                )?;
                for (key, value) in defaults.as_object().unwrap() {
                    child.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
        }
        "options" => {
            let selected = index(intent, "index", children.len())?;
            let metadata = admitted(&registry, children[selected]["type"].as_str().unwrap_or(""))?;
            if !flag(metadata, "needs_options", "needsOptions") {
                return Err("This child type does not author options.".into());
            }
            children[selected]["options"] = json!(list(
                intent["text"]
                    .as_str()
                    .ok_or("Child options must be text.")?,
                ','
            ));
        }
        _ => return Err("Unknown compound child action.".into()),
    }
    Ok(view(
        &request["field"],
        &children,
        &registry,
        json!({"children":children}),
    ))
}

pub(super) fn evaluate(request: &Value) -> Value {
    apply(request).unwrap_or_else(|message|json!({"is_kv":request["field"]["type"]=="kv","rows":[],"types":[],"can_add":false,"kv_keys_text":"","patch":null,
        "refusal":{"code":INVALID,"message":message}}))
}

#[cfg(test)]
#[path = "factory_children_tests.rs"]
mod tests;
