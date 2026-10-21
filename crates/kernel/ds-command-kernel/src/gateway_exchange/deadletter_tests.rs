use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step, PROJECT_INVALID};
use serde_json::json;

fn answered(body: Value) -> Result<DeadLetterSummary, Refusal> {
    let Step::Fetch { continuation, .. } = start(Summary {
        project: "it_rwanda".into(),
    }) else {
        panic!("expected a fetch")
    };
    match resume(
        continuation,
        Observed::Response {
            status: 200,
            content_type: Some("application/json".into()),
            retry_after: None,
        },
        body.to_string().as_bytes(),
        &Facts::default(),
    ) {
        Step::Done(summary) => Ok(summary),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

#[test]
fn the_summary_read_names_its_project_and_long_deadline() {
    let Step::Fetch { request, .. } = start(Summary {
        project: "it_rwanda".into(),
    }) else {
        panic!("expected a fetch")
    };
    assert_eq!(request.path, "/api/v1/deadletter");
    assert_eq!(
        request.body,
        Some(json!({"action": "get_summary", "project_id": "it_rwanda"}))
    );
    assert_eq!(request.timeout_ms, 300_000);
    assert!(matches!(
        start(Summary {
            project: "".into()
        }),
        Step::Refused(refusal) if refusal.code == PROJECT_INVALID
    ));
}

#[test]
fn a_summary_is_admitted_with_its_reasons() {
    let summary = answered(json!({"success": true, "data": {
        "count": 3, "last_at": "2026-10-19T08:00:00Z", "last_reason": "schema_mismatch",
        "last_message": "field gps missing", "by_reason": {"schema_mismatch": 2, "stale_form": 1}}}))
    .unwrap();
    assert_eq!(summary.count, 3);
    assert_eq!(summary.last_reason.as_deref(), Some("schema_mismatch"));
    assert_eq!(summary.by_reason.get("stale_form"), Some(&1));
    assert_eq!(
        serde_json::to_value(&summary).unwrap(),
        json!({"count": 3, "last_at": "2026-10-19T08:00:00Z", "last_reason": "schema_mismatch",
            "last_message": "field gps missing", "by_reason": {"schema_mismatch": 2, "stale_form": 1}})
    );
}

#[test]
fn nothing_recorded_reads_as_a_zero_count() {
    assert_eq!(
        answered(json!({"success": true})).unwrap(),
        DeadLetterSummary::default()
    );
    assert_eq!(answered(json!({})).unwrap().count, 0);
}

#[test]
fn a_malformed_summary_is_refused() {
    assert_eq!(
        answered(json!({"data": {"count": "three"}}))
            .unwrap_err()
            .code,
        INVALID
    );
}
