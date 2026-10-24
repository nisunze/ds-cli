//! Real native queue admission/restart acceptance. Synthetic installation
//! receipts live only in ordinary scratch; producer artifacts remain exact.
#![cfg(unix)]
mod common;
#[path = "../src/test_directory.rs"]
mod test_directory;
use ds_cli_exec::{addons, External};
use ds_command_kernel::{
    compute_jobs::{Event, Phase},
    execution_context::{Limits, Principal},
    module_install::*,
    module_release::*,
};
use ds_compute_runtime::{self as runtime, Admission, Authorizer, HostIdentity, WorkerContext};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

struct Allowed;
impl Authorizer for Allowed {
    fn authorize(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}
fn identity() -> HostIdentity {
    HostIdentity {
        owner: "pinned-test-owner".into(),
        principal: Principal {
            uid: "pin-test-user".into(),
            lane: "canary".into(),
            deployment: "https://pin-test.example".into(),
            install_id: "pin-test-install".into(),
        },
    }
}
fn limits() -> Limits {
    Limits {
        global_running: 2,
        per_project_running: 1,
        global_queued: 8,
        per_project_queued: 4,
    }
}
fn admission<'a>(identity: &'a HostIdentity, key: &'a str, project: &'a str) -> Admission<'a> {
    Admission {
        identity,
        client: "durable-engine-pin-test",
        key,
        requested_project: Some(project),
        limits: limits(),
        now_ms: runtime::now_ms(),
    }
}
fn required(name: &str) -> PathBuf {
    PathBuf::from(
        std::env::var_os(name)
            .unwrap_or_else(|| panic!("{name} must name an existing producer artifact")),
    )
}
fn probe(owner: &'static External, path: &Path) -> Value {
    owner
        .capture_pinned(path)
        .unwrap()
        .call_json("build-info", &[], Duration::from_secs(20))
        .unwrap()
}
fn select(
    base: &Path,
    component: ModuleComponent,
    lane: &str,
    binary: &Path,
    info: &Value,
    generation: u64,
    trailer: &[u8],
) -> PathBuf {
    let mut bytes = fs::read(binary).unwrap();
    bytes.extend_from_slice(trailer);
    let sha256 = ds_compute_runtime::digest(&bytes);
    let root = base.join(component.as_str()).join(lane);
    let directory = root.join("versions").join(&sha256);
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(component.executable());
    fs::write(&path, &bytes).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let network = component == ModuleComponent::Network;
    let source = if network {
        info["engineIdentity"]["sourceSha"].as_str().unwrap()
    } else {
        info["source_sha"].as_str().unwrap()
    };
    let version = if network {
        info["engineIdentity"]["coreVersion"].as_str().unwrap()
    } else {
        info["version"].as_str().unwrap()
    };
    let manifest = if network {
        &info["engineIdentity"]["dependencyClosureSha256"]
    } else {
        &info["build_manifest_sha256"]
    };
    let release = validate_module_release(component.as_str(), lane, &json!({
        "schema":"ds.module-release/v1", "component":component, "channel":lane,
        "product_id":format!("rw.datasolutions.module.{}.{lane}",component.as_str()),
        "version":format!("{version}-r{generation}"), "protocol":component.protocol(),
        "released_at":"2026-10-14T10:00:00Z", "status":"active", "assets":[{
            "platform":"linux", "arch":"x86_64", "kind":"module-executable",
            "protocol":component.protocol(),
            "url":format!("https://storage.googleapis.com/synthetic-module-fixtures/modules/{}/{lane}/{version}-r{generation}/{}",component.as_str(), component.executable()),
            "size_bytes":bytes.len(), "sha256":sha256, "source_sha":source,
            "built_at":"2026-10-14T09:00:00Z", "source_revisions":{component.engine_owner():source},
            "engines":[{"engine":component.engine_owner(),
                "release":format!("{}@{version}+{source}",component.engine_owner()),
                "manifest_sha256":manifest}]
        }]
    })).unwrap();
    let installed = InstalledModule {
        schema: INSTALLED_SCHEMA.into(),
        authority: ReleaseAuthorityReceipt {
            gateway_origin: "https://synthetic-module-authority.ue.gateway.dev".into(),
            native_catalog_sha256: "a".repeat(64),
            release_sha256: ds_compute_runtime::digest(&serde_json::to_vec(&release).unwrap()),
        },
        release,
        platform: "linux".into(),
        engine_identity: info.clone(),
    };
    let selected = ModuleSelection {
        schema: SELECTION_SCHEMA.into(),
        component,
        channel: lane.into(),
        generation,
        active: installed,
        previous: None,
    };
    validate_selection(&selected, component, lane, "linux").unwrap();
    let staged = root.join("selection.next");
    fs::write(&staged, serde_json::to_vec(&selected).unwrap()).unwrap();
    fs::rename(staged, root.join("selection.json")).unwrap();
    path
}

