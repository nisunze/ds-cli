use super::*;
use serde_json::json;

const NOW: u64 = 1_760_950_000_000;

/// The one row an entry becomes, admitted the way the import admits it.
fn adopted(legacy_key: &str, value: Value) -> (Key, Option<String>, Value) {
    let mut rows = read(legacy_key, &value, NOW);
    assert_eq!(rows.len(), 1, "{legacy_key}: {rows:?}");
    let (key, project, candidate) = rows.remove(0);
    let admitted = key
        .admit(&candidate)
        .unwrap_or_else(|| panic!("{legacy_key}: the contract refused {candidate}"));
    (key, project, admitted)
}

#[test]
fn every_legacy_name_maps_to_one_key_of_its_home() {
    let (exact, prefixes) = keys();
    assert_eq!(exact.len(), EXACT.len());
    assert_eq!(prefixes.len(), PREFIXES.len());
    for (_, key) in EXACT {
        assert!(
            !key.project_scoped() || key == Key::PinnedTransformers,
            "{key:?}"
        );
    }
    for (prefix, key) in PREFIXES {
        assert!(key.project_scoped(), "{prefix} names a project key");
    }
    for (index, (prefix, _)) in PREFIXES.iter().enumerate() {
        for (other_index, (other, _)) in PREFIXES.iter().enumerate() {
            assert!(
                index == other_index || !other.starts_with(prefix),
                "{prefix} shadows {other}"
            );
        }
    }
}

#[test]
fn a_project_key_needs_a_project_the_kernel_admits() {
    let (key, project, value) = adopted(
        "ui:layer_visibility:gisagara",
        json!({"poles": false, "bad": "x"}),
    );
    assert_eq!(
        (key, project.as_deref()),
        (Key::LayerVisibility, Some("gisagara"))
    );
    assert_eq!(value, json!({"poles": false}));
    assert!(read("ui:layer_visibility:", &json!({"poles": false}), NOW).is_empty());
    assert!(read("ui:layer_visibility:a/b", &json!({"poles": false}), NOW).is_empty());
    assert!(read(
        "viewport:_global",
        &json!({"center": [0, 0], "zoom": 1, "bearing": 0, "pitch": 0}),
        NOW
    )
    .is_empty());
    assert!(
        read("capabilities:v2", &json!({}), NOW).is_empty(),
        "another domain's row"
    );
    assert!(
        read("ui:layer_group_collapse:v1", &json!({}), NOW).is_empty(),
        "nothing chosen is no row"
    );
}

#[test]
fn pins_fan_out_to_one_row_per_project() {
    let rows = read(
        "design:pinned_transformers:v1",
        &json!({"gisagara": [" TX-2", "mv_data", "TX-1"], "a/b": ["TX-1"], "rulindo": [], "nyamagabe": ["TX-9"]}),
        NOW,
    );
    let rows: Vec<(Option<String>, Value)> = rows
        .into_iter()
        .map(|(key, project, value)| {
            assert_eq!(key, Key::PinnedTransformers);
            (project, key.admit(&value).unwrap())
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (Some("gisagara".into()), json!(["TX-2", "TX-1"])),
            (Some("nyamagabe".into()), json!(["TX-9"])),
        ]
    );
}

#[test]
fn the_camera_mode_record_keeps_only_its_relief_intensity() {
    let (_, _, value) = adopted(
        "map_mode_state",
        json!({"mode": "3d", "rotationLocked": 1, "provider": "bing", "hillshade": true, "hillshadeExaggeration": 2.0}),
    );
    assert_eq!(value, json!(1.5), "snapped to the nearest preset");
    assert!(
        read("map_mode_state", &json!({"mode": "2d", "layer3DShapes": false}), NOW).is_empty(),
        "without an intensity the cartography default stays in charge"
    );
}

#[test]
fn a_working_area_restores_as_normalize_persisted_working_area_did() {
    let (_, _, value) = adopted(
        "working_area_query:v1:gisagara",
        json!({"mode": "admin", "admin": {"code": "0101", "name": "Gasabo", "level": "district", "hierarchy": {}},
               "drawBoundary": {"type": "Polygon", "coordinates": []}, "dateFrom": 5, "surveyors": ["a@x.rw", 3],
               "applied": 1, "appliedCenter": [30], "appliedZoom": 14}),
    );
    assert_eq!(value["admin"]["code"], "0101");
    assert_eq!(value["drawBoundary"], Value::Null);
    assert_eq!(value["dateFrom"], "");
    assert_eq!(value["surveyors"], json!(["a@x.rw"]));
    assert_eq!(value["appliedAt"], NOW, "a missing apply time is now");
    assert_eq!(value["appliedCenter"], Value::Null);
    assert_eq!(value["appliedZoom"], 14);
    let rows = read(
        "working_area_query:v1:gisagara",
        &json!({"mode": "full", "applied": true}),
        NOW,
    );
    assert_eq!(rows.len(), 1);
    assert!(
        Key::WorkingArea.admit(&rows[0].2).is_none(),
        "an unscoped area is skipped"
    );
}

