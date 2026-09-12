//! The durable compute lifecycle, end to end on a real store, under one
//! verified execution context per job.
//!
//! Two projects share one queue here and never see each other; a reused key
//! keeps its scope inside its project and is other work outside it; capacity
//! bounds admission and keeps a project's share honest; a claimed job runs
//! exactly what was admitted with no upstream anywhere; and a row written by a
//! released Server recovers from its own sealed bytes or is named unrunnable.

use ds_command_kernel::compute_jobs::{Event, Phase};
use ds_command_kernel::execution_context::{Limits, Principal};
use ds_compute_runtime::{self as runtime, Admission, Authorizer, HostIdentity, WorkerContext};
use ds_solar_contracts::{
    BundleBytes, CityIdentity, PreparedSolarCityInput, RunOptions, SnapshotRevision,
    SolarCityInput, WeatherDataset, WeatherKey, WeatherPin,
};
use serde_json::json;
use std::sync::Arc;

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
) -> Admission<'a> {
    Admission {
        identity,
        client: "test:1",
        key,
        requested_project: project,
        saved_project: None,
        limits: limits(),
        now_ms: runtime::now_ms(),
    }
}
fn worker(
    path: &std::path::Path,
    identity: &HostIdentity,
    auth: Arc<dyn Authorizer>,
) -> WorkerContext {
    WorkerContext {
        path: path.to_owned(),
        identity: identity.clone(),
        limits: limits(),
        auth,
        observer: None,
    }
}

/// The id a Server released before this scheme wrote for one key: the digest
/// of (owner, lane, key), with no project in it. Such ids can no longer be
/// derived from a key, which is why a legacy row is only ever reached
/// deliberately.
fn legacy_job_id(owner: &str, lane: &str, key: &str) -> String {
    runtime::digest(&serde_json::to_vec(&(owner, lane, key)).unwrap())
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
    let job = runtime::submit(&path, &admission(&identity, "request1", Some(A)), &bytes).unwrap();
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
    let context = worker(&path, &identity, Arc::new(Allowed));
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
        runtime::submit(&path, &admission(&identity, "request1", Some(A)), &bytes)
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
    let first =
        runtime::submit(&path, &admission(&identity, "for-a", Some(A)), &request()).unwrap();
    let mut other: serde_json::Value = serde_json::from_slice(&request()).unwrap();
    other["jobs"][0]["transformer_name"] = json!("T2");
    let second = runtime::submit(
        &path,
        &admission(&identity, "for-b", Some(B)),
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
    let context = worker(&path, &identity, Arc::new(Allowed));
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
fn a_key_keeps_its_scope_inside_its_project_and_is_other_work_outside_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "canary");
    let bytes = request();
    let job = runtime::submit(&path, &admission(&identity, "shared", Some(A)), &bytes).unwrap();
    // Same key, same bytes, same project: the same job, not a second one.
    let again = runtime::submit(&path, &admission(&identity, "shared", Some(A)), &bytes).unwrap();
    assert_eq!(again.id, job.id);
    assert_eq!(again.context, job.context);
    // The same key in another project is another piece of work, with its own
    // id and its own context: two of this owner's projects running the same
    // daily key never collide, and neither can reach the other's row.
    let elsewhere =
        runtime::submit(&path, &admission(&identity, "shared", Some(B)), &bytes).unwrap();
    assert_ne!(elsewhere.id, job.id);
    assert_eq!(elsewhere.context.as_ref().unwrap().project, B);
    assert!(runtime::open(&path)
        .unwrap()
        .job(&identity.caller(Some(A)), &elsewhere.id)
        .unwrap()
        .is_none());
    // Same key, other bytes.
    let mut other: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    other["jobs"][0]["transformer_name"] = json!("T2");
    let changed = runtime::submit(
        &path,
        &admission(&identity, "shared", Some(A)),
        &serde_json::to_vec(&other).unwrap(),
    )
    .unwrap_err();
    assert_eq!(changed.code(), Some("payload_changed_for_key"));
    // A project this host has never heard of is admitted on the owner's word:
    // there is no directory to consult, and whether that project's effects may
    // leave the machine is the gateway's answer at publication and sync.
    let unheard_of = runtime::submit(
        &path,
        &admission(&identity, "elsewhere", Some("project-z")),
        &bytes,
    )
    .unwrap();
    assert_eq!(unheard_of.context.as_ref().unwrap().project, "project-z");
    // Naming nothing at all is the one thing there is no answer to.
    let unnamed =
        runtime::submit(&path, &admission(&identity, "unnamed", None), &bytes).unwrap_err();
    assert_eq!(unnamed.code(), Some("project_required"));
    assert_eq!(
        runtime::open(&path)
            .unwrap()
            .jobs(&identity.caller(None), 10)
            .unwrap()
            .len(),
        3,
        "one job per (project, key); the refusals wrote nothing"
    );
}

