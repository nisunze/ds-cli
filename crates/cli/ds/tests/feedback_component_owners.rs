//! Isolated native CLI qualification for feedback 9ab57db2. No signed-in
//! identity or operator configuration can enter this fixture.
use serde_json::{Value, json};
use std::process::Command;

#[test]
fn feedback_accepts_live_owners_and_refuses_retired_owners() {
    let root = tempfile::tempdir().unwrap();
    let mut catalog: Value = serde_json::from_str(include_str!(
        "../../ds-cli-auth/tests/fixtures/development-catalog.json"
    ))
    .unwrap();
    catalog["schema_version"] = json!(ds_client_core::CLIENT_PROFILE_SCHEMA);
    for profile in catalog["profiles"].as_object_mut().unwrap().values_mut() {
        profile["design_versions"]["actions"] = json!(ds_client_core::DESIGN_VERSIONS_ACTIONS);
    }
    let bundle = root.path().join("catalog.json");
    std::fs::write(&bundle, serde_json::to_vec(&catalog).unwrap()).unwrap();
    for (component, expected) in [
        ("ds-vector-tiler", "feedback_component_unknown"),
        ("ds-vector-tiler/tiles", "feedback_component_unknown"),
        ("ds-solar-weather/weather", "feedback_component_unknown"),
        ("unknown-repository", "feedback_component_unknown"),
        ("ds-grid-engine/sag", "feedback_component_unknown"),
        ("ds-cli/feedback", "headless_signed_out"),
        ("ds-network/spotting", "headless_signed_out"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args([
                "feedback",
                "submit",
                "--title",
                "Owner admission regression",
                "--detail",
                "Owning fixture only; no authenticated transport is available",
                "--component",
                component,
                "--agent",
                "owning-regression",
                "--yes",
                "--output",
                "json",
            ])
            .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", &bundle)
            .env("DS_CONFIG_HOME", root.path().join("empty-auth"))
            .env(
                "DS_DESKTOP_DESCRIPTOR",
                root.path().join("absent-desktop.json"),
            )
            .output()
            .unwrap();
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            envelope["error"]["code"], expected,
            "{component}: {envelope}"
        );
        if expected == "feedback_component_unknown" {
            let error = envelope["error"].to_string();
            assert!(error.contains("ds-network-reporter"), "{error}");
            assert!(!error.contains("ds-vector-tiler"), "{error}");
            assert!(!error.contains("ds-solar-weather"), "{error}");
        }
    }
}
