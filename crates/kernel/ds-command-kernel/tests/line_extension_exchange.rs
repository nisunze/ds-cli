//! The typed line extension difference answers exactly what the retired
//! Network export answered, refuses malformed requests by name, and its
//! decoder refuses replies that do not answer the request.
use ds_command_kernel::line_extension::{
    decode_response, decode_response_json, difference, difference_json,
    LineExtensionDifferenceInput,
};
use serde_json::{json, Value};

const M: f64 = 0.000_009;

fn line(id: &str, coords: &[[f64; 2]]) -> Value {
    json!({
        "type": "Feature",
        "id": id,
        "properties": { "cable_size": "35" },
        "geometry": { "type": "LineString", "coordinates": coords }
    })
}

fn extension() -> LineExtensionDifferenceInput {
    LineExtensionDifferenceInput {
        source_features: vec![
            line("incoming", &[[30.0, -2.0], [30.0 + 20.0 * M, -2.0]]),
            line(
                "branch",
                &[
                    [30.0, -2.0 + 1.5 * M],
                    [30.0 + 10.0 * M, -2.0 + 1.5 * M],
                    [30.0 + 10.0 * M, -2.0 + 15.0 * M],
                ],
            ),
        ],
        base_features: vec![line("base", &[[30.0, -2.0], [30.0 + 10.0 * M, -2.0]])],
        coverage_tolerance_m: 3.0,
        heal_tolerance_m: 1.0,
    }
}

#[test]
fn extension_matches_the_retired_network_export_exactly() {
    let input = extension();
    let reply = difference(&input).unwrap();
    let engine = ds_geo::line_difference::line_extension_difference(input.clone()).unwrap();
    assert_eq!(reply, engine);
    assert_eq!(reply.source_feature_count, 2);
    assert_eq!(reply.output_feature_count, 2);
    assert_eq!(reply.layer["features"][0]["id"], "incoming#extension-1");
    let decoded = decode_response(&input, &serde_json::to_vec(&reply).unwrap()).unwrap();
    assert_eq!(decoded, reply);
    // The historical JSON wire spelled every key the same way.
    let wire: Value = serde_json::to_value(&reply).unwrap();
    assert_eq!(
        wire.as_object().unwrap().keys().collect::<Vec<_>>(),
        [
            "source_feature_count",
            "base_feature_count",
            "output_feature_count",
            "source_length_m",
            "covered_length_m",
            "difference_length_m",
            "healed_endpoint_count",
            "coverage_tolerance_m",
            "heal_tolerance_m",
            "alignment_tolerance_degrees",
            "layer"
        ]
    );
}

/// Every object in the value lists its keys in sorted order.
fn sorted_everywhere(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            map.keys().zip(map.keys().skip(1)).all(|(a, b)| a < b)
                && map.values().all(sorted_everywhere)
        }
        Value::Array(items) => items.iter().all(sorted_everywhere),
        _ => true,
    }
}

/// The retired export's maps always iterated in sorted key order, so the layer
/// a host materializes listed columns sorted. The Kernel's maps keep insertion
/// order; the reply must still be those bytes.
#[test]
fn unsorted_feature_keys_return_the_retired_sorted_wire_bytes_with_a_sorted_control() {
    let unsorted = json!({
        "source_features": [{"type": "Feature", "properties": {"zone": "B", "cable_size": "35", "Alpha": {"z": 1, "a": 2}},
            "id": "incoming", "geometry": {"coordinates": [[30.0, -2.0], [30.0 + 20.0 * M, -2.0]], "type": "LineString"}}],
        "base_features": [line("base", &[[30.0, -2.0], [30.0 + 10.0 * M, -2.0]])],
        "heal_tolerance_m": 1.0,
        "coverage_tolerance_m": 0.4
    });
    let input = serde_json::to_vec(&unsorted).unwrap();
    let reply = difference_json(&input).unwrap();
    // Parsed with insertion order kept, so the text's own key order is read.
    let parsed: Value = serde_json::from_str(&reply).unwrap();
    assert!(sorted_everywhere(&parsed["layer"]), "{reply}");
    assert_eq!(
        parsed["layer"]["features"][0]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["Alpha", "cable_size", "zone"]
    );
    // Control: the same request with already-sorted keys gives the same bytes.
    let mut sorted = unsorted.clone();
    sorted.sort_all_objects();
    assert_ne!(
        serde_json::to_string(&sorted).unwrap(),
        serde_json::to_string(&unsorted).unwrap()
    );
    assert_eq!(
        difference_json(&serde_json::to_vec(&sorted).unwrap()).unwrap(),
        reply
    );
    assert_eq!(decode_response_json(&input, &reply).unwrap(), reply);
}

#[test]
fn malformed_requests_are_refused_by_name() {
    let mut zero = extension();
    zero.coverage_tolerance_m = 0.0;
    let mut polygon = extension();
    polygon.base_features = vec![
        json!({"type":"Feature","geometry":{"type":"Polygon","coordinates":[]},"properties":{}}),
    ];
    let mut empty = extension();
    empty.source_features.clear();
    let mut huge = extension();
    huge.base_features = vec![Value::Null; 500_000];
    for input in [zero, polygon, empty, huge] {
        let refused = difference(&input).unwrap_err().to_string();
        assert!(
            refused.starts_with("line_extension_request_invalid:"),
            "{refused}"
        );
    }
    for malformed in [
        &b"{bad"[..],
        br#"{"source_features": [], "base_features": [], "coverage_tolerance_m": "wide"}"#,
    ] {
        let refused = difference_json(malformed).unwrap_err().to_string();
        assert!(
            refused.starts_with("line_extension_request_invalid:"),
            "{refused}"
        );
    }
}

#[test]
fn the_decoder_refuses_replies_that_do_not_answer_the_request() {
    let input = extension();
    let reply = serde_json::to_value(difference(&input).unwrap()).unwrap();
    let mut changes: Vec<Value> = Vec::new();
    for (pointer, value) in [
        ("/source_feature_count", json!(7)),
        ("/base_feature_count", json!(0)),
        ("/output_feature_count", json!(5)),
        ("/coverage_tolerance_m", json!(9.0)),
        ("/difference_length_m", json!(-1.0)),
        ("/layer/type", json!("Feature")),
        ("/layer/features/0/geometry/type", json!("Point")),
        (
            "/layer/features/0/geometry/coordinates",
            json!([[30.0, "x"], [30.1, -2.0]]),
        ),
        ("/layer", json!("collection")),
    ] {
        let mut bad = reply.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        changes.push(bad);
    }
    let text = serde_json::to_vec(&input).unwrap();
    assert!(decode_response_json(&text, "[]")
        .unwrap_err()
        .to_string()
        .starts_with("line_extension_response_invalid:"));
    for bad in changes {
        let refused = decode_response(&input, &serde_json::to_vec(&bad).unwrap()).unwrap_err();
        assert!(
            refused
                .to_string()
                .starts_with("line_extension_response_invalid:"),
            "{refused}"
        );
    }
}

#[test]
fn compatible_additive_fields_are_accepted_on_both_ends() {
    let mut wire = serde_json::to_value(extension()).unwrap();
    wire["future_option"] = json!(true);
    let input: LineExtensionDifferenceInput = serde_json::from_value(wire).unwrap();
    let reply = difference(&input).unwrap();
    let mut extended = serde_json::to_value(&reply).unwrap();
    extended["receipt"] = json!({"elapsed_ms": 3});
    assert_eq!(
        decode_response(&input, &serde_json::to_vec(&extended).unwrap()).unwrap(),
        reply
    );
}
