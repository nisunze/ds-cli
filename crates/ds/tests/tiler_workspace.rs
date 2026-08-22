//! End-to-end proof of the closed local tiler adapter.
//!
//! The fake native binary is deliberately a shell fixture: this test is about
//! what `ds` is permitted to invoke and what it accepts back, while the
//! ds-vector-tiler repository owns the real pinned-tool smoke. It proves the
//! integration cannot quietly turn a workspace request into arbitrary argv or
//! accept a Cloud Run-shaped receipt.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

struct TempTree {
    path: PathBuf,
}

impl TempTree {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ds-cli-tiler-{label}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self { path }
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, body).expect("write fake executable");
    let mut permissions = std::fs::metadata(path)
        .expect("read permissions")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions).expect("make fake executable runnable");
}

fn valid_result() -> Value {
    let artifact = json!({
        "logical_name": "network.pmtiles",
        "path": "artifacts/tiles/network.pmtiles",
        "sha256": format!("sha256:{}", "c".repeat(64)),
        "size_bytes": 42,
        "content_type": "application/vnd.pmtiles",
    });
    json!({
        "schema": "ds-vector-tiler.workspace-tile-result/v1",
        "input_schema": "ds-vector-tiler.workspace-tile/v1",
        "operation": "workspace-tile",
        "status": "success",
        "execution": {
            "location": "local",
            "remote_execution": false,
            "cloud_run": false,
            "upload": false,
            "input_manifest": "snapshot/tiles.json",
            "artifact_root": "artifacts/tiles",
        },
        "publication_state": "local_preview_only",
        "shared": false,
        "serving_pointer_registered": false,
        "engine": { "name": "ds-vector-tiler", "release": "0.2.0" },
        "output_name": "network",
        "result_manifest": "artifacts/tiles/network.result.json",
        "tools": {
            "tippecanoe": { "version": "2.0", "sha256": format!("sha256:{}", "a".repeat(64)) },
            "pmtiles": { "version": "3.0", "sha256": format!("sha256:{}", "b".repeat(64)) },
        },
        "total_features": 3,
        "tile_count": 4,
        "artifacts": [artifact.clone()],
        "artifact": artifact,
    })
}

struct FakeTiler {
    _tree: TempTree,
    engine: PathBuf,
    result: PathBuf,
    trace: PathBuf,
}

impl FakeTiler {
    fn new(result: Value) -> Self {
        Self::build(result, true)
    }

    fn without_additions(result: Value) -> Self {
        Self::build(result, false)
    }

    fn build(result: Value, additions: bool) -> Self {
        let tree = TempTree::new("engine");
        let result_path = tree.path.join("result.json");
        let trace = tree.path.join("trace.txt");
        std::fs::write(
            &result_path,
            serde_json::to_vec(&result).expect("serialize fake result"),
        )
        .expect("write fake result");

        let engine = tree.path.join("ds-vector-tiler");
        write_executable(
            &engine,
            "#!/bin/sh\nset -eu\ntest \"$#\" -eq 2\ntest \"$1\" = \"workspace-tile\"\nprintf '%s\\n%s\\n%s\\n%s\\n' \"$1\" \"$2\" \"$DS_VECTOR_TILER_TIPPECANOE_BIN\" \"$DS_VECTOR_TILER_PMTILES_BIN\" > \"$DS_TILER_TRACE\"\ncat \"$DS_TILER_RESULT\"\n",
        );
        // `ds` must discover additions beside the engine when their explicit
        // overrides are absent. The fake engine does not execute them; their
        // existence proves the typed child environment was resolved first.
        if additions {
            write_executable(&tree.path.join("tippecanoe"), "#!/bin/sh\nexit 0\n");
            write_executable(&tree.path.join("pmtiles"), "#!/bin/sh\nexit 0\n");
        }

        Self {
            _tree: tree,
            engine,
            result: result_path,
            trace,
        }
    }

