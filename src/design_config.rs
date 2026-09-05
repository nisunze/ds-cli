//! Bounded inspection of prepared project settings, shared by UI and CLI.
//! Never selects a replacement engineering rule or invents a missing catalog.
use serde_json::{json, Value};
use std::collections::BTreeSet;
pub fn inspect(input: &[u8]) -> Result<String, String> {
    if input.len() > 16 * 1024 * 1024 {
        return Err("project config exceeds inspection bound".into());
    }
    let config: Value = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    if config.is_null() {
        return Ok(json!({"cached":false}).to_string());
    }
    let sheets = config.get("sheets").unwrap_or(&config);
    let selected = sheets["project_settings"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["parameter"] == "lv_poles_rule_set")
        })
        .map(|row| row["value"].clone())
        .unwrap_or(Value::Null);
    let available: Vec<_> = sheets["lv_poles_rules"]
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default();
    let entry = selected
        .as_str()
        .and_then(|name| sheets["lv_poles_rules"].get(name));
    let rows = entry.and_then(|v| v.get("table").unwrap_or(v).as_array());
    let codes: BTreeSet<_> = rows
        .into_iter()
        .flatten()
        .filter_map(|row| row["assembly_type"].as_str())
        .collect();
    let assemblies = sheets["lv_poles_assembly"].as_array();
    let columns: BTreeSet<_> = assemblies
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .flat_map(|row| row.keys())
        .collect();
    let bounded = |values: Vec<String>| -> Value {
        json!({"total":values.len(),"values":values.iter().take(30).map(|s|s.chars().take(120).collect::<String>()).collect::<Vec<_>>(),"truncated":values.len()>30 || values.iter().any(|s|s.chars().count()>120)})
    };
    Ok(json!({"cached":true,"selected_rule_set":selected,"selected_rule_found":entry.is_some(),
        "rule_sets":bounded(available),"selected_rule_row_count":rows.map_or(0,Vec::len),
        "selected_assembly_types":bounded(codes.into_iter().map(str::to_owned).collect()),
        "assembly_row_count":assemblies.map_or(0,Vec::len),"assembly_clean_names":bounded(assemblies.into_iter().flatten().filter_map(|row|row["clean_name"].as_str()).map(str::to_owned).collect()),"assembly_columns":bounded(columns.into_iter().cloned().collect()),
        "assembly_examples":assemblies.into_iter().flatten().take(3).map(|row| {
            let mut out=serde_json::Map::new();
            if let Some(row)=row.as_object() { for (key,value) in row.iter().take(12) { out.insert(key.chars().take(120).collect(), Value::String(value.as_str().map(str::to_owned).unwrap_or_else(||value.to_string()).chars().take(120).collect())); } }
            Value::Object(out)
        }).collect::<Vec<_>>()}).to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_rule_is_exact_and_empty_catalog_is_visible() {
        let config = json!({"sheets":{"project_settings":[{"parameter":"lv_poles_rule_set","value":"Tchad_rule"}],"lv_poles_rules":{"rule_tchad":{"table":[{"assembly_type":"TD1"}]}},"lv_poles_assembly":[]}});
        let result: Value =
            serde_json::from_str(&inspect(config.to_string().as_bytes()).unwrap()).unwrap();
        assert_eq!(result["selected_rule_found"], false);
        assert_eq!(result["rule_sets"]["values"], json!(["rule_tchad"]));
        assert_eq!(result["assembly_row_count"], 0);
    }
}
