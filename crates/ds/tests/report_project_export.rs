//! `ds report project export` — the headless transformer report batch.
//!
//! No native user exists in the test environment, so the credential gate is
//! the furthest a real run reaches; everything that must be decided BEFORE a
//! credential is restored is asserted to be decided there, and the descriptor
//! is asserted to be the headless, project-fenced, local-file-writing surface
//! a server host may call.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

mod common;

fn ds(args: &[&str]) -> Value {
    common::json(args).0
}

/// A `ds` that can restore nothing: the development native catalogue makes
/// the native availability gate pass, and an empty config home holds no user.
fn headless(args: &[&str], engine: Option<&str>) -> Value {
    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-cli-auth/tests/fixtures/development-catalog.json");
    let config = tempfile::tempdir().expect("temp config home");
    let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
    command
        .args(args)
        .env("NO_COLOR", "1")
        .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", &bundle)
        .env("DS_CONFIG_HOME", config.path())
        .env(
            "DS_DESKTOP_DESCRIPTOR",
            config.path().join("no-desktop.json"),
        );
    if let Some(engine) = engine {
        command
            .env("DS_REPORT_BIN", engine)
            .env("PATH", "/nonexistent");
    }
    let output = command.output().expect("ds binary runs");
    serde_json::from_slice(&output.stdout).unwrap_or(Value::Null)
}

fn code(envelope: &Value) -> String {
    envelope["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn the_descriptor_is_a_headless_project_fenced_local_file_write() {
    let descriptor = ds(&["capabilities", "report.project.export", "--output", "json"]);
    let command = &descriptor["data"]["command"];
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "local_file_write");
    assert_eq!(command["execution"], "sync");
    let inputs: Vec<&str> = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("input name"))
        .collect();
    assert_eq!(
        inputs,
        [
            "transformer",
            "out-dir",
            "concurrency",
            "admin-bounds",
            "lane"
        ]
    );
    // No project override, no Desktop, no URL: the selected project is the
    // native user's own, and the browser is never in the path.
    for forbidden in ["project", "desktop-descriptor", "url", "request"] {
        assert!(!inputs.contains(&forbidden), "{forbidden}");
    }
    let refusals: Vec<&str> = command["refusals"]
        .as_array()
        .expect("refusals")
        .iter()
        .map(|refusal| refusal["code"].as_str().expect("code"))
        .collect();
    for expected in [
        "headless_signed_out",
        "headless_project_not_selected",
        "reporter_engine_missing",
        "export_blocked",
        "engine_refused",
        "report_output_exists",
        "report_batch_failed",
        "admin_bounds_unavailable",
        "invalid_concurrency",
        "reserved_transformer_identity",
        "transformer_not_active",
    ] {
        assert!(refusals.contains(&expected), "{expected} is not documented");
    }
    // The printing story is in the words an agent reads first.
    assert!(
        command["purpose"]
            .as_str()
            .expect("purpose")
            .contains("named print output")
    );
    assert!(!command["confirmation_required"].as_bool().unwrap_or(false));
}

#[test]
fn local_refusals_are_decided_before_any_credential_is_restored() {
    let out = tempfile::tempdir().expect("out dir");
    let out_dir = out.path().to_str().expect("utf-8");
    // The bound on resident engines is the kernel's, checked first.
    for bad in ["0", "65", "two"] {
        assert_eq!(
            code(&headless(
                &[
                    "report",
                    "project",
                    "export",
                    "--out-dir",
                    out_dir,
                    "--concurrency",
                    bad,
                    "--output",
                    "json"
                ],
                None,
            )),
            "invalid_concurrency",
            "--concurrency {bad}"
        );
    }
    // A reserved computed identity is refused as scope, exactly as the
    // compounded lane refuses it.
    assert_eq!(
        code(&headless(
            &[
                "report",
                "project",
                "export",
                "--transformer",
                "collisions",
                "--out-dir",
                out_dir,
                "--output",
                "json"
            ],
            None,
        )),
        "reserved_transformer_identity"
    );
    // The output directory is required by the parser.
    let missing = headless(&["report", "project", "export", "--output", "json"], None);
    assert_ne!(code(&missing), "", "{missing}");
    assert_ne!(code(&missing), "headless_signed_out");
    // With valid inputs the credential gate is the first thing that answers,
    // and nothing was written.
    assert_eq!(
        code(&headless(
            &[
                "report",
                "project",
                "export",
                "--out-dir",
                out_dir,
                "--output",
                "json"
            ],
            None,
        )),
        "headless_signed_out"
    );
    assert!(
        std::fs::read_dir(out.path())
            .expect("out dir")
            .next()
            .is_none()
    );
}

#[test]
fn an_absent_engine_is_refused_by_the_availability_gate() {
    let out = tempfile::tempdir().expect("out dir");
    let envelope = headless(
        &[
            "report",
            "project",
            "export",
            "--out-dir",
            out.path().to_str().expect("utf-8"),
            "--output",
            "json",
        ],
        Some("/nonexistent/ds-report"),
    );
    assert_eq!(code(&envelope), "reporter_engine_missing", "{envelope}");
}
