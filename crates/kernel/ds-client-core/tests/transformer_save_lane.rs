//! A transformer save validates its analysis with the Network engine of the
//! request's own lane, never the packaged one. In an isolated process with no
//! development override, a Canary save reaches the (removed) Canary selection
//! and a Stable save reaches the (absent) Stable one: each refusal names the
//! lane it captured.
#![cfg(unix)]
use ds_client_core::{DeploymentLane, TransformerSaveBatch, TransformerSaveItem};
use ds_network_contracts::lv_processing::NativeLvPublication;
use std::{fs, path::PathBuf, process::Command};

#[test]
fn a_save_captures_the_network_engine_of_its_own_lane() {
    let root = tempfile::tempdir().unwrap();
    let lane = root.path().join(".local/lib/ds-addons/network/canary");
    fs::create_dir_all(&lane).unwrap();
    fs::write(
        lane.join("selection.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": ds_command_kernel::module_install::REMOVED_SELECTION_SCHEMA,
            "component": "network", "channel": "canary", "generation": 1, "state": "removed"
        }))
        .unwrap(),
    )
    .unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "isolated_lane_capture",
            "--ignored",
            "--nocapture",
        ])
        .env("HOME", root.path())
        .env("PATH", root.path())
        .env_remove("DS_NETWORK_ENGINE_BIN")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn batch() -> TransformerSaveBatch {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-lv-save-new.publication.json");
    let projection: NativeLvPublication =
        serde_json::from_slice(&fs::read(fixture).unwrap()).unwrap();
    let source_content_digest =
        ds_command_kernel::report_export::jcs::layers_content_digest(&projection.source_layers)
            .unwrap();
    TransformerSaveBatch {
        project_id: "project".into(),
        items: vec![TransformerSaveItem {
            transformer_name: "T1".into(),
            base_version: 7,
            source_content_digest,
            base_content_digest: None,
            source_layers: projection.source_layers,
            gdfs: projection.gdfs,
            config_dfs: projection.config_dfs,
            process_metadata: projection.process_metadata,
            process_input: projection.process_input,
            voltage_drop: projection.voltage_drop,
            operation_id: "save-T1-7".into(),
        }],
    }
}

#[test]
#[ignore = "isolated subprocess with a private HOME and no engine override"]
fn isolated_lane_capture() {
    let batch = batch();
    let canary = batch
        .validate("project", DeploymentLane::Canary)
        .unwrap_err();
    assert_eq!(canary.to_string(), "module_removed", "{canary}");
    let stable = batch
        .validate("project", DeploymentLane::Stable)
        .unwrap_err();
    assert_eq!(stable.to_string(), "network_engine_missing", "{stable}");
}
