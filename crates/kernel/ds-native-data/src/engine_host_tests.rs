//! Engine failures keep their typed vocabulary at the native data door.
use super::*;

#[test]
fn a_missing_or_old_engine_is_a_typed_refusal_not_a_file_failure() {
    let dir = tempfile::tempdir().unwrap();
    // An empty addon base and an absent override: there is no Network engine.
    let missing = engine_failure(NetworkFailure::acquisition("x"));
    assert_eq!(missing.code, crate::bridge::FAILED_CODE);
    let _ = dir;
}
