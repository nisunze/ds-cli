fn wait_inputs(
    command: &ds_cli_contract::spec::Command,
    tokens: &[&str],
) -> ds_cli_contract::Inputs {
    ds_cli_contract::parse(
        command,
        &tokens.iter().map(|t| (*t).to_owned()).collect::<Vec<_>>(),
    )
    .unwrap()
}
fn wait_context() -> ds_cli_contract::Context {
    ds_cli_contract::Context {
        confirmed: false,
        output: ds_cli_contract::output::Output {
            format: ds_cli_contract::output::Format::Json,
            pretty: false,
            color: false,
        },
    }
}
// Included in host::tests so these exercise the production router and store.
struct WaitAuth {
    allowed: std::sync::atomic::AtomicBool,
    checked: tokio::sync::mpsc::UnboundedSender<()>,
}
impl Authorizer for WaitAuth {
    fn authorize(&self, _: &str) -> Result<(), String> {
        let _ = self.checked.send(());
        if self.allowed.load(std::sync::atomic::Ordering::Acquire) {
            Ok(())
        } else {
            Err("credential revoked".into())
        }
    }
}
fn wait_fixture(
    path: &Path,
) -> (
    App,
    Arc<WaitAuth>,
    tokio::sync::mpsc::UnboundedReceiver<()>,
    Job,
) {
    let mut app = app(path, true);
    let (checked, receiver) = tokio::sync::mpsc::unbounded_channel();
    let auth = Arc::new(WaitAuth {
        allowed: std::sync::atomic::AtomicBool::new(true),
        checked,
    });
    app.auth = auth.clone();
    let input = transformer("wait-test");
    let digest = runtime::digest(&input);
    let id = runtime::job_id(&app.connection.owner, "stable", A, "wait-test");
    let context = ds_command_kernel::execution_context::ExecutionContext {
        principal_uid: UID.into(),
        lane: "stable".into(),
        deployment: DEPLOYMENT.into(),
        install_id: "install-1".into(),
        project: A.into(),
        client: "wait-test".into(),
        operation: "transformer_processing".into(),
        job_id: id.clone(),
        idempotency_key: "wait-test".into(),
        input_sha256: digest.clone(),
        admitted_at_ms: 1,
    };
    let job = Job {
        id,
        owner: app.connection.owner.clone(),
        lane: "stable".into(),
        input_sha256: digest,
        engine: ds_command_kernel::compute_jobs::EngineKind::LvProcessing,
        input_tag: "ds.fast-lv.request/v1".into(),
        phase: Phase::Queued,
        attempts: 0,
        created_at_ms: 1,
        updated_at_ms: 1,
        worker: None,
        lease_until_ms: 0,
        result_sha256: None,
        error: None,
        context: Some(context.clone()),
        engine_pin: Some(ds_command_kernel::compute_jobs::EnginePin {
            schema: "ds.compute.engine-pin/v1".into(),
            engine: ds_command_kernel::compute_jobs::EngineKind::LvProcessing,
            owner: app.connection.owner.clone(),
            lane: ds_command_kernel::compute_jobs::EngineLane::Stable,
            context,
            source: ds_command_kernel::compute_jobs::EnginePinSource::NativeLaneCapture,
            executable: "/missing-native-wait-test-component".into(),
            executable_sha256: "a".repeat(64),
            identity: json!({"storage_fixture":true}),
            workers: 1,
            development_override: true,
        }),
    };
    runtime::open(&app.database)
        .unwrap()
        .submit_job(&job, &input, limits())
        .unwrap();
    (app, auth, receiver, job)
}
async fn wait_registered(checked: &mut tokio::sync::mpsc::UnboundedReceiver<()>) {
    // Middleware authorizes first; the second check is after subscription.
    for _ in 0..2 {
        tokio::time::timeout(std::time::Duration::from_secs(5), checked.recv())
            .await
            .unwrap()
            .unwrap();
    }
}
fn wait_call(app: App, id: &str) -> tokio::task::JoinHandle<(StatusCode, Value)> {
    let uri = format!("/v1/jobs/{id}/result?project={A}&wait=600");
    tokio::spawn(async move { call(app, "GET", &uri, None).await })
}
fn wait_complete(app: &App, job: &Job) -> Job {
    let mut store = runtime::open(&app.database).unwrap();
    let caller = app.sessions.identity().caller(Some(A));
    store
        .claim_job(&caller, "worker", runtime::now_ms(), 30_000, limits())
        .unwrap()
        .unwrap();
    let bytes = b"{ \"retained\": true }\n";
    let completed = store
        .update_job(
            &caller,
            &job.id,
            Event::Complete {
                worker: "worker",
                result_sha256: &runtime::digest(bytes),
            },
            runtime::now_ms(),
            Some(bytes),
        )
        .unwrap();
    runtime::notify_terminal(&app.database, &completed);
    completed
}

