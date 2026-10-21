use super::*;
use crate::durable_store::{Identity, Principal};

const SHA: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const GROUP: &str = "tmg-0123456789abcdef01234567";

fn who(principal: Principal) -> Identity {
    Identity {
        principal,
        lane: "canary".into(),
        audience: "ds-prod".into(),
    }
}
fn alice() -> Identity {
    who(Principal::Account("alice".into()))
}
fn bob() -> Identity {
    who(Principal::Account("bob".into()))
}
fn db() -> Connection {
    Connection::open_in_memory().unwrap()
}
fn outcome(connection: &Connection, identity: &Identity, value: Value) -> Result<Outcome, String> {
    let operation = serde_json::from_value(value).map_err(|e| e.to_string())?;
    execute(connection, Input { identity, bytes: None }, operation)
}
fn run(connection: &Connection, identity: &Identity, value: Value) -> Result<Value, String> {
    outcome(connection, identity, value).map(|outcome| Value::Object(outcome.value))
}
fn ok(connection: &Connection, identity: &Identity, value: Value) -> Value {
    run(connection, identity, value).unwrap()
}
fn result(id: &str, model: &str, created: u64) -> Value {
    json!({"result_id": id, "model_id": model, "pinned_content_root": "rev:abc", "check_type": "span_limit",
           "created": created, "artifact": {"result_id": id, "payload": {"rows": [1, 2]}}})
}
fn provenance() -> Value {
    json!({"imported_from": null, "template_origin": null, "example_origin": null, "version_origin": null,
           "read_only": false, "temp_group_id": null, "workspace_owner": "temporary"})
}
fn session(selected: Option<&str>, open: &[&str]) -> Value {
    json!({
        "selected_model_id": selected, "selected_revision_id": selected.map(|_| "rev:1"),
        "selected_provenance": provenance(),
        "view_state": {"layer_visibility": {"structures": true}, "model_layer_visibility": {"m-1": {"spans": false}}, "grouping_mode": "source"},
        "last_selected": 5,
        "open_models": open.iter().map(|model| json!({"model_id": model, "provenance": provenance()})).collect::<Vec<_>>(),
    })
}
fn begin(connection: &Connection, identity: &Identity) -> Value {
    ok(
        connection,
        identity,
        json!({"action":"begin_group","name":"Line A","source":{"filename":"Line A.dsgrid","sha256":SHA,"byte_size":42},
               "created_from":"combine","now":100}),
    )
}
fn settle(connection: &Connection, model: &str) -> Value {
    ok(
        connection,
        &alice(),
        json!({"action":"settle_group","group_id":GROUP,"status":{"kind":"ready"},
               "model":{"session_model_id":model,"active_revision":"rev:9"}}),
    )
}

