//! The durable host runs detached Network engineering and retains its admitted
//! inputs/context across execution, missing-component failures and cancellation.
use ds_command_kernel::{
    compute_jobs::{EngineKind, Event, Phase},
    execution_context::{Limits, Principal},
};
use ds_compute_runtime::{self as runtime, Admission, Authorizer, HostIdentity, WorkerContext};
use serde_json::json;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

struct Allowed;
impl Authorizer for Allowed {
    fn authorize(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}
fn identity() -> HostIdentity {
    HostIdentity {
        owner: "owner".into(),
        principal: Principal {
            uid: "uid-a".into(),
            lane: "canary".into(),
            deployment: "https://gateway.example".into(),
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
fn admit<'a>(identity: &'a HostIdentity, key: &'a str) -> Admission<'a> {
    Admission {
        identity,
        client: "runtime-boundary",
        key,
        requested_project: Some("project-a"),
        limits: limits(),
        now_ms: runtime::now_ms(),
    }
}
fn worker(path: &Path, identity: &HostIdentity) -> WorkerContext {
    WorkerContext {
        path: path.to_owned(),
        identity: identity.clone(),
        limits: limits(),
        auth: Arc::new(Allowed),
        observer: None,
    }
}
fn request() -> Vec<u8> {
    serde_json::to_vec(&json!({"schema": "ds.lv.process.request/v1", "jobs": [{
        "transformer_name": "T1", "gdfs": {
            "tr": {"type": "FeatureCollection", "features": [{
                "type": "Feature", "id": "tr-1",
                "geometry": {"type": "Point", "coordinates": [30.0, -2.0]},
                "properties": {"name": "T1", "names": "T1"}
            }]}
        }, "settings": {}
    }]}))
    .unwrap()
}
fn required(name: &str) -> PathBuf {
    PathBuf::from(
        env::var_os(name).unwrap_or_else(|| panic!("{name} must name an acceptance input")),
    )
}

#[test]
fn current_schema_engine_name_and_admission_pin_are_persisted_together() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("store.sqlite");
    let identity = identity();
    let input = request();
    let job = runtime::submit(&path, &admit(&identity, "new-schema"), &input).unwrap();
    assert_eq!(job.engine, EngineKind::LvProcessing);
    assert_eq!(job.input_tag, "ds.lv.process.request/v1");
    assert_eq!(
        serde_json::to_value(&job).unwrap()["engine"],
        "lv_processing"
    );
    assert_eq!(
        runtime::open(&path)
            .unwrap()
            .job_input(&identity.caller(Some("project-a")), &job.id)
            .unwrap()
            .unwrap(),
        input
    );
    assert_eq!(job.phase, Phase::Queued);
    assert!(job.engine_pin.is_some());
}

#[test]
fn a_replayed_input_cannot_switch_its_admitted_schema() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.sqlite");
    let replay = root.path().join("replay.sqlite");
    let identity = identity();
    let input = request();
    let mut job = runtime::submit(&source, &admit(&identity, "tag-fence"), &input).unwrap();
    let context = job.context.clone();
    // Reproduce a row corrupted beyond admission, without changing the
    // input digest or scope. A historical tag cannot relabel current bytes.
    job.input_tag = "ds.fast-lv.request/v1".into();
    runtime::open(&replay)
        .unwrap()
        .submit_job(&job, &input, limits())
        .unwrap();
    assert!(runtime::run_one(&worker(&replay, &identity), "worker").unwrap());
    let store = runtime::open(&replay).unwrap();
    let retained = store
        .job(&identity.caller(Some("project-a")), &job.id)
        .unwrap()
        .unwrap();
    assert_eq!(retained.phase, Phase::Failed);
    assert_eq!(retained.context, context);
    assert_eq!(
        retained.error.as_deref(),
        Some("durable LV processing input tag mismatch")
    );
    assert_eq!(
        store
            .job_input(&identity.caller(Some("project-a")), &job.id)
            .unwrap()
            .unwrap(),
        input
    );
    assert!(store
        .job_result(&identity.caller(Some("project-a")), &job.id)
        .unwrap()
        .is_none());
}

#[test]
#[ignore = "requires the actual insertion-profile owner and captured pre-extraction goldens"]
fn detached_runtime_preserves_historical_batch_bytes_and_context() {
    let fixtures = required("DS_COMPUTE_NETWORK_FIXTURES");
    let goldens = required("DS_COMPUTE_NETWORK_GOLDENS");
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("store.sqlite");
    let identity = identity();
    for name in ["mixed", "approved"] {
        let bytes = fs::read(fixtures.join(format!("{name}.request.json"))).unwrap();
        let job = runtime::submit(&path, &admit(&identity, name), &bytes).unwrap();
        assert_eq!(job.input_tag, "ds.fast-lv.request/v1");
        let context = job.context.clone();
        assert!(runtime::run_one(&worker(&path, &identity), name).unwrap());
        let store = runtime::open(&path).unwrap();
        let finished = store
            .job(&identity.caller(Some("project-a")), &job.id)
            .unwrap()
            .unwrap();
        assert_eq!(finished.phase, Phase::Completed);
        assert_eq!(finished.context, context);
        assert_eq!(
            store
                .job_input(&identity.caller(Some("project-a")), &job.id)
                .unwrap()
                .unwrap(),
            bytes
        );
        assert_eq!(
            store
                .job_result(&identity.caller(Some("project-a")), &job.id)
                .unwrap()
                .unwrap(),
            fs::read(goldens.join(format!("{name}.original-cli.insertion.result.json"))).unwrap()
        );
        assert!(!runtime::run_one(&worker(&path, &identity), "no-recompute").unwrap());
    }
    let bytes = request();
    let job = runtime::submit(&path, &admit(&identity, "current-schema"), &bytes).unwrap();
    assert!(runtime::run_one(&worker(&path, &identity), "current-schema").unwrap());
    let result = runtime::open(&path)
        .unwrap()
        .job_result(&identity.caller(Some("project-a")), &job.id)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&result).unwrap()["schema"],
        "ds.lv.process.result/v1"
    );
}

