//! The durable compute lifecycle, end to end on a real store, under one
//! verified execution context per job.
//!
//! Two projects share one queue here and never see each other; a reused key
//! keeps its scope; capacity bounds admission and keeps a project's share
//! honest; a membership revoked mid-flight fails exactly one job; and a row
//! written by a released Server recovers before any worker can claim it.

use ds_command_kernel::compute_jobs::{Event, Phase};
use ds_command_kernel::execution_context::{Limits, Membership, Principal, Slot};
use ds_compute_runtime::{
    self as runtime, Admission, Authorizer, HostIdentity, MembershipSource, WorkerContext,
};
use ds_solar_contracts::{
    BundleBytes, CityIdentity, PreparedSolarCityInput, RunOptions, SnapshotRevision,
    SolarCityInput, WeatherDataset, WeatherKey, WeatherPin,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

const DEPLOYMENT: &str = "https://gateway.example";
const A: &str = "project-a";
const B: &str = "project-b";
const SOLAR_PROJECT: &str = "durable-server-test";

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

/// A membership snapshot a test can change while work is in flight.
struct Directory(Mutex<Vec<String>>);
impl Directory {
    fn new(projects: &[&str]) -> Arc<Self> {
        Arc::new(Self(Mutex::new(
            projects.iter().map(|p| (*p).to_owned()).collect(),
        )))
    }
    fn revoke(&self, project: &str) {
        self.0.lock().unwrap().retain(|known| known != project);
    }
    fn snapshot_now(&self) -> Membership {
        self.snapshot(runtime::now_ms()).unwrap()
    }
}
impl MembershipSource for Directory {
    fn snapshot(&self, now_ms: u64) -> Result<Membership, String> {
        Ok(Membership {
            projects: self.0.lock().unwrap().clone(),
            fetched_at_ms: now_ms,
            ttl_ms: 600_000,
        })
    }
}

fn host(owner: &str, lane: &str) -> HostIdentity {
    HostIdentity {
        owner: owner.to_owned(),
        principal: Principal {
            uid: "uid-a".into(),
            lane: lane.to_owned(),
            deployment: DEPLOYMENT.into(),
            install_id: "install-1".into(),
        },
    }
}
fn limits() -> Limits {
    Limits {
        global_running: 4,
        per_project_running: 2,
        per_project_queued: 8,
        global_queued: 16,
    }
}
fn admission<'a>(
    identity: &'a HostIdentity,
    key: &'a str,
    project: Option<&'a str>,
    membership: &'a Membership,
) -> Admission<'a> {
    Admission {
        identity,
        client: "test:1",
        key,
        requested_project: project,
        saved_project: None,
        membership,
        limits: limits(),
        now_ms: runtime::now_ms(),
    }
}
fn worker(
    path: &std::path::Path,
    identity: &HostIdentity,
    directory: &Arc<Directory>,
    auth: Arc<dyn Authorizer>,
) -> WorkerContext {
    WorkerContext {
        path: path.to_owned(),
        identity: identity.clone(),
        limits: limits(),
        auth,
        membership: directory.clone(),
        observer: None,
        saved_project: None,
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
            project_id: SOLAR_PROJECT.to_string(),
            root: "solar".to_string(),
            city_id: city.to_string(),
            display_name: snapshot["_root"]["display_name"]
                .as_str()
                .unwrap()
                .to_string(),
        },
        SnapshotRevision {
            source: "ds-brain".to_string(),
            captured_at: "2026-08-08T00:00:00Z".to_string(),
            record_updated_at_ms: None,
            revision: Some("a".repeat(64)),
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
    let identity = host("owner", "stable");
    let directory = Directory::new(&[A]);
    let membership = directory.snapshot_now();
    let job = runtime::submit(
        &path,
        &admission(&identity, "request1", Some(A), &membership),
        &bytes,
    )
    .unwrap();
    assert_eq!(job.context.as_ref().unwrap().project, A);
    assert_eq!(
        runtime::open(&path)
            .unwrap()
            .job(&identity.caller(Some(A)), &job.id)
            .unwrap()
            .unwrap()
            .phase,
        Phase::Queued
    );
    let context = worker(&path, &identity, &directory, Arc::new(Allowed));
    assert!(runtime::run_one(&context, "worker").unwrap());
    let store = runtime::open(&path).unwrap();
    let result = store
        .job_result(&identity.caller(Some(A)), &job.id)
        .unwrap()
        .unwrap();
    let decoded: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(decoded["schema"], "ds.fast-lv.result/v1");
    assert_eq!(decoded["jobs"][0]["transformer_name"], "T1");
    assert_eq!(decoded["jobs"][0]["ok"], true);
    assert_eq!(
        runtime::submit(
            &path,
            &admission(&identity, "request1", Some(A), &membership),
            &bytes
        )
        .unwrap()
        .attempts,
        1
    );
    assert!(!runtime::run_one(&context, "worker2").unwrap());
    let other_owner = host("another-owner", "stable");
    assert!(store
        .job_result(&other_owner.caller(None), &job.id)
        .unwrap()
        .is_none());
}

#[test]
fn two_projects_share_one_queue_and_neither_context_ever_crosses() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "canary");
    let directory = Directory::new(&[A, B]);
    let membership = directory.snapshot_now();
    let first = runtime::submit(
        &path,
        &admission(&identity, "for-a", Some(A), &membership),
        &request(),
    )
    .unwrap();
    let mut other: serde_json::Value = serde_json::from_slice(&request()).unwrap();
    other["jobs"][0]["transformer_name"] = json!("T2");
    let second = runtime::submit(
        &path,
        &admission(&identity, "for-b", Some(B), &membership),
        &serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();
    let store = runtime::open(&path).unwrap();
    assert_eq!(store.jobs(&identity.caller(None), 10).unwrap().len(), 2);
    assert_eq!(
        store
            .jobs(&identity.caller(Some(A)), 10)
            .unwrap()
            .iter()
            .map(|job| job.id.clone())
            .collect::<Vec<_>>(),
        vec![first.id.clone()]
    );
    // Status, result and sealed input of A's job, asked for as B: absent.
    let as_b = identity.caller(Some(B));
    assert!(store.job(&as_b, &first.id).unwrap().is_none());
    assert!(store.job_input(&as_b, &first.id).unwrap().is_none());
    assert!(store.job_result(&as_b, &first.id).unwrap().is_none());
    drop(store);
    // Both run, in their own projects, from the same pool.
    let context = worker(&path, &identity, &directory, Arc::new(Allowed));
    assert!(runtime::run_one(&context, "worker-1").unwrap());
    assert!(runtime::run_one(&context, "worker-2").unwrap());
    let store = runtime::open(&path).unwrap();
    for (job, project) in [(&first, A), (&second, B)] {
        let done = store
            .job(&identity.caller(Some(project)), &job.id)
            .unwrap()
            .unwrap();
        assert_eq!(done.phase, Phase::Completed, "{project}");
        assert_eq!(done.context.unwrap().project, project);
    }
}

