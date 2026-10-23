//! The Kernel admits upload cleaning requests into unit effects and
//! validates the unit's replies; the cleaning itself is tested beside the
//! Network unit (`ds-network-wasm` `grid::page_data`).
use super::*;
use ds_network_contracts::grid_unit::{encode_reply, GridRefusal};
use serde_json::json;

fn columns_request() -> Vec<u8> {
    serde_json::to_vec(&json!({"operation": "layer_columns", "input": {
        "configDfs": {}, "layer": "lv_poles", "vocabulary": "known"}}))
    .unwrap()
}

fn text_reply(value: serde_json::Value) -> Vec<u8> {
    encode_reply(OPERATION, &Ok(GridResult::Text(value.to_string())))
}

#[test]
fn cleaning_requests_become_unit_effects_after_admission() {
    let input = columns_request();
    let effect = request_effect(&input).unwrap();
    let sent = GridRequest::decode(&effect.payload).unwrap();
    assert_eq!(sent.operation, OPERATION);
    assert_eq!(sent.args, [GridArg::Bytes(input)]);
    for bad in [
        json!({"operation": "rewrite", "input": {}}),
        json!({"operation": "layer_columns", "input": {"configDfs": {}, "layer": "", "vocabulary": "known"}}),
    ] {
        let error = request_effect(&serde_json::to_vec(&bad).unwrap()).unwrap_err();
        assert!(error
            .to_string()
            .starts_with("upload_cleaning_request_invalid: "));
    }
}

#[test]
fn unit_replies_are_validated_and_refusals_keep_their_message() {
    let request = decode_request(&columns_request()).unwrap();
    let reply =
        text_reply(json!({"operation": "layer_columns", "result": {"columns": ["struct_type"]}}));
    assert_eq!(
        decode_unit_reply(&request, &reply).unwrap(),
        UploadCleaningResponse::LayerColumns(LayerColumns {
            columns: vec!["struct_type".into()]
        })
    );
    for bad in [
        json!({"operation": "layer_columns", "result": {"columns": ["a", "a"]}}),
        json!({"operation": "stamp_features", "result": {"features": []}}),
    ] {
        let error = decode_unit_reply(&request, &text_reply(bad)).unwrap_err();
        assert!(error
            .to_string()
            .starts_with("upload_cleaning_response_invalid: "));
    }
    let refused = encode_reply(
        OPERATION,
        &Err(GridRefusal::new(
            "upload_cleaning_request_invalid",
            "upload_cleaning_request_invalid: a layer is required",
        )),
    );
    assert_eq!(
        decode_unit_reply(&request, &refused)
            .unwrap_err()
            .to_string(),
        "upload_cleaning_request_invalid: a layer is required"
    );
    let bytes = encode_reply(OPERATION, &Ok(GridResult::Bytes(vec![1])));
    assert!(decode_unit_reply(&request, &bytes).is_err());
}
