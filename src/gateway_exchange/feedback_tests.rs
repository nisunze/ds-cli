use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step, PROJECT_INVALID};
use serde_json::json;

fn row(id: &str, status: &str) -> Value {
    json!({"id": id, "kind": "bug", "severity": "major", "title": "Layer list is empty",
        "detail": "It became empty after switching project.", "surface": "ds-web",
        "component": "ds-web/map", "project_id": "", "reporter_kind": "human", "occurrences": 2,
        "reporters": ["one@example.com", "", 7], "status": status, "resolution": " ", "version": 3,
        "created_at": "2026-10-19T08:00:00Z", "last_seen_at": "2026-10-20T08:00:00Z",
        "updated_at": "2026-10-20T08:00:00Z", "updated_by": "one@example.com", "fingerprint": "f",
        "note_count": 0})
}

fn request<C: Exchange>(call: C) -> Result<Request, Refusal> {
    match start(call) {
        Step::Fetch { request, .. } => Ok(request),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected a request or a refusal"),
    }
}

fn answered<C: Exchange>(call: C, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else {
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
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

fn sighting() -> SubmitHuman {
    SubmitHuman {
        title: " Layer list is empty ".into(),
        detail: "It became empty after switching project.\n".into(),
        component: "ds-web/map".into(),
        kind: "bug".into(),
        severity: "major".into(),
        project: Some("project-1".into()),
    }
}

#[test]
fn the_backlog_is_typed_with_its_vocabulary_and_addressed_flag() {
    assert_eq!(
        request(Backlog).unwrap().body,
        Some(json!({"action": "list", "limit": 200}))
    );
    let backlog = answered(
        Backlog,
        json!({"success": true, "data": {"reports": [row("a", "in_progress"), row("b", "wont_fix")], "count": 2}}),
    )
    .unwrap();
    assert_eq!(backlog.statuses, FeedbackStatus::ALL.to_vec());
    let a = &backlog.reports[0];
    assert_eq!((a.status, a.addressed), (FeedbackStatus::InProgress, false));
    assert_eq!(a.reporters, ["one@example.com"]);
    assert_eq!(
        (a.project_id.as_deref(), a.resolution.as_deref()),
        (None, None)
    );
    assert!(backlog.reports[1].addressed);
    assert_eq!(
        serde_json::to_value(a).unwrap()["status"],
        json!("in_progress")
    );
}

#[test]
fn a_report_outside_the_contract_refuses_the_backlog() {
    for (key, value) in [
        ("status", json!("archived")),
        ("reporter_kind", json!("robot")),
        ("occurrences", json!(0)),
        ("version", json!(0)),
        ("title", json!(" ")),
    ] {
        let mut bad = row("a", "open");
        bad[key] = value;
        let refused = answered(
            Backlog,
            json!({"success": true, "data": {"reports": [bad]}}),
        )
        .unwrap_err();
        assert_eq!(refused.code, INVALID, "{key}");
    }
    assert_eq!(
        answered(Backlog, json!({"success": true, "data": {}}))
            .unwrap_err()
            .code,
        INVALID
    );
    let stated = answered(
        Backlog,
        json!({"success": false, "error": "backlog unavailable"}),
    )
    .unwrap_err();
    assert_eq!(stated.message, "backlog unavailable");
}

#[test]
fn a_status_change_is_fenced_by_its_version_and_trimmed() {
    let call = UpdateStatus {
        id: "a".into(),
        expected_version: 3,
        status: FeedbackStatus::Resolved,
        resolution: "  fixed in r22 ".into(),
    };
    assert_eq!(
        request(call.clone()).unwrap().body,
        Some(
            json!({"action": "update_status", "id": "a", "expected_version": 3,
            "status": "resolved", "resolution": "fixed in r22"})
        )
    );
    let updated = answered(
        call.clone(),
        json!({"success": true, "data": {"report": row("a", "resolved")}}),
    )
    .unwrap();
    assert!(updated.addressed);
    for bad in [
        UpdateStatus {
            expected_version: 0,
            ..call.clone()
        },
        UpdateStatus {
            id: "".into(),
            ..call.clone()
        },
        UpdateStatus {
            resolution: "x".repeat(4_001),
            ..call
        },
    ] {
        assert_eq!(request(bad).unwrap_err().code, INPUT_INVALID);
    }
}

#[test]
fn a_human_sighting_is_filed_under_its_project_and_admitted() {
    assert_eq!(
        request(sighting()).unwrap().body,
        Some(
            json!({"action": "submit", "report": {"reporter_kind": "human", "surface": "ds-web",
            "title": "Layer list is empty", "detail": "It became empty after switching project.",
            "component": "ds-web/map", "kind": "bug", "severity": "major", "project_id": "project-1"}})
        )
    );
    let without_project = request(SubmitHuman {
        project: None,
        ..sighting()
    })
    .unwrap();
    assert!(without_project.body.unwrap()["report"]
        .get("project_id")
        .is_none());
    let submitted = answered(
        sighting(),
        json!({"success": true, "data": {"report": row("a", "open"), "deduplicated": true}}),
    )
    .unwrap();
    assert!(submitted.deduplicated);
    assert_eq!(submitted.report.id, "a");
}

#[test]
fn a_sighting_outside_its_bounds_is_refused_before_any_request() {
    for bad in [
        SubmitHuman {
            title: " ".into(),
            ..sighting()
        },
        SubmitHuman {
            detail: "x".repeat(16_385),
            ..sighting()
        },
        SubmitHuman {
            kind: "rant".into(),
            ..sighting()
        },
        SubmitHuman {
            severity: "urgent".into(),
            ..sighting()
        },
    ] {
        assert_eq!(request(bad).unwrap_err().code, INPUT_INVALID);
    }
    assert_eq!(
        request(SubmitHuman {
            project: Some("a/b".into()),
            ..sighting()
        })
        .unwrap_err()
        .code,
        PROJECT_INVALID
    );
}
