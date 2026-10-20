use super::*;
use crate::durable_store::{self, Call, Identity, Principal};
use rusqlite::Connection;
use serde_json::{json, Value};

const PATH: &str = "kigali/forms/poles/e1/photo.jpg";
const THUMB: &str = "kigali/forms/poles/e1/photo_thunder.jpeg";
const ORIGINAL: &[u8] = b"original-jpeg";
const COMPANION: &[u8] = b"thumb";

fn identity(uid: Option<&str>) -> Identity {
    Identity {
        principal: uid.map_or(Principal::Anonymous, |uid| Principal::Account(uid.into())),
        lane: "canary".into(),
        audience: "a".repeat(64),
    }
}
fn call(db: &mut Connection, store: &str, uid: Option<&str>, operation: Value, bytes: Option<Vec<u8>>) -> Result<(Value, Option<Vec<u8>>), String> {
    let call: Call = serde_json::from_value(json!({"request": {"store": store, "operation": operation}}))
        .map_err(|e| e.to_string())?;
    let answer = durable_store::execute(db, Some(&identity(uid)), call, bytes)?;
    let bytes = answer.bytes.clone();
    Ok((serde_json::to_value(&answer).unwrap(), bytes))
}
fn run(db: &mut Connection, uid: Option<&str>, operation: Value, bytes: Option<Vec<u8>>) -> Result<Value, String> {
    call(db, STORE, uid, operation, bytes).map(|(answer, _)| answer)
}
fn admit(db: &mut Connection, uid: Option<&str>, path: &str) -> Result<Value, String> {
    let companion = crate::survey_photo_outbox::thumbnail_path_for(path).unwrap();
    let bytes = [ORIGINAL, COMPANION].concat();
    run(
        db,
        uid,
        json!({"action": "admit", "project": "kigali", "path": path, "surface": "survey-media", "owner_ref": "entry-1",
            "content_type": "image/jpeg", "size": ORIGINAL.len(),
            "companion": {"path": companion, "content_type": "image/jpeg", "size": COMPANION.len()},
            "generation": "gen-00000001", "payload": {"byte_length": bytes.len()}, "now": 10}),
        Some(bytes),
    )
}
fn claim(db: &mut Connection, now: u64) -> Value {
    run(db, Some("a"), json!({"action": "claim", "online": true, "now": now}), None).unwrap()
}
fn photo(db: &mut Connection, path: &str) -> (Value, Option<Vec<u8>>) {
    call(db, "byte_cache", Some("a"), json!({"action": "get", "key": {"kind": "photo", "key": path}}), None).unwrap()
}
fn settle(db: &mut Connection, lease: u64, outcome: Value, now: u64) -> Value {
    run(
        db,
        Some("a"),
        json!({"action": "settle", "path": PATH, "generation": "gen-00000001", "lease": lease, "outcome": outcome, "now": now}),
        None,
    )
    .unwrap()
}

#[test]
fn a_pending_upload_survives_a_failure_and_settles_into_the_cache() {
    let mut db = Connection::open_in_memory().unwrap();
    let admitted = admit(&mut db, Some("a"), PATH).unwrap();
    assert_eq!(admitted["row"]["status"], "pending");
    assert_eq!(admitted["row"]["scope"]["account_id"], "a", "the bound account, never the caller's word");
    let (pending, bytes) = photo(&mut db, PATH);
    assert_eq!((pending["pending_upload"].clone(), bytes.unwrap()), (json!(true), ORIGINAL.to_vec()));
    assert_eq!(photo(&mut db, THUMB).1.unwrap(), COMPANION, "the companion reads like any photo");

    let dispatch = claim(&mut db, 20)["dispatch"][0].clone();
    assert_eq!(dispatch["lease"], 1);
    assert_eq!(dispatch["targets"][0]["sha256"], crate::durable_store::sha256(ORIGINAL));
    assert_eq!(dispatch["targets"][1]["target"], "companion");
    let (_, read) = call(
        &mut db,
        STORE,
        Some("a"),
        json!({"action": "read", "path": PATH, "generation": "gen-00000001", "target": "companion", "offset": 0, "length": COMPANION.len()}),
        None,
    )
    .unwrap();
    assert_eq!(read.unwrap(), COMPANION);

    let failed = settle(&mut db, 1, json!({"kind": "transport_failure", "status": 503, "has_session": true}), 30);
    assert_eq!(failed["row"]["status"], "pending");
    assert!(failed["wake_at"].as_u64().unwrap() > 30);
    assert_eq!(photo(&mut db, PATH).1.unwrap(), ORIGINAL, "a failed attempt keeps every byte");
    assert_eq!(claim(&mut db, 31)["dispatch"], json!([]), "the retry waits for its deadline");

    let retried = claim(&mut db, 1_000_000)["dispatch"][0].clone();
    assert_eq!(retried["lease"], 2);
    assert_eq!(settle(&mut db, 2, json!({"kind": "success"}), 1_000_001)["synced"], true);
    let counts = run(&mut db, Some("a"), json!({"action": "counts"}), None).unwrap();
    assert_eq!((counts["pending"].clone(), counts["uploading"].clone()), (json!(0), json!(0)));
    let (cached, bytes) = photo(&mut db, PATH);
    assert_eq!((cached["pending_upload"].clone(), bytes.unwrap()), (json!(false), ORIGINAL.to_vec()));
    assert_eq!(photo(&mut db, THUMB).1.unwrap(), COMPANION);
}

