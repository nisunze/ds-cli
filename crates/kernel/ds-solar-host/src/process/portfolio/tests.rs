use super::*;
use crate::process::tests::{fake, prepared_request_city};
use ds_solar_contracts::prepared_batch::{
    PREPARED_BATCH_REQUEST_SCHEMA, PreparedBatchOptions, PreparedBatchRequest,
};
use ds_solar_contracts::{
    BatchCity, PortfolioDocumentFormat, PortfolioDocumentOptions, PortfolioIdentity,
    PortfolioLanguage, PortfolioReportIntent, PortfolioRepresentativeStrategy, SolarErrorCode,
};
use std::sync::OnceLock;

fn binary() -> PathBuf {
    crate::process::tests::engine_under_test().to_path_buf()
}
fn probe_json() -> String {
    let probe = engine_probe(&binary(), &CancelToken::new()).unwrap();
    serde_json::to_string(&probe).unwrap()
}
struct Fixture {
    root: tempfile::TempDir,
    request: PortfolioTaskRequest,
    verified: VerifiedPortfolio,
}
fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let root = tempfile::tempdir().unwrap();
        let cancel = CancelToken::new();
        let prepared = ["aderm_bere", "aderm_beinamar"]
            .iter()
            .map(|city| prepared_request_city(city).prepared)
            .collect();
        let batch_request = PreparedBatchRequest {
            schema_version: PREPARED_BATCH_REQUEST_SCHEMA.into(),
            project_id: "ds-solar-fixtures".into(),
            root: "solar".into(),
            prepared,
            options: PreparedBatchOptions {
                run_id: Some("portfolio-process-city-source".into()),
                render_charts: true,
                generate_documents: false,
                report_drafts: vec![],
                input_maps: BTreeMap::new(),
            },
            concurrency: Some(2),
        };
        let batch = BatchEngine::capture(&binary(), root.path(), 2, &cancel)
            .unwrap()
            .calculate(&batch_request, &cancel, None)
            .unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        for file in &batch.outputs {
            file.copy_to(&source.join(&file.entry.name), &cancel)
                .unwrap();
        }
        let mut run = batch.output.batch.run.clone();
        // These declared but absent files prove unused cities and unrelated
        // selected-city chart slots cause neither reads nor acquisition.
        run.cities.push(BatchCity {
            city_id: "unused".into(),
            input_digest: run.cities[0].input_digest.clone(),
            result_digest: run.cities[0].result_digest.clone(),
        });
        let mut artifacts = batch.output.batch.artifacts.clone();
        artifacts.push(BatchArtifact::from_entry(
            &ArtifactEntry::from_bytes(
                "cities/unused/report_input.json",
                "application/json",
                b"unused",
            ),
            ArtifactRole::ReportInput,
            Some("unused"),
        ));
        artifacts.push(BatchArtifact::from_entry(
            &ArtifactEntry::from_bytes(
                "cities/aderm_bere/plots/site/unused.png",
                "image/png",
                b"unused",
            ),
            ArtifactRole::Chart,
            Some("aderm_bere"),
        ));
        let source_batch = SolarArtifactBatch::close(run, artifacts).unwrap();
        let mut manifest = serde_json::to_vec_pretty(&source_batch).unwrap();
        manifest.push(b'\n');
        fs::write(source.join("batch.json"), &manifest).unwrap();
        let request = PortfolioTaskRequest {
            schema_version: PORTFOLIO_TASK_REQUEST_SCHEMA.into(),
            source: PortfolioTaskSource {
                directory: source.to_str().unwrap().into(),
                manifest: ArtifactEntry::from_bytes("batch.json", "application/json", &manifest),
                batch: source_batch,
            },
            identity: PortfolioIdentity {
                project_id: "ds-solar-fixtures".into(),
                root: "solar".into(),
                portfolio_id: "process-portfolio".into(),
                portfolio_name: "Ordered saved portfolio".into(),
                membership_revision: "fixture-membership:12".into(),
            },
            cities: vec!["aderm_beinamar".into(), "aderm_bere".into()],
            run_id: "portfolio-process-run".into(),
            representative_strategy: PortfolioRepresentativeStrategy::RoundRobin { start_index: 1 },
            language: PortfolioLanguage::En,
            documents: PortfolioDocumentOptions {
                report_intents: vec![
                    PortfolioReportIntent::Financial,
                    PortfolioReportIntent::Network,
                    PortfolioReportIntent::Plant,
                    PortfolioReportIntent::Apd,
                ],
                formats: vec![PortfolioDocumentFormat::Markdown],
            },
            assertions: PortfolioTaskAssertions::default(),
        };
        let verified = PortfolioEngine::capture(&binary(), root.path(), 2, &cancel)
            .unwrap()
            .calculate(&request, &cancel)
            .unwrap();
        Fixture {
            root,
            request,
            verified,
        }
    })
}
fn cloned_task() -> PortfolioTaskResult {
    serde_json::from_value(serde_json::to_value(&fixture().verified.task).unwrap()).unwrap()
}
fn output() -> PathBuf {
    fixture().verified._directory.path().join("out")
}
fn verify(task: &PortfolioTaskResult) -> Result<(), String> {
    let fixture = fixture();
    verify_portfolio(
        task,
        &fixture.request,
        &serde_json::to_vec(&fixture.request).unwrap(),
        &fixture.verified.task.engine,
        &output(),
        &CancelToken::new(),
    )
}

