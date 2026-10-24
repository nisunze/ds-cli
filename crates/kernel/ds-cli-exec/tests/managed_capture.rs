//! Lane capture is fail-closed; explicit development and retained paths stay deliberate.
#![cfg(unix)]

use ds_cli_exec::External;
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command, time::Duration};

static OWNER: External = External {
    name: "ds-network",
    env_override: "DS_EXEC_CAPTURE_TEST_OVERRIDE",
    owner: "ds-network",
    remedy: "install fixture",
    missing_code: "fixture_missing",
};

#[test]
fn absent_corrupt_and_removed_managed_selection_never_substitutes_path() {
    let root = std::env::temp_dir().join(format!("ds-managed-capture-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "isolated_managed_capture",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("DS_EXEC_CAPTURE_TEST_ROOT", &root)
        .env("PATH", &root)
        .env_remove(OWNER.env_override)
        .output()
        .unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "isolated subprocess with private PATH and explicit override"]
fn isolated_managed_capture() {
    let root = PathBuf::from(std::env::var_os("DS_EXEC_CAPTURE_TEST_ROOT").expect("isolated test"));
    let fallback = root.join("ds-network");
    fs::write(&fallback, b"#!/bin/sh\nprintf 'path-engine\\n'\n").unwrap();
    fs::set_permissions(&fallback, fs::Permissions::from_mode(0o700)).unwrap();
    let base = root.join("addons");
    assert_eq!(
        OWNER
            .capture_for_lane_in("canary", &base)
            .err()
            .unwrap()
            .code(),
        "fixture_missing"
    );
    let lane = base.join("network/canary");
    fs::create_dir_all(&lane).unwrap();
    assert_eq!(
        OWNER
            .capture_for_lane_in("canary", &base)
            .err()
            .unwrap()
            .code(),
        "fixture_missing"
    );
    let selection = lane.join("selection.json");
    fs::write(&selection, b"{torn").unwrap();
    assert_eq!(
        OWNER
            .capture_for_lane_in("canary", &base)
            .err()
            .unwrap()
            .code(),
        "module_receipt_invalid"
    );
    fs::write(
        &selection,
        serde_json::to_vec(&serde_json::json!({
            "schema": ds_module_contracts::module_install::REMOVED_SELECTION_SCHEMA,
            "component": "network", "channel": "canary", "generation": 1, "state": "removed"
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        OWNER
            .capture_for_lane_in("canary", &base)
            .err()
            .unwrap()
            .code(),
        "module_removed"
    );

    // An explicit override is an intentional selection even in a removed lane.
    unsafe {
        std::env::set_var(OWNER.env_override, &fallback);
    }
    let deliberate = OWNER.capture_for_lane_in("canary", &base).unwrap();
    assert_eq!(deliberate.executable(), &fallback);
    assert_eq!(
        deliberate
            .call("build-info", &[], Duration::from_secs(5))
            .unwrap()
            .stdout,
        "path-engine\n"
    );
    // A retained path does not resolve an override or a managed pointer again.
    unsafe {
        std::env::set_var(OWNER.env_override, root.join("absent-explicit"));
    }
    assert_eq!(
        OWNER
            .capture_for_lane_in("canary", &base)
            .err()
            .unwrap()
            .code(),
        "fixture_missing"
    );
    assert_eq!(
        OWNER.capture_pinned(&fallback).unwrap().executable(),
        &fallback
    );
    unsafe {
        std::env::remove_var(OWNER.env_override);
    }
    assert_eq!(
        OWNER
            .capture_for_lane_in("canary", &base)
            .err()
            .unwrap()
            .code(),
        "module_removed"
    );
    // The explicit unmanaged API remains available to development callers.
    assert_eq!(OWNER.capture().unwrap().executable(), &fallback);
}
