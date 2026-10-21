//! A Project Work 409 that says the plan moved is a retryable conflict.
//!
//! On 2026-10-21 `ds pm task delete` answered ds-brain's 409
//! `pm_graph_changed_during_read` ("Retry the request") as `auth_input_invalid`,
//! class `invalid_input`, `retryable: false`: the shared kind mapping renders
//! every 409 of the correspondence route as invalid input.

use super::*;
use ds_cli_contract::outcome::ExitClass;

/// The failure the auth crate hands this domain for a 409 it relays.
fn relayed(status: u64, service_code: &str, sentence: &str) -> Failure {
    Failure::invalid(
        "auth_input_invalid",
        format!("correspondence refused a request against state that moved or already exists (HTTP {status}): {sentence}"),
    )
    .detail(json!({
        "http_status": status,
        "service_code": service_code,
        "service_message": sentence,
    }))
}

#[test]
fn a_graph_read_race_is_a_retryable_conflict_with_the_same_command_id() {
    let failure = classify(relayed(
        409,
        "pm_graph_changed_during_read",
        "The project schedule changed while it was loading. Retry the request.",
    ));
    assert_eq!(failure.code(), CONFLICT.code);
    assert_eq!(failure.class(), ExitClass::Conflict);
    assert!(failure.class().retryable());
    assert_eq!(
        failure.message(),
        "The project schedule changed while it was loading. Retry the request."
    );
    assert!(
        failure
            .remedy_text()
            .is_some_and(|remedy| remedy.contains("same --command-id"))
    );
    let detail = failure.detail_value().expect("the service's words cross");
    assert_eq!(detail["http_status"], 409);
    assert_eq!(detail["service_code"], "pm_graph_changed_during_read");
}

#[test]
fn a_moved_head_is_the_documented_revision_conflict() {
    let failure = classify(relayed(409, "pm_revision_conflict", ""));
    assert_eq!(failure.code(), CONFLICT.code);
    assert_eq!(failure.class(), ExitClass::Conflict);
    assert_eq!(failure.message(), CONFLICT.when);
    assert_eq!(failure.remedy_text(), Some(CONFLICT.remedy));
}

#[test]
fn other_refusals_keep_their_mapping() {
    // A 409 that is not the plan moving, and the same token at another
    // status, are not reclassified.
    let exists = classify(relayed(409, "party_exists", "The party exists."));
    assert_ne!(exists.class(), ExitClass::Conflict);
    let not_a_conflict = classify(relayed(400, "pm_graph_changed_during_read", "x"));
    assert_eq!(not_a_conflict.class(), ExitClass::InvalidInput);
    let unrelated = classify(Failure::invalid("invalid_date", "bad date"));
    assert_eq!(unrelated.code(), "invalid_date");
}
