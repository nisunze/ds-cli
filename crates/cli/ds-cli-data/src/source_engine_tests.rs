use super::*;
use std::io::Read;

#[test]
fn a_geometry_source_names_its_reader_by_extension() {
    assert_eq!(geometry_reader("a.KMZ"), SourceReaderV1::Kmz);
    assert_eq!(geometry_reader("poles.shp.zip"), SourceReaderV1::Archive);
    assert_eq!(geometry_reader("a.kml"), SourceReaderV1::Kml);
    assert_eq!(geometry_reader("a.geojson"), SourceReaderV1::Geojson);
    assert_eq!(geometry_reader("a.json"), SourceReaderV1::Geojson);
}

#[test]
fn a_bare_shapefile_packs_its_sidecars_deterministically() {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in [
        ("Poles.shp", "shp"),
        ("poles.DBF", "dbf"),
        ("poles.shx", "shx"),
        ("poles.prj", "prj"),
        ("poles.txt", "txt"),
        ("other.dbf", "other"),
    ] {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    let shp = dir.path().join("Poles.shp");
    let first = shapefile_with_sidecars_to_zip(&shp).unwrap();
    assert_eq!(first, shapefile_with_sidecars_to_zip(&shp).unwrap());
    let mut archive = zip::ZipArchive::new(Cursor::new(first)).unwrap();
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).unwrap();
        let mut body = String::new();
        member.read_to_string(&mut body).unwrap();
        names.push((member.name().to_string(), body));
    }
    assert_eq!(
        names,
        [
            ("Poles.shp".to_string(), "shp".to_string()),
            ("Poles.dbf".to_string(), "dbf".to_string()),
            ("Poles.prj".to_string(), "prj".to_string()),
            ("Poles.shx".to_string(), "shx".to_string()),
        ]
    );
    assert!(shapefile_with_sidecars_to_zip(&dir.path().join("absent.shp")).is_err());
}

#[test]
fn engine_failures_are_typed_refusals() {
    let missing = engine_failure(NetworkFailure {
        code: "network_engine_missing",
        message: "absent".into(),
        owner_error: None,
        owner_code: None,
        owner_detail: None,
        owner_remedy: None,
        owner_invalid_input: None,
    });
    assert_eq!(missing.code(), "network_engine_missing");
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
        let failure = engine_failure(source_error(code));
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
        let removed = engine_failure(error);
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
        let failure = engine_failure(error);
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
    let failure = engine_failure(error);
    let detail = failure.detail_value().unwrap();
    assert_eq!(detail["engine_code"].as_str().unwrap().len(), 128);
    assert_eq!(detail["cause"].as_str().unwrap().len(), 4096);
    assert_eq!(detail["code_truncated"], true);
    assert_eq!(detail["cause_truncated"], true);
    let mut error = source_error("network_engine_refused");
    error.owner_code = Some(format!("x{}", "€".repeat(80)));
    error.message = format!("xy{}", "€".repeat(2500));
    let failure = engine_failure(error);
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
    assert_eq!(split(error).unwrap(), "exact original cause");
    let mut error = source_error("network_engine_refused");
    error.owner_code = Some("grid_task_bound_refused".into());
    let failure = engine_failure(error);
    assert_eq!(failure.code(), "network_engine_refused");
    assert_eq!(failure.message(), "exact original cause");
}

#[test]
fn every_gis_consumer_declares_the_complete_source_engine_vocabulary_once() {
    for command in [&crate::inspect::COMMAND, &crate::convert::COMMAND] {
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
}