#[test]
fn capacity_bounds_the_queue_per_project_and_globally_with_retry_guidance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "canary");
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
                ..admission(&identity, key, Some(project))
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
            &admission(&identity, key, Some(project)),
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
    let mut store = runtime::open(&path).unwrap();
    let mut claim = |worker: &str, limits: Limits| {
        store
            .claim_job(
                &identity.caller(None),
                worker,
                runtime::now_ms(),
                30_000,
                limits,
            )
            .unwrap()
            .map(|(job, _)| job.context.unwrap().project)
    };
    // A is first in queue order and takes the one slot its share allows. The
    // next claim skips A — its second batch is older than B's first — and
    // starts B, so a long queue in one project never idles the machine for
    // another. Then both hold their share and both still wait: neither can
    // yield to the other, so refusing would idle the pool, and queue order
    // decides — A's second batch is older, then B's.
    for (worker, expected) in [
        ("worker-1", A),
        ("worker-2", B),
        ("worker-3", A),
        ("worker-4", B),
    ] {
        assert_eq!(claim(worker, share).as_deref(), Some(expected), "{worker}");
    }
    // Every worker busy: nothing more is claimable until one finishes.
    assert_eq!(claim("worker-5", share), None);
    // The census the share is judged from is the store's own, read inside the
    // claim, so a live pool of exactly two admits exactly two.
    let store = runtime::open(&path).unwrap();
    assert_eq!(
        store
            .census("owner", "canary", runtime::now_ms())
            .unwrap()
            .running
            .len(),
        4
    );
}

/// What replaced the revocation this file used to prove. Revocation is not a
/// decision this host makes any more: the gateway answers it where a
/// publication or a sync leaves the machine. Here, both projects' work runs to
/// completion with no upstream in the process at all, and each keeps the
/// context it was admitted under.
#[test]
fn a_claimed_job_runs_exactly_what_was_admitted_and_asks_no_one_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "canary");
    let first =
        runtime::submit(&path, &admission(&identity, "for-a", Some(A)), &request()).unwrap();
    let mut other: serde_json::Value = serde_json::from_slice(&request()).unwrap();
    other["jobs"][0]["transformer_name"] = json!("T2");
    let second = runtime::submit(
        &path,
        &admission(&identity, "for-b", Some(B)),
        &serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();
    let context = worker(&path, &identity, Arc::new(Allowed));
    assert!(runtime::run_one(&context, "worker-1").unwrap());
    assert!(runtime::run_one(&context, "worker-2").unwrap());
    let store = runtime::open(&path).unwrap();
    for (job, project) in [(&first, A), (&second, B)] {
        let done = store
            .job(&identity.caller(Some(project)), &job.id)
            .unwrap()
            .unwrap();
        assert_eq!(done.phase, Phase::Completed, "{project}");
        assert!(store
            .job_result(&identity.caller(None), &job.id)
            .unwrap()
            .is_some());
        // Nothing re-scoped it between admission and execution.
        assert_eq!(done.context.unwrap().project, project);
    }
}

