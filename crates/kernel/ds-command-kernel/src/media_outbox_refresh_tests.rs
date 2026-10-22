//! Rotation is metadata: the outbox never replaces an original, and a
//! regenerated thumbnail travels alone as a refresh of a stored original.
use super::*;
use crate::durable_store::{self, Call, Identity, Principal};
use rusqlite::Connection;
use serde_json::{Value, json};

const PATH: &str = "kigali/forms/poles/e1/photo.jpg";
const THUMB: &str = "kigali/forms/poles/e1/photo_thunder.jpeg";

fn run(db: &mut Connection, operation: Value, bytes: Option<Vec<u8>>) -> Result<Value, String> {
    let identity = Identity {
        principal: Principal::Account("a".into()),
        lane: "canary".into(),
        audience: "a".repeat(64),
    };
    let call: Call =
        serde_json::from_value(json!({"request": {"store": STORE, "operation": operation}}))
            .map_err(|e| e.to_string())?;
    durable_store::execute(db, Some(&identity), call, bytes)
        .map(|answer| serde_json::to_value(&answer).unwrap())
}

fn refresh(
    db: &mut Connection,
    bytes: &[u8],
    turns: u8,
    generation: &str,
) -> Result<Value, String> {
    run(
        db,
        json!({"action": "admit", "project": "kigali", "path": THUMB, "surface": "survey-media", "owner_ref": "entry-1",
            "content_type": "image/jpeg", "size": bytes.len(), "expected_generation": "77",
            "source": {"object_path": PATH, "generation": "1729", "quarter_turns": turns},
            "generation": generation, "payload": {"byte_length": bytes.len()}, "now": 10}),
        Some(bytes.to_vec()),
    )
}

#[test]
fn a_queued_replacement_of_an_original_is_refused() {
    let mut db = Connection::open_in_memory().unwrap();
    let error = run(
        &mut db,
        json!({"action": "admit", "project": "kigali", "path": PATH, "surface": "survey-media", "owner_ref": "entry-1",
            "content_type": "image/jpeg", "size": 4, "expected_generation": "1729",
            "generation": "gen-00000001", "payload": {"byte_length": 4}, "now": 10}),
        Some(b"jpeg".to_vec()),
    )
    .unwrap_err();
    assert!(error.contains("immutable"), "{error}");
}

#[test]
fn a_refresh_uploads_only_the_thumbnail_with_its_source_and_turns() {
    let mut db = Connection::open_in_memory().unwrap();
    let admitted = refresh(&mut db, b"thumb-1", 1, "gen-00000001").unwrap();
    assert_eq!(admitted["row"]["source"]["quarter_turns"], 1);
    let upload = run(
        &mut db,
        json!({"action": "dispatch", "path": THUMB, "online": true, "now": 20}),
        None,
    )
    .unwrap()["upload"]
        .clone();
    let targets = upload["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 1, "the original is never re-sent: {upload}");
    assert_eq!(targets[0]["path"], THUMB);
    assert_eq!(targets[0]["expected_generation"], "77");
    assert_eq!(
        targets[0]["source"],
        json!({"object_path": PATH, "generation": "1729", "quarter_turns": 1})
    );
}

#[test]
fn a_newer_turn_replaces_a_waiting_refresh_but_not_one_mid_upload() {
    let mut db = Connection::open_in_memory().unwrap();
    refresh(&mut db, b"thumb-1", 1, "gen-00000001").unwrap();
    let again = refresh(&mut db, b"thumb-2", 2, "gen-00000002").unwrap();
    assert_eq!(again["row"]["source"]["quarter_turns"], 2);
    let counts = run(&mut db, json!({"action": "counts"}), None).unwrap();
    assert_eq!(counts["pending"], 1, "one refresh per thumbnail");
    run(
        &mut db,
        json!({"action": "dispatch", "path": THUMB, "online": true, "now": 20}),
        None,
    )
    .unwrap();
    let error = refresh(&mut db, b"thumb-3", 3, "gen-00000003").unwrap_err();
    assert!(error.contains(REFRESH_IN_FLIGHT), "{error}");
}

#[test]
fn a_refresh_waits_for_the_capture_upload_and_a_malformed_refresh_is_refused() {
    let mut db = Connection::open_in_memory().unwrap();
    let bytes = [b"original".as_slice(), b"thumb"].concat();
    run(
        &mut db,
        json!({"action": "admit", "project": "kigali", "path": PATH, "surface": "survey-media", "owner_ref": "entry-1",
            "content_type": "image/jpeg", "size": 8,
            "companion": {"path": THUMB, "content_type": "image/jpeg", "size": 5, "quarter_turns": 1},
            "generation": "gen-00000001", "payload": {"byte_length": 13}, "now": 10}),
        Some(bytes),
    )
    .unwrap();
    let error = refresh(&mut db, b"thumb-2", 2, "gen-00000002").unwrap_err();
    assert!(error.contains(REFRESH_AFTER_CAPTURE), "{error}");

    let mut db = Connection::open_in_memory().unwrap();
    let wrong = run(
        &mut db,
        json!({"action": "admit", "project": "kigali", "path": THUMB, "surface": "survey-media", "owner_ref": "entry-1",
            "content_type": "image/jpeg", "size": 2,
            "source": {"object_path": "kigali/forms/poles/e1/other.jpg", "generation": "1729", "quarter_turns": 1},
            "generation": "gen-00000004", "payload": {"byte_length": 2}, "now": 10}),
        Some(b"xx".to_vec()),
    );
    assert!(
        wrong.is_err(),
        "a refresh names the original its thumbnail belongs to"
    );
}
