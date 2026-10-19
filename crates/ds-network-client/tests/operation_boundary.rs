//! Real consumer and adversarial admission for closed Grid file operations.
use ds_network_client::CapturedNetworkClient;
use ds_network_contracts::grid_operation::*;
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, process::Command};

fn required(key: &str) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{key} must name an acceptance input"))
}

fn describe(structure: &str, write: bool) -> GridOperationV1 {
    GridOperationV1::TypedMutation(TypedMutationParamsV1 {
        verb: TypedMutationVerbV1::StructureDescribe {
            structure: structure.into(),
            description: Some("restored process port".into()),
        },
        expected_revision: None,
        write,
    })
}

fn isolated(test: &str, engine: &std::path::Path, mode: Option<(&str, &str)>) {
    let mut command = Command::new(env::current_exe().unwrap());
    command
        .args(["--exact", test, "--ignored", "--nocapture"])
        .env("DS_NETWORK_ENGINE_BIN", engine)
        .env_remove("DS_NETWORK_OPERATION_MODE")
        .env_remove("DS_NETWORK_OPERATION_EXPECTED");
    if let Some((mode, expected)) = mode {
        command
            .env("DS_NETWORK_OPERATION_MODE", mode)
            .env("DS_NETWORK_OPERATION_EXPECTED", expected);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{test} {mode:?}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires the protocol-only hostile fixture executable"]
fn adversarial_operation_artifacts_cannot_escape_admission() {
    let fixture = required("DS_NETWORK_OPERATION_PROTOCOL_FIXTURE");
    let scratch = tempfile::tempdir().unwrap();
    for (mode, expected) in [
        ("missing-capability", "network_engine_incompatible"),
        ("ok", "ok"),
        ("refusal", "structure_unknown"),
        ("wrong-operation", "network_result_invalid"),
        ("wrong-package", "network_result_invalid"),
        ("artifact-digest", "network_result_digest_mismatch"),
        ("missing-artifact", "network_result_invalid"),
        ("bad-name", "network_result_invalid"),
        ("extra-file", "network_result_invalid"),
        ("receipt-input", "network_receipt_invalid"),
    ] {
        let exe = scratch.path().join(mode);
        fs::copy(&fixture, &exe).unwrap();
        isolated("isolated_operation_adversary", &exe, Some((mode, expected)));
    }
}

#[test]
#[ignore = "isolated executable selection"]
fn isolated_operation_adversary() {
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let model = b"opaque model".as_slice();
    let inputs = vec![("receipt.json".to_owned(), b"{}".to_vec())];
    let mode = env::var("DS_NETWORK_OPERATION_MODE").unwrap();
    let expected = env::var("DS_NETWORK_OPERATION_EXPECTED").unwrap();
    let outcome = captured.grid_operation(
        &describe("x", true),
        Some(model),
        &inputs,
        scratch.path(),
        &|| false,
    );
    match outcome {
        Ok(mut outcome) => {
            assert_eq!(expected, "ok", "{mode}");
            assert_eq!(outcome.result.operation, "typed_mutation");
            assert_eq!(
                outcome.result.package_sha256.as_deref(),
                Some(format!("{:x}", Sha256::digest(model)).as_str())
            );
            assert_eq!(
                outcome.take_artifact("model.dsgrid").unwrap(),
                b"complete opaque operation model"
            );
            assert!(outcome.artifacts.is_empty());
        }
        Err(failure) => {
            assert_eq!(
                failure.owner_code().unwrap_or(failure.code()),
                expected,
                "{mode}: {failure}"
            );
            if mode == "refusal" {
                let detail = failure.owner_detail.as_deref().unwrap();
                let envelope: GridOperationRefusalV1 =
                    serde_json::from_value(detail.clone()).unwrap();
                assert_eq!(envelope.class, GridOperationRefusalClassV1::Invalid);
                assert_eq!(envelope.next, ["ds dsgrid report structures"]);
                assert_eq!(envelope.detail.unwrap()["structures"], 3);
                assert_eq!(failure.owner_invalid_input, Some(true));
            }
        }
    }
    assert_eq!(
        fs::read_dir(scratch.path()).unwrap().count(),
        0,
        "{mode}: private staging is always removed"
    );
}

#[test]
#[ignore = "requires the exact built Network executable and a schema-5 model"]
fn acceptance_real_operations_round_trip_exact_artifacts() {
    isolated(
        "isolated_real_operations",
        &required("DS_NETWORK_OPERATION_REAL_ENGINE"),
        None,
    );
}

#[test]
#[ignore = "isolated environment for one retained executable selection"]
fn isolated_real_operations() {
    let model = fs::read(required("DS_NETWORK_OPERATION_MODEL")).unwrap();
    let structure = env::var("DS_NETWORK_OPERATION_STRUCTURE").unwrap_or_else(|_| "1".into());
    let scratch = tempfile::tempdir().unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let preview = captured
        .grid_operation(
            &describe(&structure, false),
            Some(&model),
            &[],
            scratch.path(),
            &|| false,
        )
        .unwrap();
    let typed: TypedMutationResultV1 =
        serde_json::from_value(preview.result.result.clone()).unwrap();
    assert!(typed.planned);
    assert!(preview.artifacts.is_empty());
    let mut written = captured
        .grid_operation(
            &describe(&structure, true),
            Some(&model),
            &[],
            scratch.path(),
            &|| false,
        )
        .unwrap();
    let typed: TypedMutationResultV1 =
        serde_json::from_value(written.result.result.clone()).unwrap();
    assert_eq!(
        typed.receipt["resulting_revision"],
        typed.resulting_revision
    );
    assert!(written
        .take_artifact("model.dsgrid")
        .is_some_and(|bytes| !bytes.is_empty()));
    let report = captured
        .grid_operation(
            &GridOperationV1::ReportStructures(ReportStructuresParamsV1 {
                only_findings: false,
                format: Some("csv".into()),
            }),
            Some(&model),
            &[],
            scratch.path(),
            &|| false,
        )
        .unwrap();
    assert!(report.artifacts.contains_key("structures.csv"));
    let cancelled = captured
        .grid_operation(
            &describe(&structure, false),
            Some(&model),
            &[],
            scratch.path(),
            &|| true,
        )
        .unwrap_err();
    assert!(matches!(
        cancelled.code(),
        "callee_cancelled" | "network_task_cancelled"
    ));
    // A narrower engine claim cannot stage work for another capability.
    let without_model = captured
        .grid_operation(
            &GridOperationV1::CriteriaShow,
            None,
            &[],
            scratch.path(),
            &|| false,
        )
        .unwrap_err();
    assert_eq!(without_model.code(), "grid_task_input_invalid");
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}
