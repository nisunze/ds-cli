use super::*;
use crate::durable_store::{execute as store, Call, Identity, Principal, Request};
use rusqlite::Connection;

fn identity(principal: Option<&str>) -> Identity {
    Identity {
        principal: principal.map_or(Principal::Anonymous, |uid| Principal::Account(uid.into())),
        lane: "canary".into(),
        audience: "a".repeat(64),
    }
}
fn run(db: &mut Connection, principal: Option<&str>, operation: Value) -> Result<Value, String> {
    let request: Request = serde_json::from_value(json!({"store": STORE, "operation": operation}))
        .map_err(|e| e.to_string())?;
    let answer = store(
        db,
        Some(&identity(principal)),
        Call {
            request,
            expect_owner: None,
        },
        None,
    )?;
    Ok(Value::Object(answer.value))
}
fn ok(db: &mut Connection, operation: Value) -> Value {
    run(db, Some("u1"), operation).unwrap()
}
fn refused(db: &mut Connection, operation: Value) -> String {
    run(db, Some("u1"), operation).unwrap_err()
}
/// Queue `rows` (each `{project?, id, record?, size?, meta?}`) with a small payload.
fn enqueue(db: &mut Connection, queue: &str, rows: Vec<Value>) -> Result<Value, String> {
    let rows: Vec<Value> = rows
        .into_iter()
        .map(|mut row| {
            row["payload"] = json!({"n": row["id"]});
            row
        })
        .collect();
    run(
        db,
        Some("u1"),
        json!({"action": "enqueue", "queue": queue, "rows": rows, "now": 10}),
    )
}
fn work(project: &str, id: &str) -> Value {
    json!({"project": project, "id": id})
}
fn claim(db: &mut Connection, lane: &str, project: Option<&str>, lease: &str, now: u64) -> Value {
    let mut operation = json!({"action": "claim", "queue": "project_work", "lane": lane, "lease": lease, "lease_ms": 1000, "now": now});
    if let Some(project) = project {
        operation["project"] = project.into();
    }
    ok(db, operation)
}
fn claimed_id(answer: &Value) -> Option<&str> {
    answer["claimed"]["id"].as_str()
}
fn settle(id: &str, lease: &str, settlement: Value, now: u64) -> Value {
    json!({"action": "settle", "queue": "project_work", "id": id, "lease": lease, "settlement": settlement, "now": now})
}
fn sent(db: &mut Connection, id: &str, lease: &str, now: u64) {
    ok(db, settle(id, lease, json!({"outcome": "sent"}), now));
}
fn rows(db: &mut Connection, queue: &str, project: Option<&str>) -> Vec<Value> {
    let mut operation = json!({"action": "list", "queue": queue});
    if let Some(project) = project {
        operation["project"] = project.into();
    }
    ok(db, operation)["rows"].as_array().unwrap().clone()
}

#[test]
fn queued_work_stays_inside_its_account_and_project() {
    let mut db = Connection::open_in_memory().unwrap();
    enqueue(
        &mut db,
        "messaging_send",
        vec![json!({"id": "m1", "meta": {"conversation": "c1"}})],
    )
    .unwrap();
    enqueue(&mut db, "project_work", vec![work("p1", "w1")]).unwrap();
    let listed = rows(&mut db, "messaging_send", None);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["payload"], json!({"n": "m1"}));
    assert_eq!(listed[0]["meta"], json!({"conversation": "c1"}));
    assert_eq!(
        (listed[0]["status"].as_str(), listed[0]["record"].as_str()),
        (Some("pending"), Some("m1"))
    );
    assert_eq!(
        (listed[0]["lane"].as_str(), listed[0]["project"].clone()),
        (Some("small"), Value::Null)
    );
    assert_eq!(
        rows(&mut db, "project_work", Some("p1"))[0]["project"],
        "p1"
    );
    assert!(rows(&mut db, "project_work", Some("p2")).is_empty());

    let other = run(
        &mut db,
        Some("u2"),
        json!({"action": "list", "queue": "messaging_send"}),
    )
    .unwrap();
    assert_eq!(other["rows"], json!([]), "another account reads none");
    let signed_out = run(&mut db, None, json!({"action": "counts"})).unwrap_err();
    assert!(signed_out.starts_with(SIGNED_OUT), "{signed_out}");
}

