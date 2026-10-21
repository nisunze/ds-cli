//! Preserve the reviewed train-5 literal ceiling across printing integration.
#[test]
fn production_sources_match_the_template_literal_ratchet() {
    ds_template_test_support::assert_source_ratchet(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        std::path::Path::new("tests/fixtures/template-literal-baseline.json"),
    );
}
