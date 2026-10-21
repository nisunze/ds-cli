//! Browser graph spelling is an input format, not a second set of plan rules.
//! Opaque geometry/properties are retained byte-for-value, including model links.
use serde_json::{Map, Value};

pub(crate) fn snake_fields(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(snake_fields).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    let name: String = key
                        .chars()
                        .flat_map(|c| {
                            if c.is_ascii_uppercase() {
                                vec!['_', c.to_ascii_lowercase()]
                            } else {
                                vec![c]
                            }
                        })
                        .collect();
                    let value =
                        if matches!(key.as_str(), "geometry" | "properties" | "serverFields") {
                            value.clone()
                        } else {
                            snake_fields(value)
                        };
                    (name, value)
                })
                .collect::<Map<_, _>>(),
        ),
        _ => value.clone(),
    }
}

pub(crate) fn graph(
    value: &Value,
    project: &str,
    format: &str,
) -> Result<super::CanonicalProjectGraph, String> {
    match format {
        "wire" => Ok(super::decode_project_graph(value, project)),
        "canonical" => {
            let mut wire = snake_fields(value);
            if let Some(tasks) = wire.get_mut("tasks").and_then(Value::as_array_mut) {
                for task in tasks {
                    let metadata = serde_json::json!({
                        "created_at": task.get("created_at"), "created_by": task.get("created_by"),
                        "updated_at": task.get("updated_at"), "updated_by": task.get("updated_by"),
                    });
                    let mut data = task
                        .get("server_fields")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    if let Some(fields) = task.as_object() {
                        data.extend(
                            fields
                                .iter()
                                .filter(|(key, _)| key.as_str() != "server_fields")
                                .map(|(key, value)| (key.clone(), value.clone())),
                        );
                    }
                    *task = serde_json::json!({"id": task.get("id"), "data": data, "metadata": metadata});
                }
            }
            let graph = super::wire::decode_canonical_graph(&wire, project);
            let source_count = wire
                .get("tasks")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            if graph.tasks.len() != source_count {
                return Err("Canonical Project Work graph has invalid task identities.".into());
            }
            Ok(graph)
        }
        other => Err(format!("Unsupported Project Work graph format `{other}`.")),
    }
}
pub(crate) fn wire_format() -> String {
    "wire".into()
}