#[test]
fn a_key_keeps_its_scope_and_a_project_outside_membership_is_never_admitted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "canary");
    let directory = Directory::new(&[A, B]);
    let membership = directory.snapshot_now();
    let bytes = request();
    let job = runtime::submit(
        &path,
        &admission(&identity, "shared", Some(A), &membership),
        &bytes,
    )
    .unwrap();
    // Same key, same bytes, same project: the same job, not a second one.
    let again = runtime::submit(
        &path,
        &admission(&identity, "shared", Some(A), &membership),
        &bytes,
    )
    .unwrap();
    assert_eq!(again.id, job.id);
    assert_eq!(again.context, job.context);
    // Same key, another project.
    let moved = runtime::submit(
        &path,
        &admission(&identity, "shared", Some(B), &membership),
        &bytes,
    )
    .unwrap_err();
    assert_eq!(moved.code(), Some("scope_mismatch_for_key"));
    // Same key, other bytes.
    let mut other: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    other["jobs"][0]["transformer_name"] = json!("T2");
    let changed = runtime::submit(
        &path,
        &admission(&identity, "shared", Some(A), &membership),
        &serde_json::to_vec(&other).unwrap(),
    )
    .unwrap_err();
    assert_eq!(changed.code(), Some("payload_changed_for_key"));
    // A project the directory does not list, and no project at all.
    let outside = runtime::submit(
        &path,
        &admission(&identity, "elsewhere", Some("project-z"), &membership),
        &bytes,
    )
    .unwrap_err();
    assert_eq!(outside.code(), Some("project_not_visible"));
    let unnamed = runtime::submit(
        &path,
        &admission(&identity, "unnamed", None, &membership),
        &bytes,
    )
    .unwrap_err();
    assert_eq!(unnamed.code(), Some("project_required"));
    // A stale snapshot says the same sentence a non-member gets.
    let stale = Membership {
        fetched_at_ms: 1,
        ttl_ms: 1_000,
        ..membership.clone()
    };
    let expired = runtime::submit(
        &path,
        &admission(&identity, "expired", Some(A), &stale),
        &bytes,
    )
    .unwrap_err();
    assert_eq!(expired.code(), Some("project_not_visible"));
    assert_eq!(
        expired.refusal().unwrap().message,
        outside.refusal().unwrap().message
    );
    assert_eq!(
        runtime::open(&path)
            .unwrap()
            .jobs(&identity.caller(None), 10)
            .unwrap()
            .len(),
        1,
        "every refusal left the queue exactly as it was"
    );
}

