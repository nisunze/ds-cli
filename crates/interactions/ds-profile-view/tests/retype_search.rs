use ds_profile_view::{panel_model::Definition, retype_search::search};
fn entry(id: &str, label: &str) -> Definition {
    Definition {
        entity_id: id.into(),
        label: label.into(),
        ..Definition::default()
    }
}
fn ids(entries: &[Definition], query: &str) -> Vec<String> {
    search(entries, query)
        .into_iter()
        .map(|e| e.entity_id.clone())
        .collect()
}
#[test]
fn assembly_and_drawing_tokens_precede_incidental_matches() {
    let entries = [
        entry("st-1", "newS325-note"),
        entry("st-2", "j-w-60d-S325.014"),
        entry("st-3", "S325 support"),
    ];
    assert_eq!(ids(&entries, "S325"), ["st-3", "st-2", "st-1"]);
    assert_eq!(ids(&entries, "W"), ["st-2", "st-1"]);
}
#[test]
fn empty_query_preserves_library_order() {
    let entries = [entry("st-2", "J assembly"), entry("st-1", "L assembly")];
    assert_eq!(ids(&entries, "  "), ["st-2", "st-1"]);
}
#[test]
fn exact_catalogue_id_precedes_label_prefix_and_tokens() {
    let entries = [
        entry("other", "pole-17 support"),
        entry("pole-17", "Drawing"),
        entry("token", "X-pole-17"),
    ];
    assert_eq!(ids(&entries, " POLE-17 "), ["pole-17", "other", "token"]);
}
#[test]
fn equal_ranks_are_stable_and_unicode_assembly_tokens_work() {
    let entries = [
        entry("b", "X-école"),
        entry("a", "Y-école"),
        entry("c", "réécole"),
    ];
    assert_eq!(ids(&entries, "ÉCO"), ["b", "a", "c"]);
}
#[test]
fn the_whole_query_is_searched_without_splitting_into_new_conditions() {
    let entries = [
        entry("a", "S325 support"),
        entry("b", "S325 another support"),
    ];
    assert_eq!(ids(&entries, "S325 support"), ["a"]);
}
