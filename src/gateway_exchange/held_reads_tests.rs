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
    edge.answer(Answer::Json(
        403,
        json!({"success": false, "error": "platform.admin required"}),
    ));
    assert!(block_on(backup_ledger::refresh(&edge, 30, None)).is_err());
    assert_eq!(block_on(backup_ledger::held(&edge, 30)).unwrap(), None);
}

#[test]
fn a_release_is_held_per_url() {
    let edge = TestEdge::new();
    let url = "https://gateway.example.test/api/v1/app_releases?channel=canary";
    assert_eq!(block_on(release::held(&edge, url)).unwrap(), None);
    block_on(release::retain(&edge, url, json!({"version": "1.2.3"})));
    assert_eq!(
        block_on(release::held(&edge, url)).unwrap().unwrap().value,
        json!({"release": {"version": "1.2.3"}})
    );
    assert_eq!(
        block_on(release::held(&edge, &format!("{url}x"))).unwrap(),
        None
    );
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
    let held = block_on(assets_catalogue::held(&edge, "p"))
        .unwrap()
        .unwrap();
    assert_eq!(
        held.value["assets"].as_array().map(Vec::len),
        fresh.value["assets"].as_array().map(Vec::len)
    );
}

/// An edge on which a configuration save lands while the refresh is in flight.
struct SaveDuringFetch<'a>(&'a TestEdge);

impl crate::gateway_exchange::retained::Edge for SaveDuringFetch<'_> {
    async fn fetch<C: crate::gateway_exchange::Exchange>(
        &self,
        call: C,
    ) -> Result<C::Output, Refusal> {
        project_config::forget(self.0, "p").await.unwrap();
        self.0.fetch(call).await
    }
    async fn store(&self, call: crate::durable_store::Call) -> Result<Value, Refusal> {
        self.0.store(call).await
    }
    fn now_ms(&self) -> u64 {
        self.0.now_ms()
    }
    fn online(&self) -> bool {
        self.0.online()
    }
}

#[test]
fn a_project_config_fetched_before_a_save_is_never_kept() {
    let edge = TestEdge::new();
    edge.json(json!({"sheets": {"rev": 1}}));
    let fresh = block_on(project_config::refresh(&edge, "p")).unwrap();
    assert_eq!(fresh.value, json!({"sheets": {"rev": 1}}));
    assert_eq!(
        block_on(project_config::held(&edge, "p"))
            .unwrap()
            .unwrap()
            .value,
        fresh.value
    );
    edge.json(json!({"sheets": {"rev": 0}}));
    let stale = block_on(project_config::refresh(&SaveDuringFetch(&edge), "p")).unwrap_err();
    assert_eq!(stale.code, crate::retained_reads::STALE);
    assert_eq!(block_on(project_config::held(&edge, "p")).unwrap(), None);
}

#[test]
fn the_pipeline_status_is_held_per_project() {
    let edge = TestEdge::new();
    edge.json(json!({"success": true, "data": {"exports": {"tile": {}}}}));
    block_on(pipeline_status::refresh(&edge, "p")).unwrap();
    assert!(block_on(pipeline_status::held(&edge, "p"))
        .unwrap()
        .is_some());
    assert_eq!(block_on(pipeline_status::held(&edge, "q")).unwrap(), None);
}

struct UnobservedOwnerDuringFetch<'a> {
    edge: &'a TestEdge,
    first_read: std::cell::Cell<bool>,
}

impl crate::gateway_exchange::retained::Edge for UnobservedOwnerDuringFetch<'_> {
    async fn fetch<C: crate::gateway_exchange::Exchange>(
        &self,
        call: C,
    ) -> Result<C::Output, Refusal> {
        self.edge.sign_in("another-account");
        self.edge.fetch(call).await
    }
    async fn store(&self, call: crate::durable_store::Call) -> Result<Value, Refusal> {
        if self.first_read.replace(false) {
            Err(Refusal::new(
                "store_unavailable",
                "The owner could not be observed.",
            ))
        } else {
            self.edge.store(call).await
        }
    }
    fn now_ms(&self) -> u64 {
        self.edge.now_ms()
    }
    fn online(&self) -> bool {
        self.edge.online()
    }
}

