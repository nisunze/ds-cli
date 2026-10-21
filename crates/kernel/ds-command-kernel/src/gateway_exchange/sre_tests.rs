use super::*;
use crate::gateway_exchange::{resume, start, Facts, Method, Observed, Step};
use serde_json::json;

fn answered<C: Exchange>(
    call: C,
    status: u16,
    body: &[u8],
    kind: &str,
) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else {
        panic!("expected a fetch")
    };
    let facts = Facts {
        now_ms: 1_760_000_000_000,
        has_session: true,
        offline_mode: false,
    };
    let observed = Observed::Response {
        status,
        content_type: Some(kind.into()),
        retry_after: None,
    };
    match resume(continuation, observed, body, &facts) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

fn json_answer<C: Exchange>(call: C, status: u16, body: Value) -> Result<C::Output, Refusal> {
    answered(
        call,
        status,
        body.to_string().as_bytes(),
        "application/json",
    )
}

fn request<C: Exchange>(call: C) -> Request {
    match start(call) {
        Step::Fetch { request, .. } => request,
        _ => panic!("expected a fetch"),
    }
}

#[test]
fn the_overview_is_one_global_get_and_reads_null_lists_as_empty() {
    let sent = request(Overview);
    assert_eq!(
        (sent.method, sent.path.as_str()),
        (Method::Get, "/api/v1/sre/overview")
    );
    assert_eq!(sent.body, None);
    let overview = json_answer(
        Overview,
        200,
        json!({"generated_at": "2026-10-20T10:00:00Z", "services": [{"key": "ds-brain",
            "display_name": "Brain", "availability": {"goal": 0.995, "sli": null,
            "budget_remaining_pct": 80.5, "burn_state": "ok"}}],
            "service_ops": null, "fleet": null, "combined_reports": null, "stale": null,
            "incidents": [], "error_catalog": [{"category": "container_pressure",
            "title": "Container pressure", "summary": "s", "runbook_url": "https://runbook"}]}),
    )
    .unwrap();
    assert_eq!(
        overview.services[0].availability.as_ref().unwrap().sli,
        None
    );
    assert!(overview.service_ops.is_empty() && overview.stale.is_empty());
    assert_eq!(overview.error_catalog[0].category, "container_pressure");
}

fn ndjson(lines: &[Value]) -> Vec<u8> {
    lines
        .iter()
        .map(|line| format!("{line}\n"))
        .collect::<String>()
        .into_bytes()
}

fn event(id: &str) -> Value {
    json!({"event_id": id, "ts": "2026-10-20T09:00:00Z", "trace_id": "t", "service": "ds-brain",
        "profile": "standard", "lane": "stable", "action_class": "report", "action": "report",
        "method": "POST", "path": "/report", "outcome": "failure", "status": 503,
        "latency_ms": 1200, "deadline_s": 30, "timed_out": false, "error_message": "busy",
        "ds_project": "it_rwanda", "user_email": "a@b.c", "capture_mode": "full"})
}

#[test]
fn an_activity_window_is_the_rows_its_summary_counts() {
    let call = Activity {
        days: 3,
        limit: 5000,
    };
    let sent = request(call);
    assert_eq!(sent.path, "/api/v1/data");
    assert_eq!(
        sent.body,
        Some(json!({"action": "query_table", "table": "sre_requests", "days": 3, "limit": 5000}))
    );
    let body = ndjson(&[
        event("a"),
        event("b"),
        json!({"__type": "summary", "table": "sre_requests", "count": 2, "status": "ok"}),
    ]);
    let window = answered(call, 200, &body, "application/x-ndjson").unwrap();
    assert_eq!(window.count, 2);
    assert_eq!(window.rows[1].event_id, "b");
    assert_eq!(window.rows[0].status, Some(503.0));
}

#[test]
fn an_activity_window_refuses_a_missing_failed_or_miscounted_summary() {
    let call = Activity { days: 3, limit: 10 };
    let without = ndjson(&[event("a")]);
    assert_eq!(
        answered(call, 200, &without, "application/x-ndjson")
            .unwrap_err()
            .code,
        INVALID
    );
    let failed = ndjson(&[
        event("a"),
        json!({"__type": "summary", "count": 1, "status": "error"}),
    ]);
    let refusal = answered(call, 200, &failed, "application/x-ndjson").unwrap_err();
    assert!(refusal.message.contains("after 1 rows"));
    let miscounted = ndjson(&[
        event("a"),
        json!({"__type": "summary", "count": 2, "status": "ok"}),
    ]);
    assert_eq!(
        answered(call, 200, &miscounted, "application/x-ndjson")
            .unwrap_err()
            .code,
        INVALID
    );
    let trailing = ndjson(&[
        json!({"__type": "summary", "count": 0, "status": "ok"}),
        event("a"),
    ]);
    assert_eq!(
        answered(call, 200, &trailing, "application/x-ndjson")
            .unwrap_err()
            .code,
        INVALID
    );
}

