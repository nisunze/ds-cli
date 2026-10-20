use super::*;
use crate::durable_store::Principal;

fn who(principal: Principal) -> Identity {
    Identity {
        principal,
        lane: "canary".into(),
        audience: "ds-prod".into(),
    }
}
fn alice() -> Identity {
    who(Principal::Account("alice".into()))
}
fn bob() -> Identity {
    who(Principal::Account("bob".into()))
}
fn db() -> Connection {
    Connection::open_in_memory().unwrap()
}
fn outcome(connection: &Connection, identity: &Identity, value: Value) -> Result<Outcome, String> {
    let operation = serde_json::from_value(value).map_err(|e| e.to_string())?;
    execute(connection, Input { identity, bytes: None }, operation)
}
fn run(connection: &Connection, identity: &Identity, value: Value) -> Result<Value, String> {
    outcome(connection, identity, value).map(|outcome| Value::Object(outcome.value))
}
fn ok(connection: &Connection, identity: &Identity, value: Value) -> Value {
    run(connection, identity, value).unwrap()
}
fn layer(id: &str) -> Value {
    json!({"id": id, "name": id, "geometryType": "Point"})
}
fn gis_layer(id: &str, handle_key: &str) -> Value {
    json!({"id": id, "name": id, "sourceMetadata": {"kind": "gis_folder", "handleKey": handle_key}})
}
fn features(n: usize) -> String {
    serde_json::to_string(&(0..n).map(|i| json!({"id": format!("f{i}"), "geometry": null, "properties": {}})).collect::<Vec<_>>()).unwrap()
}
fn features_of(connection: &Connection, identity: &Identity, id: &str) -> Value {
    ok(connection, identity, json!({"action":"get_features","layer_id":id}))["features"].clone()
}

#[test]
fn a_catalogue_round_trips_per_identity() {
    let connection = db();
    let empty = ok(&connection, &alice(), json!({"action":"read"}));
    assert!(empty["records"].is_null());
    ok(&connection, &alice(), json!({"action":"write_catalogue","records":[layer("a"), {"raw":"carried"}],"revision":3,"now":10}));
    let held = ok(&connection, &alice(), json!({"action":"read"}));
    assert_eq!(held["records"], json!([layer("a"), {"raw":"carried"}]));
    assert_eq!(held["revision"], 3);
    assert!(ok(&connection, &bob(), json!({"action":"read"}))["records"].is_null());
    assert!(ok(&connection, &who(Principal::Anonymous), json!({"action":"read"}))["records"].is_null());
}

#[test]
fn catalogue_and_payload_writes_advance_their_own_generations() {
    let connection = db();
    let written = outcome(&connection, &alice(), json!({"action":"write_catalogue","records":[],"revision":1,"now":1})).unwrap();
    assert!(written.wrote);
    assert_eq!(written.generation_key.as_deref(), Some("local_layer_store.catalogue"));
    let parked = outcome(&connection, &alice(), json!({"action":"put_features","layer_id":"a","features":"[]","now":1})).unwrap();
    assert_eq!(parked.generation_key.as_deref(), Some("local_layer_store.payloads"));
    assert!(!outcome(&connection, &alice(), json!({"action":"read"})).unwrap().wrote);
}

#[test]
fn a_payload_is_one_bounded_json_array_parked_under_its_record_id() {
    let connection = db();
    ok(&connection, &alice(), json!({"action":"put_features","layer_id":"a","features":features(3),"now":1}));
    assert_eq!(features_of(&connection, &alice(), "a"), features(3));
    assert!(features_of(&connection, &bob(), "a").is_null());
    for bad in ["{}", "[1,2", "[] []", "not json"] {
        let error = run(&connection, &alice(), json!({"action":"put_features","layer_id":"a","features":bad,"now":1})).unwrap_err();
        assert!(error.starts_with(INVALID), "{bad}: {error}");
    }
    // A carried row keeps its payload under its id, whatever the id says.
    ok(&connection, &alice(), json!({"action":"put_features","layer_id":"a/b ","features":"[]","now":1}));
    assert_eq!(ok(&connection, &alice(), json!({"action":"delete_features","layer_ids":["a","a/b ","missing"]}))["deleted"], 2);
    assert!(features_of(&connection, &alice(), "a").is_null());
}

