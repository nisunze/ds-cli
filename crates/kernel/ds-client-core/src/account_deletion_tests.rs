use super::*;

#[test]
fn the_request_is_closed_to_the_confirmation_phrase() {
    let body = payload();
    let object = body.as_object().unwrap();
    assert_eq!(object.len(), 1);
    assert_eq!(body["confirmation"], CONFIRMATION);
    assert_eq!(
        AccountDeletionCall::GATEWAY_OPERATION.path_template(),
        "/api/v1/account/delete"
    );
    assert_eq!(
        AccountDeletionCall::GATEWAY_OPERATION.method().as_str(),
        "POST"
    );
}

#[test]
fn refusals_keep_the_route_code_and_scalar_details() {
    let body = serde_json::to_vec(&json!({
        "success": false,
        "error": {"code": "account_delete_incomplete", "message": "Account deletion did not finish",
                  "details": {"step": "storage_objects", "retryable": true, "nested": {"x": 1}}}
    }))
    .unwrap();
    let error = refusal(500, &body);
    assert_eq!(error.kind(), ErrorKind::Transient);
    let service = error.service_refusal().unwrap();
    assert_eq!(service.code(), Some("account_delete_incomplete"));
    assert_eq!(service.detail_text("step"), Some("storage_objects"));
    assert!(service.details().iter().all(|(k, _)| k != "nested"));

    let confirm = serde_json::to_vec(
        &json!({"success": false, "error": {"code": "account_delete_confirmation_required"}}),
    )
    .unwrap();
    assert_eq!(refusal(400, &confirm).kind(), ErrorKind::InvalidInput);
    assert_eq!(refusal(401, b"").kind(), ErrorKind::AuthenticationRejected);
    assert_eq!(
        refusal(404, b"not json").kind(),
        ErrorKind::RouteUnavailable
    );
    assert_eq!(refusal(503, b"").kind(), ErrorKind::Transient);
}

#[test]
fn receipts_accept_additive_members_and_refuse_a_different_claim() {
    let mut receipt = json!({
        "schema": SCHEMA, "receipt_id": "r-1", "state": "deleted", "already_deleted": false,
        "started_at": "2026-10-24T09:00:00Z", "deleted_at": "2026-10-24T09:00:02Z",
        "deleted": {"profile_documents": 3, "project_memberships": 1, "storage_objects": 2,
                    "device_credentials": 1, "messaging_documents": 0, "sign_in_account": true,
                    "future_count": 7},
        "retained": ["project_records"],
        "future_field": "ok"
    });
    let parsed: Receipt = serde_json::from_value(receipt.clone()).unwrap();
    assert_eq!(parsed.deleted.profile_documents, 3);
    receipt["deleted"]["sign_in_account"] = json!("yes");
    assert!(serde_json::from_value::<Receipt>(receipt).is_err());
}
