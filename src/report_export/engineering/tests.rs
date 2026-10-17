use super::*;
use serde_json::json;

const HISTORICAL: &[u8] = br#"{"schema":"ds.report.engineering-preflight/v1","source_layers":[],"field_lineage":[],"blockers":0,"finding_counts":{},"coverage_counts":{},"findings":[],"omitted_details":0,"quantity_rows_total":0,"quantity_lineage":[],"omitted_quantity_details":0}"#;

fn preflight() -> EngineeringPreflight {
    serde_json::from_slice(HISTORICAL).unwrap()
}

// Captured from installed Network r8 (source ced50f1597c99af3a27211e578f0541625a5d8e8)
// with the report-engineering/v1 request and public humble-pole model fixtures.
// Both native receipts were checked against exact input and artifact SHA/size.
const NATIVE_REPORT_ENGINEERING_RECEIPT: &str = r#"{"outcome":"completed","schema":"ds.network.task-result/v1","inputSha256":"c85e07f2f0f985211856728687fa3d773a3151d4ed73db0e7c95105713eac99d","engineIdentity":{"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"coreVersion":"0.1.0","sourceSha":"ced50f1597c99af3a27211e578f0541625a5d8e8","dependencyClosureSha256":"e3167eaa5152e6b4837bdef4047e8de6b12cd4e42ba26e7757280f1b810646aa","environment":"native","runtime":{"environment":"native","logicalCpus":16,"internalWorkerThreads":1},"jsonObjectOrder":"insertion","capabilities":["lv_processing_v1","lv_differential_plan_v1","lv_publication_v1","lv_analysis_validation_v1","design_snapshot_v1","grid_network_calculation_v1","grid_mv_processing_v1","mv_projection_v1","mv_report_v1","grid_query_v1","grid_feature_code_table_report_v1","grid_read_v1","grid_write_admission_v1","grid_apply_v1","grid_apply_batch_v1","grid_manifest_v1","grid_inspect_v1","grid_publication_v1","profile_session_v1","profile_edit_controls_v1","profile_package_open_v1","profile_package_checkpoint_v1","profile_authored_facts_v1","profile_runtime_v1","profile_view_v1","profile_observe_v1","profile_read_projections_v1","profile_properties_v1","profile_selection_v1","report_engineering_v1","report_sizing_v1","report_feeder_selection_v1"]},"artifact":{"file":"engineering.json","byteCount":778,"sha256":"17bdb9fbb4baa090b50cd5b5425f307497280e1282d1ec6782793f7ecee2a780","schema":"ds.network.report-engineering.result/v1"}}"#;
const NATIVE_MV_REPORT_RECEIPT: &str = r#"{"outcome":"completed","schema":"ds.network.task-result/v1","inputSha256":"a44e5d0328b7dd8d20125da686b755e479438d8b44a31d9059a9745743164239","engineIdentity":{"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"coreVersion":"0.1.0","sourceSha":"ced50f1597c99af3a27211e578f0541625a5d8e8","dependencyClosureSha256":"e3167eaa5152e6b4837bdef4047e8de6b12cd4e42ba26e7757280f1b810646aa","environment":"native","runtime":{"environment":"native","logicalCpus":16,"internalWorkerThreads":1},"jsonObjectOrder":"insertion","capabilities":["lv_processing_v1","lv_differential_plan_v1","lv_publication_v1","lv_analysis_validation_v1","design_snapshot_v1","grid_network_calculation_v1","grid_mv_processing_v1","mv_projection_v1","mv_report_v1","grid_query_v1","grid_feature_code_table_report_v1","grid_read_v1","grid_write_admission_v1","grid_apply_v1","grid_apply_batch_v1","grid_manifest_v1","grid_inspect_v1","grid_publication_v1","profile_session_v1","profile_edit_controls_v1","profile_package_open_v1","profile_package_checkpoint_v1","profile_authored_facts_v1","profile_runtime_v1","profile_view_v1","profile_observe_v1","profile_read_projections_v1","profile_properties_v1","profile_selection_v1","report_engineering_v1","report_sizing_v1","report_feeder_selection_v1"]},"artifact":{"file":"mv-report.json","byteCount":9740,"sha256":"b058fb908f09316ea43f0aa9db10134edff099d0b738171a97c65bd460372cf4","schema":"ds.network.mv-report-result/v1"}}"#;

pub(crate) fn evidence() -> Value {
    native_evidence(NATIVE_REPORT_ENGINEERING_RECEIPT)
}

fn native_evidence(receipt: &str) -> Value {
    json!({
        "executable_sha256":"f92e4c3b69669efb5111888569cabb21cfa8435fe57fac38d0765d2f1ccd6747",
        "development_override":false,
        "receipt":serde_json::from_str::<Value>(receipt).unwrap()
    })
}

#[test]
fn historical_preflight_without_network_evidence_roundtrips_identical_bytes() {
    let historical = preflight();
    assert!(historical.valid());
    assert!(historical.network_engineering.is_empty());
    assert_eq!(serde_json::to_vec(&historical).unwrap(), HISTORICAL);
    let mut extra: Value = serde_json::from_slice(HISTORICAL).unwrap();
    extra["unknown_preflight_field"] = json!(true);
    assert!(serde_json::from_value::<EngineeringPreflight>(extra).is_err());
}

