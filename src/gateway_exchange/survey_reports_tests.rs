use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn answered<C: Exchange>(call: C, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else { panic!("expected a fetch") };
    let observed = Observed::Response { status: 200, content_type: Some("application/json".into()), retry_after: None };
    match resume(continuation, observed, body.to_string().as_bytes(), &Facts::default()) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

fn cached(stale: bool, missing: bool) -> Cached {
    Cached { report: json!({"activeGrandTotal": 3}), stale, missing }
}

#[test]
fn the_reads_name_their_project_and_long_deadline() {
    let Step::Fetch { request, .. } = start(ReadReports { project: "it_rwanda".into(), include_sync_counts: true }) else { panic!() };
    assert_eq!((request.path.as_str(), request.timeout_ms), ("/api/v1/pipeline", 300_000));
    assert_eq!(request.body, Some(json!({"action": "get_reports", "eds_project_id": "it_rwanda", "include_sync_counts": true})));
    let Step::Fetch { request, .. } = start(RunReports { project: "it_rwanda".into(), force: true, include_sync_counts: false }) else { panic!() };
    assert_eq!(request.path, "/api/v1/data");
    assert_eq!(request.body, Some(json!({"action": "reports", "eds_project_id": "it_rwanda", "force": true})));
}

#[test]
fn a_cached_report_carries_its_stale_and_missing_marks() {
    let read = |data: Value| answered(ReadReports { project: "it_rwanda".into(), include_sync_counts: false }, json!({"success": true, "data": data})).unwrap();
    let report = read(json!({"activeGrandTotal": 3, "stale": true}));
    assert!(report.stale && !report.missing);
    let none = read(json!({"status": "no_reports", "stale": true}));
    assert!(none.missing && none.stale);
}

#[test]
fn a_stale_report_is_regenerated_once_and_kept_when_that_fails() {
    assert_eq!(after_read(cached(false, false), true), Ok(Shown { data: Some(json!({"activeGrandTotal": 3})), ..Shown::default() }));
    assert_eq!(after_read(cached(true, false), false).unwrap().data, Some(json!({"activeGrandTotal": 3})));
    assert_eq!(after_read(cached(false, true), true), Ok(Shown::default()));
    let stale = after_read(cached(true, false), true).unwrap_err();
    let fresh = after_run(stale.clone(), Ok(json!({"activeGrandTotal": 4})));
    assert_eq!((fresh.data, fresh.refreshed), (Some(json!({"activeGrandTotal": 4})), Some(true)));
    let kept = after_run(stale, Err(Refusal::new("HTTP_503", "busy")));
    assert_eq!((kept.data, kept.refreshed, kept.error.as_deref()), (Some(json!({"activeGrandTotal": 3})), Some(false), Some("busy")));
    let none = after_run(cached(true, true), Err(Refusal::new("HTTP_503", "busy")));
    assert_eq!((none.data, none.error.as_deref()), (None, Some("busy")));
}

#[test]
#[cfg(feature = "durable-store")]
fn a_report_is_held_per_project_without_its_sync_counts() {
    use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
    let edge = TestEdge::new();
    assert_eq!(block_on(held_report(&edge, "p")).unwrap(), None);
    let report = json!({"totals": {"poles": 3}, "sync_counts": {"forms": []}});
    block_on(retain_report(&edge, "p", report)).unwrap();
    assert_eq!(block_on(held_report(&edge, "p")).unwrap(), Some(json!({"totals": {"poles": 3}})));
    assert_eq!(block_on(held_report(&edge, "q")).unwrap(), None);
    assert!(block_on(retain_report(&edge, "p", json!([1]))).is_err());
}

#[test]
#[cfg(feature = "durable-store")]
fn a_read_or_a_regeneration_holds_the_report_it_shows() {
    use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
    let edge = TestEdge::new();
    edge.json(json!({"success": true, "data": {"totals": {"poles": 3}, "stale": false}}));
    let shown = block_on(read_retained(&edge, "p", false, true));
    assert_eq!(shown.data.as_ref().unwrap()["totals"]["poles"], 3);
    assert_eq!(block_on(held_report(&edge, "p")).unwrap().unwrap()["totals"]["poles"], 3);

    edge.json(json!({"success": true, "data": {"totals": {"poles": 5}, "sync_counts": {"forms": []}}}));
    let regenerated = block_on(run_retained(&edge, "p", false, true));
    assert!(regenerated.data.as_ref().unwrap().get("sync_counts").is_some(), "shown with its counts");
    assert_eq!(
        block_on(held_report(&edge, "p")).unwrap(),
        Some(json!({"totals": {"poles": 5}})),
        "held without them"
    );

    edge.answer(crate::gateway_exchange::retained::testing::Answer::Json(
        500,
        json!({"success": false, "error": {"code": "INTERNAL", "message": "down"}}),
    ));
    let failed = block_on(run_retained(&edge, "p", false, false));
    assert!(failed.error.is_some());
    assert_eq!(block_on(held_report(&edge, "p")).unwrap().unwrap()["totals"]["poles"], 5, "a refusal holds nothing");
}
