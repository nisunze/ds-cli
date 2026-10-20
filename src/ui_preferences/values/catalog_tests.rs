use super::*;
use crate::durable_store::{Identity, Input, Principal};
use crate::ui_preferences::{execute, INVALID};
use rusqlite::Connection;
use serde_json::json;

fn run(connection: &Connection, operation: Value) -> Result<Value, String> {
    let identity = Identity {
        principal: Principal::Account("owner".into()),
        lane: "canary".into(),
        audience: "a".repeat(64),
    };
    let outcome = execute(
        connection,
        Input {
            identity: &identity,
            bytes: None,
        },
        serde_json::from_value(operation).map_err(|e| e.to_string())?,
    )?;
    Ok(Value::Object(outcome.value))
}

#[test]
fn every_key_is_named_as_its_wire_spelling_and_listed_once() {
    for key in Key::ALL {
        assert_eq!(
            serde_json::to_value(key).unwrap(),
            json!(name(*key)),
            "{key:?}"
        );
        assert!(max_bytes(*key) >= DEFAULT_MAX_BYTES, "{key:?}");
    }
    let mut names: Vec<&str> = Key::ALL.iter().map(|key| name(*key)).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), Key::ALL.len());
    assert!(
        Key::ALL.len() <= crate::ui_preferences::MAX_KEYS,
        "one read can name every key of a home"
    );
}

#[test]
fn a_set_stores_and_answers_the_canonical_value() {
    let db = Connection::open_in_memory().unwrap();
    let written = run(
        &db,
        json!({"action": "set", "key": "map_mode", "now": 1, "value": {
            "mode": "2d", "rotationLocked": true, "provider": "auto",
            "google3dBuildings": true, "layer3DShapes": true, "hillshade": true}}),
    )
    .unwrap();
    assert_eq!(written["values"]["map_mode"]["hillshade"], false);
    let layout = run(
        &db,
        json!({"action": "set", "key": "tool_layout", "now": 2, "value": {"groups": {"cad.modify": {"order": ["move", "settings"]}}}}),
    )
    .unwrap();
    assert_eq!(
        layout["values"]["tool_layout"]["groups"]["cad.modify"]["order"][0],
        "settings"
    );
    let read = run(
        &db,
        json!({"action": "get", "keys": ["map_mode", "tool_layout", "map_defaults"]}),
    )
    .unwrap();
    assert_eq!(read["values"]["map_mode"], written["values"]["map_mode"]);
    assert_eq!(
        read["values"]["tool_layout"],
        layout["values"]["tool_layout"]
    );
    assert_eq!(read["values"]["map_defaults"], Value::Null);
}

#[test]
fn a_project_key_lives_inside_its_exact_project() {
    let db = Connection::open_in_memory().unwrap();
    run(
        &db,
        json!({"action": "set", "project": "gisagara", "key": "layer_visibility", "value": {"poles": false}, "now": 1}),
    )
    .unwrap();
    let held = |project: &str| {
        run(&db, json!({"action": "get", "project": project, "keys": ["layer_visibility", "map_viewport"]})).unwrap()
    };
    assert_eq!(
        held("gisagara")["values"]["layer_visibility"],
        json!({"poles": false})
    );
    assert_eq!(held("rulindo")["values"]["layer_visibility"], Value::Null);
    let unscoped = run(&db, json!({"action": "get", "keys": ["layer_visibility"]})).unwrap_err();
    assert!(
        unscoped.contains("inside their exact project"),
        "{unscoped}"
    );
    let mixed = run(
        &db,
        json!({"action": "get", "project": "gisagara", "keys": ["layer_visibility", "map_mode"]}),
    )
    .unwrap_err();
    assert!(mixed.contains(INVALID), "{mixed}");
}

#[test]
fn a_refused_or_oversized_value_names_its_key() {
    let db = Connection::open_in_memory().unwrap();
    let refused = run(
        &db,
        json!({"action": "set", "key": "properties_dock_width", "value": "wide", "now": 1}),
    )
    .unwrap_err();
    assert_eq!(
        refused,
        format!("{INVALID}: properties_dock_width does not admit that value")
    );
    let many: serde_json::Map<String, Value> = (0..4096)
        .map(|index| {
            (
                format!("layers:card:gisagara:{index}:{}", "x".repeat(120)),
                json!(true),
            )
        })
        .collect();
    let oversized = run(
        &db,
        json!({"action": "set", "key": "layer_group_collapse", "value": many, "now": 1}),
    )
    .unwrap_err();
    assert_eq!(
        oversized,
        format!("{INVALID}: layer_group_collapse exceeds its byte bound")
    );
}

#[test]
fn the_legacy_import_fans_out_pins_and_never_replaces_a_newer_value() {
    let db = Connection::open_in_memory().unwrap();
    run(
        &db,
        json!({"action": "set", "project": "gisagara", "key": "pinned_transformers", "value": ["TX-7"], "now": 1}),
    )
    .unwrap();
    let declared = run(
        &db,
        json!({"action": "legacy", "source": "persistent_cache"}),
    )
    .unwrap();
    assert!(declared["legacy"]["keys"]
        .as_array()
        .unwrap()
        .contains(&json!("map_mode_state")));
    assert!(declared["legacy"]["prefixes"]
        .as_array()
        .unwrap()
        .contains(&json!("viewport:")));
    let entries = json!([
        {"legacy_key": "design:pinned_transformers:v1", "value": {"gisagara": ["TX-1"], "rulindo": ["TX-2", "TX-3"]}},
        {"legacy_key": "viewport:rulindo", "value": {"center": [30.0, -1.6], "zoom": 13, "bearing": 0, "pitch": 0}},
        {"legacy_key": "edit_projection_choice", "value": "mercator"},
        {"legacy_key": "help:seen", "value": true},
    ]);
    let receipt = run(
        &db,
        json!({"action": "import", "source": "persistent_cache", "entries": entries, "now": 9}),
    )
    .unwrap();
    assert_eq!(receipt["receipt"]["count"], 2, "rulindo's pins and camera");
    assert_eq!(
        receipt["receipt"]["skipped"], 3,
        "gisagara's newer pins, a refused choice, another domain's row"
    );
    let gisagara = run(
        &db,
        json!({"action": "get", "project": "gisagara", "keys": ["pinned_transformers"]}),
    )
    .unwrap();
    assert_eq!(gisagara["values"]["pinned_transformers"], json!(["TX-7"]));
    let rulindo = run(&db, json!({"action": "get", "project": "rulindo", "keys": ["pinned_transformers", "map_viewport"]})).unwrap();
    assert_eq!(
        rulindo["values"]["pinned_transformers"],
        json!(["TX-2", "TX-3"])
    );
    assert_eq!(rulindo["values"]["map_viewport"]["zoom"], 13);
}
