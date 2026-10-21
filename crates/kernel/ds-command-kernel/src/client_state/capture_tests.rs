//! Survey capture on the test host (`testing`), its store the real durable
//! store in memory.

use std::cell::RefCell;
use std::rc::Rc;

use reactive_graph::traits::GetUntracked;
use serde_json::{json, Value};

use crate::client_state::testing::{scenario, settle, state, Host};
use crate::client_state::{ClientState, Scope};
use crate::durable_store::Request;
use crate::gateway_exchange::Refusal;
use crate::survey_capture::{Context, Upload, FILE};

fn form() -> Value {
    json!({
        "slug": "poles", "version": 4,
        "fields": [
            {"key": "height", "type": "number", "label": "Height", "required": true, "validation": {"min": 1}},
            {"key": "photo", "type": "photo"}
        ],
        "entry_document_schema": {"schema_version": "v2",
            "top_level_keys": [{"key": "data"}, {"key": "metadata"}, {"key": "geometry"}]}
    })
}

fn context() -> Context {
    Context {
        project: "kigali".into(),
        form: form(),
        ..Context::default()
    }
}

fn sign_in(state: &ClientState<Host>, host: &Host, principal: &str) {
    host.sign_in(Some(principal));
    state.set_scope(Scope::new(Some(principal), Some("kigali")));
}

fn shown(state: &ClientState<Host>) -> Value {
    state.capture.view.value.get_untracked()
}

fn call(host: &Host, request: Value) -> Value {
    host.call(serde_json::from_value::<Request>(request).unwrap())
}

/// The account's held capabilities for the project, as a capabilities read keeps them.
fn grant(host: &Host, create: bool) {
    call(host, json!({"store": "retained_reads", "operation": {"action": "write", "kind": "account_capabilities",
        "key": "project:kigali", "now": 1,
        "value": {"user": {"email": "surveyor@example.test"}, "capabilities": {"entries.create": create}}}}));
}

