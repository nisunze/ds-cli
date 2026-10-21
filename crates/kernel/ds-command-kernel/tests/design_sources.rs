use ds_command_kernel::design::resolve_sources;
use serde_json::{json, Value};
fn resolve(kind: &str, project: Value, user: Value) -> Result<Value, String> {
    resolve_sources(&serde_json::to_vec(&json!({"schema":"ds.design.source-resolution/v1","kind":kind,"project":project,"user":user})).unwrap())
}
#[test]
fn explicit_authority_empty_and_absent_are_different() {
    let project = json!({"addresses":["survey"],"labels":["Survey"]});
    let user = json!({"addresses":["sketch"],"labels":["Sketch"]});
    assert_eq!(
        resolve("customers", project, user.clone()).unwrap()["scope"],
        "project"
    );
    assert_eq!(
        resolve("customers", Value::Null, user.clone()).unwrap()["scope"],
        "user"
    );
    assert_eq!(
        resolve("customers", Value::Null, Value::Null).unwrap()["addresses"][0],
        "design://layer/customers"
    );
    let empty = json!({"addresses":[],"labels":[]});
    assert!(resolve("customers", empty.clone(), user.clone()).is_err());
    let poles = resolve("poles", empty, user).unwrap();
    assert_eq!(poles["scope"], "project");
    assert_eq!(poles["addresses"], json!([]));
}
#[test]
fn unknown_shape_duplicate_and_mismatched_labels_are_refused() {
    assert!(resolve(
        "customers",
        json!({"addresses":["x","x"],"labels":["X","X"]}),
        Value::Null
    )
    .is_err());
    assert!(resolve(
        "customers",
        json!({"addresses":["x"],"labels":[]}),
        Value::Null
    )
    .is_err());
    assert!(resolve_sources(br#"{"schema":"ds.design.source-resolution/v1","kind":"poles","project":null,"user":null,"fallback":true}"#).is_err());
}
