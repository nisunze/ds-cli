//! Admission fixtures are isolated receipts, never installation/publication authority.
use super::*;
use ds_command_kernel::{module_install::*, module_release::*};
use ds_network_contracts::engine::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path, process::Command};

fn isolated(real: bool) {
    let directory = tempfile::tempdir().unwrap();
    let mut command = Command::new(env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "native::admission_tests::isolated_admission",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("DS_GRID_ADMISSION_ROOT", directory.path())
        .env("HOME", directory.path())
        .env("TMPDIR", directory.path())
        .env_remove("DS_NETWORK_ENGINE_BIN");
    if real {
        let engine = env::var_os("DS_GRID_ADMISSION_REAL_ENGINE")
            .expect("select an existing qualified release Network executable");
        command.env(
            "DS_GRID_ADMISSION_REAL_ENGINE",
            fs::canonicalize(engine).unwrap(),
        );
    } else {
        command.env_remove("DS_GRID_ADMISSION_REAL_ENGINE");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
}

#[test]
fn release_lane_admission_and_development_selection() {
    isolated(false);
}

#[test]
#[ignore = "requires DS_GRID_ADMISSION_REAL_ENGINE naming an existing qualified release"]
fn real_engine_retains_task_capture_across_activation() {
    isolated(true);
}

struct Fixture {
    root: PathBuf,
    calls: PathBuf,
    info: Value,
    real: Option<PathBuf>,
}

fn quoted(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

impl Fixture {
    fn new(root: PathBuf) -> Self {
        let calls = root.join("requests");
        fs::write(&calls, b"").unwrap();
        let workers = std::thread::available_parallelism().map_or(1, usize::from);
        let real = env::var_os("DS_GRID_ADMISSION_REAL_ENGINE").map(PathBuf::from);
        let info = if let Some(engine) = &real {
            fs::write(&calls, b"setup:build-info\n").unwrap();
            let output = Command::new(engine)
                .args(["build-info", "--workers", &workers.to_string()])
                .output()
                .unwrap();
            assert!(output.status.success());
            serde_json::from_slice(&output.stdout).unwrap()
        } else {
            serde_json::to_value(NetworkBuildInfo {
                schema: "ds.engine-build/v1".into(),
                module: "network".into(),
                engine_identity: EngineIdentity {
                    schema: "ds.network-engine/v1".into(),
                    protocol_major: 1,
                    protocol_minor: 0,
                    core_version: "0.1.0".into(),
                    source_sha: Some("b".repeat(40)),
                    dependency_closure_sha256: Some("c".repeat(64)),
                    environment: ExecutionEnvironment::Native,
                    runtime: ExecutionRuntime {
                        environment: ExecutionEnvironment::Native,
                        logical_cpus: workers,
                        internal_worker_threads: workers,
                    },
                    json_object_order: JsonObjectOrder::Insertion,
                    capabilities: vec![
                        NetworkCapability::LvProcessingV1,
                        NetworkCapability::LvPublicationV1,
                        NetworkCapability::LvAnalysisValidationV1,
                        NetworkCapability::GridQueryV1,
                        NetworkCapability::GridInspectV1,
                        NetworkCapability::GridApplyV1,
                        NetworkCapability::GridPublicationV1,
                        NetworkCapability::ProfileSessionV1,
                        NetworkCapability::ReportEngineeringV1,
                        NetworkCapability::ReportSizingV1,
                        NetworkCapability::ReportFeederSelectionV1,
                    ],
                },
                build: Some(NetworkNativeBuild {
                    target: "x86_64-unknown-linux-gnu".into(),
                    profile: "release".into(),
                    rustc: "synthetic admission fixture".into(),
                    cargo_lock_sha256: "d".repeat(64),
                }),
            })
            .unwrap()
        };
        Self {
            root,
            calls,
            info,
            real,
        }
    }

    fn program(&self, label: &str) -> Vec<u8> {
        let body = if let Some(real) = &self.real {
            format!("exec {} \"$@\"\n", quoted(real))
        } else {
            format!(
                "test \"$1\" = build-info || exit 91\nprintf '%s\\n' '{}'\n",
                serde_json::to_string(&self.info).unwrap()
            )
        };
        format!(
            "#!/bin/sh\nprintf '%s:%s\\n' '{label}' \"$1\" >> {}\n{body}",
            quoted(&self.calls)
        )
        .into_bytes()
    }

    fn write_program(&self, path: &Path, label: &str) {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, self.program(label)).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn selection(&self, lane: &str) -> PathBuf {
        self.root
            .join(".local/lib/ds-addons/network")
            .join(lane)
            .join("selection.json")
    }

    fn select(&self, lane: &str, generation: u64, label: &str) -> PathBuf {
        let bytes = self.program(label);
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let root = self.selection(lane).parent().unwrap().to_path_buf();
        let executable = root.join("versions").join(&digest).join("ds-network");
        self.write_program(&executable, label);
        let engine = &self.info["engineIdentity"];
        let source = engine["sourceSha"].as_str().unwrap();
        let version = engine["coreVersion"].as_str().unwrap();
        let release = validate_module_release("network", lane, &json!({
            "schema":"ds.module-release/v1", "component":"network", "channel":lane,
            "product_id":format!("rw.datasolutions.module.network{}", if lane == "canary" {".canary"} else {""}),
            "version":format!("{version}-r{generation}"), "protocol":"ds.network-engine/v1",
            "released_at":"2026-10-14T10:00:00Z", "status":"active", "assets":[{
                "platform":"linux", "arch":"x86_64", "kind":"module-executable",
                "protocol":"ds.network-engine/v1",
                "url":format!("https://storage.googleapis.com/synthetic-admission-fixtures/modules/network/{lane}/{version}-r{generation}/ds-network"),
                "size_bytes":bytes.len(), "sha256":digest, "source_sha":source,
                "built_at":"2026-10-14T09:00:00Z", "source_revisions":{"ds-network":source},
                "engines":[{"engine":"ds-network", "release":format!("ds-network@{version}+{source}"),
                    "manifest_sha256":engine["dependencyClosureSha256"]}]
            }]
        })).unwrap();
        let selected = ModuleSelection {
            schema: SELECTION_SCHEMA.into(),
            component: ModuleComponent::Network,
            channel: lane.into(),
            generation,
            previous: None,
            active: InstalledModule {
                schema: INSTALLED_SCHEMA.into(),
                authority: ReleaseAuthorityReceipt {
                    gateway_origin: "https://synthetic-admission.ue.gateway.dev".into(),
                    native_catalog_sha256: "a".repeat(64),
                    release_sha256: format!(
                        "{:x}",
                        Sha256::digest(serde_json::to_vec(&release).unwrap())
                    ),
                },
                release,
                platform: "linux".into(),
                engine_identity: self.info.clone(),
            },
        };
        validate_selection(&selected, ModuleComponent::Network, lane, "linux").unwrap();
        let next = root.join("selection.next");
        fs::write(&next, serde_json::to_vec(&selected).unwrap()).unwrap();
        fs::rename(next, self.selection(lane)).unwrap();
        executable
    }

    fn expect_calls(&self, expected: &[&str]) {
        let text = fs::read_to_string(&self.calls).unwrap();
        let actual: Vec<_> = text.lines().collect();
        assert_eq!(actual, expected);
        println!(
            "engine requests: {}",
            serde_json::to_string(&actual).unwrap()
        );
        fs::write(&self.calls, b"").unwrap();
    }

    fn refuse(&self, lane: &str, code: &str) -> Failure {
        let error = capture_for_release_lane(lane)
            .err()
            .expect("selection must refuse");
        assert_eq!(error.code(), code);
        self.expect_calls(&[]);
        error
    }
}

#[test]
#[ignore = "internal subprocess with isolated HOME, PATH and override"]
fn isolated_admission() {
    let fixture = Fixture::new(PathBuf::from(
        env::var_os("DS_GRID_ADMISSION_ROOT").unwrap(),
    ));
    fixture.expect_calls(if fixture.real.is_some() {
        &["setup:build-info"]
    } else {
        &[]
    });
    let fallback_dir = fixture.root.join("fallback");
    fixture.write_program(&fallback_dir.join("ds-network"), "fallback");
    unsafe { env::set_var("PATH", format!("{}:/usr/bin:/bin", fallback_dir.display())) };

    let stable = fixture.select("stable", 1, "stable");
    let canary = fixture.select("canary", 1, "canary-v1");
    assert_eq!(
        capture_for_release_lane("stable").unwrap().executable(),
        stable
    );
    let retained = capture_for_release_lane("canary").unwrap();
    assert_eq!(retained.executable(), canary);
    fixture.expect_calls(&["stable:build-info", "canary-v1:build-info"]);

    // A valid receipt from the other lane is still a refusal, never a redirect.
    fs::write(
        fixture.selection("canary"),
        fs::read(fixture.selection("stable")).unwrap(),
    )
    .unwrap();
    fixture.refuse("canary", "network_engine_incompatible");
    fixture.refuse("preview", "network_engine_refused");

    fs::write(fixture.selection("canary"), b"{torn").unwrap();
    fixture.refuse("canary", "network_engine_incompatible");
    assert_eq!(
        capture_for_release_lane("stable").unwrap().executable(),
        stable
    );
    fixture.expect_calls(&["stable:build-info"]);

    fs::write(
        fixture.selection("canary"),
        serde_json::to_vec(&json!({
            "schema":REMOVED_SELECTION_SCHEMA, "component":"network", "channel":"canary",
            "generation":2, "state":"removed"
        }))
        .unwrap(),
    )
    .unwrap();
    // The existing Network owner maps module_removed to its closed generic code.
    let removed = fixture.refuse("canary", "network_engine_refused");
    assert!(removed.to_string().contains("module_removed"));

    let explicit = fixture.root.join("explicit-network");
    fixture.write_program(&explicit, "explicit");
    unsafe { env::set_var("DS_NETWORK_ENGINE_BIN", &explicit) };
    for lane in ["stable", "canary"] {
        let client = capture_for_release_lane(lane).unwrap();
        assert_eq!(client.executable(), explicit);
        assert!(client.development_override());
    }
    fixture.expect_calls(&["explicit:build-info", "explicit:build-info"]);
    fs::write(fixture.selection("canary"), b"{}").unwrap();
    assert_eq!(
        capture_for_release_lane("canary").unwrap().executable(),
        explicit
    );
    fixture.expect_calls(&["explicit:build-info"]);
    unsafe { env::set_var("DS_NETWORK_ENGINE_BIN", fixture.root.join("missing")) };
    fixture.refuse("stable", "network_engine_missing");
    fixture.refuse("canary", "network_engine_missing");
    unsafe { env::remove_var("DS_NETWORK_ENGINE_BIN") };

    let next = fixture.select("canary", 3, "canary-v3");
    assert_ne!(canary, next);
    assert_eq!(
        capture_for_release_lane("canary").unwrap().executable(),
        next
    );
    let reopened = CapturedNetworkClient::from_pin(&retained.pin(), &|| false).unwrap();
    assert_eq!(reopened.executable(), canary);
    assert_eq!(reopened.executable_sha256(), retained.executable_sha256());
    fixture.expect_calls(&["canary-v3:build-info", "canary-v1:build-info"]);
    if fixture.real.is_some() {
        use ds_network_contracts::grid_read::{
            NATIVE_GRID_READ_REQUEST_SCHEMA_V1, NativeGridReadRequestV1,
        };
        let scratch = fixture.root.join("task");
        fs::create_dir(&scratch).unwrap();
        let result = retained
            .grid_read(
                &NativeGridReadRequestV1 {
                    schema: NATIVE_GRID_READ_REQUEST_SCHEMA_V1.into(),
                    issue_limit: 50,
                    advisory_limit: 50,
                },
                b"damaged container",
                &scratch,
                &|| false,
            )
            .unwrap()
            .result;
        assert!(!result.container.verified);
        assert!(result.model.is_none());
        fixture.expect_calls(&["canary-v1:grid-read"]);
    }

    // Local builds retain their established unmanaged sibling/PATH selection;
    // corrupt managed state must not silently turn a dev build into a release.
    for lane in ["stable", "canary"] {
        fs::write(fixture.selection(lane), b"{}").unwrap();
    }
    assert_eq!(
        capture_for_release_lane("local").unwrap().executable(),
        fallback_dir.join("ds-network")
    );
    if ds_cli_auth::reliability::release_lane() == "local" {
        assert_eq!(
            capture().unwrap().executable(),
            fallback_dir.join("ds-network")
        );
        fixture.expect_calls(&["fallback:build-info", "fallback:build-info"]);
    } else {
        fixture.expect_calls(&["fallback:build-info"]);
    }
    unsafe { env::set_var("DS_NETWORK_ENGINE_BIN", &explicit) };
    assert_eq!(
        capture_for_release_lane("local").unwrap().executable(),
        explicit
    );
    fixture.expect_calls(&["explicit:build-info"]);
}