#[tokio::test]
async fn result_wait_completion_race_fanout_and_immediate_bytes_are_durable() {
    use base64::Engine;
    let dir = tempfile::tempdir().unwrap();
    let (app, _, mut checked, job) = wait_fixture(dir.path());
    let first = wait_call(app.clone(), &job.id);
    wait_registered(&mut checked).await;
    let second = wait_call(app.clone(), &job.id);
    wait_registered(&mut checked).await;
    // Completion may land while the initial durable read is still returning.
    let completed = wait_complete(&app, &job);
    for observer in [first, second] {
        let (status, receipt) = tokio::time::timeout(std::time::Duration::from_secs(5), observer)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, StatusCode::OK, "{receipt}");
        assert_eq!(receipt["job"], json!(completed));
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(receipt["result_base64"].as_str().unwrap())
                .unwrap(),
            b"{ \"retained\": true }\n"
        );
    }
    let (status, bytes) = raw(
        app.clone(),
        "GET",
        &format!("/v1/jobs/{}/result?project={A}", job.id),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"{ \"retained\": true }\n");
    // Already terminal and restart-style reattachment need no notification.
    let (status, receipt) = call(
        app,
        "GET",
        &format!("/v1/jobs/{}/result?project={A}&wait=600", job.id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
}

#[tokio::test]
async fn result_wait_queued_cancellation_wakes_without_holding_request_door() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _, mut checked, job) = wait_fixture(dir.path());
    app.requests = Arc::new(Door::new(1));
    let observer = wait_call(app.clone(), &job.id);
    wait_registered(&mut checked).await;
    let (status, _) = call(
        app.clone(),
        "GET",
        &format!("/v1/jobs/{}?project={A}", job.id),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "status remains available with the only door free"
    );
    let (status, _) = call(
        app.clone(),
        "POST",
        &format!("/v1/jobs/{}/cancel?project={A}", job.id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, refusal) = tokio::time::timeout(std::time::Duration::from_secs(5), observer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(refusal["code"], "result_cancelled");
    assert_eq!(refusal["job"]["phase"], "cancelled");
    assert_eq!(
        runtime::open(&app.database)
            .unwrap()
            .job_input(&app.sessions.identity().caller(Some(A)), &job.id)
            .unwrap()
            .unwrap(),
        transformer("wait-test")
    );
}

#[tokio::test]
async fn result_wait_runtime_failure_wakes_after_failed_commit() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _, mut checked, job) = wait_fixture(dir.path());
    let observer = wait_call(app.clone(), &job.id);
    wait_registered(&mut checked).await;
    let context = runtime::WorkerContext {
        path: app.database.clone(),
        identity: app.sessions.identity().clone(),
        limits: limits(),
        auth: app.auth.clone(),
        observer: None,
    };
    assert!(
        tokio::task::spawn_blocking(move || runtime::run_one(&context, "worker"))
            .await
            .unwrap()
            .unwrap()
    );
    let (_, refusal) = tokio::time::timeout(std::time::Duration::from_secs(5), observer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(refusal["code"], "result_failed", "{refusal}");
    assert_eq!(refusal["job"]["phase"], "failed");
    assert!(
        refusal["job"]["error"]
            .as_str()
            .is_some_and(|e| !e.is_empty())
    );
}

