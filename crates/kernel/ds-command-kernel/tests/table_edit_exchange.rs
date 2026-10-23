#[path = "support/table_owner.rs"]
mod table_owner;
use ds_command_kernel::table_edit::{
    decode_request, decode_response, TableEditRequest, TableEditResponse,
};
use ds_geo::table::{
    AddColumnInput, CalculateFieldInput, CalculateGeometryPropertiesInput, CalculateOperation,
    ReplaceInput, ReplaceOptions, ReplaceScope, TableDataType,
};
use serde_json::{json, Map, Value};
use table_owner::execute;
fn rows() -> Vec<Map<String, Value>> {
    serde_json::from_value(
        json!([{"name":"Pole-1","notes":"pole note"},{"name":"Pole-2","notes":"fine"}]),
    )
    .unwrap()
}
fn requests() -> Vec<TableEditRequest> {
    let replace = ReplaceInput {
        scope: ReplaceScope {
            indices: vec![0, 1],
            rows: rows(),
            columns: vec!["name".into(), "notes".into()],
        },
        options: ReplaceOptions {
            find: "pole".into(),
            replace: "PL".into(),
            case_sensitive: false,
            whole_cell: false,
        },
    };
    vec![
        TableEditRequest::CountReplace(replace.clone()),
        TableEditRequest::CollectReplace(replace),
        TableEditRequest::AddColumn(AddColumnInput {
            rows: rows(),
            column: "phase".into(),
            data_type: TableDataType::Int,
            default_value: json!("-2.5"),
        }),
        TableEditRequest::CalculateField(CalculateFieldInput {
            rows: rows(),
            indices: vec![1],
            geometries: vec![],
            target_column: "phase".into(),
            data_type: TableDataType::Int,
            operation: CalculateOperation::Constant {
                value: json!("3.5"),
            },
        }),
        TableEditRequest::GeometryProperties(CalculateGeometryPropertiesInput {
            rows: vec![Map::new()],
            indices: vec![0],
            geometries: vec![json!({"type":"Point","coordinates":[30,-2]})],
            coordinate_decimals: 2,
            measurement_decimals: 2,
            columns: Default::default(),
        }),
    ]
}
#[test]
fn typed_native_constructor_real_handler_and_owning_decoder_cover_every_operation() {
    for request in requests() {
        let before = serde_json::to_value(&request).unwrap();
        let wire = serde_json::to_vec(&request).unwrap();
        let admitted = decode_request(&wire).unwrap();
        let response = execute(&admitted).unwrap();
        let decoded = decode_response(&request, &serde_json::to_vec(&response).unwrap()).unwrap();
        assert_eq!(decoded, response);
        assert_eq!(serde_json::to_value(&request).unwrap(), before);
        match decoded {
            TableEditResponse::CountReplace(reply) => assert_eq!(reply.count, 3),
            TableEditResponse::CollectReplace(reply) => assert_eq!(reply.updates.len(), 3),
            TableEditResponse::AddColumn(reply) => {
                assert_eq!(reply.default_value, json!(-3));
                assert!(reply
                    .plan
                    .updates
                    .iter()
                    .all(|update| update.new_value == json!(-3)));
            }
            TableEditResponse::CalculateField(reply) => {
                assert_eq!(reply.updates[0].new_value, json!(4))
            }
            TableEditResponse::GeometryProperties(reply) => assert_eq!(reply.updates.len(), 2),
        }
    }
}
#[test]
fn invalid_requests_refuse_before_any_state_change_including_nonfinite_integer_text_and_overflow() {
    for bad in [
        json!({}),
        json!({"operation":"unknown","input":{}}),
        json!({"operation":"calculate_field","input":{"rows":[{}],"indices":[99],"target_column":"x","data_type":"int","operation":{"kind":"constant","value":1}}}),
    ] {
        assert!(decode_request(&serde_json::to_vec(&bad).unwrap())
            .unwrap_err()
            .to_string()
            .contains("table_edit_request_invalid"));
    }
    for value in ["NaN", "Infinity", "1e40", "not-a-number"] {
        let request = TableEditRequest::AddColumn(AddColumnInput {
            rows: rows(),
            column: "phase".into(),
            data_type: TableDataType::Int,
            default_value: json!(value),
        });
        assert!(
            execute(&request)
                .unwrap_err()
                .to_string()
                .contains("table_edit_request_invalid"),
            "{value}"
        );
    }
    let request = TableEditRequest::CalculateField(CalculateFieldInput {
        rows: rows(),
        indices: vec![0, 1],
        geometries: vec![],
        target_column: "n".into(),
        data_type: TableDataType::Int,
        operation: CalculateOperation::RowNumber {
            start: i64::MAX,
            step: 1,
        },
    });
    assert!(execute(&request)
        .unwrap_err()
        .to_string()
        .contains("row_number sequence exceeds int64 range"));
}
#[test]
fn incompatible_operation_scope_values_and_required_metadata_refuse() {
    let request = requests().remove(2);
    let reply = serde_json::to_value(execute(&request).unwrap()).unwrap();
    for field in [
        "operation",
        "row",
        "column",
        "old",
        "new",
        "default",
        "missing",
    ] {
        let mut bad = reply.clone();
        match field {
            "operation" => bad["operation"] = json!("calculate_field"),
            "row" => bad["result"]["plan"]["updates"][0]["row_index"] = json!(99),
            "column" => bad["result"]["plan"]["column"] = json!("other"),
            "old" => bad["result"]["plan"]["updates"][0]["old_value"] = json!("different"),
            "new" => bad["result"]["plan"]["updates"][0]["new_value"] = json!("not-an-int"),
            "default" => bad["result"]["default_value"] = json!("-2.5"),
            _ => {
                bad["result"]
                    .as_object_mut()
                    .unwrap()
                    .remove("default_value");
            }
        };
        assert!(
            decode_response(&request, &serde_json::to_vec(&bad).unwrap())
                .unwrap_err()
                .to_string()
                .contains("table_edit_response_invalid"),
            "{field}"
        );
    }
}
#[test]
fn compatible_additions_preserve_the_actual_exchange_and_duplicate_selected_indices() {
    for request in requests() {
        let mut wire = serde_json::to_value(&request).unwrap();
        wire["future_input"] = json!(true);
        let admitted = decode_request(&serde_json::to_vec(&wire).unwrap()).unwrap();
        let response = execute(&admitted).unwrap();
        let mut wire = serde_json::to_value(&response).unwrap();
        wire["future_response"] = json!("compatible");
        assert_eq!(
            decode_response(&request, &serde_json::to_vec(&wire).unwrap()).unwrap(),
            response
        );
    }
    let mut request = requests().remove(3);
    if let TableEditRequest::CalculateField(input) = &mut request {
        input.indices = vec![1, 1];
    }
    let response = execute(&request).unwrap();
    assert!(decode_response(&request, &serde_json::to_vec(&response).unwrap()).is_ok());
}

#[test]
fn data_revision_checks_compare_owned_values_and_geometry_before_application() {
    use ds_command_kernel::table_edit::{validate_snapshot, TableEditSnapshot};
    let mut request = requests().remove(3);
    let current = if let TableEditRequest::CalculateField(input) = &request {
        TableEditSnapshot {
            rows: input.rows.clone(),
            geometries: input.geometries.clone(),
        }
    } else {
        unreachable!()
    };
    validate_snapshot(&request, &current).unwrap();
    let mut changed = current.clone();
    changed.rows[0].insert("name".into(), json!("changed elsewhere"));
    assert!(validate_snapshot(&request, &changed)
        .unwrap_err()
        .to_string()
        .contains("table_edit_stale"));
    if let TableEditRequest::CalculateField(input) = &mut request {
        input.geometries = vec![json!({"type":"Point","coordinates":[30,-2]})];
    }
    assert!(validate_snapshot(&request, &current).is_err());
}
