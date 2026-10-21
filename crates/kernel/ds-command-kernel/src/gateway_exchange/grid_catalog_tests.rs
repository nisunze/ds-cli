use super::*;
use crate::gateway_exchange::retained::testing::{block_on, Answer, TestEdge};
use serde_json::json;

fn scope() -> Scope {
    Scope {
        principal: "owner".into(),
        lane: "canary".into(),
        audience: "firebase:ds-test;gateway:https://gateway.example.test".into(),
        project: "it_rwanda".into(),
    }
}

fn head(id: &str) -> Value {
    json!({"model_id": id, "project_id": "it_rwanda", "name": id, "head_revision_id": format!("{id}-r1"),
        "updated_at": "2026-10-19T08:00:00Z"})
}

#[test]
fn a_refresh_lists_the_project_once_and_holds_what_it_listed() {
    let edge = TestEdge::new();
    edge.json(json!({"success": true, "data": {"models": [head("mv_a"), head("mv_b")], "has_more": false, "next_cursor": null}}));
    let catalog = block_on(read(&edge, scope(), ReadMode::Refresh)).expect("a catalogue");
    let sent = edge.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].path, "/api/v1/grid/models");
    assert_eq!(sent[0].body.as_ref().unwrap()["action"], "list_models");
    let held = block_on(read(&edge, scope(), ReadMode::Held)).expect("the held catalogue");
    assert_eq!(edge.requests().len(), 1, "a held read sends nothing");
    for answer in [&catalog, &held] {
        let text = answer.to_string();
        assert!(text.contains("\"mv_a\"") && text.contains("\"mv_b\""), "{text}");
    }
}

#[test]
fn a_refused_listing_keeps_what_the_page_held() {
    let edge = TestEdge::new();
    edge.json(json!({"success": true, "data": {"models": [head("mv_a")], "has_more": false, "next_cursor": null}}));
    block_on(read(&edge, scope(), ReadMode::Refresh)).expect("a catalogue");
    edge.answer(Answer::Json(503, json!({"error": "busy"})));
    let after = block_on(read(&edge, scope(), ReadMode::Refresh)).expect("the held catalogue, labelled");
    assert!(after.to_string().contains("mv_a"), "{after}");
}
