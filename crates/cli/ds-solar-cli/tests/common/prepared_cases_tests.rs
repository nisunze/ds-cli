// Copied from ds-solar 3adcc6d apps/solar/tests/common/prepared_cases.rs.
// This adapter owns its test code and frozen ADERM inputs in this repository.
#[path = "prepared_fixture_tests.rs"]
mod common;

use ds_solar_contracts::{
    PreparedCalculationRequest, PreparedCalculationTaskResult, digest::sha256_hex,
};
use ds_solar_runtime::{SolarRuntime, calculate_prepared};
use std::{fs, net::TcpListener, process::Command};

pub fn native_file_task_matches_cloud_semantics_and_makes_zero_provider_requests(binary: &str) {
    let root = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    let request = PreparedCalculationRequest {
        prepared: common::prepared("aderm_bere"),
        render_charts: true,
        run_id: Some("native-file-parity".into()),
    };
    let input = root.path().join("request.json");
    let bytes = serde_json::to_vec(&request).unwrap();
    fs::write(&input, &bytes).unwrap();
    let out = root.path().join("out");
    let child = Command::new(binary)
        .args(["calculate-prepared", "--request"])
        .arg(&input)
        .arg("--out")
        .arg(&out)
        .env("HTTP_PROXY", &proxy)
        .env("HTTPS_PROXY", &proxy)
        .env("ALL_PROXY", &proxy)
        .env("NO_PROXY", "")
        .env("RUST_LOG", "trace")
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(child.stdout.len() < 4096);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&child.stdout).unwrap()["status"],
        "ok"
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "offline execution must make zero provider requests"
    );
    let result: PreparedCalculationTaskResult =
        serde_json::from_slice(&fs::read(out.join("task-result.json")).unwrap()).unwrap();
    let cloud = calculate_prepared(&SolarRuntime::new(), &request).unwrap();
    assert_eq!(
        result.schema_version,
        ds_solar_contracts::PREPARED_CALCULATION_TASK_SCHEMA
    );
    assert_eq!(result.request_sha256, sha256_hex(&bytes));
    assert_eq!(result.prepared_input_digest, request.prepared.input_digest);
    assert_eq!(result.result.result_digest, cloud.result.result_digest);
    assert_eq!(
        serde_json::to_value(&result.result.engine).unwrap(),
        serde_json::to_value(&cloud.result.engine).unwrap()
    );
    let discovery = Command::new(binary).arg("build-info").output().unwrap();
    assert!(discovery.status.success());
    let identity: ds_solar_contracts::EngineIdentity =
        serde_json::from_slice(&discovery.stdout).unwrap();
    assert!(
        identity
            .schemas
            .iter()
            .any(|schema| schema == ds_solar_contracts::PREPARED_CALCULATION_TASK_SCHEMA)
    );
    assert_eq!(result.engine, identity);
    assert_eq!(result.outputs.len(), cloud.outputs.len());
    for (entry, expected) in result.outputs.iter().zip(cloud.outputs) {
        assert_eq!(*entry, expected.entry);
        let artifact = fs::read(out.join("artifacts").join(&entry.name)).unwrap();
        assert_eq!(artifact.len() as u64, entry.byte_count);
        assert_eq!(sha256_hex(&artifact), entry.content_digest);
    }
    assert_eq!(fs::read(&input).unwrap(), bytes);
    let again = Command::new(binary)
        .args(["calculate-prepared", "--request"])
        .arg(&input)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(!again.status.success());
    assert_eq!(fs::read(&input).unwrap(), bytes);
}

pub fn malformed_or_unprepared_requests_leave_no_output_or_stage(binary: &str) {
    let root = tempfile::tempdir().unwrap();
    let request = root.path().join("request.json");
    let out = root.path().join("out");
    for body in [serde_json::json!({"prepared":null}), {
        let mut body = serde_json::to_value(PreparedCalculationRequest {
            prepared: common::prepared("aderm_bere"),
            render_charts: false,
            run_id: None,
        })
        .unwrap();
        body["prepared"]["input_digest"] = serde_json::json!("");
        body
    }] {
        let bytes = serde_json::to_vec(&body).unwrap();
        fs::write(&request, &bytes).unwrap();
        let child = Command::new(binary)
            .args(["calculate-prepared", "--request"])
            .arg(&request)
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap();
        assert!(!child.status.success());
        let error: serde_json::Value = serde_json::from_slice(&child.stdout).unwrap();
        assert_eq!(error["status"], "error");
        assert!(error["error"]["code"].is_string());
        assert!(!out.exists());
        assert_eq!(fs::read(&request).unwrap(), bytes);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
