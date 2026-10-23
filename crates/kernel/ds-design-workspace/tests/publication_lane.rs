//! A Grid publication validates its package with the Network engine of the
//! request's own lane, never the packaged one. In an isolated process with no
//! development override, a Canary publication reaches the (removed) Canary
//! selection and a Stable one the (absent) Stable selection: each refusal
//! names the lane it captured.
#![cfg(unix)]
use ds_design_workspace::grid_publication::{prepare, publication_revision, Request};
use std::{fs, process::Command};

#[test]
fn a_publication_captures_the_network_engine_of_its_own_lane() {
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
        .env("DS_NETWORK_SCRATCH_DIR", root.path().join("staging"))
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

#[test]
#[ignore = "isolated subprocess with a private HOME and no engine override"]
fn isolated_lane_capture() {
    let bytes = b"any package bytes";
    let canary = publication_revision(bytes, "canary").unwrap_err();
    assert!(canary.starts_with("module_removed"), "{canary}");
    let stable = publication_revision(bytes, "stable").unwrap_err();
    assert!(
        stable.starts_with("network_engine_missing") && stable.contains("for stable"),
        "{stable}"
    );
    let request = Request {
        project: "project".into(),
        model_kind: "mv_line".into(),
        display_name: Some("Line".into()),
        ..Request::default()
    };
    let refused = prepare(&request, bytes, "canary").unwrap_err();
    assert!(refused.starts_with("module_removed"), "{refused}");
}
