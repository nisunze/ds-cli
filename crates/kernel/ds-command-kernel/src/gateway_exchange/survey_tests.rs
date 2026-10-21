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
    let observed = Observed::Response {
        status,
        content_type: Some(kind.into()),
        retry_after: None,
    };
    match resume(continuation, observed, body, &Facts::default()) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

#[cfg(feature = "durable-store")]
mod hold {
    use super::*;
    use crate::gateway_exchange::retained::testing::{block_on, Answer, TestEdge};
    use crate::gateway_exchange::Failure;

    fn feature(id: &str, x: f64, updated: &str, deleted: bool) -> Value {
        json!({"type": "Feature", "geometry": {"type": "Point", "coordinates": [x, -2.0]},
            "properties": {"id": id, "metadata": {"firestore_updated_at": updated, "is_deleted": deleted}}})
    }

    fn stream(rows: &[Value]) -> Answer {
        let mut lines = rows.to_vec();
        lines.push(json!({"__type": "summary", "form_count": 1, "errors": [], "total_features": rows.len()}));
        Answer::Ndjson(lines)
    }

    fn ask(refresh: &str) -> HoldRequest {
        HoldRequest {
            project: "it_rwanda".into(),
            form: "poles".into(),
            filter: Filter::default(),
            refresh: Some(refresh.into()),
            updated_after: None,
        }
    }