#[test]
fn producer_network_provenance_is_preserved_with_owner_additions() {
    let mut document: Value = serde_json::from_slice(HISTORICAL).unwrap();
    let mut rows = vec![evidence(), native_evidence(NATIVE_MV_REPORT_RECEIPT)];
    for row in &mut rows {
        row["receipt"]["owner_addition"] = json!({"retained":[1,"exact",true]});
        row["receipt"]["engineIdentity"]["owner_addition"] = json!({"exact":-0.0});
    }
    document["network_engineering"] = json!(rows);
    let admitted: EngineeringPreflight = serde_json::from_value(document.clone()).unwrap();
    assert!(admitted.valid());
    assert_eq!(serde_json::to_value(admitted).unwrap(), document);
}

#[test]
fn malformed_network_hashes_and_types_are_refused() {
    for path in [
        "/executable_sha256",
        "/receipt/inputSha256",
        "/receipt/artifact/sha256",
    ] {
        for invalid in [
            json!("A".repeat(64)),
            json!("g".repeat(64)),
            json!("a".repeat(63)),
            json!(0),
            Value::Null,
        ] {
            let mut row = evidence();
            *row.pointer_mut(path).unwrap() = invalid;
            let mut preflight = preflight();
            preflight.network_engineering = vec![row];
            assert!(!preflight.valid(), "{path}");
        }
    }
    for (path, invalid) in [
        ("/development_override", json!("false")),
        ("/receipt", json!([])),
        ("/receipt/engineIdentity", json!("identity")),
        ("/receipt/artifact", json!([])),
        ("/receipt/artifact/byteCount", json!(0)),
        ("/receipt/artifact/byteCount", json!(-1)),
        ("/receipt/artifact/byteCount", json!(1.5)),
        ("/receipt/artifact/byteCount", json!("42")),
    ] {
        let mut row = evidence();
        *row.pointer_mut(path).unwrap() = invalid;
        let mut preflight = preflight();
        preflight.network_engineering = vec![row];
        assert!(!preflight.valid(), "{path}");
    }
}

#[test]
fn crossed_network_schemas_outcomes_and_outer_fields_are_refused() {
    for (path, invalid) in [
        ("/receipt/schema", "another-task-result"),
        ("/receipt/outcome", "failed"),
        ("/receipt/engineIdentity/schema", "another-engine"),
        ("/receipt/artifact/file", "../engineering.json"),
        ("/receipt/artifact/schema", "another-result"),
    ] {
        let mut row = evidence();
        *row.pointer_mut(path).unwrap() = json!(invalid);
        let mut preflight = preflight();
        preflight.network_engineering = vec![row];
        assert!(!preflight.valid(), "{path}");
    }
    for field in ["executable_sha256", "development_override", "receipt"] {
        let mut row = evidence();
        row.as_object_mut().unwrap().remove(field);
        let mut preflight = preflight();
        preflight.network_engineering = vec![row];
        assert!(!preflight.valid(), "{field}");
    }
    let mut row = evidence();
    row["extra"] = json!(true);
    let mut preflight = preflight();
    preflight.network_engineering = vec![row];
    assert!(!preflight.valid());
}

#[test]
fn network_provenance_roster_and_each_serialized_record_are_bounded() {
    let mut preflight = preflight();
    preflight.network_engineering = vec![evidence(); 4096];
    assert!(preflight.valid());
    preflight.network_engineering.push(evidence());
    assert!(!preflight.valid());
    let mut row = evidence();
    row["receipt"]["padding"] = json!("");
    let remaining = 64 * 1024 - serde_json::to_vec(&row).unwrap().len();
    row["receipt"]["padding"] = json!("x".repeat(remaining));
    assert_eq!(serde_json::to_vec(&row).unwrap().len(), 64 * 1024);
    preflight.network_engineering = vec![row.clone()];
    assert!(preflight.valid());
    row["receipt"]["padding"] = json!("x".repeat(remaining + 1));
    preflight.network_engineering = vec![row];
    assert!(!preflight.valid());
}

#[test]
fn actual_native_report_receipts_are_admitted_and_preserved() {
    for receipt in [NATIVE_REPORT_ENGINEERING_RECEIPT, NATIVE_MV_REPORT_RECEIPT] {
        let evidence = native_evidence(receipt);
        assert!(valid_network_engineering(&evidence));
        let mut preflight = preflight();
        preflight.network_engineering = vec![evidence.clone()];
        assert!(preflight.valid());
        assert_eq!(
            serde_json::to_value(preflight).unwrap()["network_engineering"][0],
            evidence
        );
    }
}

#[test]
fn native_artifact_file_and_schema_must_match_one_exact_owner_pair() {
    for (file, schema) in [
        ("engineering.json", "ds.network.mv-report-result/v1"),
        ("mv-report.json", "ds.network.report-engineering.result/v1"),
        ("result.json", "ds.network.report-engineering.result/v1"),
        ("other.json", "ds.network.mv-report-result/v1"),
        ("engineering.json", "unknown-result-schema"),
        ("mv-report.json", "unknown-result-schema"),
    ] {
        let mut row = evidence();
        row["receipt"]["artifact"]["file"] = json!(file);
        row["receipt"]["artifact"]["schema"] = json!(schema);
        assert!(!valid_network_engineering(&row), "{file} / {schema}");
    }
}