#[test]
fn an_abandoned_lease_recovers_and_a_late_result_settles_nothing() {
    let mut db = Connection::open_in_memory().unwrap();
    admit(&mut db, Some("a"), PATH).unwrap();
    assert_eq!(claim(&mut db, 20)["dispatch"][0]["lease"], 1);
    let in_flight = run(&mut db, Some("a"), json!({"action": "claim", "online": true, "in_flight": [PATH], "now": 21}), None).unwrap();
    assert_eq!(in_flight["dispatch"], json!([]), "a live transfer is never claimed twice");
    assert_eq!(claim(&mut db, 22)["dispatch"][0]["lease"], 2, "a crashed lease is reclaimed");
    assert_eq!(settle(&mut db, 1, json!({"kind": "success"}), 23)["settled"], false);
    assert_eq!(photo(&mut db, PATH).0["pending_upload"], true);
}

#[test]
fn corrupt_pending_bytes_are_never_sent_and_never_dropped() {
    let mut db = Connection::open_in_memory().unwrap();
    admit(&mut db, Some("a"), PATH).unwrap();
    db.execute("UPDATE media_outbox_parts SET bytes=zeroblob(length(bytes))", []).unwrap();
    assert_eq!(claim(&mut db, 20)["dispatch"], json!([]));
    let failed = run(&mut db, Some("a"), json!({"action": "failed"}), None).unwrap();
    assert_eq!(failed["rows"][0]["status"], "failed");
    let parts: i64 = db
        .query_row("SELECT COUNT(*) FROM media_outbox_parts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(parts, 1, "work bytes stay for a person to recover");
}

#[test]
fn only_a_signed_in_account_queues_and_a_path_is_pending_once() {
    let mut db = Connection::open_in_memory().unwrap();
    assert!(admit(&mut db, None, PATH).unwrap_err().starts_with(REFUSED));
    admit(&mut db, Some("a"), PATH).unwrap();
    assert!(admit(&mut db, Some("a"), PATH).unwrap_err().contains("already pending"));
    assert!(admit(&mut db, Some("a"), THUMB).is_err(), "a pending companion is a pending path too");
    assert_eq!(run(&mut db, Some("b"), json!({"action": "counts"}), None).unwrap()["pending"], 0);
    let short = json!({"action": "admit", "project": "kigali", "path": PATH, "surface": "survey-media", "owner_ref": "e",
        "size": 4, "generation": "gen-00000002", "payload": {"byte_length": 5}, "now": 1});
    assert!(run(&mut db, Some("b"), short, Some(vec![0; 5])).unwrap_err().starts_with(INVALID));
    let huge = json!({"action": "admit", "project": "kigali", "path": PATH, "surface": "survey-media", "owner_ref": "e",
        "size": MAX_MEDIA_BYTES + 1, "generation": "gen-00000003", "payload": {"byte_length": MAX_MEDIA_BYTES + 1}, "now": 1});
    assert!(run(&mut db, Some("b"), huge, Some(vec![0; 1])).is_err());
}
