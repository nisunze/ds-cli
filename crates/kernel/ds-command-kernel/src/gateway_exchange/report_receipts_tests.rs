use super::*;
use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
use serde_json::json;

#[test]
fn receipts_are_held_per_account_and_bounded() {
    let edge = TestEdge::new();
    assert!(block_on(held(&edge)).unwrap().is_empty());
    let rows = vec![json!({"id": "n2", "reportJob": {"phase": "rendering"}}), json!({"id": "n1"})];
    block_on(hold(&edge, rows.clone())).unwrap();
    assert_eq!(block_on(held(&edge)).unwrap(), rows);
    assert_eq!(block_on(hold(&edge, vec![json!({"detail": "no id"})])).unwrap_err().code, INVALID);
    let too_many: Vec<Value> = (0..=MAX_RECEIPTS).map(|n| json!({"id": format!("n{n}")})).collect();
    assert_eq!(block_on(hold(&edge, too_many)).unwrap_err().code, INVALID);
    edge.sign_in("someone_else");
    assert!(block_on(held(&edge)).unwrap().is_empty(), "never another account's");
}