#[test]
fn small_records_read_their_retired_shapes() {
    assert_eq!(
        adopted(
            "ds_design_process_preset:gisagara",
            json!({"preset": "drafting", "updatedAt": 1})
        )
        .2,
        json!("drafting")
    );
    assert_eq!(
        adopted("design:active_lv_transformer:v1:gisagara", json!(" TX-4 ")).2,
        json!("TX-4")
    );
    assert_eq!(
        adopted(
            "design:active_lv_transformer:v1:gisagara",
            json!({"transformerName": "TX-5", "updatedAt": 1})
        )
        .2,
        json!("TX-5")
    );
    assert_eq!(
        adopted(
            "ui:popup_layers_disabled:gisagara",
            json!({"poles": true, "lines": false})
        )
        .2,
        json!(["poles"])
    );
    assert_eq!(
        adopted("ui:popup_layers_disabled:gisagara", json!(["b ", "a", 7])).2,
        json!(["7", "a", "b"])
    );
    assert_eq!(
        adopted(
            "design_status_sort:gisagara",
            json!({"key": "district", "asc": 0})
        )
        .2,
        json!({"key": "district", "asc": false})
    );
    assert!(read("design_status_sort:gisagara", &json!({"asc": true}), NOW).is_empty());
    assert_eq!(
        adopted("design_status_warning_types:gisagara", json!(["b", 1, "a"])).2,
        json!(["a", "b"])
    );
    assert_eq!(
        adopted(
            "design_layer_upload_dialog_layout:v1",
            json!({"splitLeftPercent": "70.4"})
        )
        .2,
        json!({"splitLeftPercent": 70})
    );
    assert!(read(
        "design_layer_upload_dialog_layout:v1",
        &json!({"splitLeftPercent": null}),
        NOW
    )
    .is_empty());
    assert_eq!(
        adopted("properties:dock-width:v1", json!(1200.5)).2,
        json!(900)
    );
    assert_eq!(
        adopted(
            "design:create-transformer-map-data-sources:v1",
            json!({"selectedKeys": ["a", 4, "a"]})
        )
        .2,
        json!({"selectedKeys": ["a"]})
    );
    assert_eq!(
        adopted("edit_projection_choice", json!("utm")).2,
        json!("utm")
    );
    assert_eq!(
        adopted("design:pinned_group_visible:v1", json!(false)).2,
        json!(false)
    );
}

#[test]
fn settings_toolbar_and_restart_records_read_their_retired_shapes() {
    let (_, _, settings) = adopted(
        "application_settings:v1",
        json!({"profileTerrainPointRadiusPx": 4, "processingLane": "fast"}),
    );
    assert!(settings.get("processingLane").is_none());
    assert!(settings.get("profileTerrainPointWidthPx").is_some());
    assert!(read(
        "application_settings:v1",
        &json!({"processingLane": "fast"}),
        NOW
    )
    .is_empty());
    let (_, _, layout) = adopted(
        "tool-layout:map-editing:v3",
        json!({"groups": {"design.actions": {"order": ["reports"]}}}),
    );
    assert_eq!(layout["groups"]["design.actions"]["order"][0], "reports");
    assert!(read(
        "profile:occupancy:v1",
        &json!({"schemaVersion": 1, "occupant": {"kind": "generic", "profileId": "g"}, "updatedAt": 1}),
        NOW
    )
    .is_empty(), "a generic record without its scene was never restorable");
    let (_, _, occupancy) = adopted(
        "profile:occupancy:v1",
        json!({"schemaVersion": 1, "occupant": {"kind": "model", "modelId": "m"}}),
    );
    assert_eq!(occupancy["updatedAt"], NOW);
}

#[test]
fn fast_mode_choices_read_every_retired_release() {
    let (key, _, first) = adopted(
        "ds_fast_mode_customer_source:v1:gisagara",
        json!({"sourceAddress": " survey://customers ", "sourceLabel": "Survey", "updatedAt": 9}),
    );
    assert_eq!(key, Key::FastModeCustomerSource);
    assert_eq!(
        first,
        json!({"addresses": ["survey://customers"], "labels": ["Survey"], "updated_at": 9})
    );
    let (_, _, listed) = adopted(
        "ds_fast_mode_customer_source:v1:gisagara",
        json!({"customerSourceAddress": "a", "customerSourceAddresses": ["design://layer/customers", "survey://c"],
               "customerSourceLabels": ["Design customers", "C"], "updatedAt": "late"}),
    );
    assert_eq!(
        listed["addresses"],
        json!(["design://layer/customers", "survey://c"])
    );
    assert_eq!(listed["updated_at"], 0);
    assert!(read(
        "ds_fast_mode_customer_source:v1:gisagara",
        &json!({"customerSourceLabel": "x"}),
        NOW
    )
    .is_empty());
    let (_, _, poles) = adopted(
        "ds_fast_mode_pole_source:v1:gisagara",
        json!({"poleSourceAddresses": ["design://layer/customers", " survey://poles ", ""], "poleSourceLabels": ["D", " Poles "], "updatedAt": 3}),
    );
    assert_eq!(
        poles,
        json!({"addresses": ["survey://poles"], "labels": ["Poles"], "updated_at": 3})
    );
}
