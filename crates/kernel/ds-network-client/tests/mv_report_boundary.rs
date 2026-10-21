use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::{
    engine::json_object_order, grid::NativeGridQuerySourcePin, mv_report::*,
};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, process::Command};
fn required(key: &str) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{key} must name a built acceptance input"))
}
fn request(bytes: &[u8]) -> MvReportRequestV1 {
    MvReportRequestV1 {
        schema: MV_REPORT_REQUEST_SCHEMA_V1.into(),
        source: NativeGridQuerySourcePin {
            package_sha256: format!("{:x}", Sha256::digest(bytes)),
            expected_revision: None,
            engineering_input_root: None,
        },
        json_object_order: json_object_order(),
    }
}
fn isolated(engine: &std::path::Path, test: &str, mode: Option<&str>) {
    let mut command = Command::new(env::current_exe().unwrap());
    command
        .args(["--exact", test, "--ignored", "--nocapture"])
        .env("DS_NETWORK_ENGINE_BIN", engine)
        .env_remove("DS_NETWORK_MV_REPORT_CASE");
    if let Some(mode) = mode {
        command.env("DS_NETWORK_MV_REPORT_CASE", mode);
    }
    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
#[test]
#[ignore = "requires exact built native engine and captured owning projection bytes"]
fn acceptance_mv_report_real_owner() {
    isolated(
        &required("DS_MV_REPORT_REAL_ENGINE"),
        "isolated_mv_report_real_owner",
        None,
    );
}
#[test]
#[ignore = "isolated executable environment"]
fn isolated_mv_report_real_owner() {
    let model_path = required("DS_MV_REPORT_MODEL");
    let model = fs::read(&model_path).unwrap();
    let scratch = tempfile::tempdir_in(required("DS_MV_REPORT_SCRATCH")).unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let mut query = request(&model);
    query.json_object_order = captured.identity().json_object_order;
    let result = captured
        .mv_report(&query, &model, scratch.path(), &|| false)
        .unwrap();
    assert_eq!(
        result.result.source.package_sha256,
        request(&model).source.package_sha256
    );
    assert_eq!(
        serde_json::to_vec(&result.result.projection).unwrap(),
        serde_json::to_vec(
            &serde_json::from_slice::<serde_json::Value>(
                &fs::read(required("DS_MV_REPORT_GOLDEN")).unwrap()
            )
            .unwrap()
        )
        .unwrap()
    );
    assert_eq!(result.identity.json_object_order, query.json_object_order);
    assert_eq!(result.receipt.artifact.file, MV_REPORT_ARTIFACT);
    assert_eq!(
        result.receipt.artifact.sha256,
        format!("{:x}", Sha256::digest(&result.bytes))
    );
    let original = required("DS_MV_REPORT_STAKING_GOLDEN");
    let expected: serde_json::Value = serde_json::from_slice(&fs::read(original).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_vec(&result.result.staking_table).unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
    let mut changed = query.clone();
    changed.source.package_sha256 = "0".repeat(64);
    assert_eq!(
        captured
            .mv_report(&changed, &model, scratch.path(), &|| false)
            .unwrap_err()
            .owner_code(),
        Some("grid_task_source_digest_mismatch")
    );
    assert_eq!(
        captured
            .mv_report(&query, &model, scratch.path(), &|| true)
            .unwrap_err()
            .code(),
        "callee_cancelled"
    );
    let mut oversized = query.clone();
    oversized.source.expected_revision = Some("r".repeat(MAX_MV_REPORT_REQUEST_BYTES));
    assert_eq!(
        captured
            .mv_report(&oversized, &model, scratch.path(), &|| false)
            .unwrap_err()
            .code(),
        "grid_task_bound_refused"
    );
    assert_eq!(fs::read(model_path).unwrap(), model);
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}
#[test]
#[ignore = "requires built adversarial process fixture"]
fn acceptance_mv_report_rejects_invalid_capability_sources_filters_and_artifacts() {
    let engine = required("DS_MV_REPORT_FIXTURE");
    let cases = tempfile::tempdir_in(required("DS_MV_REPORT_SCRATCH")).unwrap();
    for mode in [
        "control",
        "missing-capability",
        "wrong-source",
        "wrong-schema",
        "wrong-order",
        "wrong-columns",
        "wrong-rows",
        "wrong-sum",
        "wrong-warnings",
        "wrong-identity",
        "changed-input",
        "wrong-feature",
        "escaped-artifact",
        "extra-artifact",
        "bad-digest",
    ] {
        let candidate = cases
            .path()
            .join(format!("{mode}{}", std::env::consts::EXE_SUFFIX));
        fs::copy(&engine, &candidate).unwrap();
        isolated(
            &candidate,
            "isolated_mv_report_invalid_artifacts",
            Some(mode),
        );
    }
}
#[test]
#[ignore = "isolated adversarial executable environment"]
fn isolated_mv_report_invalid_artifacts() {
    let model = b"opaque prepared test bytes";
    let scratch = tempfile::tempdir_in(required("DS_MV_REPORT_SCRATCH")).unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let result = captured.mv_report(&request(model), model, scratch.path(), &|| false);
    let mode = env::var("DS_NETWORK_MV_REPORT_CASE").unwrap();
    let expected = match mode.as_str() {
        "control" => None,
        "missing-capability" => Some("network_engine_incompatible"),
        "escaped-artifact" => Some("network_receipt_invalid"),
        "bad-digest" => Some("network_result_digest_mismatch"),
        "wrong-identity" => Some("network_receipt_invalid"),
        "changed-input" => Some("network_input_changed"),
        _ => Some("network_result_invalid"),
    };
    if let Some(expected) = expected {
        assert_eq!(result.unwrap_err().code(), expected);
    } else {
        assert!(result.is_ok());
    }
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}
