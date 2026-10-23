use super::*;

#[test]
fn a_clone_observes_cancellation() {
    let token = CancelToken::new();
    let clone = token.clone();
    assert!(!clone.is_cancelled());
    token.cancel();
    assert!(clone.is_cancelled());
}
