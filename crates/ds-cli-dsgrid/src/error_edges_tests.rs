//! One class and code for a corrupt package, stale pins that are not blindly
//! retryable, and a revision pin that must be an authored token.

use ds_cli_contract::ExitClass;
use ds_network_client::NetworkFailure;
use serde_json::json;

use crate::native::{map_failure, operation_failure};
use crate::package::revision_pin;

fn owner(code: &str, invalid: Option<bool>) -> NetworkFailure {
    NetworkFailure {
        code: "network_engine_refused",
        message: "owner message".into(),
        owner_error: None,
        owner_code: Some(code.into()),
        owner_detail: None,
        owner_remedy: Some("owner remedy".into()),
        owner_invalid_input: invalid,
    }
}

#[test]
fn package_refusals_are_one_invalid_input_whatever_the_owner_called_them() {
    for (code, flag) in [
        ("package_refused", None),
        ("package_refused", Some(false)),
        ("package_decode_failed", Some(false)),
        ("package_decode_failed", Some(true)),
    ] {
        let failure = map_failure(owner(code, flag));
        assert_eq!(failure.code(), "package_decode_failed", "{code}");
        assert_eq!(failure.class(), ExitClass::InvalidInput, "{code}");
        assert!(!failure.retryable(), "{code}");
        assert!(failure.next_commands().is_empty(), "no feedback hint: {code}");
    }
}

#[test]
fn an_operation_refusal_of_a_package_keeps_the_same_code_and_class() {
    let detail = json!({
        "schema": ds_network_contracts::grid_operation::GRID_OPERATION_REFUSAL_SCHEMA_V1,
        "class": "failed",
        "next": [],
        "detail": null,
    });
    let mut error = owner("package_refused", Some(false));
    error.owner_detail = Some(Box::new(detail));
    let failure = operation_failure(error);
    assert_eq!(failure.code(), "package_decode_failed");
    assert_eq!(failure.class(), ExitClass::InvalidInput);
}

#[test]
fn stale_pins_are_conflicts_that_are_not_blindly_retryable() {
    for code in [
        "revision_conflict",
        "command_replay_conflict",
        "engineering_input_root_mismatch",
        "revision_mismatch",
    ] {
        let failure = map_failure(owner(code, Some(false)));
        assert_eq!(failure.code(), code);
        assert_eq!(failure.class(), ExitClass::Conflict, "{code}");
        assert!(!failure.retryable(), "{code}: re-read, do not repeat");
    }
    // The class default is untouched for every other conflict.
    assert!(ExitClass::Conflict.retryable());
    assert!(ds_cli_contract::outcome::Failure::conflict("x", "y").retryable());
}

#[test]
fn a_revision_pin_is_an_authored_token_or_invalid_input() {
    let good = format!("rev:{}", "ab01".repeat(16));
    assert_eq!(revision_pin(None).unwrap(), None);
    assert_eq!(revision_pin(Some(&good)).unwrap().as_deref(), Some(good.as_str()));
    assert_eq!(
        revision_pin(Some(&format!("  {good}\n"))).unwrap().as_deref(),
        Some(good.as_str())
    );
    for bad in [
        "",
        "nonsense",
        "rev:",
        "rev:abc",
        &format!("rev:{}", "AB01".repeat(16)),
        &format!("rev:{}", "g".repeat(64)),
        &format!("{}", "ab01".repeat(16)),
        &format!("rev:{}0", "ab01".repeat(16)),
    ] {
        let failure = revision_pin(Some(bad)).expect_err(bad);
        assert_eq!(failure.code(), "revision_invalid", "{bad}");
        assert_eq!(failure.class(), ExitClass::InvalidInput, "{bad}");
    }
}
