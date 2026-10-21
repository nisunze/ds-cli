//! The Sync Center queue delivers independent records independently: a
//! result whose prerequisite was refused parks with a typed reason instead of
//! waiting forever, and uploads run smallest first.
use super::*;
use serde_json::{json, Value};

const SOLAR_RELEASE: &str = "ds-solar-engine@0.1.0";
const REPORT_RELEASE: &str = "ds-network-reporter@0.1.0+0123456789abcdef0123456789abcdef01234567";

fn request(local: Vec<Value>, remote: Vec<Value>, settled: Vec<Value>) -> Input {
    serde_json::from_value(json!({
        "schema": SCHEMA,
        "project": "p",
        "install_id": "install-1",
        "now_ms": 10_000,
        "online": true,
        "trigger": "local_change",
        "remote_read_at_ms": 9_000,
        "local": local,
        "remote": remote,
        "settled": settled,
    }))
    .expect("a valid plan input")
}

fn solar_report(city: &str) -> Value {
    json!({
        "engine": "solar", "operation": format!("report-{city}"), "variant": "apd-draft-fr",
        "sha256": "a".repeat(64), "size_bytes": 2_000_000, "produced_at_ms": 2_000,
        "input_base_fingerprint": "c".repeat(64), "engine_release": SOLAR_RELEASE,
        "resource": city,
    })
}

fn calculation_head(city: &str) -> Value {
    json!({
        "engine": "solar", "operation": format!("calculate-{city}"), "variant": "default",
        "revision": "head-2", "sha256": "b".repeat(64), "published_at_ms": 1_000,
    })
}

fn calculation(city: &str) -> Value {
    json!({"engine": "solar", "operation": format!("calculate-{city}"), "variant": "default"})
}

fn export(room: &str, size_bytes: u64) -> Value {
    json!({
        "engine": "network_reporter", "operation": format!("export-{room}"),
        "sha256": "d".repeat(64), "size_bytes": size_bytes, "produced_at_ms": 2_000,
        "engine_release": REPORT_RELEASE, "resource": room,
    })
}

#[test]
fn a_report_whose_calculation_was_refused_parks_with_a_typed_reason() {
    // aderm_loc7, 10-16: the calculation's upload was refused and its bytes
    // reclaimed; the report waited on it forever and counted as queued.
    let parked = plan(&request(
        vec![solar_report("aderm_bere")],
        vec![calculation_head("aderm_bere")],
        vec![calculation("aderm_bere")],
    ))
    .unwrap();
    assert_eq!(parked.summary.waiting_on_dependency, 0);
    assert_eq!(parked.summary.refused, 1);
    assert!(parked.actions.iter().any(|action| matches!(
        action,
        Action::Refused { identity: Some(identity), reason: DEPENDENCY_REFUSED }
            if identity.operation == "report-aderm_bere"
    )));
}

#[test]
fn a_report_whose_calculation_is_still_pending_keeps_waiting() {
    // Control: no settled verdict, so the report waits for its calculation.
    let waiting = plan(&request(
        vec![solar_report("aderm_bere")],
        vec![calculation_head("aderm_bere")],
        vec![],
    ))
    .unwrap();
    assert_eq!(waiting.summary.waiting_on_dependency, 1);
    assert_eq!(waiting.summary.refused, 0);

    // Another city's refusal does not park this report.
    let other = plan(&request(
        vec![solar_report("aderm_bere")],
        vec![calculation_head("aderm_bere")],
        vec![calculation("aderm_gagal")],
    ))
    .unwrap();
    assert_eq!(other.summary.waiting_on_dependency, 1);
    assert_eq!(other.summary.refused, 0);
}

#[test]
fn a_parked_report_does_not_hold_back_an_independent_upload() {
    let plan = plan(&request(
        vec![solar_report("aderm_bere"), export("tx_1", 10)],
        vec![calculation_head("aderm_bere")],
        vec![calculation("aderm_bere")],
    ))
    .unwrap();
    assert_eq!(plan.summary.uploads, 1);
    assert_eq!(plan.summary.refused, 1);
}

#[test]
fn uploads_run_smallest_first_and_other_actions_keep_their_place() {
    let plan = plan(&request(
        vec![
            export("a_large", 9_000_000),
            export("b_small", 10),
            export("c_medium", 5_000),
        ],
        vec![json!({
            "engine": "network_reporter", "operation": "export-z_remote", "variant": "default",
            "revision": "head-1", "sha256": "e".repeat(64), "published_at_ms": 1_000,
        })],
        vec![],
    ))
    .unwrap();
    let order: Vec<&str> = plan
        .actions
        .iter()
        .filter_map(|action| match action {
            Action::Upload { identity, .. } => Some(identity.operation.as_str()),
            Action::Download { identity, .. } => Some(identity.operation.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        order,
        ["export-b_small", "export-c_medium", "export-a_large", "export-z_remote"]
    );
}

#[test]
fn an_input_without_settled_verdicts_still_parses() {
    let input: Input = serde_json::from_value(json!({
        "schema": SCHEMA, "project": "p", "install_id": "i", "now_ms": 1,
        "online": true, "trigger": "manual",
    }))
    .unwrap();
    assert!(input.settled.is_empty());
}
