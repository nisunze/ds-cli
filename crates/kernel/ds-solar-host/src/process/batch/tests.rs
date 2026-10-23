use super::*;
use crate::process::tests::{fake, identity, prepared_request_city};
use ds_solar_contracts::{ProgressPhase, SolarErrorCode};
use std::{os::unix::fs::PermissionsExt, sync::OnceLock};

fn binary() -> PathBuf {
    crate::process::tests::engine_under_test().to_path_buf()
}
fn request() -> PreparedBatchRequest {
    let prepared = vec![
        prepared_request_city("aderm_bere").prepared,
        prepared_request_city("aderm_beinamar").prepared,
    ];
    let bytes = b"a retained copied map, never acquired";
    let input_maps = prepared
        .iter()
        .map(|input| {
            (
                input.city.identity.city_id.clone(),
                BTreeMap::from([(
                    "gs://retained/map.png?generation=1".into(),
                    PreparedBatchMap {
                        entry: ArtifactEntry::from_bytes("maps/map.png", "image/png", bytes),
                        body_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                    },
                )]),
            )
        })
        .collect();
    PreparedBatchRequest {
        schema_version: PREPARED_BATCH_REQUEST_SCHEMA.into(),
        project_id: "ds-solar-fixtures".into(),
        root: "solar".into(),
        prepared,
        options: PreparedBatchOptions {
            run_id: Some("process-batch-fixture".into()),
            render_charts: true,
            generate_documents: true,
            report_drafts: vec![
                "financial".into(),
                "plant".into(),
                "network".into(),
                "apd".into(),
            ],
            input_maps,
        },
        concurrency: Some(2),
    }
}
fn probe_json(engine: &EngineIdentity, stamp: &str) -> String {
    serde_json::to_string(&ds_solar_contracts::engine_probe::EngineBuildProbe {
        identity: engine.clone(),
        completion_build_manifest_sha256: stamp.into(),
    })
    .unwrap()
}
struct Fixture {
    stamp: String,
    root: tempfile::TempDir,
    request: PreparedBatchRequest,
    task: Vec<u8>,
    engine: EngineIdentity,
    events: Vec<PreparedBatchProgress>,
}
fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let root = tempfile::tempdir().unwrap();
        let request = request();
        let bytes = serde_json::to_vec(&request).unwrap();
        let input = root.path().join("request.json");
        fs::write(&input, &bytes).unwrap();
        let probe = engine_probe(&binary(), &CancelToken::new()).unwrap();
        let engine = probe.identity;
        let events = Mutex::new(Vec::new());
        let sink = |event| events.lock().unwrap().push(event);
        let context = ProgressContext::new(
            &request,
            &bytes,
            &engine,
            &probe.completion_build_manifest_sha256,
            Some(&sink),
        )
        .unwrap();
        run_task_observed(
            &binary(),
            root.path(),
            2,
            Task::Batch {
                request: &input,
                out: &root.path().join("out"),
            },
            &CancelToken::new(),
            CALCULATION_DEADLINE,
            Some(&context),
        )
        .unwrap();
        Fixture {
            stamp: probe.completion_build_manifest_sha256,
            task: fs::read(root.path().join("out/task-result.json")).unwrap(),
            root,
            request,
            engine,
            events: events.into_inner().unwrap(),
        }
    })
}

