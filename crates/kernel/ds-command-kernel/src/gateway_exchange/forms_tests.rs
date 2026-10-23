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
fn generated_form_routes_preserve_exact_body_bytes_bounds_and_retry_budgets() {
    let get = forms(
        FormsEndpoint::FormFactory,
        json!({"action":"get","slug":"poles","ignored":null}),
    )
    .plan()
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&get.request).unwrap(),
        br#"{"method":"POST","path":"/api/v1/form-factory","body":{"action":"get","slug":"poles"},"timeout_ms":120000,"response_cap_bytes":33554432}"#,
    );
    assert_eq!(get.retries, 1);
    let activate = forms(
        FormsEndpoint::ProjectForms,
        json!({"action":"activate","project_id":"p1"}),
    )
    .plan()
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&activate.request).unwrap(),
        br#"{"method":"POST","path":"/api/v1/project-forms","body":{"action":"activate","project_id":"p1"},"timeout_ms":120000,"response_cap_bytes":33554432}"#,
    );
    assert_eq!(activate.retries, 0);
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
        json!({"action": "bulk_save", "project_id": "example_rwanda", "changes": []}),
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

#[test]
fn an_application_refusal_keeps_its_original_words_code_and_details() {
    let error=forms(FormsEndpoint::FormFactory,json!({"action":"list"})).admit(200,json!({"status":"error","error":{"code":"permission_denied","message":"Requires form factory access","details":{"remedy":"Sign in again"}}}));
    let Err(refusal) = error else {
        panic!("an application refusal cannot be admitted")
    };
    assert_eq!(refusal.code, "permission_denied");
    assert_eq!(refusal.message, "Requires form factory access");
    assert_eq!(refusal.details, json!({"remedy":"Sign in again"}).into());
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
    assert_eq!(
        block_on(held(&edge, FormsEndpoint::FormFactory, &get)).unwrap(),
        None
    );
    let form = json!({"status": "success", "form": {"slug": "poles"}});
    edge.json(form.clone());
    assert_eq!(
        block_on(request(&edge, FormsEndpoint::FormFactory, get.clone())).unwrap(),
        form
    );
    let same = json!({"slug": "poles", "action": "get"})
        .as_object()
        .cloned()
        .unwrap();
    let kept = block_on(held(&edge, FormsEndpoint::FormFactory, &same)).unwrap();
    assert_eq!(
        kept,
        Some(HeldForms {
            value: form.clone(),
            stale: false
        })
    );
    edge.json(json!({"status": "success"}));
    let update = json!({"action": "update", "slug": "poles"})
        .as_object()
        .cloned()
        .unwrap();
    block_on(request(&edge, FormsEndpoint::FormFactory, update.clone())).unwrap();
    let stale = block_on(held(&edge, FormsEndpoint::FormFactory, &get)).unwrap();
    assert_eq!(
        stale,
        Some(HeldForms {
            value: form,
            stale: true
        })
    );
    assert!(block_on(held(&edge, FormsEndpoint::FormFactory, &update)).is_err());
    let activate = json!({"action": "activate", "project_id": "example_rwanda"})
        .as_object()
        .cloned()
        .unwrap();
    assert!(FormsEndpoint::ProjectForms.kept("activate"));
    assert_eq!(
        block_on(held(&edge, FormsEndpoint::ProjectForms, &activate)).unwrap(),
        None
    );
}

#[cfg(feature = "durable-store")]
#[test]
fn restore_and_navigation_paint_held_activation_without_any_gateway_call() {
    use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
    let edge = TestEdge::new();
    edge.online.set(false);
    assert_eq!(
        block_on(project_acquire(&edge, "p1".into(), ReadMode::Held)).unwrap(),
        None
    );
    assert!(edge.requests().is_empty());
    edge.online.set(true);
    edge.json(json!({"status":"success","project_id":"p1","forms":[{"slug":"pole"}],"settings_editors":{"pole":{"version":7}},"field_types":[]}));
    let fresh = block_on(project_acquire(&edge, "p1".into(), ReadMode::Open))
        .unwrap()
        .unwrap();
    assert_eq!(fresh["settings_editors"]["pole"]["version"], 7);
    assert_eq!(edge.requests().len(), 1);
    edge.online.set(false);
    for mode in [ReadMode::Held, ReadMode::Open] {
        let held = block_on(project_acquire(&edge, "p1".into(), mode))
            .unwrap()
            .unwrap();
        assert_eq!(held["forms"], json!([{"slug":"pole"}]));
        assert_eq!(held["settings_editors"], json!({}));
    }
    assert_eq!(edge.requests().len(), 1);
    assert_eq!(
        block_on(project_acquire(&edge, "p1".into(), ReadMode::Refresh))
            .unwrap_err()
            .code,
        crate::gateway_exchange::NETWORK_OFFLINE
    );
    assert!(
        block_on(project_acquire(&edge, "p1".into(), ReadMode::Held))
            .unwrap()
            .is_some()
    );
}

