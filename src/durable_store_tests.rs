use super::*;
use rusqlite::Connection;
use serde_json::json;

fn identity(principal: Principal) -> Identity {
    Identity {
        principal,
        lane: "canary".into(),
        audience: "a".repeat(64),
    }
}
fn owner() -> Identity {
    identity(Principal::Account("owner".into()))
}
fn call(value: Value) -> Call {
    serde_json::from_value(value).unwrap()
}
fn answer(answer: Answer) -> Value {
    serde_json::to_value(&answer).unwrap()
}

#[test]
fn a_core_homed_store_requires_the_host_identity_and_counts_its_writes() {
    let mut db = Connection::open_in_memory().unwrap();
    let get = json!({"request":{"store":"ui_preferences","operation":{"action":"get","keys":["active_project"]}}});
    let unbound = execute(&mut db, None, call(get.clone()), None).unwrap_err();
    assert!(unbound.contains("binds its identity"), "{unbound}");
    let first = answer(execute(&mut db, Some(&owner()), call(get.clone()), None).unwrap());
    assert_eq!(first["generation"], 0);
    assert_eq!(first["owner"], owner().owner());
    let set = json!({"request":{"store":"ui_preferences","operation":{"action":"set","key":"active_project","value":"kigali_north","now":7}}});
    assert_eq!(answer(execute(&mut db, Some(&owner()), call(set), None).unwrap())["generation"], 1);
    let read = answer(execute(&mut db, Some(&owner()), call(get), None).unwrap());
    assert_eq!(read["generation"], 1);
    assert_eq!(read["values"]["active_project"], "kigali_north");
    assert!(read.get("effects").is_none());
}

#[test]
fn a_flow_begun_under_one_identity_cannot_write_under_another() {
    let mut db = Connection::open_in_memory().unwrap();
    let a = owner();
    let b = identity(Principal::Account("someone-else".into()));
    let fenced = json!({"request":{"store":"ui_preferences","operation":{"action":"set","key":"active_project","value":"kigali_north","now":7}},
        "expect_owner": a.owner()});
    let error = execute(&mut db, Some(&b), call(fenced.clone()), None).unwrap_err();
    assert!(error.starts_with(OWNER_CHANGED), "{error}");
    assert_eq!(answer(execute(&mut db, Some(&a), call(fenced), None).unwrap())["values"]["active_project"], "kigali_north");
}

#[test]
fn the_anonymous_principal_is_its_own_identity() {
    let anonymous = identity(Principal::Anonymous);
    assert!(anonymous.validate().is_ok());
    assert_ne!(anonymous.owner(), owner().owner());
    assert_ne!(
        anonymous.owner(),
        identity(Principal::Account("anonymous".into())).owner(),
        "no uid can collide with the reserved anonymous principal"
    );
    assert_eq!(
        serde_json::to_value(&anonymous).unwrap()["principal"],
        json!("anonymous")
    );
    assert_eq!(
        serde_json::to_value(owner()).unwrap()["principal"],
        json!({"account": "owner"})
    );
}

#[test]
fn a_refused_operation_leaves_no_partial_write() {
    let mut db = Connection::open_in_memory().unwrap();
    let bad = json!({"request":{"store":"ui_preferences","operation":{"action":"set","key":"active_project","value":"a/b","now":7}}});
    assert!(execute(&mut db, Some(&owner()), call(bad), None).is_err());
    let tables: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name IN ('ui_preferences','durable_store_generations')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0, "the failed transaction rolled back even its schema step");
}

#[test]
fn the_two_statement_protocol_stores_run_through_the_shared_executor() {
    let mut db = Connection::open_in_memory().unwrap();
    let scope = json!({"principal":"owner","lane":"canary","audience":"a","project":"template"});
    let write = json!({"request":{"store":"status","operation":{"action":"write","scope":scope,"document":{"transformers":[{"name":"TX-1"}]},"now":42}}});
    execute(&mut db, None, call(write), None).unwrap();
    let read = answer(
        execute(
            &mut db,
            None,
            call(json!({"request":{"store":"status","operation":{"action":"read","scope":scope}}})),
            None,
        )
        .unwrap(),
    );
    assert_eq!(read["observation"]["document"]["transformers"][0]["name"], "TX-1");
    assert_eq!(read["revision"], 1);
    assert!(read.get("generation").is_none(), "statement stores keep their own revision");
    let held = answer(
        execute(
            &mut db,
            None,
            call(json!({"request":{"store":"grid_project_catalog","operation":{"action":"read","scope":{
                "principal":"owner","lane":"canary","audience":"a".repeat(64),"project":"template"}}}})),
            None,
        )
        .unwrap(),
    );
    assert!(held.get("held").is_some(), "{held}");
    let fenced = json!({"request":{"store":"status","operation":{"action":"read","scope":scope}},"expect_owner":"x"});
    assert!(execute(&mut db, None, call(fenced), None).is_err());
}

