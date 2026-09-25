//! Offline survey semantics shared by native hosts and WASM. Hosts own IO only.
mod access;
mod bridge;
mod draft;
mod entry;
mod factory;
mod fields;
mod fill;
pub mod hold;
mod mutation;
mod policy;
pub mod project_forms;
mod read;

/// Project-to-project survey-data migration's one receipt, for native hosts.
pub use bridge::{migration_receipt, MIGRATION_RECEIPT_ROWS};

use serde_json::Value;
pub const MAX_SURVEY_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_SURVEY_FIELDS: usize = 512;

pub fn survey_evaluate(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > 3 * MAX_SURVEY_INPUT_BYTES {
        return Err("Survey request exceeds 3 MiB".into());
    }
    let request: Value =
        serde_json::from_slice(bytes).map_err(|_| "Survey request must be JSON")?;
    // Mutations carry one prior entry plus one bounded delta. Neither crosses
    // the 900 KiB wire-entry bound, but their combined request can exceed 1 MiB.
    if bytes.len() > MAX_SURVEY_INPUT_BYTES && request["operation"] != "mutation" {
        return Err("Survey request exceeds 1 MiB".into());
    }
    match request["operation"].as_str() {
        Some("fill") => fill::evaluate(&request),
        Some("factory") => factory::evaluate(&request),
        Some("bridge") => bridge::evaluate(&request),
        Some("access") => access::evaluate(&request),
        Some("read") => read::evaluate(&request),
        Some("mutation") => mutation::evaluate(&request),
        Some("policy") => policy::evaluate(&request),
        Some("draft") => draft::evaluate(&request),
        Some("fields") => fields::evaluate(&request),
        Some("defaults") => factory::default_values(&request["fields"]),
        Some("new_field") => factory::create_field(&request),
        Some("settings") => factory::settings(&request),
        Some("project_forms") => project_forms::evaluate(&request),
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
    use serde_json::json;
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
    fn media_counts_bind_singular_and_plural_fields_and_an_empty_list_is_missing() {
        let field = |kind: &str, extra: Value| {
            let mut f = json!({"key":kind,"label":"Photo","type":kind});
            f.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            f
        };
        let result = fields::evaluate(&json!({"fields":[
            field("photo", json!({"required":true})),
            field("photos", json!({"required":true})),
            field("video", json!({"minCount":2})),
            field("videos", json!({"maxCount":1})),
            field("document", json!({"maxCount":1})),
        ],"values":{"photo":[],"photos":[],"video":["a"],"videos":["a","b"],"document":"a"}}))
        .unwrap();
        assert_eq!(result["errors"]["photo"], "Photo is required");
        assert_eq!(result["errors"]["photos"], "Photo is required");
        assert_eq!(result["errors"]["video"], "At least 2 required");
        assert_eq!(result["errors"]["videos"], "Maximum 1 allowed");
        assert!(result["errors"]["document"].is_null());
    }
    #[test]
    fn a_bounded_update_can_carry_its_prior_entry_without_halving_the_entry_limit() {
        let request = json!({"operation":"mutation","mode":"mutate","payload":{"project_id":"p","form_id":"f","doc_id":"d","operation":"update","updated_data":{"value":"n".repeat(600_000)}},"prior":{"type":"Feature","properties":{"data":{"value":"o".repeat(600_000)}}},"grants":{"entries.edit_all":true},"mutation_id":"key","now":"2026-09-11T00:00:00Z"});
        let bytes = serde_json::to_vec(&request).unwrap();
        assert!(bytes.len() > MAX_SURVEY_INPUT_BYTES);
        assert_eq!(
            survey_evaluate(&bytes).unwrap()["payload"]["updated_data"]["value"]
                .as_str()
                .unwrap()
                .len(),
            600_000
        );
    }
    #[test]
    fn huge_form_refuses_instead_of_blocking_the_renderer() {
        let rows = vec![json!({"key":"x"}); MAX_SURVEY_FIELDS + 1];
        assert!(fields::evaluate(&json!({"fields":rows,"values":{}})).is_err());
    }
}
