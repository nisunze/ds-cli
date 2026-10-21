//! Bounded inspection of prepared project settings, shared by UI and CLI.
//! Never selects a replacement engineering rule or invents a missing catalog.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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
    let struct_types: BTreeSet<_> = rows
        .into_iter()
        .flatten()
        .filter_map(|row| row["struct_type"].as_str())
        .collect();
    let material_rows = sheets["lv_poles_struct_type"].as_array();
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
    Ok(json!({"cached":true,"config_sha256":format!("{:x}", Sha256::digest(input)),"selected_rule_set":selected,"selected_rule_found":entry.is_some(),
        "rule_sets":bounded(available),"selected_rule_row_count":rows.map_or(0,Vec::len),
        "selected_assembly_types":bounded(codes.into_iter().map(str::to_owned).collect()),
        "selected_struct_types":bounded(struct_types.into_iter().map(str::to_owned).collect()),
        "struct_type_material_catalog":project_material_rows(material_rows),
        "assembly_row_count":assemblies.map_or(0,Vec::len),"assembly_clean_names":bounded(assemblies.into_iter().flatten().filter_map(|row|row["clean_name"].as_str()).map(str::to_owned).collect()),"assembly_columns":bounded(columns.into_iter().cloned().collect()),
        "assembly_examples":assemblies.into_iter().flatten().take(3).map(|row| {
            let mut out=serde_json::Map::new();
            if let Some(row)=row.as_object() { for (key,value) in row.iter().take(12) { out.insert(key.chars().take(120).collect(), Value::String(value.as_str().map(str::to_owned).unwrap_or_else(||value.to_string()).chars().take(120).collect())); } }
            Value::Object(out)
        }).collect::<Vec<_>>()}).to_string())
}

/// Raw catalog evidence only. Engineering aliases and material resolution stay
/// with ds-network; this projection must not implement a second resolver.
fn project_material_rows(rows: Option<&Vec<Value>>) -> Value {
    let rows = rows.map(Vec::as_slice).unwrap_or_default();
    let mut truncated_values = false;
    let projected: Vec<_> = rows
        .iter()
        .take(30)
        .map(|row| {
            let mut out = serde_json::Map::new();
            for key in [
                "clean_name",
                "struct_type",
                "name",
                "default_material",
                "material",
                "misspelled_names",
            ] {
                if let Some(value) = row.get(key) {
                    let value = match value {
                        Value::String(text) => {
                            truncated_values |= text.chars().count() > 120;
                            Value::String(text.chars().take(120).collect())
                        }
                        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
                        _ => {
                            let text = value.to_string();
                            truncated_values |= text.chars().count() > 120;
                            Value::String(text.chars().take(120).collect())
                        }
                    };
                    out.insert(key.to_owned(), value);
                }
            }
            Value::Object(out)
        })
        .collect();
    json!({"total":rows.len(),"rows":projected,"omitted":rows.len().saturating_sub(30),"truncated_values":truncated_values})
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

    #[test]
    fn material_inspection_preserves_missing_values_and_pins_exact_input() {
        let config = json!({"project_settings":[{"parameter":"lv_poles_rule_set","value":"rule_tchd"}],
            "lv_poles_rules":{"rule_tchd":{"table":[{"struct_type":"9m 150daN"}]}},
            "lv_poles_struct_type":[{"clean_name":"9m 150daN"},{"clean_name":"9m 300daN","default_material":"steel","material":null}]});
        let bytes = config.to_string();
        let result: Value = serde_json::from_str(&inspect(bytes.as_bytes()).unwrap()).unwrap();
        assert_eq!(
            result["selected_struct_types"]["values"],
            json!(["9m 150daN"])
        );
        assert_eq!(
            result["struct_type_material_catalog"]["rows"],
            config["lv_poles_struct_type"]
        );
        assert_eq!(
            result["config_sha256"],
            format!("{:x}", Sha256::digest(bytes.as_bytes()))
        );
        let changed = format!("{bytes} ");
        let other: Value = serde_json::from_str(&inspect(changed.as_bytes()).unwrap()).unwrap();
        assert_ne!(result["config_sha256"], other["config_sha256"]);
    }

    #[test]
    fn material_inspection_reports_omitted_rows_and_clipped_values() {
        let rows = vec![
            json!({"clean_name":"x".repeat(121),"material":"steel","private_extra":"not projected"});
            31
        ];
        let result = project_material_rows(Some(&rows));
        assert_eq!(result["total"], 31);
        assert_eq!(result["omitted"], 1);
        assert_eq!(result["truncated_values"], true);
        assert_eq!(result["rows"].as_array().unwrap().len(), 30);
        assert_eq!(result["rows"][0]["clean_name"].as_str().unwrap().len(), 120);
        assert!(result["rows"][0].get("private_extra").is_none());
    }
}

