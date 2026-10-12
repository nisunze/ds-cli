//! Retiring the general DAG must remove executable/discoverable commands while
//! keeping the direct governed design controls that consumed the same kernels.
use serde_json::Value;
use std::process::Command;

fn call(arguments: &[&str]) -> (bool, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(arguments)
        .output()
        .expect("native ds command");
    let envelope = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid command envelope: {error}"));
    (output.status.success(), envelope)
}

#[test]
fn custom_workflow_commands_are_absent_and_refuse_before_execution() {
    let (success, catalogue) = call(&["capabilities", "data", "--output", "json"]);
    assert!(success);
    assert!(
        catalogue["data"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .all(|command| !command["id"]
                .as_str()
                .unwrap()
                .starts_with("data.vector.workflow."))
    );
    for operation in ["describe", "validate", "run", "save", "show", "list"] {
        let id = format!("data.vector.workflow.{operation}");
        let (success, descriptor) = call(&["capabilities", &id, "--output", "json"]);
        assert!(!success);
        assert_eq!(descriptor["error"]["code"], "unknown_selector", "{id}");
        let (success, refused) =
            call(&["data", "vector", "workflow", operation, "--output", "json"]);
        assert!(!success);
        assert_eq!(refused["error"]["class"], "invalid_input", "{id}");
        assert_eq!(refused["error"]["retryable"], false, "{id}");
    }
}

#[test]
fn direct_governed_repair_controls_remain_discoverable() {
    for id in [
        "design.repair.audit",
        "design.repair.compare",
        "design.repair.propose",
        "design.repair.apply-selected",
    ] {
        let (success, descriptor) = call(&["capabilities", id, "--output", "json"]);
        assert!(success, "{id}");
        assert_eq!(descriptor["data"]["command"]["id"], id);
        assert!(descriptor["data"]["command"]["effect"].is_string());
    }
}