#[test]
fn capacity_bounds_the_queue_per_project_and_globally_with_retry_guidance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "canary");
    let directory = Directory::new(&[A, B]);
    let membership = directory.snapshot_now();
    let bytes = request();
    let tight = Limits {
        global_running: 2,
        per_project_running: 1,
        per_project_queued: 2,
        global_queued: 3,
    };
    let submit = |key: &str, project: &str, limits: Limits| {
        runtime::submit(
            &path,
            &Admission {
                limits,
                ..admission(&identity, key, Some(project), &membership)
            },
            &bytes,
        )
    };
    submit("a1", A, tight).unwrap();
    submit("a2", A, tight).unwrap();
    // A is at its queued share; B still gets in.
    let refused = submit("a3", A, tight).unwrap_err();
    assert_eq!(refused.code(), Some("capacity_exhausted"));
    let refusal = refused.refusal().unwrap();
    assert_eq!(refusal.scope, Some("project"));
    assert!(refusal.retry_after_ms.is_some_and(|ms| ms > 0));
    assert!(
        !refusal.message.contains(A),
        "a capacity refusal names counts and limits, never a project: {}",
        refusal.message
    );
    submit("b1", B, tight).unwrap();
    // The host queue is full now, for everyone.
    let global = submit("b2", B, tight).unwrap_err();
    assert_eq!(global.refusal().unwrap().scope, Some("global"));
    // Cancelling one of A's releases the room it held.
    let queued = runtime::open(&path)
        .unwrap()
        .jobs(&identity.caller(Some(A)), 10)
        .unwrap();
    runtime::open(&path)
        .unwrap()
        .update_job(
            &identity.caller(Some(A)),
            &queued[0].id,
            Event::Cancel,
            runtime::now_ms(),
            None,
        )
        .unwrap();
    submit("b2", B, tight).unwrap();
}

