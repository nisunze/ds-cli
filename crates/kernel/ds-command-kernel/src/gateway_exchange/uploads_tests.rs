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

#[test]
fn a_processing_upload_lands_only_in_the_domain_it_asked_for() {
    let call = |domain: &str| ProcessingStart {
        project: "kigali".into(),
        domain: domain.into(),
        file_name: "network.gpkg".into(),
        size: 4,
        content_type: "application/geopackage+sqlite3".into(),
    };
    assert_eq!(request(call("mv_extrapolation")).path, "/mv");
    let lv = request(call("design_layer_upload"));
    assert_eq!(lv.path, "/process");
    assert_eq!(lv.body.unwrap()["eds_project_id"], "kigali");

    let body = |domain: &str| {
        json!({"data": {"session_uri": "https://storage.example/s",
            "blob_path": "project/uploads/v2/r/network.gpkg", "upload_domain": domain}})
    };
    let issued = answered(call("design_layer_upload"), body("design_layer_upload"))
        .expect("the named domain admits");
    assert_eq!(issued.blob_path, "project/uploads/v2/r/network.gpkg");
    let refused = answered(call("design_layer_upload"), body("lv_process"))
        .expect_err("another domain is refused");
    assert_eq!(refused.code, INVALID);
}

#[test]
fn a_session_must_name_where_the_bytes_go() {
    let call = ProcessingStart {
        project: "kigali".into(),
        domain: "lv_process".into(),
        file_name: "x.gpkg".into(),
        size: 1,
        content_type: "application/geopackage+sqlite3".into(),
    };
    let refused = answered(
        call,
        json!({"data": {"session_uri": "", "upload_domain": "lv_process"}}),
    )
    .expect_err("an incomplete session is refused");
    assert_eq!(refused.code, INVALID);
}
