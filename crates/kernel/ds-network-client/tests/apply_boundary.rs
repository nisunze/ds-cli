//! Real prepared mutation acceptance; no algorithm or package decoder in this host.
use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::grid_apply::*;
use serde_json::Value;
use std::{env, fs, path::PathBuf, process::Command};
#[test]
#[ignore = "requires the exact native Network owner and immutable fixture corpus"]
fn acceptance_grid_apply_preserves_package_and_refusal_fences() {
    let result = Command::new(env::current_exe().unwrap())
        .args([
            "--exact",
            "isolated_grid_apply_owner",
            "--ignored",
            "--nocapture",
        ])
        .env(
            "DS_NETWORK_ENGINE_BIN",
            env::var_os("DS_NETWORK_APPLY_ENGINE").expect("native owner"),
        )
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
#[test]
#[ignore = "isolated executable selection"]
fn isolated_grid_apply_owner() {
    let model = fs::read(PathBuf::from(
        env::var_os("DS_NETWORK_APPLY_MODEL").expect("model fixture"),
    ))
    .unwrap();
    let fixtures =
        PathBuf::from(env::var_os("DS_NETWORK_APPLY_FIXTURES").expect("mutation fixtures"));
    let scratch = tempfile::tempdir().unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let inspection = captured
        .grid_inspect(&model, scratch.path(), &|| false)
        .unwrap();
    let request = NativeGridApplyRequestV1 {
        schema: GRID_APPLY_REQUEST_SCHEMA_V1.into(),
        source: inspection.result.source,
        envelope: serde_json::from_slice(&fs::read(fixtures.join("envelope.json")).unwrap())
            .unwrap(),
        dry_run: true,
    };
    for dry_run in [true, false] {
        let request = NativeGridApplyRequestV1 {
            dry_run,
            ..request.clone()
        };
        let outcome = captured
            .grid_apply(&request, &model, scratch.path(), &|| false)
            .unwrap();
        let expected: Value = serde_json::from_slice(
            &fs::read(fixtures.join(if dry_run {
                "preview-projection.json"
            } else {
                "apply-projection.json"
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(outcome.metadata.result.projection, expected);
        assert_eq!(
            outcome.metadata.result.journal["entries"]
                .as_array()
                .unwrap()
                .len(),
            usize::from(!dry_run)
        );
        if dry_run {
            assert!(outcome.model.is_none());
        } else {
            assert_eq!(
                outcome.model.as_ref().unwrap(),
                &fs::read(fixtures.join("model-applied.dsgrid")).unwrap()
            );
        }
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
    }
    let mut stale = request.clone();
    stale.envelope["expected_revision"] = serde_json::json!(format!("rev:{}", "0".repeat(64)));
    let error = captured
        .grid_apply(&stale, &model, scratch.path(), &|| false)
        .unwrap_err();
    assert_eq!(error.owner_code(), Some("revision_conflict"));
    assert!(error.owner_detail.as_ref().unwrap()["actual_revision"]
        .as_str()
        .unwrap()
        .starts_with("rev:"));
    assert!(captured
        .grid_apply(&request, &model, scratch.path(), &|| true)
        .is_err());
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}

#[test]
#[ignore = "requires the built protocol-only adversarial fixture"]
fn acceptance_grid_apply_rejects_unbound_writable_artifacts() {
    let fixture = PathBuf::from(env::var_os("DS_NETWORK_GRID_FIXTURE").expect("protocol fixture"));
    let scratch = tempfile::tempdir().unwrap();
    for (mode, expected) in [
        ("apply-missing-capability", "network_engine_incompatible"),
        ("apply-wrong-source", "network_result_invalid"),
        ("apply-wrong-projection", "network_result_invalid"),
        ("apply-model-digest", "network_result_digest_mismatch"),
        ("apply-missing-model", "network_result_invalid"),
        ("apply-extra-file", "network_result_invalid"),
        ("apply-symlink-model", "network_result_invalid"),
        ("apply-preview-model", "network_result_invalid"),
    ] {
        if mode == "apply-symlink-model" && !cfg!(unix) {
            continue;
        }
        let executable = scratch.path().join(mode);
        fs::copy(&fixture, &executable).unwrap();
        let result = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "isolated_grid_apply_adversary",
                "--ignored",
                "--nocapture",
            ])
            .env("DS_NETWORK_ENGINE_BIN", executable)
            .env("DS_NETWORK_APPLY_MODE", mode)
            .env("DS_NETWORK_APPLY_EXPECTED", expected)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{mode}: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
#[test]
#[ignore = "isolated protocol fixture selection"]
fn isolated_grid_apply_adversary() {
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let model = b"opaque input";
    use sha2::{Digest, Sha256};
    let request = NativeGridApplyRequestV1 {
        schema: GRID_APPLY_REQUEST_SCHEMA_V1.into(),
        source: serde_json::from_value(serde_json::json!({
            "package_sha256": format!("{:x}", Sha256::digest(model)), "model_id": "fixture-model",
            "expected_revision": format!("rev:{}", "a".repeat(64)), "engineering_input_root": format!("fnv1a64:{}", "b".repeat(16)),
        })).unwrap(),
        envelope: serde_json::json!({"command_id":"intent","command":{"command_kind":"existing_owner_operation"}}),
        dry_run: env::var("DS_NETWORK_APPLY_MODE").unwrap() == "apply-preview-model",
    };
    assert_eq!(
        captured
            .grid_apply(&request, model, scratch.path(), &|| false)
            .unwrap_err()
            .code(),
        env::var("DS_NETWORK_APPLY_EXPECTED").unwrap()
    );
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}
