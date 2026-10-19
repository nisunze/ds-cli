use super::*;

#[test]
fn compute_artifact_refusals_keep_status_for_a_durable_adapter() {
    let invalid = sync_gateway_refusal(
        400,
        br#"{"error":{"code":"VALIDATION_FAILED","message":"invalid declaration"}}"#,
    );
    assert_eq!(invalid.kind(), ErrorKind::InvalidInput);
    assert_eq!(
        invalid.service_refusal().map(|refusal| refusal.status()),
        Some(400)
    );

    let conflict = sync_gateway_refusal(
        409,
        br#"{"error":{"code":"CONFLICT","message":"base moved"}}"#,
    );
    assert_eq!(conflict.kind(), ErrorKind::InvalidInput);
    assert_eq!(
        conflict.service_refusal().map(|refusal| refusal.status()),
        Some(409)
    );

    let denied = sync_gateway_refusal(403, br#"{"error":{"code":"DENIED"}}"#);
    assert_eq!(denied.kind(), ErrorKind::AuthenticationRejected);
    assert_eq!(
        denied.service_refusal().map(|refusal| refusal.status()),
        Some(403)
    );
}
