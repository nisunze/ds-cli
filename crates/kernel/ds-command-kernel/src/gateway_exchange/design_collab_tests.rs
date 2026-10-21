use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn call(surface: Surface, value: Value) -> DesignCollab {
    DesignCollab {
        surface,
        request: value.as_object().cloned().unwrap(),
    }
}

fn answered(collab: DesignCollab, body: Value) -> Result<Value, Refusal> {
    let Step::Fetch { continuation, .. } = start(collab) else {
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
fn each_surface_sends_its_actions_to_its_own_route() {
    let read = call(
        Surface::Selections,
        json!({"action": "list", "project_id": "it_rwanda", "include_archived": null}),
    )
    .plan()
    .unwrap();
    assert_eq!(read.request.path, "/api/v1/design/selections");
    assert_eq!(
        read.request.body,
        Some(json!({"action": "list", "project_id": "it_rwanda"}))
    );
    assert_eq!(read.retries, 1);

    let tags = call(
        Surface::Annotations,
        json!({"action": "set_tags", "project_id": "it_rwanda", "tags": []}),
    )
    .plan()
    .unwrap();
    assert_eq!(tags.request.path, "/api/v1/design/annotations");
    assert_eq!(tags.retries, 0);

    let finalize = call(
        Surface::Attachments,
        json!({"action": "finalize_upload", "project_id": "it_rwanda", "finalize": {}}),
    )
    .plan()
    .unwrap();
    assert_eq!(finalize.request.path, "/api/v1/design/attachments");
    assert_eq!(finalize.request.timeout_ms, FINALIZE_MS);
    assert_eq!(finalize.retries, 0);
}

#[test]
fn version_revisions_go_to_the_data_route_and_the_rest_to_design_versions() {
    let catalog = call(
        Surface::Versions,
        json!({"action": "get_version_catalog", "project_id": "it_rwanda",
            "object": {"kind": "lv_transformer", "id": "tx"}}),
    )
    .plan()
    .unwrap();
    assert_eq!(catalog.request.path, "/api/v1/design/versions");
    assert_eq!(catalog.retries, 1);

    let snapshot = call(
        Surface::Versions,
        json!({"action": "get_transformer_version_revision", "eds_project_id": "it_rwanda",
            "catalog_revision_read": {"transformer_id": "tx"}}),
    )
    .plan()
    .unwrap();
    assert_eq!(snapshot.request.path, "/api/v1/data");
    assert_eq!(snapshot.request.response_cap_bytes, SNAPSHOT_CAP);
    assert_eq!(snapshot.retries, 1);

    let promote = call(
        Surface::Versions,
        json!({"action": "promote_transformer_version_revision", "eds_project_id": "it_rwanda",
            "catalog_promotion": {}}),
    )
    .plan()
    .unwrap();
    assert_eq!(promote.request.path, "/api/v1/data");
    assert_eq!(promote.retries, 0);
}

#[test]
fn an_action_the_surface_does_not_serve_or_a_missing_project_is_refused() {
    let refused = call(
        Surface::Selections,
        json!({"action": "set_tags", "project_id": "p"}),
    )
    .plan()
    .unwrap_err();
    assert_eq!(refused.code, INPUT_INVALID);
    let unnamed = call(Surface::Annotations, json!({"action": "list_tags"}))
        .plan()
        .unwrap_err();
    assert_eq!(unnamed.code, INPUT_INVALID);
    let oversized = call(
        Surface::Annotations,
        json!({"action": "set_tags", "project_id": "p", "tags": "x".repeat(REQUEST_MAX_BYTES)}),
    )
    .plan()
    .unwrap_err();
    assert_eq!(oversized.code, INPUT_INVALID);
}

#[test]
fn the_answer_is_brains_data_object() {
    let collab = || {
        call(
            Surface::Annotations,
            json!({"action": "list_tags", "project_id": "p"}),
        )
    };
    assert_eq!(
        answered(collab(), json!({"success": true, "data": {"tags": []}})).unwrap(),
        json!({"tags": []})
    );
    assert_eq!(
        answered(collab(), json!({"tags": [1]})).unwrap(),
        json!({"tags": [1]})
    );
    assert_eq!(
        answered(collab(), json!({"data": [1]})).unwrap_err().code,
        INVALID
    );
}
