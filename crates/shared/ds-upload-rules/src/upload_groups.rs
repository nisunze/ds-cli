//! Upload grouping and source stamps, shared by parsed IPC and preview metadata.
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub file_name: String,
    pub layers: Vec<Value>,
}
#[derive(Deserialize)]
pub struct Input {
    pub sources: Vec<Source>,
    pub mode: String,
}

pub fn stamp(feature: &mut Value, file: &str, layer: &str) {
    if !feature["properties"].is_object() {
        feature["properties"] = json!({});
    }
    let properties = feature["properties"].as_object_mut().expect("object");
    for (key, value) in [("_ds_source_file", file), ("_ds_source_layer", layer)] {
        if properties.get(key).is_none_or(Value::is_null) {
            properties.insert(key.into(), json!(value));
        }
    }
}
pub fn clean_name(value: &str) -> String {
    let last = value.rsplit(['/', '\\']).next().unwrap_or(value).trim();
    let stem = last.rsplit_once('.').map_or(last, |(stem, _)| stem);
    let mut result = String::new();
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            result.push(c);
        } else if !result.ends_with('_') {
            result.push('_');
        }
    }
    let result = result.trim_matches('_');
    if result.is_empty() {
        "imported_layer".into()
    } else {
        result.into()
    }
}
fn geometry(layer: &Value) -> String {
    match layer["geometryType"].as_str().unwrap_or("Unknown") {
        "MultiPoint" => "Point",
        "MultiLineString" => "LineString",
        "MultiPolygon" => "Polygon",
        other => other,
    }
    .into()
}
pub fn plan(input: Input) -> Value {
    let mut names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for source in &input.sources {
        for layer in &source.layers {
            names
                .entry(clean_name(layer["name"].as_str().unwrap_or("")))
                .or_default()
                .insert(geometry(layer));
        }
    }
    let mut out: Vec<Value> = Vec::new();
    let mut groups = BTreeMap::<(String, String), usize>::new();
    for (source_index, source) in input.sources.iter().enumerate() {
        for (layer_index, layer) in source.layers.iter().enumerate() {
            let raw_name = layer["name"].as_str().unwrap_or("");
            let clean = clean_name(raw_name);
            let geom = geometry(layer);
            let name = if input.mode == "source" {
                format!(
                    "{}/{}",
                    source
                        .file_name
                        .rsplit_once('.')
                        .map_or(source.file_name.as_str(), |(stem, _)| stem),
                    raw_name
                )
            } else if names[&clean].len() > 1 {
                format!("{clean}_{geom}")
            } else {
                clean.clone()
            };
            let group = if input.mode == "source" {
                None
            } else {
                groups.get(&(clean.clone(), geom.clone())).copied()
            };
            let index = group.unwrap_or_else(|| {
                let index = out.len();
                groups.insert((clean,geom.clone()),index);
                out.push(json!({"name":name,"geometryType":geom,"featureCount":0,"features":[],"columns":[],"members":[],"chunkSources":[],"sourceLayers":[]}));
                index
            });
            let target = &mut out[index];
            target["members"]
                .as_array_mut()
                .unwrap()
                .push(json!([source_index, layer_index]));
            target["featureCount"] = json!(
                target["featureCount"].as_u64().unwrap_or(0)
                    + layer["featureCount"].as_u64().unwrap_or(0)
            );
            if let Some(features) = layer["features"].as_array() {
                for feature in features {
                    let mut f = feature.clone();
                    stamp(&mut f, &source.file_name, raw_name);
                    target["features"].as_array_mut().unwrap().push(f);
                }
            }
            for key in ["columns", "chunkSources"] {
                if let Some(values) = layer[key].as_array() {
                    for value in values {
                        if !target[key].as_array().unwrap().contains(value) {
                            target[key].as_array_mut().unwrap().push(value.clone());
                        }
                    }
                }
            }
            if input.mode != "source" {
                for column in ["_ds_source_file", "_ds_source_layer"] {
                    if !target["columns"]
                        .as_array()
                        .unwrap()
                        .contains(&json!(column))
                    {
                        target["columns"]
                            .as_array_mut()
                            .unwrap()
                            .push(json!(column));
                    }
                }
            }
            let refs = layer["sourceLayers"].as_array().filter(|v| !v.is_empty()).cloned().unwrap_or_else(|| vec![json!({"sourceFileName":layer["sourceFileName"].as_str().unwrap_or(&source.file_name),"sourceLayerName":layer["sourceLayerName"].as_str().unwrap_or(raw_name),"featureCount":layer["featureCount"]})]);
            for (reference_index, mut reference) in refs.into_iter().enumerate() {
                reference["member"] = json!([source_index, layer_index, reference_index]);
                target["sourceLayers"]
                    .as_array_mut()
                    .unwrap()
                    .push(reference);
            }
        }
    }
    json!(out)
}

#[cfg(test)]
#[path = "upload_groups_tests.rs"]
mod tests;