fn submit(
    database: &Path,
    base: &Path,
    host: &HostIdentity,
    key: &str,
    solar: bool,
    input: &[u8],
) -> ds_command_kernel::compute_jobs::Job {
    let project = if solar {
        "durable-server-test"
    } else {
        "project-a"
    };
    if solar {
        runtime::submit_solar_with_module_base(
            database,
            &admission(host, key, project),
            input,
            Some(base),
        )
        .unwrap()
    } else {
        runtime::submit_with_module_base(
            database,
            &admission(host, key, project),
            input,
            Some(base),
        )
        .unwrap()
    }
}
#[test]
#[ignore = "requires exact qualified Network and Solar release artifacts; runs both real offline owners"]
fn admitted_program_survives_lane_activation_restart_retry_and_cancellation() {
    assert!(std::env::var_os("DS_NETWORK_ENGINE_BIN").is_none());
    assert!(std::env::var_os("DS_SOLAR_ENGINE_BIN").is_none());
    for solar in [false, true] {
        let directory = test_directory::tempdir().unwrap();
        let base = directory.path().join("addons");
        let database = directory.path().join("store.sqlite");
        let host = identity();
        let component = if solar {
            ModuleComponent::Solar
        } else {
            ModuleComponent::Network
        };
        let owner = if solar {
            &addons::SOLAR
        } else {
            &addons::NETWORK
        };
        let binary = required(if solar {
            "DS_PIN_SOLAR_BIN"
        } else {
            "DS_PIN_NETWORK_BIN"
        });
        let info = probe(owner, &binary);
        let alpha = select(&base, component, "canary", &binary, &info, 1, b"");
        let input = if solar {
            common::prepared_solar_request()
        } else {
            fs::read(required("DS_COMPUTE_NETWORK_FIXTURES").join("mixed.request.json")).unwrap()
        };
        let first = submit(&database, &base, &host, "retry-a", solar, &input);
        let second = submit(&database, &base, &host, "queued-a", solar, &input);
        let cancelled = submit(&database, &base, &host, "cancel-a", solar, &input);
        assert_eq!(
            first.engine_pin.as_ref().unwrap().executable,
            alpha.to_str().unwrap()
        );
        assert!(!first.engine_pin.as_ref().unwrap().development_override);
        assert_eq!(
            first.engine_pin.as_ref().unwrap().context,
            first.context.clone().unwrap()
        );
        let mut full = admission(
            &host,
            "capacity-no-probe",
            if solar {
                "durable-server-test"
            } else {
                "project-a"
            },
        );
        full.limits.global_queued = 3;
        full.limits.per_project_queued = 2;
        let refused = if solar {
            runtime::submit_solar_with_module_base(&database, &full, &input, Some(&base))
        } else {
            runtime::submit_with_module_base(&database, &full, &input, Some(&base))
        };
        assert!(
            matches!(refused, Err(runtime::SubmitError::Refused(refusal)) if refusal.code == "capacity_exhausted")
        );
        let mut store = runtime::open(&database).unwrap();
        let (claimed, _) = store
            .claim_job(
                &host.caller(None),
                "abandoned",
                runtime::now_ms() - 60_000,
                1_000,
                limits(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(claimed.id, first.id);
        store
            .update_job(
                &host.caller(None),
                &cancelled.id,
                Event::Cancel,
                runtime::now_ms(),
                None,
            )
            .unwrap();
        drop(store);
        let beta = select(
            &base,
            component,
            "canary",
            &binary,
            &info,
            2,
            b"\ncompatible-pin-fixture-beta\n",
        );
        assert_ne!(alpha, beta);
        // A restarted real worker is a new process, with no original caller,
        // selection or retained client object. Both inputs execute alpha.
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "retained_worker_child",
                "--ignored",
                "--nocapture",
            ])
            .env("DS_PIN_DATABASE", &database)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let store = runtime::open(&database).unwrap();
        for (job, attempts) in [(&first, 2), (&second, 1)] {
            let retained = store.job(&host.caller(None), &job.id).unwrap().unwrap();
            assert_eq!(retained.phase, Phase::Completed, "{:?}", retained.error);
            assert_eq!(retained.attempts, attempts);
            assert_eq!(retained.engine_pin, job.engine_pin);
            let bytes = store
                .job_result(&host.caller(None), &job.id)
                .unwrap()
                .unwrap();
            if solar {
                let value: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(value["engine"], job.engine_pin.as_ref().unwrap().identity);
                assert_eq!(value["outputs"].as_array().unwrap().len(), 1);
                assert_eq!(value["identity"]["project_id"], "durable-server-test");
            } else {
                assert_eq!(
                    bytes,
                    fs::read(
                        required("DS_COMPUTE_NETWORK_GOLDENS")
                            .join("mixed.original-cli.insertion.result.json")
                    )
                    .unwrap()
                );
            }
        }
        assert_eq!(
            store
                .job(&host.caller(None), &cancelled.id)
                .unwrap()
                .unwrap()
                .phase,
            Phase::Cancelled
        );
        assert!(store
            .job_result(&host.caller(None), &cancelled.id)
            .unwrap()
            .is_none());
        drop(store);
        // Reproduce a pin admitted on a wider host, retaining every scope
        // fence. The narrower restarted host refuses before any addon probe.
        let mut wider = second.clone();
        wider.id = runtime::digest(format!("{}-wider-host", component.as_str()).as_bytes());
        let context = wider.context.as_mut().unwrap();
        context.job_id = wider.id.clone();
        context.idempotency_key = "wider-host".into();
        let pin = wider.engine_pin.as_mut().unwrap();
        pin.context = context.clone();
        pin.workers = std::thread::available_parallelism().map_or(1, usize::from) + 1;
        if !solar {
            pin.identity["runtime"]["internalWorkerThreads"] = json!(pin.workers);
        }
        runtime::open(&database)
            .unwrap()
            .submit_job(&wider, &input, limits())
            .unwrap();
        assert!(runtime::run_one(
            &WorkerContext {
                path: database.clone(),
                identity: host.clone(),
                limits: limits(),
                auth: Arc::new(Allowed),
                observer: None,
            },
            "narrower-host"
        )
        .unwrap());
        let store = runtime::open(&database).unwrap();
        let refused = store.job(&host.caller(None), &wider.id).unwrap().unwrap();
        assert_eq!(refused.phase, Phase::Failed);
        assert!(refused
            .error
            .as_deref()
            .unwrap()
            .starts_with("compute_engine_pin_budget_unavailable:"));
        assert_eq!(refused.engine_pin, wider.engine_pin);
        assert_eq!(
            store
                .job_input(&host.caller(None), &wider.id)
                .unwrap()
                .unwrap(),
            input
        );
        assert!(store
            .job_result(&host.caller(None), &wider.id)
            .unwrap()
            .is_none());
        drop(store);
        if solar {
            let mut reduced_owner = second.clone();
            assert!(
                reduced_owner.engine_pin.as_ref().unwrap().workers > 1,
                "qualified owner-width refusal acceptance requires a host share above one"
            );
            reduced_owner.id = runtime::digest(b"solar-owner-limit-changed");
            let context = reduced_owner.context.as_mut().unwrap();
            context.job_id = reduced_owner.id.clone();
            context.idempotency_key = "owner-limit-changed".into();
            reduced_owner.engine_pin.as_mut().unwrap().context = context.clone();
            runtime::open(&database)
                .unwrap()
                .submit_job(&reduced_owner, &input, limits())
                .unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "retained_worker_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("DS_PIN_DATABASE", &database)
                .env("DS_PIN_EXPECT_OWNER_BUDGET_REFUSAL", &reduced_owner.id)
                .env("DS_SOLAR_WORKER_THREADS", "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let store = runtime::open(&database).unwrap();
            let refused = store
                .job(&host.caller(None), &reduced_owner.id)
                .unwrap()
                .unwrap();
            assert_eq!(refused.engine_pin, reduced_owner.engine_pin);
            assert_eq!(
                store
                    .job_input(&host.caller(None), &reduced_owner.id)
                    .unwrap()
                    .unwrap(),
                input
            );
            assert!(store
                .job_result(&host.caller(None), &reduced_owner.id)
                .unwrap()
                .is_none());
        }
        // Destroy current selection: completed/idempotent reads stay offline
        // and do not resolve/probe any addon again.
        fs::write(
            base.join(component.as_str()).join("canary/selection.json"),
            b"{}",
        )
        .unwrap();
        assert_eq!(
            submit(&database, &base, &host, "retry-a", solar, &input),
            runtime::open(&database)
                .unwrap()
                .job(&host.caller(None), &first.id)
                .unwrap()
                .unwrap()
        );
        let invalid = if solar {
            runtime::submit_solar_with_module_base(
                &database,
                &admission(&host, "new-bad", "wrong-project"),
                &input,
                Some(&base),
            )
        } else {
            runtime::submit_with_module_base(
                &database,
                &admission(&host, "new-bad", "project-a"),
                b"{}",
                Some(&base),
            )
        };
        assert!(invalid.is_err());
        println!("retained {}: frozen pin and original result verified after activation, restart, expired-lease retry and cancellation; alpha={}, beta={}",component.as_str(),alpha.display(),beta.display());
    }
}
#[test]
#[ignore = "internal restart process for real durable admission acceptance"]
fn retained_worker_child() {
    let context = WorkerContext {
        path: required("DS_PIN_DATABASE"),
        identity: identity(),
        limits: limits(),
        auth: Arc::new(Allowed),
        observer: None,
    };
    if let Ok(id) = std::env::var("DS_PIN_EXPECT_OWNER_BUDGET_REFUSAL") {
        assert!(runtime::run_one(&context, "restarted-owner-limit").unwrap());
        let refused = runtime::open(&context.path)
            .unwrap()
            .job(&context.identity.caller(None), &id)
            .unwrap()
            .unwrap();
        assert_eq!(refused.phase, Phase::Failed);
        assert!(refused
            .error
            .as_deref()
            .unwrap()
            .starts_with("compute_engine_pin_budget_unavailable:"));
        assert!(!runtime::run_one(&context, "owner-limit-no-demand").unwrap());
        return;
    }
    assert!(runtime::run_one(&context, "restarted-1").unwrap());
    assert!(runtime::run_one(&context, "restarted-2").unwrap());
    assert!(!runtime::run_one(&context, "completed-no-demand").unwrap());
}

fn solar_selection_directory() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    root
}

