use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn answered<C: Exchange>(call: C, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else {
        panic!("expected a fetch")
    };
    let observed = Observed::Response {
        status: 200,
        content_type: Some("application/json".into()),
        retry_after: None,
    };
    match resume(
        continuation,
        observed,
        body.to_string().as_bytes(),
        &Facts::default(),
    ) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

#[test]
fn a_snapshot_is_projected_into_the_pages_spelling() {
    let Step::Fetch { request, .. } = start(SnapshotRead { revision: Some(4) }) else {
        panic!()
    };
    assert_eq!(request.body, Some(json!({"action": "get", "revision": 4})));
    let snapshot = answered(SnapshotRead::default(), json!({"revision": 4, "snapshot": {
        "revision": 4, "updated_at": "2026-10-20", "command_id": "c-1",
        "chapters": [{"id": "core", "title_key": "arch.core", "description_key": "arch.core.d"}],
        "nodes": [{"id": "kernel", "chapter": "core", "kind": "library", "lane": "edge", "label": "Kernel",
            "description_key": "k", "container_id": "edge", "column": 1, "row": 2,
            "delivery_state": "implemented", "evidence": [{"repository": "ds-command-kernel", "path": "src/lib.rs", "note": "L1"}]}],
        "edges": [{"id": "e1", "from": "web", "to": "kernel", "kind": "call", "detail": "d", "confidence": "proven"}]}})).unwrap();
    let page = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(page["updatedAt"], "2026-10-20");
    assert_eq!(page["commandId"], "c-1");
    assert_eq!(page["chapters"][0]["titleKey"], "arch.core");
    assert_eq!(page["chapters"][0]["nodes"], json!([]));
    assert_eq!(page["nodes"][0]["parent"], "edge");
    assert_eq!(page["nodes"][0]["descriptionKey"], "k");
    assert_eq!(page["nodes"][0]["deliveryState"], "implemented");
    assert_eq!(
        page["nodes"][0]["evidence"][0],
        json!({"repo": "ds-command-kernel", "path": "src/lib.rs", "anchor": "L1"})
    );
    assert_eq!(page["edges"][0]["confidence"], "proven");
    assert_eq!(
        answered(SnapshotRead::default(), json!({"revision": 4}))
            .unwrap_err()
            .code,
        INVALID
    );
}

#[test]
fn planning_commands_are_sent_as_brain_reads_them() {
    let command: PlanningCommand = serde_json::from_value(json!({"action": "preview", "expected_revision": 4,
        "idempotency_key": "c-2", "command": {"id": "c-2", "kind": "archive_node", "target_id": "kernel"}})).unwrap();
    let Step::Fetch { request, .. } = start(Planning { command }) else {
        panic!()
    };
    assert_eq!(request.body.unwrap()["command"]["kind"], "archive_node");
    let history: PlanningCommand =
        serde_json::from_value(json!({"action": "history", "limit": 20})).unwrap();
    assert!(answered(
        Planning { command: history },
        json!({"revisions": [], "next_cursor": ""})
    )
    .is_ok());
    let unbounded: PlanningCommand =
        serde_json::from_value(json!({"action": "history", "limit": 0})).unwrap();
    assert!(
        matches!(start(Planning { command: unbounded }), Step::Refused(r) if r.code == INPUT_INVALID)
    );
    assert!(serde_json::from_value::<PlanningCommand>(json!({"action": "drop"})).is_err());
}
