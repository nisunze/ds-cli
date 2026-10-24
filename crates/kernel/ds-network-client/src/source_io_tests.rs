use super::*;

#[test]
fn source_engine_metadata_has_one_row_per_legacy_or_unknown_projection() {
    let mut codes = std::collections::BTreeSet::new();
    for row in SOURCE_ENGINE_REFUSALS {
        assert!(codes.insert(row.code), "duplicate {}", row.code);
        assert!(!row.when.is_empty());
        assert!(!row.remedy.is_empty());
    }
    assert!(
        !codes.contains("module_removed"),
        "the CLI retains its existing missing alias"
    );
    assert!(codes.contains("network_engine_refusal_unknown"));
}