#[test]
fn a_claim_keeps_one_projects_share_while_another_project_is_waiting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "canary");
    let directory = Directory::new(&[A, B]);
    let membership = directory.snapshot_now();
    // Two batches each, so that after one slot apiece both projects still
    // have work waiting — the only situation in which a share is a fence.
    for (key, project, name) in [
        ("a1", A, "T1"),
        ("a2", A, "T2"),
        ("b1", B, "T3"),
        ("b2", B, "T4"),
    ] {
        let mut bytes: serde_json::Value = serde_json::from_slice(&request()).unwrap();
        bytes["jobs"][0]["transformer_name"] = json!(name);
        runtime::submit(
            &path,
            &admission(&identity, key, Some(project), &membership),
            &serde_json::to_vec(&bytes).unwrap(),
        )
        .unwrap();
    }
    let share = Limits {
        global_running: 4,
        per_project_running: 1,
        per_project_queued: 8,
        global_queued: 16,
    };
    let mut running: Vec<Slot> = Vec::new();
    let mut queued = vec![A, A, B, B]
        .into_iter()
        .map(|project| Slot {
            project: project.into(),
        })
        .collect::<Vec<_>>();
    let mut store = runtime::open(&path).unwrap();
    let mut claim = |running: &Vec<Slot>, queued: &Vec<Slot>, worker: &str| {
        let mut admits = |project: &str| {
            ds_command_kernel::execution_context::capacity(
                share,
                ds_command_kernel::execution_context::Intent::Claim,
                running,
                queued,
                project,
            )
            .is_ok()
        };
        store
            .claim_job(
                &identity.caller(None),
                worker,
                runtime::now_ms(),
                30_000,
                &mut admits,
            )
            .unwrap()
    };
    // A is first in queue order and takes the one slot its share allows. The
    // next claim skips A — its second batch is older than B's first — and
    // starts B, so a long queue in one project never idles the machine for
    // another.
    for (worker, expected) in [("worker-1", A), ("worker-2", B)] {
        let (job, _) = claim(&running, &queued, worker).expect("a project still has room");
        let project = job.context.unwrap().project;
        assert_eq!(project, expected, "{worker}");
        running.push(Slot {
            project: project.clone(),
        });
        let position = queued
            .iter()
            .position(|slot| slot.project == project)
            .unwrap();
        queued.remove(position);
    }
    // Both hold their share and both still have work waiting: nothing more is
    // claimable until one of them finishes.
    assert!(claim(&running, &queued, "worker-3").is_none());
    // When B's work is done and only A waits, A may exceed its share rather
    // than leave the machine idle.
    let queued = vec![Slot { project: A.into() }];
    let running = vec![Slot { project: A.into() }];
    assert!(claim(&running, &queued, "worker-4").is_some());
}

#[test]
fn a_membership_revoked_while_queued_fails_only_that_projects_job() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "canary");
    let directory = Directory::new(&[A, B]);
    let membership = directory.snapshot_now();
    let doomed = runtime::submit(
        &path,
        &admission(&identity, "for-a", Some(A), &membership),
        &request(),
    )
    .unwrap();
    let mut other: serde_json::Value = serde_json::from_slice(&request()).unwrap();
    other["jobs"][0]["transformer_name"] = json!("T2");
    let survivor = runtime::submit(
        &path,
        &admission(&identity, "for-b", Some(B), &membership),
        &serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();
    directory.revoke(A);
    let context = worker(&path, &identity, &directory, Arc::new(Allowed));
    assert!(runtime::run_one(&context, "worker-1").unwrap());
    assert!(runtime::run_one(&context, "worker-2").unwrap());
    let store = runtime::open(&path).unwrap();
    let failed = store
        .job(&identity.caller(None), &doomed.id)
        .unwrap()
        .unwrap();
    assert_eq!(failed.phase, Phase::Failed);
    assert!(failed
        .error
        .as_deref()
        .unwrap()
        .starts_with(runtime::MEMBERSHIP_REVOKED));
    assert!(
        store
            .job_result(&identity.caller(None), &doomed.id)
            .unwrap()
            .is_none(),
        "a revoked project's job produced no result"
    );
    assert_eq!(
        store
            .job(&identity.caller(Some(B)), &survivor.id)
            .unwrap()
            .unwrap()
            .phase,
        Phase::Completed,
        "the other project's work is untouched"
    );
    // The job keeps the context it was admitted under; nothing re-scoped it.
    assert_eq!(failed.context.unwrap().project, A);
}

