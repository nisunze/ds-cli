use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step, PROJECT_INVALID};
use serde_json::json;

fn request<C: Exchange>(call: C) -> Request {
    match start(call) {
        Step::Fetch { request, .. } => request,
        _ => panic!("expected a fetch"),
    }
}

fn answered<C: Exchange>(call: C, status: u16, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else {
        panic!("expected a fetch")
    };
    let observed = Observed::Response {
        status,
        content_type: Some("application/json".into()),
        retry_after: None,
    };
    match resume(
        continuation,
        observed,
        body.to_string().as_bytes(),
        &Facts::default(),
    ) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

fn list(query: IndexQuery) -> IndexList {
    IndexList {
        project: "kigali".into(),
        query,
    }
}

#[test]
fn a_page_names_exactly_the_keys_it_was_asked_for() {
    let plain = request(list(IndexQuery::default()));
    assert_eq!(plain.path, "/api/v1/assets");
    assert_eq!(
        plain.body,
        Some(json!({"action": "list", "ds_project": "kigali", "view": "index", "order": "recent"}))
    );
    assert_eq!(
        (plain.timeout_ms, plain.response_cap_bytes),
        (300_000, 8 * 1024 * 1024)
    );
    let full = request(list(IndexQuery {
        order: Some("name".into()),
        kind: Some("doc".into()),
        since: Some("2025-01-01".into()),
        limit: Some(60),
        cursor: Some("c1".into()),
        refresh: Some(true),
        folder: Some(String::new()),
        ..IndexQuery::default()
    }));
    assert_eq!(
        full.body,
        Some(
            json!({"action": "list", "ds_project": "kigali", "view": "index", "order": "name", "kind": "doc",
            "since": "2025-01-01", "cursor": "c1", "limit": 60, "refresh": true})
        )
    );
    let not_refreshed = request(list(IndexQuery {
        refresh: Some(false),
        limit: Some(0),
        ..IndexQuery::default()
    }));
    assert_eq!(not_refreshed.body.unwrap().as_object().unwrap().len(), 4);
}

#[test]
fn an_invalid_project_is_refused_before_any_request() {
    for project in ["", "a/b", ".."] {
        let Step::Refused(refusal) = start(IndexList {
            project: project.into(),
            query: IndexQuery::default(),
        }) else {
            panic!("{project:?} must refuse")
        };
        assert_eq!(refusal.code, PROJECT_INVALID);
    }
}

#[test]
fn a_row_without_its_identity_is_dropped_and_counted() {
    let page = answered(
        list(IndexQuery::default()),
        200,
        json!({"success": true, "data": {"assets": [
            {"asset_id": "a1", "name": "Plan.pdf", "folder": "reports", "kind": "doc", "origin": {"source": "transformers", "ref": {"path": "x"}},
             "versions": {"count": 3, "current": "v3", "latest_at": "2026-10-01"}, "modified_at": "2026-10-02"},
            {"asset_id": "a2", "name": "Sheet", "folder_path": "data/sheets", "versions": {"count": 0}},
            {"asset_id": "", "name": "nameless id"},
            {"name": "no id"},
            "not a row"
        ], "more": true, "next_cursor": "n2", "index": {"schema": "ds.assets.index/v1", "generation": 7, "built_at": "2026-10-01",
            "rebuilt": true, "stale_sources": ["solar", ""], "truncated_sources": [{"source": "pm_tasks", "bound": 500, "actual": 812}]}}}),
    )
    .unwrap();
    assert_eq!(page.dropped, 3);
    assert_eq!(page.assets.len(), 2);
    let first = &page.assets[0];
    assert_eq!(
        (first.folder_path.as_str(), first.modified_at.as_deref()),
        ("reports", Some("2026-10-02"))
    );
    assert_eq!(
        first.versions,
        Some(IndexVersions {
            count: 3,
            current: Some("v3".into()),
            latest_at: Some("2026-10-01".into())
        })
    );
    assert_eq!(first.row["kind"], "doc");
    assert_eq!(
        serde_json::to_value(first).unwrap()["origin"]["source"],
        "transformers"
    );
    assert_eq!(page.assets[1].folder_path, "data/sheets");
    assert_eq!(
        page.assets[1].versions, None,
        "a version summary needs at least one version"
    );
    assert!(page.more);
    assert_eq!(page.next_cursor.as_deref(), Some("n2"));
    let meta = page.index.unwrap();
    assert_eq!(
        (meta.generation, meta.rebuilt, meta.checked_at),
        (7.0, true, None)
    );
    assert_eq!(meta.stale_sources, ["solar"]);
    assert_eq!(meta.truncated_sources[0].actual, 812.0);
}

#[test]
fn a_bare_or_empty_answer_is_an_empty_page() {
    let page = answered(list(IndexQuery::default()), 200, json!({})).unwrap();
    assert_eq!(
        (
            page.assets.len(),
            page.more,
            page.index.is_none(),
            page.dropped
        ),
        (0, false, true, 0)
    );
}

#[test]
fn one_row_needs_its_identity_and_names_its_versions() {
    let get = IndexGet {
        project: "kigali".into(),
        asset_id: "a1".into(),
    };
    assert_eq!(
        request(get.clone()).body,
        Some(json!({"action": "get", "ds_project": "kigali", "view": "index", "asset_id": "a1"}))
    );
    let answer = answered(
        get.clone(),
        200,
        json!({"data": {"asset": {"asset_id": "a1", "name": "Plan"}, "versions": [
            {"asset_id": "a1v2", "current": true, "reason": "revised"}, {"label": "no id"}]}}),
    )
    .unwrap();
    let RowAnswer::Served { asset, versions } = answer else {
        panic!("a served row")
    };
    assert_eq!(asset.asset_id, "a1");
    assert_eq!(versions.len(), 1);
    assert_eq!(
        (
            versions[0].label.as_str(),
            versions[0].current,
            versions[0].reason.as_deref()
        ),
        ("a1v2", true, Some("revised"))
    );
    assert_eq!(
        answered(get, 200, json!({"data": {"asset": {"name": "no id"}}}))
            .unwrap_err()
            .code,
        INTEGRITY
    );
}

#[test]
fn only_a_400_validation_refusal_reads_as_a_brain_without_the_index() {
    let refusal = answered(
        list(IndexQuery::default()),
        400,
        json!({"error": {"code": "VALIDATION_FAILED", "message": "unknown field view"}}),
    )
    .unwrap_err();
    assert!(predates_index(&refusal));
    let other = answered(
        list(IndexQuery::default()),
        400,
        json!({"error": {"code": "INVALID_CURSOR", "message": "bad"}}),
    )
    .unwrap_err();
    assert!(!predates_index(&other));
    let moved = answered(
        list(IndexQuery::default()),
        409,
        json!({"error": {"code": "ASSETS_INDEX_MOVED", "message": "moved"}}),
    )
    .unwrap_err();
    assert_eq!(
        (moved.code.as_str(), moved.status),
        ("ASSETS_INDEX_MOVED", Some(409))
    );
}