#[test]
fn real_two_city_batch_verifies_exact_outcomes_files_and_progress() {
    let fixture = fixture();
    let task: PreparedBatchTaskResult = serde_json::from_slice(&fixture.task).unwrap();
    verify_batch(
        &task,
        &fixture.request,
        &serde_json::to_vec(&fixture.request).unwrap(),
        &fixture.engine,
        &fixture.root.path().join("out"),
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(
        task.output
            .cities
            .iter()
            .map(|city| city.city_id.as_str())
            .collect::<Vec<_>>(),
        ["aderm_bere", "aderm_beinamar"]
    );
    assert!(task.output.cities.iter().all(|city| city.outcome.is_ok()));
    for prepared in &fixture.request.prepared {
        let events = fixture
            .events
            .iter()
            .filter(|event| event.city_id == prepared.city.identity.city_id)
            .collect::<Vec<_>>();
        assert_eq!(
            events.first().unwrap().event.phase,
            ProgressPhase::RunStarted
        );
        assert_eq!(
            events.last().unwrap().event.phase,
            ProgressPhase::RunFinished
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event.phase == ProgressPhase::RunStarted)
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event.phase == ProgressPhase::RunFinished)
                .count(),
            1
        );
        assert!(events.len() > 15 && events.len() <= BATCH_MAX_PROGRESS_EVENTS_PER_CITY);
        assert!(
            events
                .iter()
                .all(|event| event.prepared_input_digest == prepared.input_digest)
        );
    }
    assert!(
        fixture
            .events
            .iter()
            .map(|event| encode_batch_progress_line(event).unwrap().len())
            .sum::<usize>()
            < BATCH_MAX_PROGRESS_BYTES
    );
    assert!(task.outputs.len() > 20);
}

#[test]
fn captured_real_partial_batch_keeps_ordered_failure_and_lifetime_whitelist() {
    let root = tempfile::tempdir().unwrap();
    let mut request = request();
    request.prepared[0].input_digest = "sha256:dead".into();
    request.options.render_charts = false;
    request.options.generate_documents = false;
    request.concurrency = Some(1024);
    let events = Mutex::new(Vec::new());
    let sink = |event| events.lock().unwrap().push(event);
    let engine = BatchEngine::capture(&binary(), root.path(), 2, &CancelToken::new()).unwrap();
    let result = engine
        .calculate(&request, &CancelToken::new(), Some(&sink))
        .unwrap();
    assert_eq!(
        result.output.cities[0].outcome.as_ref().unwrap_err().code,
        SolarErrorCode::CityInputNotPrepared
    );
    assert!(result.output.cities[0].portfolio_projection.is_none());
    assert!(result.output.cities[1].outcome.is_ok());
    assert_eq!(result.output.batch.run.cities.len(), 1);
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .all(|event| event.city_id == "aderm_beinamar")
    );
    assert!(result.file("../escape").is_none());
    let file = result
        .file("cities/aderm_beinamar/report_input.json")
        .unwrap();
    let source = file.read(&CancelToken::new()).unwrap();
    let copy = root.path().join("copied.json");
    file.copy_to(&copy, &CancelToken::new()).unwrap();
    assert_eq!(fs::read(&copy).unwrap(), source);
    fs::remove_file(copy).unwrap();
    let private = file.path.clone();
    assert!(private.is_file());
    fs::write(&private, b"tampered after verification").unwrap();
    assert_eq!(
        file.read(&CancelToken::new()).unwrap_err().kind,
        ProcessFailureKind::Integrity
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        file.read(&cancel).unwrap_err().kind,
        ProcessFailureKind::Cancelled
    );
    drop(result);
    assert!(!private.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn batch_capture_requires_progress_capability_and_preserves_executable_fence() {
    let root = tempfile::tempdir().unwrap();
    let mut old = identity();
    old.schemas
        .retain(|schema| schema != PREPARED_BATCH_PROGRESS_SCHEMA);
    let script = fake(
        root.path(),
        &format!(
            "assert sys.argv[1:] == ['build-info']\nprint({:?})",
            serde_json::to_string(&old).unwrap()
        ),
    );
    assert_eq!(
        BatchEngine::capture(&script, root.path(), 2, &CancelToken::new())
            .err()
            .unwrap()
            .kind,
        ProcessFailureKind::InvalidIdentity
    );
    let script = fake(
        root.path(),
        &format!(
            "assert sys.argv[1:] == ['build-info']\nprint({:?})",
            probe_json(&identity(), &"1".repeat(64))
        ),
    );
    let captured = BatchEngine::capture(&script, root.path(), 2, &CancelToken::new()).unwrap();
    fs::write(
        &script,
        b"#!/usr/bin/python3\nraise Exception('must not run')\n",
    )
    .unwrap();
    assert_eq!(
        captured
            .calculate(&request(), &CancelToken::new(), None)
            .err()
            .unwrap()
            .kind,
        ProcessFailureKind::Changed
    );
    let oversized = root.path().join("oversized");
    let file = fs::File::create(&oversized).unwrap();
    file.set_len(MAX_EXECUTABLE_BYTES + 1).unwrap();
    fs::set_permissions(&oversized, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        BatchEngine::capture(&oversized, root.path(), 2, &CancelToken::new())
            .err()
            .unwrap()
            .kind,
        ProcessFailureKind::OutputBound
    );
}

#[test]
fn real_worker_cancellation_and_deadline_are_reaped_before_output_acceptance() {
    for deadline in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("pid");
        let script = fake(
            root.path(),
            &format!(
                "if sys.argv[1] != 'build-info': open({marker:?},'w').write(str(os.getpid()))\nos.execv({binary:?}, [{binary:?}] + sys.argv[1:])",
                marker = marker.to_string_lossy(),
                binary = binary().to_string_lossy()
            ),
        );
        let cancel = CancelToken::new();
        let signal = cancel.clone();
        let sink = move |_: PreparedBatchProgress| {
            if !deadline {
                signal.cancel();
            }
        };
        let engine = BatchEngine::capture(&script, root.path(), 2, &cancel).unwrap();
        let duration = if deadline {
            Duration::from_millis(1)
        } else {
            CALCULATION_DEADLINE
        };
        let error = engine
            .calculate_with_timeout(&request(), &cancel, Some(&sink), duration)
            .err()
            .unwrap();
        assert!(
            error
                .message
                .contains(if deadline { "deadline" } else { "cancelled" })
        );
        let pid = match fs::read_to_string(&marker) {
            Ok(pid) => Some(pid.parse::<i32>().unwrap()),
            // A one-millisecond deadline includes process startup. Under
            // contention it may expire before the Python wrapper reaches its
            // marker; cancellation above always occurs inside real progress.
            Err(error) if deadline && error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("live worker PID was not recorded: {error}"),
        };
        if let Some(pid) = pid {
            assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
        }
        assert_eq!(
            fs::read_dir(root.path()).unwrap().count(),
            1 + usize::from(pid.is_some()),
            "private job output must already be removed, including startup deadline",
        );
    }
}

