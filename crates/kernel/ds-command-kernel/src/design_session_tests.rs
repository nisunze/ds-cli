use super::*;

#[test]
fn session_row_windows_use_the_canonical_feature_ipc_schema() {
    let geometry = serde_json::from_value(serde_json::json!({
        "type": "Point",
        "coordinates": [30.0, -2.0]
    }))
    .unwrap();
    let properties = serde_json::from_value(serde_json::json!({
        "name": "Pole 1",
        "voltage": 400
    }))
    .unwrap();
    let window = ds_design_session::dataframe::RowWindow {
        layer_name: "poles".to_string(),
        offset: 0,
        limit: 10,
        total_rows: 1,
        rows: vec![ds_design_session::dataframe::LayerRow {
            row_index: 0,
            feature_id: "pole-1".to_string(),
            geometry,
            properties,
        }],
    };

    let bytes = row_window_to_feature_ipc(window).unwrap();
    let batches = ds_columnar::ipc_to_batches(&bytes).unwrap();
    let features = ds_columnar::batch_to_features(&batches[0], 100).unwrap();

    assert_eq!(features.len(), 1);
    assert_eq!(features[0]["id"], "pole-1");
    assert_eq!(features[0]["geometry"]["type"], "Point");
    assert_eq!(features[0]["properties"]["voltage"], 400);
}
