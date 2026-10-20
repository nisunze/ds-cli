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
            _ => panic!("a DS Grid call never waits"),
        };
    }
}

fn head(model: &str, updated: &str) -> Value {
    json!({"model_id": model, "display_name": model, "head_revision_id": "r1",
        "model_kind": "lv_network", "lifecycle": "promoted", "state": "active",
        "updated_at": updated, "deleted_at": "0001-01-01T00:00:00Z", "authority": "project"})
}

#[test]
fn a_listing_follows_every_page_and_reads_newest_first() {
    let (models, sent) = answered(
        GridModelsList::new("kigali".into(), false),
        vec![
            json!({"data": {"models": [head("a", "2026-10-01T00:00:00Z")], "has_more": true,
                "next_cursor": "c2"}}),
            json!({"data": {"models": [head("b", "2026-10-02T00:00:00Z")], "has_more": false}}),
        ],
    );
    let models = models.expect("the listing admits");
    assert_eq!(
        models
            .iter()
            .map(|m| m.model_id.as_str())
            .collect::<Vec<_>>(),
        ["b", "a"]
    );
    assert_eq!(models[0].deleted_at, None);
    assert_eq!(
        sent[0].body,
        Some(json!({"action": "list_models", "project_id": "kigali", "page_size": 500}))
    );
    assert_eq!(sent[1].body.as_ref().unwrap()["cursor"], "c2");

    let (refused, _) = answered(
        GridModelsList::new("kigali".into(), false),
        vec![json!({"data": {"models": [], "has_more": true}})],
    );
    assert_eq!(refused.unwrap_err().code, INVALID);
}

#[test]
fn a_resolved_version_names_the_identity_it_was_asked_for() {
    let version = json!({"model_id": "m", "revision_id": "r2", "version": 2,
        "model": {"digest": "a".repeat(64), "object": "ds-grid/objects/sha256/aa/x", "byte_length": 9},
        "created_by": "ana@example.rw", "created_at": "2026-10-02T00:00:00Z"});
    let call = || GridVersionResolve {
        project: "kigali".into(),
        model: "m".into(),
        revision: "r2".into(),
    };
    let (resolved, _) = answered(
        call(),
        vec![
            json!({"data": {"revision": version, "download_url": "https://storage.example/signed",
            "expires_at": "2026-10-02T01:00:00Z"}}),
        ],
    );
    let resolved = resolved.expect("the version admits");
    assert_eq!(resolved.version.actor.as_deref(), Some("ana@example.rw"));
    assert_eq!(resolved.download_url, "https://storage.example/signed");

    let mut other = version.clone();
    other["revision_id"] = "r3".into();
    let (refused, _) = answered(
        call(),
        vec![
            json!({"data": {"revision": other, "download_url": "https://storage.example/signed"}}),
        ],
    );
    assert_eq!(refused.unwrap_err().code, INVALID);
}

#[test]
fn an_upload_session_is_for_the_exact_bytes_or_none_when_stored() {
    let call = || GridUploadStart {
        project: "kigali".into(),
        model: "m".into(),
        digest: format!("sha256:{}", "A".repeat(64)),
        byte_length: 9,
        purpose: None,
        content_type: None,
    };
    let body = request(call()).body.unwrap();
    assert_eq!(body["upload"]["digest"], "a".repeat(64));
    let artifact = json!({"digest": "a".repeat(64), "object": "ds-grid/objects/sha256/aa/x", "byte_length": 9});
    let (stored, _) = answered(
        call(),
        vec![json!({"data": {"artifact": artifact, "already_exists": true, "max_chunk_size": 0}})],
    );
    let stored = stored.expect("a stored object admits");
    assert!(stored.already_stored && stored.upload_url.is_empty());
    let (refused, _) = answered(
        call(),
        vec![json!({"data": {"artifact": artifact, "already_exists": false}})],
    );
    assert_eq!(refused.unwrap_err().code, INVALID);
}

fn input() -> GridVersionInput {
    GridVersionInput {
        manifest_model_id: "manifest".into(),
        manifest_model_revision: 4,
        ds_project: "kigali".into(),
        model_id: "m".into(),
        model_kind: "lv_network".into(),
        revision_id: "r2".into(),
        expected_head_revision_id: "r1".into(),
        bump_version: None,
        display_name: Some("Feeder".into()),
        model: GridObjectRef {
            digest: format!("sha256:{}", "a".repeat(64)),
            object: "ds-grid/objects/sha256/aa/x".into(),
            byte_length: 9,
        },
        model_schema_version: "4".into(),
        engine_version: "1.2.0".into(),
        reason: Some("  ".into()),
        design_stage_id: None,
        detail_level_id: Some(" detailed ".into()),
        approval: None,
        validation: GridValidationSummary {
            state: "valid".into(),
            issue_count: 0,
            report_digest: None,
        },
        migration_source: None,
        composition_sources: Some(vec![GridCompositionSource {
            sequence: 1,
            source_kind: "project_model".into(),
            model_id: "src".into(),
            model_digest: "b".repeat(64),
            revision_id: Some("s1".into()),
            label: Some("Shown only".into()),
        }]),
    }
}

#[test]
fn a_version_goes_out_as_brain_declares_it() {
    let body = request(GridVersionCreate(input())).body.unwrap();
    assert_eq!(body["project_id"], "kigali");
    let version = &body["version"];
    assert_eq!(version["model"]["digest"], "a".repeat(64));
    assert_eq!(version["bump_version"], false);
    assert!(version.get("reason").is_none());
    assert_eq!(version["detail_level_id"], "detailed");
    assert_eq!(
        version["composition_sources"],
        json!([{"sequence": 1, "source_kind": "project_model", "model_id": "src",
            "model_digest": "b".repeat(64), "revision_id": "s1"}])
    );

    let mut invalid = input();
    invalid.validation.state = "issues".into();
    let Step::Refused(refused) = start(GridVersionCreate(invalid)) else {
        panic!("an invalid model is refused before any request")
    };
    assert_eq!(refused.code, INPUT_INVALID);
}

#[test]
fn a_retirement_needs_a_reason_and_names_its_head() {
    let call = |reason: &str| GridModelDelete {
        project: "kigali".into(),
        model: "m".into(),
        expected_head_revision_id: "r2".into(),
        expected_model_digest: "a".repeat(64),
        reason: reason.into(),
    };
    assert!(matches!(start(call(" ")), Step::Refused(_)));
    let (deleted, _) = answered(
        call("superseded"),
        vec![
            json!({"data": {"deletion": {"project_id": "kigali", "model_id": "m",
            "head_revision_id": "r2", "head_model_digest": "a".repeat(64),
            "deleted_at": "2026-10-02T00:00:00Z", "revisions_retained": true,
            "model_object_retained": true, "backup": null}}}),
        ],
    );
    assert!(deleted.expect("the deletion admits").revisions_retained);
}
