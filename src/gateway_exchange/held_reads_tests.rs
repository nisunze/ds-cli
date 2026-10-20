use super::*;
use crate::gateway_exchange::retained::testing::{block_on, Answer, TestEdge};
use serde_json::json;

#[test]
fn a_revoked_ledger_forgets_what_this_device_held() {
    let edge = TestEdge::new();
    edge.json(json!({"__type": "summary", "count": 0}));
    let fresh = block_on(backup_ledger::refresh(&edge, 30, None)).unwrap();
    assert_eq!(fresh.value, json!({"rows": [], "count": 0}));
    assert!(block_on(backup_ledger::held(&edge, 30)).unwrap().is_some());
    assert_eq!(block_on(backup_ledger::held(&edge, 90)).unwrap(), None);
    edge.answer(Answer::Json(403, json!({"success": false, "error": "platform.admin required"})));
    assert!(block_on(backup_ledger::refresh(&edge, 30, None)).is_err());
    assert_eq!(block_on(backup_ledger::held(&edge, 30)).unwrap(), None);
}

#[test]
fn a_release_is_held_per_url() {
    let edge = TestEdge::new();
    let url = "https://gateway.example.test/api/v1/app_releases?channel=canary";
    assert_eq!(block_on(release::held(&edge, url)).unwrap(), None);
    block_on(release::retain(&edge, url, json!({"version": "1.2.3"})));
    assert_eq!(block_on(release::held(&edge, url)).unwrap().unwrap().value, json!({"release": {"version": "1.2.3"}}));
    assert_eq!(block_on(release::held(&edge, &format!("{url}x"))).unwrap(), None);
}

fn asset(id: &str) -> Value {
    json!({"asset_id": id, "project_id": "p", "title": id, "kind": "document", "status": "active", "sensitivity": "internal",
        "version": 1, "created_at": "2026-10-20T08:00:00Z", "updated_at": "2026-10-20T08:00:00Z"})
}

#[test]
fn the_assets_catalogue_walks_every_page_and_holds_the_whole_list() {
    let edge = TestEdge::new();
    edge.json(json!({"success": true, "data": {"folders": []}}));
    edge.json(json!({"success": true, "data": {"assets": [asset("a_1")], "has_more": true, "next_cursor": "c1", "limit": 200}}));
    edge.json(json!({"success": true, "data": {"assets": [asset("a_2")], "has_more": false, "limit": 200}}));
    let fresh = block_on(assets_catalogue::refresh(&edge, "p")).unwrap();
    assert_eq!(fresh.value["complete"], true);
    assert_eq!(edge.requests().len(), 3);
    let held = block_on(assets_catalogue::held(&edge, "p")).unwrap().unwrap();
    assert_eq!(held.value["assets"].as_array().map(Vec::len), fresh.value["assets"].as_array().map(Vec::len));
}
