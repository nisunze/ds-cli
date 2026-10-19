#[test]
fn protected_mode_code_is_the_owner_code() {
    assert_eq!(
        super::PROTECTED_MODEL.code,
        ds_grid_engine::model_protection::PROTECTED_CODE
    );
}
