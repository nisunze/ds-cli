use ds_command_kernel::compute_jobs::{Event, Phase};
use ds_compute_runtime::{self as runtime, Authorizer};
use ds_solar_contracts::{
    BundleBytes, CityIdentity, PreparedSolarCityInput, RunOptions, SnapshotRevision,
    SolarCityInput, WeatherDataset, WeatherKey, WeatherPin,
};
use serde_json::json;
use std::sync::Arc;
struct Allowed;
impl Authorizer for Allowed {
    fn authorize(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}
struct Revoked;
impl Authorizer for Revoked {
    fn authorize(&self, _: &str) -> Result<(), String> {
        Err("revoked".into())
    }
}
fn request() -> Vec<u8> {
    serde_json::to_vec(&json!({"schema":"ds.fast-lv.request/v1","jobs":[{"transformer_name":"T1","gdfs":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr-1","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":"T1","names":"T1"}}]},"lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line-1","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.0004,-2.0]]},"properties":{}}]},"customers":{"type":"FeatureCollection","features":[]}},"settings":{}}]})).unwrap()
}

fn prepared_solar_request() -> Vec<u8> {
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ds-solar/fixtures");
    let city = "aderm_bere";
    let snapshot: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join(format!("city/{city}.snapshot.json"))).unwrap(),
    )
    .unwrap();
    let input = SolarCityInput::from_city_snapshot(
        CityIdentity {
            project_id: "durable-server-test".to_string(),
            root: "solar".to_string(),
            city_id: city.to_string(),
            display_name: snapshot["_root"]["display_name"]
                .as_str()
                .unwrap()
                .to_string(),
        },
        SnapshotRevision {
            source: "fixture".to_string(),
            captured_at: "2026-08-08T00:00:00Z".to_string(),
            record_updated_at_ms: None,
            revision: None,
        },
        RunOptions::default(),
        &snapshot,
    )
    .unwrap();
    let weather: WeatherDataset = serde_json::from_slice(
        &std::fs::read(root.join(format!("weather/{city}.weather.json"))).unwrap(),
    )
    .unwrap();
    let weather_key = WeatherKey::new(
        input.site.latitude,
        input.site.longitude,
        weather.provenance.timezone.clone(),
        WeatherPin::pvgis_tmy(),
    )
    .unwrap();
    let reference_root = root.join("reference").join(city);
    let reference = ds_solar_contracts::reference_unit::open(&BundleBytes {
        weather_json: std::fs::read(reference_root.join("weather.json")).unwrap(),
        reference_unit_json: std::fs::read(reference_root.join("reference-unit.json")).unwrap(),
        reference_unit_parquet: std::fs::read(reference_root.join("reference-unit.parquet"))
            .unwrap(),
        manifest_json: std::fs::read(reference_root.join("manifest.json")).unwrap(),
    })
    .unwrap()
    .reference_unit;
    let prepared = PreparedSolarCityInput::commit(
        input,
        weather_key,
        weather,
        reference,
        "2026-08-20T00:00:00Z",
    )
    .unwrap();
    serde_json::to_vec(&json!({
        "prepared": prepared,
        "render_charts": false,
        "run_id": "durable-solar-run",
    }))
    .unwrap()
}
#[test]
fn real_compute_survives_reopen_and_never_recomputes_completed_work() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let bytes = request();
    let job = runtime::submit(&path, "owner", "stable", "request1", &bytes).unwrap();
    assert_eq!(
        runtime::open(&path)
            .unwrap()
            .job("owner", "stable", &job.id)
            .unwrap()
            .unwrap()
            .phase,
        Phase::Queued
    );
    assert!(runtime::run_one(&path, "owner", "stable", "worker", Arc::new(Allowed), None).unwrap());
    let store = runtime::open(&path).unwrap();
    let result = store
        .job_result("owner", "stable", &job.id)
        .unwrap()
        .unwrap();
    let decoded: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(decoded["schema"], "ds.fast-lv.result/v1");
    assert_eq!(decoded["jobs"][0]["transformer_name"], "T1");
    assert_eq!(decoded["jobs"][0]["ok"], true);
    assert_eq!(
        runtime::submit(&path, "owner", "stable", "request1", &bytes)
            .unwrap()
            .attempts,
        1
    );
    assert!(
        !runtime::run_one(&path, "owner", "stable", "worker2", Arc::new(Allowed), None).unwrap()
    );
    assert!(store
        .job_result("another-owner", "stable", &job.id)
        .unwrap()
        .is_none());
}

