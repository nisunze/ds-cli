use super::*;
use serde_json::json;

#[test]
fn only_a_preset_relief_intensity_is_kept() {
    assert_eq!(hillshade_exaggeration(&json!(1.5)), Some(json!(1.5)));
    for refused in [json!(0.7), json!("1.5"), json!({"hillshadeExaggeration": 1.5}), json!(null)] {
        assert!(hillshade_exaggeration(&refused).is_none(), "{refused}");
    }
}

#[test]
fn cartography_defaults_keep_known_flat_sections_and_never_the_basemap_catalogue() {
    let admitted = map_defaults(&json!({
        "hillshade": {"enabled": true, "exaggeration": 0.6},
        "camera": {"default_pitch_3d": 20, "max_pitch": 85, "experimental_range_warning": true},
        "basemaps": [{"id": "osm"}],
        "labels": {"halo": 2},
    }))
    .unwrap();
    assert_eq!(admitted["hillshade"]["enabled"], false);
    assert_eq!(admitted["camera"]["max_pitch"], 85);
    assert!(
        admitted.get("basemaps").is_none(),
        "the catalogue is the frontend's"
    );
    assert!(
        admitted.get("labels").is_none(),
        "an unknown section is not kept"
    );
    assert!(map_defaults(&json!({"camera": {"nested": {"x": 1}}})).is_none());
    assert!(map_defaults(&json!({"camera": 5})).is_none());
    assert!(map_defaults(&json!({"basemaps": "osm"})).is_none());
    assert!(map_defaults(&json!([1])).is_none());
}

#[test]
fn a_project_camera_is_bounded() {
    let camera = json!({"center": [30.06, -1.95], "zoom": 14.5, "bearing": -12, "pitch": 40});
    assert_eq!(map_viewport(&camera), Some(camera.clone()));
    for (field, refused) in [
        ("center", json!([30.06, 95])),
        ("center", json!([30.06])),
        ("pitch", json!(120)),
        ("zoom", json!("14")),
    ] {
        let mut value = camera.clone();
        value[field] = refused;
        assert!(map_viewport(&value).is_none(), "{field}");
    }
    assert!(map_viewport(&json!({"center": [0, 0], "zoom": 1, "bearing": 0})).is_none());
}

#[test]
fn the_overlay_registry_is_the_overlay_owners() {
    let layer = json!({"id": "tile-1", "name": "Survey", "kind": "xyz", "url": "https://tiles.example/{z}/{x}/{y}.png", "tileSize": 256, "visible": true});
    let registry = json!({"schema": 1, "revision": 3, "legacyImported": true, "layers": [layer]});
    assert_eq!(tile_layers(&registry).unwrap()["layers"][0]["id"], "tile-1");
    let mut credentials = registry.clone();
    credentials["layers"][0]["url"] = json!("https://user:secret@tiles.example/{z}/{x}/{y}.png");
    assert!(tile_layers(&credentials).is_none());
    let mut schema = registry.clone();
    schema["schema"] = json!(2);
    assert!(tile_layers(&schema).is_none());
}

#[test]
fn the_editing_projection_is_a_closed_choice_and_a_bounded_proj_text() {
    assert_eq!(
        edit_projection_choice(&json!("tm_rwanda")),
        Some(json!("tm_rwanda"))
    );
    assert!(edit_projection_choice(&json!("epsg:4326")).is_none());
    let proj = "+proj=tmerc +lat_0=0 +lon_0=30\n+k=0.9999 +x_0=500000";
    assert_eq!(edit_projection_custom(&json!(proj)), Some(json!(proj)));
    assert_eq!(edit_projection_custom(&json!("")), Some(json!("")));
    assert!(edit_projection_custom(&json!("a".repeat(MAX_CUSTOM_PROJECTION_BYTES + 1))).is_none());
    assert!(edit_projection_custom(&json!(7)).is_none());
}

#[test]
fn switches_are_booleans_under_bounded_keys() {
    let collapse = json!({"layers:tiles:osm": false, "layers:design:lv": true});
    assert_eq!(switches(&collapse), Some(collapse.clone()));
    assert!(switches(&json!({"layers": "yes"})).is_none());
    assert!(switches(&json!({"": true})).is_none());
    assert!(switches(&json!({"k".repeat(1025): true})).is_none());
    let many: serde_json::Map<String, Value> = (0..=MAX_SWITCHES)
        .map(|index| (format!("layer-{index}"), json!(true)))
        .collect();
    assert!(switches(&Value::Object(many)).is_none());
}

#[test]
fn disabled_popups_are_one_sorted_set() {
    assert_eq!(
        popup_layers_disabled(&json!([" poles", "customers", "poles", ""])),
        Some(json!(["customers", "poles"]))
    );
    assert!(popup_layers_disabled(&json!(["poles", 3])).is_none());
    assert!(popup_layers_disabled(&json!({"poles": true})).is_none());
}

#[test]
fn local_class_styles_keep_typed_documents_by_trimmed_ref() {
    let kept = local_class_styles(&json!({" ud/lv_lines ": {"type": "line", "paint": {}}}));
    assert_eq!(kept, Some(json!({"ud/lv_lines": {"type": "line", "paint": {}}})));
    assert!(local_class_styles(&json!({"ud/x": {"paint": {}}})).is_none());
    assert!(local_class_styles(&json!({"": {"type": "line"}})).is_none());
    assert!(local_class_styles(&json!([{"type": "line"}])).is_none());
}
