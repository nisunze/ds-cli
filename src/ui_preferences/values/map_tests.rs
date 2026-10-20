use super::*;
use serde_json::json;

fn mode() -> Value {
    json!({
        "mode": "3d",
        "rotationLocked": false,
        "provider": "google",
        "google3dBuildings": true,
        "layer3DShapes": false,
        "hillshade": true,
        "hillshadeExaggeration": 1.5,
    })
}

#[test]
fn relief_never_persists_and_only_a_preset_intensity_is_kept() {
    let admitted = map_mode(&mode()).unwrap();
    assert_eq!(admitted["hillshade"], false, "relief lasts one session");
    assert_eq!(admitted["hillshadeExaggeration"], 1.5);
    assert_eq!(admitted["provider"], "google");
    let mut without = mode();
    without
        .as_object_mut()
        .unwrap()
        .remove("hillshadeExaggeration");
    assert!(map_mode(&without)
        .unwrap()
        .get("hillshadeExaggeration")
        .is_none());
    for (field, refused) in [
        ("hillshadeExaggeration", json!(0.7)),
        ("mode", json!("4d")),
        ("provider", json!("bing")),
        ("rotationLocked", json!("yes")),
        ("hillshade", json!(1)),
    ] {
        let mut value = mode();
        value[field] = refused.clone();
        assert!(map_mode(&value).is_none(), "{field}: {refused}");
    }
    let mut extra = mode();
    extra["pitch"] = json!(45);
    assert!(map_mode(&extra).is_none(), "an unknown field is refused");
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
fn imagery_tiles_come_from_the_broker_or_the_public_earth_engine_path() {
    for safe in [
        "/api/v1/earth-engine/tiles/abc/{z}/{x}/{y}",
        "https://gateway.example/api/v1/imagery/tiles/s2/{z}/{x}/{y}.png",
        "https://earthengine.googleapis.com/v1/projects/p/maps/m/tiles/{z}/{x}/{y}",
    ] {
        assert!(safe_tile_template(safe), "{safe}");
    }
    for unsafe_url in [
        "https://gateway.example/api/v1/imagery/tiles/{z}/{x}/{y}?token=abc",
        "https://earthengine.googleapis.com/v1/projects/p/maps/m/tiles/{z}/{x}/{y}?key=1",
        "http://earthengine.googleapis.com/v1/projects/p/maps/m/tiles/{z}/{x}/{y}",
        "https://tiles.example/{z}/{x}/{y}.png",
        "https://gateway.example/api/v1/imagery/tiles/{z}/{x}",
        "https://googleapis.com/earthengine/api/v1/imagery/tiles/{z}/{x}/{y}",
        "javascript:alert(1)//api/v1/imagery/tiles/{z}/{x}/{y}",
    ] {
        assert!(!safe_tile_template(unsafe_url), "{unsafe_url}");
    }
}

fn overlay() -> Value {
    json!({
        "id": "ee-1",
        "name": "Sentinel-2",
        "tileUrl": "/api/v1/earth-engine/tiles/ee-1/{z}/{x}/{y}",
        "dataset": "COPERNICUS/S2_SR",
        "expiresAt": "2026-10-20T10:00:00.000Z",
        "bounds": [29.0, -2.9, 30.9, -1.0],
        "maxZoom": 16,
    })
}

fn explorer() -> Value {
    json!({
        "version": 2,
        "point": {"lng": 30.06, "lat": -1.95, "label": "Site"},
        "overlay": overlay(),
        "overlays": [{
            "id": "ee-1", "overlay": overlay(), "visible": true, "opacity": 0.7,
            "addedAt": "2026-10-20T08:00:00.000Z", "updatedAt": "2026-10-20T08:00:00.000Z",
        }],
        "activeOverlayId": "ee-1",
        "visible": true,
        "opacity": 0.7,
        "savedAt": "2026-10-20T08:00:00.000Z",
    })
}

#[test]
fn the_imagery_explorer_keeps_version_two_with_admitted_layers() {
    assert_eq!(earth_engine_explorer(&explorer()), Some(explorer()));
    let mut no_point = explorer();
    no_point["point"] = Value::Null;
    assert!(earth_engine_explorer(&no_point).is_some());
    let mut leaked = explorer();
    leaked["overlays"][0]["overlay"]["tileUrl"] =
        json!("https://evil.example/{z}/{x}/{y}?access_token=1");
    assert!(earth_engine_explorer(&leaked).is_none());
    let mut v1 = explorer();
    v1["version"] = json!(1);
    assert!(
        earth_engine_explorer(&v1).is_none(),
        "version 1 arrives only through the import"
    );
    let mut opaque = explorer();
    opaque["opacity"] = json!(1.2);
    assert!(earth_engine_explorer(&opaque).is_none());
    let mut absent = overlay();
    absent["attribution"] = Value::Null;
    assert!(earth_engine_overlay(&absent)
        .unwrap()
        .get("attribution")
        .is_none());
}
