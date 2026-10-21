use super::*;
use crate::durable_store::{self, Call, Identity, Principal};
use rusqlite::Connection;
use serde_json::{json, Value};

fn identity(uid: &str) -> Identity {
    Identity {
        principal: Principal::Account(uid.into()),
        lane: "canary".into(),
        audience: "a".repeat(64),
    }
}
fn run(db: &mut Connection, uid: &str, operation: Value, bytes: Option<Vec<u8>>) -> Result<(Value, Option<Vec<u8>>), String> {
    let call: Call = serde_json::from_value(json!({"request": {"store": "byte_cache", "operation": operation}}))
        .map_err(|e| e.to_string())?;
    let answer = durable_store::execute(db, Some(&identity(uid)), call, bytes)?;
    let bytes = answer.bytes.clone();
    Ok((serde_json::to_value(&answer).unwrap(), bytes))
}
fn key(kind: &str, key: &str) -> Value {
    json!({"kind": kind, "key": key})
}
fn put(db: &mut Connection, uid: &str, key: Value, bytes: &[u8], sha256: Option<String>) -> Result<Value, String> {
    let operation = json!({"action": "put", "key": key, "payload": {"byte_length": bytes.len()}, "sha256": sha256, "media_type": "image/jpeg", "now": 5});
    run(db, uid, operation, Some(bytes.to_vec())).map(|(answer, _)| answer)
}
fn get(db: &mut Connection, uid: &str, key: Value) -> (Value, Option<Vec<u8>>) {
    run(db, uid, json!({"action": "get", "key": key}), None).unwrap()
}
fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))
        .unwrap()
}

#[test]
fn bytes_round_trip_for_their_owner_and_never_cross_owners() {
    let mut db = Connection::open_in_memory().unwrap();
    let photo = key("photo", "kigali/forms/poles/e1/photo.jpg");
    let stored = put(&mut db, "a", photo.clone(), b"jpeg", None).unwrap();
    assert_eq!(stored["stored"], true);
    assert_eq!(stored["generation"], 1);
    let (answer, bytes) = get(&mut db, "a", photo.clone());
    assert_eq!((answer["hit"].clone(), bytes.unwrap()), (json!(true), b"jpeg".to_vec()));
    assert_eq!(answer["version"], stored["sha256"]);
    assert_eq!(answer["media_type"], "image/jpeg");
    assert_eq!(get(&mut db, "b", photo.clone()).0["hit"], false, "another account never reads it");
    let deleted = run(&mut db, "a", json!({"action": "delete", "key": photo.clone()}), None).unwrap().0;
    assert_eq!(deleted["deleted"], true);
    assert_eq!(get(&mut db, "a", photo).0["hit"], false);
    assert_eq!(count(&db, "byte_cache_blobs"), 0, "content no entry names is gone");
}

#[test]
fn a_declared_digest_must_match_and_a_mismatch_keeps_nothing() {
    let mut db = Connection::open_in_memory().unwrap();
    let version = key("grid_remote_version", "kigali/model-1/rev-1");
    let refused = put(&mut db, "a", version.clone(), b"dsgrid", Some("0".repeat(64))).unwrap_err();
    assert!(refused.starts_with(INVALID), "{refused}");
    let held: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='byte_cache_blobs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(held, 0, "the refused put rolled back even its schema step");
    assert_eq!(get(&mut db, "a", version.clone()).0["hit"], false);
    let digest = crate::durable_store::sha256(b"dsgrid");
    assert_eq!(put(&mut db, "a", version.clone(), b"dsgrid", Some(digest.clone())).unwrap()["sha256"], digest);
    assert_eq!(get(&mut db, "a", version).1.unwrap(), b"dsgrid");
}

#[test]
fn a_corrupt_part_drops_its_entries_and_answers_a_miss() {
    let mut db = Connection::open_in_memory().unwrap();
    let artifact = key("compute_artifact", "kigali/report.pdf");
    put(&mut db, "a", artifact.clone(), b"%PDF", None).unwrap();
    let has = |db: &mut Connection, length: u64| {
        run(db, "a", json!({"action": "has", "key": artifact.clone(), "byte_length": length}), None)
            .unwrap()
            .0["has"]
            .clone()
    };
    assert_eq!(has(&mut db, 4), true);
    assert_eq!(has(&mut db, 5), false);
    db.execute("UPDATE byte_cache_parts SET bytes=X'25504447'", []).unwrap();
    assert_eq!(has(&mut db, 4), true, "a presence check compares lengths and reads no bytes");
    let (miss, bytes) = get(&mut db, "a", artifact.clone());
    assert_eq!((miss["hit"].clone(), miss["dropped"].clone(), bytes), (json!(false), json!(true), None));
    assert_eq!(count(&db, "byte_cache_entries"), 0);
    assert_eq!(count(&db, "byte_cache_parts"), 0);
    assert_eq!(has(&mut db, 4), false);
}

