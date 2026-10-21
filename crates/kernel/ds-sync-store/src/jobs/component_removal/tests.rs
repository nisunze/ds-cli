use super::*;
use ds_command_kernel::compute_jobs::{EngineKind, EngineLane, EnginePin, EnginePinSource};
use std::{cell::Cell, path::Path, sync::mpsc, thread, time::Duration};

fn store(path: &Path) -> Store {
    let store = Store::open(path).unwrap();
    store.initialize_jobs().unwrap();
    store
}

fn caller() -> JobCaller<'static> {
    JobCaller::connection("owner", "user", "stable", "https://gateway.example")
}

fn limits() -> Limits {
    Limits {
        global_running: 2,
        per_project_running: 1,
        global_queued: 8,
        per_project_queued: 4,
    }
}

fn job(index: usize) -> Job {
    let id = format!("{index:064x}");
    let context = ExecutionContext {
        principal_uid: "user".into(),
        lane: "stable".into(),
        deployment: "https://gateway.example".into(),
        install_id: "install".into(),
        project: "project".into(),
        client: "core-component-test".into(),
        operation: execution_context::SOLAR_PROCESSING.into(),
        job_id: id.clone(),
        idempotency_key: format!("key-{index}"),
        input_sha256: "a".repeat(64),
        admitted_at_ms: 1,
    };
    let pin = EnginePin {
        schema: "ds.compute.engine-pin/v1".into(),
        engine: EngineKind::SolarPrepared,
        owner: "owner".into(),
        lane: EngineLane::Stable,
        context: context.clone(),
        source: EnginePinSource::NativeLaneCapture,
        executable: "/retained/immutable/solar".into(),
        executable_sha256: "b".repeat(64),
        identity: serde_json::json!({"retained": true}),
        workers: 1,
        development_override: false,
    };
    Job {
        id,
        owner: "owner".into(),
        lane: "stable".into(),
        input_sha256: context.input_sha256.clone(),
        engine: EngineKind::SolarPrepared,
        input_tag: "ds.solar.calculate.prepared/v1".into(),
        phase: Phase::Queued,
        attempts: 0,
        created_at_ms: 1,
        updated_at_ms: 1,
        worker: None,
        lease_until_ms: 0,
        result_sha256: None,
        error: None,
        context: Some(context),
        engine_pin: Some(pin),
    }
}

fn insert(store: &Store, job: &Job) {
    let phase = serde_json::to_value(&job.phase).unwrap();
    store.connection.execute(
        "INSERT INTO compute_jobs(id,owner,lane,phase,lease_until,created,row,input) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![job.id, job.owner, job.lane, phase.as_str().unwrap(), job.lease_until_ms, job.created_at_ms, serde_json::to_string(job).unwrap(), b"retained input"],
    ).unwrap();
}

fn observe(store: &mut Store, component: ModuleComponent) -> Result<()> {
    store.with_core_component_removal(&caller(), component, |guard| {
        guard.admit().map_err(Error::Kernel)
    })
}

#[test]
fn captured_queued_running_and_reopened_jobs_keep_their_exact_pin_and_input() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("store.sqlite");
    let mut database = store(&path);
    let queued = job(1);
    database
        .submit_job(&queued, b"captured input", limits())
        .unwrap();
    let mut running = job(2);
    running.phase = Phase::Running;
    running.worker = Some("captured-worker".into());
    running.lease_until_ms = 0; // Reopened/expired work keeps its old binary too.
    insert(&database, &running);
    drop(database);
    let mut database = store(&path);
    observe(&mut database, ModuleComponent::Solar).unwrap();
    assert_eq!(
        database.job(&caller(), &queued.id).unwrap(),
        Some(queued.clone())
    );
    assert_eq!(database.job(&caller(), &running.id).unwrap(), Some(running));
    assert_eq!(
        database.job_input(&caller(), &queued.id).unwrap().unwrap(),
        b"captured input"
    );
}