fn isolated(engine: &Path, mode: &str, marker: Option<&Path>) {
    let mut command = Command::new(env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "isolated_failure_or_cancel",
            "--ignored",
            "--nocapture",
        ])
        .env("DS_NETWORK_ENGINE_BIN", engine)
        .env("DS_COMPUTE_CASE", mode);
    if let Some(marker) = marker {
        command.env("DS_COMPUTE_MARKER", marker);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "isolated case failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
#[ignore = "executes the missing-component path in a separate process"]
fn missing_retained_component_fails_execution_and_retains_durable_work() {
    let root = tempfile::tempdir().unwrap();
    let retained = root.path().join("retained-network");
    fs::copy(required("DS_NETWORK_ENGINE_BIN"), &retained).unwrap();
    isolated(&retained, "missing", None);
}

#[cfg(unix)]
#[test]
#[ignore = "requires the actual compatible owner; exercises a hung component and process cancellation"]
fn cancelling_a_running_component_keeps_the_terminal_context_and_input() {
    use std::os::unix::fs::PermissionsExt;
    fn quote(path: &Path) -> String {
        format!("'{}'", path.to_str().unwrap().replace('\'', "'\"'\"'"))
    }
    let owner = required("DS_NETWORK_ENGINE_BIN");
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("started");
    let fixture = root.path().join("network-hung");
    let script = format!("#!/bin/sh\nif [ \"$1\" = build-info ]; then exec {} \"$@\"; fi\nprintf started > {}\nexec sleep 30\n", quote(&owner), quote(&marker));
    fs::write(&fixture, script).unwrap();
    fs::set_permissions(&fixture, fs::Permissions::from_mode(0o700)).unwrap();
    isolated(&fixture, "cancel", Some(&marker));
}

#[test]
#[ignore = "internal isolated acceptance helper"]
fn isolated_failure_or_cancel() {
    use std::time::{Duration, Instant};
    let mode = env::var("DS_COMPUTE_CASE").expect("explicit case");
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("store.sqlite");
    let identity = identity();
    let input = request();
    let job = runtime::submit(&path, &admit(&identity, "retained"), &input).unwrap();
    let context = job.context.clone();
    if mode == "cancel" {
        let worker = worker(&path, &identity);
        let run = std::thread::spawn(move || runtime::run_one(&worker, "worker"));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !required("DS_COMPUTE_MARKER").exists() {
            assert!(Instant::now() < deadline, "component did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
        let started = Instant::now();
        runtime::open(&path)
            .unwrap()
            .update_job(
                &identity.caller(Some("project-a")),
                &job.id,
                Event::Cancel,
                runtime::now_ms(),
                None,
            )
            .unwrap();
        assert!(run.join().unwrap().unwrap());
        assert!(
            started.elapsed() < Duration::from_secs(6),
            "hung owner was not promptly reaped"
        );
    } else {
        assert_eq!(mode, "missing");
        fs::remove_file(required("DS_NETWORK_ENGINE_BIN")).unwrap();
        assert!(runtime::run_one(&worker(&path, &identity), "worker").unwrap());
    }
    let store = runtime::open(&path).unwrap();
    let retained = store
        .job(&identity.caller(Some("project-a")), &job.id)
        .unwrap()
        .unwrap();
    assert_eq!(retained.context, context);
    assert_eq!(
        retained.phase,
        if mode == "cancel" {
            Phase::Cancelled
        } else {
            Phase::Failed
        }
    );
    if mode == "missing" {
        assert!(retained
            .error
            .unwrap()
            .starts_with("network_engine_missing:"));
    }
    assert_eq!(
        store
            .job_input(&identity.caller(Some("project-a")), &job.id)
            .unwrap()
            .unwrap(),
        input
    );
    assert!(store
        .job_result(&identity.caller(Some("project-a")), &job.id)
        .unwrap()
        .is_none());
}
