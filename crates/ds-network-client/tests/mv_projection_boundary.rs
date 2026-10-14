use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::{
    engine::json_object_order, grid::NativeGridQuerySourcePin, mv_projection::*,
};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, process::Command};
fn required(key: &str) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{key} must name a built acceptance input"))
}
fn request(bytes: &[u8]) -> MvProjectionRequestV1 {
    MvProjectionRequestV1 {
        schema: MV_PROJECTION_REQUEST_SCHEMA_V1.into(),
        source: NativeGridQuerySourcePin {
            package_sha256: format!("{:x}", Sha256::digest(bytes)),
            expected_revision: None,
            engineering_input_root: None,
        },
        json_object_order: json_object_order(),
        revision_id: "fixture-mv-projection".into(),
        layers: MV_PROJECTION_LAYERS.to_vec(),
    }
}
fn isolated(engine: &std::path::Path, test: &str, mode: Option<&str>) {
    let mut command = Command::new(env::current_exe().unwrap());
    command
        .args(["--exact", test, "--ignored", "--nocapture"])
        .env("DS_NETWORK_ENGINE_BIN", engine)
        .env_remove("DS_NETWORK_MV_PROJECTION_CASE");
    if let Some(mode) = mode {
        command.env("DS_NETWORK_MV_PROJECTION_CASE", mode);
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
fn acceptance_mv_projection_real_owner() {
    isolated(
        &required("DS_MV_PROJECTION_REAL_ENGINE"),
        "isolated_mv_projection_real_owner",
        None,
    );
}
#[test]
#[ignore = "isolated executable environment"]
fn isolated_mv_projection_real_owner() {
    let model_path = required("DS_MV_PROJECTION_MODEL");
    let model = fs::read(&model_path).unwrap();
    let scratch = tempfile::tempdir_in(required("DS_MV_PROJECTION_SCRATCH")).unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let mut query = request(&model);
    query.json_object_order = captured.identity().json_object_order;
    let result = captured
        .mv_projection(&query, &model, scratch.path(), &|| false)
        .unwrap();
    assert_eq!(
        result.result.source.package_sha256,
        request(&model).source.package_sha256
    );
    assert_eq!(
        serde_json::to_vec(&result.result.projection).unwrap(),
        fs::read(required("DS_MV_PROJECTION_GOLDEN")).unwrap()
    );
    assert_eq!(result.identity.json_object_order, query.json_object_order);
    assert_eq!(result.receipt.artifact.file, MV_PROJECTION_ARTIFACT);
    assert_eq!(
        result.receipt.artifact.sha256,
        format!("{:x}", Sha256::digest(&result.bytes))
    );
    let mut subset = query.clone();
    subset.layers = vec![MvProjectionLayer::Alignments];
    let selected = captured
        .mv_projection(&subset, &model, scratch.path(), &|| false)
        .unwrap();
    assert!(selected.result.projection["features"]
        .as_array()
        .unwrap()
        .iter()
        .all(|feature| feature["properties"]["_layer"] == "alignments"));
    assert_eq!(
        captured
            .mv_projection(&query, &model, scratch.path(), &|| true)
            .unwrap_err()
            .code(),
        "callee_cancelled"
    );
    assert_eq!(fs::read(model_path).unwrap(), model);
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}
#[test]
#[ignore = "requires built adversarial process fixture"]
fn acceptance_mv_projection_rejects_invalid_capability_sources_filters_and_artifacts() {
    let engine = required("DS_MV_PROJECTION_FIXTURE");
    let cases = tempfile::tempdir_in(required("DS_MV_PROJECTION_SCRATCH")).unwrap();
    for mode in [
        "control",
        "missing-capability",
        "wrong-source",
        "wrong-filter",
        "wrong-feature",
        "wrong-revision",
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
            "isolated_mv_projection_invalid_artifacts",
            Some(mode),
        );
    }
}
#[test]
#[ignore = "isolated adversarial executable environment"]
fn isolated_mv_projection_invalid_artifacts() {
    let model = b"opaque prepared test bytes";
    let scratch = tempfile::tempdir_in(required("DS_MV_PROJECTION_SCRATCH")).unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let result = captured.mv_projection(&request(model), model, scratch.path(), &|| false);
    let mode = env::var("DS_NETWORK_MV_PROJECTION_CASE").unwrap();
    let expected = match mode.as_str() {
        "control" => None,
        "missing-capability" => Some("network_engine_incompatible"),
        "escaped-artifact" => Some("network_receipt_invalid"),
        "bad-digest" => Some("network_result_digest_mismatch"),
        _ => Some("network_result_invalid"),
    };
    if let Some(expected) = expected {
        assert_eq!(result.unwrap_err().code(), expected);
    } else {
        assert!(result.is_ok());
    }
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}
