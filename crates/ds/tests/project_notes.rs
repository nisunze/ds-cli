//! Executable discovery and pre-authentication refusal checks for manual notes.
use serde_json::Value;
use std::process::Command;

fn invoke(args: &[&str]) -> (Value, bool) {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env(
            "DS_NATIVE_CLIENT_PROFILE_BUNDLE",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../ds-cli-auth/tests/fixtures/development-catalog.json"
            ),
        )
        .env(
            "DS_CONFIG_HOME",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/target/no-project-notes-credentials"
            ),
        )
        .env(
            "DS_DESKTOP_DESCRIPTOR",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/target/no-project-notes-desktop.json"
            ),
        )
        .output()
        .expect("ds executable runs");
    (
        serde_json::from_slice(&output.stdout).expect("DS envelope"),
        output.status.success(),
    )
}

#[test]
fn all_manual_note_operations_are_discoverable_headlessly() {
    for leaf in [
        "read",
        "query",
        "context",
        "update",
        "versions",
        "backup",
        "retire",
        "restore",
        "consolidate",
    ] {
        let id = format!("pm.note.{leaf}");
        let (result, success) = invoke(&["capabilities", &id, "--output", "json"]);
        assert!(success, "{id}: {result}");
        let command = &result["data"]["command"];
        assert_eq!(command["id"], id);
        assert_eq!(command["authority"], "headless_project");
        assert_eq!(command["requires"], "server");
        assert_eq!(command["path"], serde_json::json!(["pm", "note", leaf]));
    }
}

#[test]
fn manual_note_inputs_and_write_confirmation_refuse_before_authentication() {
    for leaf in [
        "read",
        "query",
        "context",
        "update",
        "versions",
        "backup",
        "retire",
        "restore",
        "consolidate",
    ] {
        let (result, success) = invoke(&["pm", "note", leaf, "--output", "json"]);
        assert!(!success, "{leaf}: {result}");
        assert_eq!(result["error"]["code"], "missing_input", "{leaf}: {result}");
        assert!(result["data"].is_null());
    }
    for leaf in ["update", "backup", "retire", "restore", "consolidate"] {
        let (result, success) = invoke(&[
            "pm",
            "note",
            leaf,
            "--project",
            "test-project",
            "--input",
            "/nonexistent-project-notes-request.json",
            "--output",
            "json",
        ]);
        assert!(!success, "{leaf}: {result}");
        assert_eq!(
            result["error"]["code"], "confirmation_required",
            "{leaf}: {result}"
        );
        assert!(result["data"].is_null());
    }
}
