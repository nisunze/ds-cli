use super::*;
use ds_solar_contracts::{
    SolarErrorCode,
    network_form::{self, AssetReference},
};
use serde_json::json;

fn information() -> ResolvedInformation {
    let bytes = br#"{"schema":"ds.network-information/v1","rows":[{"section":"lv_lines_info_df","description":"classified LV","unit":"m","quantity":1250.0,"transformer":"T1"},{"section":"customers_categories_info_df","description":"residential","unit":"pce","quantity":2.0,"transformer":"T1"}],"transformers":[{"name":"T1","capacity_kva":25.0}]}"#;
    ResolvedInformation {
        reference: AssetReference {
            project_id: "project-1".into(),
            asset_id: "a_000000000001".into(),
            digest: ds_solar_contracts::digest::sha256_hex(bytes),
        },
        base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}

fn document(information: &ResolvedInformation) -> Value {
    json!({
        "schema": network_form::SCHEMA,
        "city_tag": { "definition_id": "city", "value": "Bere" },
        "sources": { "information": information.reference },
        "overrides": {
            "manual_flag": false,
            "proposed": { "mv_length_by_phase_km": { "I": 0.0 } }
        }
    })
}

#[test]
fn raw_wire_operations_equal_the_typed_classified_cells() {
    let wire = information();
    let decoded = decode_information(&wire).unwrap();
    assert_eq!(decoded.reference, wire.reference);
    assert_eq!(decoded.table.rows.len(), 2);
    assert_eq!(
        decoded.table.rows[0].quantity.to_bits(),
        1250.0_f64.to_bits()
    );
    let document = document(&wire);
    let values = network_form::information_values(&decoded).unwrap();
    assert_eq!(values["proposed"]["transformers"][0]["bt_km"], 1.25);
    assert_eq!(values["customers"][0]["count"], 2);
    for (operation, expected) in [
        (
            "snapshot",
            network_form::snapshot("project-1", &document, Some(&decoded)).unwrap(),
        ),
        (
            "adopt",
            network_form::adopt_legacy("project-1", &document, &decoded).unwrap(),
        ),
        (
            "seed",
            network_form::seed("project-1", &document, Some(&values)).unwrap(),
        ),
        ("information", values),
    ] {
        let mut request = json!({
            "operation": operation,
            "project_id": "project-1",
            "information": wire,
        });
        if operation != "information" {
            request["document"] = document.clone();
        }
        let actual: Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap())
                .unwrap();
        assert_eq!(actual, expected, "{operation}");
        if operation == "snapshot" {
            assert_eq!(actual["values"]["manual_flag"], false);
            assert_eq!(
                actual["values"]["proposed"]["mv_length_by_phase_km"]["I"],
                0.0
            );
        }
    }
}

#[test]
fn bad_encoding_digest_and_invalid_classified_cells_remain_typed_refusals() {
    let mut invalid = information();
    invalid.base64 = "!".into();
    let error = decode_information(&invalid).unwrap_err();
    assert_eq!(error.code, SolarErrorCode::InvalidInput);
    assert_eq!(error.message, "invalid shared information encoding");

    let mut mismatch = information();
    mismatch.reference.digest = format!("sha256:{}", "a".repeat(64));
    assert_eq!(
        decode_information(&mismatch).unwrap_err().message,
        "shared information digest mismatch"
    );

    let mut invalid_cells = information();
    let bytes = br#"{"schema":"ds.network-information/v1","rows":[],"transformers":[{"name":"T1","capacity_kva":-1.0}]}"#;
    invalid_cells.reference.digest = ds_solar_contracts::digest::sha256_hex(bytes);
    invalid_cells.base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    assert_eq!(
        decode_information(&invalid_cells).unwrap_err().message,
        "invalid or duplicate information-table transformer"
    );
}

#[test]
fn encoded_and_decoded_bounds_refuse_before_hash_or_table_parsing() {
    let mut oversized = information();
    oversized.base64 = "!".repeat(MAX_INFORMATION_ENCODED_BYTES + 1);
    assert_eq!(
        decode_information(&oversized).unwrap_err().message,
        "shared information exceeds 32 MiB"
    );
    drop(oversized);

    let bytes = vec![b' '; MAX_INFORMATION_BYTES + 1];
    let mut oversized = information();
    oversized.base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    assert!(oversized.base64.len() <= MAX_INFORMATION_ENCODED_BYTES);
    assert_eq!(
        decode_information(&oversized).unwrap_err().message,
        "shared information exceeds 32 MiB"
    );
}

#[test]
fn source_and_project_fences_precede_reading_a_bad_transport_body() {
    let original = information();
    let document = document(&original);
    let mut wrong_source = original;
    wrong_source.reference.asset_id = "a_000000000002".into();
    wrong_source.base64 = "!".into();
    for (operation, message) in [
        (
            "snapshot",
            "resolved information does not match the form's source",
        ),
        ("adopt", "shared information does not match this form"),
        (
            "seed",
            "resolved information does not match the form's source",
        ),
    ] {
        let error = evaluate(
            &serde_json::to_vec(&json!({
                "operation": operation,
                "project_id": "project-1",
                "document": document,
                "information": wrong_source,
            }))
            .unwrap(),
        )
        .unwrap_err();
        assert!(error.contains(message), "{operation}: {error}");
        assert!(!error.contains("encoding"));
    }
    let error = evaluate(
        &serde_json::to_vec(&json!({
            "operation": "information",
            "project_id": "another-project",
            "information": wrong_source,
        }))
        .unwrap(),
    )
    .unwrap_err();
    assert!(error.contains("belongs to another project"));
    assert!(!error.contains("encoding"));
}
