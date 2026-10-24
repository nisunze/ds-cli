use super::*;
use serde_json::json;

#[test]
fn theme_and_language_name_one_member() {
    assert_eq!(theme(&json!("dark")), Some(json!("dark")));
    assert!(theme(&json!("sepia")).is_none());
    assert!(theme(&json!(true)).is_none());
    assert_eq!(language(&json!("fr")), Some(json!("fr")));
    assert!(language(&json!("de")).is_none());
    assert_eq!(
        instant(&json!(1_700_000_000_000u64)),
        Some(json!(1_700_000_000_000u64))
    );
    assert!(instant(&json!(-1)).is_none());
}

#[test]
fn hints_keep_each_id_once_and_the_latest_bounded() {
    assert_eq!(
        activity_hints(&json!({"seen": ["a", "b", "a"], "lastShownAt": 5})),
        Some(json!({"seen": ["a", "b"], "lastShownAt": 5}))
    );
    assert_eq!(
        activity_hints(&json!({"seen": []})),
        Some(json!({"seen": [], "lastShownAt": null}))
    );
    let many: Vec<String> = (0..200).map(|index| format!("hint-{index}")).collect();
    let kept = activity_hints(&json!({"seen": many, "lastShownAt": null})).unwrap();
    assert_eq!(kept["seen"].as_array().unwrap().len(), 128);
    assert_eq!(kept["seen"][127], "hint-199");
    assert!(activity_hints(&json!({"seen": [" a"]})).is_none());
    assert!(activity_hints(&json!({"seen": [], "extra": 1})).is_none());
    assert!(activity_hints(&json!({"seen": [], "lastShownAt": "soon"})).is_none());
}

#[test]
fn every_layer_group_is_answered() {
    let settings = layer_group_settings(&json!({"tiles": {"showInDrawer": false}})).unwrap();
    assert_eq!(settings.as_object().unwrap().len(), 6);
    assert_eq!(
        settings["tiles"],
        json!({"showInDrawer": false, "keepContentsVisible": true})
    );
    assert_eq!(
        settings["survey"],
        json!({"showInDrawer": true, "keepContentsVisible": true})
    );
    assert!(layer_group_settings(&json!({"retired": {}})).is_none());
    assert!(layer_group_settings(&json!({"tiles": {"showInDrawer": "no"}})).is_none());
}

#[test]
fn command_shortcuts_follow_the_navigation_owner() {
    let held = command_shortcuts(&json!({"solar": false, "retired": true})).unwrap();
    assert_eq!(held["solar"], false);
    assert_eq!(held["factory"], true);
    assert!(held.get("retired").is_none());
    assert!(command_shortcuts(&json!({"solar": "off"})).is_none());
    assert!(command_shortcuts(&json!([])).is_none());
}

#[test]
fn geometry_rounds_and_clamps_into_its_band() {
    assert_eq!(drawer_width(&json!(412.6)), Some(json!(413)));
    assert_eq!(drawer_width(&json!(10)), Some(json!(320)));
    assert_eq!(drawer_width(&json!(9000)), Some(json!(600)));
    assert!(drawer_width(&json!("wide")).is_none());
    assert_eq!(
        tools_panel_size(&json!({"width": 640.4, "height": 10})),
        Some(json!({"width": 640, "height": 280}))
    );
    assert!(tools_panel_size(&json!({"width": 640})).is_none());
    assert!(tools_panel_size(&json!({"width": -1, "height": 400})).is_none());
    assert_eq!(pdf_screen_scale(&json!(1.25)), Some(json!(1.25)));
    assert_eq!(pdf_screen_scale(&json!(1)), Some(json!(1)));
    assert!(pdf_screen_scale(&json!(3)).is_none());
}

#[test]
fn connector_style_names_both_members() {
    let style = json!({"shape": "straight", "marker": "dots"});
    assert_eq!(connector_style(&style), Some(style));
    assert!(connector_style(&json!({"shape": "straight"})).is_none());
    assert!(connector_style(&json!({"shape": "zigzag", "marker": "end"})).is_none());
}

#[test]
fn profile_workspace_keeps_known_labels_once() {
    assert_eq!(
        profile_workspace(
            &json!({"showFit": false, "showZoom": true, "mapLabels": ["type", "number", "type"], "mapLabelOrientation": "auto"})
        ),
        Some(
            json!({"showFit": false, "showZoom": true, "mapLabels": ["type", "number"], "mapLabelOrientation": "auto"})
        )
    );
    assert!(profile_workspace(&json!({"showFit": false, "showZoom": true, "mapLabels": ["name"], "mapLabelOrientation": "auto"})).is_none());
    assert!(profile_workspace(&json!({"showFit": false, "showZoom": true, "mapLabels": [], "mapLabelOrientation": "diagonal"})).is_none());
}

#[test]
fn profile_viewports_keep_the_latest_camera_of_each_model() {
    let camera = |id: &str, zoom: i64| json!({"id": id, "zoom": zoom, "panX": 1, "panY": -2.5, "cursorEntityId": null});
    assert_eq!(
        profile_viewports(&json!([camera("a", 1), camera("b", 2), camera("a", 3)])),
        Some(json!([camera("b", 2), camera("a", 3)]))
    );
    let many: Vec<Value> = (0..80)
        .map(|index| camera(&format!("m{index}"), 1))
        .collect();
    let kept = profile_viewports(&Value::Array(many)).unwrap();
    assert_eq!(kept.as_array().unwrap().len(), MAX_PROFILE_VIEWPORTS);
    assert_eq!(kept[0]["id"], "m16");
    assert!(profile_viewports(&json!([camera("a", 0)])).is_none());
    assert!(profile_viewports(&json!([{"id": "a", "zoom": 1, "panX": 0}])).is_none());
    assert!(profile_viewports(&json!({"a": camera("a", 1)})).is_none());
}