#[test]
fn missing_and_crossed_pending_pins_refuse_without_removal_or_job_mutation() {
    let mutations: [fn(&mut Job); 6] = [
        |job| job.engine_pin = None,
        |job| job.engine_pin.as_mut().unwrap().owner = "another-owner".into(),
        |job| job.engine_pin.as_mut().unwrap().lane = EngineLane::Canary,
        |job| job.engine_pin.as_mut().unwrap().context.project = "other-project".into(),
        |job| job.engine_pin.as_mut().unwrap().schema = "unknown".into(),
        |job| job.context = None,
    ];
    for (index, mutate) in mutations.into_iter().enumerate() {
        let directory = tempfile::tempdir().unwrap();
        let mut database = store(&directory.path().join("store.sqlite"));
        let mut pending = job(index);
        mutate(&mut pending);
        if index % 2 == 1 {
            pending.phase = Phase::Running;
        }
        insert(&database, &pending);
        let called = Cell::new(false);
        let error = database
            .with_core_component_removal(&caller(), ModuleComponent::Solar, |_| {
                called.set(true);
                Ok(())
            })
            .unwrap_err();
        let Error::Refused(refusal) = error else {
            panic!("expected typed refusal")
        };
        assert_eq!(refusal.code, "component_uncaptured_jobs");
        assert!(refusal.message.contains("cancel"));
        assert!(!called.get());
        assert_eq!(
            database.job(&caller(), &pending.id).unwrap(),
            Some(pending.clone())
        );
        assert_eq!(
            database.job_input(&caller(), &pending.id).unwrap().unwrap(),
            b"retained input"
        );
        database
            .update_job(&caller(), &pending.id, Event::Cancel, 10, None)
            .unwrap();
        observe(&mut database, ModuleComponent::Solar).unwrap();
    }
}

#[test]
fn other_owners_lanes_components_and_terminal_rows_do_not_block_removal() {
    let directory = tempfile::tempdir().unwrap();
    let mut database = store(&directory.path().join("store.sqlite"));
    let mut other_owner = job(1);
    other_owner.owner = "other-owner".into();
    other_owner.context.as_mut().unwrap().principal_uid = "other-user".into();
    let mut other_lane = job(2);
    other_lane.lane = "canary".into();
    let mut other_component = job(3);
    other_component.engine = EngineKind::LvProcessing;
    let mut terminal = job(4);
    terminal.phase = Phase::Completed;
    for mut retained in [other_owner, other_lane, other_component, terminal] {
        retained.engine_pin = None;
        insert(&database, &retained);
    }
    observe(&mut database, ModuleComponent::Solar).unwrap();
    observe(&mut database, ModuleComponent::Reporter).unwrap();
    assert!(observe(&mut database, ModuleComponent::Network).is_err());
}

#[test]
fn contextless_legacy_rows_beyond_ui_pages_refuse_even_with_project_narrowing() {
    let directory = tempfile::tempdir().unwrap();
    let mut database = store(&directory.path().join("store.sqlite"));
    let mut legacy = job(0);
    legacy.context = None;
    legacy.engine_pin = None;
    for index in 1..=1001 {
        insert(&database, &job(index));
    }
    insert(&database, &legacy);
    assert!(database
        .jobs(&caller(), 1000)
        .unwrap()
        .iter()
        .all(|job| job.id != legacy.id));
    let project_caller = caller().about(Some("project"));
    assert_eq!(database.jobs(&project_caller, 1000).unwrap().len(), 1000);
    assert!(database.job(&project_caller, &legacy.id).unwrap().is_none());
    let error = database
        .with_core_component_removal::<()>(&project_caller, ModuleComponent::Solar, |_| {
            panic!("legacy work must fence the selection callback")
        })
        .unwrap_err();
    assert!(
        matches!(error, Error::Refused(refusal) if refusal.code == "component_uncaptured_jobs")
    );
}

