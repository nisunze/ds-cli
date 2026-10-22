//! A photo is cached under its path AND what it shows: the original's
//! generation and the rotation a derived image carries.
use super::*;
use crate::durable_store::{self, Call, Identity, Principal};
use rusqlite::Connection;
use serde_json::{Value, json};

const THUMB: &str = "kigali/forms/poles/e1/photo_thunder.jpeg";

fn run(db: &mut Connection, operation: Value, bytes: Option<Vec<u8>>) -> (Value, Option<Vec<u8>>) {
    let identity = Identity {
        principal: Principal::Account("a".into()),
        lane: "canary".into(),
        audience: "a".repeat(64),
    };
    let call: Call =
        serde_json::from_value(json!({"request": {"store": "byte_cache", "operation": operation}}))
            .unwrap();
    let answer = durable_store::execute(db, Some(&identity), call, bytes).unwrap();
    let bytes = answer.bytes.clone();
    (serde_json::to_value(&answer).unwrap(), bytes)
}

fn put(db: &mut Connection, bytes: &[u8], meta: Value) {
    run(
        db,
        json!({"action": "put", "key": {"kind": "photo", "key": THUMB}, "payload": {"byte_length": bytes.len()},
            "media_type": "image/jpeg", "meta": meta, "now": 5}),
        Some(bytes.to_vec()),
    );
}

fn get(db: &mut Connection, identity: Value) -> (Value, Option<Vec<u8>>) {
    run(
        db,
        json!({"action": "get", "key": {"kind": "photo", "key": THUMB}, "expect_identity": identity}),
        None,
    )
}

#[test]
fn a_stale_orientation_or_generation_is_a_miss_never_shown() {
    let mut db = Connection::open_in_memory().unwrap();
    put(
        &mut db,
        b"upright",
        json!({"generation": "1729", "quarter_turns": 0}),
    );
    let (hit, bytes) = get(&mut db, json!({"generation": "1729", "quarter_turns": 0}));
    assert_eq!(
        (hit["hit"].clone(), bytes.unwrap()),
        (json!(true), b"upright".to_vec())
    );

    // The entry now records a quarter-turn: the upright copy is stale.
    let (stale, bytes) = get(&mut db, json!({"generation": "1729", "quarter_turns": 1}));
    assert_eq!(
        (stale["hit"].clone(), stale["changed"].clone(), bytes),
        (json!(false), json!(true), None)
    );
    // Another generation of the original is another photo.
    assert_eq!(get(&mut db, json!({"generation": "1730"})).0["hit"], false);

    put(
        &mut db,
        b"turned",
        json!({"generation": "1729", "quarter_turns": 1}),
    );
    let (fresh, bytes) = get(&mut db, json!({"generation": "1729", "quarter_turns": 1}));
    assert_eq!(
        (fresh["hit"].clone(), bytes.unwrap()),
        (json!(true), b"turned".to_vec())
    );
    // A legacy entry without identity meta is upright and of no named generation.
    put(&mut db, b"legacy", json!({}));
    assert_eq!(get(&mut db, json!({"quarter_turns": 0})).0["hit"], true);
    assert_eq!(get(&mut db, json!({"quarter_turns": 2})).0["hit"], false);
    assert_eq!(get(&mut db, json!({"generation": "1729"})).0["hit"], false);
}
