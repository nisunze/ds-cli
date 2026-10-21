//! The thin wrapper's process mapping and receipt lifetime, using synthetic
//! owner responses. Engineering and PDF rendering stay in the reporter tests.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

struct Harness {
    root: tempfile::TempDir,
    engine: PathBuf,
    request: PathBuf,
    receipt_log: PathBuf,
}

impl Harness {
    fn new(document: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let engine = root.path().join("ds-report");
        let request = root.path().join("request.json");
        let receipt_log = root.path().join("receipt-path");
        std::fs::write(&request, document).unwrap();
        std::fs::write(
            &engine,
            r#"#!/bin/sh
set -eu
test "$#" = 5
test "$1" = render-voltage-drop-combined
test "$2" = --request
test "$4" = --result
test ! -e "$5"
printf '%s' "$5" > "$TEST_RECEIPT_LOG"
if test "$TEST_ENGINE_MODE" != missing; then
    cp "$3" "$5"
fi
if test "$TEST_ENGINE_MODE" = refused; then
    printf '%s' 'synthetic roster refusal' >&2
    exit 17
fi
"#,
        )
        .unwrap();
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            root,
            engine,
            request,
            receipt_log,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
        command
            .env("DS_REPORT_BIN", &self.engine)
            .env("TMPDIR", self.root.path())
            .env("XDG_CONFIG_HOME", self.root.path())
            .env("TEST_RECEIPT_LOG", &self.receipt_log)
            .env("TEST_ENGINE_MODE", "complete")
            .env("DS_CLI_NONINTERACTIVE", "1");
        command
    }

    fn run(&self, request: &Path, result: Option<&Path>, mode: &str) -> (Value, i32) {
        let mut command = self.command();
        command
            .args(["report", "voltage-drop-combined", "--request"])
            .arg(request)
            .args(["--output", "json"])
            .env("TEST_ENGINE_MODE", mode);
        if let Some(result) = result {
            command.arg("--result").arg(result);
        }
        decode(command)
    }

    fn engine_result(&self) -> PathBuf {
        PathBuf::from(std::fs::read_to_string(&self.receipt_log).unwrap())
    }
}

fn decode(mut command: Command) -> (Value, i32) {
    let output = command.output().unwrap();
    (
        serde_json::from_slice(&output.stdout).expect("CLI returns a JSON envelope"),
        output.status.code().unwrap(),
    )
}

fn complete_receipt() -> Value {
    json!({
        "schema": "ds.voltage-drop-combined.render-receipt/v1",
        "status": "complete",
        "coverage": {"roster": 3, "calculated": 1, "reserved": 1, "held": 1},
        "pages": 1,
        "output": "/synthetic/overview.pdf",
        "output_sha256": "a".repeat(64),
        "sources": [{"transformer": "test_transformer", "sha256": "b".repeat(64)}],
        "publication": "nothing_published"
    })
}

#[test]
fn discovery_and_success_preserve_the_local_owner_receipt() {
    let receipt = complete_receipt();
    let harness = Harness::new(&receipt.to_string());
    let mut command = harness.command();
    command.args([
        "capabilities",
        "report.voltage-drop-combined",
        "--output",
        "json",
    ]);
    let (descriptor, code) = decode(command);
    assert_eq!(code, 0);
    assert_eq!(descriptor["data"]["command"]["effect"], "local_file_write");
    assert_eq!(descriptor["data"]["command"]["authority"], "none");

    let (answer, code) = harness.run(&harness.request, None, "complete");
    assert_eq!(code, 0);
    assert_eq!(answer["data"], receipt);
    assert!(!harness.engine_result().exists());

    let result = harness.root.path().join("retained.json");
    let (answer, code) = harness.run(&harness.request, Some(&result), "complete");
    assert_eq!(code, 0);
    assert_eq!(answer["data"]["result_path"], json!(result));
    assert_eq!(
        std::fs::read_to_string(result).unwrap(),
        receipt.to_string()
    );
}

