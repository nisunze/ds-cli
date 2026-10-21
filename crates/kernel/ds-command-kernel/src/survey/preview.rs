//! Read-only form-rule preview. The runtime field evaluator is the authority;
//! this typed boundary never acquires a form, creates an entry or grants access.
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const SCHEMA: &str = "ds.survey-form-validation/v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    form_revision: String,
    fields: Vec<Value>,
    values: Map<String, Value>,
}

/// Validate the supplied revision's runtime rules, without any host IO.
/// The byte digest identifies the exact reviewed input, not a cloud revision.
pub fn validate(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > super::MAX_SURVEY_INPUT_BYTES {
        return Err("Survey validation document exceeds 1 MiB".into());
    }
    let request: Request = serde_json::from_slice(bytes)
        .map_err(|error| format!("Invalid Survey validation document: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!("Expected {SCHEMA}"));
    }
    if request.form_revision.trim() != request.form_revision
        || request.form_revision.is_empty()
        || request.form_revision.len() > 160
        || request.form_revision.chars().any(char::is_control)
    {
        return Err("form_revision must be a nonempty trimmed label of at most 160 bytes".into());
    }
    let result = super::fields::evaluate(&json!({
        "fields": request.fields,
        "values": request.values,
    }))?;
    let visible: BTreeSet<&str> = result["visible_keys"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let hidden = request
        .fields
        .iter()
        .filter(|field| !super::fields::is_display_only(field))
        .filter_map(|field| {
            let key = field["key"].as_str()?;
            (!visible.contains(key)).then_some(key)
        })
        .collect::<Vec<_>>();
    let required_visible = request
        .fields
        .iter()
        .filter(|field| !super::fields::is_display_only(field))
        .filter_map(|field| {
            let key = field["key"].as_str()?;
            (visible.contains(key) && field["required"] == true).then_some(key)
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "schema": "ds.survey-form-validation-result/v1",
        "form_revision": request.form_revision,
        "input_sha256": format!("{:x}", Sha256::digest(bytes)),
        "valid": result["errors"].as_object().unwrap().is_empty(),
        "visible_keys": result["visible_keys"],
        "hidden_keys": hidden,
        "required_visible_keys": required_visible,
        "errors": result["errors"],
        "effects": "none",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(values: Value) -> Value {
        json!({"schema":SCHEMA,"form_revision":"draft:4","fields":[
            {"key":"category","label":"Category","type":"radio","options":["Residential","Productive","Other"],"required":true},
            {"key":"other_detail","label":"Other detail","type":"text","required":true,
             "visibility":{"conditions":[{"field":"category","operator":"equals","value":"Other"}]}},
            {"key":"count","label":"Count","type":"number","required":true,"validation":{"min":0}}
        ],"values":values})
    }

    #[test]
    fn preview_proves_visible_hidden_other_and_valid_rules_with_submission_parity() {
        for (values, expected_errors, hidden) in [
            (
                json!({"count":0}),
                json!({"category":"Category is required"}),
                json!(["other_detail"]),
            ),
            (
                json!({"category":"Residential","count":0}),
                json!({}),
                json!(["other_detail"]),
            ),
            (
                json!({"category":"Other","count":0}),
                json!({"other_detail":"Other detail is required"}),
                json!([]),
            ),
            (
                json!({"category":"Other","other_detail":"Clinic","count":0}),
                json!({}),
                json!([]),
            ),
        ] {
            let request = document(values);
            let bytes = serde_json::to_vec(&request).unwrap();
            let original = bytes.clone();
            let preview = validate(&bytes).unwrap();
            let submission = super::super::fill::evaluate(&json!({
                "mode":"submission","form_revision":request["form_revision"],
                "fields":request["fields"],"values":request["values"]
            }))
            .unwrap();
            assert_eq!(preview["errors"], expected_errors);
            assert_eq!(preview["hidden_keys"], hidden);
            assert_eq!(
                preview["valid"],
                expected_errors.as_object().unwrap().is_empty()
            );
            assert_eq!(preview["errors"], submission["errors"]);
            assert_eq!(preview["visible_keys"], submission["visible_keys"]);
            assert_eq!(preview["effects"], "none");
            assert!(preview.get("data").is_none());
            assert_eq!(bytes, original);
        }
    }

    /// b7ae5c73: a choice value outside the field's options is not valid,
    /// and the error names the field and the allowed values.
    #[test]
    fn a_choice_outside_the_declared_options_is_invalid_and_names_them() {
        let request = document(json!({"category":"Industrial","count":0}));
        let preview = validate(&serde_json::to_vec(&request).unwrap()).unwrap();
        assert_eq!(preview["valid"], false);
        assert_eq!(
            preview["errors"],
            json!({"category":"Category must be one of: Residential, Productive, Other"})
        );
        let submission = super::super::fill::evaluate(&json!({
            "mode":"submission","form_revision":request["form_revision"],
            "fields":request["fields"],"values":request["values"]
        }))
        .unwrap();
        assert_eq!(preview["errors"], submission["errors"]);

        let multi = json!({"schema":SCHEMA,"form_revision":"draft:4","fields":[
            {"key":"loads","label":"Loads","type":"multiselect","options":["Lighting","Mill"]},
            {"key":"kind","label":"Kind","type":"dropdown","options":["A","B"]}
        ],"values":{"loads":["Lighting","Welding"],"kind":"B"}});
        let preview = validate(&serde_json::to_vec(&multi).unwrap()).unwrap();
        assert_eq!(
            preview["errors"],
            json!({"loads":"Loads must be one of: Lighting, Mill"})
        );
        let mut accepted = multi.clone();
        accepted["values"]["loads"] = json!(["Mill", "Lighting"]);
        let preview = validate(&serde_json::to_vec(&accepted).unwrap()).unwrap();
        assert_eq!(preview["valid"], true);
    }

    #[test]
    fn exact_document_bound_and_closed_shape_refuse_without_host_access() {
        let mut value = document(json!({}));
        value["operation"] = json!("create");
        assert!(validate(&serde_json::to_vec(&value).unwrap()).is_err());
        value.as_object_mut().unwrap().remove("operation");
        value["fields"][1]["key"] = json!("category");
        assert!(validate(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(validate(&vec![b' '; super::super::MAX_SURVEY_INPUT_BYTES + 1]).is_err());
    }
}
