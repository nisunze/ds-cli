//! Actual captured transport and corrupt output controls; no engineering clone.
use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::differential::*;
use std::{env, fs, path::PathBuf, process::Command};

fn request() -> DifferentialPlanRequest {
    serde_json::from_value(serde_json::json!({"gdfs":{},"differential":{"selected_feeders":[]},"customer_source_addresses":[]})).unwrap()
}
#[test]
#[ignore = "requires the built protocol-only adversarial fixture"]
fn acceptance_differential_rejects_unbound_artifacts() {
    let fixture =
        PathBuf::from(env::var_os("DS_NETWORK_GRID_FIXTURE").expect("fixture executable"));
    let scratch = tempfile::tempdir().unwrap();
    for (mode, expected) in [
        (
            "differential-missing-capability",
            "network_engine_incompatible",
        ),
        ("differential-wrong-schema", "network_result_invalid"),
        ("differential-wrong-scope", "network_result_invalid"),
        ("differential-wrong-reason", "network_result_invalid"),
        ("differential-wrong-full", "network_result_invalid"),
        ("wrong-digest", "network_result_digest_mismatch"),
        ("wrong-input", "network_receipt_invalid"),
        ("differential-wrong-identity", "network_receipt_invalid"),
        ("wrong-file", "network_receipt_invalid"),
        ("wrong-receipt-schema", "network_receipt_invalid"),
        ("symlink-result", "network_result_invalid"),
        ("changed-input", "network_input_changed"),
    ] {
        if mode == "symlink-result" && !cfg!(unix) {
            continue;
        }
        let executable = scratch.path().join(mode);
        fs::copy(&fixture, &executable).unwrap();
        let output = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "isolated_differential_adversary",
                "--ignored",
                "--nocapture",
            ])
            .env("DS_NETWORK_ENGINE_BIN", &executable)
            .env("DS_DIFFERENTIAL_EXPECTED", expected)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{mode}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
#[test]
#[ignore = "isolated process selection"]
fn isolated_differential_adversary() {
    let scratch = tempfile::tempdir().unwrap();
    let client = CapturedNetworkClient::capture(1, &|| false).unwrap();
    assert_eq!(
        client
            .lv_differential_plan(&request(), scratch.path(), &|| false)
            .unwrap_err()
            .code(),
        env::var("DS_DIFFERENTIAL_EXPECTED").unwrap()
    );
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}
#[test]
#[ignore = "requires a matched built native owner"]
fn acceptance_differential_owner_corpus() {
    let cases: serde_json::Value =
        serde_json::from_slice(&fs::read(env::var_os("DS_DIFFERENTIAL_CORPUS").unwrap()).unwrap())
            .unwrap();
    let baseline: serde_json::Value = serde_json::from_slice(
        &fs::read(env::var_os("DS_DIFFERENTIAL_BASELINE").unwrap()).unwrap(),
    )
    .unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let client = CapturedNetworkClient::capture(1, &|| false).unwrap();
    for (case, old) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(baseline.as_array().unwrap())
    {
        let body = &case["document"]["differential_scope"];
        let Ok(request) = serde_json::from_value::<DifferentialPlanRequest>(body.clone()) else {
            continue;
        };
        if old["answer"]["status"] == "ok" {
            let result = client
                .lv_differential_plan(&request, scratch.path(), &|| false)
                .unwrap();
            assert_eq!(
                serde_json::to_value(result.result.plan).unwrap(),
                old["answer"]["data"]["differential_scope"],
                "{}",
                case["name"]
            );
        } else if case["name"] == "forbidden-auto-selection" {
            let error = client
                .lv_differential_plan(&request, scratch.path(), &|| false)
                .unwrap_err();
            assert_eq!(error.owner_code(), Some("autoprocess_request_invalid"));
        }
    }
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}
