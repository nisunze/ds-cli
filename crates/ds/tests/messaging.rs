//! Public messaging descriptors and default output, on an isolated signed-out
//! native profile. No operator credentials or deployed endpoint are reachable.
use serde_json::Value;
use std::process::Command;

fn invoke(args: &[&str]) -> (Value, i32) {
    let home = tempfile::tempdir().unwrap();
    let catalog = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-cli-auth/tests/fixtures/development-catalog.json");
    let reply = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("DS_CONFIG_HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", catalog)
        .output()
        .unwrap();
    (
        serde_json::from_slice(&reply.stdout).unwrap(),
        reply.status.code().unwrap(),
    )
}

#[test]
fn messaging_json_is_default_even_when_global_flags_precede_it() {
    for args in [
        vec!["messaging", "config"],
        vec!["--no-color", "messaging", "config"],
    ] {
        let (reply, code) = invoke(&args);
        assert_ne!(code, 0);
        assert_eq!(reply["command"], "messaging.config");
        assert_eq!(reply["error"]["code"], "headless_signed_out");
    }
}

#[test]
fn every_messaging_command_declares_a_headless_owner_and_real_refusal() {
    let (index, code) = invoke(&["capabilities", "messaging", "--output", "json"]);
    assert_eq!(code, 0);
    let commands = index["data"]["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 12);
    for command in commands {
        let id = command["id"].as_str().unwrap();
        let (descriptor, code) = invoke(&["capabilities", id, "--output", "json"]);
        assert_eq!(code, 0);
        let descriptor = &descriptor["data"]["command"];
        assert_eq!(descriptor["requires"], "server");
        assert_eq!(descriptor["authority"], "headless_user");
        assert!(
            descriptor["refusals"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["code"] == "messaging_not_permitted")
        );
    }
}

#[test]
fn send_and_reply_require_explicit_global_write_confirmation() {
    for command in ["send", "reply"] {
        let (reply, code) = invoke(&[
            "messaging",
            command,
            "--conversation",
            "fixture",
            "--text",
            "hello",
            "--key",
            "same-key",
        ]);
        assert_ne!(code, 0);
        assert_eq!(reply["error"]["code"], "confirmation_required");
    }
}

#[test]
fn human_output_can_be_requested_explicitly() {
    let reply = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["messaging", "read", "--help", "--output", "human"])
        .output()
        .unwrap();
    assert!(reply.status.success());
    let help = String::from_utf8(reply.stdout).unwrap();
    assert!(help.contains("--since"));
    assert!(help.contains("--cursor"));
    assert!(!help.starts_with('{'));
}
