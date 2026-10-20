use super::*;
use crate::gateway_exchange::PROJECT_INVALID;
use serde_json::json;

fn forms(endpoint: FormsEndpoint, request: Value) -> Forms {
    Forms {
        endpoint,
        request: request.as_object().cloned().unwrap(),
    }
}

#[test]
fn a_form_request_is_sent_flat_to_its_endpoint() {
    let plan = forms(
        FormsEndpoint::FormFactory,
        json!({"action": "get", "slug": "poles", "status": null}),
    )
    .plan()
    .unwrap();
    assert_eq!(plan.request.path, "/api/v1/form-factory");
    assert_eq!(
        plan.request.body,
        Some(json!({"action": "get", "slug": "poles"}))
    );
    assert_eq!((plan.request.timeout_ms, plan.retries), (120_000, 1));
    let save = forms(
        FormsEndpoint::ProjectForms,
        json!({"action": "bulk_save", "project_id": "it_rwanda", "changes": []}),
    )
    .plan()
    .unwrap();
    assert_eq!(
        (save.request.path.as_str(), save.retries),
        ("/api/v1/project-forms", 0)
    );
}

#[test]
fn an_unknown_action_or_project_is_never_sent() {
    assert_eq!(
        forms(FormsEndpoint::FormFactory, json!({"action": "drop"}))
            .plan()
            .unwrap_err()
            .code,
        INPUT_INVALID
    );
    assert_eq!(
        forms(FormsEndpoint::ProjectForms, json!({"action": "list"}))
            .plan()
            .unwrap_err()
            .code,
        INPUT_INVALID
    );
    assert_eq!(
        forms(FormsEndpoint::ProjectForms, json!({"action": "activate"}))
            .plan()
            .unwrap_err()
            .code,
        INPUT_INVALID
    );
    assert_eq!(
        forms(
            FormsEndpoint::ProjectForms,
            json!({"action": "activate", "project_id": "a b"})
        )
        .plan()
        .unwrap_err()
        .code,
        PROJECT_INVALID
    );
}
