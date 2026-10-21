use super::*;
use crate::durable_store::{
    execute as store, Answer, Call, Identity, Principal, Request, OWNER_CHANGED,
};
use rusqlite::Connection;

fn identity(principal: &str) -> Identity {
    Identity {
        principal: Principal::Account(principal.into()),
        lane: "canary".into(),
        audience: "a".repeat(64),
    }
}
fn call(db: &mut Connection, principal: &str, operation: Value) -> Result<Answer, String> {
    call_as(db, principal, None, operation)
}
fn call_as(
    db: &mut Connection,
    principal: &str,
    expect_owner: Option<String>,
    operation: Value,
) -> Result<Answer, String> {
    let request: Request = serde_json::from_value(json!({"store": STORE, "operation": operation}))
        .map_err(|e| e.to_string())?;
    store(
        db,
        Some(&identity(principal)),
        Call {
            request,
            expect_owner,
        },
        None,
    )
}
fn read(
    db: &mut Connection,
    principal: &str,
    kind: &str,
    project: Option<&str>,
    key: &str,
) -> Value {
    let mut operation = json!({"action": "read", "kind": kind, "key": key, "now": 100});
    if let Some(project) = project {
        operation["project"] = project.into();
    }
    call(db, principal, operation).unwrap().value["held"].clone()
}
fn write(
    db: &mut Connection,
    principal: &str,
    kind: &str,
    project: Option<&str>,
    key: &str,
    value: Value,
    now: u64,
) -> Answer {
    let mut operation =
        json!({"action": "write", "kind": kind, "key": key, "value": value, "now": now});
    if let Some(project) = project {
        operation["project"] = project.into();
    }
    call(db, principal, operation).unwrap()
}
const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn every_kind_is_named_once_with_a_bounded_rule() {
    let mut names: Vec<_> = Kind::ALL.iter().map(|kind| kind.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), Kind::ALL.len());
    for kind in Kind::ALL {
        let rule = kind.rule();
        assert!(
            rule.max_rows > 0 && rule.max_value_bytes > 0 && rule.max_value_bytes <= 48 * MIB,
            "{kind:?}"
        );
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.name()));
    }
}

#[test]
fn a_value_round_trips_and_two_accounts_never_share_a_row() {
    let mut db = Connection::open_in_memory().unwrap();
    assert_eq!(
        read(&mut db, "a", "map_state", Some("kigali"), ""),
        Value::Null
    );
    let written = write(
        &mut db,
        "a",
        "map_state",
        Some("kigali"),
        "",
        json!({"layers": [1, 2]}),
        7,
    );
    assert_eq!(written.value["written"], 1);
    assert_eq!(written.generation, Some(1));
    assert_eq!(written.owner, Some(identity("a").owner()));
    assert_eq!(
        read(&mut db, "a", "map_state", Some("kigali"), ""),
        json!({"value": {"layers": [1, 2]}, "saved_at_ms": 7, "invalidated": false})
    );
    assert_eq!(
        read(&mut db, "b", "map_state", Some("kigali"), ""),
        Value::Null,
        "another account never sees it"
    );
    assert_eq!(
        read(&mut db, "a", "map_state", Some("musanze"), ""),
        Value::Null,
        "another project never sees it"
    );
}

#[test]
fn the_fence_names_a_project_only_for_a_project_kind() {
    let mut db = Connection::open_in_memory().unwrap();
    let account_with_project = call(
        &mut db,
        "a",
        json!({"action": "read", "kind": "sre_activity", "project": "kigali", "now": 1}),
    );
    assert!(account_with_project.unwrap_err().starts_with(INVALID));
    let project_without = call(
        &mut db,
        "a",
        json!({"action": "read", "kind": "map_state", "now": 1}),
    );
    assert!(project_without.unwrap_err().contains("one valid project"));
    let bad_project = call(
        &mut db,
        "a",
        json!({"action": "read", "kind": "map_state", "project": "a/b", "now": 1}),
    );
    assert!(bad_project.unwrap_err().starts_with(INVALID));
    write(
        &mut db,
        "a",
        "sre_activity",
        None,
        "",
        json!({"rows": []}),
        1,
    );
    assert!(read(&mut db, "a", "sre_activity", None, "").is_object());
}