#[test]
fn a_transformer_row_written_by_a_released_server_is_named_and_never_claimed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "canary");
    let bytes = request();
    // Exactly what the released binary's INSERT wrote: no `context` member.
    let id = legacy_job_id("owner", "canary", "released");
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
    // A transformer row names no project and nothing outside its own bytes may
    // name one for it. It is named unrecoverable, and no worker runs it.
    let unrecovered = runtime::recover_contexts(&path, &identity).unwrap();
    assert_eq!(unrecovered.unrecoverable, vec![id.clone()]);
    let idle = worker(&path, &identity, Arc::new(Allowed));
    assert!(!runtime::run_one(&idle, "worker-0").unwrap());
    // Starting the pool changes nothing about it: recovery runs first, says
    // the same thing, and the row is still there, readable and unrun. Work
    // submitted afterwards runs past it.
    let workers =
        runtime::Workers::start(Arc::new(worker(&path, &identity, Arc::new(Allowed))), 1).unwrap();
    let mut other: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    other["jobs"][0]["transformer_name"] = json!("T2");
    let running = runtime::submit(
        &path,
        &admission(&identity, "after", Some(A)),
        &serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let job = runtime::open(&path)
            .unwrap()
            .job(&identity.caller(Some(A)), &running.id)
            .unwrap()
            .expect("the new job is in project A");
        if job.phase == Phase::Completed {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{job:?}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    drop(workers);
    let legacy = runtime::open(&path)
        .unwrap()
        .job(&identity.caller(None), &id)
        .unwrap()
        .expect("the released row is still readable by its own connection");
    assert_eq!(legacy.phase, Phase::Queued, "it was never claimed");
    assert!(legacy.context.is_none(), "and never given a project");
}

#[test]
fn prepared_solar_survives_reopen_without_a_client_path_or_live_request() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "stable");
    let mut request: serde_json::Value = serde_json::from_slice(&prepared_solar_request()).unwrap();
    request["provenance"] = valid_provenance();
    let bytes = serde_json::to_vec(&request).unwrap();
    assert!(runtime::submit(
        &path,
        &admission(&identity, "solar-request", Some(SOLAR_PROJECT)),
        &bytes
    )
    .is_err());
    let job = runtime::submit_solar(
        &path,
        // The caller names nothing: the sealed input names its own project.
        &admission(&identity, "solar-request", None),
        &bytes,
    )
    .unwrap();
    assert_eq!(job.input_tag, "ds.solar.calculate.prepared/v1");
    assert_eq!(job.context.as_ref().unwrap().project, SOLAR_PROJECT);
    // A caller who names a contradicting project is refused outright.
    let mismatch = runtime::submit_solar(
        &path,
        &admission(&identity, "solar-elsewhere", Some(A)),
        &bytes,
    )
    .unwrap_err();
    assert_eq!(mismatch.code(), Some("scope_mismatch"));
    // `run_one` reopens the durable store after submission. Its only input is
    // the retained request bytes, so this proves no client workspace/cache path
    // or original HTTP request remains in the execution boundary.
    let context = worker(&path, &identity, Arc::new(Allowed));
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
    let job = runtime::submit_solar_with_provenance(
        &path,
        &admission(&identity, "claim", None),
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
            runtime::submit_solar(&path, &admission(&identity, "invalid", None), &invalid)
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
    let mut job =
        runtime::submit_solar(&path, &admission(&identity, "valid", None), &valid).unwrap();
    let replay_path = dir.path().join("replay.sqlite");
    request["provenance"]["root"] = json!("another-root");
    let invalid = serde_json::to_vec(&request).unwrap();
    job.input_sha256 = runtime::digest(&invalid);
    let context = job.context.as_mut().unwrap();
    context.input_sha256 = job.input_sha256.clone();
    runtime::open(&replay_path)
        .unwrap()
        .submit_job(&job, &invalid, limits())
        .unwrap();
    let worker = worker(&replay_path, &identity, Arc::new(Allowed));
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
    for key in ["a", "b"] {
        runtime::submit(&path, &admission(&identity, key, Some(A)), &bytes).unwrap();
    }
    let revoked = worker(&path, &identity, Arc::new(Revoked));
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
                        limits(),
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
        .claim_job(&identity.caller(None), "recovered", 1100, 1000, limits())
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(recovered.id, jobs[1].id);
    assert_eq!(recovered.attempts, 2);
    let mut different: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    different["jobs"][0]["transformer_name"] = json!("T2");
    assert!(runtime::submit(
        &path,
        &admission(&identity, "a", Some(A)),
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
    // One batch per project, so the pool proves it serves both at once.
    for (key, project) in [("first", A), ("second", B)] {
        let mut bytes: serde_json::Value = serde_json::from_slice(&request()).unwrap();
        bytes["jobs"][0]["transformer_name"] = json!(key);
        runtime::submit(
            &path,
            &admission(&identity, key, Some(project)),
            &serde_json::to_vec(&bytes).unwrap(),
        )
        .unwrap();
    }
    let observed = Arc::new(AtomicBool::new(false));
    let context = Arc::new(worker(&path, &identity, Arc::new(Bound(observed.clone()))));
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

#[test]
fn resubmitting_a_released_rows_key_under_a_project_is_new_work_beside_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let identity = host("owner", "canary");
    let bytes = request();
    // A transformer row a released Server wrote: no context, and nothing that
    // may name a project for it.
    let id = legacy_job_id("owner", "canary", "released");
    let row = json!({
        "id": id, "owner": "owner", "lane": "canary", "input_sha256": runtime::digest(&bytes),
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
    assert_eq!(
        runtime::recover_contexts(&path, &identity)
            .unwrap()
            .unrecoverable,
        vec![id.clone()]
    );
    // The remedy the contract states, carried out: the same key, now under an
    // explicit project. Its id digests the project, so this is a NEW job that
    // runs -- the released row is not adopted, not re-scoped and not touched.
    let resubmitted =
        runtime::submit(&path, &admission(&identity, "released", Some(A)), &bytes).unwrap();
    assert_ne!(
        resubmitted.id, id,
        "a released row's id is never re-derived"
    );
    let context = resubmitted.context.clone().unwrap();
    assert_eq!(context.project, A);
    assert_eq!(context.idempotency_key, "released");
    assert_eq!(
        runtime::open(&path)
            .unwrap()
            .jobs(&identity.caller(None), 10)
            .unwrap()
            .len(),
        2,
        "the released row stands beside the new job, not under it"
    );
    let worker = worker(&path, &identity, Arc::new(Allowed));
    assert!(runtime::run_one(&worker, "worker-1").unwrap());
    let done = runtime::open(&path)
        .unwrap()
        .job(&identity.caller(Some(A)), &resubmitted.id)
        .unwrap()
        .unwrap();
    assert_eq!(done.phase, Phase::Completed);
    assert_eq!(done.context.unwrap().project, A);
    // And the released row is still exactly what it was.
    let legacy = runtime::open(&path)
        .unwrap()
        .job(&identity.caller(None), &id)
        .unwrap()
        .unwrap();
    assert_eq!(legacy.phase, Phase::Queued);
    assert!(legacy.context.is_none());
    assert!(!runtime::run_one(&worker, "worker-2").unwrap());
}