fn isolated_solar_admission(override_path: Option<&Path>) {
    let root = solar_selection_directory();
    let child = root.path().join("admission-selection-test");
    fs::hard_link(std::env::current_exe().unwrap(), &child).unwrap();
    use std::os::unix::fs::PermissionsExt;
    for name in ["solar", "ds-solar"] {
        let trap = root.path().join(name);
        fs::write(
            &trap,
            format!(
                "#!/bin/sh\necho called > '{}'\nexit 99\n",
                root.path().join("called").display()
            ),
        )
        .unwrap();
        fs::set_permissions(trap, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut command = Command::new(child);
    command
        .args([
            "--exact",
            "solar_admission_selection_child",
            "--ignored",
            "--nocapture",
        ])
        .env("DS_SOLAR_SELECTION_ROOT", root.path())
        .env("PATH", root.path())
        .env_remove("DS_SOLAR_ENGINE_BIN");
    if let Some(path) = override_path {
        command.env("DS_SOLAR_ENGINE_BIN", path);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.path().join("called").exists());
}

#[test]
fn missing_and_corrupt_solar_admission_never_reserve_or_probe_sibling_or_path() {
    isolated_solar_admission(None);
}

#[test]
#[ignore = "isolated durable Solar selection control"]
fn solar_admission_selection_child() {
    let root = required("DS_SOLAR_SELECTION_ROOT");
    let base = root.join("addons");
    let database = root.join("store.sqlite");
    let host = identity();
    let input = common::prepared_solar_request();
    if let Some(path) = std::env::var_os("DS_SOLAR_ENGINE_BIN") {
        fs::create_dir_all(base.join("solar/canary")).unwrap();
        fs::write(base.join("solar/canary/selection.json"), b"{torn").unwrap();
        let job = submit(&database, &base, &host, "explicit-override", true, &input);
        let pin = job.engine_pin.as_ref().unwrap();
        assert!(pin.development_override);
        assert_eq!(Path::new(&pin.executable), fs::canonicalize(path).unwrap());
        let context = WorkerContext {
            path: database.clone(),
            identity: host.clone(),
            limits: limits(),
            auth: Arc::new(Allowed),
            observer: None,
        };
        assert!(runtime::run_one(&context, "explicit-override-worker").unwrap());
        assert_eq!(
            runtime::open(&database)
                .unwrap()
                .job(&host.caller(None), &job.id)
                .unwrap()
                .unwrap()
                .phase,
            Phase::Completed
        );
    } else {
        for (key, expected) in [
            ("missing", "solar_engine_missing"),
            ("corrupt", "module_receipt_invalid"),
        ] {
            if key == "corrupt" {
                fs::create_dir_all(base.join("solar/canary")).unwrap();
                fs::write(base.join("solar/canary/selection.json"), b"{torn").unwrap();
            }
            let error = runtime::submit_solar_with_module_base(
                &database,
                &admission(&host, key, "durable-server-test"),
                &input,
                Some(&base),
            )
            .unwrap_err();
            assert!(format!("{error:?}").contains(expected), "{error:?}");
            assert!(runtime::open(&database)
                .unwrap()
                .jobs(&host.caller(None), 10)
                .unwrap()
                .is_empty());
        }
    }
    assert!(!root.join("called").exists());
}

#[test]
#[ignore = "requires an explicitly qualified retained Solar executable"]
fn solar_managed_pin_and_explicit_override_survive_unavailable_current_selection() {
    assert!(std::env::var_os("DS_SOLAR_ENGINE_BIN").is_none());
    let binary = required("DS_PIN_SOLAR_BIN");
    let info = probe(&addons::SOLAR, &binary);
    let root = solar_selection_directory();
    let base = root.path().join("addons");
    let database = root.path().join("store.sqlite");
    let host = identity();
    let input = common::prepared_solar_request();
    let first = select(
        &base,
        ModuleComponent::Solar,
        "canary",
        &binary,
        &info,
        1,
        b"",
    );
    let job = submit(&database, &base, &host, "managed-retained", true, &input);
    assert_eq!(
        job.engine_pin.as_ref().unwrap().executable,
        first.to_str().unwrap()
    );
    assert!(!job.engine_pin.as_ref().unwrap().development_override);
    let next = select(
        &base,
        ModuleComponent::Solar,
        "canary",
        &binary,
        &info,
        2,
        b"\nselection-control\n",
    );
    assert_ne!(first, next);
    fs::write(base.join("solar/canary/selection.json"), b"{torn").unwrap();
    let context = WorkerContext {
        path: database.clone(),
        identity: host.clone(),
        limits: limits(),
        auth: Arc::new(Allowed),
        observer: None,
    };
    assert!(runtime::run_one(&context, "retained-managed-worker").unwrap());
    let store = runtime::open(&database).unwrap();
    let completed = store.job(&host.caller(None), &job.id).unwrap().unwrap();
    assert_eq!(completed.phase, Phase::Completed, "{:?}", completed.error);
    assert_eq!(completed.engine_pin, job.engine_pin);
    let result: Value = serde_json::from_slice(
        &store
            .job_result(&host.caller(None), &job.id)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["engine"], job.engine_pin.as_ref().unwrap().identity);
    assert_eq!(
        submit(&database, &base, &host, "managed-retained", true, &input),
        completed
    );
    isolated_solar_admission(Some(&binary));
}

fn solar_integrity_wrapper(root: &Path, binary: &Path, mutate_probe: bool) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join("solar-wrapper");
    fs::write(&path, format!(
        "#!/usr/bin/python3\nimport os, subprocess, sys\nwith open({calls:?}, 'a') as f: f.write(sys.argv[1] + '\\n')\nresult = subprocess.run([{binary:?}] + sys.argv[1:])\nif (sys.argv[1] == 'build-info') == {probe}:\n s = os.stat(__file__)\n data = open(__file__, 'rb').read()\n open(__file__, 'wb').write(data[:-2] + b'1\\n')\n os.utime(__file__, ns=(s.st_atime_ns, s.st_mtime_ns))\n if sys.argv[1] != 'build-info': open({complete:?}, 'w').write(str(result.returncode))\nsys.exit(result.returncode)\n# stamp:0\n",
        calls=root.join("calls").to_str().unwrap(), binary=binary.to_str().unwrap(),
        probe=if mutate_probe { "True" } else { "False" },
        complete=root.join("completed").to_str().unwrap()
    )).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
#[ignore = "requires DS_PIN_SOLAR_BIN qualified exact producer executable"]
fn solar_receipt_capture_and_execution_tamper_never_adopt_a_job_result() {
    assert!(std::env::var_os("DS_SOLAR_ENGINE_BIN").is_none());
    let binary = required("DS_PIN_SOLAR_BIN");
    let info = probe(&addons::SOLAR, &binary);
    for stage in ["capture", "queued", "execution"] {
        let root = solar_selection_directory();
        let wrapper = solar_integrity_wrapper(root.path(), &binary, stage == "capture");
        let base = root.path().join("addons");
        let executable = select(
            &base,
            ModuleComponent::Solar,
            "canary",
            &wrapper,
            &info,
            1,
            b"",
        );
        let before = fs::metadata(&executable).unwrap();
        let database = root.path().join("store.sqlite");
        let host = identity();
        let input = common::prepared_solar_request();
        let admitted = runtime::submit_solar_with_module_base(
            &database,
            &admission(&host, stage, "durable-server-test"),
            &input,
            Some(&base),
        );
        if stage == "capture" {
            let error = admitted.unwrap_err();
            assert!(format!("{error:?}").contains("module_"), "{error:?}");
            assert!(runtime::open(&database)
                .unwrap()
                .jobs(&host.caller(None), 10)
                .unwrap()
                .is_empty());
        } else {
            let job = admitted.unwrap();
            let calls_before = fs::read(root.path().join("calls")).unwrap();
            fs::write(base.join("solar/canary/selection.json"), b"{torn").unwrap();
            if stage == "queued" {
                let mut bytes = fs::read(&executable).unwrap();
                let last = bytes.len() - 2;
                bytes[last] = b'1';
                fs::write(&executable, bytes).unwrap();
                fs::File::options()
                    .write(true)
                    .open(&executable)
                    .unwrap()
                    .set_times(fs::FileTimes::new().set_modified(before.modified().unwrap()))
                    .unwrap();
            }
            let context = WorkerContext {
                path: database.clone(),
                identity: host.clone(),
                limits: limits(),
                auth: Arc::new(Allowed),
                observer: None,
            };
            assert!(runtime::run_one(&context, "integrity-worker").unwrap());
            let store = runtime::open(&database).unwrap();
            let retained = store.job(&host.caller(None), &job.id).unwrap().unwrap();
            assert_eq!(retained.phase, Phase::Failed, "{:?}", retained.error);
            let expected = if stage == "queued" {
                "compute_engine_pin_changed"
            } else {
                "solar_engine_changed_during_job"
            };
            assert!(
                retained.error.as_deref().unwrap().contains(expected),
                "{:?}",
                retained.error
            );
            assert_eq!(retained.engine_pin, job.engine_pin);
            assert_eq!(
                store
                    .job_input(&host.caller(None), &job.id)
                    .unwrap()
                    .unwrap(),
                input
            );
            assert!(store
                .job_result(&host.caller(None), &job.id)
                .unwrap()
                .is_none());
            if stage == "queued" {
                assert_eq!(fs::read(root.path().join("calls")).unwrap(), calls_before);
                assert!(!root.path().join("completed").exists());
            } else {
                assert_eq!(
                    fs::read_to_string(root.path().join("completed")).unwrap(),
                    "0",
                    "real engine succeeded before the postcheck refused adoption"
                );
            }
        }
        let after = fs::metadata(&executable).unwrap();
        assert_eq!(before.len(), after.len());
        assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    }
}