#[tokio::test]
async fn result_wait_timeout_detach_refusal_and_revocation_preserve_job() {
    let dir = tempfile::tempdir().unwrap();
    let (app, auth, mut checked, job) = wait_fixture(dir.path());
    let (status, refusal) = call(
        app.clone(),
        "GET",
        &format!("/v1/jobs/{}/result?project={A}&wait=1", job.id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(refusal["code"], "result_pending");
    assert_eq!(refusal["job"]["phase"], "queued");
    assert_eq!(refusal["retryable"], true);
    while checked.try_recv().is_ok() {}
    let observer = wait_call(app.clone(), &job.id);
    wait_registered(&mut checked).await;
    observer.abort();
    let _ = observer.await;
    assert_eq!(app.requests.waiters.available_permits(), 32);
    assert_eq!(
        runtime::open(&app.database)
            .unwrap()
            .job(&app.sessions.identity().caller(Some(A)), &job.id)
            .unwrap()
            .unwrap()
            .phase,
        Phase::Queued
    );
    while checked.try_recv().is_ok() {}
    let observer = wait_call(app.clone(), &job.id);
    wait_registered(&mut checked).await;
    auth.allowed
        .store(false, std::sync::atomic::Ordering::Release);
    wait_complete(&app, &job);
    let (status, refusal) = tokio::time::timeout(std::time::Duration::from_secs(5), observer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(refusal["code"], "server_owner_changed");
    assert!(
        refusal.get("job").is_none(),
        "revocation exposes no job bytes"
    );
    auth.allowed
        .store(true, std::sync::atomic::Ordering::Release);
    for query in [
        format!("project={B}&wait=1"),
        format!("project={A}&wait=0"),
        format!("project={A}&wait=601"),
    ] {
        let (_, refusal) = call(
            app.clone(),
            "GET",
            &format!("/v1/jobs/{}/result?{query}", job.id),
            None,
        )
        .await;
        assert!(
            matches!(
                refusal["code"].as_str(),
                Some("not_visible" | "result_wait_invalid")
            ),
            "{refusal}"
        );
    }
}

#[tokio::test]
async fn result_wait_budget_is_separate_and_released_on_detach() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _, mut checked, job) = wait_fixture(dir.path());
    app.requests = Arc::new(Door::new(1));
    let mut observers = Vec::new();
    for _ in 0..32 {
        observers.push(wait_call(app.clone(), &job.id));
        wait_registered(&mut checked).await;
    }
    let (status, refusal) = call(
        app.clone(),
        "GET",
        &format!("/v1/jobs/{}/result?project={A}&%77ait=600", job.id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(refusal["scope"], "result_waiters");
    let (status, _) = call(
        app.clone(),
        "GET",
        &format!("/v1/jobs/{}?project={A}", job.id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for observer in observers {
        observer.abort();
        let _ = observer.await;
    }
    assert_eq!(app.requests.waiters.available_permits(), 32);
}

#[cfg(unix)]
async fn wait_socket(
    app: &App,
    directory: &Path,
) -> (
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
    connection(directory, app.connection.owner.clone(), "stable".into()).unwrap();
    let listening = crate::transport::listen(directory).unwrap();
    let mut served = app.clone();
    served.os_account = crate::transport::own_account();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let serving = tokio::spawn(async move {
        crate::transport::serve(listening, router(served), async {
            let _ = stopped.await;
        })
        .await
        .unwrap();
    });
    (stop, serving)
}

#[tokio::test]
#[cfg(unix)]
async fn result_wait_socket_disconnect_and_restart_detach_without_cancel() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let (app, _, mut checked, job) = wait_fixture(dir.path());
    let (stop, serving) = wait_socket(&app, dir.path()).await;
    let mut stream = std::os::unix::net::UnixStream::connect(&app.connection.socket).unwrap();
    write!(
        stream,
        "GET /v1/jobs/{}/result?project={A}&wait=600 HTTP/1.1\r\nHost: localhost\r\n\r\n",
        job.id
    )
    .unwrap();
    wait_registered(&mut checked).await;
    drop(stream);
    // Semaphore wake proves socket observation was dropped, without polling.
    let permits = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        app.requests.waiters.clone().acquire_many_owned(32),
    )
    .await
    .unwrap()
    .unwrap();
    drop(permits);
    let socket = app.connection.socket.clone();
    let path = format!("/v1/jobs/{}/result?project={A}&wait=600", job.id);
    let observer = tokio::task::spawn_blocking(move || {
        crate::transport::call(
            &socket,
            "GET",
            &path,
            &[],
            None,
            1024 * 1024,
            std::time::Duration::from_secs(660),
        )
    });
    wait_registered(&mut checked).await;
    stop.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), serving)
        .await
        .unwrap()
        .unwrap();
    let reply = tokio::time::timeout(std::time::Duration::from_secs(5), observer)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(reply.status, 503);
    assert!(String::from_utf8_lossy(&reply.body).contains("Core stopped observing"));
    assert_eq!(
        runtime::open(&app.database)
            .unwrap()
            .job(&app.sessions.identity().caller(Some(A)), &job.id)
            .unwrap()
            .unwrap()
            .phase,
        Phase::Queued
    );
    let (stop, serving) = wait_socket(&app, dir.path()).await;
    let observer = wait_call(app.clone(), &job.id);
    wait_registered(&mut checked).await;
    wait_complete(&app, &job);
    assert_eq!(observer.await.unwrap().0, StatusCode::OK);
    stop.send(()).unwrap();
    serving.await.unwrap();
}

#[tokio::test]
#[cfg(unix)]
async fn result_wait_cli_file_receipt_and_timeout_are_truthful() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _, mut checked, job) = wait_fixture(dir.path());
    let (stop, serving) = wait_socket(&app, dir.path()).await;
    let output = dir.path().join("result.json");
    let arguments = |out: &Path, wait: &str| {
        wait_inputs(
            &crate::RESULT,
            &[
                "--state-dir",
                dir.path().to_str().unwrap(),
                "--lane",
                "stable",
                "--project",
                A,
                "--job",
                &job.id,
                "--out",
                out.to_str().unwrap(),
                "--wait",
                wait,
            ],
        )
    };
    let input = arguments(&output, "1");
    let refused = tokio::task::spawn_blocking(move || crate::result(&input, &wait_context()))
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(refused.code(), "result_pending");
    assert_eq!(refused.detail_value().unwrap()["job"]["id"], job.id);
    assert!(!output.exists());
    while checked.try_recv().is_ok() {}
    let input = arguments(&output, "600");
    let observer = tokio::task::spawn_blocking(move || crate::result(&input, &wait_context()));
    wait_registered(&mut checked).await;
    let completed = wait_complete(&app, &job);
    let receipt = observer.await.unwrap().unwrap();
    let bytes = fs::read(&output).unwrap();
    assert_eq!(bytes, b"{ \"retained\": true }\n");
    assert_eq!(receipt["byte_count"], bytes.len());
    assert_eq!(receipt["sha256"], runtime::digest(&bytes));
    assert_eq!(receipt["job"], json!(completed));
    let input = arguments(&output, "600");
    let refused = crate::result(&input, &wait_context()).unwrap_err();
    assert_eq!(refused.code(), "server_output_exists");
    stop.send(()).unwrap();
    serving.await.unwrap();
}

