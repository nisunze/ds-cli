use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn published_portfolio_path_is_validated_before_authentication_or_desktop() {
    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-cli-auth/tests/fixtures/development-catalog.json");
    let config = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", bundle)
        .env("DS_CONFIG_HOME", config.path())
        .env("DS_DESKTOP_DESCRIPTOR", config.path().join("no-desktop.json"))
        .args([
            "solar",
            "portfolio",
            "published",
            "read",
            "--project",
            "p-1",
            "--portfolio",
            "pf-1",
            "--run-id",
            "selected-run",
            "--path",
            "",
            "--output",
            "json",
        ])
        .output()
        .unwrap();
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!output.status.success());
    assert_eq!(result["command"], "solar.portfolio.published.read");
    assert_eq!(result["error"]["code"], "invalid_portfolio_path");
}
