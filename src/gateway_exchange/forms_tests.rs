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

#[cfg(feature = "durable-store")]
#[test]
fn a_form_read_is_kept_and_a_change_marks_it_stale() {
    use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
    let edge = TestEdge::new();
    let get = json!({"action": "get", "slug": "poles", "ignored": null})
        .as_object()
        .cloned()
        .unwrap();
    assert_eq!(block_on(held(&edge, FormsEndpoint::FormFactory, &get)).unwrap(), None);
    let form = json!({"status": "success", "form": {"slug": "poles"}});
    edge.json(form.clone());
    assert_eq!(
        block_on(request(&edge, FormsEndpoint::FormFactory, get.clone())).unwrap(),
        form
    );
    let same = json!({"slug": "poles", "action": "get"}).as_object().cloned().unwrap();
    let kept = block_on(held(&edge, FormsEndpoint::FormFactory, &same)).unwrap();
    assert_eq!(kept, Some(HeldForms { value: form.clone(), stale: false }));
    edge.json(json!({"status": "success"}));
    let update = json!({"action": "update", "slug": "poles"}).as_object().cloned().unwrap();
    block_on(request(&edge, FormsEndpoint::FormFactory, update.clone())).unwrap();
    let stale = block_on(held(&edge, FormsEndpoint::FormFactory, &get)).unwrap();
    assert_eq!(stale, Some(HeldForms { value: form, stale: true }));
    assert!(block_on(held(&edge, FormsEndpoint::FormFactory, &update)).is_err());
    let activate = json!({"action": "activate", "project_id": "it_rwanda"})
        .as_object()
        .cloned()
        .unwrap();
    assert!(FormsEndpoint::ProjectForms.kept("activate"));
    assert_eq!(block_on(held(&edge, FormsEndpoint::ProjectForms, &activate)).unwrap(), None);
}
