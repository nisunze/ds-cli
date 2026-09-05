//! Bounded projection of backend-authored canonical layers for native and WASM hosts.
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub const MAX_LAYER_CATALOG_BYTES: usize = 32 * 1024 * 1024;

pub fn project_layer_catalog(input: &[u8], limit: usize) -> Result<String, String> {
    if input.len() > MAX_LAYER_CATALOG_BYTES || !(1..=500).contains(&limit) {
        return Err("layer catalogue exceeds its input or result bound".into());
    }
    let state: Value = serde_json::from_slice(input).map_err(|_| "invalid layer catalogue JSON")?;
    let layers = state["layers"]
        .as_array()
        .ok_or("missing layer catalogue")?;
    if layers.len() > 50_000 {
        return Err("too many runtime layers".into());
    }
    let mut groups: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    let mut order = Vec::new();
    let mut runtime_ids = HashSet::new();
    for layer in layers {
        let runtime = layer["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("missing runtime layer id")?;
        if !runtime_ids.insert(runtime) {
            return Err("duplicate runtime layer id".into());
        }
        let Some(id) = layer["metadata"]["config_layer_id"]
            .as_str()
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        if !groups.contains_key(id) {
            order.push(id);
        }
        groups.entry(id).or_default().push(layer);
    }
    let rows: Vec<Value> = order.iter().take(limit).map(|id| {
        let family = &groups[id];
        let primary = family.iter().find(|layer| layer["metadata"]["parent_layer"].as_str().is_none_or(str::is_empty)).unwrap_or(&family[0]);
        let metadata = &primary["metadata"];
        let text = |key| metadata[key].as_str().filter(|v| !v.is_empty());
        json!({
            "id": id,
            "label": text("label").unwrap_or(id),
            "class": text("layer_class").or_else(|| text("layer_type")).unwrap_or("unknown"),
            "geometry": text("geometry_type").or_else(|| primary["type"].as_str()),
            "order": metadata["order"].as_i64().unwrap_or(0),
            "runtimeIds": family.iter().filter_map(|layer| layer["id"].as_str()).collect::<Vec<_>>(),
            "styleRef": primary["style_ref"],
        })
    }).collect();
    serde_json::to_string(
        &json!({"layer_count": order.len(), "layers": rows, "more": order.len() > limit}),
    )
    .map_err(|_| "cannot encode layer catalogue".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn groups_companions_by_canonical_id_preserves_order_and_reports_truncation() {
        let input = json!({"layers": [
            {"id":"label", "type":"symbol", "metadata":{"config_layer_id":"roads","parent_layer":"road"}},
            {"id":"road", "type":"line", "metadata":{"config_layer_id":"roads","label":"Roads","order":4}},
            {"id":"p", "type":"circle", "metadata":{"config_layer_id":"poles"}},
            {"id":"notes", "type":"circle"}
        ]});
        let result: Value = serde_json::from_str(
            &project_layer_catalog(&serde_json::to_vec(&input).unwrap(), 1).unwrap(),
        )
        .unwrap();
        assert_eq!(result["layers"][0]["label"], "Roads");
        assert_eq!(result["layers"][0]["runtimeIds"], json!(["label", "road"]));
        assert_eq!(result["layer_count"], 2);
        assert_eq!(result["more"], true);
        assert!(result["layers"][0].get("visible").is_none());
        assert!(project_layer_catalog(b"{\"layers\":[{\"id\":\"a\"},{\"id\":\"a\"}]}", 1).is_err());
    }
}
