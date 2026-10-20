use super::*;
use crate::durable_store::{execute as store, Answer, Call, Identity, Principal, Request};
use rusqlite::Connection;

fn identity(principal: Option<&str>) -> Identity {
    Identity {
        principal: principal.map_or(Principal::Anonymous, |uid| Principal::Account(uid.into())),
        lane: "canary".into(),
        audience: "a".repeat(64),
    }
}
fn call(db: &mut Connection, principal: Option<&str>, operation: Value) -> Result<Answer, String> {
    let request: Request = serde_json::from_value(json!({"store": STORE, "operation": operation}))
        .map_err(|e| e.to_string())?;
    store(
        db,
        Some(&identity(principal)),
        Call {
            request,
            expect_owner: None,
        },
        None,
    )
}
fn put(
    db: &mut Connection,
    uid: &str,
    kind: &str,
    project: Option<&str>,
    key: &str,
    value: Value,
) -> Result<Answer, String> {
    let mut operation =
        json!({"action": "put", "kind": kind, "key": key, "value": value, "now": 10});
    if let Some(project) = project {
        operation["project"] = project.into();
    }
    call(db, Some(uid), operation)
}
fn read(db: &mut Connection, uid: &str, kind: &str, project: Option<&str>, key: &str) -> Value {
    let mut operation = json!({"action": "read", "kind": kind, "key": key});
    if let Some(project) = project {
        operation["project"] = project.into();
    }
    call(db, Some(uid), operation).unwrap().value["held"].clone()
}

#[test]
fn a_draft_round_trips_inside_its_account_and_project() {
    let mut db = Connection::open_in_memory().unwrap();
    put(
        &mut db,
        "u1",
        "design_comparison",
        Some("p1"),
        "poles",
        json!({"values": {"a": 1}}),
    )
    .unwrap();
    assert_eq!(
        read(&mut db, "u1", "design_comparison", Some("p1"), "poles")["value"],
        json!({"values": {"a": 1}})
    );
    assert_eq!(
        read(&mut db, "u1", "design_comparison", Some("p2"), "poles"),
        Value::Null
    );
    assert_eq!(
        read(&mut db, "u2", "design_comparison", Some("p1"), "poles"),
        Value::Null
    );
    let removed = call(
        &mut db,
        Some("u1"),
        json!({"action": "delete", "kind": "design_comparison", "project": "p1", "key": "poles"}),
    )
    .unwrap();
    assert_eq!(removed.value["removed"], 1);
    assert_eq!(
        read(&mut db, "u1", "design_comparison", Some("p1"), "poles"),
        Value::Null
    );
}

#[test]
fn list_answers_meta_newest_first_and_never_values() {
    let mut db = Connection::open_in_memory().unwrap();
    for (key, now) in [("a", 5), ("b", 9)] {
        call(
            &mut db,
            Some("u1"),
            json!({"action": "put", "kind": "design_comparison", "project": "p1", "key": key,
            "value": {"big": key}, "meta": {"transformer": "TR_1"}, "now": now}),
        )
        .unwrap();
    }
    let rows = call(
        &mut db,
        Some("u1"),
        json!({"action": "list", "kind": "design_comparison", "project": "p1"}),
    )
    .unwrap()
    .value["rows"]
        .clone();
    assert_eq!(
        rows,
        json!([
            {"key": "b", "meta": {"transformer": "TR_1"}, "saved_at_ms": 9},
            {"key": "a", "meta": {"transformer": "TR_1"}, "saved_at_ms": 5},
        ])
    );
}

#[test]
fn fences_grammars_and_bounds_refuse() {
    let mut db = Connection::open_in_memory().unwrap();
    assert!(
        put(&mut db, "u1", "design_comparison", None, "poles", json!({}))
            .unwrap_err()
            .starts_with(INVALID)
    );
    assert!(put(
        &mut db,
        "u1",
        "messaging_message",
        Some("p1"),
        "c1",
        json!({})
    )
    .unwrap_err()
    .starts_with(INVALID));
    assert!(
        put(&mut db, "u1", "form_editor", None, " padded ", json!({}))
            .unwrap_err()
            .starts_with(INVALID)
    );
    assert!(put(
        &mut db,
        "u1",
        "messaging_message",
        None,
        "c1",
        json!({"text": "x".repeat(20_000)})
    )
    .unwrap_err()
    .starts_with(FULL));
    let signed_out = call(
        &mut db,
        None,
        json!({"action": "read", "kind": "messaging_message", "key": "c1"}),
    )
    .unwrap_err();
    assert!(signed_out.starts_with(SIGNED_OUT));
    // A refused put leaves nothing behind, not even its table.
    let tables: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='work_drafts'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0);
}

#[test]
fn a_full_kind_refuses_a_new_key_but_replaces_a_held_one() {
    let mut db = Connection::open_in_memory().unwrap();
    put(&mut db, "u1", "form_editor", None, "poles", json!([1])).unwrap();
    put(&mut db, "u1", "form_editor", None, "poles", json!([2])).unwrap();
    assert_eq!(
        read(&mut db, "u1", "form_editor", None, "poles")["value"],
        json!([2])
    );
    let max = Kind::MessagingMessage.rule().max_rows;
    db.execute_batch("BEGIN").unwrap();
    for index in 0..max {
        db.execute(
            "INSERT INTO work_drafts(owner,kind,project,key,value,saved_at_ms) VALUES(?1,'messaging_message','',?2,'{}',1)",
            params![identity(Some("u1")).owner(), format!("c{index}")],
        )
        .unwrap();
    }
    db.execute_batch("COMMIT").unwrap();
    assert!(put(
        &mut db,
        "u1",
        "messaging_message",
        None,
        "new",
        json!({"text": "x"})
    )
    .unwrap_err()
    .starts_with(FULL));
    put(
        &mut db,
        "u1",
        "messaging_message",
        None,
        "c0",
        json!({"text": "y"}),
    )
    .unwrap();
}
