use super::*;
use crate::session::FeatureId;
use serde_json::json;

fn pt(id: &str, lng: f64, lat: f64) -> Feature {
    Feature::new(id, Geometry::Point { coords: [lng, lat] })
}

fn pt_with_props(id: &str, lng: f64, lat: f64, props: serde_json::Value) -> Feature {
    Feature::with_properties(
        id,
        Geometry::Point { coords: [lng, lat] },
        props.as_object().cloned().unwrap_or_default(),
    )
}

fn line(id: &str, coords: &[[f64; 2]]) -> Feature {
    Feature::new(
        id,
        Geometry::LineString {
            coords: coords.to_vec(),
        },
    )
}

#[test]
fn layer_table_round_trips_features() {
    let features = vec![
        pt("pole-1", 30.0, -2.0),
        line("line-1", &[[30.0, -2.0], [30.001, -2.001]]),
    ];

    let table = LayerTable::from_features("lv_lines", &features).unwrap();

    assert_eq!(table.layer_name(), "lv_lines");
    assert_eq!(table.height(), 2);
    assert_eq!(table.feature_ids().unwrap(), vec!["pole-1", "line-1"]);
    assert_eq!(table.to_features().unwrap(), features);
}

#[test]
fn empty_layer_table_has_expected_schema() {
    let table = LayerTable::from_features("lv_poles", &[]).unwrap();

    assert_eq!(table.height(), 0);
    assert!(table.dataframe().column(FEATURE_ID_COLUMN).is_ok());
    assert!(table.dataframe().column(GEOMETRY_COLUMN).is_ok());
    assert!(table.dataframe().column(PROPERTIES_COLUMN).is_ok());
}

#[test]
fn get_rows_returns_clamped_window_with_original_indices() {
    let features = vec![
        pt("pole-1", 30.0, -2.0),
        pt("pole-2", 30.1, -2.1),
        pt("pole-3", 30.2, -2.2),
    ];
    let table = LayerTable::from_features("lv_poles", &features).unwrap();

    let window = table.get_rows(1, 10).unwrap();

    assert_eq!(window.layer_name, "lv_poles");
    assert_eq!(window.offset, 1);
    assert_eq!(window.limit, 10);
    assert_eq!(window.total_rows, 3);
    assert_eq!(window.rows.len(), 2);
    assert_eq!(window.rows[0].row_index, 1);
    assert_eq!(window.rows[0].feature_id, "pole-2");
    assert_eq!(window.rows[1].row_index, 2);
    assert_eq!(window.rows[1].feature_id, "pole-3");
}

#[test]
fn get_rows_beyond_end_returns_empty_window() {
    let table = LayerTable::from_features("lv_poles", &[pt("pole-1", 30.0, -2.0)]).unwrap();

    let window = table.get_rows(10, 5).unwrap();

    assert_eq!(window.offset, 10);
    assert_eq!(window.limit, 5);
    assert_eq!(window.total_rows, 1);
    assert!(window.rows.is_empty());
}

#[test]
fn get_row_returns_matching_feature_and_row_index() {
    let features = vec![pt("pole-1", 30.0, -2.0), pt("pole-2", 30.1, -2.1)];
    let table = LayerTable::from_features("lv_poles", &features).unwrap();

    let row = table.get_row("pole-2").unwrap().unwrap();

    assert_eq!(row.row_index, 1);
    assert_eq!(row.feature_id, "pole-2");
    assert_eq!(
        row.geometry,
        Geometry::Point {
            coords: [30.1, -2.1]
        }
    );
    assert!(table.get_row("missing").unwrap().is_none());
}

#[test]
fn rows_include_properties() {
    let features = vec![pt_with_props(
        "pole-1",
        30.0,
        -2.0,
        json!({ "id": "pole-1", "pole_number": "P1", "count": 3 }),
    )];
    let table = LayerTable::from_features("lv_poles", &features).unwrap();

    let row = table.get_row("pole-1").unwrap().unwrap();

    assert_eq!(row.properties.get("pole_number"), Some(&json!("P1")));
    assert_eq!(row.properties.get("count"), Some(&json!(3)));
    assert_eq!(table.to_features().unwrap(), features);
}

#[test]
fn insert_feature_appends_a_row() {
    let mut table = LayerTable::from_features("lv_poles", &[pt("pole-1", 30.0, -2.0)]).unwrap();

    table.insert_feature(pt("pole-2", 30.1, -2.1)).unwrap();

    let window = table.get_rows(0, 10).unwrap();
    assert_eq!(window.total_rows, 2);
    assert_eq!(window.rows[0].feature_id, "pole-1");
    assert_eq!(window.rows[1].feature_id, "pole-2");
    assert_eq!(window.rows[1].row_index, 1);
}

#[test]
fn insert_feature_rejects_duplicate_ids() {
    let mut table = LayerTable::from_features("lv_poles", &[pt("pole-1", 30.0, -2.0)]).unwrap();

    let err = table.insert_feature(pt("pole-1", 30.1, -2.1)).unwrap_err();

    assert!(err.contains("pole-1"));
    let row = table.get_row("pole-1").unwrap().unwrap();
    assert_eq!(
        row.geometry,
        Geometry::Point {
            coords: [30.0, -2.0]
        }
    );
}

