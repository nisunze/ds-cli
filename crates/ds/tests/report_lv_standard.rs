//! The CLI forwards governed jobs intact and preserves the owner's refusals.
#![cfg(unix)]
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, process::Command};

#[test]
fn standard_export_routes_exact_bytes_without_desktop_and_preserves_refusals() {
    let root = tempfile::tempdir().unwrap();
    let engine = root.path().join("ds-report");
    std::fs::write(&engine, r#"#!/bin/sh
set -eu
case "$1" in
task-schemas) printf '%s' '{"tasks":[{"name":"export_lv_standard","subcommand":"export-lv-standard"}]}' ;;
export-lv-standard)
  test "$#" = 5
  test "$2" = --request
  test "$4" = --result
  cmp "$3" "$TEST_REQUEST"
  test ! -e "$5"
  if test "$TEST_MODE" = refused; then
    printf '%s' '{"status":"failed","blockers":[{"code":"print_logo_invalid","message":"held digest differs"}],"artifacts":[]}' > "$5"
    exit 17
  fi
  printf '%s' '{"schema":"ds.lv-print-result/v1","status":"completed","publication":"nothing_published","artifacts":[],"pages":[]}' > "$5" ;;
*) exit 99 ;;
esac
"#).unwrap();
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
    let request = root.path().join("job.json");
    let raw = b"exact captured job bytes\n";
    std::fs::write(&request, raw).unwrap();
    for mode in ["complete", "refused"] {
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(["report", "export", "--task", "lv-standard", "--request"])
            .arg(&request)
            .args(["--output", "json"])
            .env("DS_REPORT_BIN", &engine)
            .env("TEST_MODE", mode)
            .env("TEST_REQUEST", &request)
            .env("TMPDIR", root.path())
            .env("DS_CONFIG_HOME", root.path())
            .env("DS_DESKTOP_DESCRIPTOR", root.path().join("absent"))
            .output()
            .unwrap();
        let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
        if mode == "complete" {
            assert!(output.status.success(), "{answer}");
            assert_eq!(answer["data"]["publication"], "nothing_published");
        } else {
            assert!(!output.status.success());
            assert_eq!(answer["error"]["code"], "export_blocked");
            assert_eq!(
                answer["error"]["detail"]["blockers"][0]["code"],
                "print_logo_invalid"
            );
        }
        assert_eq!(std::fs::read(&request).unwrap(), raw);
    }
}

#[test]
fn standard_registry_and_missing_job_have_named_refusals() {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["report", "layout", "schema", "--output", "json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
    let text = answer.to_string();
    for value in [
        "ds.print-standard/v1",
        "ds.print-standard-overrides/v1",
        "lv-combined-a3-v1",
        "general_notes",
    ] {
        assert!(text.contains(value), "{value} absent from registry");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "report",
            "export",
            "--task",
            "lv-standard",
            "--output",
            "json",
        ])
        .env("DS_REPORT_BIN", "/nonexistent")
        .output()
        .unwrap();
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(answer["error"]["code"], json!("reporter_engine_missing"));
    // The platform availability gate precedes the handler. With an executable
    // present, the missing job is refused before any engine task is invoked.
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "report",
            "export",
            "--task",
            "lv-standard",
            "--output",
            "json",
        ])
        .env("DS_REPORT_BIN", "/bin/false")
        .output()
        .unwrap();
    let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(answer["error"]["code"], json!("missing_input"));
}
