//! Fixed A4 render routing; no test launches Chromium or solves a network.
#![cfg(unix)]
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

struct Harness {
    root: tempfile::TempDir,
}
impl Harness {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let engine = root.path().join("ds-report");
        std::fs::write(&engine, r#"#!/bin/sh
set -eu
printf '%s\n' "$1" >> "$TEST_LOG"
case "$1" in
 task-schemas)
   if test "$TEST_MODE" = unavailable; then
     printf '%s' '{"tasks":[]}'
   elif test "$TEST_MODE" = governed; then
     printf '%s' '{"tasks":[{"name":"render_voltage_drop_result","subcommand":"render-voltage-drop-result","request_schema":{"anyOf":[{"properties":{"network_config":{"required":["printing_context","printing_a4"]}}}]}}]}'
   else
     printf '%s' '{"tasks":[{"name":"render_voltage_drop_result","subcommand":"render-voltage-drop-result"}]}'
   fi ;;
 render-voltage-drop-result)
   test "$#" = 5
   test "$2" = --request
   test "$4" = --result
   test ! -e "$5"
   cmp "$3" "$TEST_REQUEST"
   if test "$TEST_MODE" = locale_missing; then
      printf '%s' 'ds-report: report_locale_missing: project_settings missing' >&2
      exit 17
   fi
   if test "$TEST_MODE" = refused; then
      printf '%s' 'voltage_drop_pdf_input_invalid: missing calculated analysis' >&2
      exit 17
   fi
   cp "$TEST_RECEIPT" "$5" ;;
 *) exit 99 ;;
esac
"#).unwrap();
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(root.path().join("request.json"), b"exact caller bytes\n").unwrap();
        std::fs::write(
            root.path().join("receipt.json"),
            Self::receipt().to_string(),
        )
        .unwrap();
        Self { root }
    }
    fn receipt() -> Value {
        json!({"schema":"ds.voltage-drop-pdf.render/v1", "transformer":"T1",
          "source_sha256":"a".repeat(64), "output_sha256":"b".repeat(64),
          "out_pdf":"/synthetic/T1.pdf", "bytes":123, "pages":1,
          "publication":"nothing_published"})
    }
    fn run(&self, extra: &[&str], mode: &str, with_request: bool) -> (Value, i32) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
        command
            .args([
                "report",
                "export",
                "--task",
                "voltage-drop",
                "--output",
                "json",
            ])
            .env("DS_REPORT_BIN", self.root.path().join("ds-report"))
            .env("TEST_LOG", self.root.path().join("calls"))
            .env("TEST_REQUEST", self.root.path().join("request.json"))
            .env("TEST_RECEIPT", self.root.path().join("receipt.json"))
            .env("TEST_MODE", mode)
            .env("TMPDIR", self.root.path())
            .env("XDG_CONFIG_HOME", self.root.path())
            .env(
                "DS_DESKTOP_DESCRIPTOR",
                self.root.path().join("stale-desktop.json"),
            );
        if with_request {
            command
                .arg("--request")
                .arg(self.root.path().join("request.json"));
        }
        let output = command.args(extra).output().unwrap();
        (
            serde_json::from_slice(&output.stdout).unwrap(),
            output.status.code().unwrap(),
        )
    }
    fn calls(&self) -> String {
        std::fs::read_to_string(self.root.path().join("calls")).unwrap_or_default()
    }
}
#[test]
fn a4_passes_exact_request_and_returns_owner_receipt_without_project_state() {
    let h = Harness::new();
    let (answer, code) = h.run(&[], "complete", true);
    assert_eq!(code, 0, "{answer}");
    assert_eq!(answer["data"], Harness::receipt());
    assert_eq!(h.calls(), "task-schemas\nrender-voltage-drop-result\n");
    assert_eq!(
        std::fs::read(h.root.path().join("request.json")).unwrap(),
        b"exact caller bytes\n"
    );
    assert!(!std::fs::read_dir(h.root.path()).unwrap().any(|p| {
        p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("ds-report-result")
    }));
}
#[test]
fn a4_retains_caller_result_and_refuses_overwrite() {
    let h = Harness::new();
    let result = h.root.path().join("kept.json");
    let raw = result.to_str().unwrap();
    let (answer, code) = h.run(&["--result", raw], "complete", true);
    assert_eq!(code, 0, "{answer}");
    assert_eq!(answer["data"]["result_path"], raw);
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&result).unwrap()).unwrap(),
        Harness::receipt()
    );
    let (answer, code) = h.run(&["--result", raw], "complete", true);
    assert_ne!(code, 0);
    assert_eq!(answer["error"]["code"], "result_exists");
    assert_eq!(h.calls().matches("render-voltage-drop-result").count(), 1);
}
#[test]
fn a4_missing_request_or_conflicting_flags_never_reaches_renderer() {
    let h = Harness::new();
    let (answer, code) = h.run(&[], "complete", false);
    assert_ne!(code, 0);
    assert_eq!(answer["error"]["code"], "missing_input");
    let (answer, code) = h.run(&["--network-config", "config.json"], "complete", true);
    assert_ne!(code, 0);
    assert_eq!(answer["error"]["code"], "conflicting_inputs");
    assert!(h.calls().is_empty());
}
#[test]
fn a4_missing_task_is_typed_unavailable_without_export_fallback() {
    let h = Harness::new();
    let (answer, code) = h.run(&[], "unavailable", true);
    assert_ne!(code, 0);
    assert_eq!(answer["error"]["class"], "unavailable");
    assert_eq!(answer["error"]["code"], "unknown_task");
    assert_eq!(h.calls(), "task-schemas\n");
}
#[test]
fn a4_owner_refusal_preserves_diagnostic_without_recomputation() {
    let h = Harness::new();
    let (answer, code) = h.run(&[], "refused", true);
    assert_ne!(code, 0);
    assert_eq!(answer["error"]["code"], "engine_refused");
    assert!(
        answer["error"]["detail"]["engine"]
            .as_str()
            .unwrap_or("")
            .contains("missing calculated analysis"),
        "{answer}"
    );
    assert_eq!(h.calls(), "task-schemas\nrender-voltage-drop-result\n");
}

