use super::*;
use crate::durable_store::{self, Call, Identity, Principal};
use rusqlite::Connection;
use serde_json::{json, Value};

fn identity(uid: Option<&str>) -> Identity {
    Identity {
        principal: uid.map_or(Principal::Anonymous, |uid| Principal::Account(uid.into())),
        lane: "canary".into(),
        audience: "a".repeat(64),
    }
}
fn run(db: &mut Connection, uid: Option<&str>, operation: Value, bytes: Option<Vec<u8>>) -> Result<(Value, Option<Vec<u8>>), String> {
    let call: Call = serde_json::from_value(json!({"request": {"store": "upload_chunks", "operation": operation}}))
        .map_err(|e| e.to_string())?;
    let answer = durable_store::execute(db, Some(&identity(uid)), call, bytes)?;
    let bytes = answer.bytes.clone();
    Ok((serde_json::to_value(&answer).unwrap(), bytes))
}
fn key(file: &str, layer: &str, chunk: u64) -> Value {
    json!({"workspace": "temporary-upload:1", "file": file, "layer": layer, "chunk": chunk})
}
fn put(db: &mut Connection, uid: Option<&str>, key: Value, bytes: &[u8]) -> Result<Value, String> {
    let operation = json!({"action": "put", "key": key, "format": FORMAT, "feature_count": 3, "payload": {"byte_length": bytes.len()}, "now": 1});
    run(db, uid, operation, Some(bytes.to_vec())).map(|(answer, _)| answer)
}
fn get(db: &mut Connection, uid: Option<&str>, key: Value) -> (Value, Option<Vec<u8>>) {
    run(db, uid, json!({"action": "get", "key": key}), None).unwrap()
}

#[test]
fn chunks_round_trip_in_order_and_clear_by_layer_then_workspace() {
    let mut db = Connection::open_in_memory().unwrap();
    put(&mut db, None, key("roads.gpkg", "roads", 1), b"second").unwrap();
    put(&mut db, None, key("roads.gpkg", "roads", 0), b"first").unwrap();
    put(&mut db, None, key("roads.gpkg", "poles", 0), b"poles").unwrap();
    put(&mut db, None, key("roads.gpkg", "roads", 0), b"FIRST").unwrap();
    let (held, bytes) = get(&mut db, None, key("roads.gpkg", "roads", 0));
    assert_eq!((held["held"].clone(), held["byte_size"].clone(), bytes.unwrap()), (json!(true), json!(5), b"FIRST".to_vec()));
    let listed = run(&mut db, None, json!({"action": "list", "workspace": "temporary-upload:1", "file": "roads.gpkg", "layer": "roads"}), None)
        .unwrap()
        .0;
    let chunks: Vec<u64> = listed["chunks"].as_array().unwrap().iter().map(|chunk| chunk["chunk"].as_u64().unwrap()).collect();
    assert_eq!(chunks, vec![0, 1]);
    assert_eq!(get(&mut db, Some("a"), key("roads.gpkg", "roads", 0)).0["held"], false, "another identity never reads it");
    let cleared = run(&mut db, None, json!({"action": "clear_layer", "workspace": "temporary-upload:1", "file": "roads.gpkg", "layer": "roads"}), None)
        .unwrap()
        .0;
    assert_eq!(cleared["cleared"], 2);
    assert_eq!(get(&mut db, None, key("roads.gpkg", "poles", 0)).0["held"], true);
    run(&mut db, None, json!({"action": "clear_workspace", "workspace": "temporary-upload:1"}), None).unwrap();
    let blobs: i64 = db
        .query_row("SELECT COUNT(*) FROM upload_chunks_blobs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(blobs, 0, "a clear removes the bytes with the rows");
}

#[test]
fn damaged_chunk_bytes_are_reported_never_dropped() {
    let mut db = Connection::open_in_memory().unwrap();
    put(&mut db, None, key("a.geojson", "", 0), b"arrow").unwrap();
    db.execute("UPDATE upload_chunks_parts SET bytes=X'0000000000'", []).unwrap();
    let (answer, bytes) = get(&mut db, None, key("a.geojson", "", 0));
    assert_eq!((answer["damaged"].clone(), bytes), (json!(true), None));
    assert_eq!(get(&mut db, None, key("a.geojson", "", 0)).0["held"], true);
    let wrong = json!({"action": "put", "key": key("a.geojson", "", 1), "format": "geojson", "feature_count": 1, "payload": {"byte_length": 1}, "now": 1});
    assert!(run(&mut db, None, wrong, Some(vec![1])).unwrap_err().starts_with(INVALID));
    assert!(put(&mut db, None, json!({"workspace": "", "file": "a", "layer": "", "chunk": 0}), b"x").is_err());
}

#[test]
fn a_sketch_layers_chunk_keys_survive_the_import_once() {
    let mut db = Connection::open_in_memory().unwrap();
    assert_eq!(run(&mut db, None, json!({"action": "legacy"}), None).unwrap().0["import_needed"], true);
    let record = json!({"workspaceKey": "temporary-upload:7", "fileName": "trace.kml", "parsedLayer": "Lines", "chunkIndex": 2,
        "format": "arrow-ipc-v1", "featureCount": 40, "byteSize": 6});
    let entry = json!({"action": "import_entry", "record": record, "payload": {"byte_length": 6}, "now": 3});
    assert_eq!(run(&mut db, None, entry.clone(), Some(b"arrow!".to_vec())).unwrap().0["written"], 1);
    let inline = json!({"action": "import_entry", "record": {"workspaceKey": "w", "fileName": "f", "parsedLayer": "", "chunkIndex": 0, "featureCount": 1, "features": []}, "now": 3});
    assert_eq!(run(&mut db, None, inline, None).unwrap().0["skipped"], 1, "inline feature arrays carry no chunk bytes");
    let finished = run(&mut db, None, json!({"action": "import_finish", "now": 4}), None).unwrap().0;
    assert_eq!(finished["receipt"]["count"], 1);
    assert!(run(&mut db, None, entry, Some(b"arrow!".to_vec())).is_err(), "once per identity");
    let stored = json!({"workspace": "temporary-upload:7", "file": "trace.kml", "layer": "Lines", "chunk": 2});
    let (held, bytes) = get(&mut db, None, stored);
    assert_eq!((held["feature_count"].clone(), bytes.unwrap()), (json!(40), b"arrow!".to_vec()));
}
