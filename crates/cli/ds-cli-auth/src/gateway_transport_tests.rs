use super::*;
use ds_client_core::{
    AuthorizedTransport, DeviceAccessSession, DeviceBinding, DeviceCredential, DevicePrivateKey,
    ErrorKind, SurveyEntryCreateCall, UploadBytesCall,
};
use serde_json::json;

struct MockNative {
    calls: usize,
    response: Option<Result<TransportResponse, TransportError>>,
}

fn mock_send(
    transport: &mut MockNative,
    request: CapturedGatewayRequest<'_>,
) -> Result<TransportResponse, TransportError> {
    transport.calls += 1;
    assert_eq!(request.method(), "POST");
    assert_eq!(request.path(), "/api/v1/project-forms");
    assert_eq!(
        request.body(),
        br#"{"action":"activate","project_id":"project-a"}"#
    );
    assert_eq!(
        (request.timeout_seconds(), request.response_limit()),
        (120, 32 * 1024 * 1024)
    );
    let bearer = format!("Bearer {}", request.bearer_token());
    assert_eq!(
        headers(&request, &bearer, "request-1", "action-1"),
        [
            ("Accept", "application/json"),
            ("Content-Type", "application/json"),
            ("X-App-Id", "ds-native-client"),
            ("X-Request-Id", "request-1"),
            ("X-DS-Action-Id", "action-1"),
            ("X-User-Email", "user@example.com"),
            ("x-api-key", "public-gateway-key"),
            ("Authorization", bearer.as_str()),
            ("X-Forwarded-Authorization", bearer.as_str()),
        ]
    );
    transport.response.take().expect("one expected request")
}

impl AuthorizedTransport for MockNative {
    ds_client_core::__ds_gateway_calls!(native, mock_send);

    fn survey_entry_create(
        &mut self,
        _: SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        panic!("direct Firestore create is outside ordinary gateway capture")
    }
    fn upload_bytes(
        &mut self,
        _: UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        panic!("storage upload is outside ordinary gateway capture")
    }
}

fn credential(profile: &ds_client_core::ClientProfile) -> DeviceCredential {
    let binding = DeviceBinding::for_profile(profile, &"b".repeat(64)).unwrap();
    DeviceCredential::new(
        "device-1".into(),
        "Workstation".into(),
        "linux".into(),
        format!("sha256:{}", "c".repeat(64)),
        "uid-1".into(),
        "user@example.com".into(),
        &binding,
        "2036-01-01T00:00:00Z".into(),
        DevicePrivateKey::from_secret_bytes([7u8; 32]),
    )
    .unwrap()
}

#[test]
fn public_device_method_runs_one_captured_native_request_and_retains_domain_admission() {
    let profile = crate::test_support::profile();
    let credential = credential(&profile);
    let now = 1_900_000_000;
    let access = DeviceAccessSession::new("fixture-memory-bearer".into(), now + 600, now).unwrap();
    let authorized = credential
        .authorize_api(&access, &profile, "uid-1", now)
        .unwrap();
    let response = json!({
        "status":"success", "project_id":"project-a", "resolved_at":now,
        "user":{"email":"user@example.com","roles":[],"is_owner":false,"is_admin":false},
        "forms":[],"field_types":[],"condition_operators":[],"type_aliases":{},
    });
    let mut transport = MockNative {
        calls: 0,
        response: Some(Ok(TransportResponse::new(200, response.to_string()))),
    };
    let forms = authorized
        .project_forms(&mut transport, "project-a", now)
        .unwrap();
    assert_eq!(forms.ds_project(), "project-a");
    assert_eq!(transport.calls, 1);
}

#[test]
fn transport_refusal_is_returned_once_and_invalid_project_spends_no_transport() {
    let profile = crate::test_support::profile();
    let credential = credential(&profile);
    let now = 1_900_000_000;
    let access = DeviceAccessSession::new("fixture-memory-bearer".into(), now + 600, now).unwrap();
    let authorized = credential
        .authorize_api(&access, &profile, "uid-1", now)
        .unwrap();
    let mut transport = MockNative {
        calls: 0,
        response: Some(Err(TransportError::TimedOut)),
    };
    assert_eq!(
        authorized
            .project_forms(&mut transport, "project-a", now)
            .unwrap_err()
            .kind(),
        ErrorKind::Transient
    );
    assert_eq!(transport.calls, 1);
    assert_eq!(
        authorized
            .project_forms(&mut transport, "", now)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(transport.calls, 1);
}