#[test]
fn a_queue_refuses_what_it_cannot_keep_and_evicts_nothing() {
    let mut db = Connection::open_in_memory().unwrap();
    enqueue(&mut db, "messaging_send", vec![json!({"id": "m0"})]).unwrap();
    assert!(
        enqueue(&mut db, "messaging_send", vec![json!({"id": "m0"})])
            .unwrap_err()
            .starts_with(EXISTS)
    );
    let batch = enqueue(
        &mut db,
        "messaging_send",
        vec![json!({"id": "fresh"}), json!({"id": " bad"})],
    )
    .unwrap_err();
    assert!(batch.starts_with(INVALID), "{batch}");
    assert_eq!(
        rows(&mut db, "messaging_send", None).len(),
        1,
        "a refused batch queues none of its rows"
    );
    enqueue(
        &mut db,
        "messaging_send",
        (1..100).map(|n| json!({"id": format!("m{n}")})).collect(),
    )
    .unwrap();
    let full = enqueue(&mut db, "messaging_send", vec![json!({"id": "m100"})]).unwrap_err();
    assert!(full.starts_with(FULL), "{full}");
    assert_eq!(rows(&mut db, "messaging_send", None).len(), 100);

    let big = refused(
        &mut db,
        json!({"action": "enqueue", "queue": "project_work", "rows": [{"project": "p1", "id": "big", "payload": {"x": "y".repeat(300 * 1024)}}], "now": 1}),
    );
    assert!(big.starts_with(FULL), "{big}");
    for operation in [
        json!({"action": "enqueue", "queue": "project_work", "rows": [{"project": "p1", "id": "a", "payload": [1]}], "now": 1}),
        json!({"action": "enqueue", "queue": "messaging_send", "rows": [{"project": "p1", "id": "a", "payload": {}}], "now": 1}),
        json!({"action": "enqueue", "queue": "project_work", "rows": [{"id": "a", "payload": {}}], "now": 1}),
        json!({"action": "enqueue", "queue": "project_work", "rows": [], "now": 1}),
        json!({"action": "claim", "queue": "project_work", "lane": "small", "lease": "l", "lease_ms": MAX_LEASE_MS + 1, "now": 1}),
    ] {
        assert!(
            refused(&mut db, operation.clone()).starts_with(INVALID),
            "{operation}"
        );
    }
    assert!(
        serde_json::from_value::<Request>(
            json!({"store": STORE, "operation": {"action": "list", "queue": "solar_input"}})
        )
        .is_err(),
        "the queues are closed"
    );
}

#[test]
fn a_record_s_steps_leave_in_order_and_records_do_not_wait_for_each_other() {
    let mut db = Connection::open_in_memory().unwrap();
    enqueue(
        &mut db,
        "project_work",
        vec![
            json!({"project": "p1", "id": "a1", "record": "a"}),
            json!({"project": "p1", "id": "a2", "record": "a"}),
            json!({"project": "p1", "id": "b1", "record": "b"}),
        ],
    )
    .unwrap();
    let first = claim(&mut db, "small", Some("p1"), "l1", 20);
    assert_eq!(
        (claimed_id(&first), first["claimed"]["status"].as_str()),
        (Some("a1"), Some("syncing"))
    );
    assert_eq!(
        claimed_id(&claim(&mut db, "small", Some("p1"), "l2", 21)),
        Some("b1"),
        "record b does not wait for a"
    );
    assert_eq!(
        claimed_id(&claim(&mut db, "small", Some("p1"), "l3", 21)),
        None,
        "a2 waits for a1"
    );
    sent(&mut db, "a1", "l1", 30);
    assert_eq!(
        claimed_id(&claim(&mut db, "small", None, "l3", 31)),
        Some("a2")
    );
}

