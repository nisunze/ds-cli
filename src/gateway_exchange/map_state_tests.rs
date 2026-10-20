use super::*;
use crate::gateway_exchange::retained::testing::{account, block_on, TestEdge};
use serde_json::json;

fn state(project: &str) -> Value {
    json!({"project_id": project, "layers": [{"id": "lv_lines"}], "sources": {}})
}

#[test]
fn a_kept_state_is_held_for_its_project_and_account_only() {
    let edge = TestEdge::new();
    assert_eq!(block_on(held(&edge, "p")).unwrap(), None);
    block_on(keep(&edge, "p", &state("p"))).unwrap();
    let held_state = block_on(held(&edge, "p")).unwrap().unwrap();
    assert_eq!(held_state.value, state("p"));
    assert!(!held_state.invalidated);
    assert_eq!(block_on(held(&edge, "q")).unwrap(), None);
    edge.identity.replace(account("other"));
    assert_eq!(block_on(held(&edge, "p")).unwrap(), None);
}

#[test]
fn a_state_is_kept_only_under_its_own_project_and_can_be_forgotten() {
    let edge = TestEdge::new();
    assert!(block_on(keep(&edge, "p", &state("q"))).is_err());
    block_on(keep(&edge, "p", &state("p"))).unwrap();
    block_on(forget(&edge, "p")).unwrap();
    assert_eq!(block_on(held(&edge, "p")).unwrap(), None);
    assert!(edge.requests().is_empty());
}
