use arrow_array::{RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use ds_columnar::ipc::{batch_to_ipc, ipc_to_rows};
use serde_json::json;
use std::sync::Arc;

#[test]
fn standard_utf8_strings_keep_the_browser_callers_values_and_nulls() {
    let schema = Arc::new(Schema::new(vec![Field::new(
        "cable_size",
        DataType::Utf8,
        true,
    )]));
    let batch = RecordBatch::try_new(
        schema,
        vec![Arc::new(StringArray::from(vec![
            Some("70; 95"),
            None,
            Some("ABC 120"),
        ]))],
    )
    .unwrap();
    let rows = ipc_to_rows(&batch_to_ipc(&batch).unwrap()).unwrap();
    assert_eq!(rows[0]["cable_size"], json!("70; 95"));
    assert_eq!(rows[1]["cable_size"], json!(null));
    assert_eq!(rows[2]["cable_size"], json!("ABC 120"));
}

#[test]
fn malformed_ipc_is_refused_without_a_partial_result() {
    assert!(
        ipc_to_rows(b"not an Arrow file")
            .unwrap_err()
            .contains("could not open the Arrow IPC file")
    );
}

#[test]
fn unsupported_populated_properties_are_refused_instead_of_becoming_null() {
    use arrow_array::Int32Array;
    let schema = Arc::new(Schema::new(vec![Field::new(
        "cable_size",
        DataType::Int32,
        false,
    )]));
    let batch = RecordBatch::try_new(schema, vec![Arc::new(Int32Array::from(vec![70]))]).unwrap();
    assert!(
        ipc_to_rows(&batch_to_ipc(&batch).unwrap())
            .unwrap_err()
            .contains("column cable_size: unsupported Arrow property type")
    );
}

#[test]
fn non_finite_numbers_are_refused_instead_of_becoming_null() {
    use arrow_array::Float64Array;
    let schema = Arc::new(Schema::new(vec![Field::new(
        "length",
        DataType::Float64,
        false,
    )]));
    let batch = RecordBatch::try_new(
        schema,
        vec![Arc::new(Float64Array::from(vec![1.0, f64::NAN]))],
    )
    .unwrap();
    assert!(
        ipc_to_rows(&batch_to_ipc(&batch).unwrap())
            .unwrap_err()
            .contains("column length: non-finite numeric property")
    );
}

#[test]
fn duplicate_column_names_are_refused_without_choosing_a_value() {
    use arrow_array::Float64Array;
    let schema = Arc::new(Schema::new(vec![
        Field::new("length", DataType::Float64, false),
        Field::new("length", DataType::Float64, false),
    ]));
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Float64Array::from(vec![1.0])),
            Arc::new(Float64Array::from(vec![2.0])),
        ],
    )
    .unwrap();
    assert!(
        ipc_to_rows(&batch_to_ipc(&batch).unwrap())
            .unwrap_err()
            .contains("ambiguous duplicate Arrow column length")
    );
}
