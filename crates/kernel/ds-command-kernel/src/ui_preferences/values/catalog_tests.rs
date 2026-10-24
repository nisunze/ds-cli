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
    for project in [false, true] {
        let home = Key::ALL
            .iter()
            .filter(|key| project_scoped(**key) == project)
            .count();
        assert!(
            home <= crate::ui_preferences::MAX_KEYS,
            "one read can name every key of a home"
        );
    }
}

#[test]
fn window_and_project_view_keys_round_trip_in_their_homes() {
    let db = Connection::open_in_memory().unwrap();
    run(&db, json!({"action": "set", "key": "theme", "value": "dark", "now": 1})).unwrap();
    run(&db, json!({"action": "set", "key": "drawer_width", "value": 451.4, "now": 1})).unwrap();
    run(
        &db,
        json!({"action": "set", "project": "gisagara", "key": "layer_workspace", "value": {"workspace": "tiles", "design": "lv", "local": "gis"}, "now": 1}),
    )
    .unwrap();
    let account = run(
        &db,
        json!({"action": "get", "keys": ["theme", "language", "drawer_width"]}),
    )
    .unwrap();
    assert_eq!(
        account["values"],
        json!({"theme": "dark", "language": null, "drawer_width": 451})
    );
    let project = run(
        &db,
        json!({"action": "get", "project": "gisagara", "keys": ["layer_workspace", "paper_snap"]}),
    )
    .unwrap();
    assert_eq!(
        project["values"],
        json!({"layer_workspace": {"workspace": "tiles", "design": "lv", "local": "gis"}, "paper_snap": null})
    );
    let unscoped = run(
        &db,
        json!({"action": "set", "key": "board_view", "value": {"grouping": "status", "density": "compact", "collapsedColumns": []}, "now": 1}),
    )
    .unwrap_err();
    assert!(unscoped.contains("inside their exact project"), "{unscoped}");
}

#[test]
fn a_signed_out_window_keeps_its_own_device_scope() {
    let db = Connection::open_in_memory().unwrap();
    let anonymous = Identity {
        principal: Principal::Anonymous,
        lane: "canary".into(),
        audience: "a".repeat(64),
    };
    execute(
        &db,
        Input {
            identity: &anonymous,
            bytes: None,
        },
        serde_json::from_value(json!({"action": "set", "key": "language", "value": "en", "now": 1}))
            .unwrap(),
    )
    .unwrap();
    let account = run(&db, json!({"action": "get", "keys": ["language"]})).unwrap();
    assert_eq!(account["values"]["language"], Value::Null);
}

#[test]
fn a_set_stores_and_answers_the_canonical_value() {
    let db = Connection::open_in_memory().unwrap();
    let written = run(
        &db,
        json!({"action": "set", "key": "hillshade_exaggeration", "now": 1, "value": 1.5}),
    )
    .unwrap();
    assert_eq!(written["values"]["hillshade_exaggeration"], 1.5);
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
        json!({"action": "get", "keys": ["hillshade_exaggeration", "tool_layout", "map_defaults"]}),
    )
    .unwrap();
    assert_eq!(
        read["values"]["hillshade_exaggeration"],
        written["values"]["hillshade_exaggeration"]
    );
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
        json!({"action": "get", "project": "gisagara", "keys": ["layer_visibility", "hillshade_exaggeration"]}),
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