#[test]
fn a_fresh_answer_never_enters_an_account_whose_owner_was_not_observed() {
    let edge = TestEdge::new();
    edge.json(json!({"success":true,"data":{"exports":{"tile":{}}}}));
    let unobserved = UnobservedOwnerDuringFetch {
        edge: &edge,
        first_read: std::cell::Cell::new(true),
    };
    assert!(block_on(pipeline_status::refresh(&unobserved, "p")).is_ok());
    assert_eq!(block_on(pipeline_status::held(&edge, "p")).unwrap(), None);
}

fn version_row(id: &str, ordinal: u64) -> Value {
    json!({"project_id": "p", "object": {"kind": "lv_transformer", "id": "T1"}, "version_id": id, "version": ordinal,
        "source_revision": "r", "playback_available": true})
}

#[test]
fn version_history_is_admitted_by_the_authority_before_it_is_held() {
    let edge = TestEdge::new();
    edge.json(json!({"success": true, "data": {"versions": [version_row("v2", 2), version_row("v1", 1)]}}));
    let fresh = block_on(transformer_versions::refresh(&edge, "p", "T1")).unwrap();
    assert_eq!(fresh.value.as_array().map(Vec::len), Some(2));
    assert_eq!(
        block_on(transformer_versions::held(&edge, "p", "T1"))
            .unwrap()
            .unwrap()
            .value,
        fresh.value
    );
    let mut foreign = version_row("v3", 3);
    foreign["object"]["id"] = json!("T2");
    edge.json(json!({"success": true, "data": {"versions": [foreign]}}));
    assert!(block_on(transformer_versions::refresh(&edge, "p", "T1")).is_err());
    assert_eq!(
        block_on(transformer_versions::held(&edge, "p", "T1"))
            .unwrap()
            .unwrap()
            .value,
        fresh.value
    );
}

#[test]
fn a_downloaded_version_is_held_counted_and_cleaned_per_project() {
    let edge = TestEdge::new();
    let snapshot = json!({"schema": "ds.transformer-version-snapshot/v1", "layers": {"tr":{"type":"FeatureCollection","features":[]}}});
    for (id, ordinal) in [("v1", 1), ("v2", 2)] {
        edge.json(json!({"success": true, "data": {"version": version_row(id, ordinal), "snapshot": snapshot}}));
        block_on(transformer_versions::download(&edge, "p", "T1", id)).unwrap();
    }
    let held = block_on(transformer_versions::downloaded(&edge, "p", "T1", "v1"))
        .unwrap()
        .unwrap();
    assert_eq!(held.value["snapshot"], snapshot);
    assert_eq!(held.value["version"]["version_id"], "v1");
    assert_eq!(
        block_on(transformer_versions::downloads(&edge, "p")).unwrap(),
        2
    );
    edge.json(
        json!({"success": true, "data": {"version": version_row("v2", 2), "snapshot": snapshot}}),
    );
    assert!(block_on(transformer_versions::download(&edge, "p", "T1", "v1")).is_err());
    for invalid_snapshot in [
        json!(null),
        json!({"schema":"wrong","layers":{}}),
        json!({"schema":"ds.transformer-version-snapshot/v1","layers":{}}),
    ] {
        edge.json(json!({"success":true,"data":{"version":version_row("v1",1),"snapshot":invalid_snapshot}}));
        assert!(block_on(transformer_versions::download(&edge, "p", "T1", "v1")).is_err());
        assert_eq!(
            block_on(transformer_versions::downloaded(&edge, "p", "T1", "v1"))
                .unwrap()
                .unwrap(),
            held
        );
    }
    let mut unavailable = version_row("v1", 1);
    unavailable["playback_available"] = false.into();
    edge.json(json!({"success":true,"data":{"version":unavailable,"snapshot":snapshot}}));
    assert!(block_on(transformer_versions::download(&edge, "p", "T1", "v1")).is_err());
    assert_eq!(
        block_on(transformer_versions::downloads(&edge, "p")).unwrap(),
        2
    );
    block_on(transformer_versions::forget_downloads(&edge, "p")).unwrap();
    assert_eq!(
        block_on(transformer_versions::downloads(&edge, "p")).unwrap(),
        0
    );
    assert_eq!(
        block_on(transformer_versions::downloaded(&edge, "p", "T1", "v2")).unwrap(),
        None
    );
}