#[test]
fn project_run_discovers_missing_task_or_browser_before_any_effect() {
    for mode in ["unavailable", "complete", "governed"] {
        let h = Harness::new();
        let directory = h.root.path().join("run");
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args([
                "design",
                "lv",
                "project-run",
                "--project",
                "explicit-project",
                "--transformer",
                "T1",
                "--print-a4",
                "--yes",
                "--output",
                "json",
                "--out-dir",
            ])
            .arg(&directory)
            .env("DS_REPORT_BIN", h.root.path().join("ds-report"))
            .env(
                "DS_WORKSTATION_COMPONENT_ROOT",
                h.root.path().join("components"),
            )
            .env("TEST_MODE", mode)
            .env("TEST_LOG", h.root.path().join("calls"))
            .env("DS_CONFIG_HOME", h.root.path().join("native-state"))
            .env(
                "DS_NATIVE_CLIENT_PROFILE_BUNDLE",
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../ds-cli-auth/tests/fixtures/development-catalog.json"),
            )
            .output()
            .unwrap();
        let answer: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            answer["error"]["code"],
            if mode == "governed" {
                "reporter_browser_missing"
            } else {
                "unknown_task"
            }
        );
        assert!(!directory.exists());
        assert_eq!(h.calls(), "task-schemas\n");
    }
}

#[test]
fn missing_project_report_locale_is_a_named_refusal() {
    let harness = Harness::new();
    let (result, status) = harness.run(&[], "locale_missing", true);
    assert_ne!(status, 0);
    assert_eq!(result["error"]["code"], "report_locale_missing");
}
