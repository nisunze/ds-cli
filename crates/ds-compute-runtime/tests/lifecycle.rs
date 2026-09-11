use ds_command_kernel::compute_jobs::{Event, Phase};
use ds_compute_runtime::{self as runtime, Authorizer};
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
    assert!(runtime::run_one(&path, "owner", "stable", "worker", Arc::new(Allowed)).unwrap());
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
    assert!(!runtime::run_one(&path, "owner", "stable", "worker2", Arc::new(Allowed)).unwrap());
    assert!(store
        .job_result("another-owner", "stable", &job.id)
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
    assert!(runtime::run_one(&path, "owner", "stable", "revoked", Arc::new(Revoked)).is_err());
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
