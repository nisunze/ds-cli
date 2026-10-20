use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn call(action: Action, orders: Vec<Order>) -> LayerAction {
    LayerAction {
        action,
        project: "it_rwanda".into(),
        orders,
    }
}

fn answered(call: LayerAction, body: Value) -> Result<Value, Refusal> {
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

fn document() -> Value {
    json!({
        "project_id": "it_rwanda",
        "resolved_at": 1,
        "sources": {"s": {"type": "geojson"}},
        "styles": {"a": {}},
        "style_editors": [],
        "layers": [{"id": "a", "type": "circle", "source": "s"}],
        "sidebar": {"groups": [{"key": "g", "layers": ["a"]}]},
        "survey_layers": [],
        "design_layers": []
    })
}

#[test]
fn the_document_read_sends_the_owning_body_under_its_bounds() {
    let Step::Fetch { request, .. } = start(call(Action::Refresh, vec![])) else {
        panic!("expected a fetch")
    };
    assert_eq!(request.path, "/api/v1/layers");
    assert_eq!(
        request.body,
        Some(json!({"action": "refresh", "project_id": "it_rwanda", "platform": "web"}))
    );
    assert_eq!(request.timeout_ms, 120_000);
    assert_eq!(request.response_cap_bytes, 32 * 1024 * 1024);
}

#[test]
fn a_document_is_admitted_and_a_broken_one_names_its_issues() {
    let admitted = answered(
        call(Action::GetConfig, vec![]),
        json!({"success": true, "data": document()}),
    )
    .unwrap();
    assert_eq!(admitted["project_id"], "it_rwanda");

    let mut broken = document();
    broken["layers"] = json!([{"id": "a", "type": "circle", "source": "missing"}]);
    let refusal = answered(
        call(Action::GetConfig, vec![]),
        json!({"success": true, "data": broken}),
    )
    .unwrap_err();
    assert_eq!(refusal.code, "contract_violation");
    assert!(refusal.details.is_some());
}

#[test]
fn a_reorder_sends_its_rows_and_admits_only_their_receipt() {
    let orders = vec![
        Order {
            layer_id: "a".into(),
            order: 2,
        },
        Order {
            layer_id: "b".into(),
            order: 1,
        },
    ];
    let Step::Fetch { request, .. } = start(call(Action::Reorder, orders.clone())) else {
        panic!("expected a fetch")
    };
    assert_eq!(
        request.body,
        Some(json!({"action": "reorder", "project_id": "it_rwanda",
            "layer_order": [{"layer_id": "a", "order": 2}, {"layer_id": "b", "order": 1}]}))
    );
    let receipt = answered(
        call(Action::Reorder, orders.clone()),
        json!({"success": true, "data": {"project_id": "it_rwanda", "reordered": 2}}),
    )
    .unwrap();
    assert_eq!(receipt, json!({"project_id": "it_rwanda", "reordered": 2}));
    let short = answered(
        call(Action::Reorder, orders),
        json!({"success": true, "data": {"project_id": "it_rwanda", "reordered": 1}}),
    );
    assert_eq!(short.unwrap_err().code, "unreadable_response");
}

#[test]
fn an_invalid_request_never_leaves() {
    for bad in [
        call(Action::Reorder, vec![]),
        LayerAction {
            action: Action::GetConfig,
            project: " spaced ".into(),
            orders: vec![],
        },
        call(Action::SetDefaultVisibility, vec![]),
    ] {
        assert!(matches!(start(bad), Step::Refused(refusal) if refusal.code == INPUT_INVALID));
    }
}
