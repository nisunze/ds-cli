//! Local collision detection and its publication through the real `ds`
//! dispatcher. The engine is admitted before any credential or project read,
//! an engine without the task is refused as incompatible rather than replaced,
//! and publish refuses an unconfirmed or unproved result file before it reads
//! a credential. Nothing here may reach a service: every native run uses a
//! signed-out private config.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn ds(args: &[&str], reporter: Option<&Path>) -> Value {
    let config = tempfile::tempdir().expect("config root");
    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-cli-auth/tests/fixtures/development-catalog.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
    command
        .args(args)
        .env("NO_COLOR", "1")
        .env("DS_SRE_REPORTING", "off")
        .env(
            "DS_DESKTOP_DESCRIPTOR",
            config.path().join("no-desktop.json"),
        )
        .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", &bundle)
        .env("DS_CONFIG_HOME", config.path())
        .env_remove("DS_REPORT_BIN");
    if let Some(reporter) = reporter {
        command.env("DS_REPORT_BIN", reporter);
    }
    let output = command.output().expect("ds runs");
    serde_json::from_slice(&output.stdout).unwrap_or(Value::Null)
}

fn code(envelope: &Value) -> &str {
    envelope["error"]["code"].as_str().unwrap_or_default()
}

fn described(id: &str) -> Value {
    ds(&["capabilities", id, "--output", "json"], None)["data"]["command"].clone()
}

#[test]
fn detect_writes_only_its_private_result_and_publish_is_the_confirmed_write() {
    let detect = described("design.collisions.detect");
    assert_eq!(detect["authority"], "headless_project");
    assert_eq!(detect["effect"], "local_file_write");
    assert_eq!(detect["confirmation_required"], false);
    let read = detect["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|input| input["name"] == "read")
        .expect("--read");
    assert_eq!(read["choices"], serde_json::json!(["refresh", "held"]));

    let publish = described("design.collisions.publish");
    assert_eq!(publish["authority"], "headless_project");
    assert_eq!(publish["effect"], "global_write");
    assert_eq!(publish["confirmation_required"], true);
    assert!(
        publish["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|input| input["name"] == "result")
    );
}

#[test]
fn detect_refuses_its_output_and_a_missing_engine_before_any_credential() {
    let fixture = tempfile::tempdir().unwrap();
    let relative = ds(
        &[
            "design",
            "collisions",
            "detect",
            "--project",
            "p1",
            "--out",
            "collisions.json",
            "--output",
            "json",
        ],
        None,
    );
    assert_eq!(code(&relative), "collisions_output_invalid", "{relative}");

    let taken = fixture.path().join("taken.json");
    std::fs::write(&taken, b"owner bytes").unwrap();
    let existing = ds(
        &[
            "design",
            "collisions",
            "detect",
            "--project",
            "p1",
            "--out",
            taken.to_str().unwrap(),
            "--output",
            "json",
        ],
        None,
    );
    assert_eq!(code(&existing), "collisions_output_invalid", "{existing}");
    assert_eq!(std::fs::read(&taken).unwrap(), b"owner bytes");

    let missing = fixture.path().join("missing-reporter");
    let refused = ds(
        &[
            "design",
            "collisions",
            "detect",
            "--project",
            "p1",
            "--read",
            "held",
            "--output",
            "json",
        ],
        Some(&missing),
    );
    assert_eq!(code(&refused), "reporter_engine_missing", "{refused}");
    assert!(refused["data"].is_null());
}

#[cfg(unix)]
fn fake_reporter(dir: &Path, schemas: &str) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let log = dir.join("calls.log");
    let path = dir.join("reporter");
    let script = format!(
        "#!/bin/sh\necho \"$1\" >> '{}'\ncase \"$1\" in\ntask-schemas) printf '%s\\n' '{}' ;;\n*) exit 9 ;;\nesac\n",
        log.display(),
        schemas
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    (path, log)
}

#[cfg(unix)]
#[test]
fn the_engine_contract_is_read_from_the_engine_and_never_substituted() {
    let detect = [
        "design",
        "collisions",
        "detect",
        "--project",
        "p1",
        "--read",
        "held",
        "--output",
        "json",
    ];

    let older = tempfile::tempdir().unwrap();
    let (reporter, log) = fake_reporter(
        older.path(),
        r#"{"tasks":[{"subcommand":"export-combined","network":"none","request_schema":{}}]}"#,
    );
    let refused = ds(&detect, Some(&reporter));
    assert_eq!(
        code(&refused),
        "collisions_engine_incompatible",
        "{refused}"
    );
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "task-schemas\n");

    // A compatible engine is admitted, then the signed-out account stops the
    // run before any project read: detection never started.
    let current = tempfile::tempdir().unwrap();
    let (reporter, log) = fake_reporter(
        current.path(),
        r#"{"tasks":[{"subcommand":"detect-collisions","network":"none","request_schema":{"properties":{"schema":{"const":"ds.reporter.detect-collisions-request/v1"},"transformers":{"items":{"properties":{"layers":{"properties":{"customers":{},"lv_lines":{},"service_cables":{},"tr":{}}}}}}}}}]}"#,
    );
    let signed_out = ds(&detect, Some(&reporter));
    assert_eq!(code(&signed_out), "headless_signed_out", "{signed_out}");
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "task-schemas\n");
}

#[test]
fn publish_refuses_unconfirmed_and_unproved_results_before_any_credential() {
    let fixture = tempfile::tempdir().unwrap();
    let file = fixture.path().join("collisions.json");
    std::fs::write(&file, b"{}").unwrap();
    let path = file.to_str().unwrap();

    let unconfirmed = ds(
        &[
            "design",
            "collisions",
            "publish",
            "--project",
            "p1",
            "--result",
            path,
            "--output",
            "json",
        ],
        None,
    );
    assert_eq!(code(&unconfirmed), "confirmation_required", "{unconfirmed}");

    let relative = ds(
        &[
            "design",
            "collisions",
            "publish",
            "--project",
            "p1",
            "--result",
            "collisions.json",
            "--yes",
            "--output",
            "json",
        ],
        None,
    );
    assert_eq!(code(&relative), "collisions_result_invalid", "{relative}");

    // Not a private, proved detect result: refused as invalid, never sent.
    let unproved = ds(
        &[
            "design",
            "collisions",
            "publish",
            "--project",
            "p1",
            "--result",
            path,
            "--yes",
            "--output",
            "json",
        ],
        None,
    );
    assert_eq!(code(&unproved), "collisions_result_invalid", "{unproved}");
}