#[test]
fn an_abandoned_lease_is_taken_over_and_the_old_drain_cannot_settle() {
    let mut db = Connection::open_in_memory().unwrap();
    enqueue(&mut db, "project_work", vec![work("p1", "a")]).unwrap();
    assert_eq!(
        claimed_id(&claim(&mut db, "small", Some("p1"), "tab1", 100)),
        Some("a")
    );
    assert_eq!(
        claimed_id(&claim(&mut db, "small", Some("p1"), "tab2", 1100)),
        Some("a"),
        "the lease lapsed"
    );
    let lost = refused(
        &mut db,
        settle("a", "tab1", json!({"outcome": "sent"}), 1200),
    );
    assert!(lost.starts_with(LEASE_LOST), "{lost}");
    let revise = |lease: &str| json!({"action": "revise", "queue": "project_work", "id": "a", "lease": lease, "payload": {"frozen": true}, "lease_ms": 1000, "now": 1900});
    assert!(refused(&mut db, revise("tab1")).starts_with(LEASE_LOST));
    ok(&mut db, revise("tab2"));
    assert_eq!(
        rows(&mut db, "project_work", Some("p1"))[0]["payload"],
        json!({"frozen": true})
    );
    sent(&mut db, "a", "tab2", 2800);
    assert!(
        rows(&mut db, "project_work", None).is_empty(),
        "the renewed lease settles; a sent row leaves"
    );
}

#[test]
fn deferred_spends_no_attempt_and_a_retry_waits_for_its_time() {
    let mut db = Connection::open_in_memory().unwrap();
    enqueue(&mut db, "project_work", vec![work("p1", "a")]).unwrap();
    claim(&mut db, "small", Some("p1"), "l1", 10);
    ok(
        &mut db,
        settle(
            "a",
            "l1",
            json!({"outcome": "deferred", "error": "offline"}),
            11,
        ),
    );
    let row = &rows(&mut db, "project_work", None)[0];
    assert_eq!(
        (
            row["status"].as_str(),
            row["attempts"].as_i64(),
            row["error"].as_str()
        ),
        (Some("pending"), Some(0), Some("offline"))
    );

    claim(&mut db, "small", Some("p1"), "l2", 12);
    ok(
        &mut db,
        settle(
            "a",
            "l2",
            json!({"outcome": "retry", "error": "503", "next_attempt_at": 500}),
            13,
        ),
    );
    let waiting = claim(&mut db, "small", Some("p1"), "l3", 100);
    assert_eq!(
        (claimed_id(&waiting), waiting["due_at"].as_i64()),
        (None, Some(500))
    );
    let due = claim(&mut db, "small", Some("p1"), "l3", 500);
    assert_eq!(
        (claimed_id(&due), due["claimed"]["attempts"].as_i64()),
        (Some("a"), Some(1))
    );
}

