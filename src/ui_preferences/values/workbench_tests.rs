use super::*;
use serde_json::json;

/// ds-web's `DEFAULT_TOOL_LAYOUT`, as the toolbar renders it before a choice.
fn default_layout() -> Value {
    json!({
        "position": "top-right",
        "groups": {
            "cad.modify": {
                "order": ["settings", "move", "vertex", "trim", "extend", "split", "splitWithCutter", "join", "offset", "buffer", "delete"],
                "pinned": {
                    "settings": true, "move": true, "vertex": false, "trim": false, "extend": false,
                    "split": false, "splitWithCutter": false, "join": false, "calculateProperties": false,
                    "offset": false, "buffer": false, "delete": false,
                },
            },
            "design.actions": {
                "order": ["process", "cloudSave", "reports"],
                "pinned": {"process": true, "cloudSave": true, "reports": true},
            },
        },
    })
}

#[test]
fn an_empty_layout_is_the_default_toolbar() {
    assert_eq!(tool_layout(&json!({})), Some(default_layout()));
    assert_eq!(tool_layout(&default_layout()), Some(default_layout()));
    assert!(tool_layout(&json!([])).is_none());
}

#[test]
fn a_layout_keeps_known_tools_once_with_settings_first_and_pins_only_ordered_tools() {
    let admitted = tool_layout(&json!({
        "position": "bottom-left",
        "groups": {
            "cad.modify": {
                "order": ["delete", "settings", "delete", "laser", "calculateProperties", "move"],
                "pinned": {"delete": true, "calculateProperties": true, "laser": true, "vertex": "yes"},
            },
            "design.actions": {"order": ["reports"], "pinned": {"process": false}},
        },
    }))
    .unwrap();
    let modify = &admitted["groups"]["cad.modify"];
    assert_eq!(
        modify["order"],
        json!([
            "settings",
            "delete",
            "move",
            "vertex",
            "trim",
            "extend",
            "split",
            "splitWithCutter",
            "join",
            "offset",
            "buffer"
        ])
    );
    assert_eq!(modify["pinned"]["delete"], true);
    assert_eq!(
        modify["pinned"]["calculateProperties"], false,
        "a tool outside the order keeps its default"
    );
    assert_eq!(
        modify["pinned"]["vertex"], false,
        "a non-boolean pin keeps the default"
    );
    assert!(modify["pinned"].get("laser").is_none());
    assert_eq!(
        admitted["groups"]["design.actions"]["order"],
        json!(["reports", "process", "cloudSave"])
    );
    assert_eq!(
        admitted["groups"]["design.actions"]["pinned"]["process"],
        false
    );
    assert_eq!(admitted["position"], "top-right");
}

#[test]
fn panel_geometry_is_clamped_to_whole_units() {
    assert_eq!(properties_dock_width(&json!(412.6)), Some(json!(413)));
    assert_eq!(properties_dock_width(&json!(1400)), Some(json!(900)));
    assert_eq!(properties_dock_width(&json!(10)), Some(json!(280)));
    assert!(properties_dock_width(&json!("400")).is_none());
    assert_eq!(
        upload_dialog_layout(&json!({"splitLeftPercent": 62.5})),
        Some(json!({"splitLeftPercent": 63}))
    );
    assert_eq!(
        upload_dialog_layout(&json!({"splitLeftPercent": 99})),
        Some(json!({"splitLeftPercent": 80}))
    );
    assert!(upload_dialog_layout(&json!({"splitLeftPercent": 60, "other": 1})).is_none());
    assert!(upload_dialog_layout(&json!({})).is_none());
    let panel = json!({"x": -120.5, "y": 24, "w": 560, "h": 640});
    assert_eq!(style_center_layout(&panel), Some(panel.clone()));
    assert!(style_center_layout(&json!({"x": 0, "y": 0, "w": 0, "h": 640})).is_none());
    assert!(style_center_layout(&json!({"x": 0, "y": 0, "w": 560})).is_none());
}

#[test]
fn application_settings_are_the_settings_owners_record_and_device_choices_stay_unset() {
    let admitted = application_settings(&json!({
        "mapMemoryBudget": 9,
        "processingLane": "fast",
        "notASetting": true,
    }))
    .unwrap();
    assert_eq!(
        admitted["mapMemoryBudget"], 5,
        "clipped by the settings owner"
    );
    assert!(
        admitted.get("processingLane").is_none(),
        "a retired key is dropped"
    );
    assert!(admitted.get("notASetting").is_none());
    assert!(
        admitted.get("popupMode").is_none(),
        "the device decides an unset popup mode"
    );
    assert!(admitted.get("showMyLocationControl").is_none());
    let record =
        application_settings(&json!({"popupMode": "minimal", "showMyLocationControl": true}))
            .unwrap();
    assert_eq!(record["popupMode"], "minimal");
    assert_eq!(record["showMyLocationControl"], true);
    assert_eq!(
        application_settings(&record),
        Some(record.clone()),
        "the record is its own canonical form"
    );
    assert!(application_settings(&json!("settings")).is_none());
}

fn drawn_area() -> Value {
    json!({
        "mode": "draw",
        "admin": {"code": "0101", "name": "Gasabo", "level": "district", "hierarchy": {"province": "Kigali"}},
        "drawBoundary": {"type": "Polygon", "coordinates": [[[30.0, -1.9], [30.1, -1.9], [30.1, -2.0], [30.0, -1.9]]]},
        "fullProjectLoad": true,
        "dateFrom": "2026-01-01",
        "dateTo": "",
        "surveyors": ["a@datasolutions.rw"],
        "applied": true,
        "appliedAt": 1_760_000_000_000_u64,
        "appliedCenter": [30.05, -1.95],
        "appliedZoom": 14,
        "appliedBbox": [30.0, -2.0, 30.1, -1.9],
    })
}

