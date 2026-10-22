use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn answered<C: Exchange>(call: C, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else { panic!("expected a fetch") };
    let observed = Observed::Response { status: 200, content_type: Some("application/json".into()), retry_after: None };
    match resume(continuation, observed, body.to_string().as_bytes(), &Facts::default()) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

fn row() -> Value {
    json!({"id": "a".repeat(64), "version": "b".repeat(64), "label": "Buildings", "active": true, "kind": "bigquery",
        "group": "built", "layer": "buildings", "country": "Rwanda", "rows": 10, "bytes": 100, "cataloged_at": 1})
}

#[test]
fn a_catalogue_is_admitted_whole_or_refused() {
    let Step::Fetch { request, .. } = start(Catalogue { project: "it_rwanda".into() }) else { panic!() };
    assert_eq!(request.body, Some(json!({"action": "list_datasets", "project_id": "it_rwanda"})));
    assert_eq!(answered(Catalogue { project: "it_rwanda".into() }, json!({"success": true, "data": [row()]})).unwrap(), vec![row()]);
    let mut bundled = row();
    bundled["download_url"] = json!("https://evil.example/bundle");
    assert_eq!(answered(Catalogue { project: "it_rwanda".into() }, json!({"data": [row(), bundled]})).unwrap_err().code, INVALID);
    let mut inactive = row();
    inactive["active"] = json!(false);
    assert!(!dataset(&inactive));
}

fn page(offset: u64, count: usize, next: Option<u64>, area: Value) -> Value {
    let features: Vec<Value> = (0..count).map(|i| json!({"type": "Feature", "properties": {"i": i}, "geometry": null})).collect();
    let feature_json = serde_json::to_string(&features).unwrap();
    json!({"success": true, "data": {"resource_id": "a".repeat(64), "version": "b".repeat(64), "offset": offset, "area": area,
        "feature_json": feature_json, "feature_count": count, "next_offset": next,
        "sha256": format!("{:x}", Sha256::digest(feature_json.as_bytes()))}})
}

fn query(offset: u64, area: Option<Value>) -> QueryPage {
    QueryPage { project: "it_rwanda".into(), resource_id: "a".repeat(64), version: "b".repeat(64), offset, area }
}

#[test]
fn a_page_answers_exactly_what_was_asked() {
    let area = json!({"type": "Polygon", "coordinates": [[[30.0, -2.0], [30.1, -2.0], [30.1, -2.1], [30.0, -2.0]]]});
    let reordered = json!({"coordinates": area["coordinates"], "type": "Polygon"});
    let admitted = answered(query(0, Some(area.clone())), page(0, 3, None, reordered)).unwrap();
    assert_eq!((admitted.features.len(), admitted.next_offset), (3, None));
    assert!(answered(query(0, None), page(0, 3, None, area.clone())).is_err(), "another area");
    assert!(answered(query(1000, Some(area.clone())), page(0, 3, None, area.clone())).is_err(), "another offset");
    assert!(answered(query(0, Some(area.clone())), page(0, 3, Some(1000), area.clone())).is_err(), "a short page that claims more");
    let mut tampered = page(0, 3, None, area.clone());
    tampered["data"]["sha256"] = json!("0".repeat(64));
    assert!(answered(query(0, Some(area)), tampered).is_err(), "a digest mismatch");
}

#[test]
#[cfg(feature = "durable-store")]
fn a_catalogue_is_held_per_project_and_admitted_again() {
    use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
    let edge = TestEdge::new();
    assert_eq!(block_on(held_catalogue(&edge, "p")).unwrap(), None);
    let retained = block_on(retain_catalogue(&edge, "p", vec![row()])).unwrap();
    assert_eq!(retained.resources, vec![row()]);
    let held = block_on(held_catalogue(&edge, "p")).unwrap().unwrap();
    assert_eq!(held.resources, vec![row()]);
    assert_eq!(block_on(held_catalogue(&edge, "q")).unwrap(), None);
    assert!(block_on(retain_catalogue(&edge, "p", vec![json!({"id": "x"})])).is_err());
    edge.sign_in("someone_else");
    assert_eq!(block_on(held_catalogue(&edge, "p")).unwrap(), None);
}
