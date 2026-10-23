use super::*;
use serde_json::json;

fn record(id: &str, extra: Value) -> Value {
    let mut data = json!({
        "category": "request_for_information", "channel": "email", "direction": "outbound",
        "subject": "Questions on spans", "happened_at": "2026-09-20T09:00:00Z", "thread_id": "R1",
        "response_required": true, "response_owner_party_id": "p_tbea", "response_due_date": "2026-09-24",
        "external_party_ids": ["p_tbea"], "source_asset_id": "a_mail", "sensitivity": "internal",
        "resulting_task_ids": ["T0"],
    });
    for (key, value) in extra.as_object().expect("extra") {
        data[key] = value.clone();
    }
    json!({"id": id, "version": 3, "data": data,
           "computed": {"response_status": "outstanding", "days_until_due": 4, "days_overdue": 0}})
}

#[test]
fn a_record_view_carries_the_record_and_what_the_server_projected_beside_it() {
    let view = record_view("p", &json!({
        "item": record("R5", json!({})),
        "attachments": [
            {"asset_id": "a_mail", "name": "review.eml", "kind": "mail", "format": "eml", "bytes": 1805,
             "sensitivity": "internal", "folder": "correspondence", "bytes_held": true,
             "mail": {"parts": [{"path": "part-1-1.txt"}, {"path": "picture.png"}]}},
            {"asset_id": "a_drive", "name": "package", "kind": "pack", "format": "unknown", "bytes": 18033672,
             "sensitivity": "confidential", "folder": "correspondence", "bytes_held": false,
             "external": {"url": "https://drive.google.com/x", "provider": "google_drive"}},
            {"name": "no id"},
        ],
        "attachments_total": 2,
        "documents": [{"asset_id": "a_gtp", "name": "GTP.pdf", "kind": "doc", "format": "pdf", "bytes": 10,
                       "sensitivity": "internal", "folder": "", "document": {"number": "GTP-001", "revision_label": "B", "state": "issued"}}],
        "blocked_tasks": [{"task_id": "T2b", "title": "Replace spans", "set_by": "lead@ds.rw", "set_at": 1_789_689_600_000i64}],
        "resulting_tasks": [{"task_id": "T0", "title": "Address review"}],
        "thread_total": 4, "thread_truncated": false, "as_of": "2026-09-20",
    }))
    .expect("view");
    assert_eq!(view.record.row.id, "R5");
    assert_eq!(
        view.record.row.response_status.as_deref(),
        Some("outstanding")
    );
    assert_eq!(
        view.record.row.response_owner_party_id.as_deref(),
        Some("p_tbea")
    );
    assert_eq!(view.record.days_until_due, 4);
    assert_eq!(view.record.source_asset_id.as_deref(), Some("a_mail"));
    assert_eq!(view.record.resulting_task_ids, vec!["T0"]);
    assert_eq!(
        view.attachments.len(),
        2,
        "a row without an id is not an attachment"
    );
    assert_eq!(view.attachments_total, 2);
    assert_eq!(
        view.attachments[0].mail_parts,
        vec!["part-1-1.txt", "picture.png"]
    );
    assert!(!view.attachments[1].bytes_held);
    assert_eq!(
        view.attachments[1].external_url.as_deref(),
        Some("https://drive.google.com/x")
    );
    assert_eq!(
        view.documents[0].document.as_ref().unwrap()["number"],
        "GTP-001"
    );
    assert_eq!(
        view.blocked_tasks[0].set_at.as_deref(),
        Some("2026-09-18T00:00:00.000Z")
    );
    assert_eq!(view.resulting_tasks[0].task_id, "T0");
    assert_eq!(view.thread_total, 4);
    assert!(
        record_view("p", &json!({"item": {"data": {}}})).is_none(),
        "an idless item is no view"
    );
}

#[test]
fn a_thread_lists_its_readable_records_and_counts_them_all() {
    let thread = thread_view(
        "p",
        &json!({
            "thread_id": "R1", "total": 3, "truncated": false, "as_of": "2026-09-20",
            "records": [
                {"item": record("R1", json!({"direction": "inbound"})), "attachments": []},
                {"item": record("R5", json!({}))},
            ],
        }),
    );
    assert_eq!(thread.thread_id, "R1");
    assert_eq!(thread.records.len(), 2);
    assert_eq!(
        thread.total, 3,
        "a record the caller may not read still counts"
    );
    assert_eq!(thread.records[1].record.row.id, "R5");
}

