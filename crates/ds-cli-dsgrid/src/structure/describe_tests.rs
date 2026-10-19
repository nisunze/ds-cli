#[test]
fn description_bound_matches_the_owner() {
    assert_eq!(
        super::MAX_STRUCTURE_DESCRIPTION_CHARS,
        ds_grid_engine::MAX_STRUCTURE_DESCRIPTION_CHARS
    );
}
