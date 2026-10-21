//! Real consumer and adversarial admission for complete Grid batch artifacts.
use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::grid_apply_batch::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, process::Command};
fn fixture(name: &str) -> Vec<u8> {
    fs::read(PathBuf::from(env::var_os("DS_NETWORK_BATCH_FIXTURES").unwrap()).join(name)).unwrap()
}
#[test]
#[ignore = "requires exact native Network executable and retained byte corpus"]
fn real_batch_and_correction_preserve_complete_bytes_and_atomic_refusals() {
    let model = fs::read(env::var_os("DS_NETWORK_BATCH_MODEL").unwrap()).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let pin = captured
        .grid_inspect(&model, scratch.path(), &|| false)
        .unwrap()
        .result
        .source;
    for correction in [false, true] {
        for dry_run in [true, false] {
            let name = if correction {
                "correction.json"
            } else {
                "batch.json"
            };
            let batch: Value = serde_json::from_slice(&fixture(name)).unwrap();
            let guard = correction.then(|| {
                let mut doc: Value = serde_json::from_slice(&fixture("guard.json")).unwrap();
                for (key, value) in [
                    ("preservation_source_ref", Value::Null),
                    ("level", json!("alignment")),
                    ("intervals", json!([])),
                    ("preserved_structure_ids", json!([])),
                    ("preserved_route_node_ids", json!([])),
                    ("preserved_section_ids", json!([])),
                ] {
                    doc[key] = value;
                }
                NativeGridCorrectionGuardV1 {
                    sha256: format!("sha256:{:x}", Sha256::digest(fixture("guard.json"))),
                    document: doc,
                }
            });
            let mut request = NativeGridApplyBatchRequestV1 {
                schema: GRID_APPLY_BATCH_REQUEST_SCHEMA_V1.into(),
                source: pin.clone(),
                expected_revision: batch["expected_revision"].as_str().unwrap().into(),
                batch_sha256: format!("sha256:{:x}", Sha256::digest(fixture(name))),
                commands: batch["commands"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|row| NativeGridBatchCommandV1 {
                        command_id: row["command_id"].as_str().unwrap().into(),
                        command: row["command"].clone(),
                        review_ref: row["review_ref"].as_str().map(str::to_owned),
                    })
                    .collect(),
                guard,
                dry_run,
            };
            let output = captured
                .grid_apply_batch(&request, &model, scratch.path(), &|| false)
                .unwrap();
            let expected: Value = serde_json::from_slice(&fixture(match (correction, dry_run) {
                (false, true) => "preview.json",
                (false, false) => "apply.json",
                (true, true) => "correction-preview.json",
                (true, false) => "correction-apply.json",
            }))
            .unwrap();
            assert_eq!(output.metadata.result.projection, expected);
            if dry_run {
                assert!(output.model.is_none());
            } else {
                assert_eq!(
                    output.model.unwrap(),
                    fixture(if correction {
                        "correction-applied.dsgrid"
                    } else {
                        "batch-applied.dsgrid"
                    })
                );
            }
            request.expected_revision = format!("rev:{}", "0".repeat(64));
            let error = captured
                .grid_apply_batch(&request, &model, scratch.path(), &|| false)
                .unwrap_err();
            assert_eq!(error.owner_code(), Some("revision_conflict"));
            assert!(captured
                .grid_apply_batch(&request, &model, scratch.path(), &|| true)
                .is_err());
            assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
        }
    }
}
#[test]
#[ignore = "requires protocol-only hostile fixture executable"]
fn adversarial_batch_artifacts_cannot_escape_complete_admission() {
    let fixture = PathBuf::from(env::var_os("DS_NETWORK_BATCH_PROTOCOL_FIXTURE").unwrap());
    let scratch = tempfile::tempdir().unwrap();
    for (mode, expected) in [
        ("missing-capability", "network_engine_incompatible"),
        ("wrong-source", "network_result_invalid"),
        ("reversed-ids", "network_result_invalid"),
        ("wrong-digest", "network_result_invalid"),
        ("wrong-guard", "network_result_invalid"),
        ("model-digest", "network_result_digest_mismatch"),
        ("missing-model", "network_result_invalid"),
        ("extra-file", "network_result_invalid"),
        ("symlink-model", "network_result_invalid"),
        ("preview-model", "network_result_invalid"),
    ] {
        if mode == "symlink-model" && !cfg!(unix) {
            continue;
        }
        let exe = scratch.path().join(mode);
        fs::copy(&fixture, &exe).unwrap();
        let output = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "isolated_batch_adversary",
                "--ignored",
                "--nocapture",
            ])
            .env("DS_NETWORK_ENGINE_BIN", exe)
            .env("DS_NETWORK_BATCH_MODE", mode)
            .env("DS_NETWORK_BATCH_EXPECTED", expected)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{mode}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
#[test]
#[ignore = "isolated executable selection"]
fn isolated_batch_adversary() {
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let model = b"opaque model";
    let request=NativeGridApplyBatchRequestV1{schema:GRID_APPLY_BATCH_REQUEST_SCHEMA_V1.into(),source:serde_json::from_value(json!({"package_sha256":format!("{:x}",Sha256::digest(model)),"model_id":"fixture-model","expected_revision":format!("rev:{}","a".repeat(64)),"engineering_input_root":format!("fnv1a64:{}","b".repeat(16))})).unwrap(),expected_revision:format!("rev:{}","a".repeat(64)),batch_sha256:format!("sha256:{}","d".repeat(64)),commands:["first","second"].map(|id|NativeGridBatchCommandV1{command_id:id.into(),command:json!({"command_kind":"existing_owner_operation"}),review_ref:Some("review".into())}).to_vec(),guard:Some(NativeGridCorrectionGuardV1{sha256:format!("sha256:{}","e".repeat(64)),document:json!({"source_sha256":format!("sha256:{:x}",Sha256::digest(model)),"alignment_ids":["alignment-1"]})}),dry_run:env::var("DS_NETWORK_BATCH_MODE").unwrap()=="preview-model"};
    assert_eq!(
        captured
            .grid_apply_batch(&request, model, scratch.path(), &|| false)
            .unwrap_err()
            .code(),
        env::var("DS_NETWORK_BATCH_EXPECTED").unwrap()
    );
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}
