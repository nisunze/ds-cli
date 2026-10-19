use super::*;

#[test]
fn supplied_request_cannot_redirect_the_protected_pending_link() {
    assert!(exact_request_id("owned-request", "owned-request").is_ok());
    let failure = exact_request_id("another-request", "owned-request").unwrap_err();
    assert_eq!(failure.code(), "device_request_mismatch");
}

#[test]
fn begin_projection_has_only_public_handoff_fields() {
    let value = begin_public_json(DeviceBeginPublic {
        request_id: "request-1".to_owned(),
        user_code: "BLUE-OTTER".to_owned(),
        verification_uri: "https://example.test/device".to_owned(),
        expires_at: "2026-08-30T12:00:00Z".to_owned(),
        poll_interval_seconds: 5,
        device_fingerprint: format!("sha256:{}", "a".repeat(64)),
        scopes: vec!["ds.api".to_owned()],
        binding: DeviceBinding {
            lane: "stable".to_owned(),
            audience: "b".repeat(64),
            profile_digest: format!("sha256:{}", "c".repeat(64)),
            catalog_digest: format!("sha256:{}", "d".repeat(64)),
        },
    });
    let transcript = serde_json::to_string(&value).unwrap();
    for secret in [
        "device_code",
        "code_verifier",
        "private_key",
        "access_token",
        "signature",
        "nonce",
    ] {
        assert!(!transcript.contains(secret), "CLI output exposed {secret}");
    }
}

#[test]
fn a_device_call_the_kernel_retried_says_how_often_and_stays_transient() {
    let failure = device_call_failure(
        DeviceCallFailure::<Failure>::Transient {
            transport: None,
            http_status: Some(503),
            retries: 3,
        },
        std::convert::identity,
    );
    assert_eq!(failure.code(), "device_auth_transient");
    assert!(failure.class().retryable());
    assert!(failure.message().ends_with("(retried 3 times)"));
    assert_eq!(
        failure.detail_value(),
        Some(&json!({ "retries": 3, "http_status": 503 }))
    );
    // A definitive answer keeps its own code; the host's refusal is its own.
    let refused = device_call_failure(
        DeviceCallFailure::<Failure>::Device(DeviceError::Response),
        std::convert::identity,
    );
    assert_eq!(refused.code(), "device_auth_response_invalid");
    let host = device_call_failure(
        DeviceCallFailure::Host(Failure::unavailable(RNG_UNAVAILABLE.code, "no rng")),
        std::convert::identity,
    );
    assert_eq!(host.code(), RNG_UNAVAILABLE.code);
}

#[test]
fn missing_begin_route_is_reported_as_deployment_unavailable() {
    let failure = device_response_failure(DeviceError::Response, 404);
    assert_eq!(failure.code(), "device_auth_endpoint_unavailable");
    assert_eq!(failure.detail_value(), Some(&json!({ "http_status": 404 })));
    assert_eq!(
        failure
            .detail_value()
            .and_then(Value::as_object)
            .map(|object| object.len()),
        Some(1)
    );
}

#[test]
fn malformed_begin_success_remains_response_invalid() {
    let failure = device_response_failure(DeviceError::Response, 201);
    assert_eq!(failure.code(), "device_auth_response_invalid");
    assert_eq!(failure.detail_value(), Some(&json!({ "http_status": 201 })));
}

#[test]
fn transient_begin_statuses_are_retryable_before_body_validation() {
    for status in [429, 500, 502, 503, 504] {
        let failure = device_response_failure(DeviceError::Response, status);
        assert_eq!(failure.code(), "device_auth_transient");
        assert_eq!(
            failure.detail_value(),
            Some(&json!({ "http_status": status }))
        );
    }
}

#[test]
fn permanent_authorization_statuses_remain_contract_refusals() {
    for status in [401, 403] {
        assert_eq!(
            device_response_failure(DeviceError::Response, status).code(),
            "device_auth_response_invalid"
        );
    }
}

/// A lane linked by device could read its projects but not the estate:
/// `ds install list --lane canary` refused `headless_signed_out` on a box
/// whose `ds auth status --lane canary` said signed in. Each projectless
/// governance door now reaches the gateway under the DEVICE credential —
/// the device bearer and the device id, never the password session.
#[test]
fn the_projectless_governance_doors_carry_the_device_credential() {
    use crate::test_support::{DEVICE_ACCESS_TOKEN, FixtureTransport, linked_device};
    use ds_client_core::{admin_bounds, global_tiles, grid_catalog, installs, sre};

    let transport = FixtureTransport::default();
    let mut device = linked_device(transport.clone(), unix_seconds());

    // Every door answers Unreachable from the script; what is asserted is
    // which credential arrived, not what came back.
    let _ = device.installs(&installs::Command::List {
        cursor: None,
        limit: 10,
    });
    let _ = device.sre(&sre::Command::Overview);
    let _ = device.admin_bounds(&admin_bounds::Command::Children {
        country: admin_bounds::Country::Rwanda,
        level: admin_bounds::Level::Province,
        parent_code: None,
    });
    let _ = device.grid_catalog(&grid_catalog::Command::ListLibraries);
    let _ = device.global_tiles(&global_tiles::Command::List {
        domain: global_tiles::Domain::NetworkTemplate,
    });
    // Project creation and property edits are the same kind of door:
    // server-side, decided by the principal's capabilities.
    let _ = device.project_properties(&ds_client_core::project_properties::Command::Update {
        project_id: "it_rwanda".into(),
        properties: ds_client_core::project_properties::Properties {
            country: Some("Rwanda".into()),
            ..Default::default()
        },
    });

    let expected: Vec<String> = [
        "installs",
        "sre_overview",
        "admin_bounds",
        "grid_catalog",
        "global_tiles",
        "project_properties",
    ]
    .iter()
    .map(|door| format!("{door} {DEVICE_ACCESS_TOKEN} device-1"))
    .collect();
    assert_eq!(transport.calls(), expected);
}
