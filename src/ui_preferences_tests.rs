use super::*;
use crate::durable_store::{Identity, Principal};
use rusqlite::Connection;

fn identity(principal: &str) -> Identity {
    Identity {
        principal: Principal::Account(principal.into()),
        lane: "canary".into(),
        audience: "a".repeat(64),
    }
}
fn run(connection: &Connection, principal: &str, operation: Value) -> Result<Value, String> {
    let identity = identity(principal);
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
fn an_account_key_round_trips_and_two_accounts_never_share_a_row() {
    let db = Connection::open_in_memory().unwrap();
    let absent = run(&db, "a", json!({"action":"get","keys":["active_project"]})).unwrap();
    assert_eq!(absent, json!({"values":{"active_project":null}}));
    let written = run(
        &db,
        "a",
        json!({"action":"set","key":"active_project","value":"kigali_north","now":42}),
    )
    .unwrap();
    assert_eq!(written["values"]["active_project"], "kigali_north");
    assert_eq!(
        run(&db, "a", json!({"action":"get","keys":["active_project"]})).unwrap()["values"]["active_project"],
        "kigali_north"
    );
    assert_eq!(
        run(&db, "b", json!({"action":"get","keys":["active_project"]})).unwrap()["values"]["active_project"],
        Value::Null,
        "another account on the same machine never sees it"
    );
    run(&db, "a", json!({"action":"clear","key":"active_project"})).unwrap();
    assert_eq!(
        run(&db, "a", json!({"action":"get","keys":["active_project"]})).unwrap()["values"]["active_project"],
        Value::Null
    );
}

#[test]
fn values_and_scopes_are_admitted_before_any_write() {
    let db = Connection::open_in_memory().unwrap();
    for refused in [json!(""), json!("a/b"), json!(7), json!(null), json!(["kigali"])] {
        let error = run(
            &db,
            "a",
            json!({"action":"set","key":"active_project","value":refused,"now":1}),
        )
        .unwrap_err();
        assert!(error.contains(INVALID), "{error}");
    }
    let crossed = run(
        &db,
        "a",
        json!({"action":"set","project":"kigali","key":"active_project","value":"kigali","now":1}),
    )
    .unwrap_err();
    assert!(crossed.contains("without a project"), "{crossed}");
    assert!(run(&db, "a", json!({"action":"get","keys":[]})).is_err());
    assert!(run(
        &db,
        "a",
        json!({"action":"get","keys":["active_project","active_project"]})
    )
    .is_err());
    assert!(run(&db, "a", json!({"action":"get","keys":["unknown_key"]})).is_err());
    assert!(run(&db, "a", json!({"action":"drop"})).is_err());
}

#[test]
fn a_stored_value_that_no_longer_admits_reads_as_absent_and_stays_in_place() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(TABLE).unwrap();
    let fence = scope(&identity("a").owner(), None, &[Key::ActiveProject]).unwrap();
    db.execute(
        "INSERT INTO ui_preferences VALUES(?1,'active_project','\"a/b\"',1)",
        rusqlite::params![fence],
    )
    .unwrap();
    assert_eq!(
        run(&db, "a", json!({"action":"get","keys":["active_project"]})).unwrap()["values"]["active_project"],
        Value::Null
    );
    let kept: i64 = db
        .query_row("SELECT count(*) FROM ui_preferences", [], |row| row.get(0))
        .unwrap();
    assert_eq!(kept, 1);
}

#[test]
fn an_import_runs_once_per_identity_and_answers_its_held_receipt() {
    let db = Connection::open_in_memory().unwrap();
    let legacy = run(&db, "a", json!({"action":"legacy","source":"persistent_cache"})).unwrap();
    let (keys, prefixes) = legacy::keys();
    assert_eq!(legacy, json!({"legacy":{"keys":keys,"prefixes":prefixes}}));
    let entries = json!([{"legacy_key":"not_owned_by_any_key","value":1}]);
    let first = run(
        &db,
        "a",
        json!({"action":"import","source":"persistent_cache","entries":entries,"now":5}),
    )
    .unwrap();
    assert_eq!(first["receipt"]["count"], 0);
    assert_eq!(first["receipt"]["skipped"], 1, "an entry no key owns is skipped, never stored");
    let again = run(
        &db,
        "a",
        json!({"action":"import","source":"persistent_cache","entries":[],"now":9}),
    )
    .unwrap();
    assert_eq!(again["receipt"], first["receipt"], "a second import answers the held receipt");
    let other = run(
        &db,
        "b",
        json!({"action":"import","source":"persistent_cache","entries":[],"now":9}),
    )
    .unwrap();
    assert_eq!(other["receipt"]["at_ms"], 9, "each identity imports once on its own");
}

#[test]
fn preferences_carry_no_payload_bytes() {
    let db = Connection::open_in_memory().unwrap();
    let identity = identity("a");
    let error = execute(
        &db,
        Input {
            identity: &identity,
            bytes: Some(vec![1]),
        },
        Operation::Get {
            project: None,
            keys: vec![Key::ActiveProject],
        },
    )
    .unwrap_err();
    assert!(error.contains("no payload bytes"), "{error}");
}