#[test]
fn prepared_solar_survives_reopen_without_a_client_path_or_live_request() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let mut request: serde_json::Value = serde_json::from_slice(&prepared_solar_request()).unwrap();
    request["provenance"] = valid_provenance();
    let bytes = serde_json::to_vec(&request).unwrap();
    assert!(runtime::submit(&path, "owner", "stable", "solar-request", &bytes).is_err());
    let job = runtime::submit_solar(&path, "owner", "stable", "solar-request", &bytes).unwrap();
    assert_eq!(job.input_tag, "ds.solar.calculate.prepared/v1");
    // `run_one` reopens the durable store after submission. Its only input is
    // the retained request bytes, so this proves no client workspace/cache path
    // or original HTTP request remains in the execution boundary.
    assert!(runtime::run_one(
        &path,
        "owner",
        "stable",
        "recovered",
        Arc::new(Allowed),
        None
    )
    .unwrap());
    let result = runtime::open(&path)
        .unwrap()
        .job_result("owner", "stable", &job.id)
        .unwrap()
        .unwrap();
    let decoded: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(decoded["run_id"], "durable-solar-run");
    assert!(decoded["result_digest"].as_str().is_some());
    assert_eq!(decoded["plots_generated"], false);
    assert_eq!(decoded["outputs"].as_array().unwrap().len(), 1);
    let stored = runtime::open(&path).unwrap();
    let durable_job = stored
        .job("owner", "stable", &job.id)
        .unwrap()
        .expect("completed job remains visible");
    let publication = runtime::solar_publication(&durable_job, &bytes, &result).unwrap();
    assert_eq!(publication.project_id, "durable-server-test");
    assert_eq!(publication.city_id, "aderm_bere");
    assert_eq!(publication.outputs.len(), 1);
    assert_eq!(publication.outputs[0].output_id, "report_input.json");
    assert_eq!(
        publication.provenance.unwrap().snapshot_receipt_id,
        "550e8400-e29b-51d4-a716-446655440000"
    );
    // Publication may not accept a different or damaged sealed request even
    // when the result bytes still match the durable result digest.
    request["provenance"]["root"] = json!("another-root");
    let invalid = serde_json::to_vec(&request).unwrap();
    assert!(runtime::solar_publication(&durable_job, &invalid, &result).is_err());
    assert!(runtime::solar_publication_metadata(&durable_job, &invalid, &result).is_err());
}

fn valid_provenance() -> serde_json::Value {
    json!({
        "project_id": "durable-server-test", "root": "solar", "template_id": "aderm_bere",
        "input_base_fingerprint": "a".repeat(64), "source_snapshot_sha256": "b".repeat(64),
        "snapshot_receipt_id": "550e8400-e29b-51d4-a716-446655440000"
    })
}