#[test]
fn historic_contextless_network_engine_alias_is_included_in_the_census() {
    let directory = tempfile::tempdir().unwrap();
    let mut database = store(&directory.path().join("store.sqlite"));
    let mut legacy = job(1);
    legacy.engine = EngineKind::LvProcessing;
    legacy.context = None;
    legacy.engine_pin = None;
    insert(&database, &legacy);
    let mut row = serde_json::to_value(&legacy).unwrap();
    row["engine"] = serde_json::json!("fast_lv");
    database
        .connection
        .execute(
            "UPDATE compute_jobs SET row=?1 WHERE id=?2",
            params![serde_json::to_string(&row).unwrap(), legacy.id],
        )
        .unwrap();
    assert!(matches!(
        observe(&mut database, ModuleComponent::Network),
        Err(Error::Refused(refusal)) if refusal.code == "component_uncaptured_jobs"
    ));
    observe(&mut database, ModuleComponent::Solar).unwrap();
    assert_eq!(database.job(&caller(), &legacy.id).unwrap(), Some(legacy));
}

#[test]
fn removal_holds_admission_until_selection_is_committed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("store.sqlite");
    let mut removal_store = store(&path);
    let mut submit_store = store(&path);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    let (captured_tx, captured_rx) = mpsc::channel();
    let removal = thread::spawn(move || {
        removal_store
            .with_core_component_removal(&caller(), ModuleComponent::Solar, |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                Ok(()) // The selection replacement completes before writer release.
            })
            .unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let submission = thread::spawn(move || {
        let pending = job(1);
        started_tx.send(()).unwrap();
        submit_store.submit_job_capturing_pin(&pending, b"new input", limits(), || {
            captured_tx.send(()).unwrap();
            Err(Error::Kernel("module_removed".into()))
        })
    });
    started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(matches!(
        captured_rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    release_tx.send(()).unwrap();
    removal.join().unwrap();
    captured_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(
        matches!(submission.join().unwrap(), Err(Error::Kernel(code)) if code == "module_removed")
    );
    assert!(store(&path).job(&caller(), &job(1).id).unwrap().is_none());
}

#[test]
fn submission_capture_and_retention_finish_before_racing_removal() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("store.sqlite");
    let mut submit_store = store(&path);
    let mut removal_store = store(&path);
    let (captured_tx, captured_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    let (removed_tx, removed_rx) = mpsc::channel();
    let submission = thread::spawn(move || {
        let pending = job(1);
        submit_store
            .submit_job_capturing_pin(&pending, b"new input", limits(), || {
                captured_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                Ok(pending.engine_pin.clone())
            })
            .unwrap()
    });
    captured_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let removal = thread::spawn(move || {
        started_tx.send(()).unwrap();
        removal_store
            .with_core_component_removal(&caller(), ModuleComponent::Solar, |_| {
                removed_tx.send(()).unwrap();
                Ok(())
            })
            .unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(matches!(
        removed_rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    release_tx.send(()).unwrap();
    let retained = submission.join().unwrap();
    removal.join().unwrap();
    removed_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(
        store(&path).job(&caller(), &retained.id).unwrap(),
        Some(retained)
    );
}

#[test]
fn callback_and_row_read_errors_release_writer_without_changing_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("store.sqlite");
    let mut database = store(&path);
    let error = database
        .with_core_component_removal::<()>(&caller(), ModuleComponent::Solar, |_| {
            Err(Error::Kernel("module_selection_stale".into()))
        })
        .unwrap_err();
    assert!(matches!(error, Error::Kernel(code) if code == "module_selection_stale"));
    store(&path)
        .submit_job(&job(1), b"input", limits())
        .unwrap();
    database
        .connection
        .execute("UPDATE compute_jobs SET row='unreadable'", [])
        .unwrap();
    assert!(matches!(
        observe(&mut database, ModuleComponent::Solar),
        Err(Error::Row(_))
    ));
    let other = store(&path);
    other
        .connection
        .execute("DELETE FROM compute_jobs", [])
        .unwrap();
    observe(&mut database, ModuleComponent::Solar).unwrap();
}