#[test]
fn a_row_written_by_a_released_server_recovers_before_any_worker_claims_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "canary");
    let bytes = request();
    // Exactly what the released binary's INSERT wrote: no `context` member.
    let id = runtime::job_id("owner", "canary", "released");
    let digest = runtime::digest(&bytes);
    let row = json!({
        "id": id, "owner": "owner", "lane": "canary", "input_sha256": digest,
        "engine": "fast_lv", "input_tag": "ds.fast-lv.request/v1", "phase": "queued",
        "attempts": 0, "created_at_ms": 1_700_000_000_000u64, "updated_at_ms": 1_700_000_000_000u64,
        "worker": serde_json::Value::Null, "lease_until_ms": 0,
        "result_sha256": serde_json::Value::Null, "error": serde_json::Value::Null
    });
    runtime::open(&path).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO compute_jobs(id,owner,lane,phase,lease_until,created,row,input) VALUES (?1,'owner','canary','queued',0,1700000000000,?2,?3)",
            rusqlite::params![id, row.to_string(), bytes.as_slice()],
        )
        .unwrap();
    // Without a selection to recover into, the row is named, not guessed at,
    // and no worker will run it.
    let unrecovered = runtime::recover_contexts(&path, &identity, None).unwrap();
    assert_eq!(unrecovered.unrecoverable, vec![id.clone()]);
    let directory = Directory::new(&[A]);
    let idle = worker(&path, &identity, &directory, Arc::new(Allowed));
    assert!(!runtime::run_one(&idle, "worker-0").unwrap());
    // Started with a selection, the pool recovers it first and then runs it in
    // the recovered project.
    let context = Arc::new(WorkerContext {
        saved_project: Some(A.into()),
        ..worker(&path, &identity, &directory, Arc::new(Allowed))
    });
    let workers = runtime::Workers::start(context, 1).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let job = runtime::open(&path)
            .unwrap()
            .job(&identity.caller(Some(A)), &id)
            .unwrap()
            .expect("recovered into project A");
        if job.phase == Phase::Completed {
            let context = job.context.unwrap();
            assert_eq!(context.project, A);
            assert_eq!(context.client, "recovery:legacy-row");
            assert_eq!(context.admitted_at_ms, 1_700_000_000_000);
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{job:?}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    drop(workers);
}

#[test]
fn prepared_solar_survives_reopen_without_a_client_path_or_live_request() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "stable");
    let directory = Directory::new(&[SOLAR_PROJECT]);
    let membership = directory.snapshot_now();
    let mut request: serde_json::Value = serde_json::from_slice(&prepared_solar_request()).unwrap();
    request["provenance"] = valid_provenance();
    let bytes = serde_json::to_vec(&request).unwrap();
    assert!(runtime::submit(
        &path,
        &admission(&identity, "solar-request", Some(SOLAR_PROJECT), &membership),
        &bytes
    )
    .is_err());
    let job = runtime::submit_solar(
        &path,
        // The caller names nothing: the sealed input names its own project.
        &admission(&identity, "solar-request", None, &membership),
        &bytes,
    )
    .unwrap();
    assert_eq!(job.input_tag, "ds.solar.calculate.prepared/v1");
    assert_eq!(job.context.as_ref().unwrap().project, SOLAR_PROJECT);
    // A caller who names a contradicting project is refused outright.
    let contradicting = Directory::new(&[SOLAR_PROJECT, A]);
    let mismatch = runtime::submit_solar(
        &path,
        &admission(
            &identity,
            "solar-elsewhere",
            Some(A),
            &contradicting.snapshot_now(),
        ),
        &bytes,
    )
    .unwrap_err();
    assert_eq!(mismatch.code(), Some("scope_mismatch"));
    // `run_one` reopens the durable store after submission. Its only input is
    // the retained request bytes, so this proves no client workspace/cache path
    // or original HTTP request remains in the execution boundary.
    let context = worker(&path, &identity, &directory, Arc::new(Allowed));
    assert!(runtime::run_one(&context, "recovered").unwrap());
    let result = runtime::open(&path)
        .unwrap()
        .job_result(&identity.caller(Some(SOLAR_PROJECT)), &job.id)
        .unwrap()
        .unwrap();
    let decoded: serde_json::Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(decoded["run_id"], "durable-solar-run");
    assert!(decoded["result_digest"].as_str().is_some());
    assert_eq!(decoded["plots_generated"], false);
    assert_eq!(decoded["outputs"].as_array().unwrap().len(), 1);
    let stored = runtime::open(&path).unwrap();
    let durable_job = stored
        .job(&identity.caller(None), &job.id)
        .unwrap()
        .expect("completed job remains visible");
    let publication = runtime::solar_publication(&durable_job, &bytes, &result).unwrap();
    assert_eq!(publication.project_id, SOLAR_PROJECT);
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
        "project_id": SOLAR_PROJECT, "root": "solar", "template_id": "aderm_bere",
        "input_base_fingerprint": "a".repeat(64), "source_snapshot_sha256": "b".repeat(64),
        "snapshot_receipt_id": "550e8400-e29b-51d4-a716-446655440000"
    })
}

