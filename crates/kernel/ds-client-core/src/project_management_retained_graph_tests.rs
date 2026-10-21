use super::*;
fn graph() -> Value {
    json!({"project_id":"p","revision":3,"schedule_revision":2,"calendar":{},"tasks":[],"dependencies":[],"residual_items":[],"permissions":{"view":true,"comment":false,"contribute":false,"edit_schedule":false,"admin":false}})
}
#[test]
fn canonical_graph_requires_explicit_scope_revision_shape_and_captured_read_permission() {
    verify_graph("p", &graph()).unwrap();
    for (key, value) in [
        ("project_id", json!("other")),
        ("revision", json!(-1)),
        ("schedule_revision", Value::Null),
        ("tasks", json!({})),
        ("permissions", json!({})),
        ("retained", json!({})),
        ("correspondence", json!([])),
    ] {
        let mut bad = graph();
        bad[key] = value;
        assert!(verify_graph("p", &bad).is_err(), "{key}");
    }
    let mut denied = graph();
    denied["permissions"]["view"] = json!(false);
    assert!(verify_graph("p", &denied).is_err());
}
#[test]
fn missing_legacy_correspondence_remains_distinct_from_partial_capture() {
    let old = graph();
    verify_graph("p", &old).unwrap();
    assert!(old.get("correspondence").is_none());
    let mut partial = graph();
    partial["correspondence"] = json!({"records":[],"truncated":true,"records_total":10,"as_of":"2026-10-13","due_window_days":7,"blocked_tasks":[],"counters":{"recordsOutstanding":10,"recordsOverdue":1,"tasksAwaitingCorrespondence":0}});
    verify_graph("p", &partial).unwrap();
    assert_eq!(partial["correspondence"]["truncated"], true);
    partial["correspondence"]
        .as_object_mut()
        .unwrap()
        .remove("truncated");
    assert!(verify_graph("p", &partial).is_err());
}
