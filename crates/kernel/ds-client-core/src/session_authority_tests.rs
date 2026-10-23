use super::*;
use crate::profile::{DeploymentLane, ProfileParts};
use crate::{CapturedGatewayRequest, SurveyEntryCreateCall, UploadBytesCall};
use serde_json::json;

const NOW: u64 = 1_900_000_000;

#[derive(Default)]
struct MockTransport {
    gateway_calls: usize,
    refresh_calls: usize,
    reply: Option<Result<TransportResponse, crate::TransportError>>,
}

fn gateway(
    transport: &mut MockTransport,
    request: CapturedGatewayRequest<'_>,
) -> Result<TransportResponse, crate::TransportError> {
    transport.gateway_calls += 1;
    assert_eq!(request.bearer_token(), "fixture-memory-bearer");
    assert_eq!(request.canonical_email(), "user@example.com");
    assert_eq!(request.path(), "/api/v1/project-forms");
    assert_eq!(
        request.body(),
        br#"{"action":"activate","project_id":"project-a"}"#
    );
    transport.reply.take().expect("one scripted request")
}

impl Transport for MockTransport {
    crate::__ds_gateway_calls!(native, gateway);

    fn sign_in(&mut self, _: SignInCall<'_>) -> Result<TransportResponse, crate::TransportError> {
        panic!("this public read never signs in")
    }
    fn refresh(&mut self, _: RefreshCall<'_>) -> Result<TransportResponse, crate::TransportError> {
        self.refresh_calls += 1;
        Err(crate::TransportError::Unreachable)
    }
    fn survey_entry_create(
        &mut self,
        _: SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, crate::TransportError> {
        panic!("this public read never creates an entry")
    }
    fn upload_bytes(
        &mut self,
        _: UploadBytesCall<'_>,
    ) -> Result<TransportResponse, crate::TransportError> {
        panic!("this public read never uploads bytes")
    }
}

#[derive(Default)]
struct Store {
    acquisitions: usize,
}
impl RefreshTokenStore for Store {
    fn acquire(&mut self, _: &StoreKey) -> Result<(), StoreError> {
        self.acquisitions += 1;
        Err(StoreError::Unavailable)
    }
    fn load(&mut self, _: &StoreKey) -> Result<Option<Vec<u8>>, StoreError> {
        panic!("no acquired lease")
    }
    fn compare_and_swap(
        &mut self,
        _: &StoreKey,
        _: Option<&[u8]>,
        _: Option<&[u8]>,
    ) -> Result<(), StoreError> {
        panic!("no acquired lease")
    }
    fn release(&mut self, _: &StoreKey) -> Result<(), StoreError> {
        panic!("no acquired lease")
    }
}

fn client(status: u16) -> Client<MockTransport, Store> {
    let profile = ClientProfile::from_parts(ProfileParts {
        lane: DeploymentLane::Canary,
        source_revision: "0123456789abcdef0123456789abcdef01234567",
        descriptor_sha256: &"a".repeat(64),
        firebase_project_id: "firebase-project",
        firebase_api_key: "public-firebase-key",
        gateway_api_key: "public-gateway-key",
        gateway_origin: "https://eds-gateway-canary-3c0q477h.ue.gateway.dev",
    })
    .unwrap();
    let response = json!({
        "status":"success", "project_id":"project-a", "resolved_at":NOW,
        "user":{"email":"user@example.com","roles":[],"is_owner":false,"is_admin":false},
        "forms":[],"field_types":[],"condition_operators":[],"type_aliases":{},
    });
    let mut client = Client::new(
        profile,
        MockTransport {
            reply: Some(Ok(TransportResponse::new(status, response.to_string()))),
            ..Default::default()
        },
        Store::default(),
    );
    client.session = Some(Session {
        stored: StoredRefresh {
            schema: STORED_SCHEMA.into(),
            credential_audience: client.profile.binding().into(),
            uid: "uid-a".into(),
            email: "user@example.com".into(),
            refresh_token: "fixture-protected-refresh".into(),
            credential_instance_sha256: "b".repeat(64),
        },
        stored_bytes: SecretBuffer::new(Vec::new()),
        id_token: "fixture-memory-bearer".into(),
        expires_at: NOW + 7200,
    });
    client
}

#[test]
fn fresh_public_read_reuses_captured_authority_once_without_touching_the_store() {
    let mut client = client(200);
    assert_eq!(
        client.project_forms("project-a", NOW).unwrap().ds_project(),
        "project-a"
    );
    assert_eq!(client.transport.gateway_calls, 1);
    assert_eq!(client.transport.refresh_calls, 0);
    assert_eq!(client.store.acquisitions, 0);
}

#[test]
fn invalid_input_is_refused_before_freshness_even_for_an_expired_session() {
    let mut client = client(200);
    client.session.as_mut().unwrap().expires_at = NOW;
    assert_eq!(
        client.project_forms("", NOW).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(client.transport.gateway_calls, 0);
    assert_eq!(client.transport.refresh_calls, 0);
    assert_eq!(client.store.acquisitions, 0);
}

#[test]
fn failed_existing_refresh_gate_never_dispatches_a_gateway_read() {
    let mut client = client(200);
    client.session.as_mut().unwrap().expires_at = NOW;
    assert_eq!(
        client.project_forms("project-a", NOW).unwrap_err().kind(),
        ErrorKind::DurableState
    );
    assert_eq!(client.store.acquisitions, 1);
    assert_eq!(client.transport.gateway_calls, 0);
    assert_eq!(client.transport.refresh_calls, 0);
}

#[test]
fn domain_auth_refusal_keeps_its_existing_error_and_is_not_retried() {
    let mut client = client(403);
    assert_eq!(
        client.project_forms("project-a", NOW).unwrap_err().kind(),
        ErrorKind::AuthenticationRejected
    );
    assert_eq!(client.transport.gateway_calls, 1);
    assert_eq!(client.transport.refresh_calls, 0);
    assert_eq!(client.store.acquisitions, 0);
}
