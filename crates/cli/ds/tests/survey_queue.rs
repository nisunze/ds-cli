//! Public CLI/MCP metadata and closed queue request refusal smoke.
use serde_json::Value;
use std::process::Command;
#[test]
fn survey_queue_is_a_registered_native_survey_mutation_with_a_closed_private_file_contract() {
    let result = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["capabilities", "core.survey.queue", "--output", "json"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let envelope: Value = serde_json::from_slice(&result.stdout).unwrap();
    let descriptor = &envelope["data"]["command"];
    assert_eq!(descriptor["id"], "core.survey.queue");
    // Contract 2 (2026-10-18): create preparation and receipt bind the direct
    // Firestore commit effect; every other action keeps its contract-1 shape.
    assert_eq!(descriptor["contract"], 2);
    assert_eq!(descriptor["authority"], "headless_user");
    assert_eq!(descriptor["effect"], "local_auth_state");
    let args = descriptor["inputs"].as_array().unwrap();
    for name in ["lane", "project", "request", "out"] {
        assert!(
            args.iter()
                .any(|a| a["name"] == name && a["required"] == true),
            "{name}: {args:?}"
        );
    }
    assert!(!args.iter().any(|a| matches!(
        a["name"].as_str(),
        Some("uid" | "sql" | "transport" | "state-dir")
    )));
}
#[cfg(target_os = "linux")]
#[test]
fn malformed_request_refuses_before_native_identity_and_preserves_source() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let request = dir.path().join("request.json");
    let out = dir.path().join("output.json");
    std::fs::write(&request, b"{\"action\":\"headers\",\"sql\":\"select 1\"}").unwrap();
    std::fs::set_permissions(&request, std::fs::Permissions::from_mode(0o600)).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "core",
            "survey",
            "queue",
            "--lane",
            "stable",
            "--project",
            "exact-project",
            "--request",
            request.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--output",
            "json",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let envelope: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(envelope["error"]["code"], "survey_queue_invalid_input");
    assert!(!out.exists());
    assert_eq!(
        std::fs::read(request).unwrap(),
        b"{\"action\":\"headers\",\"sql\":\"select 1\"}"
    );
}