#[test]
fn actual_portfolio_preserves_exact_ordered_receipt_and_unused_sources_stay_unread() {
    let fixture = fixture();
    let task = &fixture.verified.task;
    verify(task).unwrap();
    assert_eq!(
        task.input
            .members
            .iter()
            .map(|member| member.city_id.as_str())
            .collect::<Vec<_>>(),
        ["aderm_beinamar", "aderm_bere"]
    );
    assert_eq!(
        task.documents
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        [
            "portfolio-financial-en.md",
            "portfolio-network-en.md",
            "portfolio-plant-en.md",
            "portfolio-draft-en.md"
        ]
    );
    assert_eq!(
        task.completion_build_manifest_sha256,
        engine_probe(&binary(), &CancelToken::new())
            .unwrap()
            .completion_build_manifest_sha256
    );
    assert!(task.charts.len() > 8);
    let source = Path::new(&fixture.request.source.directory);
    assert!(!source.join("cities/unused/report_input.json").exists());
    assert!(
        !source
            .join("cities/aderm_bere/plots/site/unused.png")
            .exists()
    );
    for file in &fixture.verified.outputs {
        let bytes = file.read(&CancelToken::new()).unwrap();
        assert_eq!(
            ArtifactEntry::from_bytes(&file.entry.name, &file.entry.media_type, &bytes),
            file.entry
        );
    }
    let copied = tempfile::tempdir_in(fixture.root.path()).unwrap();
    let entry = &task.documents[0];
    fixture
        .verified
        .file(&entry.name)
        .unwrap()
        .copy_to(&copied.path().join("draft.md"), &CancelToken::new())
        .unwrap();
    assert_eq!(
        fs::read(copied.path().join("draft.md")).unwrap(),
        fixture
            .verified
            .file(&entry.name)
            .unwrap()
            .read(&CancelToken::new())
            .unwrap()
    );
}

