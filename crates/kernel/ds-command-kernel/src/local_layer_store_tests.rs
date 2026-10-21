use super::*;
use crate::durable_store::{Identity, Principal};

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

#[test]
fn the_sweep_removes_only_orphan_payloads() {
    let connection = db();
    ok(&connection, &alice(), json!({"action":"write_catalogue","records":[layer("a"), {"id":"carried/id"}],"revision":1,"now":1}));
    for id in ["a", "carried/id", "gone"] {
        ok(&connection, &alice(), json!({"action":"put_features","layer_id":id,"features":"[]","now":1}));
    }
    let swept = ok(&connection, &alice(), json!({"action":"sweep"}));
    assert_eq!(swept["orphan_payloads"], 1);
    assert!(features_of(&connection, &alice(), "a").is_string());
    assert!(features_of(&connection, &alice(), "gone").is_null());
    assert_eq!(features_of(&connection, &alice(), "carried/id"), "[]");
    let clean = ok(&connection, &alice(), json!({"action":"sweep"}));
    assert_eq!(clean, json!({"orphan_payloads": 0}));
}