#[test]
fn a_snapshot_commits_catalogue_and_payload_together_or_neither() {
    let mut connection = db();
    let transaction = connection.transaction().unwrap();
    let refused = run(
        &transaction,
        &alice(),
        json!({"action":"put_snapshot","records":[layer("a")],"revision":1,"layer_id":"a","features":"{","now":1}),
    );
    assert!(refused.is_err());
    drop(transaction);
    assert!(ok(&connection, &alice(), json!({"action":"read"}))["records"].is_null());
    ok(&connection, &alice(), json!({"action":"put_snapshot","records":[layer("a")],"revision":1,"layer_id":"a","features":features(2),"now":1}));
    assert_eq!(ok(&connection, &alice(), json!({"action":"read"}))["records"], json!([layer("a")]));
    assert_eq!(features_of(&connection, &alice(), "a"), features(2));
}

#[test]
fn a_catalogue_is_bounded_like_a_reopen_and_takes_no_bytes() {
    let connection = db();
    let too_many: Vec<Value> = (0..=MAX_RECORDS).map(|i| layer(&format!("l{i}"))).collect();
    assert!(run(&connection, &alice(), json!({"action":"write_catalogue","records":too_many,"revision":1,"now":1})).is_err());
    assert!(run(&connection, &alice(), json!({"action":"write_catalogue","records":[1],"revision":1,"now":1})).is_err());
    let identity = alice();
    let bytes = execute(&connection, Input { identity: &identity, bytes: Some(vec![1]) }, serde_json::from_value(json!({"action":"read"})).unwrap());
    assert!(bytes.unwrap_err().starts_with(INVALID));
}

const T1: &str = "11111111111111111111111111111111";
const T2: &str = "22222222222222222222222222222222";
const T3: &str = "33333333333333333333333333333333";
const T9: &str = "99999999999999999999999999999999";

fn connect(connection: &Connection, key: &str, token: &str) {
    let identity = alice();
    crate::folder_connections::execute(
        connection,
        Input { identity: &identity, bytes: None },
        serde_json::from_value(json!({"action":"connect","project":"kigali","logical_key":key,"folder_name":"F","token":token,"now":1})).unwrap(),
    )
    .unwrap();
}

#[test]
fn the_sweep_removes_orphan_payloads_and_unreferenced_gis_folders_only() {
    let connection = db();
    ok(&connection, &alice(), json!({"action":"write_catalogue","records":[gis_layer("a","gisFolder:live"), {"id":"carried/id"}],"revision":1,"now":1}));
    for id in ["a", "carried/id", "gone"] {
        ok(&connection, &alice(), json!({"action":"put_features","layer_id":id,"features":"[]","now":1}));
    }
    connect(&connection, "gisFolder:live", T1);
    connect(&connection, "gisFolder:stale", T2);
    connect(&connection, "plsFolder:kigali:model", T3);
    let swept = ok(&connection, &alice(), json!({"action":"sweep","present_tokens":[T1, T2, T3, T9]}));
    assert_eq!(swept["orphan_payloads"], 1);
    assert_eq!(swept["stale_connections"], 1);
    assert_eq!(swept["drop"], json!([T2, T9]));
    assert!(features_of(&connection, &alice(), "gone").is_null());
    assert_eq!(features_of(&connection, &alice(), "carried/id"), "[]");
    let clean = ok(&connection, &alice(), json!({"action":"sweep"}));
    assert_eq!(clean, json!({"orphan_payloads": 0, "stale_connections": 0, "drop": []}));
}

fn entry(key: &str, value: Value) -> Value {
    json!({"key": key, "value": value})
}

