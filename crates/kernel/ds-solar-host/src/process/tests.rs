#[path = "../../../../../tests/common/solar_opened_fixture_tests.rs"]
mod solar_opened_fixtures;

use super::*;
use ds_solar_contracts::{
    CityIdentity, PreparedSolarCityInput, RunOptions, SnapshotRevision, SolarCityInput, WeatherKey,
    WeatherPin,
};
use std::{os::unix::fs::PermissionsExt, sync::OnceLock};

pub(super) fn fake(root: &Path, body: &str) -> PathBuf {
    let path = root.join("fake-solar");
    fs::write(
        &path,
        format!("#!/usr/bin/python3\nimport os, sys, json, time, shutil\n{body}\n"),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

/// The exact `solar` addon these process tests drive. An explicit
/// `DS_SOLAR_ENGINE_BIN` selects a prebuilt engine; otherwise Cargo builds the
/// addon from this workspace into the caller's target directory, so the suite
/// never depends on a manually prepared or stale out-of-tree executable.
pub(super) fn engine_under_test() -> &'static Path {
    static ENGINE: OnceLock<PathBuf> = OnceLock::new();
    ENGINE.get_or_init(|| {
        if let Some(path) = std::env::var_os("DS_SOLAR_ENGINE_BIN") {
            return PathBuf::from(path);
        }
        let manifest =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/Cargo.toml");
        let output = Command::new(env!("CARGO"))
            .args(["build", "--package", "ds-solar-addon", "--bin", "solar"])
            .arg("--message-format=json-render-diagnostics")
            .arg("--manifest-path")
            .arg(manifest)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "building the solar addon failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let executables: Vec<PathBuf> = output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
            .filter(|message| {
                message["reason"] == "compiler-artifact"
                    && message["target"]["name"] == "solar"
                    && message["target"]["kind"] == serde_json::json!(["bin"])
            })
            .filter_map(|message| message["executable"].as_str().map(PathBuf::from))
            .collect();
        assert_eq!(
            executables.len(),
            1,
            "one solar executable: {executables:?}"
        );
        executables.into_iter().next().unwrap()
    })
}

pub(super) fn identity() -> EngineIdentity {
    let mut identity = EngineIdentity::current();
    identity.schemas = EngineIdentity::stamped_schema_inventory();
    identity
}

#[test]
fn engine_without_worker_budget_refuses_before_calculation() {
    let root = tempfile::tempdir().unwrap();
    let mut old = identity();
    old.schemas.retain(|s| s != "ds-solar.worker-budget/v1");
    let script = fake(
        root.path(),
        &format!(
            "assert sys.argv == [sys.argv[0], 'build-info']\nprint({:?})",
            serde_json::to_string(&old).unwrap()
        ),
    );
    let error = PreparedEngine::capture(&script, root.path(), 2, &CancelToken::new())
        .err()
        .unwrap();
    assert_eq!(error.kind, ProcessFailureKind::InvalidIdentity);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn cancellation_kills_and_reaps_worker_before_returning() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("worker-pid");
    let script = fake(
        root.path(),
        "open('worker-pid','w').write(str(os.getpid()))\nwhile True: time.sleep(1)",
    );
    let cancel = CancelToken::new();
    let signal = cancel.clone();
    let watched = marker.clone();
    let interrupter = thread::spawn(move || {
        let started = Instant::now();
        while !watched.exists() {
            assert!(started.elapsed() < Duration::from_secs(3));
            thread::sleep(Duration::from_millis(10));
        }
        signal.cancel();
    });
    let error = run_task(
        &script,
        root.path(),
        1,
        Task::Calculate {
            request: &root.path().join("input"),
            out: &root.path().join("out"),
        },
        &cancel,
        Duration::from_secs(5),
    )
    .unwrap_err();
    interrupter.join().unwrap();
    assert_eq!(error, "solar_process_cancelled");
    let pid: i32 = fs::read_to_string(marker).unwrap().parse().unwrap();
    // SAFETY: signal zero only checks existence; wait already reaped the child.
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

#[test]
fn process_deadline_log_and_output_limits_refuse() {
    for (body, expected, deadline) in [
        (
            "while True: time.sleep(1)",
            "solar_process_deadline_exceeded",
            Duration::from_millis(80),
        ),
        (
            "sys.stdout.write('x' * 100000); sys.stdout.flush()\nwhile True: time.sleep(1)",
            "solar_process_log_bound_exceeded",
            Duration::from_secs(3),
        ),
        (
            "open('oversized','wb').truncate(268435457)\nwhile True: time.sleep(1)",
            "solar_process_output_bound_exceeded",
            Duration::from_secs(3),
        ),
        (
            "open('request.json','wb').truncate(67108865)\nwhile True: time.sleep(1)",
            "solar_process_output_bound_exceeded",
            Duration::from_secs(3),
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let script = fake(root.path(), body);
        assert_eq!(
            run_task(
                &script,
                root.path(),
                1,
                Task::Calculate {
                    request: &root.path().join("input"),
                    out: &root.path().join("out")
                },
                &CancelToken::new(),
                deadline
            )
            .unwrap_err(),
            expected
        );
    }
}

#[test]
fn captured_executable_change_is_refused_before_task_spawn() {
    let root = tempfile::tempdir().unwrap();
    let script = fake(
        root.path(),
        &format!("print({:?})", serde_json::to_string(&identity()).unwrap()),
    );
    let engine = PreparedEngine::capture(&script, root.path(), 1, &CancelToken::new()).unwrap();
    fs::write(
        &script,
        b"#!/usr/bin/python3\nraise Exception('must not execute')\n",
    )
    .unwrap();
    let error = engine
        .calculate(&prepared_request(), &CancelToken::new())
        .err()
        .unwrap();
    assert_eq!(error.kind, ProcessFailureKind::Changed);
}

fn prepared_request() -> PreparedCalculationRequest {
    prepared_request_city("aderm_bere")
}

pub(super) fn prepared_request_city(city: &str) -> PreparedCalculationRequest {
    let snapshot = solar_opened_fixtures::snapshot(city);
    let input = SolarCityInput::from_city_snapshot(
        CityIdentity {
            project_id: "ds-solar-fixtures".into(),
            root: "solar".into(),
            city_id: city.into(),
            display_name: snapshot["_root"]["display_name"].as_str().unwrap().into(),
        },
        SnapshotRevision {
            source: "frozen-fixture".into(),
            captured_at: "2026-08-08T00:00:00Z".into(),
            record_updated_at_ms: None,
            revision: None,
        },
        RunOptions::default(),
        &snapshot,
    )
    .unwrap();
    let (weather, reference) = solar_opened_fixtures::opened(city);
    let weather_key = WeatherKey::new(
        input.site.latitude,
        input.site.longitude,
        weather.provenance.timezone.clone(),
        WeatherPin::pvgis_tmy(),
    )
    .unwrap();
    PreparedCalculationRequest {
        prepared: PreparedSolarCityInput::commit(
            input,
            weather_key,
            weather,
            reference,
            "2026-08-20T00:00:00Z",
        )
        .unwrap(),
        render_charts: false,
        run_id: Some("process-fixture".into()),
    }
}

struct ActualFixture {
    root: tempfile::TempDir,
    request: PreparedCalculationRequest,
    task: Vec<u8>,
    identity: EngineIdentity,
}
fn actual_fixture() -> &'static ActualFixture {
    static FIXTURE: OnceLock<ActualFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let root = tempfile::tempdir().unwrap();
        let request = prepared_request();
        let bytes = serde_json::to_vec(&request).unwrap();
        let input = root.path().join("input.json");
        fs::write(&input, bytes).unwrap();
        let binary = engine_under_test();
        let identity = engine_identity(binary, &CancelToken::new()).unwrap();
        run_task(
            binary,
            root.path(),
            1,
            Task::Calculate {
                request: &input,
                out: &root.path().join("out"),
            },
            &CancelToken::new(),
            CALCULATION_DEADLINE,
        )
        .unwrap();
        let task = fs::read(root.path().join("out/task-result.json")).unwrap();
        ActualFixture {
            root,
            request,
            task,
            identity,
        }
    })
}

