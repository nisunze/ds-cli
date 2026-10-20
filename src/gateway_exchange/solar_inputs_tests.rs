use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn row(value: Value) -> QueuedInput {
    serde_json::from_value(value).unwrap()
}

fn save() -> QueuedInput {
    row(json!({"kind": "solar_input", "id": "solar-input-1", "status": "pending", "attempts": 0,
        "root": "eds_project/it_rwanda/eds_solar", "templateId": "kigali", "subcollection": "01_city_inputs",
        "docId": "plant_config", "payload": {"pv_kwp": 120}}))
}

fn commit() -> QueuedInput {
    row(json!({"operation": "project_commit", "root": "eds_project/it_rwanda/eds_solar", "templateId": "huye",
        "subcollection": "__city__", "docId": "huye",
        "payload": {"snapshot_json": "{\"city\":1}", "snapshot_sha256": "a".repeat(64)}}))
}

fn answered(call: SendQueued, body: Value) -> Result<(), Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else { panic!("expected a fetch") };
    let observed = Observed::Response { status: 200, content_type: Some("application/json".into()), retry_after: None };
    match resume(continuation, observed, body.to_string().as_bytes(), &Facts::default()) {
        Step::Done(()) => Ok(()),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

#[test]
fn a_queued_edit_is_sent_as_the_input_it_names() {
    let Step::Fetch { request, .. } = start(SendQueued(save())) else { panic!("expected a fetch") };
    assert_eq!(request.path, "/api/v1/solar");
    assert_eq!(
        request.body,
        Some(json!({"action": "save_input", "template_id": "kigali", "subcollection": "01_city_inputs",
            "doc_id": "plant_config", "data": {"pv_kwp": 120}, "root": "eds_project/it_rwanda/eds_solar"}))
    );
    let receipt = json!({"success": true, "data": {"template_id": "kigali", "subcollection": "01_city_inputs", "doc_id": "plant_config"}});
    assert_eq!(answered(SendQueued(save()), receipt), Ok(()));
    let crossed = json!({"success": true, "data": {"template_id": "kigali", "subcollection": "01_city_inputs", "doc_id": "other"}});
    assert_eq!(answered(SendQueued(save()), crossed).unwrap_err().code, RECEIPT_INVALID);
}

#[test]
fn a_locally_created_city_commits_its_snapshot_once() {
    let Step::Fetch { request, .. } = start(SendQueued(commit())) else { panic!("expected a fetch") };
    assert_eq!(
        request.body,
        Some(json!({"action": "project_commit", "root": "eds_project/it_rwanda/eds_solar", "template_id": "huye",
            "project_commit": {"expected_base": "", "snapshot_json": "{\"city\":1}"}}))
    );
    let receipt = |sha: &str| json!({"success": true, "data": {"root": "eds_project/it_rwanda/eds_solar", "city_id": "huye",
        "client_snapshot_sha256": sha, "input_base_fingerprint": "b".repeat(64)}});
    assert_eq!(answered(SendQueued(commit()), receipt(&"a".repeat(64))), Ok(()));
    assert_eq!(answered(SendQueued(commit()), receipt(&"c".repeat(64))).unwrap_err().code, RECEIPT_INVALID);
}

#[test]
fn a_row_that_names_no_project_input_is_refused_before_sending() {
    for broken in [
        json!({"root": "eds_solar", "templateId": "k", "subcollection": "s", "docId": "d", "payload": {"a": 1}}),
        json!({"root": "eds_project/it_rwanda/eds_solar", "templateId": " ", "subcollection": "s", "docId": "d", "payload": {"a": 1}}),
        json!({"root": "eds_project/it_rwanda/eds_solar", "templateId": "k", "subcollection": "s", "docId": "d", "payload": {}}),
        json!({"operation": "delete", "root": "eds_project/it_rwanda/eds_solar", "templateId": "k", "subcollection": "s", "docId": "d", "payload": {"a": 1}}),
    ] {
        assert!(matches!(start(SendQueued(row(broken.clone()))), Step::Refused(refusal) if refusal.code == INPUT_INVALID), "{broken}");
    }
}
