use super::*;
use std::collections::VecDeque;
use std::time::Duration;

const IMMEDIATE: WeakNetwork = WeakNetwork::new(&[Duration::ZERO; 3]);
const NOW: u64 = 1_900_000_000;

fn binding() -> DeviceBinding {
    DeviceBinding {
        lane: "stable".to_owned(),
        audience: "d".repeat(64),
        profile_digest: format!("sha256:{}", "a".repeat(64)),
        catalog_digest: format!("sha256:{}", "b".repeat(64)),
    }
}

/// Answers every refresh and status from a script, in order; an `Err` is
/// a dropped connection. Begin and complete are never retried, so they
/// are never reached.
#[derive(Default)]
struct Scripted {
    answers: VecDeque<Result<u16, TransportError>>,
    body: Vec<u8>,
    sent: Vec<serde_json::Value>,
}

impl Scripted {
    fn new(answers: impl IntoIterator<Item = Result<u16, TransportError>>, body: Value) -> Self {
        Self {
            answers: answers.into_iter().collect(),
            body: serde_json::to_vec(&body).unwrap(),
            sent: Vec::new(),
        }
    }
    fn answer(&mut self, body: SecretRequestBody) -> Result<TransportResponse, TransportError> {
        self.sent
            .push(serde_json::from_slice(body.as_bytes()).unwrap());
        let status = self.answers.pop_front().expect("unscripted device IO")?;
        Ok(TransportResponse::new(status, self.body.clone()))
    }
}

impl DeviceTransport for Scripted {
    fn begin(&mut self, _: SecretRequestBody) -> Result<TransportResponse, TransportError> {
        unreachable!("begin creates a request and is never retried")
    }
    fn status(&mut self, body: SecretRequestBody) -> Result<TransportResponse, TransportError> {
        self.answer(body)
    }
    fn complete(&mut self, _: SecretRequestBody) -> Result<TransportResponse, TransportError> {
        unreachable!("complete consumes a request and is never retried")
    }
    fn refresh(&mut self, body: SecretRequestBody) -> Result<TransportResponse, TransportError> {
        self.answer(body)
    }
    fn protected(
        &mut self,
        _: DeviceProtectedCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        unreachable!("protected calls are not part of sign-in")
    }
}

use serde_json::Value;

fn credential() -> DeviceCredential {
    DeviceCredential::new(
        "device-1".to_owned(),
        "Workstation".to_owned(),
        "linux".to_owned(),
        format!("sha256:{}", "c".repeat(64)),
        "uid-1".to_owned(),
        "user@example.com".to_owned(),
        &binding(),
        "2031-01-01T00:00:00Z".to_owned(),
        DevicePrivateKey::from_secret_bytes([5; 32]),
    )
    .unwrap()
}

fn refreshed() -> Value {
    serde_json::json!({"data": {
        "access_token": "ds-access-jwt",
        "token_type": "Bearer",
        "expires_in": 300,
        "expires_at": "2030-03-17T17:51:40Z",
        "credential_expires_at": "2031-02-01T00:00:00Z"
    }})
}

/// A fresh `(timestamp, nonce)` per attempt, as a host's clock and RNG.
fn proofs() -> impl FnMut() -> Result<(u64, String), &'static str> {
    let mut attempt = 0_u8;
    move || {
        attempt += 1;
        Ok((
            NOW + u64::from(attempt),
            URL_SAFE_NO_PAD.encode([attempt; 16]),
        ))
    }
}

#[test]
fn a_refresh_survives_one_dropout_and_signs_the_retry_afresh() {
    let mut transport = Scripted::new([Err(TransportError::Unreachable), Ok(200)], refreshed());
    let (session, credential_expires_at) = credential()
        .refresh_session(&mut transport, &binding(), &IMMEDIATE, proofs())
        .unwrap();
    assert_eq!(session.expires_at(), NOW + 2 + 300);
    assert_eq!(credential_expires_at, "2031-02-01T00:00:00Z");
    // ds-brain spends a nonce the moment a proof arrives: the retry
    // carries its own nonce, timestamp and signature, never the first's.
    let [first, second] = transport.sent.as_slice() else {
        panic!("two attempts, {} sent", transport.sent.len());
    };
    for field in ["nonce", "timestamp", "signature"] {
        assert_ne!(first[field], second[field], "{field}");
    }
    assert_eq!(first["device_id"], second["device_id"]);
}

