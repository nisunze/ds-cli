use super::*;
use ed25519_dalek::{Signature, Verifier};

fn binding() -> DeviceBinding {
    DeviceBinding {
        lane: "stable".to_owned(),
        audience: "d".repeat(64),
        profile_digest: format!("sha256:{}", "a".repeat(64)),
        catalog_digest: format!("sha256:{}", "b".repeat(64)),
    }
}

#[test]
fn complete_and_refresh_sign_only_the_frozen_transcripts() {
    let key = DevicePrivateKey::from_secret_bytes([7; 32]);
    let nonce = URL_SAFE_NO_PAD.encode([3; 16]);
    let verifier = "A".repeat(43);
    let complete = key
        .complete_signature("request-1", "device-secret", &verifier, &nonce)
        .unwrap();
    let complete_bytes = URL_SAFE_NO_PAD.decode(complete).unwrap();
    let complete_signature = Signature::from_slice(&complete_bytes).unwrap();
    let transcript = format!(
        "DS-DEVICE-COMPLETE-V1\nrequest-1\n{}\n{}\n{}",
        sha256_hex(b"device-secret"),
        sha256_hex(verifier.as_bytes()),
        nonce,
    );
    key.0
        .verifying_key()
        .verify(transcript.as_bytes(), &complete_signature)
        .unwrap();

    let refresh = key
        .refresh_signature("device-1", 1_900_000_000, &nonce, &binding())
        .unwrap();
    let refresh_signature =
        Signature::from_slice(&URL_SAFE_NO_PAD.decode(refresh).unwrap()).unwrap();
    let refresh_transcript = format!(
        "DS-DEVICE-REFRESH-V1\ndevice-1\n1900000000\n{}\nstable\n{}\nsha256:{}\nsha256:{}",
        nonce,
        "d".repeat(64),
        "a".repeat(64),
        "b".repeat(64),
    );
    key.0
        .verifying_key()
        .verify(refresh_transcript.as_bytes(), &refresh_signature)
        .unwrap();
}

#[test]
fn begin_is_exact_pkce_and_contains_no_signature_or_secret_key() {
    let key = DevicePrivateKey::from_secret_bytes([9; 32]);
    let nonce = URL_SAFE_NO_PAD.encode([4; 16]);
    let verifier = "z".repeat(43);
    let request = DeviceBeginRequest::new(
        "Workstation",
        "linux",
        &key.public_key_base64url(),
        &nonce,
        &verifier,
        &binding(),
    )
    .unwrap();
    let body = secret_json(&request).unwrap();
    let json: serde_json::Value = serde_json::from_slice(body.as_bytes()).unwrap();
    assert_eq!(json["schema"], DEVICE_AUTH_SCHEMA);
    assert_eq!(json["public_key"]["alg"], "Ed25519");
    assert_eq!(json["public_key"]["encoding"], "base64url");
    assert_eq!(json["pkce"]["method"], "S256");
    assert_eq!(
        json["pkce"]["challenge"],
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
    );
    assert!(json.get("signature").is_none());
    assert!(!String::from_utf8_lossy(body.as_bytes()).contains(&URL_SAFE_NO_PAD.encode([9; 32])));
}

#[test]
fn memory_access_token_is_bounded_to_five_minutes_and_not_debuggable() {
    assert!(DeviceAccessSession::new("token".to_owned(), 1_601, 1_000).is_err());
    let session = DeviceAccessSession::new("token".to_owned(), 1_600, 1_000).unwrap();
    let call = session.protected_call(DeviceProtectedOperation::Read("device-1"));
    assert_eq!(call.method(), "GET");
    assert_eq!(call.path().unwrap(), "/api/v1/auth/devices/device-1");
    assert_eq!(call.bearer_token(), "token");
}

#[test]
fn pending_bundle_exposes_only_the_public_begin_projection() {
    let nonce = URL_SAFE_NO_PAD.encode([6; 16]);
    let verifier = "v".repeat(43);
    let fingerprint = format!("sha256:{}", "e".repeat(64));
    let response = TransportResponse::new(
        201,
        serde_json::to_vec(&serde_json::json!({
            "success": true,
            "data": {
                "request_id": "request-1",
                "device_code": "device-code-secret",
                "user_code": "ABCD-EFGH",
                "verification_uri": "https://ds.example/account",
                "expires_at": "2030-01-01T00:10:00Z",
                "poll_interval_seconds": 3,
                "device_fingerprint": fingerprint,
                "scopes": ["ds.api"],
                "binding": binding()
            }
        }))
        .unwrap(),
    );
    let begin = parse_device_begin(response, &binding()).unwrap();
    let pending = DevicePendingAuthorization::new(
        begin,
        verifier.clone(),
        nonce,
        DevicePrivateKey::from_secret_bytes([4; 32]),
    )
    .unwrap();
    let public = serde_json::to_string(&pending.public()).unwrap();
    assert!(!public.contains("device-code-secret"));
    assert!(!public.contains(&verifier));
    assert!(!public.contains(&URL_SAFE_NO_PAD.encode([4; 32])));
    let protected = pending.encode_protected().unwrap();
    assert!(String::from_utf8_lossy(protected.as_bytes()).contains("device-code-secret"));

    let mut pending = pending;
    pending.result.expires_at = "1970-01-01T01:00:10.5+01:00".to_owned();
    assert_eq!(pending.is_expired(10), Ok(false));
    assert_eq!(pending.is_expired(11), Ok(true));
    pending.result.expires_at = "1970-01-01T00:00:10Z".to_owned();
    assert_eq!(pending.is_expired(9), Ok(false));
    assert_eq!(pending.is_expired(10), Ok(true));
    pending.result.expires_at = "not a server deadline".to_owned();
    assert_eq!(pending.is_expired(u64::MAX), Err(DeviceError::Response));

    let wrong_status = TransportResponse::new(200, br#"{"data":{}}"#);
    assert!(parse_device_begin(wrong_status, &binding()).is_err());
}

#[test]
fn server_digests_require_the_exact_wire_prefix() {
    let mut response_binding = binding();
    response_binding.profile_digest = "a".repeat(64);
    let response = TransportResponse::new(
        201,
        serde_json::to_vec(&serde_json::json!({
            "success": true,
            "data": {
                "request_id": "request-1",
                "device_code": "device-code-secret",
                "user_code": "ABCD-EFGH",
                "verification_uri": "https://ds.example/account",
                "expires_at": "2030-01-01T00:10:00Z",
                "poll_interval_seconds": 3,
                "device_fingerprint": "e".repeat(64),
                "scopes": ["ds.api"],
                "binding": binding()
            }
        }))
        .unwrap(),
    );
    assert!(parse_device_begin(response, &binding()).is_err());
    assert!(validate_binding(&response_binding).is_err());
}
