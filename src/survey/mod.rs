//! Offline survey semantics shared by native hosts and WASM. Hosts own IO only.
mod entry;
mod fields;
pub mod project_forms;

use serde_json::{json, Value};
pub const MAX_SURVEY_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_SURVEY_FIELDS: usize = 512;

pub fn survey_evaluate(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > MAX_SURVEY_INPUT_BYTES {
        return Err("Survey request exceeds 1 MiB".into());
    }
    let request: Value =
        serde_json::from_slice(bytes).map_err(|_| "Survey request must be JSON")?;
    match request["operation"].as_str() {
        Some("fields") => fields::evaluate(&request),
        Some("encode") => entry::encode(&request),
        Some("geometry") => entry::geometry(&request),
        Some("distance") => {
            let a = entry::point(&request["a"])?;
            let b = entry::point(&request["b"])?;
            Ok(json!(ds_geo_ops::haversine_m(a.0, a.1, b.0, b.1)))
        }
        Some("length") => Ok(json!(entry::length(&request["geometry"])?)),
        Some("project_plan") => {
            let changes: Vec<project_forms::ProjectFormChange> =
                serde_json::from_value(request["changes"].clone())
                    .map_err(|_| "Invalid settings changes")?;
            project_forms::plan(
                request["project"].as_str().ok_or("Project is required")?,
                &request["payload"],
                &changes,
            )
        }
        _ => Err("Unknown survey operation".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn field_visibility_validation_and_hidden_values_share_one_pass() {
        let result = survey_evaluate(&serde_json::to_vec(&json!({"operation":"fields", "fields":[
            {"key":"n","type":"number","label":"Count","required":true,"validation":{"min":1}},
            {"key":"secret","type":"text","visibility":{"conditions":[{"field":"mode","operator":"equals","value":"show"}]}}
        ],"values":{"n":0,"mode":"hide","secret":"stale"}})).unwrap()).unwrap();
        assert_eq!(result["visible_keys"], json!(["n"]));
        assert_eq!(result["data"], json!({"n":0}));
        assert_eq!(result["errors"]["n"], "Minimum is 1");
    }
    #[test]
    fn malformed_pattern_and_nonfinite_numeric_input_are_refused() {
        let result = fields::evaluate(&json!({"fields":[{"key":"n","type":"number"},{"key":"s","type":"text","validation":{"pattern":"["}}],"values":{"n":"Infinity","s":"x"}})).unwrap();
        assert_eq!(result["errors"]["n"], "Must be a number");
        assert_eq!(
            result["errors"]["s"],
            "Invalid validation pattern in form schema"
        );
    }
    #[test]
    fn huge_form_refuses_instead_of_blocking_the_renderer() {
        let rows = vec![json!({"key":"x"}); MAX_SURVEY_FIELDS + 1];
        assert!(fields::evaluate(&json!({"fields":rows,"values":{}})).is_err());
    }
}
