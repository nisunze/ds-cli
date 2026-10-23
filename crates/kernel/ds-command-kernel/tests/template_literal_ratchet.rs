//! Preserve the reviewed train-5 literal ceiling across every Kernel crate.
//! The scan root is `crates/kernel`, so recorded paths name their crate.
#[test]
fn production_sources_match_the_template_literal_ratchet() {
    let kernel = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("ds-command-kernel sits in the Kernel crate group");
    ds_template_test_support::assert_source_ratchet(
        kernel,
        std::path::Path::new("ds-command-kernel/tests/fixtures/template-literal-baseline.json"),
    );
}
