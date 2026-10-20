use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn request<C: Exchange>(call: C) -> Request {
    match start(call) {
        Step::Fetch { request, .. } => request,
        _ => panic!("expected a fetch"),
    }
}

fn answered<C: Exchange>(call: C, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else {
        panic!("expected a fetch")
    };
    let observed = Observed::Response {
        status: 200,
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

fn page() -> ArtifactDeclaration {
    ArtifactDeclaration {
        project_id: "kigali".into(),
        scope: "mv".into(),
        transformer: "mv_data".into(),
        file_name: "mv.a3.pdf".into(),
        size_bytes: 300,
        content_type: "application/pdf".into(),
        sha256: "a".repeat(64),
        map_family: "mv-map".into(),
        layout_name: "mv_data".into(),
        paper_size: "A3".into(),
        orientation: "landscape".into(),
        page_role: "sheet".into(),
        source_receipt_sha256: None,
    }
}

#[test]
fn a_page_is_declared_once_for_its_session_and_its_attachment() {
    let start = request(ArtifactStart(page()));
    assert_eq!(start.path, PATH);
    let body = start.body.unwrap();
    assert_eq!(body["action"], "upload_start");
    assert_eq!(body["sha256"], "a".repeat(64));
    assert!(body.get("source_receipt_sha256").is_none());

    let attach = request(ArtifactAttach {
        declaration: page(),
        blob_path: "prints/mv.pdf".into(),
    });
    assert_eq!(attach.timeout_ms, ATTACH_TIMEOUT_MS);
    let body = attach.body.unwrap();
    assert_eq!(
        (body["action"].as_str(), body["blob_path"].as_str()),
        (Some("attach"), Some("prints/mv.pdf"))
    );
}

#[test]
fn an_attachment_receipt_must_name_the_declared_bytes() {
    let receipt = |sha256: &str| {
        json!({"data": {"project_id": "kigali", "scope": "mv", "transformer": "mv_data",
            "file_name": "mv.a3.pdf", "sha256": sha256, "gcs_path": "gs://b/prints/mv.pdf",
            "map_family": "mv-map"}})
    };
    let attach = || ArtifactAttach {
        declaration: page(),
        blob_path: "prints/mv.pdf".into(),
    };
    let attached = answered(attach(), receipt(&"a".repeat(64))).expect("the declared bytes attach");
    assert_eq!(attached.gcs_path, "gs://b/prints/mv.pdf");
    let refused =
        answered(attach(), receipt(&"b".repeat(64))).expect_err("other bytes are refused");
    assert_eq!(refused.code, INVALID);
}

#[test]
fn a_removal_receipt_must_name_the_exact_reference_and_keep_its_bytes() {
    let remove = || ArtifactRemove {
        project_id: "kigali".into(),
        scope: "mv".into(),
        transformer: "mv_data".into(),
        file_name: "mv.a3.pdf".into(),
        gcs_path: "gs://b/prints/mv.pdf".into(),
        sha256: "a".repeat(64),
    };
    let body = request(remove()).body.unwrap();
    assert_eq!(body["blob_path"], "gs://b/prints/mv.pdf");
    let receipt = json!({"project_id": "kigali", "scope": "mv", "transformer": "mv_data",
        "file_name": "mv.a3.pdf", "gcs_path": "gs://b/prints/mv.pdf", "sha256": "a".repeat(64),
        "removed": 1, "bytes_deleted": false});
    answered(remove(), json!({"data": receipt.clone()})).expect("the exact receipt admits");
    let mut other = receipt;
    other["project_id"] = "other".into();
    let refused =
        answered(remove(), json!({"data": other})).expect_err("another project is refused");
    assert_eq!(refused.code, INVALID);
}
