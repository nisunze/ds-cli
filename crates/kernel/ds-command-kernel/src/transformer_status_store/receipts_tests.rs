use super::*;
use serde_json::json;

fn held(needs_refresh: bool) -> Observation {
    let document = json!({"transformers":[{"name":"T-1","kind":"individual"}]});
    Observation {
        sha256: super::super::digest(&document).unwrap(),
        document,
        observed_at_ms: 123,
        needs_refresh,
        last_error: None,
        revision: 1,
    }
}
fn projection(observation: Option<StatusObservation>) -> Vec<u8> {
    serde_json::to_vec(
        &json!({"lane":"canary","project":{"ds_project":"project-a"},
        "count":1,"transformers":[{"name":"T-1","kind":"individual","future_row_field":true}],
        "observation":observation}),
    )
    .unwrap()
}
fn path() -> PathBuf {
    std::env::temp_dir().join("ds-status-contract/status.json")
}
fn receipt(bytes: &[u8]) -> StatusFileReceipt {
    StatusFileReceipt::from_projection_bytes(&path(), "canary", "project-a", "principal-a", bytes)
        .unwrap()
}
fn decode(receipt: &StatusFileReceipt, bytes: &[u8]) -> Result<Value, String> {
    receipt.decode_projection_bytes(bytes, &path(), "canary", "project-a", "principal-a", true)
}

#[test]
fn actual_constructor_serde_and_reader_share_one_receipt_and_observation_contract() {
    let observation = status_observation(&held(true), false, Some("offline")).unwrap();
    let bytes = projection(Some(observation.clone()));
    let constructed = receipt(&bytes);
    let encoded = serde_json::to_vec(&constructed).unwrap();
    let decoded = StatusFileReceipt::decode(serde_json::from_slice(&encoded).unwrap()).unwrap();
    assert_eq!(constructed, decoded);
    let value = decode(&decoded, &bytes).unwrap();
    assert_eq!(
        value["observation"],
        serde_json::to_value(observation).unwrap()
    );
    assert_eq!(value["count"], 1);
    assert_eq!(decoded.size_bytes, bytes.len() as u64);
}

#[test]
fn additive_fields_are_compatible_and_required_field_types_remain_enforced() {
    let bytes = projection(Some(status_observation(&held(false), true, None).unwrap()));
    let original = receipt(&bytes);
    let mut wire = serde_json::to_value(&original).unwrap();
    wire["v"] = json!(77);
    wire["contract"] = json!(31);
    wire["future_receipt_field"] = json!({"hint":true});
    wire["observation"]["future_observation_field"] = json!(2);
    assert_eq!(StatusFileReceipt::decode(wire.clone()).unwrap(), original);
    assert!(decode(&StatusFileReceipt::decode(wire.clone()).unwrap(), &bytes).is_ok());
    wire["count"] = json!("one");
    assert_eq!(
        StatusFileReceipt::decode(wire).unwrap_err(),
        "malformed file receipt"
    );
    let mut wire = serde_json::to_value(&original).unwrap();
    wire.as_object_mut().unwrap().remove("principal");
    assert!(StatusFileReceipt::decode(wire).is_err());
    let mut wire = serde_json::to_value(&original).unwrap();
    wire["observation"]["state"] = json!("future_state");
    assert!(StatusFileReceipt::decode(wire).is_err());
}

#[test]
fn crossed_identity_scope_path_count_digest_and_observation_refuse() {
    let bytes = projection(Some(status_observation(&held(false), false, None).unwrap()));
    let original = receipt(&bytes);
    for field in [
        "principal",
        "project",
        "lane",
        "path",
        "sha256",
        "count",
        "observation",
    ] {
        let mut wire = serde_json::to_value(&original).unwrap();
        wire[field] = match field {
            "count" => json!(2),
            "observation" => json!(null),
            "sha256" => json!("b".repeat(64)),
            "path" => json!(std::env::temp_dir().join("other-status.json")),
            _ => json!("foreign"),
        };
        let changed = StatusFileReceipt::decode(wire).unwrap();
        assert!(decode(&changed, &bytes).is_err(), "{field}");
    }
    let mut tampered = bytes.clone();
    tampered.push(b' ');
    assert_eq!(
        decode(&original, &tampered).unwrap_err(),
        "status output digest mismatch"
    );
    assert!(
        decode(&original, &bytes).is_ok(),
        "a failed admission changes no retained state"
    );
}

#[test]
fn constructor_refuses_invalid_domain_fields_before_a_host_creates_a_file() {
    let observation = status_observation(&held(false), false, None).unwrap();
    for changed in [
        json!({"state":"ready","source":"held","observed_at_ms":1,"sha256":"a".repeat(64),"last_error":"offline"}),
        json!({"state":"held","source":"refresh","observed_at_ms":1,"sha256":"a".repeat(64),"last_error":null}),
        json!({"state":"ready","source":"held","observed_at_ms":9007199254740992u64,"sha256":"a".repeat(64),"last_error":null}),
        json!({"state":"ready","source":"held","observed_at_ms":1,"sha256":"A".repeat(64),"last_error":null}),
        json!({"state":"held","source":"held","observed_at_ms":1,"sha256":"a".repeat(64),"last_error":"x".repeat(257)}),
    ] {
        let mut value: Value =
            serde_json::from_slice(&projection(Some(observation.clone()))).unwrap();
        value["observation"] = changed;
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(
            StatusFileReceipt::from_projection_bytes(
                &path(),
                "canary",
                "project-a",
                "principal-a",
                &bytes
            )
            .is_err()
        );
    }
    let mut value: Value = serde_json::from_slice(&projection(Some(observation))).unwrap();
    value["transformers"][0]["name"] = json!(null);
    assert!(
        StatusFileReceipt::from_projection_bytes(
            &path(),
            "canary",
            "project-a",
            "principal-a",
            &serde_json::to_vec(&value).unwrap()
        )
        .is_err()
    );
}

#[test]
fn held_refresh_metadata_and_legacy_default_reads_keep_their_distinct_meanings() {
    let ready = status_observation(&held(false), false, None).unwrap();
    assert_eq!(ready.source, StatusObservationSource::Held);
    assert_eq!(ready.state, StatusObservationState::Ready);
    let failed = status_observation(&held(false), false, Some("auth_transient")).unwrap();
    assert_eq!(failed.state, StatusObservationState::Held);
    assert_eq!(failed.last_error.as_deref(), Some("auth_transient"));
    assert!(status_observation(&held(true), true, None).is_err());
    let bytes = projection(None);
    let original = receipt(&bytes);
    assert!(
        decode(&original, &bytes).is_err(),
        "an explicit native read requires its observation"
    );
    assert!(
        original
            .decode_projection_bytes(&bytes, &path(), "canary", "project-a", "principal-a", false)
            .is_ok()
    );
}