#[test]
fn an_activity_window_outside_its_bounds_is_never_sent() {
    for call in [
        Activity { days: 0, limit: 10 },
        Activity { days: 3, limit: 0 },
        Activity {
            days: 366,
            limit: 10,
        },
        Activity {
            days: 3,
            limit: 10_001,
        },
    ] {
        assert!(matches!(start(call), Step::Refused(refusal) if refusal.code == INPUT_INVALID));
    }
}

#[test]
fn a_snapshot_read_names_each_outcome() {
    let sent = request(ServerSnapshot);
    assert_eq!(
        (sent.method, sent.path.as_str()),
        (Method::Get, "/api/v1/sre/server-status")
    );
    let missing = json_answer(ServerSnapshot, 404, json!({"error": "not published"}));
    assert_eq!(snapshot_read(missing, 0), ServerSnapshotRead::Missing);
    let forbidden = json_answer(
        ServerSnapshot,
        403,
        json!({"error": "platform.admin required"}),
    );
    assert_eq!(
        snapshot_read(forbidden, 0),
        ServerSnapshotRead::Unauthorized
    );
    let malformed = json_answer(ServerSnapshot, 200, json!({"schema": 1}));
    assert_eq!(snapshot_read(malformed, 0), ServerSnapshotRead::Malformed);
    let failed = json_answer(ServerSnapshot, 400, json!({"error": "bad"}));
    assert_eq!(
        snapshot_read(failed, 0),
        ServerSnapshotRead::Transport {
            http_status: Some(400)
        }
    );
}

#[test]
fn a_snapshot_is_fresh_for_a_minute_and_stale_for_five() {
    assert_eq!(liveness(1_000, 1_060_000), (60, Liveness::Fresh));
    assert_eq!(liveness(1_000, 1_061_000), (61, Liveness::Stale));
    assert_eq!(liveness(1_000, 1_300_000), (300, Liveness::Stale));
    assert_eq!(liveness(1_000, 1_301_000), (301, Liveness::Offline));
    assert_eq!(liveness(2_000, 1_000_000), (1_000, Liveness::Offline));
}

fn runtime_event() -> SreClientEvent {
    SreClientEvent {
        source: "client_runtime".into(),
        action: "runtime_window_error".into(),
        action_class: "orchestration".into(),
        method: "CLIENT".into(),
        path: "/projects/[id]".into(),
        page_route: "/projects/[id]".into(),
        outcome: "failure".into(),
        error_message: "TypeError: x is undefined".into(),
        ds_project: Some("it_rwanda".into()),
        user_visible: true,
        ..SreClientEvent::default()
    }
}

#[test]
fn a_client_event_carries_the_web_lane_release_and_one_request_id() {
    let call = ClientEvent {
        event: runtime_event(),
        deploy_lane: "run".into(),
        release: Some("abc123".into()),
        request_id: "r-1".into(),
    };
    let sent = request(call.clone());
    assert_eq!(sent.path, "/api/v1/sre/requests");
    let body = sent.body.unwrap();
    assert_eq!(body["platform"], "web");
    assert_eq!(body["lane"], "canary");
    assert_eq!(body["release"], "abc123");
    assert_eq!(body["request_id"], "r-1");
    assert_eq!(body["user_action_id"], "r-1");
    assert_eq!(body["ds_project"], "it_rwanda");
    assert!(body.get("status").is_none());
    assert_eq!(json_answer(call, 202, json!({"accepted": true})), Ok(true));
    assert_eq!(event_lane("local"), "local");
    assert_eq!(event_lane("stable"), "stable");
    assert_eq!(event_lane(""), "stable");
}

#[test]
fn a_client_event_without_a_source_or_target_is_never_sent() {
    let mut event = runtime_event();
    event.source = "server".into();
    let call = ClientEvent {
        event,
        deploy_lane: String::new(),
        release: None,
        request_id: "r".into(),
    };
    assert!(matches!(start(call), Step::Refused(refusal) if refusal.code == INPUT_INVALID));
    let mut event = runtime_event();
    event.path.clear();
    event.action.clear();
    let call = ClientEvent {
        event,
        deploy_lane: String::new(),
        release: None,
        request_id: "r".into(),
    };
    assert!(matches!(start(call), Step::Refused(refusal) if refusal.code == INPUT_INVALID));
}

#[cfg(feature = "durable-store")]
mod held_activity {
    use super::super::{activity, held_activity, Activity};
    use crate::gateway_exchange::retained::testing::{block_on, Answer, TestEdge};
    use serde_json::json;

    #[test]
    fn the_last_activity_window_is_held_for_the_account_that_read_it() {
        let edge = TestEdge::new();
        assert_eq!(block_on(held_activity(&edge)).unwrap(), None);
        edge.answer(Answer::Ndjson(vec![
            json!({"event_id": "e1", "trace_id": "t1", "service": "brain", "outcome": "success"}),
            json!({"__type": "summary", "table": "sre_requests", "count": 1, "status": "ok"}),
        ]));
        let fresh = block_on(activity(&edge, Activity { days: 1, limit: 10 })).expect("a window");
        assert_eq!(fresh.count, 1);
        edge.online.set(false);
        assert_eq!(block_on(held_activity(&edge)).unwrap(), Some(fresh));
        edge.sign_in("someone-else");
        assert_eq!(block_on(held_activity(&edge)).unwrap(), None);
    }
}