/// Canonical UI/CLI envelope for the authenticated catalog IO owner. This does
/// not resolve aliases or calculate material; ds-network owns that meaning.
pub fn material_propagation_request(input: &[u8]) -> Result<String, String> {
    #[derive(serde::Deserialize, serde::Serialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        schema: String,
        source_project: String,
        template: String,
        rule_set: String,
        rows: Vec<String>,
        mode: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        expected_digest: String,
    }
    if input.len() > 32 * 1024 {
        return Err("material propagation request exceeds 32 KiB".into());
    }
    let mut request: Request = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    if request.schema != "ds.design.material-propagation/v1"
        || !matches!(request.mode.as_str(), "preview" | "apply")
    {
        return Err("invalid material propagation schema or mode".into());
    }
    for id in [
        &request.source_project,
        &request.template,
        &request.rule_set,
    ] {
        if id.is_empty()
            || id.len() > 128
            || id.trim() != id
            || id.chars().any(|c| c.is_control() || c == '/' || c == '\\')
        {
            return Err(
                "source project, template and rule set must be explicit identifiers".into(),
            );
        }
    }
    let mut seen = BTreeSet::new();
    if request.rows.is_empty()
        || request.rows.len() > 64
        || request.rows.iter().any(|s| {
            s.is_empty()
                || s.len() > 128
                || s.trim() != s
                || s.chars().any(char::is_control)
                || !seen.insert(s.clone())
        })
    {
        return Err("provide 1-64 unique explicit catalog row names".into());
    }
    request.rows.sort();
    if request.mode == "apply" {
        if request.expected_digest.len() != 64
            || !request
                .expected_digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("apply requires the preview digest".into());
        }
    } else if !request.expected_digest.is_empty() {
        return Err("preview cannot carry an apply digest".into());
    }
    serde_json::to_string(&request).map_err(|e| e.to_string())
}

/// The receipt half of the same command: does what came back describe the
/// request that went out?
///
/// The request half has always been Rust; the verdict over the answer was a
/// conditional in the browser's API client, so a headless caller had no way to
/// tell a governed receipt from an unrelated document. Both halves are one
/// decision — a fenced write is only fenced if its acknowledgement is checked
/// against the fence — so they live together.
///
/// `request` is the exact envelope `material_propagation_request` returned.
pub fn material_propagation_receipt(input: &[u8]) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Verdict {
        request: Value,
        receipt: Value,
    }
    if input.len() > 64 * 1024 {
        return Err("material propagation receipt exceeds 64 KiB".into());
    }
    let Verdict { request, receipt } = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    let digest = receipt["digest"].as_str().unwrap_or_default();
    let applied = request["mode"] == "apply";
    let mismatch = if receipt["schema"] != request["schema"] {
        "schema"
    } else if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        "digest"
    } else if receipt["source_project"] != request["source_project"] {
        "source_project"
    } else if receipt["template"] != request["template"] {
        "template"
    } else if receipt["rule_set"] != request["rule_set"] {
        "rule_set"
    } else if receipt["applied"] != Value::Bool(applied) {
        "applied"
    } else if applied && digest != request["expected_digest"].as_str().unwrap_or_default() {
        "expected_digest"
    } else {
        return serde_json::to_string(&json!({"ok": true})).map_err(|e| e.to_string());
    };
    serde_json::to_string(&json!({"ok": false, "mismatch": mismatch})).map_err(|e| e.to_string())
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designMaterialPropagationReceipt)]
pub fn material_propagation_receipt_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    material_propagation_receipt(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designMaterialPropagationRequest)]
pub fn material_propagation_request_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    material_propagation_request(input).map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

