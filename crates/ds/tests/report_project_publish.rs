//! `ds report project publish` — the route that rescues report artifacts a
//! machine already holds.
//!
//! An operator ended a day with 593 verified artifacts, 379 MB, on one PC and
//! no way to publish them short of re-running the engine — which would cost
//! another day and produce different bytes for the same rooms. This command is
//! the other way, and these tests pin the two things that make it trustworthy:
//! it is discoverable as a rescue, and everything it can refuse locally is
//! refused BEFORE a credential is restored, so pointing it at the wrong
//! directory never touches protected native state.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

mod common;

fn ds(args: &[&str]) -> Value {
    common::json(args).0
}

/// A `ds` that can restore nothing: the development native catalogue makes the
/// native availability gate pass, and an empty config home holds no user. Any
/// refusal that still arrives is therefore one decided locally.
fn headless(args: &[&str]) -> Value {
    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-cli-auth/tests/fixtures/development-catalog.json");
    let config = tempfile::tempdir().expect("temp config home");
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("NO_COLOR", "1")
        .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", &bundle)
        .env("DS_CONFIG_HOME", config.path())
        .env(
            "DS_DESKTOP_DESCRIPTOR",
            config.path().join("no-desktop.json"),
        )
        .output()
        .expect("ds binary runs");
    serde_json::from_slice(&output.stdout).unwrap_or(Value::Null)
}

fn code(envelope: &Value) -> String {
    envelope["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// A stranger's agent has to be able to find this with its own words, and has
/// to be told the two things that decide whether it is safe to run: the engine
/// does not run again, and a digest that does not verify is refused by name.
#[test]
fn the_rescue_route_is_discoverable_and_states_what_it_will_not_do() {
    let descriptor = ds(&["capabilities", "report.project.publish", "--output", "json"]);
    let command = &descriptor["data"]["command"];
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["requires"], "server");
    let purpose = command["purpose"].as_str().expect("purpose");
    assert!(
        purpose.contains("WITHOUT running the"),
        "the one guarantee an operator needs is missing: {purpose}"
    );
    let codes: Vec<&str> = command["refusals"]
        .as_array()
        .expect("refusals")
        .iter()
        .filter_map(|refusal| refusal["code"].as_str())
        .collect();
    for expected in [
        "report_artifact_digest_mismatch",
        "report_artifact_missing",
        "report_publish_source_invalid",
        "report_publish_nothing_held",
    ] {
        assert!(codes.contains(&expected), "{expected} is not declared");
    }
    let inputs: Vec<&str> = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .filter_map(|input| input["name"].as_str())
        .collect();
    assert_eq!(
        inputs,
        ["from", "transformer", "server-state-dir", "lane"],
        "the source directory is the whole request; nothing here re-runs a report"
    );
}

/// A wrong `--from` is the operator's most likely mistake, and it must cost
/// nothing: no credential restored, no protected state opened, no queue
/// touched. This is the ordering the command relies on to be safe to retry.
#[test]
fn an_unreadable_source_is_refused_before_any_credential_is_restored() {
    let missing = tempfile::tempdir().expect("temp root");
    let absent = missing.path().join("never-exported");
    let envelope = headless(&[
        "report",
        "project",
        "publish",
        "--from",
        absent.to_str().expect("path"),
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(
        code(&envelope),
        "report_publish_source_invalid",
        "{envelope:#?}"
    );
    assert!(
        envelope["error"]["remedy"]
            .as_str()
            .unwrap_or_default()
            .contains("report-run.json"),
        "the remedy has to name what a publishable directory looks like: {envelope:#?}"
    );
}

/// Publishing is a durable effect, so it is confirmed like every other one.
/// Without `--yes` nothing is read and nothing is sealed.
#[test]
fn publishing_held_artifacts_is_a_confirmed_effect() {
    let source = tempfile::tempdir().expect("temp root");
    let envelope = headless(&[
        "report",
        "project",
        "publish",
        "--from",
        source.path().to_str().expect("path"),
        "--output",
        "json",
    ]);
    assert_eq!(code(&envelope), "confirmation_required", "{envelope:#?}");
}
