//! Held-first reads through the real `ds` binary under a disposable native
//! identity (DECOUPLING-012): `design config read`, `design status` and
//! `report project export --held-export` answer from what this device holds
//! with no network, acquire only what is missing, and refuse a held-only read
//! of nothing with its typed `*_not_held` code. The fixture gateway does not
//! resolve, so any network read would fail the held assertions.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use ds_command_kernel::durable_store::{Call, Identity, Principal, Request};
use ds_command_kernel::retained_reads::{Kind, Operation};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const OWNER: &str = "held-first-owner";
const PROJECT: &str = "held_project";
const EMPTY_PROJECT: &str = "never_held_project";
const LANE: &str = "stable";
const SECRET: &str = "fixture-only-refresh-never-output";
const INSTALL: &str = "123e4567-e89b-42d3-a456-426614174000";

struct Fixture {
    root: tempfile::TempDir,
    audience: String,
    /// A proxy that never answers: an offline read must not even connect.
    dead_proxy: std::net::TcpListener,
}

/// Create `path` and every missing parent owner-only (native state refuses a
/// group- or world-writable ancestor).
fn private_dir(path: &Path) {
    if path.exists() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    if let Some(parent) = path.parent() {
        private_dir(parent);
    }
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn private_file(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn receipt_config() -> Value {
    let sheets_json =
        r#"{"project_settings":[{"parameter":"report_formats","value":"voltage_drop"}]}"#;
    json!({
        "sheets": {"project_settings": [{"parameter":"report_formats","value":"voltage_drop"}],
                   "transfo_sizes": [{"kva": 50}]},
        "future_document_member": {"producer": "independent-build"},
        "network_reporter_input_receipt": {
            "schema": 1, "country": "Burundi", "sheets_json": sheets_json,
            "sheets_sha256": sha(sheets_json.as_bytes()), "reference_semantic_sha256": "b".repeat(64)
        }
    })
}

impl Fixture {
    fn new() -> Self {
        // Native state refuses group/world-writable ancestors (/tmp is 1777),
        // so the disposable root lives under the owner-only cache directory.
        let base = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache");
        let root = tempfile::Builder::new()
            .prefix("ds-held-first-")
            .tempdir_in(base)
            .unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let mut audience = Sha256::new();
        for value in [
            ds_client_core::DEVICE_CREDENTIAL_AUDIENCE_SCHEMA,
            LANE,
            ds_client_core::NATIVE_CLIENT_ID,
            "fixture-project",
        ] {
            audience.update((value.len() as u64).to_be_bytes());
            audience.update(value.as_bytes());
        }
        let audience = format!("{:x}", audience.finalize());
        let dead_proxy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        dead_proxy.set_nonblocking(true).unwrap();
        let fixture = Self {
            root,
            audience,
            dead_proxy,
        };
        let key = sha(format!("ds-client/{LANE}/{}", fixture.audience).as_bytes());
        private_dir(&fixture.config());
        private_dir(&fixture.config().join("ds"));
        let credentials = fixture.config().join("ds/credentials");
        private_dir(&credentials);
        private_file(
            &credentials.join(format!("{key}.json")),
            &serde_json::to_vec(&json!({
                "schema":"ds-client.refresh/v1", "credential_audience":fixture.audience,
                "uid":OWNER, "email":"held@example.test", "refresh_token":SECRET,
            }))
            .unwrap(),
        );
        // The lane's existing install identity (the Server registered once).
        let install = fixture.config().join("ds/edge-admission").join(LANE);
        private_dir(&install);
        private_file(
            &install.join("install-id"),
            format!("{INSTALL}\n").as_bytes(),
        );
        for dir in [fixture.state(), fixture.data()] {
            private_dir(&dir);
        }
        fixture
    }
    fn config(&self) -> PathBuf {
        self.root.path().join("config")
    }
    fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }
    fn data(&self) -> PathBuf {
        self.root.path().join("data")
    }
    /// A held read: every proxy is a listener that never answers, and the
    /// listener must not have seen a single connection afterwards.
    fn ds_offline(&self, args: &[&str]) -> Value {
        let proxy = format!("http://{}", self.dead_proxy.local_addr().unwrap());
        let envelope = self.run(
            args,
            &[
                ("HTTP_PROXY", &proxy),
                ("HTTPS_PROXY", &proxy),
                ("ALL_PROXY", &proxy),
                ("NO_PROXY", ""),
            ],
        );
        assert!(
            self.dead_proxy.accept().is_err(),
            "{args:?} attempted a network connection"
        );
        envelope
    }
    fn ds(&self, args: &[&str]) -> Value {
        self.run(args, &[])
    }
    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .env_clear()
            .envs(extra.iter().copied())
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root.path())
            .env("XDG_STATE_HOME", self.state())
            .env("XDG_DATA_HOME", self.data())
            .env("DS_CONFIG_HOME", self.config())
            .env(
                "DS_NATIVE_CLIENT_PROFILE_BUNDLE",
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../ds-cli-auth/tests/fixtures/development-catalog.json"),
            )
            .env("DS_CLI_NONINTERACTIVE", "1")
            // Refusals are not reported to SRE here: only the read itself may touch the network.
            .env("DS_SRE_REPORTING", "off")
            .env(
                "DS_DESKTOP_DESCRIPTOR",
                self.root.path().join("no-desktop.json"),
            )
            .args(args)
            .args(["--lane", LANE, "--output", "json"])
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(SECRET));
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("{args:?}: {}", String::from_utf8_lossy(&output.stderr)))
    }
    /// Retain a configuration exactly as a refresh does: Core's protected
    /// Store, the Kernel retained-read owner, the CLI's identity fence.
    fn hold_configuration(&self, project: &str, document: Value) {
        let directory = self.state().join("ds/server").join(LANE);
        private_dir(&directory);
        let identity = Identity {
            principal: Principal::Account(OWNER.into()),
            lane: LANE.into(),
            audience: ds_cli_auth::job_owner_fence(OWNER, LANE, &self.audience).unwrap(),
        };
        ds_sync_store::Store::open(&directory.join("store.sqlite"))
            .unwrap()
            .durable_store(
                &identity,
                Call {
                    request: Request::RetainedReads {
                        operation: Operation::Write {
                            kind: Kind::ProjectConfig,
                            project: Some(project.into()),
                            key: String::new(),
                            value: document,
                            now: 1_760_000_000_000,
                            expect_generation: None,
                        },
                    },
                    expect_owner: None,
                },
                None,
            )
            .unwrap();
    }
    /// Retain a whole-project status observation exactly where and how the
    /// native status owner keeps it (ds-project-data `metadata::transformer_status`).
    fn hold_status(&self, project: &str, rows: Value) {
        use ds_command_kernel::{sql_effect::execute, transformer_status_store as status};
        let scope = status::Scope {
            principal: OWNER.into(),
            lane: LANE.into(),
            audience: self.audience.clone(),
            project: project.into(),
        };
        let account =
            sha(&serde_json::to_vec(&(&scope.principal, &scope.lane, &scope.audience)).unwrap());
        let directory = self
            .data()
            .join("ds/layers/project-metadata/v1")
            .join(account)
            .join(sha(&serde_json::to_vec(&scope.project).unwrap()));
        for dir in directory.ancestors().take(5) {
            if dir.starts_with(self.data()) {
                private_dir(dir);
            }
        }
        let path = directory.join("transformer-status.sqlite");
        if !path.exists() {
            private_file(&path, b"");
        }
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA journal_mode=WAL; BEGIN IMMEDIATE;")
            .unwrap();
        execute(&connection, &status::schema()).unwrap();
        let document = json!({"user_email": "held@example.test", "transformers": rows});
        execute(
            &connection,
            &status::write(&scope, &document, 1_760_000_000_000).unwrap(),
        )
        .unwrap();
        connection
            .execute_batch("PRAGMA user_version=2; COMMIT;")
            .unwrap();
        drop(connection);
        for entry in fs::read_dir(&directory).unwrap() {
            fs::set_permissions(entry.unwrap().path(), fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
}

fn ok(envelope: &Value) -> &Value {
    assert_eq!(envelope["status"], "ok", "{envelope}");
    &envelope["data"]
}

fn refused(envelope: &Value) -> &str {
    assert_eq!(envelope["status"], "error", "{envelope}");
    envelope["error"]["code"].as_str().unwrap()
}

#[test]
fn configuration_reads_answer_from_held_sqlite_offline_and_acquire_only_what_is_missing() {
    let fixture = Fixture::new();
    fixture.hold_configuration(PROJECT, receipt_config());
    // Default document read: held-first acquire answers from SQLite, offline.
    let out = fixture.root.path().join("config-out.json");
    let data = ok(&fixture.ds_offline(&[
        "design",
        "config",
        "read",
        "--project",
        PROJECT,
        "--document",
        "--out",
        out.to_str().unwrap(),
    ]))
    .clone();
    assert_eq!(data["observation"]["source"], "held");
    let file: Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(
        file["future_document_member"]["producer"],
        "independent-build"
    );
    // Default sheet read is held-first too; explicit held never reads.
    for args in [
        vec![
            "design",
            "config",
            "read",
            "--project",
            PROJECT,
            "--sheet",
            "transfo_sizes",
        ],
        vec![
            "design",
            "config",
            "read",
            "--project",
            PROJECT,
            "--sheet",
            "transfo_sizes",
            "--read",
            "held",
        ],
    ] {
        let data = ok(&fixture.ds_offline(&args)).clone();
        assert_eq!(data["observation"]["source"], "held", "{args:?}");
        assert_eq!(data["total"], 1, "{args:?}");
    }
    // Explicit refresh reads the network (unreachable here) and refuses.
    let refresh = fixture.ds(&[
        "design",
        "config",
        "read",
        "--project",
        PROJECT,
        "--document",
        "--read",
        "refresh",
    ]);
    assert_ne!(refused(&refresh), "project_config_not_held");
    // The held document is untouched by the failed refresh.
    assert_eq!(
        ok(&fixture.ds_offline(&[
            "design",
            "config",
            "read",
            "--project",
            PROJECT,
            "--document",
            "--read",
            "held"
        ]))["document"]["future_document_member"]["producer"],
        "independent-build"
    );
    // Nothing held: held-only is the typed refusal; acquire attempts the one read.
    let held = fixture.ds_offline(&[
        "design",
        "config",
        "read",
        "--project",
        EMPTY_PROJECT,
        "--document",
        "--read",
        "held",
    ]);
    assert_eq!(refused(&held), "project_config_not_held");
    let acquire = fixture.ds(&[
        "design",
        "config",
        "read",
        "--project",
        EMPTY_PROJECT,
        "--document",
    ]);
    assert_ne!(refused(&acquire), "project_config_not_held");
}

#[test]
fn status_reads_answer_from_the_held_observation_and_match_rows_by_kernel_identity() {
    let fixture = Fixture::new();
    let held = fixture.ds_offline(&["design", "status", "--project", PROJECT, "--read", "held"]);
    assert_eq!(refused(&held), "design_status_not_held");
    // Acquire with nothing held attempts the one read (unreachable here).
    let acquire = fixture.ds(&["design", "status", "--project", PROJECT]);
    assert_ne!(refused(&acquire), "design_status_not_held");
    fixture.hold_status(
        PROJECT,
        json!([{"name":"TX-1","kind":"individual","metadata":{"version":3,"content_digest":"c".repeat(64)}},
               {"name":"other","kind":"individual","future_row_field":{"x":1}}]),
    );
    for args in [
        vec!["design", "status", "--project", PROJECT],
        vec!["design", "status", "--project", PROJECT, "--read", "held"],
    ] {
        let data = ok(&fixture.ds_offline(&args)).clone();
        assert_eq!(data["observation"]["source"], "held", "{args:?}");
        assert_eq!(data["count"], 2, "{args:?}");
    }
    // A stored name outside the Kernel canonical spelling answers its target.
    let data = ok(&fixture.ds_offline(&[
        "design",
        "status",
        "--project",
        PROJECT,
        "--transformer",
        "TX-1",
    ]))
    .clone();
    let names: Vec<&str> = data["transformers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["TX-1"]);
}

#[test]
fn held_export_reads_only_held_status_and_configuration() {
    use ds_command_kernel::report_export::{InputReceipt, held_export::Capture};
    let fixture = Fixture::new();
    let layers = json!({"lv_poles":{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[30,-2]},"properties":{"id":"p1"}}]}});
    let receipt = InputReceipt::from_config(&receipt_config()).unwrap();
    let capture: Capture = serde_json::from_value(json!({"project":PROJECT,"transformer":"tx_1",
        "room":{"source":"lv","transformer_name":"tx_1","server_known":true,"local_dirty":false,"server_version":3,"layers":layers},
        "receipt":receipt,"selection":{"schema":"ds.design-output-selection/v1","geospatial":["shp"],"tabular":["xlsx"]},"media_grant":null}))
        .unwrap();
    let request = capture.prepare(LANE, OWNER, &fixture.audience).unwrap();
    let input = fixture.root.path().join("held-export.json");
    private_file(&input, &serde_json::to_vec(&request).unwrap());
    let export = |name: &str| {
        fixture.ds_offline(&[
            "report",
            "project",
            "export",
            "--project",
            PROJECT,
            "--held-export",
            input.to_str().unwrap(),
            "--out-dir",
            fixture.root.path().join(name).to_str().unwrap(),
        ])
    };
    // Nothing held: typed refusals, never a live read.
    assert_eq!(refused(&export("out-1")), "design_status_not_held");
    fixture.hold_status(
        PROJECT,
        json!([{"name":"tx_1","kind":"individual","metadata":{"version":3,"content_digest":request.content_digest}}]),
    );
    let status = fixture.ds_offline(&["design", "status", "--project", PROJECT, "--read", "held"]);
    assert_eq!(ok(&status)["count"], 1, "{status}");
    assert_eq!(refused(&export("out-2")), "project_config_not_held");
    // A held revision that differs from the room is the existing typed refusal.
    fixture.hold_configuration(PROJECT, receipt_config());
    fixture.hold_status(
        PROJECT,
        json!([{"name":"tx_1","kind":"individual","metadata":{"version":4,"content_digest":request.content_digest}}]),
    );
    assert_eq!(
        refused(&export("out-3")),
        "report_held_export_source_changed"
    );
    // Held status and configuration agree: admission passes offline and the
    // export proceeds to its local owners (no engine is installed here).
    fixture.hold_status(
        PROJECT,
        json!([{"name":"tx_1","kind":"individual","metadata":{"version":3,"content_digest":request.content_digest}}]),
    );
    let code = refused(&export("out-4")).to_owned();
    assert!(
        !matches!(
            code.as_str(),
            "design_status_not_held"
                | "project_config_not_held"
                | "report_held_export_source_changed"
                | "report_held_export_configuration_changed"
                | "report_held_export_invalid"
        ),
        "{code}"
    );
}