#[test]
fn a_refresh_that_blinks_through_every_retry_is_transient_and_counts_them() {
    let mut transport = Scripted::new(
        [
            Err(TransportError::TimedOut),
            Ok(503),
            Err(TransportError::Unreachable),
            Ok(503),
        ],
        serde_json::json!({"error": {"code": "UNAVAILABLE"}}),
    );
    let failure = credential()
        .refresh_session(&mut transport, &binding(), &IMMEDIATE, proofs())
        .err()
        .expect("no session");
    assert_eq!(
        failure,
        DeviceCallFailure::Transient {
            transport: None,
            http_status: Some(503),
            retries: 3,
        }
    );
    assert_eq!(transport.sent.len(), 4);

    let mut transport = Scripted::new([Err(TransportError::TimedOut); 4], Value::Null);
    assert_eq!(
        credential()
            .refresh_session(&mut transport, &binding(), &IMMEDIATE, proofs())
            .err()
            .expect("no session"),
        DeviceCallFailure::Transient {
            transport: Some(TransportError::TimedOut),
            http_status: None,
            retries: 3,
        }
    );
}

#[test]
fn a_definitive_refusal_is_answered_once_and_never_retried() {
    for status in [401, 403, 404, 409] {
        let mut transport = Scripted::new(
            [Ok(status), Ok(200)],
            serde_json::json!({"error": {"code": "DEVICE_CREDENTIAL_INVALID"}}),
        );
        let failure = credential()
            .refresh_session(&mut transport, &binding(), &IMMEDIATE, proofs())
            .err()
            .expect("no session");
        assert_eq!(failure, DeviceCallFailure::Device(DeviceError::Response));
        assert_eq!(transport.sent.len(), 1, "{status}");
    }
}

#[test]
fn a_host_that_cannot_mint_a_proof_sends_nothing() {
    let mut transport = Scripted::new([Ok(200)], refreshed());
    let failure = credential()
        .refresh_session(&mut transport, &binding(), &IMMEDIATE, || {
            Err::<(u64, String), _>("rng unavailable")
        })
        .err()
        .expect("no session");
    assert_eq!(failure, DeviceCallFailure::Host("rng unavailable"));
    assert!(transport.sent.is_empty());
}

#[test]
fn a_status_poll_survives_one_dropout_with_the_same_request() {
    let begin = parse_device_begin(
        TransportResponse::new(
            201,
            serde_json::to_vec(&serde_json::json!({"data": {
                "request_id": "request-1",
                "device_code": "device-code-secret",
                "user_code": "ABCD-EFGH",
                "verification_uri": "https://ds.example/account",
                "expires_at": "2030-01-01T00:10:00Z",
                "poll_interval_seconds": 3,
                "device_fingerprint": format!("sha256:{}", "e".repeat(64)),
                "scopes": ["ds.api"],
                "binding": binding()
            }}))
            .unwrap(),
        ),
        &binding(),
    )
    .unwrap();
    let pending = DevicePendingAuthorization::new(
        begin,
        "v".repeat(43),
        URL_SAFE_NO_PAD.encode([6; 16]),
        DevicePrivateKey::from_secret_bytes([4; 32]),
    )
    .unwrap();
    let mut transport = Scripted::new(
        [Err(TransportError::TimedOut), Ok(200)],
        serde_json::json!({"data": {
            "status": "approved",
            "expires_at": "2030-01-01T00:10:00Z",
            "poll_interval_seconds": 3
        }}),
    );
    let status = pending.observe_status(&mut transport, &IMMEDIATE).unwrap();
    assert_eq!(status.status, DeviceAuthorizationStatus::Approved);
    assert_eq!(transport.sent.len(), 2);
    assert_eq!(transport.sent[0], transport.sent[1]);

    let mut transport = Scripted::new([Ok(503); 4], Value::Null);
    assert_eq!(
        pending
            .observe_status(&mut transport, &IMMEDIATE)
            .unwrap_err(),
        DeviceCallFailure::Transient {
            transport: None,
            http_status: Some(503),
            retries: 3,
        }
    );
    let mut transport = Scripted::new([Ok(404), Ok(200)], Value::Null);
    assert_eq!(
        pending
            .observe_status(&mut transport, &IMMEDIATE)
            .unwrap_err(),
        DeviceCallFailure::Device(DeviceError::Response)
    );
    assert_eq!(transport.sent.len(), 1);
}
