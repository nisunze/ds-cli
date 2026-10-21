use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn request<C: Exchange>(call: C) -> Request {
    match start(call) {
        Step::Fetch { request, .. } => request,
        _ => panic!("expected a fetch"),
    }
}

/// Run a call through as many pages as it asks for.
fn answered<C: Exchange>(
    call: C,
    bodies: Vec<Value>,
) -> (Result<C::Output, Refusal>, Vec<Request>) {
    let mut sent = Vec::new();
    let mut bodies = bodies.into_iter();
    let mut step = start(call);
    loop {
        step = match step {
            Step::Done(value) => return (Ok(value), sent),
            Step::Refused(refusal) => return (Err(refusal), sent),
            Step::Fetch {
                request,
                continuation,
            } => {
                sent.push(request);
                let body = bodies
                    .next()
                    .expect("the call asked for an unscripted page");
                let observed = Observed::Response {
                    status: 200,
                    content_type: Some("application/json".into()),
                    retry_after: None,
                };
                resume(
                    continuation,
                    observed,
                    body.to_string().as_bytes(),
                    &Facts::default(),
                )
            }
            _ => panic!("a notes call never waits"),
        };
    }
}

fn wire_note() -> Value {
    json!({"id": "n1", "version": 3, "title": "Pole 12", "folder": "",
        "content": {"format": "markdown", "markdown": "Lean **east**"},
        "checklist": [{"id": "c1", "text": "Photograph", "checked": true}],
        "tags": null,
        "links": [{"kind": "pm_task", "project_id": "kigali", "entity_id": "t1",
            "attached_by": "ana@example.rw", "attached_at": "2026-10-01T08:00:00Z"},
            {"kind": "project", "project_id": ""}],
        "geometry": {"type": "Point", "coordinates": [30.06, -1.95]},
        "canvas": {"x": 48, "y": 48, "width": 300, "height": 230, "z": 1, "color": "mint"},
        "pinned": true, "archived": false, "trashed": false,
        "created_at": "2026-10-01T08:00:00Z", "created_by": "ana@example.rw",
        "updated_at": "2026-10-02T08:00:00Z", "updated_by": "ana@example.rw"})
}

#[test]
fn a_note_reads_as_the_page_shows_it() {
    let (opened, _) = answered(
        NotesGet { id: "n1".into() },
        vec![json!({"data": {"note": wire_note(), "attachments": [
            {"id": "a1", "note_id": "n1", "state": "ready", "original_name": "pole.jpg",
             "content_type": "image/jpeg", "size_bytes": 3, "created_at": "2026-10-01T08:00:00Z",
             "created_by": "ana@example.rw"}]}})],
    );
    let opened = opened.expect("the note admits");
    let note = serde_json::to_value(&opened.note).unwrap();
    assert_eq!(note["markdown"], "Lean **east**");
    assert_eq!(note["createdBy"], "ana@example.rw");
    assert!(note.get("folder").is_none());
    assert_eq!(note["tags"], json!([]));
    assert_eq!(
        note["links"],
        json!([{"kind": "pm_task", "projectId": "kigali", "entityId": "t1",
        "attachedBy": "ana@example.rw", "attachedAt": "2026-10-01T08:00:00Z"}])
    );
    assert_eq!(note["geometry"]["type"], "Point");
    assert_eq!(note["canvas"]["color"], "mint");
    let attachment = serde_json::to_value(&opened.attachments[0]).unwrap();
    assert_eq!(attachment["noteId"], "n1");
    assert_eq!(attachment["originalName"], "pole.jpg");
}

#[test]
fn a_list_follows_brains_pages_and_reads_metadata_with_neutral_content() {
    let page = |id: &str, next: Option<&str>| {
        json!({"data": {"notes": [{"id": id, "version": 1, "title": id, "tags": ["site"],
            "pinned": false, "archived": false, "trashed": false,
            "updated_at": "2026-10-02T08:00:00Z"}], "next_cursor": next}})
    };
    let (listed, sent) = answered(
        NotesList::new(true, false),
        vec![page("n1", Some("c2")), page("n2", None)],
    );
    let listed = listed.expect("the list admits");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[1].markdown, "");
    assert_eq!(listed[1].canvas, NoteCanvas::default());
    assert_eq!(
        sent[0].body,
        Some(json!({"action": "list", "include_archived": true, "include_trashed": false}))
    );
    assert_eq!(sent[1].body.as_ref().unwrap()["cursor"], "c2");
}

#[test]
fn a_draft_goes_out_in_the_shape_brain_stores() {
    let draft = json!({"title": "Pole 12", "folder": "Sites / Kigali", "markdown": "Lean",
        "checklist": [{"id": "c1", "text": "Photograph", "checked": false}], "tags": ["site"],
        "links": [{"kind": "ds_object", "projectId": "kigali", "objectType": "mv_model",
            "objectRevision": "r1", "modelVersion": 2, "modelDigest": "d", "attachedBy": "x"}],
        "canvas": {"x": 1, "y": 2, "width": 300, "height": 230, "z": 1, "color": "sand"},
        "pinned": false, "archived": false});
    let sent = request(NotesChange::Update {
        id: "n1".into(),
        expected_version: 3,
        draft,
    });
    let body = sent.body.unwrap();
    assert_eq!(body["action"], "update");
    assert_eq!(body["expected_version"], 3);
    assert_eq!(
        body["note"]["content"],
        json!({"format": "markdown", "markdown": "Lean"})
    );
    assert_eq!(body["note"]["folder"], "Sites / Kigali");
    assert_eq!(
        body["note"]["links"],
        json!([{"kind": "ds_object", "project_id": "kigali", "object_type": "mv_model",
            "object_revision": "r1", "model_version": 2, "model_digest": "d"}])
    );
    assert!(body["note"].get("geometry").is_none());

    let trash = request(NotesChange::Trash {
        id: "n1".into(),
        expected_version: 4,
        trashed: false,
    });
    assert_eq!(
        trash.body,
        Some(json!({"action": "restore", "id": "n1", "expected_version": 4}))
    );
}

#[test]
fn only_a_drawable_geometry_reaches_the_page() {
    assert!(geometry(Some(json!({"type": "Point", "coordinates": [1.0, 2.0]}))).is_some());
    assert!(geometry(Some(
        json!({"type": "LineString", "coordinates": [[1.0, 2.0]]})
    ))
    .is_none());
    let ring = json!([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]);
    assert!(geometry(Some(json!({"type": "Polygon", "coordinates": [ring]}))).is_some());
    assert!(geometry(Some(json!({"type": "MultiPoint", "coordinates": []}))).is_none());
}
