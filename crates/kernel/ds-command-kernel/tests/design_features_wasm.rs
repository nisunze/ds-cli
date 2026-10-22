#![cfg(all(feature = "wasm", target_arch = "wasm32"))]

use ds_command_kernel::design_features::{
    DesignFeaturesSelectRequest,
    browser::{decode_wasm, encode_wasm, execute_wasm},
};
use serde_json::Value;
use tsify::Tsify;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn one_shot_feature_selection_has_the_same_wasm_bindgen_wire_contract() {
    let input = serde_json::json!({
        "source": {"kind":"transformer_snapshot","dsProject":"project-a","transformer":"T-1","version":4},
        "layers": {
            "poles": {
                "type": "FeatureCollection",
                "features": [{
                    "type": "Feature",
                    "id": "pole-1",
                    "properties": {"kind": "pole"},
                    "geometry": {"type":"Point", "coordinates":[30.0, -2.0]}
                }]
            }
        },
        "selector": {"ids": ["pole-1"], "sample": 1}
    })
    .to_string();
    let request: DesignFeaturesSelectRequest = serde_json::from_str(&input).unwrap();
    let encoded = encode_wasm(request.clone().into_ts().unwrap()).unwrap();
    let raw = execute_wasm(&encoded);
    let admitted = decode_wasm(&raw, request.source.clone().into_ts().unwrap())
        .unwrap()
        .to_rust()
        .unwrap();
    assert_eq!(admitted.source, request.source);
    let response: Value = serde_json::from_str(&raw).unwrap();
    let output = &response["selection"];
    assert_eq!(output["receipt"]["matchedFeatures"], 1);
    assert_eq!(output["receipt"]["sampleWithGeometry"], 1);
    assert_eq!(output["matches"][0]["id"], "pole-1");
    assert_eq!(output["sample"][0]["properties"]["kind"], "pole");
    assert_eq!(output["sample"][0]["geometry"]["type"], "Point");
    assert_eq!(output["sample"][0]["geometryState"], "included");
}

#[wasm_bindgen_test]
fn one_shot_feature_selection_preserves_typed_errors_on_wasm() {
    let input = serde_json::json!({"source":{"kind":"transformer_snapshot","dsProject":"project-a","transformer":"T-1"},"layers": {}, "selector": {"sample": 201}}).to_string();
    let raw = execute_wasm(input.as_bytes());
    let request: DesignFeaturesSelectRequest = serde_json::from_str(&input).unwrap();
    let thrown: JsValue = decode_wasm(&raw, request.source.into_ts().unwrap()).unwrap_err();
    let thrown: js_sys::Error = thrown.unchecked_into();
    let message = String::from(thrown.message());
    let envelope: Value = serde_json::from_str(&message).unwrap();
    assert_eq!(envelope["code"], "sample_too_large");
    assert_eq!(envelope["details"]["count"], 201);
}
