use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step, PROJECT_INVALID};
use serde_json::json;

fn asset(id: &str) -> Value {
    json!({"schema": "ds.assets.asset/v1", "asset_id": id, "project_id": "it_rwanda", "folder": "Reports",
        "name": format!("{id}.pdf"), "kind": "doc", "format": "pdf", "bytes": 4, "digest": format!("sha256:{}", "a".repeat(64)),
        "status": "durable", "sensitivity": "internal", "dated": {"created_at": "2026-10-19T08:00:00Z"},
        "owner": {"kind": "person", "id": "one@example.com"}, "source": {"kind": "upload"},
        "classified_by": "user", "container": null, "links": [], "bytes_held": true,
        "created_by": "one@example.com", "updated_at": "2026-10-19T08:00:00Z", "updated_by": "one@example.com", "version": 2})
}

fn request<C: Exchange>(call: C) -> Request {
    match start(call) {
        Step::Fetch { request, .. } => request,
        Step::Refused(refusal) => panic!("refused: {refusal:?}"),
        _ => panic!("expected a fetch"),
    }
}

fn answered<C: Exchange>(call: C, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else { panic!("expected a fetch") };
    let observed = Observed::Response { status: 200, content_type: Some("application/json".into()), retry_after: None };
    match resume(continuation, observed, body.to_string().as_bytes(), &Facts::default()) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

#[test]
fn a_page_asks_only_for_what_was_named_and_drops_unreadable_rows() {
    let query = ListQuery { kind: Some("doc".into()), cursor: Some("".into()), limit: Some(50), ..ListQuery::default() };
    let sent = request(List { project: "it_rwanda".into(), query: query.clone() });
    assert_eq!(sent.body, Some(json!({"action": "list", "ds_project": "it_rwanda", "kind": "doc", "limit": 50})));
    assert_eq!(sent.timeout_ms, 180_000);
    let page = answered(List { project: "it_rwanda".into(), query }, json!({"success": true, "data": {
        "assets": [asset("a"), {"asset_id": "broken"}, asset("b")], "has_more": true, "limit": 50,
        "next_cursor": "c2", "truncated": false}})).unwrap();
    assert_eq!(page.assets.iter().map(|row| row["asset_id"].as_str().unwrap()).collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!((page.dropped, page.has_more, page.next_cursor.as_deref(), page.scanned), (1, true, Some("c2"), 3));
    assert_eq!(page.assets[0], asset("a"), "a row travels as served");
}

#[test]
fn the_tree_keeps_only_declared_folders() {
    let folders = answered(Tree { project: "it_rwanda".into() }, json!({"data": {"folders": [
        {"schema": "ds.assets.folder/v1", "folder_id": "f1", "project_id": "it_rwanda", "kind": "user", "path": "Reports",
            "name": "Reports", "parent": "", "counts": {"assets": 2, "folders": 0}},
        {"folder_id": "f2"}]}})).unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0]["folder_id"], "f1");
}

#[test]
fn one_row_and_one_signed_read_are_admitted_whole() {
    assert_eq!(request(Get { project: "it_rwanda".into(), asset_id: " a ".into() }).body,
        Some(json!({"action": "get", "ds_project": "it_rwanda", "asset_id": "a"})));
    assert_eq!(answered(Get { project: "it_rwanda".into(), asset_id: "a".into() }, json!({"data": asset("a")})).unwrap(), asset("a"));
    assert_eq!(answered(Get { project: "it_rwanda".into(), asset_id: "a".into() }, json!({"data": {"asset_id": "a"}})).unwrap_err().code, INVALID);
    let read = SignedRead { project: "it_rwanda".into(), asset_id: "a".into(), surface: Some("ui".into()) };
    assert_eq!(request(read.clone()).body, Some(json!({"action": "download", "ds_project": "it_rwanda", "asset_id": "a", "surface": "ui"})));
    let download = answered(read.clone(), json!({"data": {"asset": asset("a"),
        "download_url": "https://storage.googleapis.com/b/a.pdf?sig=1", "expires_at": "2026-10-20T09:00:00Z"}})).unwrap();
    assert_eq!(download.download_url, "https://storage.googleapis.com/b/a.pdf?sig=1");
    assert!(answered(read, json!({"data": {"asset": asset("a"), "download_url": "javascript:x", "expires_at": "t"}})).is_err());
    assert!(matches!(start(SignedRead { project: "it_rwanda".into(), asset_id: "a".into(), surface: Some("web".into()) }), Step::Refused(_)));
    assert!(matches!(start(Get { project: "".into(), asset_id: "a".into() }), Step::Refused(refusal) if refusal.code == PROJECT_INVALID));
}

#[test]
fn a_classification_is_fenced_by_its_version() {
    let mut classify = Map::new();
    classify.insert("status".into(), json!("durable"));
    classify.insert("reason".into(), Value::Null);
    assert!(matches!(start(Classify { project: "it_rwanda".into(), asset_id: "a".into(), classify: classify.clone() }), Step::Refused(_)));
    classify.insert("expected_version".into(), json!(2));
    let call = Classify { project: "it_rwanda".into(), asset_id: "a".into(), classify };
    assert_eq!(request(call.clone()).body, Some(json!({"action": "classify", "ds_project": "it_rwanda", "asset_id": "a",
        "classify": {"status": "durable", "expected_version": 2}})));
    let receipt = answered(call, json!({"data": {"asset": asset("a"), "audit_id": "audit-1", "changed": ["status"]}})).unwrap();
    assert_eq!((receipt.audit_id.as_str(), receipt.changed.as_slice()), ("audit-1", &["status".to_owned()][..]));
}

#[test]
fn a_link_travels_in_its_one_shape() {
    let attach = Relink { project: "it_rwanda".into(), asset_id: "a".into(),
        link: json!({"kind": "pm_task", "id": "t1", "object_type": "ignored"}), detach: false };
    assert_eq!(request(attach).body, Some(json!({"action": "attach", "ds_project": "it_rwanda", "asset_id": "a",
        "link": {"kind": "pm_task", "id": "t1"}})));
    let detach = Relink { project: "it_rwanda".into(), asset_id: "a".into(),
        link: json!({"kind": "ds_object", "object_type": "transformer", "entity_id": "TX-1"}), detach: true };
    assert_eq!(request(detach.clone()).body.unwrap()["action"], "detach");
    assert_eq!(answered(detach, json!({"data": asset("a")})).unwrap(), asset("a"));
    let unknown = Relink { project: "it_rwanda".into(), asset_id: "a".into(), link: json!({"kind": "email"}), detach: false };
    assert!(matches!(start(unknown), Step::Refused(refusal) if refusal.code == INPUT_INVALID));
}