#[test]
fn a_working_area_keeps_only_what_its_mode_uses() {
    let admitted = working_area(&drawn_area()).unwrap();
    assert_eq!(admitted["drawBoundary"]["type"], "Polygon");
    assert_eq!(
        admitted["admin"],
        Value::Null,
        "an admin unit belongs to admin mode"
    );
    assert_eq!(
        admitted["fullProjectLoad"], false,
        "a whole-project load belongs to full mode"
    );
    assert_eq!(working_area(&admitted), Some(admitted.clone()));
    let full = json!({
        "mode": "full", "admin": null, "drawBoundary": null, "fullProjectLoad": true,
        "dateFrom": "", "dateTo": "", "surveyors": [], "applied": true, "appliedAt": 5,
        "appliedCenter": null, "appliedZoom": null, "appliedBbox": null,
    });
    assert_eq!(
        working_area(&full),
        Some(full.clone()),
        "the download plan's whole-project area"
    );
}

#[test]
fn an_unapplied_unscoped_or_malformed_working_area_is_refused() {
    let mut unapplied = drawn_area();
    unapplied["applied"] = json!(false);
    assert!(working_area(&unapplied).is_none());
    let unscoped = json!({
        "mode": "full", "drawBoundary": null, "fullProjectLoad": false, "dateFrom": "", "dateTo": "",
        "surveyors": [], "applied": true, "appliedAt": 5,
    });
    assert!(
        working_area(&unscoped).is_none(),
        "an applied area without a scope is no area"
    );
    for (field, refused) in [
        ("dateFrom", json!("01/02/2026")),
        ("surveyors", json!([" padded "])),
        ("appliedAt", json!(-1)),
        ("appliedBbox", json!([1, 2, 3])),
        ("mode", json!("lasso")),
        (
            "drawBoundary",
            json!({"type": "Point", "coordinates": [30, -2]}),
        ),
    ] {
        let mut value = drawn_area();
        value[field] = refused;
        assert!(working_area(&value).is_none(), "{field}");
    }
    let mut huge = drawn_area();
    huge["drawBoundary"]["coordinates"] = json!([vec![[30.123456789, -1.987654321]; 40_000]]);
    assert!(
        working_area(&huge).is_none(),
        "the survey read refuses a boundary over 1 MiB"
    );
}

fn generic(scene: Value) -> Value {
    json!({
        "schemaVersion": 1,
        "occupant": {"kind": "generic", "profileId": "generic-profile-0a1b2c3d"},
        "genericScene": scene,
        "updatedAt": 42,
    })
}

#[test]
fn the_profile_restart_record_names_one_owner_and_retains_a_generic_scene_within_its_bound() {
    let model = json!({
        "schemaVersion": 1,
        "occupant": {"kind": "model", "modelId": "model-1", "viewportIdentity": {
            "kind": "project-version", "dsProject": "gisagara", "projectModelId": "m3", "revisionId": "r9"}},
        "genericScene": {"schema_version": 1},
        "updatedAt": 42,
    });
    let admitted = profile_occupancy(&model).unwrap();
    assert!(
        admitted.get("genericScene").is_none(),
        "a model occupant carries no scene"
    );
    assert_eq!(admitted["occupant"]["viewportIdentity"]["revisionId"], "r9");
    let kept = profile_occupancy(&generic(json!({"schema_version": 1, "bands": []}))).unwrap();
    assert_eq!(kept["genericScene"]["schema_version"], 1);
    let large = json!({"schema_version": 1, "points": "x".repeat(PROFILE_OCCUPANCY_MAX_BYTES)});
    let dropped = profile_occupancy(&generic(large)).unwrap();
    assert!(
        dropped.get("genericScene").is_none(),
        "a scene over the bound is not retained"
    );
    assert_eq!(dropped["occupant"]["kind"], "generic");
    for refused in [
        json!({"schemaVersion": 2, "occupant": {"kind": "generic", "profileId": "p"}, "updatedAt": 1}),
        json!({"schemaVersion": 1, "occupant": {"kind": "model", "modelId": "  "}, "updatedAt": 1}),
        json!({"schemaVersion": 1, "occupant": {"kind": "map", "modelId": "m"}, "updatedAt": 1}),
        json!({"schemaVersion": 1, "occupant": {"kind": "model", "modelId": "m", "viewportIdentity": {"kind": "local", "extra": 1}}, "updatedAt": 1}),
        generic(json!("scene")),
    ] {
        assert!(profile_occupancy(&refused).is_none(), "{refused}");
    }
}

#[test]
fn dismissals_keep_each_id_once_and_the_latest_256() {
    assert_eq!(
        notification_dismissals(&json!(["m1", "m2", "m1"])),
        Some(json!(["m1", "m2"]))
    );
    let many: Vec<String> = (0..300).map(|n| format!("m{n}")).collect();
    let kept = notification_dismissals(&json!(many)).unwrap();
    assert_eq!(kept.as_array().unwrap().len(), 256);
    assert_eq!(kept[0], "m44");
    assert!(notification_dismissals(&json!([" m1"])).is_none());
    assert!(notification_dismissals(&json!([""])).is_none());
    assert!(notification_dismissals(&json!({"m1": true})).is_none());
}
