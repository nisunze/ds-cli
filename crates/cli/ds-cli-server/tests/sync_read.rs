//! Production protected identity and default-lane state, without a Core daemon.
#![cfg(unix)]
#[allow(dead_code)]
#[path = "fixtures/device_home.rs"]
mod device_home;
use ds_cli_contract::{Context, Format, Output, parse};
use ds_cli_server::sync_read::{READ, read};
use serde_json::Value;
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
fn private_dir(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
fn inventory(path: &Path) -> Vec<(std::path::PathBuf, Vec<u8>, u32)> {
    let mut rows = Vec::new();
    if path.exists() {
        for entry in fs::read_dir(path).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                rows.extend(inventory(&p));
            } else {
                rows.push((
                    p.clone(),
                    fs::read(&p).unwrap(),
                    fs::metadata(p).unwrap().permissions().mode(),
                ));
            }
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}
#[test]
fn native_retained_read_preserves_protected_auth_and_default_state_without_daemon() {
    let home = device_home::DeviceHome::linked("stable", "sync-read-device");
    let work = tempfile::tempdir().unwrap();
    private_dir(work.path());
    // Bind the older shared fixture's routes to the current native owner's schema.
    let mut catalogue: Value =
        serde_json::from_slice(&fs::read(device_home::catalogue()).unwrap()).unwrap();
    catalogue["schema_version"] = ds_client_core::CLIENT_PROFILE_SCHEMA.into();
    for lane in ["stable", "canary"] {
        catalogue["profiles"][lane]["design_versions"]["actions"] =
            serde_json::json!(ds_client_core::DESIGN_VERSIONS_ACTIONS);
    }
    let catalogue_path = work.path().join("catalogue.json");
    fs::write(&catalogue_path, serde_json::to_vec(&catalogue).unwrap()).unwrap();
    let install = home.config_home().join("ds/edge-admission/stable");
    private_dir(&install);
    fs::set_permissions(install.parent().unwrap(), fs::Permissions::from_mode(0o700)).unwrap();
    let install = install.join("install-id");
    fs::write(&install, b"11111111-1111-4111-8111-111111111111").unwrap();
    fs::set_permissions(&install, fs::Permissions::from_mode(0o600)).unwrap();
    // This isolated integration-test process owns its environment; no other test shares it.
    unsafe {
        std::env::set_var("DS_CONFIG_HOME", home.config_home());
        std::env::set_var("DS_NATIVE_CLIENT_PROFILE_BUNDLE", &catalogue_path);
        std::env::set_var("XDG_STATE_HOME", work.path().join("state"));
    }
    let before = inventory(home.config_home());
    let out = work.path().join("projects.json");
    let tokens = vec![
        "--view".into(),
        "projects".into(),
        "--out".into(),
        out.to_str().unwrap().into(),
    ];
    let inputs = parse(&READ, &tokens).unwrap();
    let context = Context {
        confirmed: false,
        output: Output {
            format: Format::Json,
            pretty: false,
            color: false,
        },
    };
    let receipt = read(&inputs, &context).unwrap();
    assert_eq!(receipt["lane"], "stable");
    assert_eq!(inventory(home.config_home()), before);
    assert!(!work.path().join("state").exists());
    let data: Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(data["projects"], serde_json::json!([]));
    fs::remove_file(&out).unwrap();
    home.unlink();
    let signed_out = read(&inputs, &context).unwrap_err();
    assert_eq!(signed_out.code(), "headless_signed_out");
    assert!(!out.exists());
    assert!(
        parse(
            &READ,
            &[
                "--view".into(),
                "projects".into(),
                "--out".into(),
                out.to_str().unwrap().into(),
                "--state-dir".into(),
                work.path().to_str().unwrap().into()
            ]
        )
        .is_err()
    );
}
