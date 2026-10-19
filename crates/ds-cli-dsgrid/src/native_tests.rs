use super::*;
#[test]
fn structured_owner_refusals_keep_code_class_detail_and_remedy() {
    let detail = serde_json::json!({"refusal":{"code":"missing_basis","structure_id":"S1"}});
    let failure = map_failure(NetworkFailure {
        code: "network_engine_refused",
        message: "owner message".into(),
        owner_error: None,
        owner_code: Some("operation_failed".into()),
        owner_detail: Some(Box::new(detail.clone())),
        owner_remedy: Some("author the exact missing basis".into()),
        owner_invalid_input: Some(false),
    });
    assert_eq!(failure.code(), "operation_failed");
    assert_eq!(failure.class(), ds_cli_contract::ExitClass::Failed);
    assert_eq!(failure.detail_value(), Some(&detail));
    assert_eq!(
        failure.remedy_text(),
        Some("author the exact missing basis")
    );
}