#[test]
fn keys_follow_their_kind_grammar() {
    let mut db = Connection::open_in_memory().unwrap();
    for (kind, project, key) in [
        ("map_state", Some("kigali"), "extra"),
        ("assets_index_page", Some("kigali"), "ABCDEF"),
        ("assets_index_page", Some("kigali"), ""),
        ("admin_children", None, " padded"),
        ("admin_children", None, ""),
        ("messaging_timeline", None, "line\nbreak"),
    ] {
        let mut operation =
            json!({"action": "write", "kind": kind, "key": key, "value": 1, "now": 1});
        if let Some(project) = project {
            operation["project"] = project.into();
        }
        let error = call(&mut db, "a", operation).unwrap_err();
        assert!(error.contains("grammar"), "{kind} {key:?}: {error}");
    }
    write(
        &mut db,
        "a",
        "assets_index_page",
        Some("kigali"),
        DIGEST,
        json!({}),
        1,
    );
    write(
        &mut db,
        "a",
        "admin_children",
        None,
        "district:1",
        json!([]),
        1,
    );
    assert!(call(
        &mut db,
        "a",
        json!({"action": "read", "kind": "indexeddb", "now": 1})
    )
    .is_err());
}

#[test]
fn a_value_past_its_bound_is_refused_and_leaves_nothing_behind() {
    let mut db = Connection::open_in_memory().unwrap();
    let big = "x".repeat(MIB + 1);
    let error = call(&mut db, "a", json!({"action": "write", "kind": "admin_children", "key": "province:", "value": big, "now": 1})).unwrap_err();
    assert!(error.starts_with(TOO_LARGE), "{error}");
    let tables: i64 = db
        .query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('retained_reads','durable_store_generations')", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        tables, 0,
        "the refused call rolled back even its schema step"
    );
}

#[test]
fn a_write_past_the_row_bound_evicts_the_least_recently_used() {
    let mut db = Connection::open_in_memory().unwrap();
    // 24 map states across projects; reading the first keeps it in use.
    for index in 0..24u64 {
        write(
            &mut db,
            "a",
            "map_state",
            Some(&format!("p{index:02}")),
            "",
            json!(index),
            10 + index,
        );
    }
    call(
        &mut db,
        "a",
        json!({"action": "read", "kind": "map_state", "project": "p00", "key": "", "now": 50}),
    )
    .unwrap();
    write(&mut db, "a", "map_state", Some("p24"), "", json!(24), 60);
    assert!(
        read(&mut db, "a", "map_state", Some("p00"), "").is_object(),
        "a recently read row stays"
    );
    assert_eq!(
        read(&mut db, "a", "map_state", Some("p01"), ""),
        Value::Null,
        "the least recently used row went"
    );
    assert!(read(&mut db, "a", "map_state", Some("p24"), "").is_object());
    let count: i64 = db
        .query_row("SELECT count(*) FROM retained_reads", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 24);
    // Another account's rows never count against this one's bound.
    write(&mut db, "b", "map_state", Some("p01"), "", json!("b"), 70);
    assert!(read(&mut db, "a", "map_state", Some("p02"), "").is_object());
}

