use super::*;
use serde_json::json;

const HISTORICAL: &[u8] = br#"{"schema":"ds.report.engineering-preflight/v1","source_layers":[],"field_lineage":[],"blockers":0,"finding_counts":{},"coverage_counts":{},"findings":[],"omitted_details":0,"quantity_rows_total":0,"quantity_lineage":[],"omitted_quantity_details":0}"#;

fn preflight() -> EngineeringPreflight {
    serde_json::from_slice(HISTORICAL).unwrap()
}

fn evidence() -> Value {
    json!({
        "executable_sha256":"a".repeat(64),
        "development_override":false,
        "receipt":{
            "schema":"ds.network.task-result/v1","outcome":"completed",
            "inputSha256":"b".repeat(64),"resultInputSha256":"c".repeat(64),
            "engineIdentity":{"schema":"ds.network-engine/v1","version":"0.1.0",
                "owner_addition":{"exact":-0.0}},
            "artifact":{"file":"result.json","byteCount":42,"sha256":"d".repeat(64),
                "schema":"ds.network.report-engineering.result/v1"},
            "owner_addition":{"retained":[1,"exact",true]}
        }
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
    document["network_engineering"] = json!([evidence()]);
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
        ("/receipt/artifact/file", "../result.json"),
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