#[test]
fn a_failure_no_retry_fixes_is_parked_with_its_remedy_and_holds_only_its_record() {
    let mut db = Connection::open_in_memory().unwrap();
    enqueue(
        &mut db,
        "project_work",
        vec![
            json!({"project": "p1", "id": "a1", "record": "a"}),
            json!({"project": "p1", "id": "a2", "record": "a"}),
            json!({"project": "p1", "id": "b1", "record": "b"}),
        ],
    )
    .unwrap();
    claim(&mut db, "small", Some("p1"), "l1", 20);
    let bad = refused(
        &mut db,
        settle(
            "a1",
            "l1",
            json!({"outcome": "failed", "error": "x", "remedy": "Edit!"}),
            21,
        ),
    );
    assert!(bad.starts_with(INVALID), "{bad}");
    ok(
        &mut db,
        settle(
            "a1",
            "l1",
            json!({"outcome": "failed", "error": "é".repeat(4000), "remedy": "edit"}),
            21,
        ),
    );
    let parked = &rows(&mut db, "project_work", Some("p1"))[0];
    assert_eq!(
        (parked["status"].as_str(), parked["remedy"].as_str()),
        (Some("failed"), Some("edit"))
    );
    assert!(
        parked["error"].as_str().unwrap().len() <= 2048,
        "a long error is cut, not refused"
    );
    assert_eq!(
        claimed_id(&claim(&mut db, "small", Some("p1"), "l2", 22)),
        Some("b1"),
        "another record goes on"
    );
    assert_eq!(
        claimed_id(&claim(&mut db, "small", Some("p1"), "l3", 22)),
        None,
        "a2 waits behind its parked step"
    );

    let counts = ok(&mut db, json!({"action": "counts"}))["counts"].clone();
    assert_eq!(
        counts["project_work"],
        json!({"pending": 1, "syncing": 1, "failed": 1})
    );
    assert_eq!(
        counts["messaging_send"],
        json!({"pending": 0, "syncing": 0, "failed": 0})
    );

    let leased = refused(
        &mut db,
        json!({"action": "discard", "queue": "project_work", "id": "b1", "now": 23}),
    );
    assert!(leased.starts_with(LEASED), "{leased}");
    assert_eq!(
        ok(
            &mut db,
            json!({"action": "retry", "queue": "project_work", "now": 24})
        )["retried"],
        1
    );
    let retried = &rows(&mut db, "project_work", Some("p1"))[0];
    assert_eq!(
        (retried["status"].as_str(), retried["remedy"].clone()),
        (Some("pending"), Value::Null)
    );
    assert_eq!(
        ok(
            &mut db,
            json!({"action": "discard", "queue": "project_work", "id": "a1", "now": 25})
        )["removed"],
        1
    );
    assert_eq!(
        claimed_id(&claim(&mut db, "small", Some("p1"), "l4", 26)),
        Some("a2"),
        "the record goes on once the person acts"
    );
}

#[test]
fn a_large_upload_in_flight_never_holds_the_small_items_behind_it() {
    let mut db = Connection::open_in_memory().unwrap();
    // One 200 MB photo of entry e0, then a small step of e0 and 20 small
    // items of other records, all queued after it.
    enqueue(
        &mut db,
        "project_work",
        vec![
            json!({"project": "p1", "id": "photo", "record": "e0", "size": 200_000_000}),
            json!({"project": "p1", "id": "e0-edit", "record": "e0"}),
        ],
    )
    .unwrap();
    enqueue(
        &mut db,
        "project_work",
        (0..20)
            .map(|n| json!({"project": "p1", "id": format!("s{n}"), "record": format!("r{n}")}))
            .collect(),
    )
    .unwrap();
    assert_eq!(rows(&mut db, "project_work", None)[0]["lane"], "large");

    let upload = claim(&mut db, "large", None, "uploader", 1000);
    assert_eq!(claimed_id(&upload), Some("photo"));
    assert_eq!(
        claim(&mut db, "large", None, "other", 1000)["busy"],
        true,
        "one large transfer at a time"
    );

    // The transfer runs for a long while, renewing its lease and keeping
    // where it stands; every small item drains meanwhile.
    let mut now = 1000;
    let mut drained = Vec::new();
    loop {
        now += 5;
        let next = claim(&mut db, "small", None, "small-lane", now);
        let Some(id) = claimed_id(&next).map(str::to_owned) else {
            break;
        };
        sent(&mut db, &id, "small-lane", now + 1);
        drained.push(id);
        ok(
            &mut db,
            json!({"action": "revise", "queue": "project_work", "id": "photo", "lease": "uploader", "meta": {"offset": drained.len() * 10_000_000}, "lease_ms": 1000, "now": now}),
        );
    }
    assert_eq!(
        drained,
        (0..20).map(|n| format!("s{n}")).collect::<Vec<_>>()
    );
    assert!(
        now < 1200,
        "twenty small items drained within the upload's first lease"
    );
    assert_eq!(
        rows(&mut db, "project_work", None).len(),
        2,
        "the photo and its record's next step remain"
    );
    assert_eq!(rows(&mut db, "project_work", None)[0]["status"], "syncing");

    // The large transfer's own record still keeps its order.
    sent(&mut db, "photo", "uploader", now + 1);
    assert_eq!(
        claimed_id(&claim(&mut db, "small", None, "small-lane", now + 2)),
        Some("e0-edit")
    );
}