#[test]
fn actual_addon_result_and_exact_artifacts_verify_without_linked_engine() {
    let fixture = actual_fixture();
    let task = serde_json::from_slice(&fixture.task).unwrap();
    let outcome = verify_task(
        task,
        &fixture.request,
        &serde_json::to_vec(&fixture.request).unwrap(),
        &fixture.identity,
        &fixture.root.path().join("out"),
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(outcome.result.run_id, fixture.request.run_id);
    assert_eq!(outcome.outputs.len(), 1);
    assert_eq!(outcome.outputs[0].entry.name, "report_input.json");
}

#[test]
fn actual_addon_charts_and_documents_preserve_the_closed_manifest() {
    let root = tempfile::tempdir().unwrap();
    let mut request = prepared_request();
    request.render_charts = true;
    let outcome = PreparedEngine::capture(engine_under_test(), root.path(), 1, &CancelToken::new())
        .unwrap()
        .calculate(&request, &CancelToken::new())
        .unwrap();
    assert!(outcome.outputs.len() > 1);
    assert_eq!(
        outcome.result.artifacts.artifacts.len() + 1,
        outcome.outputs.len()
    );
    assert!(outcome.plots_generated);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn mismatched_request_engine_scope_digest_and_artifacts_are_refused() {
    let fixture = actual_fixture();
    for (pointer, value) in [
        ("/request_sha256", "sha256:wrong"),
        ("/prepared_input_digest", "sha256:wrong"),
        ("/engine/version", "other"),
        ("/result/identity/project_id", "another-project"),
        ("/result/run_id", "another-run"),
        ("/result/result_digest", "sha256:wrong"),
        ("/outputs/0/content_digest", "sha256:wrong"),
        ("/outputs/0/name", "../escape"),
    ] {
        let mut task: serde_json::Value = serde_json::from_slice(&fixture.task).unwrap();
        *task.pointer_mut(pointer).unwrap() = serde_json::json!(value);
        assert!(
            verify_task(
                serde_json::from_value(task).unwrap(),
                &fixture.request,
                &serde_json::to_vec(&fixture.request).unwrap(),
                &fixture.identity,
                &fixture.root.path().join("out"),
                &CancelToken::new()
            )
            .is_err(),
            "{pointer}"
        );
    }
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        verify_task(
            serde_json::from_slice(&fixture.task).unwrap(),
            &fixture.request,
            &serde_json::to_vec(&fixture.request).unwrap(),
            &fixture.identity,
            &fixture.root.path().join("out"),
            &cancel
        )
        .err()
        .unwrap(),
        "solar_process_cancelled"
    );
}

fn change_bytes_preserving_metadata(path: &Path) {
    let metadata = fs::metadata(path).unwrap();
    let bytes = fs::read(path).unwrap();
    let changed = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("# stamp:0", "# stamp:1");
    assert_ne!(bytes, changed.as_bytes());
    assert_eq!(bytes.len(), changed.len());
    fs::write(path, changed).unwrap();
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(metadata.modified().unwrap()))
        .unwrap();
    let after = fs::metadata(path).unwrap();
    assert_eq!(metadata.len(), after.len());
    assert_eq!(metadata.modified().unwrap(), after.modified().unwrap());
}