#[test]
fn core_serves_only_stores_homed_in_its_protected_store() {
    assert!(call(json!({"request":{"store":"ui_preferences","operation":{"action":"get","keys":["active_project"]}}}))
        .request
        .core_homed()
        .is_ok());
    for store in [
        json!({"request":{"store":"status","operation":{"action":"schema"}}}),
        json!({"request":{"store":"grid_project_catalog","operation":{"action":"schema"}}}),
    ] {
        let error = call(store).request.core_homed().unwrap_err();
        assert!(error.starts_with(NOT_CORE), "{error}");
    }
    assert!(serde_json::from_value::<Call>(json!({"request":{"store":"indexeddb","operation":{}}})).is_err());
    assert!(serde_json::from_value::<Call>(json!({"request":{"store":"ui_preferences","operation":{"action":"get","keys":["active_project"]}},"identity":{}})).is_err(),
        "a caller never states an identity");
}

#[test]
fn effects_are_a_closed_vocabulary_with_kernel_named_files() {
    let effect = Effect::WriteFile {
        file: EffectFile::PushProjection,
        contents: "{}".into(),
    };
    assert_eq!(
        serde_json::to_value(&effect).unwrap(),
        json!({"effect":"write_file","file":"push_projection","contents":"{}"})
    );
    assert_eq!(EffectFile::PushProjection.path(), "ds-kernel-effects/push-projection.json");
    assert!(serde_json::from_value::<Effect>(json!({"effect":"write_file","file":"/etc/passwd","contents":""})).is_err());
    assert!(effects_before(&call(json!({"request":{"store":"ui_preferences","operation":{"action":"get","keys":["active_project"]}}}))).is_empty());
}

#[test]
fn bytes_are_bounded_in_both_directions() {
    let mut db = Connection::open_in_memory().unwrap();
    let get = json!({"request":{"store":"ui_preferences","operation":{"action":"get","keys":["active_project"]}}});
    assert!(execute(&mut db, Some(&owner()), call(get.clone()), Some(vec![0; MAX_BYTES + 1])).is_err());
    let refused = execute(&mut db, Some(&owner()), call(get), Some(vec![1])).unwrap_err();
    assert!(refused.contains("no payload bytes"), "{refused}");
}

/// Drive a native port call, which completes without suspending.
fn ready<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(value) => value,
        std::task::Poll::Pending => panic!("a native store call completes at once"),
    }
}

#[test]
fn the_native_port_runs_the_same_transaction_and_fences_a_flow_by_owner() {
    use port::StorePort;
    let mut db = Connection::open_in_memory().unwrap();
    let port = port::Direct::new(&mut db, owner());
    let request = |value: Value| serde_json::from_value::<Request>(value).unwrap();
    let set = request(json!({"store":"ui_preferences","operation":{"action":"set","key":"active_project","value":"kigali_north","now":7}}));
    let written = ready(port.call(set.clone(), None, None)).unwrap();
    assert_eq!(written.owner(), Some(owner().owner().as_str()));
    assert_eq!(written.answer["generation"], 1);
    let again = ready(port.call(set.clone(), Some(owner().owner()), None)).unwrap();
    assert_eq!(again.answer["generation"], 2);
    let stale = ready(port.call(set, Some("0".repeat(64)), None)).unwrap_err();
    assert!(stale.starts_with(OWNER_CHANGED), "{stale}");
    let status = request(json!({"store":"status","operation":{"action":"read","scope":
        {"principal":"owner","lane":"canary","audience":"a","project":"p"}}}));
    let held = ready(port.call(status, None, None)).unwrap();
    assert!(held.owner().is_none(), "statement stores carry their callers' scopes");
}
