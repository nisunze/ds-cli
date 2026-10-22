use super::*;
use ds_core_contracts::envelope::{error_envelope, success_envelope};
use serde_json::json;

fn input() -> Value {
    json!({"jobs":[
        {"ds_project":"project_a","transformer_name":"T2","gdfs":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","geometry":null,"properties":{"seed":2}}]}},"settings":{}},
        {"ds_project":"project_a","transformer_name":"T1","gdfs":{"tr":{"type":"FeatureCollection","features":[]}},"settings":{}}
    ],"local_tables":{}})
}
fn captured() -> CapturedLvProcess {
    capture_bytes("project_a", &serde_json::to_vec(&input()).unwrap()).unwrap()
}
fn native() -> NativeLvResultV1 {
    serde_json::from_value(json!({"schema":NATIVE_LV_RESULT_SCHEMA_V1,"engine_core_version":"owner-provenance", "jobs":[
        {"transformer_name":"T2","ok":true,"output":{"gdfs":{"tr":{"type":"FeatureCollection","features":[]}},"settings_used":{},"warnings":[]}},
        {"transformer_name":"T1","ok":false,"error":"the named local table is unavailable","error_code":"source_unavailable"}
    ]})).unwrap()
}
fn result() -> Value {
    serde_json::to_value(captured().response_from_native(native()).unwrap()).unwrap()
}
fn envelope(output: Value) -> Value {
    let mut value = serde_json::to_value(success_envelope(COMMAND, 4, json!({"jobs":2}))).unwrap();
    value["files"] = json!({"out":output});
    value
}

#[test]
fn capture_is_immutable_and_the_native_request_does_not_claim_project_authority() {
    let mut source = input();
    let capture = capture_bytes("project_a", &serde_json::to_vec(&source).unwrap()).unwrap();
    source["jobs"][0]["ds_project"] = json!("project_b");
    source["jobs"][0]["gdfs"]["tr"]["features"][0]["properties"]["seed"] = json!(9);
    let retained: Value = serde_json::from_slice(capture.input()).unwrap();
    assert_eq!(retained["jobs"][0]["ds_project"], "project_a");
    assert_eq!(
        retained["jobs"][0]["gdfs"]["tr"]["features"][0]["properties"]["seed"],
        2
    );
    let native: Value = serde_json::from_slice(&capture.native_request().unwrap()).unwrap();
    assert!(native["jobs"][0].get("ds_project").is_none());
    assert_eq!(native["jobs"][0]["transformer_name"], "T2");
    assert_eq!(
        capture_bytes("project_a", &serde_json::to_vec(&source).unwrap())
            .unwrap_err()
            .code,
        "fast_lv_input_invalid"
    );
    for bytes in [
        b"null".as_slice(),
        b"{\"jobs\":\"wrong\"}".as_slice(),
        b"{".as_slice(),
    ] {
        assert_eq!(
            capture_bytes("project_a", bytes).unwrap_err().code,
            "fast_lv_input_invalid"
        );
    }
}

#[test]
fn native_constructor_serialization_and_owner_decoder_keep_order_and_outcomes() {
    let capture = captured();
    let output = capture.response_from_native(native()).unwrap();
    let bytes = serde_json::to_vec(&output).unwrap();
    let decoded = capture.decode_response(&bytes).unwrap();
    assert_eq!(decoded.jobs[0].ds_project, "project_a");
    assert_eq!(decoded.jobs[0].transformer_name, "T2");
    assert!(decoded.jobs[0].ok);
    assert_eq!(
        decoded.jobs[1].error_code.as_deref(),
        Some("source_unavailable")
    );
    assert!(decoded.jobs[1].output.is_none());
}

#[test]
fn crossed_or_incomplete_results_refuse_before_state_changes() {
    let capture = captured();
    let original = json!({"cached":"unchanged"});
    let mut variants = Vec::new();
    let mut value = result();
    value["jobs"][0]["ds_project"] = json!("project_b");
    variants.push(value);
    let mut value = result();
    value["jobs"][0]["transformer_name"] = json!("Other");
    variants.push(value);
    let mut value = result();
    value["jobs"].as_array_mut().unwrap().swap(0, 1);
    variants.push(value);
    let mut value = result();
    value["jobs"].as_array_mut().unwrap().pop();
    variants.push(value);
    let mut value = result();
    value["jobs"][0].as_object_mut().unwrap().remove("output");
    variants.push(value);
    let mut value = result();
    value["jobs"][0]["output"]["warnings"] = json!("wrong");
    variants.push(value);
    let mut value = result();
    value["jobs"][1]["output"] = result()["jobs"][0]["output"].clone();
    variants.push(value);
    let mut value = result();
    value["jobs"][1].as_object_mut().unwrap().remove("error");
    variants.push(value);
    for value in variants {
        let mut state = original.clone();
        match capture.decode_core_response(envelope(value)) {
            Ok(output) => state = serde_json::to_value(output).unwrap(),
            Err(error) => assert_eq!(error.code, "network_result_invalid"),
        }
        assert_eq!(state, original);
    }
}

#[test]
fn compatible_additions_and_numeric_provenance_do_not_reject_valid_domain_fields() {
    let mut value = envelope(result());
    value["v"] = json!(7);
    value["contract"] = json!(29);
    value["future"] = json!({"metadata":true});
    value["files"]["out"]["future"] = json!([1]);
    value["files"]["out"]["jobs"][0]["future"] = json!(true);
    value["files"]["out"]["jobs"][0]["output"]["future"] = json!("compatible");
    assert!(captured().decode_core_response(value).unwrap().jobs[0].ok);
    for (field, bad) in [
        ("status", json!("future_status")),
        ("command", json!("design.other")),
        ("contract", json!("4")),
    ] {
        let mut value = envelope(result());
        value[field] = bad;
        assert_eq!(
            captured().decode_core_response(value).unwrap_err().code,
            "network_result_invalid"
        );
    }
}

#[test]
fn actual_refusal_serializer_to_decoder_preserves_every_owner_field_and_retry_override() {
    let failure = Failure::conflict("source_changed", "The source changed.")
        .remedy("Read the captured project.")
        .next("ds design inspect --project project_a")
        .detail(json!({"project":"project_a","base":3}))
        .not_retryable();
    let value = serde_json::to_value(error_envelope(COMMAND, 4, &failure)).unwrap();
    let error = captured().decode_core_response(value).unwrap_err();
    assert_eq!(error, CommandRefusal::from_failure(&failure));
    let mut value = envelope(result());
    value["status"] = json!("error");
    value["error"] = json!({"code":"source_changed"});
    assert_eq!(
        captured().decode_core_response(value).unwrap_err().code,
        "network_result_invalid"
    );
}