#[cfg(feature = "durable-store")]
#[test]
fn activation_refuses_an_unbound_project_or_missing_authoritative_catalogue_and_recovers() {
    use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
    let edge = TestEdge::new();
    edge.json(json!({"status":"success","project_id":"p2","forms":[]}));
    assert_eq!(
        block_on(project_acquire(&edge, "p1".into(), ReadMode::Open))
            .unwrap_err()
            .code,
        ANSWER_INVALID
    );
    assert!(
        block_on(project_acquire(&edge, "p1".into(), ReadMode::Held))
            .unwrap()
            .is_none()
    );
    edge.json(json!({"status":"success","project_id":"p1"}));
    assert_eq!(
        block_on(project_acquire(&edge, "p1".into(), ReadMode::Open))
            .unwrap_err()
            .code,
        ANSWER_INVALID
    );
    edge.json(json!({"status":"success","project_id":"p1","forms":[]}));
    assert_eq!(
        block_on(project_acquire(&edge, "p1".into(), ReadMode::Open))
            .unwrap()
            .unwrap()["forms"],
        json!([])
    );
    edge.sign_in("other");
    assert_eq!(
        block_on(project_acquire(&edge, "p1".into(), ReadMode::Held)).unwrap(),
        None
    );
}

#[cfg(feature = "durable-store")]
#[test]
fn held_restore_preserves_a_storage_refusal_and_never_starts_a_fresh_request() {
    use crate::durable_store::Call;
    use crate::gateway_exchange::retained::{
        testing::{block_on, TestEdge},
        Edge,
    };
    struct Refusing(TestEdge, std::cell::Cell<bool>);
    impl Edge for Refusing {
        async fn fetch<C: Exchange>(&self, call: C) -> Result<C::Output, Refusal> {
            self.0.fetch(call).await
        }
        async fn store(&self, call: Call) -> Result<Value, Refusal> {
            if self.1.replace(false) {
                return Err(Refusal::new(
                    "auth_context_mismatch",
                    "Restore the current account.",
                ));
            }
            self.0.store(call).await
        }
        fn now_ms(&self) -> u64 {
            self.0.now_ms()
        }
        fn online(&self) -> bool {
            false
        }
    }
    let edge = Refusing(TestEdge::new(), std::cell::Cell::new(true));
    let refused = block_on(project_acquire(&edge, "p1".into(), ReadMode::Held)).unwrap_err();
    assert_eq!(refused.code, "auth_context_mismatch");
    assert_eq!(refused.message, "Restore the current account.");
    assert!(edge.0.requests().is_empty());
    assert!(
        block_on(project_acquire(&edge, "p1".into(), ReadMode::Held))
            .unwrap()
            .is_none()
    );
    assert!(edge.0.requests().is_empty());
}

#[cfg(feature = "durable-store")]
#[test]
fn held_factory_answers_use_the_same_admission_as_fresh_answers() {
    use crate::gateway_exchange::retained::{
        testing::{block_on, TestEdge},
        write,
    };
    let edge = TestEdge::new();
    let body = Map::from_iter([("action".into(), json!("list"))]);
    let key = kept_key(FormsEndpoint::FormFactory, &body).unwrap();
    block_on(write(
        &edge,
        None,
        crate::retained_reads::Kind::FormsRead,
        None,
        &key,
        &json!({"status":"success","forms":"bad"}),
        None,
    ))
    .unwrap();
    assert_eq!(
        block_on(acquire(
            &edge,
            FormsEndpoint::FormFactory,
            body.clone(),
            ReadMode::Open
        ))
        .unwrap_err()
        .code,
        ANSWER_INVALID
    );
    assert!(edge.requests().is_empty());
    edge.json(json!({"status":"success","forms":[]}));
    assert_eq!(
        block_on(acquire(
            &edge,
            FormsEndpoint::FormFactory,
            body,
            ReadMode::Refresh
        ))
        .unwrap()
        .unwrap()
        .value["forms"],
        json!([])
    );
}
