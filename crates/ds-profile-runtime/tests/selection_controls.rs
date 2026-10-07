use ds_profile_runtime::{Request, Runtime};
use serde_json::{json, Value};
fn request(scope: &str, action: Value) -> Request {
    serde_json::from_value(json!({"operation":"selection_controls","scope":scope,"space":"profile","available_families":["structures","terrain_points"],"request":action})).unwrap()
}
#[test]
fn native_control_state_is_scoped_atomic_and_shared_across_ui_cli_requests() {
    let mut runtime = Runtime::default();
    let read = || json!({"action":"read"});
    let default = runtime.execute(request("project/model", read())).unwrap();
    assert_eq!(default["state"]["predicate"], "within");
    assert_eq!(
        default["resolved_families"],
        json!(["structures", "terrain_points"])
    );
    runtime
        .execute(request(
            "project/model",
            json!({"action":"configure","patch":{"predicate":"outside","families":["structures"]}}),
        ))
        .unwrap();
    let query = json!({"model_id":"model","expected_revision":"rev","space":"profile","polygon":[[0,0],[10,0],[10,10]]});
    let admitted = runtime
        .execute(request(
            "project/model",
            json!({"action":"admit","query":query}),
        ))
        .unwrap();
    assert_eq!(admitted["query"]["predicate"], "outside");
    assert_eq!(admitted["query"]["families"], json!(["structures"]));
    assert_eq!(
        runtime.execute(request("project/other", read())).unwrap(),
        default
    );
    let before = runtime.execute(request("project/model", read())).unwrap();
    runtime.execute(request("project/model", json!({"action":"admit","query":{"model_id":"model","expected_revision":"rev","space":"profile","polygon":[[0,0],[10,0],[10,10]],"mode":"add","predicate":"within","families":["terrain_points"]}}))).unwrap();
    assert_eq!(
        runtime.execute(request("project/model", read())).unwrap(),
        before
    );

    assert!(runtime.execute(request("project/model",json!({"action":"configure","patch":{"predicate":"within","families":["structures","structures"]}}))).is_err());
    assert_eq!(
        runtime.execute(request("project/model", read())).unwrap(),
        before
    );
    let composed = runtime
        .execute(request(
            "project/model",
            json!({"action":"compose","mode":"remove","current":["a","b","a"],"hits":["b"]}),
        ))
        .unwrap();
    assert_eq!(composed["entity_ids"], json!(["a"]));
    assert_eq!(composed["primary"], "a");
    runtime
        .execute(request(
            "project/model",
            json!({"action":"configure","patch":{"families":null}}),
        ))
        .unwrap();
    assert_eq!(
        runtime.execute(request("project/model", read())).unwrap()["state"]["families"],
        Value::Null
    );
}
