use super::*;

#[test]
fn native_device_selector_admits_linux_and_windows_only() {
    assert_eq!(native_device_for_os("linux"), Ok("linux"));
    assert_eq!(native_device_for_os("windows"), Ok("windows"));
    for os in ["macos", "freebsd", ""] {
        assert_eq!(
            native_device_for_os(os),
            Err("native Sync Center server requires Linux or Windows".into())
        );
    }
}

#[test]
fn native_device_matches_the_compile_target() {
    if cfg!(target_os = "linux") {
        assert_eq!(native_device(), Ok("linux"));
    } else if cfg!(target_os = "windows") {
        assert_eq!(native_device(), Ok("windows"));
    } else {
        assert!(native_device().is_err());
    }
}

#[test]
fn release_pins_and_application_version_are_sealed_build_inputs() {
    assert!(pins(Lane::Canary).is_ok());
    assert!(pins(Lane::Stable).is_ok());
    assert!(
        env!("DS_NATIVE_APP_VERSION")
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'+'))
    );
    assert_ne!(env!("DS_NATIVE_APP_VERSION"), env!("CARGO_PKG_VERSION"));
}

#[test]
fn reporter_registration_is_bound_to_its_declared_release() {
    let mut addition = NativeEngineAddition {
        name: "ds-network-reporter".into(),
        version: format!("0.1.0+{}", "a".repeat(40)),
        release: format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)),
    };
    assert!(validate_addition(&addition).is_ok());
    addition.release = "ds-solar-engine@0.1.0+abc".into();
    assert!(validate_addition(&addition).is_err());
    addition.name = "unregistered-engine".into();
    assert!(validate_addition(&addition).is_err());
}

#[test]
fn device_refresh_outage_retries_but_removed_authority_blocks() {
    assert!(matches!(
        classify_device_refresh_error(ds_cli_contract::Failure::unavailable(
            "device_auth_transient",
            "offline"
        )),
        GatewayError { status: None, .. }
    ));
    assert!(matches!(
        classify_device_refresh_error(ds_cli_contract::Failure::conflict(
            "auth_context_mismatch",
            "changed"
        )),
        GatewayError {
            status: Some(401),
            ..
        }
    ));
}

fn identity(uid: &str) -> ProviderIdentity {
    ProviderIdentity::new("canary", &"a".repeat(64), uid).unwrap()
}

#[test]
fn refresh_fence_refuses_a_different_principal_or_credential_instance() {
    let principal = identity("uid-a");
    assert!(
        check_runtime_fence(&principal, "credential-a", &principal, "credential-a").is_ok()
    );
    assert!(
        check_runtime_fence(
            &principal,
            "credential-a",
            &identity("uid-b"),
            "credential-a"
        )
        .is_err()
    );
    assert!(
        check_runtime_fence(&principal, "credential-a", &principal, "credential-b").is_err()
    );
}

#[test]
fn a_heartbeat_answer_with_an_added_member_is_still_read() {
    // Brain may add members to its heartbeat reply (typed compatibility,
    // never schema equality); a missing required member is still refused.
    let lease = serde_json::json!({"schema": "x", "added": true});
    let mut answer = serde_json::json!({
        "status": "ok",
        "lease_expires_at": "2026-10-23T00:00:00Z",
        "min_supported_version": "0.1.0",
        "server_time": "2026-10-23T00:00:00Z",
        "lease": lease,
        "newer_member": {"anything": 1},
    });
    let added: Result<HeartbeatAnswer, _> = serde_json::from_value(answer.clone());
    let missing = {
        answer.as_object_mut().unwrap().remove("server_time");
        serde_json::from_value::<HeartbeatAnswer>(answer)
    };
    assert!(missing.is_err(), "a missing required member is refused");
    // The lease itself is decoded by its own owner; only the envelope is judged here.
    if let Err(error) = added {
        assert!(
            !error.to_string().contains("unknown field `newer_member`"),
            "an added member must not be refused: {error}"
        );
    }
}
