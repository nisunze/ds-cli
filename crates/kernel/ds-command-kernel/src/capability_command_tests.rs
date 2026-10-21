use super::*;
use serde_json::json;

fn descriptor() -> Value {
    json!({"id":"design.process.settings","contract":1,"availability":"available","requires":"server",
        "inputs":[{"name":"preset","kind":"value","required":true,"choices":["sketch","final"]},
                  {"name":"catalog","kind":"switch","required":false}]})
}

#[test]
fn omitted_required_input_is_refused_before_execution() {
    let refusal = admit("design.process.settings", &descriptor(), &json!({}), "").unwrap();
    assert_eq!(refusal.code, "inputs_invalid");
    assert!(refusal.message.contains("preset"));
    assert!(!refusal.remedy.is_empty());
}

#[test]
fn descriptor_drives_choices_switches_and_unknown_input_refusals() {
    let descriptor = descriptor();
    assert!(admit(
        "design.process.settings",
        &descriptor,
        &json!({"preset":"sketch","catalog":true}),
        ""
    )
    .is_none());
    for args in [
        json!({"preset":"old"}),
        json!({"preset":"sketch","catalog":"yes"}),
        json!({"preset":"sketch","invented":true}),
    ] {
        assert_eq!(
            admit("design.process.settings", &descriptor, &args, "")
                .unwrap()
                .code,
            "inputs_invalid"
        );
    }
}

#[test]
fn skewed_descriptor_refuses_with_a_remedy_and_does_not_poison_later_admission() {
    let mut wrong = descriptor();
    wrong["id"] = json!("design.other");
    assert_eq!(
        admit(
            "design.process.settings",
            &wrong,
            &json!({"preset":"sketch"}),
            ""
        )
        .unwrap()
        .code,
        "engine_incompatible"
    );
    assert!(admit(
        "design.process.settings",
        &descriptor(),
        &json!({"preset":"sketch"}),
        ""
    )
    .is_none());
}

#[test]
#[cfg(feature = "platform-desktop")]
fn unsupported_core_routes_are_not_admitted_by_a_valid_descriptor() {
    let mut wrong = descriptor();
    wrong["id"] = json!("survey.entries.create");
    assert_eq!(
        admit("survey.entries.create", &wrong, &json!({}), "")
            .unwrap()
            .code,
        "capability_not_on_core"
    );
}

#[test]
fn transport_failures_preserve_typed_owners_and_give_untyped_disconnects_a_remedy() {
    let absent = connection_failure(None, "network_engine_missing: Network is absent", None);
    assert_eq!(absent.code, "network_engine_missing");
    assert_eq!(absent.message, "Network is absent");
    assert!(!absent.remedy.is_empty());
    let disconnected = connection_failure(None, "IPC closed", None);
    assert_eq!(disconnected.code, "engine_connection_lost");
    assert!(disconnected.retryable);
    assert_eq!(
        connection_failure(
            Some("network_engine_incompatible"),
            "Skew",
            Some("Install compatible Network")
        )
        .remedy,
        "Install compatible Network"
    );
}