#[test]
fn the_import_merges_the_retired_catalogue_and_its_payloads_once_with_a_receipt() {
    let connection = db();
    let legacy = ok(&connection, &alice(), json!({"action":"legacy"}));
    assert_eq!(legacy["import_needed"], true);
    assert_eq!(
        legacy["legacy"],
        json!({"database":"ds-web","stores":[{"store":"app","keys":["sketchLayersMeta","sketchLayers"],"prefixes":["sketchLayerFeatures:"]}]})
    );
    // Work made before the import finished stays first and is never replaced.
    ok(&connection, &alice(), json!({"action":"write_catalogue","records":[layer("new")],"revision":4,"now":1}));
    ok(&connection, &alice(), json!({"action":"put_features","layer_id":"new","features":features(1),"now":1}));
    let catalogue = json!([layer("old"), layer("new"), {"name":"no id"}]).to_string();
    let first = ok(&connection, &alice(), json!({"action":"import_entries","now":5,"entries":[entry("sketchLayersMeta", json!(catalogue))]}));
    assert_eq!(first, json!({"written": 1, "skipped": 0}));
    let second = ok(&connection, &alice(), json!({"action":"import_entries","now":5,"entries":[
        entry("sketchLayerFeatures:old", json!(features(2))),
        entry("sketchLayerFeatures:new", json!("[]")),
        entry("sketchLayerFeatures:orphan", json!("[]")),
        entry("sketchLayerFeatures:bad", json!({"not": "text"})),
        entry("unrelated", json!("x")),
    ]}));
    assert_eq!(second, json!({"written": 2, "skipped": 3}));
    assert_eq!(
        ok(&connection, &alice(), json!({"action":"read"}))["records"],
        json!([layer("new"), layer("old"), {"name":"no id"}])
    );
    let finished = ok(&connection, &alice(), json!({"action":"import_finish","now":6}));
    assert_eq!(finished["imported"], true);
    let receipt = &finished["receipt"];
    assert_eq!(receipt["store"], STORE);
    assert_eq!(receipt["source"], LEGACY_SOURCE);
    // The orphan payload was written, then pruned: it counts as skipped.
    assert_eq!(receipt["count"], 2);
    assert_eq!(receipt["skipped"], 4);
    assert_eq!(receipt["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(features_of(&connection, &alice(), "new"), features(1));
    assert_eq!(features_of(&connection, &alice(), "old"), features(2));
    assert!(features_of(&connection, &alice(), "orphan").is_null());
    assert_eq!(ok(&connection, &alice(), json!({"action":"legacy"}))["import_needed"], false);
    // It never runs again, and each identity imports on its own.
    let late = run(&connection, &alice(), json!({"action":"import_entries","now":7,"entries":[entry("sketchLayersMeta", json!("[]"))]}));
    assert!(late.unwrap_err().starts_with(crate::import_batches::INVALID));
    let again = ok(&connection, &alice(), json!({"action":"import_finish","now":8}));
    assert_eq!(again["imported"], false);
    assert_eq!(again["receipt"], *receipt);
    assert_eq!(ok(&connection, &bob(), json!({"action":"legacy"}))["import_needed"], true);
    let empty = ok(&connection, &bob(), json!({"action":"import_finish","now":9}));
    assert_eq!(empty["receipt"]["count"], 0);
}

#[test]
fn the_oldest_inline_shape_parks_each_layers_features_under_its_reopened_id() {
    let connection = db();
    let legacy = json!([
        {"id": "named", "name": "Named", "geometryType": "Point", "features": [{"id":"x"}]},
        {"name": "Anonymous", "geometryType": "Point", "features": [{"id":"y"},{"id":"z"}]},
    ])
    .to_string();
    ok(&connection, &alice(), json!({"action":"import_entries","now":1700000000000u64,"entries":[entry("sketchLayers", json!(legacy))]}));
    let records = ok(&connection, &alice(), json!({"action":"read"}))["records"].clone();
    let ids: Vec<&str> = records.as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["named", "sketch-1700000000000-1"]);
    assert!(records.as_array().unwrap().iter().all(|record| record.get("features").is_none()));
    assert_eq!(features_of(&connection, &alice(), "named"), r#"[{"id":"x"}]"#);
    assert_eq!(features_of(&connection, &alice(), "sketch-1700000000000-1"), r#"[{"id":"y"},{"id":"z"}]"#);
    // The browser read the oldest key only when no catalogue key existed.
    let both = db();
    ok(&both, &alice(), json!({"action":"import_entries","now":1,"entries":[
        entry("sketchLayersMeta", json!(json!([layer("meta")]).to_string())),
        entry("sketchLayers", json!(legacy)),
    ]}));
    assert_eq!(ok(&both, &alice(), json!({"action":"read"}))["records"], json!([layer("meta")]));
}