#[test]
fn direct_submission_and_durable_replay_reject_invalid_snapshot_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let mut request: serde_json::Value = serde_json::from_slice(&prepared_solar_request()).unwrap();
    for (field, value) in [
        ("project_id", "another-project"),
        ("root", "another-root"),
        ("template_id", "another-city"),
        ("input_base_fingerprint", "invalid"),
        ("source_snapshot_sha256", "invalid"),
        (
            "snapshot_receipt_id",
            "zzzzzzzz-zzzz-5zzz-azzz-zzzzzzzzzzzz",
        ),
    ] {
        request["provenance"] = valid_provenance();
        request["provenance"][field] = json!(value);
        let invalid = serde_json::to_vec(&request).unwrap();
        assert!(
            runtime::submit_solar(&path, "owner", "stable", "invalid", &invalid)
                .unwrap_err()
                .contains("provenance"),
            "{field}"
        );
    }
    // Simulate a pre-validation durable row retained by an earlier release.
    // The worker must refuse it after reopening instead of calculating it.
    request["provenance"] = valid_provenance();
    let valid = serde_json::to_vec(&request).unwrap();
    let mut job = runtime::submit_solar(&path, "owner", "stable", "valid", &valid).unwrap();
    let replay_path = dir.path().join("replay.sqlite");
    request["provenance"]["root"] = json!("another-root");
    let invalid = serde_json::to_vec(&request).unwrap();
    job.input_sha256 = runtime::digest(&invalid);
    runtime::open(&replay_path)
        .unwrap()
        .submit_job(&job, &invalid)
        .unwrap();
    assert!(runtime::run_one(
        &replay_path,
        "owner",
        "stable",
        "recovered",
        Arc::new(Allowed),
        None
    )
    .unwrap());
    let stored = runtime::open(&replay_path).unwrap();
    let refused = stored.job("owner", "stable", &job.id).unwrap().unwrap();
    assert_eq!(refused.phase, Phase::Failed);
    assert!(refused.error.unwrap().contains("provenance"));
    assert!(stored
        .job_result("owner", "stable", &job.id)
        .unwrap()
        .is_none());
}
#[test]
fn parallel_claims_are_unique_and_revocation_and_cancellation_stop_work() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let bytes = request();
    for key in ["a", "b"] {
        runtime::submit(&path, "owner", "stable", key, &bytes).unwrap();
    }
    assert!(
        runtime::run_one(&path, "owner", "stable", "revoked", Arc::new(Revoked), None).is_err()
    );
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let handles = (0..2)
        .map(|i| {
            let p = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = runtime::open(&p).unwrap();
                barrier.wait();
                store
                    .claim_job("owner", "stable", &format!("worker{i}"), 100, 1000)
                    .unwrap()
                    .unwrap()
                    .0
            })
        })
        .collect::<Vec<_>>();
    let jobs = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_ne!(jobs[0].id, jobs[1].id);
    let mut store = runtime::open(&path).unwrap();
    store
        .update_job("owner", "stable", &jobs[0].id, Event::Cancel, 101, None)
        .unwrap();
    assert!(store
        .update_job(
            "owner",
            "stable",
            &jobs[0].id,
            Event::Complete {
                worker: jobs[0].worker.as_ref().unwrap(),
                result_sha256: &runtime::digest(b"x")
            },
            102,
            Some(b"x")
        )
        .is_err());
    let recovered = store
        .claim_job("owner", "stable", "recovered", 1100, 1000)
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(recovered.id, jobs[1].id);
    assert_eq!(recovered.attempts, 2);
    let mut different: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    different["jobs"][0]["transformer_name"] = json!("T2");
    assert!(runtime::submit(
        &path,
        "owner",
        "stable",
        "a",
        &serde_json::to_vec(&different).unwrap()
    )
    .is_err());
}

#[test]
fn workers_complete_real_batches_inside_the_shared_resource_bound() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    struct Bound(Arc<AtomicBool>);
    impl Authorizer for Bound {
        fn authorize(&self, _: &str) -> Result<(), String> {
            if rayon::current_thread_index().is_some() {
                assert_eq!(rayon::current_num_threads(), 2);
                self.0.store(true, Ordering::Release);
            }
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    for key in ["first", "second"] {
        runtime::submit(&path, "owner", "stable", key, &request()).unwrap();
    }
    let observed = Arc::new(AtomicBool::new(false));
    let workers = runtime::Workers::start(
        path.clone(),
        "owner".into(),
        "stable".into(),
        2,
        Arc::new(Bound(observed.clone())),
        None,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let jobs = runtime::open(&path)
            .unwrap()
            .jobs("owner", "stable", 10)
            .unwrap();
        if jobs.iter().all(|job| job.phase == Phase::Completed) {
            assert_eq!(jobs.len(), 2);
            assert!(jobs.iter().all(|job| job.attempts == 1));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "workers did not complete: {jobs:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(workers);
    assert!(observed.load(Ordering::Acquire));
}