#[test]
fn content_is_shared_within_an_owner_only() {
    let mut db = Connection::open_in_memory().unwrap();
    put(&mut db, "a", key("photo", "one.jpg"), b"same", None).unwrap();
    put(&mut db, "a", key("report_preview", "kigali/a_1"), b"same", None).unwrap();
    assert_eq!(count(&db, "byte_cache_blobs"), 1);
    put(&mut db, "b", key("photo", "one.jpg"), b"same", None).unwrap();
    assert_eq!(count(&db, "byte_cache_blobs"), 2, "owners never share content");
    run(&mut db, "a", json!({"action": "delete", "key": key("photo", "one.jpg")}), None).unwrap();
    assert_eq!(get(&mut db, "a", key("report_preview", "kigali/a_1")).1.unwrap(), b"same");
}

#[test]
fn a_kind_past_its_budget_loses_its_oldest_entries_first() {
    let mut db = Connection::open_in_memory().unwrap();
    for (name, now) in [("old.jpg", 1), ("mid.jpg", 2), ("new.jpg", 3)] {
        let operation = json!({"action": "put", "key": key("photo", name), "payload": {"byte_length": 2}, "now": now});
        run(&mut db, "a", operation, Some(name.as_bytes()[..2].to_vec())).unwrap();
    }
    let owner = identity("a").owner();
    let keep = Key {
        kind: Kind::Photo,
        key: "new.jpg".into(),
    };
    assert_eq!(keep_within(&db, &owner, &keep, 4).unwrap(), 1);
    assert_eq!(get(&mut db, "a", key("photo", "old.jpg")).0["hit"], false);
    assert_eq!(get(&mut db, "a", key("photo", "mid.jpg")).0["hit"], true);
    assert_eq!(keep_within(&db, &owner, &keep, 0).unwrap(), 1, "the entry just written stays");
    assert_eq!(get(&mut db, "a", key("photo", "new.jpg")).0["hit"], true);
}

#[test]
fn a_payload_over_one_part_puts_and_reads_part_by_part() {
    let mut db = Connection::open_in_memory().unwrap();
    let length = PART_BYTES + 5;
    let whole: Vec<u8> = (0..length).map(|at| (at % 241) as u8).collect();
    let (head, tail) = whole.split_at(PART_BYTES as usize);
    let version = key("grid_remote_version", "kigali/model-1/rev-2");
    let digest = crate::durable_store::sha256(&whole);
    let first = json!({"action": "put", "key": version.clone(), "payload": {"byte_length": length}, "sha256": digest, "now": 1});
    let staged = run(&mut db, "a", first, Some(head.to_vec())).unwrap().0;
    assert_eq!(staged["staged"]["parts"], 2);
    assert_eq!(get(&mut db, "a", version.clone()).0["hit"], false, "nothing is visible before the last part");
    let last = json!({"action": "put", "key": version.clone(), "payload": {"byte_length": length, "part": 1, "staging": staged["staged"]["staging"]}, "sha256": digest, "now": 2});
    assert_eq!(run(&mut db, "a", last, Some(tail.to_vec())).unwrap().0["stored"], true);
    let (first_part, head_bytes) = get(&mut db, "a", version.clone());
    assert_eq!((first_part["parts"].clone(), head_bytes.unwrap().len() as u64), (json!(2), PART_BYTES));
    let later = |expect: &str| json!({"action": "get", "key": version.clone(), "part": 1, "expect_version": expect});
    let (_, tail_bytes) = run(&mut db, "a", later(&digest), None).unwrap();
    assert_eq!(tail_bytes.unwrap(), tail);
    assert_eq!(run(&mut db, "a", later("other"), None).unwrap().0["changed"], true);
}

#[test]
fn malformed_calls_are_refused_before_any_write() {
    let mut db = Connection::open_in_memory().unwrap();
    let past = json!({"action": "put", "key": key("report_preview", "kigali/a_2"), "payload": {"byte_length": Kind::ReportPreview.max_entry() + 1}, "now": 1});
    assert!(run(&mut db, "a", past, Some(vec![1])).is_err(), "past the kind's bound");
    assert!(put(&mut db, "a", key("photo", ""), b"x", None).is_err(), "an empty key");
    assert!(put(&mut db, "a", key("photo", &"k".repeat(MAX_KEY_BYTES + 1)), b"x", None).is_err());
    assert!(put(&mut db, "a", key("sketch", "x"), b"x", None).is_err(), "a closed kind list");
    let meta = json!({"action": "put", "key": key("report_preview", "kigali/a_3"), "payload": {"byte_length": 1}, "meta": [1], "now": 1});
    assert!(run(&mut db, "a", meta, Some(vec![1])).is_err(), "meta is an object");
    assert!(run(&mut db, "a", json!({"action": "get", "key": key("photo", "x")}), Some(vec![1])).is_err());
    let limits = run(&mut db, "a", json!({"action": "limits"}), None).unwrap().0;
    assert_eq!(limits["part_bytes"], PART_BYTES);
}