#[test]
fn portfolio_refuses_wrong_request_engine_source_and_membership_receipts() {
    for (pointer, value) in [
        ("/schema_version", "ds-solar.portfolio-task-result/v999"),
        ("/engine/version", "foreign"),
        ("/request_sha256", "sha256:wrong"),
        ("/source/source_run_id", "foreign-run"),
        ("/source/source_batch_id", "foreign-batch"),
        ("/source/source_batch_digest", "sha256:wrong"),
        ("/source/manifest/content_digest", "sha256:wrong"),
        ("/source/cities/0/input_digest", "sha256:wrong"),
        ("/input/identity/project_id", "foreign-project"),
        ("/input/identity/membership_revision", "foreign-membership"),
        ("/input/run_id", "foreign-run"),
        ("/result/root", "foreign-root"),
        ("/result/portfolio_digest", "sha256:wrong"),
        ("/input/members/0/source_batch_digest", "sha256:wrong"),
        (
            "/result/cities/0/calculation_projection_digest",
            "sha256:wrong",
        ),
        ("/batch/batch_digest", "sha256:wrong"),
        ("/outputs/0/name", "../escape"),
        ("/outputs/0/content_digest", "sha256:wrong"),
    ] {
        let mut value_json = serde_json::to_value(&fixture().verified.task).unwrap();
        *value_json.pointer_mut(pointer).unwrap() = serde_json::json!(value);
        let task = serde_json::from_value(value_json).unwrap();
        assert!(verify(&task).is_err(), "{pointer}");
    }
    let mut task = cloned_task();
    task.documents.swap(0, 1);
    assert!(verify(&task).unwrap_err().contains("document_order"));
    let mut task = cloned_task();
    task.outputs.push(task.outputs[0].clone());
    assert!(verify(&task).is_err());
    let mut task = cloned_task();
    task.outputs[0].byte_count = u64::MAX;
    let completion = serde_json::to_vec(&serde_json::json!({"status": "ok"})).unwrap();
    assert!(
        verify_completion(&completion, &task)
            .unwrap_err()
            .contains("bound_exceeded")
    );
}