#[test]
fn replace_set_makes_the_fence_exactly_the_set_or_changes_nothing() {
    let mut db = Connection::open_in_memory().unwrap();
    let set = |rows: Value| json!({"action": "replace_set", "kind": "messaging_timeline", "rows": rows, "now": 5});
    call(
        &mut db,
        "a",
        set(json!([{"key": "c1", "value": [1]}, {"key": "c2", "value": [2]}])),
    )
    .unwrap();
    let replaced = call(
        &mut db,
        "a",
        set(json!([{"key": "c2", "value": [22]}, {"key": "c3", "value": [3]}])),
    )
    .unwrap();
    assert_eq!(replaced.value["removed"], 1);
    assert_eq!(
        read(&mut db, "a", "messaging_timeline", None, "c1"),
        Value::Null
    );
    assert_eq!(
        read(&mut db, "a", "messaging_timeline", None, "c2")["value"],
        json!([22])
    );
    // One bad row refuses the whole set: the previous set stays intact.
    let refused = call(
        &mut db,
        "a",
        set(json!([{"key": "c4", "value": [4]}, {"key": "bad\u{7}", "value": [5]}])),
    );
    assert!(refused.is_err());
    let duplicate = call(
        &mut db,
        "a",
        set(json!([{"key": "c4", "value": 1}, {"key": "c4", "value": 2}])),
    );
    assert!(duplicate.unwrap_err().contains("each key once"));
    let rows: Vec<String> = call(
        &mut db,
        "a",
        json!({"action": "list", "kind": "messaging_timeline", "now": 9}),
    )
    .unwrap()
    .value["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["key"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(rows, ["c2", "c3"]);
    let too_many: Vec<Value> = (0..25)
        .map(|index| json!({"key": format!("c{index}"), "value": index}))
        .collect();
    assert!(call(&mut db, "a", set(json!(too_many)))
        .unwrap_err()
        .starts_with(TOO_LARGE));
}

#[test]
fn every_change_advances_only_its_own_kind() {
    let mut db = Connection::open_in_memory().unwrap();
    assert_eq!(
        write(&mut db, "a", "map_state", Some("kigali"), "", json!(1), 1).generation,
        Some(1)
    );
    assert_eq!(
        write(&mut db, "a", "sre_activity", None, "", json!(1), 1).generation,
        Some(1)
    );
    assert_eq!(
        write(&mut db, "a", "map_state", Some("kigali"), "", json!(2), 2).generation,
        Some(2)
    );
    let read = call(
        &mut db,
        "a",
        json!({"action": "read", "kind": "sre_activity", "key": "", "now": 3}),
    )
    .unwrap();
    assert_eq!(
        read.generation,
        Some(1),
        "a read moves nothing and reports its kind's generation"
    );
    let invalidated = call(
        &mut db,
        "a",
        json!({"action": "invalidate", "kind": "project_work_graph"}),
    )
    .unwrap();
    assert_eq!(invalidated.value["invalidated"], 0);
    assert_eq!(
        invalidated.generation,
        Some(1),
        "an invalidation fences in-flight writes even with no row held"
    );
    let map = call(
        &mut db,
        "a",
        json!({"action": "read", "kind": "map_state", "project": "kigali", "key": "", "now": 3}),
    )
    .unwrap();
    assert_eq!(map.generation, Some(2));
}

#[test]
fn a_conditional_write_is_refused_once_its_kind_moved() {
    let mut db = Connection::open_in_memory().unwrap();
    let observed = call(
        &mut db,
        "a",
        json!({"action": "read", "kind": "map_state", "project": "kigali", "key": "", "now": 1}),
    )
    .unwrap()
    .generation
    .unwrap();
    call(
        &mut db,
        "a",
        json!({"action": "invalidate", "kind": "map_state", "project": "kigali"}),
    )
    .unwrap();
    let stale = call(
        &mut db,
        "a",
        json!({"action": "write", "kind": "map_state", "project": "kigali", "value": 1, "now": 2, "expect_generation": observed}),
    );
    assert!(stale.unwrap_err().starts_with(STALE));
    assert_eq!(
        read(&mut db, "a", "map_state", Some("kigali"), ""),
        Value::Null
    );
    let current = call(
        &mut db,
        "a",
        json!({"action": "write", "kind": "map_state", "project": "kigali", "value": 1, "now": 2, "expect_generation": observed + 1}),
    );
    assert!(current.is_ok());
}

#[test]
fn invalidate_and_forget_reach_one_key_one_project_or_the_whole_account() {
    let mut db = Connection::open_in_memory().unwrap();
    for project in ["kigali", "musanze"] {
        write(
            &mut db,
            "a",
            "map_state",
            Some(project),
            "",
            json!(project),
            1,
        );
    }
    write(&mut db, "b", "map_state", Some("kigali"), "", json!("b"), 1);
    let one = call(
        &mut db,
        "a",
        json!({"action": "invalidate", "kind": "map_state", "project": "kigali"}),
    )
    .unwrap();
    assert_eq!(one.value["invalidated"], 1);
    assert_eq!(
        read(&mut db, "a", "map_state", Some("kigali"), "")["invalidated"],
        true
    );
    assert_eq!(
        read(&mut db, "a", "map_state", Some("musanze"), "")["invalidated"],
        false
    );
    let all = call(
        &mut db,
        "a",
        json!({"action": "invalidate", "kind": "map_state"}),
    )
    .unwrap();
    assert_eq!(
        all.value["invalidated"], 1,
        "only the row not yet stale changes"
    );
    assert_eq!(
        read(&mut db, "b", "map_state", Some("kigali"), "")["invalidated"],
        false,
        "another account is untouched"
    );
    write(
        &mut db,
        "a",
        "map_state",
        Some("kigali"),
        "",
        json!("fresh"),
        2,
    );
    assert_eq!(
        read(&mut db, "a", "map_state", Some("kigali"), "")["invalidated"],
        false,
        "a write clears the mark"
    );
    let removed = call(
        &mut db,
        "a",
        json!({"action": "forget", "kind": "map_state"}),
    )
    .unwrap();
    assert_eq!(removed.value["removed"], 2);
    assert!(read(&mut db, "b", "map_state", Some("kigali"), "").is_object());
    let keyed = call(
        &mut db,
        "a",
        json!({"action": "forget", "kind": "assets_index_page", "key": DIGEST}),
    );
    assert!(keyed.unwrap_err().contains("inside its project"));
}

#[test]
fn an_unreadable_value_reads_as_absent() {
    let mut db = Connection::open_in_memory().unwrap();
    write(
        &mut db,
        "a",
        "sre_activity",
        None,
        "",
        json!({"rows": []}),
        1,
    );
    db.execute("UPDATE retained_reads SET value='{not json'", [])
        .unwrap();
    assert_eq!(read(&mut db, "a", "sre_activity", None, ""), Value::Null);
    let rows = call(
        &mut db,
        "a",
        json!({"action": "list", "kind": "sre_activity", "now": 2}),
    )
    .unwrap();
    assert_eq!(rows.value["rows"], json!([]));
}

#[test]
fn a_flow_write_after_an_account_switch_is_refused_before_it_runs() {
    let mut db = Connection::open_in_memory().unwrap();
    let began = call(
        &mut db,
        "a",
        json!({"action": "read", "kind": "map_state", "project": "kigali", "key": "", "now": 1}),
    )
    .unwrap()
    .owner;
    let write = json!({"action": "write", "kind": "map_state", "project": "kigali", "value": "a's answer", "now": 2});
    let switched = call_as(&mut db, "b", began.clone(), write.clone()).unwrap_err();
    assert!(switched.starts_with(OWNER_CHANGED), "{switched}");
    assert_eq!(
        read(&mut db, "b", "map_state", Some("kigali"), ""),
        Value::Null
    );
    assert!(call_as(&mut db, "a", began, write).is_ok());
}

#[test]
fn values_never_travel_as_payload_bytes() {
    let mut db = Connection::open_in_memory().unwrap();
    let request: Request = serde_json::from_value(
        json!({"store": STORE, "operation": {"action": "read", "kind": "sre_activity", "now": 1}}),
    )
    .unwrap();
    let error = store(
        &mut db,
        Some(&identity("a")),
        Call {
            request,
            expect_owner: None,
        },
        Some(vec![1]),
    )
    .unwrap_err();
    assert!(error.contains("JSON"), "{error}");
}

#[test]
fn count_reads_one_fence_and_no_value() {
    let mut db = Connection::open_in_memory().unwrap();
    for key in ["[\"t1\",\"v1\"]", "[\"t1\",\"v2\"]"] {
        write(
            &mut db,
            "a",
            "transformer_version",
            Some("kigali"),
            key,
            json!({"v": key}),
            1,
        );
    }
    write(
        &mut db,
        "a",
        "transformer_version",
        Some("musanze"),
        "[\"t1\",\"v1\"]",
        json!(1),
        1,
    );
    write(
        &mut db,
        "b",
        "transformer_version",
        Some("kigali"),
        "[\"t1\",\"v1\"]",
        json!(1),
        1,
    );
    let count = |db: &mut Connection, principal: &str, project: &str| {
        call(
            db,
            principal,
            json!({"action": "count", "kind": "transformer_version", "project": project}),
        )
        .unwrap()
        .value["count"]
            .clone()
    };
    assert_eq!(count(&mut db, "a", "kigali"), 2);
    assert_eq!(count(&mut db, "a", "musanze"), 1);
    assert_eq!(count(&mut db, "b", "kigali"), 1);
    assert_eq!(count(&mut db, "a", "rubavu"), 0);
    let error = call(
        &mut db,
        "a",
        json!({"action": "count", "kind": "transformer_version"}),
    )
    .unwrap_err();
    assert!(error.starts_with(INVALID), "{error}");
}