#[test]
fn executable_digest_refuses_same_metadata_changes_and_oversized_files() {
    let root = tempfile::tempdir().unwrap();
    let path = fake(root.path(), "# stamp:0");
    let stamp = executable_stamp(&path).unwrap();
    change_bytes_preserving_metadata(&path);
    assert_eq!(
        verify_executable(&path, &stamp).unwrap_err(),
        "solar_engine_changed_during_job"
    );
    let oversized = root.path().join("oversized");
    File::create(&oversized)
        .unwrap()
        .set_len(MAX_EXECUTABLE_BYTES + 1)
        .unwrap();
    assert_eq!(
        executable_stamp(&oversized).err().unwrap(),
        "solar_engine_executable_too_large"
    );
    let link = root.path().join("link");
    std::os::unix::fs::symlink(path, &link).unwrap();
    assert!(executable_stamp(&link).is_err());
}

#[test]
fn queued_same_metadata_change_refuses_before_spawn() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("spawned");
    let script = fake(
        root.path(),
        &format!(
            "if sys.argv[1] == 'build-info': print({:?})\nelse: open({:?}, 'w').write('spawned')\n# stamp:0",
            serde_json::to_string(&identity()).unwrap(),
            marker.display().to_string()
        ),
    );
    let cancel = CancelToken::new();
    let engine = PreparedEngine::capture(&script, root.path(), 1, &cancel).unwrap();
    let request_file = engine.directory.path().join("request.json");
    let held = budget::acquire(crate::compute_parallelism(), &cancel).unwrap();
    let worker = thread::spawn(move || engine.calculate(&prepared_request(), &cancel));
    let start = Instant::now();
    while !request_file.exists() {
        assert!(start.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(10));
    }
    change_bytes_preserving_metadata(&script);
    drop(held);
    let failure = worker.join().unwrap().err().unwrap();
    assert_eq!(failure.kind, ProcessFailureKind::Changed);
    assert!(!marker.exists());
}

#[test]
fn failed_child_still_checks_same_metadata_executable_change() {
    let root = tempfile::tempdir().unwrap();
    let script = fake(
        root.path(),
        &format!(
            "if sys.argv[1] == 'build-info': print({:?}); sys.exit(0)\ns = os.stat(__file__)\nb = open(__file__, 'rb').read().replace(b'# stamp:0', b'# stamp:1')\nopen(__file__, 'wb').write(b)\nos.utime(__file__, ns=(s.st_atime_ns, s.st_mtime_ns))\nsys.exit(7)\n# stamp:0",
            serde_json::to_string(&identity()).unwrap()
        ),
    );
    let cancel = CancelToken::new();
    let engine = PreparedEngine::capture(&script, root.path(), 1, &cancel).unwrap();
    let before = fs::metadata(&script).unwrap();
    let failure = engine
        .calculate(&prepared_request(), &cancel)
        .err()
        .unwrap();
    assert_eq!(failure.kind, ProcessFailureKind::Changed);
    let after = fs::metadata(script).unwrap();
    assert_eq!(before.len(), after.len());
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
}
