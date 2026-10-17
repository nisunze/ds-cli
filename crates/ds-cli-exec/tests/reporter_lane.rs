//! Captured Reporter lane is invocation context, independent of ambient state.
#![cfg(unix)]
use ds_cli_exec::{CapturedExternal, External};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command, time::Duration};

static REPORTER: External = External {
    name: "reporter",
    owner: "ds-network-reporter",
    env_override: "DS_EXEC_REPORTER_LANE_FIXTURE",
    remedy: "repair fixture",
    missing_code: "fixture_missing",
};
static OTHER: External = External {
    name: "solar",
    owner: "ds-solar",
    env_override: "DS_EXEC_OTHER_LANE_FIXTURE",
    remedy: "repair fixture",
    missing_code: "fixture_missing",
};
const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn retained_reporter_lane_reaches_every_launch_without_changing_other_owners() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "isolated_reporter_lane",
            "--ignored",
            "--nocapture",
        ])
        .env("DS_REPORTER_NETWORK_LANE", "stable")
        .env_remove(REPORTER.env_override)
        .env_remove(OTHER.env_override)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn check_launches(capture: &CapturedExternal, expected: &str) {
    let check = |done: ds_cli_exec::Completed| {
        assert!(done.succeeded());
        assert_eq!(done.stdout, format!("\"{expected}\""));
    };
    check(capture.call("build-info", &[], TIMEOUT).unwrap());
    assert_eq!(
        capture.call_json("build-info", &[], TIMEOUT).unwrap(),
        expected
    );
    check(
        capture
            .call_cancelled("build-info", &[], TIMEOUT, 1, &|| false)
            .unwrap(),
    );
    check(
        capture
            .call_cancelled_observed("build-info", &[], TIMEOUT, 1, &|| false, &|_| {})
            .unwrap(),
    );
    check(
        capture
            .call_host_observed("build-info", &[], TIMEOUT, &|| false, &|_| {})
            .unwrap(),
    );
}

// This constructs a qualified receipt for transport bytes, not an engine or an
// installation grant. Production capture still verifies every receipt and hash.
fn managed_reporter(base: &Path, bytes: &[u8]) {
    let hash = format!("{:x}", Sha256::digest(bytes));
    let source = "b".repeat(40);
    let manifest = "d".repeat(64);
    let producer = format!("ds-network-reporter@0.1.0+{source}");
    let release: ds_command_kernel::module_release::ModuleRelease = serde_json::from_value(json!({
        "schema":"ds.module-release/v1", "component":"reporter", "channel":"canary",
        "product_id":"rw.datasolutions.module.reporter.canary", "version":"0.1.0-r1",
        "protocol":"ds.module/v1", "released_at":"2026-10-17T08:00:00Z", "status":"active",
        "assets":[{
            "platform":"linux", "arch":"x86_64", "kind":"module-executable", "protocol":"ds.module/v1",
            "url":"https://storage.googleapis.com/dedicated-public-releases/modules/reporter/canary/0.1.0-r1/reporter",
            "size_bytes":bytes.len(), "sha256":hash, "source_sha":source, "built_at":"2026-10-17T08:00:00Z",
            "source_revisions":{"ds-network-reporter":source},
            "engines":[{"engine":"ds-network-reporter", "release":producer, "manifest_sha256":manifest}]
        }]
    })).unwrap();
    let release_hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&release).unwrap())
    );
    let lane = base.join("reporter/canary");
    let version = lane.join("versions").join(&hash);
    fs::create_dir_all(&version).unwrap();
    let executable = version.join("reporter");
    fs::write(&executable, bytes).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(lane.join("selection.json"), serde_json::to_vec(&json!({
        "schema":"ds.module-selection/v1", "component":"reporter", "channel":"canary",
        "generation":1, "previous":null,
        "active":{
            "schema":"ds.module-installation/v1", "release":release, "platform":"linux",
            "authority":{"gatewayOrigin":"https://fixture.ue.gateway.dev", "nativeCatalogSha256":"a".repeat(64), "releaseSha256":release_hash},
            "engineIdentity":{
                "schema":"ds.engine-build/v1", "service":"ds-network-reporter", "binary":"ds-report",
                "profile":"release", "target":"x86_64-unknown-linux-gnu", "features":[],
                "cargo_lock_sha256":"e".repeat(64), "rustc":"fixture", "package_version":"0.1.0",
                "source_sha":source, "engine_version":producer, "build_manifest_sha256":manifest,
                "schemas":["ds.engine-build/v1","ds-network-reporter.task-result/v1"]
            }
        }
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "isolates ambient lane changes and explicit development selection"]
fn isolated_reporter_lane() {
    let root = std::env::temp_dir().join(format!("ds-reporter-lane-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let executable = root.join("development-reporter");
    let bytes = r#"#!/bin/sh
printf '"%s"' "${DS_REPORTER_NETWORK_LANE-unset}"
"#;
    fs::write(&executable, bytes).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    unsafe {
        std::env::set_var(REPORTER.env_override, &executable);
    }
    let development = REPORTER
        .capture_for_lane_in("canary", &root.join("addons"))
        .unwrap();
    let stable = REPORTER.capture_for_lane("stable").unwrap();
    assert_eq!(std::env::var("DS_REPORTER_NETWORK_LANE").unwrap(), "stable");
    unsafe {
        std::env::remove_var(REPORTER.env_override);
    }
    managed_reporter(&root.join("addons"), bytes.as_bytes());
    let managed = REPORTER
        .capture_managed_for_lane_in("canary", &root.join("addons"))
        .unwrap()
        .unwrap();
    let selected = REPORTER
        .capture_for_lane_in("canary", &root.join("addons"))
        .unwrap();
    unsafe {
        std::env::set_var("DS_REPORTER_NETWORK_LANE", "changed-after-capture");
    }
    check_launches(&stable, "stable");
    for capture in [&development, &managed, &selected] {
        check_launches(capture, "canary");
    }
    check_launches(&REPORTER.capture_pinned(&executable).unwrap(), "unset");
    unsafe {
        std::env::set_var(REPORTER.env_override, &executable);
    }
    check_launches(&REPORTER.capture().unwrap(), "unset");
    assert_eq!(
        REPORTER.call("build-info", &[], TIMEOUT).unwrap().stdout,
        "\"unset\""
    );
    assert_eq!(
        REPORTER
            .call_with_stdin("build-info", &[], b"{}", TIMEOUT)
            .unwrap()
            .stdout,
        "\"unset\""
    );
    unsafe {
        std::env::set_var(OTHER.env_override, &executable);
    }
    let other = OTHER
        .capture_for_lane_in("canary", &root.join("addons"))
        .unwrap();
    assert_eq!(
        other.call("build-info", &[], TIMEOUT).unwrap().stdout,
        "\"changed-after-capture\""
    );
    assert_eq!(
        std::env::var("DS_REPORTER_NETWORK_LANE").unwrap(),
        "changed-after-capture"
    );
    fs::remove_dir_all(root).unwrap();
}