#[test]
fn failure_receipt_is_returned_in_full_and_caller_bytes_are_retained() {
    let receipt = json!({
        "schema": "ds.voltage-drop-combined.render-receipt/v1",
        "status": "refused", "reason": "synthetic pinned evidence mismatch",
        "publication": "nothing_published"
    });
    let harness = Harness::new(&receipt.to_string());
    for keep in [false, true] {
        let result = harness.root.path().join("refusal.json");
        let (answer, code) = harness.run(
            &harness.request,
            keep.then_some(result.as_path()),
            "refused",
        );
        assert_eq!(code, 6);
        assert_eq!(answer["error"]["code"], "engine_refused");
        let detail = &answer["error"]["detail"];
        assert_eq!(detail["receipt"], receipt);
        assert_eq!(detail["exit_code"], 17);
        assert_eq!(detail["engine"], "synthetic roster refusal");
        assert_eq!(harness.engine_result().exists(), keep);
        if keep {
            assert_eq!(detail["result_path"], json!(result));
            assert_eq!(
                std::fs::read_to_string(result).unwrap(),
                receipt.to_string()
            );
        }
    }
}

#[test]
fn malformed_temporary_receipt_remains_available_for_inspection() {
    let harness = Harness::new("not JSON");
    let (answer, code) = harness.run(&harness.request, None, "complete");
    assert_eq!(code, 6);
    assert_eq!(answer["error"]["code"], "engine_refused");
    let result = harness.engine_result();
    assert_eq!(answer["error"]["detail"]["result_path"], json!(result));
    assert!(
        answer["error"]["detail"]["receipt_error"]
            .as_str()
            .unwrap()
            .contains("decode receipt")
    );
    assert_eq!(std::fs::read_to_string(result).unwrap(), "not JSON");
}

#[test]
fn success_requires_a_complete_receipt_even_when_the_process_exits_zero() {
    let mut receipt = complete_receipt();
    receipt["status"] = json!("refused");
    let harness = Harness::new(&receipt.to_string());
    let (answer, code) = harness.run(&harness.request, None, "complete");
    assert_eq!(code, 6);
    assert_eq!(answer["error"]["detail"]["receipt"], receipt);
    assert!(!harness.engine_result().exists());

    let (answer, code) = harness.run(&harness.request, None, "missing");
    assert_eq!(code, 6);
    assert_eq!(answer["error"]["code"], "engine_refused");
    assert!(
        answer["error"]["detail"]["receipt_error"]
            .as_str()
            .unwrap()
            .contains("read receipt")
    );
}

#[test]
fn relative_paths_and_existing_receipts_refuse_before_the_engine_runs() {
    let harness = Harness::new(&complete_receipt().to_string());
    let (answer, code) = harness.run(Path::new("relative.json"), None, "complete");
    assert_eq!(code, 2);
    assert_eq!(answer["error"]["code"], "path_not_absolute");
    let (answer, code) = harness.run(
        &harness.request,
        Some(Path::new("relative.json")),
        "complete",
    );
    assert_eq!(code, 2);
    assert_eq!(answer["error"]["code"], "path_not_absolute");

    let result = harness.root.path().join("existing.json");
    std::fs::write(&result, "retained evidence").unwrap();
    let (answer, code) = harness.run(&harness.request, Some(&result), "complete");
    assert_eq!(code, 5);
    assert_eq!(answer["error"]["code"], "result_exists");
    assert_eq!(
        std::fs::read_to_string(result).unwrap(),
        "retained evidence"
    );
    let link = harness.root.path().join("dangling.json");
    std::os::unix::fs::symlink(harness.root.path().join("absent.json"), &link).unwrap();
    let (answer, code) = harness.run(&harness.request, Some(&link), "complete");
    assert_eq!(code, 5);
    assert_eq!(answer["error"]["code"], "result_exists");
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());

    let (answer, code) = harness.run(
        &harness.root.path().join("absent-request.json"),
        None,
        "complete",
    );
    assert_eq!(code, 2);
    assert_eq!(answer["error"]["code"], "request_not_found");
    assert!(!harness.receipt_log.exists());
}