#[cfg(test)]
mod material_request_tests {
    use super::*;
    #[test]
    fn country_independent_and_explicit_with_digest_fence() {
        let request = json!({"schema":"ds.design.material-propagation/v1", "source_project":"p", "template":"master", "rule_set":"rule_x", "rows":["pole_z","pole_a"], "mode":"preview"});
        let result: Value = serde_json::from_str(
            &material_propagation_request(request.to_string().as_bytes()).unwrap(),
        )
        .unwrap();
        assert_eq!(result["rule_set"], "rule_x");
        assert_eq!(result["rows"], json!(["pole_a", "pole_z"]));
        let mut apply = request.clone();
        apply["mode"] = json!("apply");
        assert!(material_propagation_request(apply.to_string().as_bytes()).is_err());
        apply["expected_digest"] = json!("a".repeat(64));
        assert!(material_propagation_request(apply.to_string().as_bytes()).is_ok());
        apply["all_projects"] = json!(true);
        assert!(material_propagation_request(apply.to_string().as_bytes()).is_err());
    }

    /// A receipt is a governed acknowledgement only when every fenced member of
    /// the request comes back in it. Each mismatch is named, so a caller can
    /// say WHICH fence the answer broke rather than "invalid receipt".
    #[test]
    fn a_receipt_is_checked_against_its_own_fence() {
        let digest = "a".repeat(64);
        let request = json!({"schema":"ds.design.material-propagation/v1","source_project":"p","template":"master","rule_set":"rule_x","rows":["pole_a"],"mode":"apply","expected_digest":digest});
        let receipt = json!({"schema":"ds.design.material-propagation/v1","source_project":"p","template":"master","rule_set":"rule_x","applied":true,"digest":digest});
        let verdict = |request: &Value, receipt: &Value| -> Value {
            serde_json::from_str(
                &material_propagation_receipt(
                    json!({"request":request,"receipt":receipt})
                        .to_string()
                        .as_bytes(),
                )
                .unwrap(),
            )
            .unwrap()
        };
        assert_eq!(verdict(&request, &receipt)["ok"], true);
        for (member, value) in [
            ("schema", json!("other")),
            ("source_project", json!("q")),
            ("template", json!("other")),
            ("rule_set", json!("other")),
            ("applied", json!(false)),
            ("digest", json!("b".repeat(64))),
        ] {
            let mut broken = receipt.clone();
            broken[member] = value;
            let answer = verdict(&request, &broken);
            assert_eq!(answer["ok"], false, "{member} must break the fence");
            assert!(answer["mismatch"].is_string());
        }
        // A preview must NOT come back applied, and its digest is simply the
        // fence a later apply will echo — it is never compared to one here.
        let mut preview = request.clone();
        preview["mode"] = json!("preview");
        preview["expected_digest"] = json!("");
        let mut answered = receipt.clone();
        answered["applied"] = json!(false);
        answered["digest"] = json!("c".repeat(64));
        assert_eq!(verdict(&preview, &answered)["ok"], true);
        assert_eq!(verdict(&preview, &receipt)["ok"], false);
        // A non-hex digest is refused before any member comparison.
        let mut unhexed = receipt.clone();
        unhexed["digest"] = json!("Z".repeat(64));
        assert_eq!(verdict(&request, &unhexed)["mismatch"], "digest");
    }
}

mod editor;
pub use editor::*;

pub fn canonical_parameter_name(name: &str) -> String {
    let trimmed = name.trim().to_ascii_lowercase();
    let experimental = trimmed.starts_with('_');
    let mut body = String::new();
    let mut pending = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending && !body.is_empty() {
                body.push('_');
            }
            pending = false;
            body.push(ch);
        } else {
            pending = true;
        }
    }
    if experimental && !body.is_empty() {
        format!("_{body}")
    } else {
        body
    }
}
mod projection;
pub use projection::{bounded_page, read_sheet};

mod catalog;
pub use catalog::{alias_map, canonical, catalog_hazards};

mod sheet_copy;
pub use sheet_copy::{copy_sheet, plan_sheet_copy, SheetCopy, SHEET_COPY_SCHEMA};