    fn run(&self, args: &[&str]) -> (Value, i32, String, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(args)
            .env("NO_COLOR", "1")
            .env("DS_VECTOR_TILER_BIN", &self.engine)
            .env_remove("DS_VECTOR_TILER_TIPPECANOE_BIN")
            .env_remove("DS_VECTOR_TILER_PMTILES_BIN")
            .env("DS_TILER_RESULT", &self.result)
            .env("DS_TILER_TRACE", &self.trace)
            .output()
            .expect("ds runs");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let envelope = serde_json::from_str(&stdout)
            .unwrap_or_else(|error| panic!("ds did not emit JSON ({error}): {stdout}{stderr}"));
        (envelope, output.status.code().unwrap_or(-1), stdout, stderr)
    }

    fn trace_lines(&self) -> Vec<String> {
        std::fs::read_to_string(&self.trace)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

#[test]
fn workspace_adapter_invokes_only_the_closed_local_binary_contract() {
    let fake = FakeTiler::new(valid_result());
    let workspace = TempTree::new("workspace");
    let workspace_arg = workspace.path.to_string_lossy().into_owned();

    let (envelope, code, stdout, stderr) = fake.run(&[
        "tiler",
        "workspace",
        "--workspace",
        &workspace_arg,
        "--output",
        "json",
    ]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(envelope["command"], "tiler.workspace");
    assert_eq!(envelope["data"]["execution"]["location"], "local");
    assert_eq!(envelope["data"]["execution"]["cloud_run"], false);
    assert_eq!(
        envelope["data"]["artifacts"][0]["path"],
        "artifacts/tiles/network.pmtiles"
    );
    assert!(envelope["data"].get("workspace").is_none());
    let tool_directory = fake.engine.parent().expect("engine has parent");
    assert_eq!(
        fake.trace_lines(),
        vec![
            "workspace-tile".to_string(),
            workspace.path.to_string_lossy().into_owned(),
            tool_directory
                .join("tippecanoe")
                .canonicalize()
                .expect("tippecanoe is installed")
                .to_string_lossy()
                .into_owned(),
            tool_directory
                .join("pmtiles")
                .canonicalize()
                .expect("pmtiles is installed")
                .to_string_lossy()
                .into_owned(),
        ],
        "the adapter must provide only the static subcommand/canonical root and the two pinned sibling additions"
    );
}

#[test]
fn workspace_adapter_refuses_before_spawning_when_a_bundled_addition_is_missing() {
    let fake = FakeTiler::without_additions(valid_result());
    let workspace = TempTree::new("workspace");
    let workspace_arg = workspace.path.to_string_lossy().into_owned();

    let (envelope, code, _stdout, _stderr) = fake.run(&[
        "tiler",
        "workspace",
        "--workspace",
        &workspace_arg,
        "--output",
        "json",
    ]);
    assert_eq!(code, 3);
    assert_eq!(envelope["error"]["code"], "tiler_addition_missing");
    assert!(fake.trace_lines().is_empty());
}

#[test]
fn workspace_adapter_refuses_a_remote_shaped_engine_receipt() {
    let mut remote = valid_result();
    remote["execution"]["cloud_run"] = Value::Bool(true);
    let fake = FakeTiler::new(remote);
    let workspace = TempTree::new("workspace");
    let workspace_arg = workspace.path.to_string_lossy().into_owned();

    let (envelope, code, _stdout, _stderr) = fake.run(&[
        "tiler",
        "workspace",
        "--workspace",
        &workspace_arg,
        "--output",
        "json",
    ]);
    assert_eq!(code, 6);
    assert_eq!(envelope["error"]["code"], "tiler_contract_mismatch");
    assert_eq!(
        fake.trace_lines().first().map(String::as_str),
        Some("workspace-tile")
    );
}

#[test]
fn workspace_adapter_rejects_relative_roots_before_spawning_the_engine() {
    let fake = FakeTiler::new(valid_result());
    let (envelope, code, _stdout, _stderr) = fake.run(&[
        "tiler",
        "workspace",
        "--workspace",
        "relative-workspace",
        "--output",
        "json",
    ]);
    assert_eq!(code, 2);
    assert_eq!(envelope["error"]["code"], "workspace_not_absolute");
    assert!(fake.trace_lines().is_empty());
}
