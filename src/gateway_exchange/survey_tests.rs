use super::*;
use crate::gateway_exchange::{resume, start, Facts, Method, Observed, Step, PROJECT_INVALID};
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

fn read(filter: SurveyFilter) -> Entries {
    Entries {
        project: "it_rwanda".into(),
        form: "poles".into(),
        filter,
        updated_after: None,
        include_tombstones: false,
    }
}

#[test]
fn a_read_names_its_form_and_filter_in_brains_fields() {
    let filter = SurveyFilter {
        bbox: Some(vec![29.0, -2.5, 30.0, -1.5]),
        admin_boundary: Some(
            json!({"level": "district", "code": "11"})
                .as_object()
                .cloned()
                .unwrap(),
        ),
        surveyors: Some(vec!["a@b.c".into()]),
        ..SurveyFilter::default()
    };
    let call = Entries {
        updated_after: Some("2026-10-20T00:00:00Z".into()),
        include_tombstones: true,
        ..read(filter)
    };
    let plan = call.plan().unwrap();
    assert_eq!(plan.request.path, "/api/v1/data");
    assert_eq!(
        plan.request.body,
        Some(
            json!({"action": "query_entries", "eds_project_id": "it_rwanda",
        "form_slugs": ["poles"], "bbox": [29.0, -2.5, 30.0, -1.5],
        "filter_admin_boundary": {"level": "district", "code": "11"}, "surveyors": ["a@b.c"],
        "updated_after": "2026-10-20T00:00:00Z", "exclude_deleted": false})
        )
    );
    assert_eq!(
        read(SurveyFilter::default()).plan().unwrap().request.body,
        Some(
            json!({"action": "query_entries", "eds_project_id": "it_rwanda", "form_slugs": ["poles"]})
        )
    );
}

#[test]
fn a_read_answers_every_line_with_the_summary_last() {
    let body = b"{\"type\":\"Feature\",\"properties\":{\"id\":\"e1\"}}\n\n{\"__type\":\"summary\",\"count\":1}\n";
    let lines = answered(
        read(SurveyFilter::default()),
        200,
        body,
        "application/x-ndjson",
    )
    .unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["__type"], "summary");
}

#[test]
fn a_read_without_a_form_project_or_square_bbox_is_never_sent() {
    let mut call = read(SurveyFilter::default());
    call.form = " ".into();
    assert_eq!(call.plan().unwrap_err().code, INPUT_INVALID);
    let mut call = read(SurveyFilter::default());
    call.project = "a b".into();
    assert_eq!(call.plan().unwrap_err().code, PROJECT_INVALID);
    let call = read(SurveyFilter {
        bbox: Some(vec![1.0, 2.0]),
        ..SurveyFilter::default()
    });
    assert_eq!(call.plan().unwrap_err().code, INPUT_INVALID);
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
        payload: json!({"operation": "create"}).as_object().cloned().unwrap(),
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