fn queued(host: &Host) -> Vec<Value> {
    call(host, json!({"store": "outbox", "operation": {"action": "list", "queue": "survey"}}))["rows"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn draft(host: &Host) -> Value {
    call(host, json!({"store": "work_drafts", "operation": {"action": "read", "kind": "survey_entry",
        "project": "kigali", "key": "poles"}}))["held"]
        .clone()
}

scenario! {
    /// A form opens on its defaults, keeps its draft as it is filled, and
    /// another account sees no form.
    async fn a_form_keeps_its_draft_and_opens_only_for_its_account() {
        let (state, host) = state();
        sign_in(&state, &host, "u1");
        state.capture_open(context()).await.unwrap();
        assert_eq!(shown(&state)["open"], true);
        state.capture_set("height", json!(9));
        settle().await;
        assert_eq!(draft(&host)["value"]["values"], json!({"height": 9}));

        state.capture_close();
        state.capture_open(context()).await.unwrap();
        assert_eq!(shown(&state)["values"]["height"], 9, "the held draft is restored");
        assert_eq!(shown(&state)["restored_draft"], true);

        sign_in(&state, &host, "u2");
        assert_eq!(shown(&state), json!({"open": false}), "another account never sees this form");
    }
}

scenario! {
    /// A submission keeps its files through the host, queues the create and
    /// schedules the files after it, and drops the draft.
    async fn a_submission_queues_its_create_then_its_photo() {
        let (state, host) = state();
        sign_in(&state, &host, "u1");
        grant(&host, true);
        state.capture_open(context()).await.unwrap();
        state.capture_locate(json!({"longitude": 30.06, "latitude": -1.95, "accuracy": 4.0}), "gps");
        state.capture_set("height", json!(9));
        state.capture_set("photo", json!({FILE: "t1", "name": "pole.jpg"}));
        settle().await;
        let kept = Rc::new(RefCell::new(Vec::new()));
        let (keeper, holder) = (Rc::clone(&kept), host.clone());
        let submitted = state
            .capture_submit(move |upload: Upload| {
                keeper.borrow_mut().push(upload.clone());
                let admitted = holder.admit(&upload.path);
                async move { admitted }
            })
            .await
            .unwrap();
        let doc = submitted["doc_id"].as_str().unwrap().to_owned();
        assert!(doc.starts_with("e_") && doc.len() == 34);
        assert_eq!(kept.borrow().len(), 1);
        let rows = queued(&host);
        let operations: Vec<&Value> = rows.iter().map(|row| &row["payload"]["operation"]).collect();
        assert_eq!(operations, vec!["create", "media"], "the photo leaves after its entry's create");
        assert_eq!(rows[0]["payload"]["wire"]["data"]["photo"], json!(kept.borrow()[0].path));
        assert_eq!(rows[0]["payload"]["uid"], "u1");
        assert_eq!(shown(&state)["submitted"], true);
        assert!(draft(&host).is_null(), "a submitted form's draft is gone");
    }
}

scenario! {
    /// Field errors show beside their fields, a missing grant refuses, and a
    /// file the host could not keep queues nothing.
    async fn a_refused_submission_queues_nothing() {
        let (state, host) = state();
        sign_in(&state, &host, "u1");
        grant(&host, true);
        state.capture_open(context()).await.unwrap();
        state.capture_set("height", json!(0));
        let refused = state.capture_submit(|_: Upload| async { Ok(()) }).await.unwrap_err();
        assert_eq!(refused.code, super::INVALID);
        assert_eq!(shown(&state)["errors"]["height"], "Minimum is 1");

        state.capture_set("height", json!(9));
        state.capture_set("photo", json!({FILE: "t1", "name": "pole.jpg"}));
        let lost = state
            .capture_submit(|_: Upload| async { Err(Refusal::new("file_gone", "The photo could not be read")) })
            .await
            .unwrap_err();
        assert_eq!(lost.code, "file_gone");
        assert_eq!(shown(&state)["error"], "The photo could not be read");
        assert_eq!(shown(&state)["submitting"], false);
        assert!(queued(&host).is_empty());

        grant(&host, false);
        let ungranted = state.capture_submit(|_: Upload| async { Ok(()) }).await.unwrap_err();
        assert_eq!(ungranted.code, super::UNAVAILABLE);
        assert!(queued(&host).is_empty());
    }
}

fn grant_edits(host: &Host) {
    call(host, json!({"store": "retained_reads", "operation": {"action": "write", "kind": "account_capabilities",
        "key": "project:kigali", "now": 1,
        "value": {"user": {"email": "surveyor@example.test"},
            "capabilities": {"entries.edit_all": true, "entries.delete": true}}}}));
}

fn row(doc: &str, precedent: Option<&str>) -> Value {
    json!({"type": "Feature", "properties": {"doc_id": doc, "form_slug": "poles",
        "metadata": {"client_version": 3, "created_by": "surveyor@example.test"}, "data": {"height": 9},
        "connectivity": {"precedent": precedent}}})
}

scenario! {
    /// A correction is planned against the row the window shows and queued
    /// as a step of its entry; one the grants refuse queues nothing.
    async fn a_correction_queues_against_the_shown_row() {
        let (state, host) = state();
        sign_in(&state, &host, "u1");
        let payload = json!({"project_id": "kigali", "form_id": "poles", "doc_id": "e_1", "operation": "update",
            "updated_data": {"height": 10}});
        assert!(state.survey_mutate(payload.clone(), row("e_1", None)).await.is_err(), "no grant, no correction");
        assert!(queued(&host).is_empty());
        grant_edits(&host);
        let feature = state.survey_mutate(payload, row("e_1", None)).await.unwrap();
        assert!(!feature.is_null());
        let rows = queued(&host);
        assert_eq!((rows[0]["payload"]["operation"].clone(), rows[0]["payload"]["wire"]["expected_version"].clone()),
            (json!("update"), json!(3)));
        assert_eq!(rows[0]["payload"]["uid"], "u1");
    }
}

scenario! {
    /// A node's deletion queues its own and each cascaded edge's deletion,
    /// each with the tombstone the map shows.
    async fn a_deletion_queues_its_cascade() {
        let (state, host) = state();
        sign_in(&state, &host, "u1");
        grant_edits(&host);
        let edge = json!({"type": "Feature", "properties": {"doc_id": "e_2", "form_slug": "spans",
            "metadata": {"client_version": 1}, "connectivity": {"precedent": "e_1"}}});
        let request = json!({"project": "kigali", "form": "poles", "doc": "e_1", "prior": row("e_1", None),
            "candidates": [{"form_slug": "spans", "feature": {"id": "e_2", "geometry": {"type": "LineString"},
                "properties": {"doc_id": "e_2", "connectivity": {"precedent": "e_1"}, "metadata": {"client_version": 1}}}}],
            "priors": {"poles/e_1": row("e_1", None), "spans/e_2": edge}});
        let deleted = state.survey_delete_cascade(request).await.unwrap();
        let rows = queued(&host);
        assert_eq!(rows.len(), 2, "the node and its edge");
        assert!(rows.iter().all(|row| row["payload"]["operation"] == "delete"));
        assert_eq!(deleted["tombstones"].as_array().unwrap().len(), rows.len());
        assert_eq!(rows[0]["payload"]["doc"], "e_1", "the node leaves first");
    }
}
