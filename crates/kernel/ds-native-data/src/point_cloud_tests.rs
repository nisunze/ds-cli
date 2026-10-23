//! The host half keeps one extraction at a time and its progress readout;
//! generation and sampling are the Network engine's (its own tests).
use super::*;
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn progress_starts_idle_and_reports_what_it_is_doing() {
    let state = NativePointCloudState::default();
    assert!(!state.snapshot().running);
    state.begin(1_000);
    let running = state.snapshot();
    assert!(running.running);
    assert_eq!(running.total, 1_000);
    state.publish(true, "sampling", 512, 1_000, "Reading Rwanda elevation");
    assert_eq!(state.snapshot().done, 512);
    state.finish("Extraction complete", 1_000);
    let done = state.snapshot();
    assert!(!done.running);
    assert_eq!(done.stage, "complete");
}

#[test]
fn heap_owned_completed_evidence_keeps_the_exact_receiver_wire() {
    let evidence = NativePointCloudEvidence {
        task: "native_elevation_point_cloud".into(),
        execution_host: "desktop".into(),
        sampling: "grid".into(),
        output_path: "/data/α.geojson".into(),
        output_bytes: 4096,
        output_sha256: "a".repeat(64),
        csv_path: "/data/α.csv".into(),
        csv_bytes: 2048,
        csv_sha256: "b".repeat(64),
        output_formats: vec!["geojson".into(), "csv".into()],
        point_count: 480,
        interpolated_count: 478,
        rwanda_count: 480,
        fallback_count: 0,
        unresolved_count: 2,
        rwanda_coverage: "partial".into(),
        rwanda_source: "rwanda_dem_10m".into(),
        fallback_source: Some("aws_terrarium_30m".into()),
        dem_access: "direct_range".into(),
        direct_public_range_reads: true,
        full_local_required_above_points: 4000,
        cloud_service_used: false,
        renamed_attributes: BTreeMap::new(),
        areas: vec![NativePointCloudAreaStats {
            area_index: 0,
            area_id: "bbox".into(),
            part_count: 1,
            area_km2: 1.2,
            point_count: 480,
        }],
    };
    let expected = json!({"status":"completed", "evidence":&evidence});
    let outcome = NativePointCloudOutcome::Completed {
        evidence: Box::new(evidence),
    };
    assert_eq!(
        serde_json::to_vec(&outcome).unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
    let actual = serde_json::to_value(outcome).unwrap();
    assert_eq!(actual["status"], "completed");
    assert_eq!(actual["evidence"]["outputBytes"], 4096);
    assert_eq!(actual["evidence"]["cloudServiceUsed"], false);
    assert_eq!(actual["evidence"]["areas"][0]["pointCount"], 480);
    assert!(actual["evidence"].get("output_bytes").is_none());
}

#[test]
fn a_running_extraction_retains_its_scope_instead_of_being_replaced() {
    let state = NativePointCloudState::default();
    let a = RequestContext::local_files(crate::ExecutionHost::Headless, crate::online);
    let b = RequestContext::local_files(crate::ExecutionHost::Desktop, crate::online);
    let admission = state.admit(&a).unwrap();
    assert!(state.admit(&b).is_err());
    assert_eq!(
        state
            .active_request
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .request_id(),
        a.request_id()
    );
    drop(admission);
    assert!(state.admit(&b).is_ok());
}

#[test]
fn an_inline_area_travels_as_a_file_beside_the_job() {
    let dir = tempfile::tempdir().unwrap();
    let request: NativePointCloudRequest = serde_json::from_value(json!({
        "areaGeojson": "{\"type\":\"Polygon\",\"coordinates\":[]}",
        "sampling": {"mode": "grid", "spacingM": 25.0}}))
    .unwrap();
    let task = engine_task(&request, dir.path()).unwrap();
    assert!(task.request.area_geojson.is_none());
    let path = task.inline_area.unwrap();
    assert!(path.starts_with(dir.path()));
    assert!(std::fs::read_to_string(path).unwrap().contains("Polygon"));
}
