//! Held-first project configuration: the held document answers without a
//! fetch, only a project with nothing held is acquired once, and the answer
//! names its actual source.
use super::project_config;
use crate::gateway_exchange::design::{
    admit_project_configuration_reply, ProjectConfigurationRead as Read,
    PROJECT_CONFIG_RECEIPT_INVALID,
};
use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
use serde_json::json;

#[test]
fn acquire_fetches_only_when_nothing_is_held_and_names_its_source() {
    let edge = TestEdge::new();
    edge.json(json!({"sheets": {"project_settings": [{"parameter": "a", "value": 1}]}}));
    let (first, source) = block_on(project_config::acquire(&edge, "p")).unwrap();
    assert_eq!(source, Read::Refresh);
    assert_eq!(
        edge.requests().len(),
        1,
        "a missing document is acquired once"
    );
    let (second, source) = block_on(project_config::acquire(&edge, "p")).unwrap();
    assert_eq!(source, Read::Held);
    assert_eq!(second.value, first.value);
    assert_eq!(
        edge.requests().len(),
        1,
        "a held document is never fetched again"
    );
}

#[test]
fn a_held_document_answers_offline_without_any_request() {
    let edge = TestEdge::new();
    edge.json(json!({"sheets": {"rev": 1}}));
    block_on(project_config::refresh(&edge, "p")).unwrap();
    edge.online.set(false);
    let (held, source) = block_on(project_config::acquire(&edge, "p")).unwrap();
    assert_eq!(
        (held.value, source),
        (json!({"sheets": {"rev": 1}}), Read::Held)
    );
    assert_eq!(
        edge.requests().len(),
        1,
        "offline held-first acquisition sends nothing"
    );
}

#[test]
fn offline_with_nothing_held_refuses_and_keeps_nothing() {
    let edge = TestEdge::new();
    edge.online.set(false);
    let refusal = block_on(project_config::acquire(&edge, "p")).unwrap_err();
    assert_eq!(
        refusal.code,
        crate::gateway_exchange::offline_refusal().code
    );
    assert!(edge.requests().is_empty());
    assert_eq!(block_on(project_config::held(&edge, "p")).unwrap(), None);
}

#[test]
fn a_save_forgets_the_held_document_so_the_next_acquire_reads_again() {
    let edge = TestEdge::new();
    edge.json(json!({"sheets": {"rev": 1}}));
    block_on(project_config::acquire(&edge, "p")).unwrap();
    block_on(project_config::forget(&edge, "p")).unwrap();
    edge.json(json!({"sheets": {"rev": 2}}));
    let (fresh, source) = block_on(project_config::acquire(&edge, "p")).unwrap();
    assert_eq!(
        (fresh.value, source),
        (json!({"sheets": {"rev": 2}}), Read::Refresh)
    );
    assert_eq!(edge.requests().len(), 2);
}

#[test]
fn an_observation_names_what_produced_the_document_never_the_request_mode() {
    for (read, source, admitted) in [
        (Read::Acquire, Read::Held, true),
        (Read::Acquire, Read::Refresh, true),
        (Read::Acquire, Read::Acquire, false),
        (Read::Held, Read::Held, true),
        (Read::Held, Read::Refresh, false),
        (Read::Refresh, Read::Refresh, true),
        (Read::Refresh, Read::Held, false),
        (Read::Held, Read::Acquire, false),
    ] {
        assert_eq!(read.admits_source(source), admitted, "{read:?} {source:?}");
        let reply = json!({"project": "p", "document": {"sheets": {}, "future": 1},
            "observation": {"source": source, "saved_at_ms": 5}, "future_receipt": true});
        let answer = admit_project_configuration_reply("p", read, reply);
        if admitted {
            let answer = answer.unwrap();
            assert_eq!(answer.observation.source, source);
            assert_eq!(answer.document.additional["future"], 1);
        } else {
            assert_eq!(answer.unwrap_err().code, PROJECT_CONFIG_RECEIPT_INVALID);
        }
    }
}
