use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn answered(call: Installations, body: Value) -> Result<Value, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else {
        panic!("expected a fetch")
    };
    let observed = Observed::Response {
        status: 200,
        content_type: Some("application/json".into()),
        retry_after: None,
    };
    match resume(
        continuation,
        observed,
        body.to_string().as_bytes(),
        &Facts::default(),
    ) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

fn command(value: Value) -> Installations {
    Installations {
        command: serde_json::from_value(value).unwrap(),
    }
}

#[test]
fn a_command_is_sent_as_its_action_and_data() {
    let call = command(json!({"action": "list_installs", "cursor": "", "limit": 50}));
    let Step::Fetch { request, .. } = start(call.clone()) else {
        panic!("expected a fetch")
    };
    assert_eq!(request.path, "/api/v1/governance");
    assert_eq!(
        request.body,
        Some(json!({"action": "list_installs", "data": {"cursor": "", "limit": 50}}))
    );
    let page = answered(
        call,
        json!({"success": true, "data": {"installs": [], "next_cursor": "c2"}}),
    )
    .unwrap();
    assert_eq!(page["next_cursor"], "c2");
    let policy = command(
        json!({"action": "set_install_policy", "install_id": "i-1", "expected_revision": 3,
        "device_status": "blocked", "license_status": "active", "reason": "lost laptop"}),
    );
    let Step::Fetch { request, .. } = start(policy) else {
        panic!("expected a fetch")
    };
    assert_eq!(request.body.unwrap()["data"]["expected_revision"], 3);
}

#[test]
fn a_command_outside_its_schema_or_bounds_is_never_sent() {
    assert!(serde_json::from_value::<InstallCommand>(
        json!({"action": "delete_install", "install_id": "i"})
    )
    .is_err());
    assert!(serde_json::from_value::<InstallCommand>(
        json!({"action": "list_installs", "limit": 5, "extra": 1})
    )
    .is_err());
    for value in [
        json!({"action": "list_installs", "limit": 51}),
        json!({"action": "get_install", "install_id": "", "limit": 10}),
        json!({"action": "set_install_policy", "install_id": "i", "expected_revision": 1,
            "device_status": "gone", "license_status": "active", "reason": "r"}),
        json!({"action": "set_install_retirement", "install_id": "i", "expected_revision": 1, "retired": true, "reason": " "}),
    ] {
        assert!(matches!(start(command(value)), Step::Refused(r) if r.code == INPUT_INVALID));
    }
}

#[test]
fn an_answer_without_its_record_is_refused() {
    let call = command(json!({"action": "get_install", "install_id": "i-1", "limit": 50}));
    assert_eq!(
        answered(call.clone(), json!({"data": {"users": []}}))
            .unwrap_err()
            .code,
        INVALID
    );
    assert!(answered(
        call,
        json!({"data": {"install": {"install_id": "i-1"}, "users": [], "events": []}})
    )
    .is_ok());
}