    fn ids(answer: &HoldAnswer) -> Vec<&str> {
        answer
            .rows
            .iter()
            .map(|row| row["properties"]["id"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn a_form_is_read_once_kept_and_then_painted_offline() {
        let edge = TestEdge::new();
        assert_eq!(
            block_on(hold_read(&edge, ask("local"))).unwrap_err().code,
            NOT_HELD
        );
        edge.answer(stream(&[
            feature("a", 29.5, "2026-10-20T09:00:00Z", false),
            feature("b", 29.6, "2026-10-20T09:01:00Z", false),
        ]));
        let first = block_on(hold_read(&edge, ask("auto"))).unwrap();
        assert_eq!((first.source, ids(&first)), (Source::Replace, vec!["a", "b"]));
        let sent = edge.requests();
        assert_eq!(sent[0].path, "/api/v1/data");
        assert_eq!(
            sent[0].body,
            Some(json!({"action": "query_entries", "eds_project_id": "it_rwanda",
                "form_slugs": ["poles"], "exclude_deleted": true}))
        );
        assert_eq!(first.held.cursor.as_deref(), Some("2026-10-20T09:01:00Z"));

        edge.online.set(false);
        let offline = block_on(hold_read(&edge, ask("local"))).unwrap();
        assert_eq!((offline.source, ids(&offline)), (Source::Held, vec!["a", "b"]));
        assert_eq!(edge.requests().len(), 1, "a local read never reaches the cloud");
        // A young copy is reused without asking.
        let young = block_on(hold_read(&edge, ask("auto"))).unwrap();
        assert_eq!(young.source, Source::Held);
    }

    #[test]
    fn a_delta_merges_changes_and_tombstones_onto_what_is_held() {
        let edge = TestEdge::new();
        edge.answer(stream(&[
            feature("a", 29.5, "2026-10-20T09:00:00Z", false),
            feature("b", 29.6, "2026-10-20T09:01:00Z", false),
        ]));
        block_on(hold_read(&edge, ask("full"))).unwrap();
        edge.answer(stream(&[
            feature("b", 29.6, "2026-10-20T10:00:00Z", true),
            feature("c", 29.7, "2026-10-20T10:01:00Z", false),
        ]));
        let delta = block_on(hold_read(&edge, ask("delta"))).unwrap();
        assert_eq!((delta.source, ids(&delta)), (Source::Delta, vec!["a", "c"]));
        let body = edge.requests()[1].body.clone().unwrap();
        assert_eq!(body["updated_after"], "2026-10-20T09:01:00Z");
        assert_eq!(body["exclude_deleted"], false, "a delta must see deletions");
        let kept = block_on(hold_read(&edge, ask("local"))).unwrap();
        assert_eq!(ids(&kept), vec!["a", "c"]);
        assert_eq!(kept.held.rows, 2);
    }

    #[test]
    fn a_failed_refresh_leaves_the_copy_and_another_account_holds_nothing() {
        let edge = TestEdge::new();
        edge.answer(stream(&[feature("a", 29.5, "2026-10-20T09:00:00Z", false)]));
        block_on(hold_read(&edge, ask("full"))).unwrap();
        // A stream cut off before its summary is refused.
        edge.answer(Answer::Ndjson(vec![feature("b", 29.6, "2026-10-20T10:00:00Z", false)]));
        assert!(block_on(hold_read(&edge, ask("full"))).is_err());
        edge.answer(Answer::Failed(Failure::Network));
        assert!(block_on(hold_read(&edge, ask("full"))).is_err());
        assert_eq!(ids(&block_on(hold_read(&edge, ask("local"))).unwrap()), vec!["a"]);
        edge.sign_in("someone-else");
        assert_eq!(
            block_on(hold_read(&edge, ask("local"))).unwrap_err().code,
            NOT_HELD
        );
    }

    #[test]
    fn a_box_narrows_a_whole_form_locally_and_bad_requests_never_leave() {
        let edge = TestEdge::new();
        edge.answer(stream(&[
            feature("a", 29.5, "2026-10-20T09:00:00Z", false),
            feature("b", 30.5, "2026-10-20T09:01:00Z", false),
        ]));
        block_on(hold_read(&edge, ask("full"))).unwrap();
        let boxed = HoldRequest {
            filter: Filter {
                bbox: Some([29.0, -3.0, 30.0, -1.0]),
                ..Filter::default()
            },
            ..ask("local")
        };
        assert_eq!(ids(&block_on(hold_read(&edge, boxed)).unwrap()), vec!["a"]);
        for bad in [
            HoldRequest { form: "Poles A".into(), ..ask("auto") },
            HoldRequest { project: "a b".into(), ..ask("auto") },
            HoldRequest { refresh: Some("soon".into()), ..ask("auto") },
        ] {
            assert!(block_on(hold_read(&edge, bad)).is_err());
        }
        assert_eq!(edge.requests().len(), 1);
    }
}

#[test]
fn a_mutation_is_one_put_never_resent_and_read_as_the_outbox_does() {
    let payload = json!({"operation": "delete", "eds_project_id": "it_rwanda", "form_id": "poles", "doc_id": "d1"});
    let call = Mutation {
        payload: payload.as_object().cloned().unwrap(),
    };
    let plan = call.plan().unwrap();
    assert_eq!(
        (
            plan.request.method,
            plan.request.path.as_str(),
            plan.retries
        ),
        (Method::Put, "/api/v1/entries/mutate", 0)
    );
    let ok = answered(
        call.clone(),
        200,
        br#"{"success":true,"data":{"doc_id":"d1"}}"#,
        "application/json",
    );
    assert_eq!(mutation_outcome(ok), MutationOutcome::Success);
    let conflict = answered(call, 409, br#"{"success":false,"error":{"code":"VERSION_CONFLICT","message":"stale","details":{"current_version":4}}}"#, "application/json");
    assert_eq!(
        mutation_outcome(conflict),
        MutationOutcome::Response {
            status: 409,
            details: Some(json!({"current_version": 4})),
            network: false
        }
    );
    let invalid = Mutation {
        payload: json!({"operation": "archive"}).as_object().cloned().unwrap(),
    };
    assert_eq!(
        mutation_outcome(invalid.plan().map(|_| Value::Null)),
        MutationOutcome::Response {
            status: 400,
            details: None,
            network: false
        }
    );
}

#[test]
fn a_discard_audit_names_its_project_and_bounded_entries() {
    let call = Discarded {
        project: "it_rwanda".into(),
        entries: vec![json!({"id": "e1"})],
    };
    assert_eq!(
        call.plan().unwrap().request.body,
        Some(json!({"action": "submit_client_discarded",
        "project_id": "it_rwanda", "entries": [{"id": "e1"}]}))
    );
    assert_eq!(
        Discarded {
            project: "it_rwanda".into(),
            entries: vec![]
        }
        .plan()
        .unwrap_err()
        .code,
        INPUT_INVALID
    );
}

#[test]
fn a_restore_is_a_mutation_too() {
    let payload = json!({"operation": "restore", "project_id": "it_rwanda", "form_id": "poles", "doc_id": "d1"});
    let call = Mutation {
        payload: payload.as_object().cloned().unwrap(),
    };
    assert_eq!(call.plan().unwrap().request.path, "/api/v1/entries/mutate");
}

#[test]
fn a_create_is_never_a_mutation_brain_never_creates() {
    let call = Mutation {
        payload: json!({"operation": "create", "project_id": "kigali", "form_id": "poles", "doc_id": "d1"})
            .as_object()
            .cloned()
            .unwrap(),
    };
    assert_eq!(call.plan().unwrap_err().code, INPUT_INVALID);
}
