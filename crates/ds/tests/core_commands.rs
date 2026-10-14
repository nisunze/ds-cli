//! Real registry projection and refusal controls; no component state mutations.
use serde_json::Value;
use std::process::{Command, Output};

fn ds(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env_remove("DS_NETWORK_ENGINE_BIN")
        .output()
        .expect("native command execution")
}
fn envelope(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("typed JSON envelope")
}

#[test]
fn core_serve_and_components_are_registered_without_server_aliases() {
    for id in [
        "core.serve",
        "core.component.selection",
        "core.component.remove",
    ] {
        let output = ds(&["capabilities", id, "--output", "json"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value = envelope(&output);
        assert_eq!(value["data"]["command"]["id"], id);
        assert_eq!(value["data"]["command"]["path"][0], "core");
    }
    for args in [
        vec!["server", "serve", "--help"],
        vec!["capabilities", "server.serve", "--output", "json"],
    ] {
        assert!(
            !ds(&args).status.success(),
            "retired alias remained reachable"
        );
    }
    let help = ds(&["core", "serve", "--help"]);
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("--lane") && text.contains("--state-dir"));
}

#[test]
fn removal_requires_confirmation_and_validates_generation_before_native_effects() {
    let args = [
        "core",
        "component",
        "remove",
        "--component",
        "network",
        "--channel",
        "canary",
        "--expected-generation",
        "0",
        "--output",
        "json",
    ];
    let unconfirmed = ds(&args);
    assert!(!unconfirmed.status.success());
    assert_eq!(
        envelope(&unconfirmed)["error"]["code"],
        "confirmation_required"
    );
    let invalid = ds(&[
        "core",
        "component",
        "remove",
        "--component",
        "network",
        "--channel",
        "canary",
        "--expected-generation",
        "invalid",
        "--yes",
        "--output",
        "json",
    ]);
    assert!(!invalid.status.success());
    assert_eq!(
        envelope(&invalid)["error"]["code"],
        "component_generation_invalid"
    );
}
