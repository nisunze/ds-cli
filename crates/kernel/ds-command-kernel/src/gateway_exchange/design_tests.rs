use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step, PROJECT_INVALID};
use serde_json::json;

fn call(value: Value) -> DesignCall {
    serde_json::from_value(value).expect("a valid design call")
}

fn request(value: Value) -> Request {
    match start(call(value)) {
        Step::Fetch { request, .. } => request,
        Step::Refused(refusal) => panic!("refused: {}", refusal.message),
        _ => panic!("expected a fetch"),
    }
}

fn refused(value: Value) -> Refusal {
    match start(call(value)) {
        Step::Refused(refusal) => refusal,
        _ => panic!("expected a refusal"),
    }
}

fn answered(value: Value, status: u16, body: Value) -> Result<Value, Refusal> {
    let Step::Fetch { continuation, .. } = start(call(value)) else {
        panic!("expected a fetch")
    };
    match resume(
        continuation,
        Observed::Response {
            status,
            content_type: Some("application/json".into()),
            retry_after: None,
        },
        body.to_string().as_bytes(),
        &Facts {
            has_session: true,
            ..Facts::default()
        },
    ) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

fn layer() -> Value {
    json!({"type": "FeatureCollection", "features": []})
}

#[test]
fn the_project_configuration_is_one_encoded_get() {
    let request = request(json!({"action": "project_config", "project": "kigali#2"}));
    assert_eq!(request.method, Method::Get);
    assert_eq!(request.path, "/config/kigali%232");
    assert_eq!(request.body, None);
    assert_eq!(request.timeout_ms, 180_000);
    let document = answered(
        json!({"action": "project_config", "project": "it_rwanda"}),
        200,
        json!({"sheets": {"lv_poles": []}, "parameters": {}}),
    )
    .unwrap();
    assert_eq!(document["sheets"]["lv_poles"], json!([]));
}

#[test]
fn a_configuration_sheet_save_is_fenced_by_the_rows_it_was_edited_from() {
    let request = request(json!({"action": "save_config_sheet", "project": "it_rwanda",
        "sheet": "lv_poles", "rows": [{"a": 1}], "expected": [{"a": 0}]}));
    assert_eq!(request.path, "/report");
    assert_eq!(
        request.body,
        Some(json!({"action": "save_config", "eds_project_id": "it_rwanda", "sheet_name": "lv_poles",
            "data": [{"a": 1}], "expected_data": [{"a": 0}]}))
    );
    let conflict = answered(
        json!({"action": "save_config_sheet", "project": "it_rwanda", "sheet": "lv_poles", "rows": []}),
        409,
        json!({"detail": "sheet lv_poles changed since it was read"}),
    )
    .unwrap_err();
    assert_eq!(conflict.status, Some(409));
    assert_eq!(conflict.code, "HTTP_409");
}

#[test]
fn transformer_reads_name_their_batch_and_admit_each_document() {
    let request = request(json!({"action": "read_transformers", "project": "it_rwanda",
        "transformers": [" tr_1 ", "tr_2"], "fields": "context"}));
    assert_eq!(request.path, "/api/v1/data");
    assert_eq!(
        request.body,
        Some(json!({"action": "get_transformers_data", "eds_project_id": "it_rwanda",
            "transformers": ["tr_1", "tr_2"], "fields": "context"}))
    );
    let read = json!({"action": "read_transformers", "project": "it_rwanda", "transformers": ["tr_1", "tr_2"]});
    let answer = answered(read.clone(), 200, json!({"success": true, "data": {"total": 2, "found_count": 1,
        "failed_count": 1, "results": [
            {"transformer_name": "tr_1", "ok": true, "data": {"layers": {"poles": layer()}, "metadata": {"version": 4}}},
            {"transformer_name": "tr_2", "ok": false, "error": "transformer not found", "error_code": "NOT_FOUND"}
        ]}}))
    .unwrap();
    assert_eq!(answer["results"][0]["data"]["metadata"]["version"], 4);
    assert_eq!(answer["results"][1]["error_code"], "NOT_FOUND");
    let broken = answered(read, 200, json!({"success": true, "data": {"results": [
        {"transformer_name": "tr_1", "ok": true, "data": {}}]}}));
    assert_eq!(broken.unwrap_err().code, ANSWER_INVALID);
}

#[test]
fn a_save_carries_each_edit_with_its_fence_and_never_an_empty_document() {
    let request = request(json!({"action": "save_transformers", "project": "it_rwanda", "saves": [
        {"transformer": "tr_1", "layers": {"poles": layer()}, "base_version": 3,
            "process_metadata": {"lv_process": {}}, "operation_id": "op-1"},
        {"transformer": "tr_2", "layers": {"poles": layer()}, "overwrite_reviewed_version": 7}
    ]}));
    assert_eq!(request.timeout_ms, 180_000);
    let body = request.body.unwrap();
    assert_eq!(body["action"], "save_transformers_data");
    assert_eq!(
        body["transformer_saves"][0],
        json!({"transformer_name": "tr_1", "gdfs": {"poles": layer()}, "base_version": 3,
            "process_metadata": {"lv_process": {}}, "operation_id": "op-1"})
    );
    assert_eq!(body["transformer_saves"][1]["force_version_conflict"], true);
    assert_eq!(body["transformer_saves"][1]["reviewed_current_version"], 7);

    for bad in [
        json!({"action": "save_transformers", "project": "it_rwanda", "saves": []}),
        json!({"action": "save_transformers", "project": "it_rwanda", "saves": [
            {"transformer": "tr_1", "layers": {}}]}),
        json!({"action": "save_transformers", "project": "it_rwanda", "saves": [
            {"transformer": "tr_1", "layers": {"poles": layer()}, "overwrite_reviewed_version": 0}]}),
    ] {
        assert_eq!(refused(bad).code, INPUT_INVALID);
    }
}

#[test]
fn a_save_above_the_gateway_body_bound_never_leaves() {
    let big = "x".repeat(MAX_BODY_BYTES);
    let refusal = refused(json!({"action": "save_transformers", "project": "it_rwanda", "saves": [
        {"transformer": "tr_1", "layers": {"poles": {"type": "FeatureCollection", "features": [], "pad": big}}}]}));
    assert_eq!(refusal.code, SAVE_TOO_LARGE);
}

#[test]
fn a_version_conflict_keeps_the_backend_code_and_versions() {
    let refusal = answered(
        json!({"action": "save_transformers", "project": "it_rwanda", "saves": [
            {"transformer": "tr_1", "layers": {"poles": layer()}, "base_version": 3}]}),
        409,
        json!({"success": false, "error": {"code": "TRANSFORMER_VERSION_MISMATCH", "message": "Save conflict",
            "details": {"current_version": 5, "base_version": 3}}}),
    )
    .unwrap_err();
    assert_eq!(refusal.code, "TRANSFORMER_VERSION_MISMATCH");
    assert_eq!(refusal.details, Some(json!({"current_version": 5, "base_version": 3})));
}

#[test]
fn the_edit_lease_carries_unsaved_work_evidence_and_admits_its_state() {
    let lease = json!({"action": "lease", "project": "it_rwanda", "transformer": "tr_1", "op": "heartbeat",
        "session": "tab-1", "mode": "fast", "unsaved": {"since": "2026-10-20T08:00:00Z",
        "local_digest": "abc", "feature_counts": {"poles": 3}}});
    assert_eq!(
        request(lease.clone()).body,
        Some(json!({"action": "lock_transformer", "eds_project_id": "it_rwanda", "transfo": "tr_1",
            "lock_op": "heartbeat", "session_id": "tab-1", "edit_mode": "fast", "lock_hold": true,
            "unsaved_since": "2026-10-20T08:00:00Z", "local_digest": "abc", "feature_counts": {"poles": 3}}))
    );
    let held = answered(lease.clone(), 200, json!({"locked": true, "locked_by": "a@example.com",
        "locked_at": "2026-10-20T08:00:00Z", "expires_at": "2026-10-20T08:05:00Z", "hold": true}))
    .unwrap();
    assert_eq!(held["hold"], true);
    let taken = answered(lease, 423, json!({"error_code": "TRANSFORMER_EDIT_LOCKED",
        "detail": "Transformer is locked by b@example.com.", "locked_by": "b@example.com"}))
    .unwrap_err();
    assert_eq!(taken.code, "TRANSFORMER_EDIT_LOCKED");
    let released = answered(
        json!({"action": "lease", "project": "it_rwanda", "transformer": "tr_1", "op": "release_hold",
            "session": "tab-1", "take_over": true}),
        200,
        json!({"released": true}),
    )
    .unwrap();
    assert_eq!(released, json!({"released": true}));
}

#[test]
fn drafts_digests_and_copies_name_their_rows() {
    assert_eq!(
        request(json!({"action": "get_draft", "project": "it_rwanda", "transformer": "tr_1",
            "user": "b@example.com"})).body,
        Some(json!({"action": "get_transformer_draft", "eds_project_id": "it_rwanda",
            "transformer_name": "tr_1", "draft_user": "b@example.com"}))
    );
    assert_eq!(
        request(json!({"action": "put_draft", "project": "it_rwanda", "transformer": "tr_1",
            "layers": {"poles": layer()}, "local_digest": "abc", "base_version": 2})).body,
        Some(json!({"action": "put_transformer_draft", "eds_project_id": "it_rwanda",
            "transformer_name": "tr_1", "gdfs": {"poles": layer()}, "local_digest": "abc", "base_version": 2}))
    );
    assert_eq!(
        request(json!({"action": "digest_status", "project": "it_rwanda", "digests": [
            {"transformer": "tr_1", "content_digest": "abc", "base_version": 2}]})).body,
        Some(json!({"action": "transformer_digest_status", "eds_project_id": "it_rwanda",
            "transformer_digests": [{"transformer_name": "tr_1", "content_digest": "abc", "base_version": 2}]}))
    );
    assert_eq!(
        request(json!({"action": "copy_transformers", "project": "it_rwanda", "target_project": "it_tchad",
            "transformers": ["tr_1"]})).body,
        Some(json!({"action": "copy_transformers", "eds_project_id": "it_rwanda",
            "target_project_id": "it_tchad", "transformers": ["tr_1"], "overwrite_existing": false}))
    );
    let digests = answered(
        json!({"action": "digest_status", "project": "it_rwanda", "digests": [{"transformer": "tr_1"}]}),
        200,
        json!({"success": true, "data": {"items": [{"transformer_name": "tr_1", "status": "current", "version": 2}]}}),
    )
    .unwrap();
    assert_eq!(digests["items"][0]["status"], "current");
}

#[test]
fn rename_delete_and_cleanup_carry_their_governance() {
    let rename = request(json!({"action": "rename", "project": "it_rwanda", "transformer": "tr_1", "to": "tr_9"}));
    assert_eq!(
        rename.body,
        Some(json!({"action": "rename_transformer", "eds_project_id": "it_rwanda", "transfo": "tr_1",
            "new_transformer_name": "tr_9", "governance_reason": "Rename transformer tr_1 to tr_9",
            "governance_acknowledged": true, "governance_confirm_phrase": "RENAME TRANSFORMER"}))
    );
    let renamed = answered(
        json!({"action": "rename", "project": "it_rwanda", "transformer": "tr_1", "to": "tr_9"}),
        200,
        json!({"message": "renamed", "old_name": "tr_1", "new_name": "tr_9"}),
    )
    .unwrap();
    assert_eq!(renamed["new_name"], "tr_9");
    let deleted = answered(
        json!({"action": "delete_transformers", "project": "it_rwanda", "transformers": ["tr_1", "tr_2"]}),
        200,
        json!({"results": [{"name": "tr_1"}, {"name": "tr_2", "error": "not yours"}],
            "deleted_count": 1, "failed_count": 1}),
    )
    .unwrap();
    assert_eq!(deleted["results"][1]["error"], "not yours");
    let cleanup = request(json!({"action": "cleanup_collision", "project": "it_rwanda", "collision": "c-1",
        "sources": ["tr_1", "tr_2"], "delete": ["tr_2"], "target": "tr_1", "overlap": "duplicated",
        "coverage_pct": 99.6, "confirm_phrase": "DELETE DUPLICATES"}));
    let body = cleanup.body.unwrap();
    assert_eq!(body["coverage_pct"], 100);
    assert_eq!(body["governance_reason"], "Collision cleanup c-1");
    assert_eq!(body["governance_confirm_phrase"], "DELETE DUPLICATES");
}

#[test]
fn compounded_reports_list_reads_an_absent_list_as_empty() {
    let listed = answered(
        json!({"action": "compounded_reports", "project": "it_rwanda"}),
        200,
        json!({"compounded_reports": null, "composition_template": {}}),
    )
    .unwrap();
    assert_eq!(listed["compounded_reports"], json!([]));
}

#[test]
fn status_reads_the_shared_status_request_and_admission() {
    let request = request(json!({"action": "transformer_status", "project": "it_rwanda"}));
    assert_eq!(
        request.body,
        Some(json!({"action": "list_transformers_status", "eds_project_id": "it_rwanda"}))
    );
    let document = answered(
        json!({"action": "transformer_status", "project": "it_rwanda"}),
        200,
        json!({"transformers": [{"name": "tr_1", "kind": "individual"}]}),
    )
    .unwrap();
    assert_eq!(document["transformers"][0]["name"], "tr_1");
    let broken = answered(
        json!({"action": "transformer_status", "project": "it_rwanda"}),
        200,
        json!({"rows": []}),
    );
    assert_eq!(broken.unwrap_err().code, ANSWER_INVALID);
}

#[test]
fn input_outside_the_contract_is_refused_before_a_request() {
    assert!(serde_json::from_value::<DesignCall>(json!({"action": "add_transfo_to_combined",
        "project": "it_rwanda"})).is_err());
    assert!(serde_json::from_value::<DesignCall>(json!({"action": "rename", "project": "it_rwanda",
        "transformer": "a", "to": "b", "extra": 1})).is_err());
    assert_eq!(
        refused(json!({"action": "read_transformers", "project": "", "transformers": ["a"]})).code,
        PROJECT_INVALID
    );
    assert_eq!(
        refused(json!({"action": "read_transformers", "project": "p", "transformers": ["a\u{7}"]})).code,
        INPUT_INVALID
    );
    let many: Vec<String> = (0..=MAX_BATCH).map(|n| format!("tr_{n}")).collect();
    assert_eq!(
        refused(json!({"action": "read_transformers", "project": "p", "transformers": many})).code,
        INPUT_INVALID
    );
}
