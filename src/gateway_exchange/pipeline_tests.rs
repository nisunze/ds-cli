use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn call(value: Value) -> PipelineCall {
    serde_json::from_value(value).expect("a valid pipeline call")
}

fn request(value: Value) -> Request {
    match start(call(value)) {
        Step::Fetch { request, .. } => request,
        Step::Refused(refusal) => panic!("refused: {}", refusal.message),
        _ => panic!("expected a fetch"),
    }
}

fn refused(value: Value) -> Refusal {
    match start(call(value)) {
        Step::Refused(refusal) => refusal,
        _ => panic!("expected a refusal"),
    }
}

fn answered(value: Value, body: Value) -> Result<Value, Refusal> {
    let Step::Fetch { continuation, .. } = start(call(value)) else {
        panic!("expected a fetch")
    };
    match resume(
        continuation,
        Observed::Response {
            status: 200,
            content_type: Some("application/json".into()),
            retry_after: None,
        },
        body.to_string().as_bytes(),
        &Facts::default(),
    ) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

#[test]
fn the_status_is_one_pipeline_read() {
    let request = request(json!({"action": "status", "project": "it_rwanda"}));
    assert_eq!(request.path, "/api/v1/pipeline");
    assert_eq!(
        request.body,
        Some(json!({"action": "pipeline_status", "eds_project_id": "it_rwanda"}))
    );
    let status = answered(
        json!({"action": "status", "project": "it_rwanda"}),
        json!({"success": true, "data": {"sync": {"status": "success"}}}),
    )
    .unwrap();
    assert_eq!(status["sync"]["status"], "success");
}

#[test]
fn a_migration_names_its_target_and_its_filter() {
    let migrate = request(json!({"action": "migrate", "project": "it_rwanda", "target_project": "it_tchad",
        "dry_run": true, "delete_source": true, "filter": {"forms": ["poles"], "surveyors": ["a@example.com"],
        "date_from": "2026-10-01", "admin_boundaries": [{"level": "district", "code": "11"}],
        "boundary": {"type": "Polygon", "coordinates": []}}}));
    assert_eq!(migrate.timeout_ms, 600_000);
    assert_eq!(
        migrate.body,
        Some(json!({"action": "migrate", "eds_project_id": "it_rwanda", "target_project_id": "it_tchad",
            "dry_run": true, "delete_source_entries": true, "forms": ["poles"], "surveyors": ["a@example.com"],
            "date_from": "2026-10-01", "filter_admin_boundaries": [{"level": "district", "code": "11"}],
            "filter_boundary": {"type": "Polygon", "coordinates": []}}))
    );
    let delete_only = request(json!({"action": "migrate", "project": "it_rwanda", "dry_run": false,
        "delete_only": true}));
    assert_eq!(
        delete_only.body,
        Some(json!({"action": "migrate", "eds_project_id": "it_rwanda", "dry_run": false, "delete_only": true}))
    );
}

#[test]
fn a_migration_without_a_distinct_target_is_refused() {
    for bad in [
        json!({"action": "migrate", "project": "it_rwanda", "dry_run": true}),
        json!({"action": "migrate", "project": "it_rwanda", "target_project": "it_rwanda", "dry_run": true}),
        json!({"action": "migrate", "project": "it_rwanda", "target_project": "it_tchad", "dry_run": true,
            "filter": {"boundary": {"type": "Point", "coordinates": [0, 0]}}}),
    ] {
        assert_eq!(refused(bad).code, INPUT_INVALID);
    }
}

#[test]
fn tile_actions_name_their_publication() {
    assert_eq!(
        request(json!({"action": "tile_status", "project": "it_rwanda", "tile_type": "design"})).body,
        Some(json!({"action": "status", "eds_project_id": "it_rwanda", "tile_type": "design"}))
    );
    assert_eq!(
        request(json!({"action": "tile_add", "project": "it_rwanda", "tile_type": "survey",
            "source_project": "it_tchad"})).body,
        Some(json!({"action": "add", "eds_project_id": "it_rwanda", "tile_type": "survey",
            "source_project_id": "it_tchad"}))
    );
    assert_eq!(
        request(json!({"action": "global_tile_access", "tile_id": "rwanda-data-1",
            "access_policy": {"mode": "restricted", "users": ["a@example.com"]}})).body,
        Some(json!({"action": "global_access", "tile_id": "rwanda-data-1",
            "access_policy": {"mode": "restricted", "users": ["a@example.com"]}}))
    );
    assert_eq!(
        refused(json!({"action": "tile_status", "project": "it_rwanda", "tile_type": "solar"})).code,
        INPUT_INVALID
    );
}

#[test]
fn a_tile_list_reads_its_rows_and_an_error_status_is_a_refusal() {
    let list = json!({"action": "tile_list", "project": "it_rwanda", "include_global": true});
    let tiles = answered(
        list.clone(),
        json!({"status": "success", "tiles": [{"tile_id": "t1"}], "count": 1}),
    )
    .unwrap();
    assert_eq!(tiles, json!([{"tile_id": "t1"}]));
    assert_eq!(
        answered(list.clone(), json!({"status": "success", "tiles": null, "count": 0})).unwrap(),
        json!([])
    );
    let refusal = answered(list, json!({"status": "error", "error": "Requires access"})).unwrap_err();
    assert_eq!(refusal.message, "Requires access");
}
