//! A Sync Center gateway failure keeps the cause the native transport
//! classified: the closed token as its code and the cause's sentence as its
//! detail, never only "temporarily unreachable". Still no status, so the loop
//! reads it as the moment and holds the row.
use super::*;
use ds_client_core::TransportError;

#[test]
fn a_call_past_its_deadline_names_deadline_exceeded() {
    crate::network_cause::request_started();
    let _ = crate::network_cause::classify(ureq::Error::Timeout(ureq::Timeout::Global));
    let error = gateway_error(ClientError::from(TransportError::TimedOut));
    assert_eq!(error.status, None, "a transport cause is never a verdict");
    assert_eq!(error.code.as_deref(), Some("deadline_exceeded"));
    assert_eq!(
        error.detail,
        "the DS service took the request and did not answer within the call's deadline"
    );
}

#[test]
fn a_gateway_answering_503_names_service_status_and_its_status() {
    crate::network_cause::request_started();
    crate::network_cause::answered(503, None);
    let error = gateway_error(ClientError::from(TransportError::Unreachable));
    assert_eq!(error.status, None);
    assert_eq!(error.code.as_deref(), Some("service_status"));
    assert!(error.detail.contains("HTTP 503"), "{}", error.detail);
}

#[test]
fn a_refused_connection_says_no_service_was_reached() {
    crate::network_cause::request_started();
    let _ = crate::network_cause::classify(ureq::Error::ConnectionFailed);
    let error = gateway_error(ClientError::from(TransportError::Unreachable));
    assert_eq!(error.code.as_deref(), Some("connection_refused"));
    assert!(
        error.detail.starts_with("no DS service was reached: "),
        "{}",
        error.detail
    );
}

#[test]
fn without_a_recorded_cause_the_failure_keeps_its_old_shape() {
    crate::network_cause::request_started();
    let error = gateway_error(ClientError::from(TransportError::Unreachable));
    assert_eq!(error.status, None);
    assert_eq!(error.code, None, "no code is inferred from prose");
    assert_eq!(
        error.detail,
        "the authentication or project service is temporarily unreachable"
    );
}

#[test]
fn a_device_refresh_outage_carries_its_explained_cause_as_the_code() {
    let explained = ds_cli_contract::Failure::unavailable(
        "device_auth_transient",
        "no DS service was reached: the connection to the DS gateway did not open within the connect deadline",
    )
    .detail(json!({"transport": {"cause": "connect_timed_out", "reach": "not_reached"}}));
    let error = classify_device_refresh_error(explained);
    assert_eq!(error.status, None);
    assert_eq!(error.code.as_deref(), Some("connect_timed_out"));
    assert!(error.detail.contains("connect deadline"));
    // Unexplained, the refusal's own code still names it.
    let bare = classify_device_refresh_error(ds_cli_contract::Failure::unavailable(
        "device_auth_transient",
        "offline",
    ));
    assert_eq!(bare.code.as_deref(), Some("device_auth_transient"));
}
