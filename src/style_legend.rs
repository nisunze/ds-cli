//! Category rows shared by the screen legend and physical print pens.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_ROWS: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Category {
    pub value: String,
    pub color: String,
}

fn field(lookup: &Value) -> Option<&str> {
    match lookup.get(0)?.as_str()? {
        "get" => lookup.get(1)?.as_str(),
        "coalesce" => field(lookup.get(1)?),
        _ => None,
    }
}

fn match_rows(expression: &Value) -> Option<(Option<&str>, &Value, Vec<(String, &Value)>)> {
    let items = expression.as_array()?;
    if items.first()?.as_str()? != "match" || items.len() < 5 || items.len() % 2 == 0 {
        return None;
    }
    let mut rows = Vec::new();
    for pair in items[2..items.len() - 1].chunks_exact(2) {
        let labels = pair[0]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_else(|| std::slice::from_ref(&pair[0]));
        for label in labels {
            let value = match label {
                Value::String(s) => s.clone(),
                Value::Number(n) => n
                    .as_f64()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| n.to_string()),
                _ => continue,
            };
            rows.push((value, &pair[1]));
        }
    }
    Some((field(&items[1]), items.last()?, rows))
}

/// One row per scalar label, including each member of a grouped match arm.
pub fn categories(expression: &Value) -> (Option<String>, Option<String>, Vec<Category>) {
    let Some((field, fallback, rows)) = match_rows(expression) else {
        return (None, None, Vec::new());
    };
    (
        field.map(str::to_owned),
        fallback.as_str().map(str::to_owned),
        rows.into_iter()
            .filter_map(|(value, output)| {
                Some(Category {
                    value,
                    color: output.as_str()?.into(),
                })
            })
            .take(MAX_ROWS)
            .collect(),
    )
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct Scope {
    present_values: BTreeMap<String, Vec<String>>,
    labels: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    style: Value,
    #[serde(default)]
    scope: Scope,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    8
}
fn compact(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}
fn halo(property: &str) -> bool {
    property.ends_with("-stroke-color")
        || property.ends_with("-halo-color")
        || property == "fill-outline-color"
}
fn size(property: &str) -> bool {
    property.ends_with("-width") || property.ends_with("-radius") || property == "icon-size"
}
fn kind(property: &str) -> Option<&'static str> {
    if property == "icon-image" {
        Some("icon")
    } else if halo(property) {
        Some("opacity")
    } else if property.ends_with("-color") {
        Some("color")
    } else if property.ends_with("-opacity") || size(property) {
        Some("opacity")
    } else {
        None
    }
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > crate::style_authoring::MAX_STYLE_REQUEST_BYTES {
        return Err("legend request exceeds 2 MiB".into());
    }
    let request: Request = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    if request.limit == 0 || request.limit > MAX_ROWS {
        return Err("legend row limit must be 1 through 48".into());
    }
    let matches: Vec<_> = ["paint", "layout"]
        .into_iter()
        .flat_map(|bucket| request.style[bucket].as_object().into_iter().flatten())
        .filter_map(|(property, expression)| Some((property, match_rows(expression)?)))
        .collect();
    let primary: BTreeSet<_> = matches
        .iter()
        .filter(|(property, _)| kind(property) == Some("color"))
        .filter_map(|(_, (field, _, _))| *field)
        .collect();
    let mut dimensions: Vec<Value> = Vec::new();
    let mut positions = BTreeMap::new();
    let mut entries: Vec<BTreeMap<String, usize>> = Vec::new();
    for (property, (field, _, rows)) in matches {
        let Some(field) = field.filter(|f| !f.is_empty()) else {
            continue;
        };
        let channel = if halo(property) && primary.contains(field) {
            Some("color")
        } else {
            kind(property)
        };
        let Some(channel) = channel else { continue };
        let key = (field.to_owned(), channel);
        let index = *positions.entry(key).or_insert_with(|| {
            let index = dimensions.len();
            dimensions.push(
                json!({"field": field, "kind": channel, "property": property, "entries": []}),
            );
            entries.push(BTreeMap::new());
            index
        });
        let present = request
            .scope
            .present_values
            .get(field)
            .map(|v| v.iter().map(|s| compact(s)).collect::<BTreeSet<_>>());
        if present.is_some() {
            dimensions[index]["presenceScoped"] = json!(true);
        }
        for (value, output) in rows {
            if present
                .as_ref()
                .is_some_and(|p| !p.contains(&compact(&value)))
            {
                continue;
            }
            let table = dimensions[index]["entries"]
                .as_array_mut()
                .expect("entries");
            let entry_index = match entries[index].get(&value) {
                Some(index) => *index,
                None if table.len() >= request.limit => continue,
                None => {
                    let next_entry = table.len();
                    let mut entry = json!({"value": value});
                    if let Some(label) = request
                        .scope
                        .labels
                        .get(field)
                        .and_then(|l| l.get(&value))
                        .filter(|s| !s.is_empty())
                    {
                        entry["label"] = json!(label);
                    }
                    table.push(entry);
                    entries[index].insert(value, next_entry);
                    next_entry
                }
            };
            let entry = &mut table[entry_index];
            if channel == "color" && output.is_string() {
                entry["color"] = output.clone();
            }
            if channel == "icon" && output.is_string() {
                entry["iconName"] = output.clone();
            }
            if channel == "opacity" && halo(property) && output.is_string() {
                entry["haloColor"] = output.clone();
            }
            if channel == "opacity" && property.ends_with("-opacity") && output.is_number() {
                entry["opacity"] = output.clone();
                entry["hollow"] = json!(output.as_f64().unwrap_or(1.) <= 0.);
            }
            if channel == "opacity" && size(property) && output.is_number() {
                entry["width"] = output.clone();
            }
        }
    }
    dimensions.retain(|d| d["entries"].as_array().is_some_and(|e| !e.is_empty()));
    let answer = if dimensions.is_empty() {
        Value::Null
    } else {
        json!({"kind": "categorical", "geom": request.style["type"], "dimensions": dimensions})
    };
    serde_json::to_string(&answer).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grouped_labels_are_shared_by_screen_and_print() {
        let expression = json!([
            "match",
            ["coalesce", ["get", "conductor"], ["get", "wire"]],
            ["3x35", "3x50"],
            "#FF0000",
            "#888888"
        ]);
        let (field, fallback, print) = categories(&expression);
        assert_eq!(field.as_deref(), Some("conductor"));
        assert_eq!(fallback.as_deref(), Some("#888888"));
        assert_eq!(
            print.iter().map(|r| r.value.as_str()).collect::<Vec<_>>(),
            ["3x35", "3x50"]
        );
        let request = json!({"style": {"type": "line", "paint": {"line-color": expression}}, "scope": {"presentValues": {"conductor": ["3 X 50"]}, "labels": {"conductor": {"3x50": "50 mm²"}}}});
        let screen: Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap())
                .unwrap();
        assert_eq!(
            screen["dimensions"][0]["entries"],
            json!([{"value": "3x50", "color": "#FF0000", "label": "50 mm²"}])
        );
    }
    #[test]
    fn numeric_groups_and_malformed_matches() {
        assert_eq!(
            categories(&json!(["match", ["get", "grade"], [1, 2], "red", "black"]))
                .2
                .len(),
            2
        );
        assert!(categories(&json!(["match", ["get", "grade"], "black"]))
            .2
            .is_empty());
        assert!(evaluate(br#"{"style":{},"limit":49}"#).is_err());
    }

    #[test]
    fn multiple_rows_and_same_field_halos_merge_without_crossing_dimensions() {
        let request = json!({"style":{"type":"circle", "paint":{
            "circle-color":["match", ["get", "kind"], ["a", "b"], "red", "black"],
            "circle-opacity":["match", ["get", "state"], [0., 1.], 0, 2, 1, 1],
            "circle-stroke-color":["match", ["get", "kind"], ["a", "b"], "red", "black"],
            "circle-stroke-width":["match", ["get", "state"], [0, 1], 2.5, 2, 1, 0]
        }}});
        let answer: Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap())
                .unwrap();
        assert_eq!(answer["dimensions"].as_array().unwrap().len(), 2);
        assert_eq!(
            answer["dimensions"][0]["entries"].as_array().unwrap().len(),
            2
        );
        assert_eq!(
            answer["dimensions"][1]["entries"][1],
            json!({"value":"1", "opacity":0, "hollow":true, "width":2.5})
        );
    }
}
