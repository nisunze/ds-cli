//! Captured executable acceptance, including a receipt-correct forged operation.
use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::{grid::NativeGridQuerySourcePin, grid_write_admission::*};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn request(bytes: &[u8]) -> NativeGridWriteAdmissionRequestV1 {
    NativeGridWriteAdmissionRequestV1 {
        schema: GRID_WRITE_ADMISSION_SCHEMA_V1.into(),
        source: NativeGridQuerySourcePin {
            package_sha256: format!("{:x}", Sha256::digest(bytes)),
            expected_revision: None,
            engineering_input_root: None,
        },
        operation: "delete".into(),
    }
}

#[test]
#[ignore = "requires selected exact native executable and canonical write-guard fixtures"]
fn captured_guard_fences_source_refusal_and_cleanup() {
    let root = PathBuf::from(env::var_os("DS_NETWORK_WRITE_FIXTURES").unwrap());
    let scratch = root.join("client-scratch");
    fs::create_dir_all(&scratch).unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let ordinary = fs::read(root.join("ordinary.dsgrid")).unwrap();
    let mut request = request(&ordinary);
    let outcome = captured
        .grid_write_admission(&request, &ordinary, &scratch, &|| false)
        .unwrap();
    assert_eq!(outcome.result.operation, "delete");
    fs::write(root.join("ordinary-admission.json"), &outcome.bytes).unwrap();
    request.source.expected_revision = Some(outcome.result.source.expected_revision.clone());
    request.source.engineering_input_root =
        Some(outcome.result.source.engineering_input_root.clone());
    assert!(captured
        .grid_write_admission(&request, &ordinary, &scratch, &|| false)
        .is_ok());
    request.source.expected_revision = Some(format!("rev:{}", "0".repeat(64)));
    assert_eq!(
        captured
            .grid_write_admission(&request, &ordinary, &scratch, &|| false)
            .unwrap_err()
            .owner_code(),
        Some("stale_revision")
    );
    let protected = fs::read(root.join("protected.dsgrid")).unwrap();
    let expected: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("protected-delete.json")).unwrap()).unwrap();
    let outcome = captured
        .grid_write_admission(&crate::request(&protected), &protected, &scratch, &|| false)
        .unwrap();
    match outcome.result.verdict {
        NativeGridWriteAdmissionVerdictV1::Denied { code, message } => {
            assert_eq!(code, "composite_model_protected");
            assert_eq!(message, expected["message"].as_str().unwrap());
        }
        NativeGridWriteAdmissionVerdictV1::Admitted => panic!("protected model admitted"),
    }
    assert!(captured
        .grid_write_admission(&crate::request(&ordinary), &ordinary, &scratch, &|| true)
        .is_err());
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
    assert_eq!(fs::read(root.join("ordinary.dsgrid")).unwrap(), ordinary);
    assert_eq!(fs::read(root.join("protected.dsgrid")).unwrap(), protected);
}

#[test]
#[ignore = "requires receipt-correct operation-forging native test adapter"]
fn captured_guard_rejects_receipt_correct_forged_facts() {
    let root = PathBuf::from(env::var_os("DS_NETWORK_WRITE_FIXTURES").unwrap());
    let scratch = root.join("hostile-scratch");
    fs::create_dir_all(&scratch).unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let bytes = fs::read(root.join("ordinary.dsgrid")).unwrap();
    let valid: NativeGridWriteAdmissionResultV1 =
        serde_json::from_slice(&fs::read(root.join("ordinary-admission.json")).unwrap()).unwrap();
    let mut pinned = request(&bytes);
    pinned.source.expected_revision = Some(valid.source.expected_revision);
    pinned.source.engineering_input_root = Some(valid.source.engineering_input_root);
    assert_eq!(
        captured
            .grid_write_admission(&pinned, &bytes, &scratch, &|| false)
            .unwrap_err()
            .code(),
        "network_result_invalid"
    );
    assert_eq!(fs::read_dir(&scratch).unwrap().count(), 0);
}
