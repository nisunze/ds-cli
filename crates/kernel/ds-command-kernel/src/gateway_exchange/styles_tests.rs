use super::*;
use crate::gateway_exchange::{resume, start, Facts, Method, Observed, Step};
use serde_json::json;

fn answered(call: StyleAction, body: Value) -> Result<Value, Refusal> {
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

fn sent(call: StyleAction) -> Result<Request, Refusal> {
    match start(call) {
        Step::Fetch { request, .. } => Ok(request),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected a fetch"),
    }
}

fn payload() -> Value {
    json!({
        "style_ref": "master/roads", "style_target": "design", "layer_name": "roads",
        "style_data": {"type": "line", "paint": {"line-width": 3}}
    })
}

#[test]
fn a_publish_sends_the_payload_and_admits_the_receipt_for_it() {
    let call = StyleAction::Publish { payload: payload() };
    let request = sent(call.clone()).unwrap();
    assert_eq!(
        (request.method, request.path.as_str(), request.timeout_ms),
        (Method::Post, "/api/v1/styles", 30_000)
    );
    assert_eq!(request.response_cap_bytes, 1024 * 1024);
    assert_eq!(request.body, Some(json!({"action": "update_style", "data": payload()})));

    let receipt = json!({"success": true, "data": {
        "updated": true, "style_target": "design", "layer_name": "roads",
        "style_data": {"type": "line", "paint": {"line-width": 3}}
    }});
    let admitted = answered(call.clone(), receipt).unwrap();
    assert_eq!(admitted["layer_name"], "roads");

    let elsewhere = json!({"success": true, "data": {
        "updated": true, "style_target": "print", "layer_name": "roads",
        "style_data": {"type": "line"}
    }});
    let refused = answered(call, elsewhere).unwrap_err();
    assert_eq!(refused.code, "unreadable_response");
}

#[test]
fn the_read_family_sends_its_closed_bodies() {
    let seed = StyleAction::Seed {
        style_ref: "ud/x".into(),
        geometry: "MultiLineString".into(),
        seed_kind: Some("vt".into()),
        seed: Some("x".into()),
    };
    let body = sent(seed).unwrap().body.unwrap();
    assert_eq!(body["action"], "seed_style");
    assert_eq!(body["data"], json!({"style_ref": "ud/x", "geometry": "line", "kind": "vt", "seed": "x"}));

    let shared = sent(StyleAction::ReadShared { style_ref: "pd/p/roads".into() }).unwrap();
    assert_eq!(shared.response_cap_bytes, 4 * 1024 * 1024);
    assert_eq!(shared.body.unwrap()["action"], "get_user_data_style");

    let runtime = StyleAction::AssembleRuntime {
        logical_root: "notes".into(),
        geometry: "point".into(),
        style_data: json!({"type": "circle"}),
    };
    assert_eq!(
        sent(runtime).unwrap().body.unwrap()["action"],
        "assemble_work_map_runtime_style"
    );
}

#[test]
fn an_input_the_contract_refuses_is_never_sent() {
    for call in [
        StyleAction::Publish { payload: json!({"style_ref": ""}) },
        StyleAction::Seed {
            style_ref: "ud/x".into(),
            geometry: "GeometryCollection".into(),
            seed_kind: None,
            seed: None,
        },
        StyleAction::AssembleRuntime {
            logical_root: String::new(),
            geometry: "point".into(),
            style_data: json!({"type": "circle"}),
        },
    ] {
        assert_eq!(sent(call).unwrap_err().code, INPUT_INVALID);
    }
}

#[test]
fn a_refusal_names_the_backend_issues_as_text() {
    let call = StyleAction::Publish { payload: payload() };
    let Step::Fetch { continuation, .. } = start(call) else {
        panic!("expected a fetch")
    };
    let body = json!({"success": false, "error": {"code": "VALIDATION_FAILED",
        "message": "Invalid style document",
        "details": {"issues": [{"path": "paint.line-color", "message": "unknown expression operator"},
                               {"message": "document has no type"}]}}});
    let observed = Observed::Response {
        status: 400,
        content_type: Some("application/json".into()),
        retry_after: None,
    };
    let Step::Refused(refusal) = resume(
        continuation,
        observed,
        body.to_string().as_bytes(),
        &Facts::default(),
    ) else {
        panic!("expected a refusal")
    };
    let named = with_issues(refusal);
    assert_eq!(named.code, "VALIDATION_FAILED");
    assert_eq!(
        named.details,
        Some(json!(["paint.line-color: unknown expression operator", "document has no type"]))
    );
    let bare = with_issues(Refusal {
        details: Some(json!({"valid_actions": ["update_style"]})),
        ..Refusal::new("x", "y")
    });
    assert_eq!(bare.details, None);
}