#[test]
fn a_server_page_is_restated_in_the_clis_zero_based_bounds() {
    let page = record_page(
        "p",
        &json!({
            "records": [record("R5", json!({})), record("R2", json!({}))],
            "total": 7, "page": 2, "limit": 2, "scanned": 7, "truncated": false, "as_of": "2026-09-20",
        }),
    );
    assert_eq!(page.bounds.total, 7);
    assert_eq!(
        page.bounds.page, 1,
        "the server's page 2 is the CLI's page 1"
    );
    assert_eq!(page.bounds.page_count, 4);
    assert_eq!((page.bounds.from, page.bounds.to), (3, 4));
    assert_eq!(page.records[0].id, "R5");
    assert_eq!(page.records[0].party_ids, vec!["p_tbea"]);
    let empty = record_page(
        "p",
        &json!({"records": [], "total": 0, "page": 1, "limit": 50}),
    );
    assert_eq!(
        (empty.bounds.from, empty.bounds.to, empty.bounds.page_count),
        (0, 0, 1)
    );
}

#[test]
fn parties_read_their_envelope_and_page_the_same_way() {
    let page = party_page(
        "p",
        &json!({
            "parties": [
                {"id": "p_tbea", "version": 1, "data": {"kind": "organisation", "name": "TBEA Co., Ltd", "role": "contractor", "emails": ["a@example.com"]},
                 "metadata": {"updated_at": 1_789_689_600_000i64}},
                {"id": "p_zhang", "version": 2, "data": {"kind": "person", "name": "Zhang", "organisation_party_id": "p_tbea", "archived": true}},
                {"data": {"name": "no id"}},
            ],
            "total": 2, "page": 1, "limit": 50, "scanned": 2, "truncated": false,
        }),
    );
    assert_eq!(page.parties.len(), 2);
    assert_eq!(page.parties[0].role.as_deref(), Some("contractor"));
    assert_eq!(
        page.parties[0].updated_at.as_deref(),
        Some("2026-09-18T00:00:00.000Z")
    );
    assert!(page.parties[1].archived);
    assert_eq!(
        page.parties[1].organisation_party_id.as_deref(),
        Some("p_tbea")
    );
    assert_eq!(page.bounds.total, 2);
}

#[test]
fn a_timeline_keeps_the_servers_order_and_its_bound() {
    let folded = timeline(&json!({
        "task_id": "T2b",
        "entries": [
            {"at": "2026-09-20T09:00:00Z", "kind": "blocker_set", "entity_id": "R5", "summary": "blocked on R5", "actor": "lead@ds.rw", "detail": {"record_id": "R5"}},
            {"at": 1_789_689_600_000i64, "kind": "created", "summary": "created", "detail": {}},
            {"summary": "no kind"},
        ],
        "total": 2, "truncated": false,
        "sources": {"records": 1, "blockers": 1},
    }))
    .expect("timeline");
    assert_eq!(folded.entries.len(), 2);
    assert_eq!(folded.entries[0].kind, "blocker_set");
    assert_eq!(
        folded.entries[0].detail.as_ref().unwrap()["record_id"],
        "R5"
    );
    assert_eq!(folded.entries[1].at, "2026-09-18T00:00:00.000Z");
    assert!(
        folded.entries[1].detail.is_none(),
        "an empty detail is omitted"
    );
    assert_eq!(folded.sources.as_ref().unwrap()["blockers"], 1);
    assert!(timeline(&Value::Null).is_none());
}

/// A record's own uploads read back with their state: a ready file and an
/// interrupted one are both listed, each with the asset id that opens it.
#[test]
fn a_record_view_lists_the_records_own_uploads_with_their_state() {
    let attachment = |id: &str, version: i64, state: &str| {
        json!({"id": id, "version": version, "is_deleted": false,
            "metadata": {"created_by": "me@example.com", "created_at": 1},
            "data": {"linked_entity_type": "record", "linked_entity_id": "R5",
                "original_name": "EPCGIS_1.RAR", "content_type": "application/vnd.rar",
                "size_bytes": 101093342, "state": state}})
    };
    let view = record_view(
        "p",
        &json!({"item": record("R5", json!({})),
            "pm_attachments": [attachment("a-ready", 3, "ready"), attachment("a-cut", 1, "uploading"), {"data": {}}]}),
    )
    .expect("view");
    assert_eq!(
        view.pm_attachments.len(),
        2,
        "a row without an id is not listed"
    );
    let ready = &view.pm_attachments[0];
    assert_eq!(
        (
            ready.id.as_str(),
            ready.version,
            ready.state.as_str(),
            ready.size_bytes
        ),
        ("a-ready", 3, "ready", 101093342)
    );
    assert_eq!(ready.asset_id, "sys:pm_attachment:a-ready");
    assert_eq!(ready.created_by.as_deref(), Some("me@example.com"));
    assert_eq!(view.pm_attachments[1].state, "uploading");
    let wire = serde_json::to_value(&view).unwrap();
    assert_eq!(
        wire["pmAttachments"][0]["contentType"],
        "application/vnd.rar"
    );
    let without = record_view("p", &json!({"item": record("R5", json!({}))})).expect("view");
    assert!(without.pm_attachments.is_empty());
}
