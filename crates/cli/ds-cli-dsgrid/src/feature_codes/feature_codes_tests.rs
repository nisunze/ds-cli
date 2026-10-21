#[test]
fn bundled_standard_names_match_the_owner() {
    assert_eq!(
        super::BUNDLED_STANDARD_NAME,
        ds_grid_engine::feature_code_standard::BUNDLED_STANDARD_NAME
    );
    assert_eq!(
        super::VOLTAGE_CLASS_ARG.choices,
        ds_grid_engine::feature_code_standard::VOLTAGE_CLASSES
    );
}
