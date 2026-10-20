use super::*;
use crate::gateway_exchange::{resume, start, Facts, Method, Observed, Step, PROJECT_INVALID};
use serde_json::json;

fn answered<C: Exchange>(call: C, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else {
        panic!("expected a fetch")
    };
    let observed = Observed::Response {
        status: 200,
        content_type: Some("application/json".into()),
        retry_after: None,
    };
    match resume(
        continuation,
        observed,
        body.to_string().as_bytes(),
        &Facts::default(),
    ) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

fn action(endpoint: ProjectEndpoint, name: &str, data: Value) -> ProjectAction {
    ProjectAction {
        endpoint,
        action: name.into(),
        data: data.as_object().cloned().unwrap(),
    }
}

#[test]
fn the_directory_is_one_status_and_an_older_brain_holds_none_of_a_new_state() {
    let call = Directory {
        status: "archived".into(),
    };
    let plan = call.plan().unwrap();
    assert_eq!(
        (plan.request.method, plan.request.path.as_str()),
        (Method::Get, "/api/v1/user/projects")
    );
    assert_eq!(
        plan.request.query,
        [("status".to_owned(), "archived".to_owned())]
    );
    let listed = answered(
        call,
        json!({"success": true, "data": {"status": "archived", "count": 1,
        "projects": [{"id": "p1", "project_name": "p1"}]}}),
    )
    .unwrap();
    assert_eq!(listed["projects"][0]["id"], "p1");
    let older = answered(
        Directory {
            status: "template".into(),
        },
        json!({"data": {"status": "active", "projects": [{"id": "p1"}]}}),
    )
    .unwrap();
    assert_eq!(older["projects"], json!([]));
    assert!(
        matches!(start(Directory { status: "all".into() }), Step::Refused(r) if r.code == INPUT_INVALID)
    );
}

#[test]
fn an_action_is_sent_in_brains_envelope_with_its_project_checked() {
    let share = action(
        ProjectEndpoint::Access,
        "share_project",
        json!({"project_id": "it_rwanda", "target_user_email": "a@b.c", "role": "viewer", "note": null}),
    );
    let plan = share.plan().unwrap();
    assert_eq!(plan.request.path, "/api/v1/projects/access");
    assert_eq!(
        plan.request.body,
        Some(json!({"action": "share_project",
        "data": {"project_id": "it_rwanda", "target_user_email": "a@b.c", "role": "viewer"}}))
    );
    assert_eq!(plan.retries, 0);
    assert_eq!(
        action(ProjectEndpoint::Templates, "list", json!({"limit": 500}))
            .plan()
            .unwrap()
            .retries,
        1
    );
    let archive = action(
        ProjectEndpoint::Governance,
        "archive_project",
        json!({"project_id": "it_rwanda"}),
    );
    assert_eq!(archive.plan().unwrap().request.path, "/api/v1/governance");
    let users = answered(
        action(
            ProjectEndpoint::Access,
            "list_users",
            json!({"project_id": "it_rwanda"}),
        ),
        json!({"data": {"users": [{"email": "a@b.c"}], "members_version": 7}}),
    )
    .unwrap();
    assert_eq!(users["members_version"], 7);
}

#[test]
fn an_action_outside_its_endpoint_or_with_a_bad_project_is_never_sent() {
    assert_eq!(
        action(ProjectEndpoint::Governance, "set_app_roles", json!({}))
            .plan()
            .unwrap_err()
            .code,
        INPUT_INVALID
    );
    assert_eq!(
        action(ProjectEndpoint::Templates, "share_project", json!({}))
            .plan()
            .unwrap_err()
            .code,
        INPUT_INVALID
    );
    assert_eq!(
        action(
            ProjectEndpoint::Access,
            "copy_users",
            json!({"from_project_id": "a/b", "to_project_id": "it_rwanda"})
        )
        .plan()
        .unwrap_err()
        .code,
        PROJECT_INVALID
    );
}