#[test]
fn results_are_retained_per_model_newest_first_and_per_identity() {
    let connection = db();
    ok(&connection, &alice(), json!({"action":"put_result","result":result("r-1","m-1",10)}));
    ok(&connection, &alice(), json!({"action":"put_result","result":result("r-2","m-1",20)}));
    ok(&connection, &alice(), json!({"action":"put_result","result":result("r-3","m-2",30)}));
    let listed = ok(&connection, &alice(), json!({"action":"list_results","model_id":"m-1"}));
    let ids: Vec<&str> = listed["results"].as_array().unwrap().iter().map(|r| r["result_id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["r-2", "r-1"]);
    assert_eq!(listed["results"][0], result("r-2", "m-1", 20));
    assert_eq!(ok(&connection, &bob(), json!({"action":"list_results","model_id":"m-1"}))["results"], json!([]));
    // An artifact must be the result it claims to be.
    let mut forged = result("r-4", "m-1", 1);
    forged["artifact"]["result_id"] = json!("other");
    assert!(run(&connection, &alice(), json!({"action":"put_result","result":forged})).is_err());
}

#[test]
fn each_kind_advances_its_own_generation() {
    let connection = db();
    let put = outcome(&connection, &alice(), json!({"action":"put_result","result":result("r-1","m-1",1)})).unwrap();
    assert!(put.wrote);
    assert_eq!(put.generation_key.as_deref(), Some("grid_model_store.results"));
    let read = outcome(&connection, &alice(), json!({"action":"list_groups"})).unwrap();
    assert!(!read.wrote);
    assert_eq!(read.generation_key.as_deref(), Some("grid_model_store.groups"));
    let session = outcome(&connection, &alice(), json!({"action":"put_session","session":session(None,&[])})).unwrap();
    assert_eq!(session.generation_key.as_deref(), Some("grid_model_store.sessions"));
}

#[test]
fn a_group_is_derived_from_its_source_and_settles_its_status() {
    let connection = db();
    let begun = begin(&connection, &alice());
    assert_eq!(begun["group"]["group_id"], GROUP);
    assert_eq!(begun["group"]["status"], json!({"kind":"importing"}));
    assert_eq!(settle(&connection, "model-1")["group"]["session_model_id"], "model-1");
    // A refusal keeps the model it had unless it names one.
    let blocked = ok(
        &connection,
        &alice(),
        json!({"action":"settle_group","group_id":GROUP,"status":{"kind":"blocked","code":"grid_host_error","message":"boom"}}),
    );
    assert_eq!(blocked["group"]["session_model_id"], "model-1");
    assert_eq!(blocked["group"]["status"]["code"], "grid_host_error");
    assert_eq!(ok(&connection, &alice(), json!({"action":"list_groups"}))["groups"].as_array().unwrap().len(), 1);
    assert!(ok(&connection, &bob(), json!({"action":"get_group","group_id":GROUP}))["group"].is_null());
    let unknown = run(&connection, &alice(), json!({"action":"settle_group","group_id":"tmg-x","status":{"kind":"ready"}})).unwrap_err();
    assert!(unknown.starts_with(UNKNOWN_GROUP), "{unknown}");
    // Beginning the same source again starts that group over.
    let again = begin(&connection, &alice());
    assert_eq!(again["group"]["status"]["kind"], "importing");
    assert!(again["group"]["session_model_id"].is_null());
    let bad = run(
        &connection,
        &alice(),
        json!({"action":"begin_group","name":"X","source":{"filename":"x","sha256":"sha256:short","byte_size":1},"created_from":"drop","now":1}),
    );
    assert!(bad.unwrap_err().starts_with(INVALID));
}

#[test]
fn a_session_is_remembered_per_project_and_outside_any_project() {
    let connection = db();
    ok(&connection, &alice(), json!({"action":"put_session","project":"kigali","session":session(Some("m-1"),&["m-1","m-2"])}));
    ok(&connection, &alice(), json!({"action":"put_session","session":session(None,&["m-3"])}));
    assert_eq!(ok(&connection, &alice(), json!({"action":"get_session","project":"kigali"}))["session"], session(Some("m-1"), &["m-1", "m-2"]));
    assert_eq!(ok(&connection, &alice(), json!({"action":"get_session"}))["session"], session(None, &["m-3"]));
    assert!(ok(&connection, &alice(), json!({"action":"get_session","project":"musanze"}))["session"].is_null());
    assert!(ok(&connection, &bob(), json!({"action":"get_session","project":"kigali"}))["session"].is_null());
    ok(&connection, &alice(), json!({"action":"clear_session","project":"kigali"}));
    assert!(ok(&connection, &alice(), json!({"action":"get_session","project":"kigali"}))["session"].is_null());
    let mut bad = session(None, &[]);
    bad["view_state"]["layer_visibility"]["x"] = json!("yes");
    assert!(run(&connection, &alice(), json!({"action":"put_session","session":bad})).is_err());
    assert!(run(&connection, &alice(), json!({"action":"put_session","project":"a/b","session":session(None,&[])})).is_err());
}

#[test]
fn removing_a_model_removes_its_groups_results_and_sessions_in_one_transaction() {
    let connection = db();
    begin(&connection, &alice());
    settle(&connection, "m-1");
    ok(&connection, &alice(), json!({"action":"put_result","result":result("r-1","m-1",1)}));
    ok(&connection, &alice(), json!({"action":"put_result","result":result("r-2","m-2",1)}));
    ok(&connection, &alice(), json!({"action":"put_session","project":"kigali","session":session(Some("m-1"),&["m-1"])}));
    ok(&connection, &alice(), json!({"action":"put_session","session":session(Some("m-2"),&["m-2","m-1"])}));
    // Removing by group alone resolves the model it backs.
    let removed = ok(&connection, &alice(), json!({"action":"remove_model","group_id":GROUP}));
    assert_eq!(removed["model_id"], "m-1");
    assert_eq!(removed["removed_group_ids"], json!([GROUP]));
    assert_eq!(removed["removed_result_count"], 1);
    assert!(ok(&connection, &alice(), json!({"action":"get_session","project":"kigali"}))["session"].is_null());
    // A background mention forgets only that model.
    assert_eq!(ok(&connection, &alice(), json!({"action":"get_session"}))["session"], session(Some("m-2"), &["m-2"]));
    assert_eq!(ok(&connection, &alice(), json!({"action":"list_results","model_id":"m-2"}))["results"].as_array().unwrap().len(), 1);
    assert!(run(&connection, &alice(), json!({"action":"remove_model"})).is_err());
}

#[test]
fn results_groups_and_sessions_take_no_bytes() {
    let connection = db();
    let identity = alice();
    let refused = execute(
        &connection,
        Input { identity: &identity, bytes: Some(vec![1]) },
        serde_json::from_value(json!({"action":"list_groups"})).unwrap(),
    )
    .unwrap_err();
    assert!(refused.starts_with(INVALID), "{refused}");
}