#[test]
fn server_submission_binds_existing_preparation_claim_to_exact_sealed_input() {
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&prepared_solar_request()).unwrap();
    envelope["schema_version"] = json!("ds.solar.server-submission/v1");
    envelope["publication_claim"] = json!({
        "schema_version": "ds-solar.prepared-publication-claim/v1",
        "claim_kind": "server-revalidated-publication-handoff",
        "project_id": SOLAR_PROJECT, "city_id": "aderm_bere",
        "city_content_digest": envelope["prepared"]["city"]["content_digest"],
        "prepared_input_digest": envelope["prepared"]["input_digest"],
        "source_snapshot_sha256": "b".repeat(64),
        "input_base_fingerprint": "a".repeat(64),
        "snapshot_receipt_id": "550e8400-e29b-51d4-a716-446655440000",
        "snapshot_receipt_expires_at": "2026-09-12T00:00:00Z"
    });
    let (input, provenance) =
        runtime::decode_solar_server_submission(&serde_json::to_vec(&envelope).unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "canary");
    let directory = Directory::new(&[SOLAR_PROJECT]);
    let membership = directory.snapshot_now();
    let job = runtime::submit_solar_with_provenance(
        &path,
        &admission(&identity, "claim", None, &membership),
        &input,
        provenance,
    )
    .unwrap();
    assert_eq!(job.context.as_ref().unwrap().project, SOLAR_PROJECT);
    let sealed = runtime::open(&path)
        .unwrap()
        .job_input(&identity.caller(Some(SOLAR_PROJECT)), &job.id)
        .unwrap()
        .unwrap();
    let sealed: serde_json::Value = serde_json::from_slice(&sealed).unwrap();
    assert_eq!(
        sealed["provenance"]["input_base_fingerprint"],
        "a".repeat(64)
    );
    assert!(sealed.get("publication_claim").is_none());
    for (field, value) in [
        ("prepared_input_digest", "sha256:other".to_owned()),
        ("city_content_digest", "sha256:other".to_owned()),
        ("input_base_fingerprint", "c".repeat(64)),
        ("project_id", "other".to_owned()),
        ("city_id", "other".to_owned()),
    ] {
        let mut stale = envelope.clone();
        stale["publication_claim"][field] = json!(value);
        assert!(
            runtime::decode_solar_server_submission(&serde_json::to_vec(&stale).unwrap()).is_err(),
            "{field}"
        );
    }
    envelope
        .as_object_mut()
        .unwrap()
        .remove("publication_claim");
    assert!(
        runtime::decode_solar_server_submission(&serde_json::to_vec(&envelope).unwrap())
            .unwrap_err()
            .contains("claim_missing")
    );
}