#[test]
fn real_worker_receipt_scope_order_digests_manifests_and_outcome_metadata_are_fenced() {
    let fixture = fixture();
    for (pointer, value) in [
        ("/request_sha256", serde_json::json!("sha256:wrong")),
        ("/engine/version", serde_json::json!("other")),
        (
            "/prepared_input_digests/0/input_digest",
            serde_json::json!("sha256:wrong"),
        ),
        ("/output/cities/0/city_id", serde_json::json!("other")),
        ("/output/batch/run/project_id", serde_json::json!("other")),
        ("/output/batch/run/root", serde_json::json!("other")),
        ("/output/batch/run/run_id", serde_json::json!("other")),
        (
            "/output/cities/0/outcome/Ok/result/result_digest",
            serde_json::json!("sha256:wrong"),
        ),
        (
            "/output/cities/0/outcome/Ok/daily_load_profile/load_w/0",
            serde_json::json!(123456.0),
        ),
        (
            "/output/cities/0/outcome/Ok/report_input/content_digest",
            serde_json::json!("sha256:wrong"),
        ),
        (
            "/outputs/0/content_digest",
            serde_json::json!("sha256:wrong"),
        ),
        ("/outputs/0/name", serde_json::json!("../escape")),
    ] {
        let mut value_task: serde_json::Value = serde_json::from_slice(&fixture.task).unwrap();
        *value_task.pointer_mut(pointer).unwrap() = value;
        let task: PreparedBatchTaskResult = serde_json::from_value(value_task).unwrap();
        assert!(
            verify_batch(
                &task,
                &fixture.request,
                &serde_json::to_vec(&fixture.request).unwrap(),
                &fixture.engine,
                &fixture.root.path().join("out"),
                &CancelToken::new()
            )
            .is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn missing_malformed_receipts_and_changed_artifact_bytes_refuse_captured_task() {
    let fixture = fixture();
    for corruption in [
        "p=out+'/task-result.json'; v=json.load(open(p)); v['completion_build_manifest_sha256']='2'*64; json.dump(v,open(p,'w'))",
        "os.remove(out+'/task-result.json')",
        "open(out+'/task-result.json','w').write('{')",
        "open(out+'/artifacts/cities/aderm_bere/report_input.json','w').write('{}')",
        "open(out+'/artifacts/undeclared','w').write('x')",
    ] {
        let root = tempfile::tempdir().unwrap();
        let completion = serde_json::json!({"status":"ok", "schema_version":PREPARED_BATCH_RESULT_SCHEMA,
            "request_sha256":sha256_hex(&serde_json::to_vec(&fixture.request).unwrap())});
        let script = fake(
            root.path(),
            &format!(
                "if sys.argv[1] == 'build-info': print({identity:?})\nelse:\n out=sys.argv[sys.argv.index('--out')+1]\n shutil.copytree({source:?},out)\n {corruption}\n print({completion:?})",
                identity = probe_json(&fixture.engine, &fixture.stamp),
                source = fixture.root.path().join("out").to_string_lossy(),
                completion = completion.to_string()
            ),
        );
        let error = BatchEngine::capture(&script, root.path(), 2, &CancelToken::new())
            .unwrap()
            .calculate(&fixture.request, &CancelToken::new(), None)
            .err()
            .unwrap();
        assert_eq!(
            error.kind,
            ProcessFailureKind::Integrity,
            "{corruption}: {error}"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}

#[test]
fn pipe_fences_and_bounds_refuse_without_lossy_truncation_or_late_delivery() {
    let fixture = fixture();
    let bytes = serde_json::to_vec(&fixture.request).unwrap();
    let delivered = Mutex::new(Vec::new());
    let sink = |event| delivered.lock().unwrap().push(event);
    let context = ProgressContext::new(
        &fixture.request,
        &bytes,
        &fixture.engine,
        &fixture.stamp,
        Some(&sink),
    )
    .unwrap();
    let completion =
        serde_json::json!({"status":"ok", "schema_version":PREPARED_BATCH_RESULT_SCHEMA,
        "request_sha256":context.request_sha256})
        .to_string()
            + "\n";
    for mutate in [
        "request",
        "engine",
        "project",
        "root",
        "run",
        "city",
        "input",
        "oversized",
    ] {
        let mut event = fixture.events[0].clone();
        match mutate {
            "request" => event.request_sha256.push('x'),
            "engine" => event.engine_sha256.push('x'),
            "project" => event.project_id.push('x'),
            "root" => event.root.push('x'),
            "run" => event.run_id.push('x'),
            "city" => event.city_id.push('x'),
            "input" => event.prepared_input_digest.push('x'),
            _ => event.event.message = "x".repeat(BATCH_MAX_PROGRESS_LINE_BYTES),
        }
        let mut data = serde_json::to_vec(&event).unwrap();
        data.push(b'\n');
        data.extend(completion.as_bytes());
        assert!(
            read_progress(data.as_slice(), &context, &CancelToken::new()).is_err(),
            "{mutate}"
        );
    }
    assert!(delivered.lock().unwrap().is_empty());
    let mut data = Vec::new();
    for _ in 0..=BATCH_MAX_PROGRESS_EVENTS_PER_CITY {
        data.extend(encode_batch_progress_line(&fixture.events[0]).unwrap());
    }
    data.extend(completion.as_bytes());
    assert!(
        read_progress(data.as_slice(), &context, &CancelToken::new())
            .unwrap_err()
            .contains("event_bound")
    );
    assert_eq!(
        delivered.lock().unwrap().len(),
        BATCH_MAX_PROGRESS_EVENTS_PER_CITY
    );
    delivered.lock().unwrap().clear();
    let cancel = CancelToken::new();
    cancel.cancel();
    let data = [
        encode_batch_progress_line(&fixture.events[0]).unwrap(),
        completion.as_bytes().to_vec(),
    ]
    .concat();
    assert!(
        read_progress(data.as_slice(), &context, &cancel)
            .unwrap_err()
            .contains("cancelled")
    );
    assert!(delivered.lock().unwrap().is_empty());
}

#[test]
fn cancelled_and_malformed_unused_maps_acquire_nothing_and_spawn_no_calculation() {
    let root = tempfile::tempdir().unwrap();
    let script = fake(
        root.path(),
        &format!(
            "assert sys.argv[1:] == ['build-info']\nprint({:?})",
            probe_json(&identity(), &"1".repeat(64))
        ),
    );
    for cancelled in [false, true] {
        let cancel = CancelToken::new();
        let engine = BatchEngine::capture(&script, root.path(), 2, &cancel).unwrap();
        let mut request = request();
        request.options.input_maps.insert(
            "unused-city".into(),
            BTreeMap::from([(
                "gs://must-never-be-read".into(),
                PreparedBatchMap {
                    entry: ArtifactEntry::from_bytes("map.png", "image/png", b"retained"),
                    body_base64: "malformed!".into(),
                },
            )]),
        );
        if cancelled {
            cancel.cancel();
        }
        let failure = engine.calculate(&request, &cancel, None).err().unwrap();
        assert_eq!(
            failure.kind,
            if cancelled {
                ProcessFailureKind::Cancelled
            } else {
                ProcessFailureKind::Integrity
            }
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}

#[test]
fn wrapper_512_slot_bound_keeps_existing_owner_200_city_refusal() {
    let root = tempfile::tempdir().unwrap();
    let mut request = request();
    let mut minimal = request.prepared.remove(0);
    minimal.weather.timestamps.clear();
    minimal.weather.series.clear();
    let profile = &mut minimal.reference_unit.profile;
    profile.epoch_seconds.clear();
    profile.local_timestamp.clear();
    profile.effective_irradiance_w_m2.clear();
    profile.gross_unit_ac_energy_mwh.clear();
    profile.net_unit_energy_mwh.clear();
    profile.unit_inverter_loss_mwh.clear();
    request.prepared = (0..BATCH_MAX_CITIES)
        .map(|index| {
            let mut input = minimal.clone();
            input.city.identity.city_id = format!("invalid-{index}");
            input
        })
        .collect();
    request.options.input_maps.clear();
    request.options.render_charts = false;
    request.options.generate_documents = false;
    assert!(admit_request(&request).is_ok());
    let captured = BatchEngine::capture(&binary(), root.path(), 2, &CancelToken::new()).unwrap();
    let error = captured
        .calculate(&request, &CancelToken::new(), None)
        .err()
        .unwrap();
    assert_eq!(error.kind, ProcessFailureKind::Refused);
    assert_eq!(
        error.refusal.as_ref().unwrap().code,
        SolarErrorCode::OutOfBounds
    );
    assert!(error.message.contains("maximum is 200"));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    request.prepared.push(minimal);
    assert!(admit_request(&request).is_err());
}

#[test]
fn batch_pipe_and_file_overflow_stop_and_reap_before_returning() {
    for (body, expected) in [
        (
            "sys.stdout.write('x'*40000); sys.stdout.flush()",
            ProcessFailureKind::OutputBound,
        ),
        (
            "sys.stderr.write('x'*100000); sys.stderr.flush()",
            ProcessFailureKind::OutputBound,
        ),
        (
            "open('worker-oversized','wb').truncate(67108865)",
            ProcessFailureKind::OutputBound,
        ),
        (
            "sys.stdout.write('not-json\\n'); sys.stdout.flush()",
            ProcessFailureKind::Integrity,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("pid");
        let script = fake(
            root.path(),
            &format!(
                "if sys.argv[1] == 'build-info': print({probe:?})\nelse:\n assert sys.argv[1] == 'calculate-batch' and sys.argv[-1] == '--progress-jsonl'\n assert all(os.environ[key] == '2' for key in ['DS_SOLAR_WORKER_THREADS','RAYON_NUM_THREADS','OMP_NUM_THREADS'])\n open({marker:?},'w').write(str(os.getpid()))\n {body}\n while True: time.sleep(1)",
                probe = probe_json(&identity(), &"1".repeat(64)),
                marker = marker.to_string_lossy()
            ),
        );
        let error = BatchEngine::capture(&script, root.path(), 2, &CancelToken::new())
            .unwrap()
            .calculate(&request(), &CancelToken::new(), None)
            .err()
            .unwrap();
        assert_eq!(error.kind, expected, "{body}: {error}");
        let pid: i32 = fs::read_to_string(marker).unwrap().parse().unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }
}

#[test]
fn explicit_batch_timeout_retains_two_hour_default_and_caps_at_four_hours() {
    assert_eq!(CALCULATION_DEADLINE, Duration::from_secs(2 * 60 * 60));
    assert_eq!(MAX_BATCH_TIMEOUT, Duration::from_secs(4 * 60 * 60));
    for timeout in [Duration::ZERO, MAX_BATCH_TIMEOUT + Duration::from_nanos(1)] {
        let root = tempfile::tempdir().unwrap();
        let identity = identity();
        let script = fake(
            root.path(),
            &format!(
                "assert sys.argv[1:] == ['build-info']\nprint({:?})",
                probe_json(&identity, EngineIdentity::publication_manifest_sha256())
            ),
        );
        let engine = BatchEngine::capture(&script, root.path(), 1, &CancelToken::new()).unwrap();
        let error = engine
            .calculate_with_timeout(&request(), &CancelToken::new(), None, timeout)
            .err()
            .unwrap();
        assert_eq!(error.kind, ProcessFailureKind::Deadline);
        assert!(error.message.contains("deadline_invalid"));
    }
}

#[test]
fn pure_batch_admission_preserves_exact_identity_and_serialized_size_boundaries() {
    let mut request = request();
    request.project_id = "p".repeat(BATCH_MAX_ID_BYTES);
    request.root = "r".repeat(BATCH_MAX_ID_BYTES);
    request.options.run_id = Some("i".repeat(BATCH_MAX_ID_BYTES));
    for (index, prepared) in request.prepared.iter_mut().enumerate() {
        prepared.city.identity.project_id = request.project_id.clone();
        prepared.city.identity.root = request.root.clone();
        prepared.city.identity.city_id = format!("{index}{}", "c".repeat(BATCH_MAX_ID_BYTES - 1));
    }
    BatchEngine::validate_request(&request).unwrap();
    for field in ["project", "root", "city", "run"] {
        let mut oversized = request.clone();
        match field {
            "project" => oversized.project_id.push('p'),
            "root" => oversized.root.push('r'),
            "city" => oversized.prepared[0].city.identity.city_id.push('c'),
            "run" => oversized.options.run_id.as_mut().unwrap().push('i'),
            _ => unreachable!(),
        }
        assert!(
            BatchEngine::validate_request(&oversized).is_err(),
            "{field}"
        );
    }
    let bytes = serde_json::to_vec(&request).unwrap();
    let mut exact = Vec::new();
    write_request(&request, &mut exact, bytes.len() as u64).unwrap();
    assert_eq!(
        exact, bytes,
        "admission and transport retain exact request bytes"
    );
    assert!(
        write_request(&request, std::io::sink(), bytes.len() as u64 - 1)
            .unwrap_err()
            .contains("solar_batch_request_too_large")
    );
    assert_eq!(BATCH_MAX_REQUEST_BYTES, 512 * 1024 * 1024);
    assert_eq!(BATCH_MAX_FILE_BYTES, 64 * 1024 * 1024);
    assert_eq!(BATCH_MAX_OUTPUT_BYTES, 4 * 1024 * 1024 * 1024);
    let map = request
        .options
        .input_maps
        .values_mut()
        .next()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    map.entry.byte_count = BATCH_MAX_FILE_BYTES + 1;
    assert!(
        BatchEngine::validate_request(&request)
            .unwrap_err()
            .message
            .contains("solar_task_batch_map_descriptor_invalid")
    );
}
