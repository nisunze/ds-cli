#[path = "support/table_owner.rs"]
mod table_owner;
use ds_command_kernel::selection_field::{
    decode_selection_field_profile, SelectionFieldError, SelectionFieldQuery,
};
use serde_json::{json, Map, Value};
use table_owner::selection_field_info;

fn rows(values: &[Value]) -> Vec<Map<String, Value>> {
    values
        .iter()
        .map(|value| json!({"cable_size": value}).as_object().unwrap().clone())
        .collect()
}
fn query() -> SelectionFieldQuery {
    SelectionFieldQuery {
        column: "cable_size".into(),
    }
}

#[test]
fn caller_handler_response_decoder_keeps_values_tokens_and_numeric_range() {
    let query = query();
    let result =
        selection_field_info(&query, rows(&[json!("70; 95"), json!("70"), Value::Null])).unwrap();
    let decoded =
        decode_selection_field_profile(&query, &serde_json::to_vec(&result).unwrap()).unwrap();
    assert_eq!(decoded, result);
    assert_eq!(decoded.stats.total_rows, 3);
    assert_eq!(decoded.stats.non_empty_count, 2);
    assert_eq!(decoded.token_values, ["70", "95"]);
    let result =
        selection_field_info(&query, rows(&[json!(-3), json!(12.5), Value::Null])).unwrap();
    assert_eq!(
        (result.stats.numeric_min, result.stats.numeric_max),
        (Some(-3.0), Some(12.5))
    );
}

#[test]
fn malformed_requests_and_incompatible_responses_fail_at_the_owner() {
    assert!(serde_json::from_value::<SelectionFieldQuery>(json!({"column": 3})).is_err());
    assert!(matches!(
        selection_field_info(&SelectionFieldQuery { column: " ".into() }, vec![]),
        Err(SelectionFieldError::InvalidRequest(_))
    ));
    let result = selection_field_info(&query(), rows(&[json!(70)])).unwrap();
    let original = serde_json::to_value(result).unwrap();
    for (key, value) in [
        ("kind", json!("unrecognized")),
        ("total_rows", json!("three")),
        ("column", json!("another")),
        ("non_empty_count", json!(99)),
    ] {
        let mut incompatible = original.clone();
        incompatible[key] = value;
        assert!(
            matches!(
                decode_selection_field_profile(
                    &query(),
                    &serde_json::to_vec(&incompatible).unwrap()
                ),
                Err(SelectionFieldError::InvalidResponse(_))
            ),
            "{key}"
        );
    }
    let mut incompatible = original;
    incompatible.as_object_mut().unwrap().remove("values");
    assert!(
        decode_selection_field_profile(&query(), &serde_json::to_vec(&incompatible).unwrap())
            .is_err()
    );
}

#[test]
fn compatible_additions_do_not_require_equal_releases() {
    let query: SelectionFieldQuery =
        serde_json::from_value(json!({"column": "cable_size", "future_option": true})).unwrap();
    let result = selection_field_info(&query, rows(&[json!(70)])).unwrap();
    let mut extended = serde_json::to_value(&result).unwrap();
    extended["future_hint"] = json!({"value": 123});
    assert_eq!(
        decode_selection_field_profile(&query, &serde_json::to_vec(&extended).unwrap()).unwrap(),
        result
    );
}
