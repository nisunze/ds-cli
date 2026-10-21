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

fn geodata(project: Option<&str>) -> GeodataStart {
    GeodataStart {
        project: project.map(str::to_owned),
        file_name: "poles.gpkg".into(),
        size: 4,
        content_type: "application/geopackage+sqlite3".into(),
    }
}

#[test]
fn geodata_goes_to_its_owner() {
    let own = request(geodata(None));
    assert_eq!(own.path, "/api/v1/user_data");
    assert_eq!(
        own.body,
        Some(
            json!({"action": "upload_start", "file_name": "poles.gpkg", "size": 4,
            "content_type": "application/geopackage+sqlite3"})
        )
    );
    let project = request(geodata(Some("kigali")));
    assert_eq!(project.path, "/api/v1/project_data");
    assert_eq!(project.body.unwrap()["project_id"], "kigali");

    let register = request(GeodataRegister {
        project: None,
        blob_path: "user_uploads/a/poles.gpkg".into(),
        file_name: "poles.gpkg".into(),
    });
    assert_eq!(
        register.body,
        Some(
            json!({"action": "upload", "blob_path": "user_uploads/a/poles.gpkg",
            "file_name": "poles.gpkg"})
        )
    );
}

#[test]
fn a_session_must_name_where_the_bytes_go() {
    let issued = answered(
        geodata(None),
        json!({"data": {"session_uri": "https://storage.example/s", "blob_path": "user_uploads/a/x"}}),
    )
    .expect("a complete session admits");
    assert_eq!(issued.blob_path, "user_uploads/a/x");
    let refused = answered(geodata(None), json!({"data": {"session_uri": ""}}))
        .expect_err("an incomplete session is refused");
    assert_eq!(refused.code, crate::gateway_exchange::uploads::INVALID);
}

#[test]
fn registering_answers_what_brain_started() {
    let registered = answered(
        GeodataRegister {
            project: Some("kigali".into()),
            blob_path: "project_data/kigali/x/x.gpkg".into(),
            file_name: "x.gpkg".into(),
        },
        json!({"data": {"upload_id": "x", "tile_status": "queued"}}),
    )
    .expect("registration admits");
    assert_eq!(registered.upload_id.as_deref(), Some("x"));
    assert_eq!(registered.tile_status.as_deref(), Some("queued"));
}

#[test]
fn a_list_reads_what_brain_keeps_and_passes_the_rest_through() {
    let listed = answered(
        GeodataList { project: None },
        json!({"data": {"uploads": [{"upload_id": "u1", "file_name": "poles.gpkg",
            "user_email": "ana@example.rw", "tile_status": "ready", "layers": {"poles": {}},
            "pmtiles_url": "pmtiles://https://tiles.example/u1.pmtiles"}]}}),
    )
    .expect("the list admits");
    assert_eq!(listed[0].tile_status.as_deref(), Some("ready"));
    assert_eq!(
        listed[0].rest["pmtiles_url"],
        "pmtiles://https://tiles.example/u1.pmtiles"
    );
    let empty = answered(
        GeodataList {
            project: Some("kigali".into()),
        },
        json!({"data": {"uploads": null}}),
    )
    .expect("an empty list admits");
    assert!(empty.is_empty());

    let audit = request(GeodataAudit {
        project: "kigali".into(),
    });
    assert_eq!(
        audit.body,
        Some(json!({"action": "audit", "project_id": "kigali"}))
    );
    let delete = request(GeodataDelete {
        project: None,
        upload_id: "u1".into(),
    });
    assert_eq!(
        delete.body,
        Some(json!({"action": "delete", "upload_id": "u1"}))
    );
}