#[test]
fn direct_submission_and_durable_replay_reject_invalid_snapshot_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "stable");
    let directory = Directory::new(&[SOLAR_PROJECT, "another-project"]);
    let membership = directory.snapshot_now();
    let mut request: serde_json::Value = serde_json::from_slice(&prepared_solar_request()).unwrap();
    for (field, value) in [
        ("project_id", "another-project"),
        ("root", "another-root"),
        ("template_id", "another-city"),
        ("input_base_fingerprint", "invalid"),
        (
            "input_base_fingerprint",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
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
            runtime::submit_solar(
                &path,
                &admission(&identity, "invalid", None, &membership),
                &invalid
            )
            .unwrap_err()
            .to_string()
            .contains("provenance"),
            "{field}"
        );
    }
    // Simulate a pre-validation durable row retained by an earlier release.
    // The worker must refuse it after reopening instead of calculating it.
    request["provenance"] = valid_provenance();
    let valid = serde_json::to_vec(&request).unwrap();
    let mut job = runtime::submit_solar(
        &path,
        &admission(&identity, "valid", None, &membership),
        &valid,
    )
    .unwrap();
    let replay_path = dir.path().join("replay.sqlite");
    request["provenance"]["root"] = json!("another-root");
    let invalid = serde_json::to_vec(&request).unwrap();
    job.input_sha256 = runtime::digest(&invalid);
    let context = job.context.as_mut().unwrap();
    context.input_sha256 = job.input_sha256.clone();
    runtime::open(&replay_path)
        .unwrap()
        .submit_job(&job, &invalid)
        .unwrap();
    let worker = worker(&replay_path, &identity, &directory, Arc::new(Allowed));
    assert!(runtime::run_one(&worker, "recovered").unwrap());
    let stored = runtime::open(&replay_path).unwrap();
    let refused = stored
        .job(&identity.caller(None), &job.id)
        .unwrap()
        .unwrap();
    assert_eq!(refused.phase, Phase::Failed);
    assert!(refused.error.unwrap().contains("provenance"));
    assert!(stored
        .job_result(&identity.caller(None), &job.id)
        .unwrap()
        .is_none());
}

#[test]
fn parallel_claims_are_unique_and_revocation_and_cancellation_stop_work() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let bytes = request();
    let identity = host("owner", "stable");
    let directory = Directory::new(&[A]);
    let membership = directory.snapshot_now();
    for key in ["a", "b"] {
        runtime::submit(
            &path,
            &admission(&identity, key, Some(A), &membership),
            &bytes,
        )
        .unwrap();
    }
    let revoked = worker(&path, &identity, &directory, Arc::new(Revoked));
    assert!(runtime::run_one(&revoked, "revoked").is_err());
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let handles = (0..2)
        .map(|i| {
            let p = path.clone();
            let barrier = barrier.clone();
            let identity = identity.clone();
            std::thread::spawn(move || {
                let mut store = runtime::open(&p).unwrap();
                barrier.wait();
                store
                    .claim_job(
                        &identity.caller(None),
                        &format!("worker{i}"),
                        100,
                        1000,
                        &mut |_| true,
                    )
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
        .update_job(
            &identity.caller(None),
            &jobs[0].id,
            Event::Cancel,
            101,
            None,
        )
        .unwrap();
    assert!(store
        .update_job(
            &identity.caller(None),
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
        .claim_job(&identity.caller(None), "recovered", 1100, 1000, &mut |_| {
            true
        })
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(recovered.id, jobs[1].id);
    assert_eq!(recovered.attempts, 2);
    let mut different: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    different["jobs"][0]["transformer_name"] = json!("T2");
    assert!(runtime::submit(
        &path,
        &admission(&identity, "a", Some(A), &membership),
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
    let identity = host("owner", "stable");
    let directory = Directory::new(&[A, B]);
    let membership = directory.snapshot_now();
    // One batch per project, so the pool proves it serves both at once.
    for (key, project) in [("first", A), ("second", B)] {
        let mut bytes: serde_json::Value = serde_json::from_slice(&request()).unwrap();
        bytes["jobs"][0]["transformer_name"] = json!(key);
        runtime::submit(
            &path,
            &admission(&identity, key, Some(project), &membership),
            &serde_json::to_vec(&bytes).unwrap(),
        )
        .unwrap();
    }
    let observed = Arc::new(AtomicBool::new(false));
    let context = Arc::new(worker(
        &path,
        &identity,
        &directory,
        Arc::new(Bound(observed.clone())),
    ));
    let workers = runtime::Workers::start(context, 2).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let jobs = runtime::open(&path)
            .unwrap()
            .jobs(&identity.caller(None), 10)
            .unwrap();
        if jobs.iter().all(|job| job.phase == Phase::Completed) {
            assert_eq!(jobs.len(), 2);
            assert!(jobs.iter().all(|job| job.attempts == 1));
            let mut projects = jobs
                .iter()
                .map(|job| job.context.as_ref().unwrap().project.clone())
                .collect::<Vec<_>>();
            projects.sort();
            assert_eq!(projects, vec![A.to_owned(), B.to_owned()]);
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