#[tokio::test]
#[ignore = "requires an explicitly selected compatible native Network executable"]
async fn result_wait_real_network_completion_preserves_admitted_pin() {
    use base64::Engine;
    let dir = tempfile::tempdir().unwrap();
    let mut app = app(dir.path(), true);
    app.requests = Arc::new(Door::new(1));
    let (checked, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    app.auth = Arc::new(WaitAuth {
        allowed: std::sync::atomic::AtomicBool::new(true),
        checked,
    });
    let job = runtime::submit(
        &app.database,
        &Admission {
            identity: app.sessions.identity(),
            client: "wait-native-proof",
            key: "wait-native-proof",
            requested_project: Some(A),
            limits: limits(),
            now_ms: runtime::now_ms(),
        },
        &transformer("native-wait-proof"),
    )
    .unwrap();
    let observer = wait_call(app.clone(), &job.id);
    wait_registered(&mut receiver).await;
    let (submitted, receipt) = call(
        app.clone(),
        "POST",
        &format!("/v1/transformer-processing/other-project?project={B}"),
        Some(transformer("other-project")),
    )
    .await;
    assert_eq!(
        submitted,
        StatusCode::ACCEPTED,
        "a waiting observer cannot starve submission: {receipt}"
    );
    assert_eq!(receipt["job"]["context"]["project"], B);
    let context = runtime::WorkerContext {
        path: app.database.clone(),
        identity: app.sessions.identity().clone(),
        limits: limits(),
        auth: app.auth.clone(),
        observer: None,
    };
    assert!(
        tokio::task::spawn_blocking(move || runtime::run_one(&context, "native-worker"))
            .await
            .unwrap()
            .unwrap()
    );
    let (status, receipt) = tokio::time::timeout(std::time::Duration::from_secs(15), observer)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, StatusCode::OK, "{receipt}");
    assert_eq!(receipt["job"]["engine_pin"], json!(job.engine_pin));
    assert_eq!(receipt["job"]["context"], json!(job.context));
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(receipt["result_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(receipt["job"]["result_sha256"], runtime::digest(&bytes));
    let result: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["jobs"][0]["transformer_name"], "native-wait-proof");
    assert_eq!(result["jobs"][0]["ok"], true, "{result}");
}

#[tokio::test]
async fn result_wait_running_cancel_and_post_wait_visibility_fence() {
    for mutate_scope in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (app, _, mut checked, job) = wait_fixture(dir.path());
        runtime::open(&app.database)
            .unwrap()
            .claim_job(
                &app.sessions.identity().caller(Some(A)),
                "worker",
                runtime::now_ms(),
                30_000,
                limits(),
            )
            .unwrap()
            .unwrap();
        let observer = wait_call(app.clone(), &job.id);
        wait_registered(&mut checked).await;
        let cancelled = runtime::open(&app.database)
            .unwrap()
            .update_job(
                &app.sessions.identity().caller(Some(A)),
                &job.id,
                Event::Cancel,
                runtime::now_ms(),
                None,
            )
            .unwrap();
        if mutate_scope {
            // A terminal hint cannot bypass the post-wait durable scope fence.
            let mut changed = cancelled.clone();
            changed.context.as_mut().unwrap().project = B.into();
            rusqlite::Connection::open(&app.database)
                .unwrap()
                .execute(
                    "UPDATE compute_jobs SET row=?1 WHERE id=?2",
                    rusqlite::params![serde_json::to_string(&changed).unwrap(), job.id],
                )
                .unwrap();
        }
        runtime::notify_terminal(&app.database, &cancelled);
        let (_, refusal) = tokio::time::timeout(std::time::Duration::from_secs(5), observer)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            refusal["code"],
            if mutate_scope {
                "not_visible"
            } else {
                "result_cancelled"
            }
        );
        if mutate_scope {
            assert!(refusal.get("job").is_none());
        }
    }
}

