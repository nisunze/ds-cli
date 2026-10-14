//! Actual child execution is explicit; source tests never qualify installation.
use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::lv_processing::*;
use std::{env, fs, path::PathBuf, process::Command};
fn required(key: &str) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{key} must name the retained actual executable/fixture"))
}
fn input() -> (Vec<u8>, Vec<u8>) {
    (
        fs::read(required("DS_NETWORK_SAVE_INPUT")).unwrap(),
        fs::read(required("DS_NETWORK_SAVE_RESULT")).unwrap(),
    )
}
fn validation(publication: NativeLvPublication) -> NativeLvAnalysisValidationRequestV1 {
    NativeLvAnalysisValidationRequestV1 {
        schema: NATIVE_LV_ANALYSIS_VALIDATION_SCHEMA_V1.into(),
        jobs: vec![NativeLvAnalysisValidationJobV1 {
            source: publication.process_input,
            gdfs: publication.gdfs.into_iter().collect(),
            settings_used: serde_json::from_value(publication.process_metadata["settings"].clone())
                .unwrap(),
            analysis: publication.voltage_drop,
        }],
    }
}
#[test]
#[ignore = "requires an immutable actual Network executable and exact historical inputs"]
fn acceptance_saved_lv_real_owner() {
    let (input, result) = input();
    let stage = tempfile::tempdir().unwrap();
    let engine = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let published = engine
        .project_lv(&input, &result, "T_approved", stage.path(), &|| false)
        .unwrap();
    assert_eq!(
        published.bytes,
        fs::read(required("DS_NETWORK_SAVE_PUBLICATION")).unwrap()
    );
    let mut request = validation(published.result);
    let validated = engine
        .validate_lv_analysis(&request, stage.path(), &|| false)
        .unwrap();
    assert_eq!(validated.result.transformer_names, ["T_approved"]);
    request.jobs[0].analysis.processed_digest = "0".repeat(64);
    let refused = engine
        .validate_lv_analysis(&request, stage.path(), &|| false)
        .unwrap_err();
    assert_eq!(refused.code(), "fast_lv_publication_invalid");
    assert_eq!(fs::read_dir(stage.path()).unwrap().count(), 0);
}
#[test]
#[ignore = "requires a compiled wire fixture; this test invokes all adversarial children"]
fn acceptance_saved_lv_adversaries() {
    let stage = tempfile::tempdir().unwrap();
    for mode in [
        "valid",
        "digest",
        "identity",
        "extra",
        "symlink",
        "input-changed",
        "cancelled",
        "retained",
        "source",
        "names",
        "oldcap",
    ] {
        let dir = stage.path().join(mode);
        fs::create_dir(&dir).unwrap();
        let engine = dir.join(format!("lv-save-{mode}"));
        fs::copy(required("DS_NETWORK_SAVE_ADVERSARIAL"), &engine).unwrap();
        let o = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "isolated_saved_lv_adversary",
                "--ignored",
                "--nocapture",
            ])
            .env("DS_NETWORK_ENGINE_BIN", engine)
            .env("DS_NETWORK_SAVE_CASE", mode)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "{mode}: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
    }
}
#[test]
#[ignore = "one selected wire fixture environment"]
fn isolated_saved_lv_adversary() {
    let (input, result) = input();
    let mode = env::var("DS_NETWORK_SAVE_CASE").unwrap();
    let stage = tempfile::tempdir().unwrap();
    let engine = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let request: NativeLvAnalysisValidationRequestV1 =
        serde_json::from_slice(&fs::read(required("DS_NETWORK_SAVE_VALIDATION")).unwrap()).unwrap();
    let validation_mode = matches!(mode.as_str(), "names" | "oldcap");
    let o = if validation_mode {
        engine
            .validate_lv_analysis(&request, stage.path(), &|| false)
            .map(|_| ())
    } else {
        engine
            .project_lv(&input, &result, "T_approved", stage.path(), &|| false)
            .map(|_| ())
    };
    if mode == "valid" {
        o.unwrap();
    } else {
        let e = o.unwrap_err();
        assert_eq!(
            e.code(),
            match mode.as_str() {
                "digest" => "network_result_digest_mismatch",
                "identity" | "retained" => "network_receipt_invalid",
                "extra" | "symlink" | "source" | "names" => "network_result_invalid",
                "input-changed" => "network_input_changed",
                "cancelled" => "callee_cancelled",
                "oldcap" => "network_engine_incompatible",
                _ => panic!("unknownmode"),
            }
        );
    }
    assert_eq!(fs::read_dir(stage.path()).unwrap().count(), 0);
    let calls = fs::read_to_string(engine.executable().parent().unwrap().join("calls")).unwrap();
    assert_eq!(
        calls.lines().filter(|line| *line == "build-info").count(),
        1
    );
    assert_eq!(calls.lines().count(), if mode == "oldcap" { 1 } else { 2 });
}
