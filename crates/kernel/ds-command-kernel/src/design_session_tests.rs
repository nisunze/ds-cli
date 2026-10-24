use super::*;

#[test]
fn session_row_windows_are_the_held_rows_as_typed_json() {
    let geometry = serde_json::from_value(serde_json::json!({
        "type": "Point",
        "coordinates": [30.0, -2.0, 1540.5]
    }))
    .unwrap();
    let properties = serde_json::from_value(serde_json::json!({
        "name": "Pole 1",
        "voltage": 400,
        "height_m": 9.5,
        "note": null
    }))
    .unwrap();
    let window = ds_design_session::dataframe::RowWindow {
        layer_name: "poles".to_string(),
        offset: 3,
        limit: 10,
        total_rows: 4,
        rows: vec![ds_design_session::dataframe::LayerRow {
            row_index: 3,
            feature_id: "pole-1".to_string(),
            geometry,
            properties,
        }],
    };

    let text = row_window_json(&window).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["layer_name"], "poles");
    assert_eq!(
        (value["offset"].as_u64(), value["total_rows"].as_u64()),
        (Some(3), Some(4))
    );
    let row = &value["rows"][0];
    assert_eq!(
        (row["row_index"].as_u64(), row["feature_id"].as_str()),
        (Some(3), Some("pole-1"))
    );
    assert_eq!(
        row["geometry"]["coordinates"],
        serde_json::json!([30.0, -2.0, 1540.5])
    );
    // Property values and their order are the session's own, nulls included.
    let keys: Vec<&String> = row["properties"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["name", "voltage", "height_m", "note"]);
    assert_eq!(row["properties"]["voltage"], 400);
    assert!(row["properties"]["note"].is_null());
}
