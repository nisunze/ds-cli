//! The report, intake and survey-dashboard doors compute on this machine.
//!
//! Owner rule: there is no hybrid compute. ds-brain only authorizes, stores
//! and manages; reports, design processing and survey dashboard aggregations
//! run on an edge. These tests read the installed command surface the way an
//! agent does, and run each door signed out with every outbound connection
//! trapped: a refusal must be decided here, before any request leaves.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

mod common;

/// One `ds` run with no signed-in user and a trap for any socket it opens.
fn signed_out(args: &[&str]) -> (Value, bool) {
    let trap = std::net::TcpListener::bind("127.0.0.1:0").expect("trap");
    trap.set_nonblocking(true).expect("nonblocking trap");
    let proxy = format!("http://{}", trap.local_addr().expect("trap address"));
    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-cli-auth/tests/fixtures/development-catalog.json");
    let config = tempfile::tempdir().expect("temp config home");
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("NO_COLOR", "1")
        .env("DS_SRE_REPORTING", "off")
        .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", &bundle)
        .env("DS_CONFIG_HOME", config.path())
        .env(
            "DS_DESKTOP_DESCRIPTOR",
            config.path().join("no-desktop.json"),
        )
        .env("HTTP_PROXY", &proxy)
        .env("HTTPS_PROXY", &proxy)
        .env("ALL_PROXY", &proxy)
        .env("NO_PROXY", "")
        .output()
        .expect("ds binary runs");
    let connected = trap.accept().is_ok();
    (
        serde_json::from_slice(&output.stdout).unwrap_or(Value::Null),
        connected,
    )
}

fn descriptor(id: &str) -> Value {
    let (descriptor, code) = common::json(&["capabilities", id, "--output", "json"]);
    assert_eq!(code, 0, "`{id}` is not described: {descriptor}");
    descriptor["data"]["command"].clone()
}

fn codes(command: &Value) -> Vec<String> {
    command["refusals"]
        .as_array()
        .expect("refusals")
        .iter()
        .filter_map(|refusal| refusal["code"].as_str().map(str::to_owned))
        .collect()
}

/// The cloud individual-report computation is gone from every surface; its
/// edge twin is `report project export`.
#[test]
fn the_cloud_report_computation_is_deleted() {
    let (refused, code) =
        common::json(&["capabilities", "report.project.compute", "--output", "json"]);
    assert_ne!(code, 0, "{refused}");
    let (export, code) = common::json(&["capabilities", "--export", "tools", "--output", "json"]);
    assert_eq!(code, 0);
    let ids: Vec<&str> = export["data"]["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .filter_map(|command| command["id"].as_str())
        .collect();
    assert!(!ids.contains(&"report.project.compute"));
    assert!(ids.contains(&"report.project.export"));
    assert!(ids.contains(&"report.project.compounded"));
    assert!(ids.contains(&"survey.dashboard"));
}

/// The Compounded Report is composed here and queued for the governed door;
/// its descriptor says so and declares the edge refusals.
#[test]
fn the_compounded_report_is_composed_on_this_machine() {
    let command = descriptor("report.project.compounded");
    let purpose = command["purpose"].as_str().expect("purpose");
    assert!(purpose.contains("on this machine"), "{purpose}");
    assert!(purpose.contains("export-compounded_report"), "{purpose}");
    assert!(!purpose.to_ascii_lowercase().contains("cloud"), "{purpose}");
    let codes = codes(&command);
    for code in [
        "compounded_inputs_invalid",
        "compounded_archive_too_large",
        "compounded_store_unreadable",
        "combined_inputs_not_current",
        "reporter_engine_missing",
    ] {
        assert!(codes.iter().any(|c| c == code), "{code} undeclared");
    }
    let (refusal, connected) = signed_out(&[
        "report",
        "project",
        "compounded",
        "--project",
        "test-project",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(refusal["status"], "error", "{refusal}");
    assert!(
        !connected,
        "a signed-out compounded run opened a connection"
    );
}

/// Design intake processes on this machine; ds-brain only stores the bytes.
#[test]
fn design_intake_processes_on_this_machine() {
    let command = descriptor("design.intake.upload");
    let purpose = command["purpose"].as_str().expect("purpose");
    assert!(purpose.contains("processed on this machine"), "{purpose}");
    assert!(purpose.contains("ds-brain only stores them"), "{purpose}");
    let codes = codes(&command);
    for code in [
        "status_upload_layers_unrecognized",
        "status_upload_transformer_unregistered",
        "network_engine_missing",
    ] {
        assert!(codes.iter().any(|c| c == code), "{code} undeclared");
    }
    let file = tempfile::NamedTempFile::with_suffix(".geojson").expect("temp file");
    std::fs::write(
        file.path(),
        br#"{"type":"FeatureCollection","features":[]}"#,
    )
    .unwrap();
    let (refusal, connected) = signed_out(&[
        "design",
        "intake",
        "upload",
        "--project",
        "test-project",
        "--file",
        file.path().to_str().unwrap(),
        "--mode",
        "lv-process",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(refusal["status"], "error", "{refusal}");
    assert!(!connected, "a signed-out intake opened a connection");
}

/// The survey dashboard is an offline read over the held copy.
#[test]
fn the_survey_dashboard_is_computed_from_held_rows_offline() {
    let command = descriptor("survey.dashboard");
    assert_eq!(command["effect"], "read_only");
    assert_eq!(command["authority"], "none");
    let codes = codes(&command);
    assert!(codes.iter().any(|c| c == "survey_dashboard_nothing_held"));
    let (refusal, connected) = signed_out(&[
        "survey",
        "dashboard",
        "--project",
        "test-project",
        "--output",
        "json",
    ]);
    assert_eq!(refusal["status"], "error", "{refusal}");
    assert!(!connected, "the dashboard opened a connection");
}
