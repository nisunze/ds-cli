//! Protected-state refusals lead with the comparison that separates a
//! restricted execution context from damaged state, and keep their codes.

use super::{STATE_ACCESS_REMEDY, STATE_UNSAFE_REMEDY, StoreError, state_failure};

#[test]
fn access_and_unsafe_refusals_compare_a_normal_shell_before_any_repair() {
    for (error, code, remedy) in [
        (
            StoreError::Unavailable,
            "native_state_unavailable",
            STATE_ACCESS_REMEDY,
        ),
        (
            StoreError::UnsafeOrUnreadable,
            "native_state_unsafe",
            STATE_UNSAFE_REMEDY,
        ),
    ] {
        let failure = state_failure(error);
        assert_eq!(failure.code(), code);
        let text = failure.remedy_text().unwrap();
        assert_eq!(text, remedy);
        let shell = text.find("normal shell for this OS user").unwrap();
        let repair = text.find("only if it also fails there").unwrap();
        assert!(shell < repair, "the context comparison comes first");
        assert!(text.contains("do not reinstall or sign in again"));
    }
}

#[test]
fn a_lease_conflict_keeps_its_own_wait_and_retry_reading() {
    let failure = state_failure(StoreError::Conflict);
    assert_eq!(failure.code(), "native_state_conflict");
    assert!(!failure.remedy_text().unwrap().contains("normal shell"));
}