#[test]
fn the_small_lane_is_bounded_too() {
    let mut db = Connection::open_in_memory().unwrap();
    enqueue(
        &mut db,
        "project_work",
        (0..6).map(|n| work("p1", &format!("s{n}"))).collect(),
    )
    .unwrap();
    for n in 0..4 {
        assert_eq!(
            claimed_id(&claim(&mut db, "small", None, &format!("tab{n}"), 10)),
            Some(format!("s{n}").as_str())
        );
    }
    let full = claim(&mut db, "small", None, "tab4", 10);
    assert_eq!(
        (claimed_id(&full), full["busy"].as_bool()),
        (None, Some(true))
    );
}

/// A test machine: `{"settle": id}` removes that row and names an effect;
/// `{"park": id}` parks it; a version it was not given refuses.
fn machine(step: Step) -> Result<Vec<Action>, String> {
    let version = |id: &str| {
        step.rows
            .iter()
            .find(|row| row["id"] == id)
            .and_then(|row| row["version"].as_u64())
            .unwrap_or(99)
    };
    let mut actions = Vec::new();
    if let Some(id) = step.event["settle"].as_str() {
        actions.push(Action::Remove {
            id: id.into(),
            expected_version: version(id),
        });
        actions.push(Action::Effect {
            effect: json!({"kind": "surface", "id": id}),
        });
    }
    if let Some(id) = step.event["park"].as_str() {
        let expected_version = step.event["version"]
            .as_u64()
            .unwrap_or_else(|| version(id));
        actions.push(Action::Persist {
            id: id.into(),
            expected_version,
            set: Box::new(Patch {
                status: Some(Status::Failed),
                error: Some("conflict".into()),
                remedy: Some("resolve_conflict".into()),
                meta: Some(json!({"phase": "conflict"})),
                ..Patch::default()
            }),
        });
    }
    Ok(actions)
}

#[test]
fn a_queue_s_machine_moves_its_rows_inside_one_transaction_under_their_versions() {
    let mut db = Connection::open_in_memory().unwrap();
    enqueue(
        &mut db,
        "survey",
        vec![work("p1", "e1"), work("p1", "e2"), work("p2", "e3")],
    )
    .unwrap();
    let refused_op = refused(
        &mut db,
        json!({"action": "transition", "queue": "survey", "project": "p1", "event": {}, "now": 1}),
    );
    assert!(
        refused_op.starts_with(INVALID),
        "a queue without a machine refuses: {refused_op}"
    );

    let owner = identity(Some("u1")).owner();
    let effects = transition(
        &db,
        &owner,
        Queue::Survey,
        Some("p1"),
        &json!({"settle": "e1", "park": "e2"}),
        5,
        machine,
    )
    .unwrap();
    assert_eq!(effects, vec![json!({"kind": "surface", "id": "e1"})]);
    let held = rows(&mut db, "survey", None);
    assert_eq!(
        held.iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["e2", "e3"]
    );
    assert_eq!(
        (
            held[0]["status"].as_str(),
            held[0]["remedy"].as_str(),
            held[0]["version"].as_i64()
        ),
        (Some("failed"), Some("resolve_conflict"), Some(2))
    );
    assert_eq!(held[0]["meta"], json!({"phase": "conflict"}));

    let stale = transition(
        &db,
        &owner,
        Queue::Survey,
        Some("p1"),
        &json!({"park": "e2", "version": 1}),
        6,
        machine,
    )
    .unwrap_err();
    assert!(stale.starts_with(FENCE_CHANGED), "{stale}");
    let escaped = transition(
        &db,
        &owner,
        Queue::Survey,
        Some("p1"),
        &json!({"settle": "e3"}),
        6,
        machine,
    )
    .unwrap_err();
    assert!(
        escaped.starts_with(FENCE_CHANGED),
        "a row of another project is not the machine's: {escaped}"
    );
    let other = identity(Some("u2")).owner();
    assert!(
        transition(
            &db,
            &other,
            Queue::Survey,
            Some("p1"),
            &json!({"settle": "e2"}),
            6,
            machine
        )
        .is_err(),
        "another account's rows are never read"
    );
}