#[test]
fn result_wait_cli_bounds_and_typed_refusals_preserve_job_details() {
    let descriptor = ds_cli_contract::help::command_json(&crate::RESULT);
    assert_eq!(descriptor["contract"], 2);
    assert!(
        descriptor["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["name"] == "wait" && i["kind"] == "value" && i["required"] == false)
    );
    for refusal in [
        "result_pending",
        "result_failed",
        "result_cancelled",
        "result_wait_invalid",
    ] {
        assert!(
            descriptor["refusals"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["code"] == refusal)
        );
    }
    for value in ["0", "601", "-1", "1.5", "soon", "18446744073709551616"] {
        let input = wait_inputs(
            &crate::RESULT,
            &[
                "--wait",
                value,
                "--job",
                &"0".repeat(64),
                "--out",
                "absent.json",
                "--project",
                A,
            ],
        );
        assert_eq!(
            crate::result(&input, &wait_context()).unwrap_err().code(),
            "result_wait_invalid"
        );
    }
    for (code, class, status) in [
        ("result_pending", "unavailable", 503),
        ("result_failed", "failed", 502),
        ("result_cancelled", "conflict", 409),
    ] {
        let refusal = crate::typed_refusal(status,&serde_json::to_vec(&json!({"code":code,"class":class,"error":"retained","job":{"id":"retained"},"wait_seconds":600})).unwrap());
        assert_eq!(refusal.code(), code);
        assert_eq!(refusal.class().token(), class);
        assert_eq!(refusal.detail_value().unwrap()["job"]["id"], "retained");
        assert!(refusal.remedy_text().is_some());
    }
}