#[test]
fn modified_consumed_source_refuses_before_host_commit_without_acquisition() {
    let fixture = fixture();
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    for artifact in &fixture.request.source.batch.artifacts {
        let path = Path::new(&fixture.request.source.directory).join(&artifact.name);
        if path.exists() {
            files::write(&source.join(&artifact.name), &fs::read(path).unwrap()).unwrap();
        }
    }
    fs::copy(
        Path::new(&fixture.request.source.directory).join("batch.json"),
        source.join("batch.json"),
    )
    .unwrap();
    let mut request = fixture.request.clone();
    request.source.directory = source.to_str().unwrap().into();
    let entry = source.join("cities/aderm_beinamar/report_input.json");
    fs::write(entry, b"changed").unwrap();
    let error = PortfolioEngine::capture(&binary(), root.path(), 2, &CancelToken::new())
        .unwrap()
        .calculate(&request, &CancelToken::new())
        .err()
        .unwrap();
    assert_eq!(error.kind, ProcessFailureKind::Refused);
    assert_eq!(error.refusal.unwrap().code, SolarErrorCode::ArtifactFailed);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn portfolio_output_whitelist_rejects_links_extra_files_and_late_byte_changes() {
    let fixture = fixture();
    for changed in ["late", "extra", "link"] {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("out");
        fs::create_dir(&out).unwrap();
        for file in &fixture.verified.outputs {
            file.copy_to(
                &out.join("artifacts").join(&file.entry.name),
                &CancelToken::new(),
            )
            .unwrap();
        }
        fs::write(out.join("task-result.json"), b"receipt").unwrap();
        let entry = fixture.verified.task.outputs[0].clone();
        match changed {
            "late" => {
                fs::write(out.join("artifacts").join(&entry.name), b"changed").unwrap();
            }
            "extra" => {
                fs::write(out.join("unexpected"), b"undeclared").unwrap();
            }
            "link" => {
                let path = out.join("artifacts").join(&entry.name);
                fs::remove_file(&path).unwrap();
                std::os::unix::fs::symlink("/etc/passwd", &path).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            verify_files(&fixture.verified.task, &out, &CancelToken::new()).is_err(),
            "{changed}"
        );
        if changed != "extra" {
            let file = VerifiedPortfolioFile { entry, root: out };
            assert!(file.read(&CancelToken::new()).is_err());
        }
    }
}

#[test]
fn portfolio_capture_refuses_missing_schemas_or_invalid_completion_stamp() {
    for removed in [
        PORTFOLIO_TASK_REQUEST_SCHEMA,
        PORTFOLIO_TASK_RESULT_SCHEMA,
        "stamp",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&probe_json()).unwrap();
        if removed == "stamp" {
            value["completion_build_manifest_sha256"] = serde_json::json!("wrong");
        } else {
            value["schemas"]
                .as_array_mut()
                .unwrap()
                .retain(|schema| schema != removed);
        }
        let script = fake(
            root.path(),
            &format!(
                "assert sys.argv[1] == 'build-info'\nprint({:?})",
                value.to_string()
            ),
        );
        assert_eq!(
            PortfolioEngine::capture(&script, root.path(), 2, &CancelToken::new())
                .err()
                .unwrap()
                .kind,
            ProcessFailureKind::InvalidIdentity
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}

#[test]
fn portfolio_cancellation_deadline_and_output_bound_reap_before_returning() {
    let request = &fixture().request;
    for kind in ["cancel", "deadline", "file", "log"] {
        let root = tempfile::tempdir().unwrap();
        let probe = probe_json();
        let body = format!(
            "if sys.argv[1] == 'build-info':\n print({probe:?}); sys.exit(0)\nwith open('pid.tmp','w') as marker: marker.write(str(os.getpid()))\nos.replace('pid.tmp','pid')\n{}\nwhile True: time.sleep(1)",
            match kind {
                "file" => "open('oversized','wb').truncate(67108865)",
                "log" => "sys.stdout.write('x' * 100000); sys.stdout.flush()",
                _ => "",
            }
        );
        let script = fake(root.path(), &body);
        let cancel = CancelToken::new();
        let engine = PortfolioEngine::capture(&script, root.path(), 1, &cancel).unwrap();
        let marker = engine.captured.directory.path().join("pid");
        let interrupter = if kind == "cancel" {
            let signal = cancel.clone();
            let watched = marker.clone();
            Some(thread::spawn(move || {
                let started = Instant::now();
                while !watched.exists() {
                    assert!(started.elapsed() < Duration::from_secs(3));
                    thread::sleep(Duration::from_millis(10));
                }
                signal.cancel();
            }))
        } else {
            None
        };
        // Keep the marker outside the private lease so it survives cleanup.
        let copied_pid = root.path().join("reaped-pid");
        let watcher = thread::spawn({
            let marker = marker.clone();
            let copied_pid = copied_pid.clone();
            move || {
                let started = Instant::now();
                while !marker.exists() {
                    assert!(started.elapsed() < Duration::from_secs(3));
                    thread::sleep(Duration::from_millis(1));
                }
                fs::copy(marker, copied_pid).unwrap();
            }
        });
        let error = engine
            .calculate_inner(request, &cancel, Duration::from_millis(400))
            .err()
            .unwrap();
        if let Some(interrupter) = interrupter {
            interrupter.join().unwrap();
        }
        watcher.join().unwrap();
        assert!(
            error.contains(match kind {
                "cancel" => "cancelled",
                "deadline" => "deadline",
                "file" => "output_bound",
                "log" => "log_bound",
                _ => unreachable!(),
            }),
            "{kind}: {error}"
        );
        let pid: i32 = fs::read_to_string(copied_pid).unwrap().parse().unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}

#[test]
fn cancelled_portfolio_capture_and_run_do_not_leave_private_output() {
    let root = tempfile::tempdir().unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        PortfolioEngine::capture(&binary(), root.path(), 1, &cancel)
            .err()
            .unwrap()
            .kind,
        ProcessFailureKind::Cancelled
    );
    let engine = PortfolioEngine::capture(&binary(), root.path(), 1, &CancelToken::new()).unwrap();
    assert_eq!(
        engine
            .calculate(&fixture().request, &cancel)
            .err()
            .unwrap()
            .kind,
        ProcessFailureKind::Cancelled
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn replaced_portfolio_executable_is_refused_before_task_launch() {
    let root = tempfile::tempdir().unwrap();
    let script = fake(root.path(), &format!("print({:?})", probe_json()));
    let engine = PortfolioEngine::capture(&script, root.path(), 1, &CancelToken::new()).unwrap();
    fs::write(
        script,
        b"#!/usr/bin/python3\nraise Exception('must not execute')\n",
    )
    .unwrap();
    assert_eq!(
        engine
            .calculate(&fixture().request, &CancelToken::new())
            .err()
            .unwrap()
            .kind,
        ProcessFailureKind::Changed
    );
}
