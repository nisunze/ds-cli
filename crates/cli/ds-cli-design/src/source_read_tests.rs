use super::*;

#[test]
fn a_geopackage_with_a_live_journal_is_refused_before_anything_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("t001.gpkg");
    std::fs::write(&source, b"SQLite format 3\0").unwrap();
    std::fs::write(dir.path().join("t001.gpkg-wal"), b"live").unwrap();
    let error = closed_geopackage(&source, 1024).unwrap_err();
    assert!(error.contains("live SQLite journal sidecar"), "{error}");
    assert_eq!(
        std::fs::read(dir.path().join("t001.gpkg-wal")).unwrap(),
        b"live"
    );
}

#[test]
fn a_geopackage_is_bounded_and_read_whole_once_admitted() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("t001.gpkg");
    std::fs::write(&source, vec![7u8; 64]).unwrap();
    assert!(
        closed_geopackage(&source, 63)
            .unwrap_err()
            .contains("within 63 bytes")
    );
    assert_eq!(closed_geopackage(&source, 64).unwrap(), vec![7u8; 64]);
    assert!(
        closed_geopackage(&dir.path().join("absent.gpkg"), 64)
            .unwrap_err()
            .contains("cannot open GeoPackage source")
    );
}

fn source_error(code: &'static str) -> NetworkFailure {
    NetworkFailure {
        code,
        message: "exact original cause".into(),
        owner_error: None,
        owner_code: None,
        owner_detail: None,
        owner_remedy: None,
        owner_invalid_input: None,
    }
}
fn projected(error: NetworkFailure) -> Failure {
    match engine_failure(error) {
        ReadFailure::Engine(failure) => failure,
        ReadFailure::Source(_) => panic!("expected engine refusal"),
    }
}

#[test]
fn every_known_source_engine_code_preserves_its_original_class_and_message() {
    use ds_cli_contract::ExitClass;
    // Golden vocabulary of the previous adapter, including causes accepted by
    // the shared refusal reader even when no current GIS producer emits them.
    const LEGACY: &[&str] = &[
        "network_engine_missing",
        "network_engine_incompatible",
        "network_engine_path_invalid",
        "network_engine_invalid",
        "network_engine_unqualified",
        "network_engine_identity_invalid",
        "network_engine_changed",
        "network_engine_refused",
        "network_workers_invalid",
        "network_serialization_incompatible",
        "network_scratch_invalid",
        "network_request_invalid",
        "network_request_io",
        "grid_task_bound_refused",
        "network_receipt_bounded",
        "network_receipt_invalid",
        "network_refusal_invalid",
        "network_result_invalid",
        "network_result_digest_mismatch",
        "network_input_changed",
        "callee_cancelled",
        "callee_timed_out",
        "callee_wait_failed",
        "callee_supervision_failed",
        "network_task_cancelled",
        "network_control_unavailable",
        "network_control_invalid",
        "network_input_invalid",
        "network_output_invalid",
        "network_artifact_io",
        "callee_stdin_failed",
        "fast_lv_input_too_large",
        "fast_lv_input_invalid",
        "fast_lv_schema_unsupported",
        "fast_lv_bound_refused",
        "fast_lv_output_too_large",
        "fast_lv_result_encoding_failed",
        "fast_lv_publication_invalid",
        "design_snapshot_processing_failed",
    ];
    for &code in LEGACY {
        let failure = projected(source_error(code));
        assert_eq!(failure.code(), code);
        let class = if matches!(
            code,
            "network_engine_missing"
                | "network_engine_incompatible"
                | "network_serialization_incompatible"
        ) {
            ExitClass::Unavailable
        } else {
            ExitClass::Failed
        };
        assert_eq!(failure.class(), class, "{code}");
        assert_eq!(failure.message(), "exact original cause");
        assert!(failure.detail_value().is_none());
        assert!(
            REFUSALS.iter().any(|row| row.code == code),
            "undeclared legacy {code}"
        );
    }
    for owner_code in [None, Some("module_removed".into())] {
        let mut error = source_error("module_removed");
        error.owner_code = owner_code;
        let removed = projected(error);
        assert_eq!(removed.code(), "network_engine_missing");
        assert_eq!(removed.class(), ExitClass::Unavailable);
    }
}

#[test]
fn unknown_source_engine_codes_keep_a_bounded_original_cause() {
    for owner in [None, Some("future_owner_refusal".to_string())] {
        let expected = owner
            .as_deref()
            .unwrap_or("future_host_refusal")
            .to_string();
        let mut error = source_error(if owner.is_some() {
            "network_engine_refused"
        } else {
            "future_host_refusal"
        });
        error.owner_code = owner;
        let failure = projected(error);
        assert_eq!(failure.code(), "network_engine_refusal_unknown");
        assert_eq!(failure.class(), ds_cli_contract::ExitClass::Failed);
        let detail = failure.detail_value().unwrap();
        assert_eq!(detail["engine_code"], expected);
        assert_eq!(detail["cause"], "exact original cause");
        assert_eq!(detail["code_truncated"], false);
        assert_eq!(detail["cause_truncated"], false);
    }
    let mut error = source_error("network_engine_refused");
    error.owner_code = Some("é".repeat(80));
    error.message = "é".repeat(2500);
    let failure = projected(error);
    let detail = failure.detail_value().unwrap();
    assert_eq!(detail["engine_code"].as_str().unwrap().len(), 128);
    assert_eq!(detail["cause"].as_str().unwrap().len(), 4096);
    assert_eq!(detail["code_truncated"], true);
    assert_eq!(detail["cause_truncated"], true);
    let mut error = source_error("network_engine_refused");
    error.owner_code = Some(format!("x{}", "€".repeat(80)));
    error.message = format!("xy{}", "€".repeat(2500));
    let failure = projected(error);
    let detail = failure.detail_value().unwrap();
    assert_eq!(detail["engine_code"].as_str().unwrap().len(), 127);
    assert_eq!(detail["cause"].as_str().unwrap().len(), 4094);
    assert_eq!(detail["code_truncated"], true);
    assert_eq!(detail["cause_truncated"], true);
}

#[test]
fn source_reader_refusals_and_known_owner_codes_keep_their_existing_projection() {
    let mut error = source_error("network_engine_refused");
    error.owner_code = Some(ds_network_client::SOURCE_UNREADABLE.into());
    assert!(
        matches!(engine_failure(error), ReadFailure::Source(message) if message == "exact original cause")
    );
    let mut error = source_error("network_engine_refused");
    error.owner_code = Some("grid_task_bound_refused".into());
    let failure = projected(error);
    assert_eq!(failure.code(), "network_engine_refused");
    assert_eq!(failure.message(), "exact original cause");
}

#[test]
fn every_gis_consumer_declares_the_complete_source_engine_vocabulary_once() {
    for command in [
        &crate::history_import::PREPARE,
        &crate::intake_upload::COMMAND,
    ] {
        let mut codes = std::collections::BTreeSet::new();
        for refusal in command.refusals {
            assert!(
                codes.insert(refusal.code),
                "{} duplicates {}",
                command.id,
                refusal.code
            );
        }
        for row in ds_network_client::SOURCE_ENGINE_REFUSALS {
            assert!(
                codes.contains(row.code),
                "{} misses {}",
                command.id,
                row.code
            );
        }
    }
    assert!(
        !crate::history_import::PUBLISH
            .refusals
            .iter()
            .any(|row| row.code == "network_engine_refusal_unknown")
    );
}
