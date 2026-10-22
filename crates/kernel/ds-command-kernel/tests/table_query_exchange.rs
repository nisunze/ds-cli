use ds_command_kernel::table_query::{
    TableFilterMode, TableFilterQuery, TableFilterResult, TableProfilesQuery, TableProfilesResult,
    filter_rows, profile_rows, validate_filter_result, validate_profiles_result,
};
use ds_geo::table::{ColumnFilterInput, FilterOp};
use serde_json::{Map, Value, json};

fn rows() -> Vec<Map<String, Value>> {
    [
        json!({"code":"A;B","class":"poles","n":3}),
        json!({"code":"B","class":"lines","n":1}),
        json!({"code":null,"class":"poles","n":2}),
    ]
    .into_iter()
    .map(|row| row.as_object().unwrap().clone())
    .collect()
}

#[test]
fn native_typed_caller_and_handler_share_membership_and_row_order() {
    let request = TableFilterQuery {
        filters: vec![ColumnFilterInput {
            column: "code".into(),
            op: FilterOp::TokenAll,
            value: "a,b".into(),
            values: None,
            value2: None,
        }],
        ..Default::default()
    };
    let decoded_request = serde_json::from_slice(&serde_json::to_vec(&request).unwrap()).unwrap();
    let response = filter_rows(&decoded_request, rows()).unwrap();
    let decoded_response = serde_json::from_slice(&serde_json::to_vec(&response).unwrap()).unwrap();
    assert_eq!(
        validate_filter_result(3, decoded_response).unwrap().indices,
        [0]
    );
    let request: TableFilterQuery = serde_json::from_value(json!({"groups":[[{"column":"code","op":"token_all","value":"a,b"}],[{"column":"class","op":"equals","value":"poles"}]], "mode":"any", "sort":{"column":"n","direction":"desc"}})).unwrap();
    assert!(matches!(request.mode, TableFilterMode::Any));
    assert_eq!(filter_rows(&request, rows()).unwrap().indices, [0, 2]);
}

#[test]
fn profiles_keep_existing_kind_authority_and_full_range_past_the_value_cap() {
    let request: TableProfilesQuery = serde_json::from_value(
        json!({"columns":["n","flag","text","missing"],"options":{"value_cap":1}}),
    )
    .unwrap();
    let rows = [
        json!({"n":1,"flag":true,"text":"42"}),
        json!({"n":2.5,"flag":false,"text":"43"}),
    ]
    .into_iter()
    .map(|row| row.as_object().unwrap().clone())
    .collect();
    let response = profile_rows(&request, rows).unwrap();
    let response: TableProfilesResult =
        serde_json::from_slice(&serde_json::to_vec(&response).unwrap()).unwrap();
    let response = validate_profiles_result(&request, 2, response).unwrap();
    assert_eq!(
        (
            response.stats[0].stats.numeric_min,
            response.stats[0].stats.numeric_max
        ),
        (Some(1.0), Some(2.5))
    );
    assert!(response.stats[0].stats.capped);
    let result = serde_json::to_value(response).unwrap();
    assert_eq!(result["stats"][0]["kind"], "number");
    assert_eq!(result["stats"][1]["kind"], "boolean");
    assert_eq!(result["stats"][2]["kind"], "text");
    assert_eq!(result["stats"][3]["kind"], "null");
}

#[test]
fn invalid_operations_and_incompatible_indices_are_refused() {
    assert!(serde_json::from_value::<TableFilterQuery>(json!({"mode":"invented"})).is_err());
    assert!(
        serde_json::from_value::<TableFilterQuery>(
            json!({"filters":[{"column":"n","op":"invented"}]})
        )
        .is_err()
    );
    for response in [
        TableFilterResult {
            indices: vec![3],
            total_rows: 3,
        },
        TableFilterResult {
            indices: vec![0, 0],
            total_rows: 3,
        },
        TableFilterResult {
            indices: vec![0],
            total_rows: 99,
        },
    ] {
        assert!(validate_filter_result(3, response).is_err());
    }
    let query: TableFilterQuery = serde_json::from_value(
        json!({"filters":[{"column":"n","op":"gt","value":"not a number"}]}),
    )
    .unwrap();
    assert!(filter_rows(&query, rows()).is_err());
}

#[test]
fn compatible_additions_do_not_change_the_filter_exchange() {
    let request: TableFilterQuery = serde_json::from_value(json!({"future":true})).unwrap();
    let result = filter_rows(&request, rows()).unwrap();
    let mut addition = serde_json::to_value(&result).unwrap();
    addition["future"] = json!("hint");
    let result = serde_json::from_value(addition).unwrap();
    assert_eq!(
        validate_filter_result(3, result).unwrap().indices,
        [0, 1, 2]
    );
}
