//! Public CLI/MCP metadata of `core.store` and its closed request refusals.
use serde_json::Value;
use std::process::Command;

#[test]
fn core_store_is_a_registered_local_state_command_with_a_closed_private_file_contract() {
    let result = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["capabilities", "core.store", "--output", "json"])
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stdout));
    let envelope: Value = serde_json::from_slice(&result.stdout).unwrap();
    let descriptor = &envelope["data"]["command"];
    assert_eq!(descriptor["id"], "core.store");
    assert_eq!(descriptor["contract"], 1);
    assert_eq!(descriptor["authority"], "headless_user");
    assert_eq!(descriptor["effect"], "local_auth_state");
    let args = descriptor["inputs"].as_array().unwrap();
    for name in ["lane", "request", "out"] {
        assert!(
            args.iter().any(|a| a["name"] == name && a["required"] == true),
            "{name}: {args:?}"
        );
    }
    for name in ["bytes", "bytes-out"] {
        assert!(
            args.iter().any(|a| a["name"] == name && a["required"] == false),
            "{name}: {args:?}"
        );
    }
    assert!(!args.iter().any(|a| matches!(
        a["name"].as_str(),
        Some("uid" | "principal" | "identity" | "sql" | "project" | "state-dir")
    )));
}

#[cfg(target_os = "linux")]
fn refusal_for(request: &[u8]) -> (Value, bool) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("request.json");
    let out = dir.path().join("answer.json");
    std::fs::write(&path, request).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "core", "store", "--lane", "stable",
            "--request", path.to_str().unwrap(),
            "--out", out.to_str().unwrap(),
            "--output", "json",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(std::fs::read(&path).unwrap(), request, "the request is preserved");
    (serde_json::from_slice(&result.stdout).unwrap(), out.exists())
}

#[cfg(target_os = "linux")]
#[test]
fn a_stated_identity_or_a_store_with_its_own_command_refuses_before_the_native_identity() {
    let (forged, wrote) = refusal_for(
        br#"{"request":{"store":"ui_preferences","operation":{"action":"get","keys":["active_project"]}},"identity":{"principal":"x"}}"#,
    );
    assert_eq!(forged["error"]["code"], "core_store_invalid_input");
    assert!(!wrote);
    let (served, wrote) =
        refusal_for(br#"{"request":{"store":"status","operation":{"action":"schema"}}}"#);
    assert_eq!(served["error"]["code"], "core_store_not_served");
    assert!(!wrote);
}
