//! The real CLI registry must reject retirement before credentials or cache
//! state are reached unless its declared preview or confirmation was selected.
use serde_json::Value;
use std::{path::Path, process::Command};

fn invoke(arguments: &[&str], root: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(arguments)
        .env("NO_COLOR", "1")
        .env("DS_CONFIG_HOME", root.join("untouched-auth"))
        .env(
            "DS_NATIVE_CLIENT_PROFILE_BUNDLE",
            root.join("absent-catalog.json"),
        )
        .env_remove("DS_NATIVE_LOCAL_GO")
        .env("DS_DESKTOP_DESCRIPTOR", root.join("no-desktop.json"))
        .output()
        .expect("real ds executable runs");
    serde_json::from_slice(&output.stdout).expect("structured CLI envelope")
}

#[test]
fn retirement_without_yes_refuses_before_authentication_or_state_creation() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("untouched-core");
    let digest = "a".repeat(64);
    let envelope = invoke(
        &[
            "core",
            "cache",
            "retire",
            "--lane",
            "canary",
            "--state-dir",
            state.to_str().unwrap(),
            "--digest",
            &digest,
            "--output",
            "json",
        ],
        root.path(),
    );
    assert_eq!(envelope["status"], "error");
    assert_eq!(envelope["error"]["code"], "confirmation_required");
    assert!(!state.exists());
    assert!(!root.path().join("untouched-auth").exists());
}

#[test]
fn declared_preview_reaches_owner_validation_without_confirmation_or_cache_effects() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("untouched-core");
    let envelope = invoke(
        &[
            "core",
            "cache",
            "retire",
            "--lane",
            "canary",
            "--state-dir",
            state.to_str().unwrap(),
            "--dry-run",
            "--output",
            "json",
        ],
        root.path(),
    );
    // The deliberately absent catalog prevents any account/state restoration.
    // This is an actual dispatch control, not a successful retirement preview.
    assert_eq!(envelope["status"], "error");
    assert_ne!(envelope["error"]["code"], "confirmation_required");
    assert_eq!(envelope["error"]["code"], "core_cache_retire_refused");
    assert!(!state.exists());
}

#[test]
fn discovery_exposes_one_shared_confirmation_and_preview_boundary() {
    let root = tempfile::tempdir().unwrap();
    let envelope = invoke(
        &["capabilities", "core.cache.retire", "--output", "json"],
        root.path(),
    );
    assert_eq!(envelope["status"], "ok");
    let command = &envelope["data"]["command"];
    assert_eq!(command["effect"], "machine_write");
    assert_eq!(command["confirmation_required"], true);
    assert_eq!(command["preview_switch"], "--dry-run");
    assert!(!root.path().join("untouched-auth").exists());
}
