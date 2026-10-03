//! Actual Server files through CLI/chapter MCP/typed MCP, under disposable
//! protected native identity. No operator state, provider or publication call.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ds_command_kernel::{report, sync, sync_store::*};
use ds_report_artifacts::{
    ARTIFACT_FILE, NetworkReporterArtifactReceipt, RECEIPT_FILE, RECEIPT_SCHEMA,
    confined_fs::HeldDirectory,
    publication::{self, PendingPublicationCommit, PublicationBatchReceipt, PublicationOutput},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const INSTALL: &str = "123e4567-e89b-42d3-a456-426614174000";
const OWNER: &str = "inventory-owner";
const PROJECT: &str = "inventory-project";
const TRANSFORMER: &str = "tx_1";
const SECRET: &str = "fixture-only-refresh-never-output";

struct Fixture {
    root: tempfile::TempDir,
    config: PathBuf,
    state: PathBuf,
    credential: PathBuf,
    bundle: PathBuf,
    audience: String,
}

fn private_dir(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn private_file(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

impl Fixture {
    fn new(identity: bool, install: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        let state = root.path().join("server");
        let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../ds-cli-auth/tests/fixtures/development-catalog.json");
        let mut audience = Sha256::new();
        // Exact native fixture binding, using its public protocol constants.
        // A changed native audience fails the REAL binary observation below.
        for value in [
            ds_client_core::DEVICE_CREDENTIAL_AUDIENCE_SCHEMA,
            "stable",
            ds_client_core::NATIVE_CLIENT_ID,
            "fixture-project",
        ] {
            audience.update((value.len() as u64).to_be_bytes());
            audience.update(value.as_bytes());
        }
        let audience = format!("{:x}", audience.finalize());
        let key = format!(
            "{:x}",
            Sha256::digest(format!("ds-client/stable/{audience}").as_bytes())
        );
        let credential = config.join("ds/credentials").join(format!("{key}.json"));
        if identity {
            private_dir(&config.join("ds"));
            private_dir(credential.parent().unwrap());
            private_file(
                &credential,
                &serde_json::to_vec(&json!({
                    "schema":"ds-client.refresh/v1", "credential_audience":audience,
                    "uid":OWNER, "email":"inventory@example.test", "refresh_token":SECRET,
                }))
                .unwrap(),
            );
        }
        if install {
            private_dir(&config.join("ds/edge-admission"));
            private_dir(&config.join("ds/edge-admission/stable"));
            private_file(
                &config.join("ds/edge-admission/stable/install-id"),
                format!("{INSTALL}\n").as_bytes(),
            );
        }
        Self {
            root,
            config,
            state,
            credential,
            bundle,
            audience,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
        command
            .env("DS_CONFIG_HOME", &self.config)
            .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", &self.bundle)
            .env("DS_CLI_NONINTERACTIVE", "1")
            .env(
                "DS_DESKTOP_DESCRIPTOR",
                self.root.path().join("no-desktop.json"),
            );
        command
    }

    fn cli(&self, project: &str, transformer: &str, lane: &str) -> Value {
        let output = self
            .command()
            .args([
                "report",
                "outbox",
                "inventory",
                "--project",
                project,
                "--transformer",
                transformer,
                "--lane",
                lane,
                "--server-state-dir",
                self.state.to_str().unwrap(),
                "--output",
                "json",
            ])
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(SECRET));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn mcp(&self, exposure: &str, requests: &[Value]) -> Vec<Value> {
        let mut command = self.command();
        command.args(["mcp", "serve", "--exposure", exposure]);
        if exposure == "commands" {
            command.args(["--profile", "printing"]);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        for request in requests {
            serde_json::to_writer(&mut stdin, request).unwrap();
            stdin.write_all(b"\n").unwrap();
        }
        serde_json::to_writer(
            &mut stdin,
            &json!({"jsonrpc":"2.0","id":999,"method":"shutdown"}),
        )
        .unwrap();
        stdin.write_all(b"\n").unwrap();
        drop(stdin);
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn assert_mcp_parity(&self, cli: &Value) {
        let arguments = json!({"project":PROJECT,"transformer":TRANSFORMER,"server-state-dir":self.state,"lane":"stable"});
        let chapter = self.mcp("chapters", &[json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ds_reports","arguments":{"operation":"invoke","command":"report.outbox.inventory","arguments":arguments}}})]);
        assert_eq!(&response(&chapter, 1)["result"]["structuredContent"], cli);
        let typed = self.mcp("commands", &[
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"report_outbox_inventory","arguments":arguments}}),
        ]);
        let tool = response(&typed, 1)["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "report_outbox_inventory")
            .unwrap();
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(
            tool["inputSchema"]["required"],
            json!(["project", "transformer"])
        );
        assert_eq!(&response(&typed, 2)["result"]["structuredContent"], cli);
    }

    fn fence(&self) -> Fence {
        Fence {
            account: OWNER.into(),
            deployment: "https://fixture.ue.gateway.dev".into(),
            install_id: INSTALL.into(),
        }
    }

    fn device(&self, uid: &str) {
        let directory = self.config.join("ds/devices");
        private_dir(&directory);
        let key = format!(
            "{:x}",
            Sha256::digest(format!("device:stable:{}", self.audience).as_bytes())
        );
        private_file(&directory.join(format!("{key}.json")), &serde_json::to_vec(&json!({
            "schema":"ds-client.device-credential/v1", "device_id":"fixture-device",
            "device_name":"Inventory fixture", "platform":"linux", "fingerprint":format!("sha256:{}", "c".repeat(64)),
            "uid":uid, "email":"inventory@example.test", "lane":"stable", "credential_audience":self.audience,
            "approved_profile_digest":format!("sha256:{}", "a".repeat(64)),
            "approved_catalog_digest":format!("sha256:{}", "b".repeat(64)),
            "credential_expires_at":"2027-01-01T00:00:00Z", "private_key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        })).unwrap());
    }

    fn commit(&self, batch: char, ids: &[&str]) -> PublicationBatchReceipt {
        private_dir(&self.state);
        let root = self.state.join("report-artifacts");
        // Existing native catalogue order, never a generation date authority.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let pending = HeldDirectory::ensure_absolute(&root).unwrap();
        let batch_id = batch.to_string().repeat(64);
        let mut outputs = vec![];
        for (index, id) in ids.iter().enumerate() {
            let bytes = format!("{batch}-{id}-real-fixture-bytes").into_bytes();
            let entry = format!("{batch}{index:063x}");
            let locator = format!("ds-reporter:v1:{entry}");
            let sha256 = format!("{:x}", Sha256::digest(&bytes));
            pending.create_child(OsStr::new(&entry)).unwrap();
            fs::write(root.join(&entry).join(ARTIFACT_FILE), &bytes).unwrap();
            fs::write(
                root.join(&entry).join(RECEIPT_FILE),
                serde_json::to_vec(&NetworkReporterArtifactReceipt {
                    schema: RECEIPT_SCHEMA,
                    batch_id: batch_id.clone(),
                    output_id: (*id).into(),
                    locator: locator.clone(),
                    artifact_file: ARTIFACT_FILE.into(),
                    size_bytes: bytes.len() as u64,
                    sha256: sha256.clone(),
                })
                .unwrap(),
            )
            .unwrap();
            let metadata = report::artifacts::output(id).unwrap();
            outputs.push(PublicationOutput {
                output_id: (*id).into(),
                locator,
                filename: ds_command_kernel::report_formats::report_filename(TRANSFORMER, &metadata.format).unwrap(),
                format: metadata.format,
                content_type: metadata.content_type.into(),
                sha256,
                size_bytes: bytes.len() as u64,
                paper_size: id
                    .starts_with("pdf__")
                    .then(|| if id.ends_with("a0l") { "a0" } else { "a3" }.into()),
                presentation: None,
            });
        }
        let receipt = PublicationBatchReceipt {
            schema: publication::BATCH_SCHEMA,
            batch_id,
            owner_uid: OWNER.into(),
            organization_id: None,
            project_id: PROJECT.into(),
            root: format!("eds_project/{PROJECT}/eds_design"),
            client_run_id: format!("report-{}", batch.to_string().repeat(32)),
            engine: "network_reporter".into(),
            operation: format!("export-{TRANSFORMER}"),
            variant: "default".into(),
            engine_version: format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)),
            engine_build_manifest_sha256: "d".repeat(64),
            transformer: TRANSFORMER.into(),
            transformer_revision: 7,
            input_base_fingerprint: "b".repeat(64),
            room_content_sha256: "c".repeat(64),
            grant_id: None,
            snapshot_base: BTreeMap::new(),
            client_publish_id: format!("publish-123e4567-e89b-42d3-a456-42661417400{batch}"),
            outputs,
        };
        PendingPublicationCommit::stage(&pending, &receipt)
            .unwrap()
            .commit()
            .unwrap();
        let row = ArtifactRow {
            scope: Scope::Project {
                project: PROJECT.into(),
            },
            identity: sync::Identity {
                engine: "network_reporter".into(),
                operation: receipt.operation.clone(),
                variant: "default".into(),
            },
            sha256: inventory_digest(
                &receipt
                    .outputs
                    .iter()
                    .map(|output| (output.output_id.clone(), output.sha256.clone()))
                    .collect::<Vec<_>>(),
            ),
            size_bytes: receipt.outputs.iter().map(|output| output.size_bytes).sum(),
            produced_at_ms: 111,
            base_revision: None,
            input_base_fingerprint: Some(receipt.input_base_fingerprint.clone()),
            engine_release: receipt.engine_version.clone(),
            engine_build_manifest_sha256: Some(receipt.engine_build_manifest_sha256.clone()),
            grant_engine: None,
            resource: Some(TRANSFORMER.into()),
            client_publish_id: receipt.client_publish_id.clone(),
            acknowledged_work_id: None,
            outputs: receipt
                .outputs
                .iter()
                .map(|output| ArtifactOutput {
                    filename: Some(output.filename.clone()),
                    paper_size: output.paper_size.clone(),
                    presentation: output.presentation.as_ref().map(|p| serde_json::json!(p)),
                    output_id: output.output_id.clone(),
                    format: output.format.clone(),
                    content_type: output.content_type.clone(),
                    sha256: output.sha256.clone(),
                    size_bytes: output.size_bytes,
                })
                .collect(),
            bytes_locator: format!("network_reporter:batch:{}", receipt.batch_id),
            readable: true,
            state: ArtifactState::Held,
            state_reason: None,
            replay_key: receipt.client_publish_id.clone(),
            updated_at_ms: 999,
            transfer_state: None,
        };
        let mut store = ds_sync_store::Store::open(&self.state.join("store.sqlite")).unwrap();
        let outcome = store
            .apply(&self.fence(), 1000, Event::LocalProduced { row })
            .unwrap();
        assert!(outcome.refusals.is_empty(), "{:?}", outcome.refusals);
        receipt
    }

    fn files(&self) -> BTreeMap<PathBuf, (Vec<u8>, u32)> {
        fn collect(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, (Vec<u8>, u32)>) {
            for entry in fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                let contents = if metadata.is_dir() {
                    collect(root, &path, files);
                    vec![]
                } else if metadata.file_type().is_symlink() {
                    fs::read_link(&path)
                        .unwrap()
                        .as_os_str()
                        .as_encoded_bytes()
                        .to_vec()
                } else {
                    fs::read(&path).unwrap()
                };
                files.insert(
                    path.strip_prefix(root).unwrap().into(),
                    (contents, metadata.permissions().mode()),
                );
            }
        }
        let mut files = BTreeMap::new();
        collect(self.root.path(), self.root.path(), &mut files);
        files
    }

    fn assert_read_only(&self, before: &BTreeMap<PathBuf, (Vec<u8>, u32)>) {
        let mut remaining_before = before.clone();
        let mut after = self.files();
        for suffix in ["wal", "shm"] {
            let exact = PathBuf::from(format!("server/store.sqlite-{suffix}"));
            let prior = remaining_before.remove(&exact);
            let observed = after.remove(&exact);
            if prior != observed {
                assert!(
                    before.contains_key(Path::new("server/store.sqlite")),
                    "no SQLite coordination may initialize an absent store"
                );
            }
            if let Some((bytes, mode)) = &observed {
                let primary_mode = before.get(Path::new("server/store.sqlite")).unwrap().1;
                assert_eq!(
                    *mode, primary_mode,
                    "coordination protection remains the database's exact mode"
                );
                if suffix == "wal" {
                    // A read may create an EMPTY coordination WAL. Existing
                    // queued WAL content must remain byte-for-byte unchanged.
                    assert_eq!(
                        bytes.as_slice(),
                        prior.as_ref().map_or(&[][..], |value| value.0.as_slice())
                    );
                } else {
                    assert_eq!(
                        bytes.len(),
                        32 * 1024,
                        "this bounded fixture's WAL index occupies one SQLite index region"
                    );
                }
            } else {
                assert!(
                    prior.is_none(),
                    "a read must not remove existing WAL coordination"
                );
            }
            println!(
                "sqlite-coordination {}",
                json!({
                    "exact_path":exact, "present_before":prior.is_some(), "present_after":observed.is_some(),
                    "changed":prior != observed, "scope":"sqlite_coordination_only",
                })
            );
        }
        // No other filename, auth directory, install, credential, mode,
        // committed report byte or primary database is exempted.
        assert_eq!(after, remaining_before);
    }
}

/// Test-only complete logical inspection of the OWNED fixture through the
/// store's explicitly test-only connection. Never used by the command host.
fn all_store_facts(store: &ds_sync_store::Store) -> Value {
    let connection = store.connection_for_test();
    let mut statement = connection
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name")
        .unwrap();
    let schema = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let mut tables = BTreeMap::new();
    for (_, name, _, _) in schema.iter().filter(|object| object.0 == "table") {
        let escaped = name.replace('"', "\"\"");
        let mut statement = connection
            .prepare(&format!("SELECT * FROM \"{escaped}\""))
            .unwrap();
        let columns = statement.column_count();
        let mut rows = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|column| row.get_ref(column).map(|value| format!("{value:?}")))
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows.sort();
        tables.insert(name.clone(), rows);
    }
    let mut schema_pragmas = BTreeMap::new();
    for pragma in ["schema_version", "user_version", "application_id"] {
        let value = connection
            .query_row(&format!("PRAGMA {pragma}"), [], |row| row.get::<_, i64>(0))
            .unwrap();
        schema_pragmas.insert(pragma, value);
    }
    json!({"schema":schema,"schema_pragmas":schema_pragmas,"all_tables":tables})
}

fn native_inventory(fixture: &Fixture, store: &ds_sync_store::Store) -> Value {
    let rows = store
        .snapshot(
            &fixture.fence(),
            &Scope::Project {
                project: PROJECT.into(),
            },
        )
        .unwrap()
        .artifacts;
    assert_eq!(rows.len(), 1);
    let snapshot = ds_report_artifacts::inventory::read_snapshot(
        &fixture.state.join("report-artifacts"),
        OWNER,
        PROJECT,
        TRANSFORMER,
        &rows,
    )
    .unwrap();
    serde_json::to_value(report::inventory::project(&snapshot).unwrap()).unwrap()
}

fn response(responses: &[Value], id: u64) -> &Value {
    responses
        .iter()
        .find(|response| response["id"] == id)
        .unwrap()
}

#[test]
fn actual_six_files_match_native_cli_chapter_and_typed_mcp_without_writes_or_publication() {
    let fixture = Fixture::new(true, true);
    let old = fixture.commit('a', &["shp", "kmz", "xlsx", "geojson"]);
    let latest = fixture.commit('b', &["pdf__a3l", "pdf__a0l"]);
    let store = ds_sync_store::Store::open_read_only(&fixture.state.join("store.sqlite"))
        .unwrap()
        .unwrap();
    let facts = all_store_facts(&store);
    let before = fixture.files();
    let cli = fixture.cli(PROJECT, TRANSFORMER, "stable");
    assert_eq!(cli["status"], "ok", "{cli}");
    let data = &cli["data"];
    assert_eq!(data["schema"], "ds.report.inventory/v1");
    assert_eq!(data["local_output_count"], 6);
    assert_eq!(data["published_output_count"], 0);
    assert_eq!(data["cloud_read"]["state"], "not_observed");
    assert!(data["generated_at_ms"].is_null());
    for output in data["outputs"].as_array().unwrap() {
        let recent = output["output_id"].as_str().unwrap().starts_with("pdf__");
        assert_eq!(
            output["client_publish_id"].as_str(),
            if recent {
                Some(latest.client_publish_id.as_str())
            } else {
                Some(old.client_publish_id.as_str())
            }
        );
        assert_eq!(
            output["publication_phase"],
            if recent { "queued" } else { "sealed" }
        );
        assert!(output["generated_at_ms"].is_null());
    }
    assert_eq!(*data, native_inventory(&fixture, &store));
    fixture.assert_mcp_parity(&cli);
    assert_eq!(all_store_facts(&store), facts);
    fixture.assert_read_only(&before);
}

#[test]
fn live_wal_current_report_is_observed_without_content_schema_or_lease_effects() {
    let fixture = Fixture::new(true, true);
    let old = fixture.commit('a', &["shp", "kmz", "xlsx", "geojson"]);
    // This owning writer stays open throughout every read; it performs no
    // concurrent effects. Its presence preserves the newly committed WAL.
    let mut writer = ds_sync_store::Store::open(&fixture.state.join("store.sqlite")).unwrap();
    let lease = writer
        .apply(
            &fixture.fence(),
            1000,
            Event::LeaseTake {
                scope: Scope::Project {
                    project: PROJECT.into(),
                },
                worker_id: "fixture-live-writer".into(),
                ttl_ms: 600_000,
            },
        )
        .unwrap();
    assert!(lease.refusals.is_empty(), "{:?}", lease.refusals);
    let latest = fixture.commit('b', &["pdf__a3l", "pdf__a0l"]);
    let held_lease = writer
        .snapshot(
            &fixture.fence(),
            &Scope::Project {
                project: PROJECT.into(),
            },
        )
        .unwrap()
        .lease
        .unwrap();
    assert_eq!(held_lease.worker_id, "fixture-live-writer");
    let before = fixture.files();
    let contains = |bytes: &[u8], needle: &str| {
        bytes
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    };
    assert!(contains(
        &before[Path::new("server/store.sqlite")].0,
        &old.client_publish_id
    ));
    assert!(
        !contains(
            &before[Path::new("server/store.sqlite")].0,
            &latest.client_publish_id
        ),
        "latest report must not already be checkpointed into the primary DB"
    );
    assert!(
        contains(
            &before[Path::new("server/store.sqlite-wal")].0,
            &latest.client_publish_id
        ),
        "this fixture must exercise committed current WAL content"
    );
    let facts = all_store_facts(&writer);
    let expected = native_inventory(&fixture, &writer);
    assert_eq!(expected["local_output_count"], 6);
    assert_eq!(expected["published_output_count"], 0);
    for output in expected["outputs"].as_array().unwrap() {
        let recent = output["output_id"].as_str().unwrap().starts_with("pdf__");
        assert_eq!(
            output["client_publish_id"].as_str(),
            Some(if recent {
                latest.client_publish_id.as_str()
            } else {
                old.client_publish_id.as_str()
            })
        );
        assert_eq!(
            output["publication_phase"],
            if recent { "queued" } else { "sealed" }
        );
    }
    let cli = fixture.cli(PROJECT, TRANSFORMER, "stable");
    assert_eq!(cli["status"], "ok", "{cli}");
    assert_eq!(
        cli["data"], expected,
        "CLI must observe current live WAL facts"
    );
    fixture.assert_mcp_parity(&cli);
    assert_eq!(
        all_store_facts(&writer),
        facts,
        "every Store row, lease and schema remains identical"
    );
    fixture.assert_read_only(&before);
    // Drop may checkpoint the owned writer, AFTER the invariance assertions.
    drop(writer);
}

#[test]
fn missing_native_identity_install_or_store_never_initializes_state() {
    for (identity, install, native_code) in [
        (false, false, "headless_signed_out"),
        (true, false, "headless_install_unavailable"),
    ] {
        let fixture = Fixture::new(identity, install);
        let before = fixture.files();
        let refused = fixture.cli(PROJECT, TRANSFORMER, "stable");
        assert_eq!(
            refused["error"]["code"],
            "report_inventory_identity_unavailable"
        );
        assert_eq!(refused["error"]["detail"]["native_code"], native_code);
        fixture.assert_read_only(&before);
    }
    let fixture = Fixture::new(true, true);
    let before = fixture.files();
    let empty = fixture.cli(PROJECT, TRANSFORMER, "stable");
    assert_eq!(empty["status"], "ok", "{empty}");
    assert_eq!(empty["data"]["local_output_count"], 0);
    assert_eq!(empty["data"]["sync_read"]["state"], "not_observed");
    fixture.assert_read_only(&before);
}

#[test]
fn native_scope_and_install_are_captured_without_borrowing_selected_context() {
    let fixture = Fixture::new(true, true);
    fixture.commit('a', &["xlsx"]);
    let before = fixture.files();
    for (project, transformer) in [("foreign-project", TRANSFORMER), (PROJECT, "tx_2")] {
        let isolated = fixture.cli(project, transformer, "stable");
        assert_eq!(isolated["status"], "ok", "{isolated}");
        assert_eq!(isolated["data"]["project"], project);
        assert_eq!(isolated["data"]["transformer"], transformer);
        assert_eq!(isolated["data"]["local_output_count"], 0);
    }
    assert_eq!(
        fixture.cli(PROJECT, TRANSFORMER, "canary")["error"]["detail"]["native_code"],
        "headless_signed_out"
    );
    assert_eq!(
        fixture.cli("../foreign", TRANSFORMER, "stable")["error"]["code"],
        "report_inventory_scope_invalid"
    );
    fixture.assert_read_only(&before);
    private_file(
        &fixture.config.join("ds/edge-admission/stable/install-id"),
        b"223e4567-e89b-42d3-a456-426614174000\n",
    );
    let before = fixture.files();
    let other_install = fixture.cli(PROJECT, TRANSFORMER, "stable");
    assert_eq!(other_install["status"], "ok", "{other_install}");
    // Physical receipts bind owner/project; install fences queue facts. An
    // unrelated install must not inherit its queued state or ACK.
    assert_eq!(
        other_install["data"]["outputs"][0]["publication_phase"],
        "sealed"
    );
    let mut credential: Value =
        serde_json::from_slice(&fs::read(&fixture.credential).unwrap()).unwrap();
    credential["uid"] = json!("foreign-owner");
    private_file(
        &fixture.credential,
        &serde_json::to_vec(&credential).unwrap(),
    );
    let after_identity_change = fixture.files();
    let other_owner = fixture.cli(PROJECT, TRANSFORMER, "stable");
    assert_eq!(other_owner["status"], "ok", "{other_owner}");
    assert_eq!(other_owner["data"]["local_output_count"], 0);
    fixture.assert_read_only(&after_identity_change);
    assert_ne!(before, after_identity_change);
}

#[test]
fn unsafe_identity_and_corrupt_selected_bytes_refuse_without_repair_or_fallback() {
    let fixture = Fixture::new(true, true);
    fixture.commit('a', &["xlsx"]);
    let current = fixture.commit('b', &["xlsx"]);
    let directory = current.outputs[0]
        .locator
        .strip_prefix("ds-reporter:v1:")
        .unwrap();
    let path = fixture
        .state
        .join("report-artifacts")
        .join(directory)
        .join(ARTIFACT_FILE);
    fs::write(&path, b"corrupt-selected-bytes").unwrap();
    let before = fixture.files();
    assert_eq!(
        fixture.cli(PROJECT, TRANSFORMER, "stable")["error"]["code"],
        "report_inventory_unreadable"
    );
    fixture.assert_read_only(&before);
    fs::remove_file(&path).unwrap();
    let before = fixture.files();
    assert_eq!(
        fixture.cli(PROJECT, TRANSFORMER, "stable")["error"]["code"],
        "report_inventory_unreadable"
    );
    fixture.assert_read_only(&before);
    fs::set_permissions(&fixture.credential, fs::Permissions::from_mode(0o644)).unwrap();
    let before = fixture.files();
    assert_eq!(
        fixture.cli(PROJECT, TRANSFORMER, "stable")["error"]["detail"]["native_code"],
        "native_state_unsafe"
    );
    fixture.assert_read_only(&before);
    fs::set_permissions(&fixture.credential, fs::Permissions::from_mode(0o600)).unwrap();
    let target = fixture.root.path().join("fixture-token");
    fs::rename(&fixture.credential, &target).unwrap();
    std::os::unix::fs::symlink(&target, &fixture.credential).unwrap();
    let before = fixture.files();
    assert_eq!(
        fixture.cli(PROJECT, TRANSFORMER, "stable")["error"]["detail"]["native_code"],
        "native_state_unsafe"
    );
    fixture.assert_read_only(&before);
}

#[test]
fn existing_device_provider_is_observed_without_leases_and_disagreement_refuses() {
    let fixture = Fixture::new(true, true);
    fixture.commit('a', &["xlsx"]);
    fixture.device(OWNER);
    // A corrupt saved selection cannot decide an explicitly named read.
    private_dir(&fixture.config.join("ds/contexts"));
    private_file(
        &fixture.config.join("ds/contexts/unrelated.json"),
        b"unreadable-selection",
    );
    let before = fixture.files();
    let same = fixture.cli(PROJECT, TRANSFORMER, "stable");
    assert_eq!(same["status"], "ok", "{same}");
    assert_eq!(same["data"]["local_output_count"], 1);
    fixture.assert_read_only(&before);
    fs::remove_file(&fixture.credential).unwrap();
    let before = fixture.files();
    assert_eq!(fixture.cli(PROJECT, TRANSFORMER, "stable")["status"], "ok");
    fixture.assert_read_only(&before);
    let other = Fixture::new(true, true);
    other.device("different-device-owner");
    let before = other.files();
    let refused = other.cli(PROJECT, TRANSFORMER, "stable");
    assert_eq!(
        refused["error"]["detail"]["native_code"],
        "auth_context_mismatch"
    );
    other.assert_read_only(&before);
}

#[test]
fn malformed_existing_store_is_not_treated_as_an_empty_inventory() {
    let fixture = Fixture::new(true, true);
    private_dir(&fixture.state);
    fs::write(
        fixture.state.join("store.sqlite"),
        b"not a native SQLite store",
    )
    .unwrap();
    let before = fixture.files();
    assert_eq!(
        fixture.cli(PROJECT, TRANSFORMER, "stable")["error"]["code"],
        "report_inventory_unreadable"
    );
    fixture.assert_read_only(&before);
}