#[test]
fn replace_feature_keeps_existing_row_position() {
    let mut table = LayerTable::from_features(
        "lv_poles",
        &[pt("pole-1", 30.0, -2.0), pt("pole-2", 30.1, -2.1)],
    )
    .unwrap();

    let old = table.replace_feature(pt("pole-1", 31.0, -3.0)).unwrap();

    assert_eq!(old.unwrap().id, FeatureId::new("pole-1"));
    let window = table.get_rows(0, 10).unwrap();
    assert_eq!(window.rows[0].feature_id, "pole-1");
    assert_eq!(window.rows[0].row_index, 0);
    assert_eq!(
        window.rows[0].geometry,
        Geometry::Point {
            coords: [31.0, -3.0]
        }
    );
    assert_eq!(window.rows[1].feature_id, "pole-2");
}

#[test]
fn replace_feature_appends_missing_ids() {
    let mut table = LayerTable::from_features("lv_poles", &[pt("pole-1", 30.0, -2.0)]).unwrap();

    let old = table.replace_feature(pt("pole-2", 30.1, -2.1)).unwrap();

    assert!(old.is_none());
    let window = table.get_rows(0, 10).unwrap();
    assert_eq!(window.total_rows, 2);
    assert_eq!(window.rows[1].feature_id, "pole-2");
}

#[test]
fn remove_feature_drops_row_and_preserves_remaining_order() {
    let mut table = LayerTable::from_features(
        "lv_poles",
        &[
            pt("pole-1", 30.0, -2.0),
            pt("pole-2", 30.1, -2.1),
            pt("pole-3", 30.2, -2.2),
        ],
    )
    .unwrap();

    let removed = table.remove_feature("pole-2").unwrap();

    assert_eq!(removed.unwrap().id, FeatureId::new("pole-2"));
    let window = table.get_rows(0, 10).unwrap();
    assert_eq!(window.total_rows, 2);
    assert_eq!(window.rows[0].feature_id, "pole-1");
    assert_eq!(window.rows[1].feature_id, "pole-3");
    assert_eq!(window.rows[1].row_index, 1);
    assert!(table.remove_feature("missing").unwrap().is_none());
}

#[test]
fn edit_cell_updates_properties_and_returns_diff() {
    let mut table = LayerTable::from_features(
        "lv_poles",
        &[pt_with_props(
            "pole-1",
            30.0,
            -2.0,
            json!({ "id": "pole-1", "pole_number": "P1" }),
        )],
    )
    .unwrap();

    let diff = table
        .edit_cell("pole-1", "pole_number", json!("P2"))
        .unwrap()
        .unwrap();

    assert_eq!(diff.layer_name, "lv_poles");
    assert_eq!(diff.row_index, 0);
    assert_eq!(diff.before, Some(json!("P1")));
    assert_eq!(diff.after, Some(json!("P2")));
    assert_eq!(
        table
            .get_row("pole-1")
            .unwrap()
            .unwrap()
            .properties
            .get("pole_number"),
        Some(&json!("P2"))
    );
}

#[test]
fn edit_cells_updates_many_rows_with_one_table_replace() {
    let mut table = LayerTable::from_features(
        "lv_poles",
        &[
            pt_with_props(
                "pole-1",
                30.0,
                -2.0,
                json!({ "struct_type": "S100", "material": "wooden" }),
            ),
            pt_with_props(
                "pole-2",
                30.1,
                -2.1,
                json!({ "struct_type": "S190", "material": "wooden" }),
            ),
        ],
    )
    .unwrap();
    let mut values = Map::new();
    values.insert("struct_type".to_string(), json!("S190"));
    values.insert("material".to_string(), json!("concrete"));

    let result = table
        .edit_cells(
            &[
                "pole-1".to_string(),
                "pole-2".to_string(),
                "missing".to_string(),
            ],
            &values,
        )
        .unwrap();

    assert_eq!(result.missing_feature_ids, vec!["missing"]);
    assert_eq!(result.changed.len(), 3);
    assert!(result.changed.iter().any(|diff| diff.feature_id == "pole-1"
        && diff.column == "struct_type"
        && diff.before == Some(json!("S100"))
        && diff.after == Some(json!("S190"))));
    assert!(result.changed.iter().any(|diff| diff.feature_id == "pole-2"
        && diff.column == "material"
        && diff.before == Some(json!("wooden"))
        && diff.after == Some(json!("concrete"))));
    assert_eq!(
        table
            .get_row("pole-2")
            .unwrap()
            .unwrap()
            .properties
            .get("struct_type"),
        Some(&json!("S190"))
    );
    assert_eq!(
        table
            .get_row("pole-1")
            .unwrap()
            .unwrap()
            .properties
            .get("material"),
        Some(&json!("concrete"))
    );
}

#[test]
fn delete_column_removes_property_from_all_rows() {
    let mut table = LayerTable::from_features(
        "lv_poles",
        &[
            pt_with_props("pole-1", 30.0, -2.0, json!({ "status": "old" })),
            pt_with_props("pole-2", 30.1, -2.1, json!({ "status": "old" })),
        ],
    )
    .unwrap();

    assert_eq!(table.delete_column("status").unwrap(), 2);
    assert!(
        !table
            .get_row("pole-1")
            .unwrap()
            .unwrap()
            .properties
            .contains_key("status")
    );
    assert_eq!(table.delete_column("missing").unwrap(), 0);
}
