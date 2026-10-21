use ds_profile_view::{model::Scene, navigation::search};
use serde_json::json;
use std::collections::BTreeMap;
fn scene() -> Scene {
    serde_json::from_value(json!({"bands":[{"structures":[{"entity_id":"p10","engineering_name":"HS255 Pole","engineering_number":"10","ground":{"scene_x":20}},{"entity_id":"p1","engineering_name":"S190 strain","engineering_number":"1","ground":{"scene_x":50}},{"entity_id":"p2","engineering_name":"HS255 Pole","engineering_number":"2","ground":{"scene_x":10}}]}]})).unwrap()
}
#[test]
fn exact_number_precedes_prefix_and_names() {
    let s = scene();
    let result = search(&s, &BTreeMap::new(), "1");
    assert_eq!(
        result.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        vec!["p1", "p10"]
    );
}
#[test]
fn engineering_name_search_ignores_case_and_uses_station_order() {
    let s = scene();
    let result = search(&s, &BTreeMap::new(), "hS255");
    assert_eq!(
        result.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        vec!["p2", "p10"]
    );
}
#[test]
fn published_external_numbers_are_used_without_recomposing_labels() {
    let s = scene();
    let numbers = BTreeMap::from([("p2".into(), "R-17".into())]);
    assert_eq!(search(&s, &numbers, "R-17")[0].id, "p2");
}
