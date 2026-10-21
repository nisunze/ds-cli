//! Explicit native execution proof; ordinary `cargo test` cannot claim these gates.
use ds_network_client::{CapturedNetworkClient, NetworkFailure};
use ds_network_contracts::grid::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, process::Command};

fn required(key: &str) -> PathBuf {
    env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{key} must name an acceptance input"))
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn request(model: &[u8]) -> NativeGridQueryRequestV1 {
    NativeGridQueryRequestV1 {
        schema: NATIVE_GRID_QUERY_SCHEMA_V1.into(),
        source: NativeGridQuerySourcePin {
            package_sha256: sha(model),
            expected_revision: None,
            engineering_input_root: None,
        },
        operation_id: "project_plan".into(),
        params: json!({}),
    }
}
fn isolated(test: &str, engine: &std::path::Path, mode: Option<&str>) {
    let mut command = Command::new(env::current_exe().unwrap());
    command
        .args(["--exact", test, "--ignored", "--nocapture"])
        .env("DS_NETWORK_ENGINE_BIN", engine)
        .env_remove("DS_NETWORK_GRID_CASE");
    if let Some(mode) = mode {
        command.env("DS_NETWORK_GRID_CASE", mode);
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
#[ignore = "requires exact built Network executable and immutable schema-5 owner fixture"]
fn acceptance_grid_read_real_owner() {
    isolated(
        "isolated_grid_read_real_owner",
        &required("DS_NETWORK_GRID_REAL_ENGINE"),
        None,
    );
}

#[test]
#[ignore = "isolated environment for one retained executable selection"]
fn isolated_grid_read_real_owner() {
    let model = fs::read(required("DS_NETWORK_GRID_MODEL")).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let request = ds_network_contracts::grid_read::NativeGridReadRequestV1 {
        schema: ds_network_contracts::grid_read::NATIVE_GRID_READ_REQUEST_SCHEMA_V1.into(),
        issue_limit: 50,
        advisory_limit: 50,
    };
    let result = captured
        .grid_read(&request, &model, scratch.path(), &|| false)
        .unwrap();
    assert!(result.result.container.verified);
    assert_eq!(result.result.package_sha256, sha(&model));
    assert_eq!(result.receipt.artifact.sha256, sha(&result.bytes));
    let cancelled = captured
        .grid_read(&request, &model, scratch.path(), &|| true)
        .unwrap_err();
    assert!(matches!(
        cancelled.code(),
        "callee_cancelled" | "network_task_cancelled"
    ));
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}

#[test]
#[ignore = "requires exact built Network executable and immutable schema-5 owner fixtures"]
fn acceptance_grid_real_owner_artifacts_and_refusals() {
    isolated(
        "isolated_grid_real_owner",
        &required("DS_NETWORK_GRID_REAL_ENGINE"),
        None,
    );
}
#[test]
#[ignore = "isolated environment for one retained executable selection"]
fn isolated_grid_real_owner() {
    let model_path = required("DS_NETWORK_GRID_MODEL");
    let model = fs::read(&model_path).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let mut query = request(&model);
    let result = captured
        .grid_query(&query, &model, scratch.path(), &|| false)
        .unwrap();
    assert_eq!(result.result.operation["operation_id"], "project_plan");
    assert_eq!(result.result.operation["journaled"], false);
    assert_eq!(result.result.source.package_sha256, sha(&model));
    assert_eq!(result.result.model_revision, 0);
    assert!(result.development_override);
    assert_eq!(result.receipt.artifact.sha256, sha(&result.bytes));
    assert_eq!(
        result.receipt.input_sha256,
        sha(&serde_json::to_vec(&query).unwrap())
    );
    let goldens = required("DS_NETWORK_GRID_GOLDENS");
    assert_eq!(
        result.bytes,
        fs::read(goldens.join("query/query.json")).unwrap(),
        "complete project plan bytes must match the owning executable's prior artifact"
    );

    let manifest = captured
        .grid_manifest(&model, scratch.path(), &|| false)
        .unwrap();
    assert!(!manifest.result.decoded);
    assert_eq!(manifest.result.manifest["model"]["schema_version"], 5);
    assert_eq!(manifest.receipt.input_sha256, sha(&model));
    assert_eq!(
        manifest.bytes,
        fs::read(goldens.join("manifest/manifest.json")).unwrap()
    );
    let read = captured
        .grid_read(
            &ds_network_contracts::grid_read::NativeGridReadRequestV1 {
                schema: ds_network_contracts::grid_read::NATIVE_GRID_READ_REQUEST_SCHEMA_V1.into(),
                issue_limit: 50,
                advisory_limit: 50,
            },
            &model,
            scratch.path(),
            &|| false,
        )
        .unwrap();
    assert!(read.result.container.verified);
    assert_eq!(read.result.package_sha256, sha(&model));
    assert!(
        read.result
            .model
            .as_ref()
            .unwrap()
            .authored_revision
            .starts_with("rev:")
    );
    assert_eq!(read.receipt.artifact.sha256, sha(&read.bytes));

    let inspection = captured
        .grid_inspect(&model, scratch.path(), &|| false)
        .unwrap();
    assert_eq!(inspection.result.model_schema, 5);
    assert_eq!(
        inspection.result.source.expected_revision,
        result.result.source.expected_revision
    );
    assert_eq!(inspection.result.source.package_sha256, sha(&model));
    assert_eq!(
        inspection.bytes,
        fs::read(goldens.join("inspection/inspection.json")).unwrap()
    );
    let publication = NativeGridPublicationRequestV1 {
        schema: NATIVE_GRID_PUBLICATION_SCHEMA_V1.into(),
        source: query.source.clone(),
        mode: NativeGridPublicationModeV1::UserPublish,
    };
    let facts = captured
        .grid_publication(&publication, &model, scratch.path(), &|| false)
        .unwrap();
    assert_eq!(facts.result.model_schema, 5);
    assert_eq!(facts.result.manifest_model_revision, 0);
    assert_eq!(
        facts.result.source_revision_id,
        result.result.source.expected_revision
    );
    assert_eq!(
        facts.bytes,
        fs::read(goldens.join("publication/publication.json")).unwrap()
    );

    query.params = json!({"unowned": true});
    let error = captured
        .grid_query(&query, &model, scratch.path(), &|| false)
        .unwrap_err();
    assert_eq!(error.owner_code(), Some("params_invalid"));
    assert_eq!(error.owner_invalid_input, Some(true));
    assert_eq!(
        error.owner_detail.as_ref().unwrap()["unknown"],
        json!(["unowned"])
    );
    assert!(
        error
            .owner_remedy
            .as_ref()
            .unwrap()
            .contains("project_plan")
    );
    query.params = json!({});
    query.source.expected_revision = Some(format!("rev:{}", "0".repeat(64)));
    assert_eq!(
        captured
            .grid_query(&query, &model, scratch.path(), &|| false)
            .unwrap_err()
            .owner_code(),
        Some("stale_revision")
    );
    query.source.expected_revision = None;
    query.source.package_sha256 = "0".repeat(64);
    assert_eq!(
        captured
            .grid_query(&query, &model, scratch.path(), &|| false)
            .unwrap_err()
            .owner_code(),
        Some("grid_task_source_digest_mismatch")
    );
    let error = captured
        .grid_manifest(&model, scratch.path(), &|| true)
        .unwrap_err();
    assert!(matches!(
        error.code(),
        "callee_cancelled" | "network_task_cancelled"
    ));
    assert_eq!(
        fs::read(model_path).unwrap(),
        model,
        "the host's original package must remain untouched"
    );
    assert_eq!(
        fs::read_dir(scratch.path()).unwrap().count(),
        0,
        "all private staging must be retired"
    );
}

#[test]
#[ignore = "requires explicit built adversarial native fixture and schema-5 package"]
fn acceptance_grid_receiver_rejects_unbound_corrupt_and_cancelled_results() {
    let fixture = required("DS_NETWORK_GRID_FIXTURE");
    let directory = tempfile::tempdir().unwrap();
    for (mode, expected) in [
        ("wrong-input", "network_receipt_invalid"),
        ("wrong-file", "network_receipt_invalid"),
        ("wrong-receipt-schema", "network_receipt_invalid"),
        ("wrong-digest", "network_result_digest_mismatch"),
        ("wrong-package", "network_result_invalid"),
        ("wrong-operation", "network_result_invalid"),
        ("wrong-revision", "network_result_invalid"),
        ("changed-input", "network_input_changed"),
        ("cancelled", "callee_cancelled"),
        ("missing-capability", "network_engine_incompatible"),
        ("wrong-progress-input", "network_result_invalid"),
        (
            "inspection-missing-capability",
            "network_engine_incompatible",
        ),
        ("malformed-refusal", "network_refusal_invalid"),
        ("structured-refusal", "network_engine_refused"),
        ("publication-package-revision", "success"),
        ("manifest-wrong-source", "network_result_invalid"),
        ("manifest-decoded", "network_result_invalid"),
        ("read-wrong-package", "network_result_invalid"),
        ("read-wrong-limits", "network_result_invalid"),
        ("read-invalid-counts", "network_result_invalid"),
        ("read-damaged-shape", "network_result_invalid"),
        ("read-wrong-schema", "network_result_invalid"),
        ("read-changed-input", "network_input_changed"),
        ("read-missing-capability", "network_engine_incompatible"),
    ] {
        let name = if cfg!(windows) {
            format!("{mode}.exe")
        } else {
            mode.into()
        };
        let engine = directory.path().join(name);
        fs::copy(&fixture, &engine).unwrap();
        isolated("isolated_grid_adversarial_owner", &engine, Some(expected));
    }
    #[cfg(unix)]
    {
        let engine = directory.path().join("symlink-result");
        fs::copy(&fixture, &engine).unwrap();
        isolated(
            "isolated_grid_adversarial_owner",
            &engine,
            Some("network_result_invalid"),
        );
    }
}
#[test]
#[ignore = "isolated adversarial process fixture case"]
fn isolated_grid_adversarial_owner() {
    let model = fs::read(required("DS_NETWORK_GRID_MODEL")).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let captured = CapturedNetworkClient::capture(1, &|| false).unwrap();
    let mode = captured.executable().file_stem().unwrap().to_str().unwrap();
    let expected = env::var("DS_NETWORK_GRID_CASE").unwrap();
    let mut query = request(&model);
    query.source.expected_revision = Some(format!("rev:{}", "a".repeat(64)));
    let error: NetworkFailure = if mode == "publication-package-revision" {
        let result = captured
            .grid_publication(
                &NativeGridPublicationRequestV1 {
                    schema: NATIVE_GRID_PUBLICATION_SCHEMA_V1.into(),
                    source: query.source,
                    mode: NativeGridPublicationModeV1::Inspect,
                },
                &model,
                scratch.path(),
                &|| false,
            )
            .unwrap();
        assert_ne!(
            result.result.source_revision_id, result.result.source.expected_revision,
            "package/library identity is distinct from authored revision"
        );
        assert_eq!(
            result.result.source_revision_id,
            format!("rev:{}", sha(&model))
        );
        assert_eq!(expected, "success");
        return;
    } else if mode.starts_with("read-") {
        captured
            .grid_read(
                &ds_network_contracts::grid_read::NativeGridReadRequestV1 {
                    schema: ds_network_contracts::grid_read::NATIVE_GRID_READ_REQUEST_SCHEMA_V1
                        .into(),
                    issue_limit: 50,
                    advisory_limit: 50,
                },
                &model,
                scratch.path(),
                &|| false,
            )
            .unwrap_err()
    } else if mode.starts_with("inspection-") {
        captured
            .grid_inspect(&model, scratch.path(), &|| false)
            .unwrap_err()
    } else if mode.starts_with("manifest-") {
        captured
            .grid_manifest(&model, scratch.path(), &|| false)
            .unwrap_err()
    } else {
        captured
            .grid_query(&query, &model, scratch.path(), &|| false)
            .unwrap_err()
    };
    assert_eq!(error.code(), expected, "{mode}: {error:?}");
    if mode == "structured-refusal" {
        assert_eq!(error.owner_code(), Some("precise_grid_refusal"));
        assert_eq!(
            error.owner_detail.as_deref(),
            Some(&json!({"constraint": "retained"}))
        );
        assert_eq!(
            error.owner_remedy.as_deref(),
            Some("use the exact owner parameters")
        );
        assert_eq!(error.owner_invalid_input, Some(false));
    }
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}
