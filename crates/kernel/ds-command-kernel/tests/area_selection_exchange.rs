//! The typed area selection answers the spatial-selection oracle exactly as
//! the retired Network lasso export did, byte for byte on the JSON wire,
//! refuses malformed selectors, and its decoder refuses replies that do not
//! answer the request.
use ds_command_kernel::area_selection::{
    decode_response, decode_response_json, select, select_json, AreaSelectionLayer,
    AreaSelectionRequest,
};
use ds_geo_lite::region::RegionPredicate;
use serde_json::{json, Value};

/// The lasso selection oracle, the Kernel's own copy of the semantics Network
/// also tests; the Kernel never reads another repository's sources.
const ORACLE: &str = include_str!(
    "fixtures/spatial-selection-semantics.json"
);

fn oracle() -> Value {
    serde_json::from_str(ORACLE).unwrap()
}

fn request(selector: Value, predicate: RegionPredicate) -> AreaSelectionRequest {
    let fixture = oracle();
    AreaSelectionRequest {
        polygon: selector,
        predicate,
        layers: vec![AreaSelectionLayer {
            name: "oracle".into(),
            features: fixture["layer"]["features"].as_array().unwrap().clone(),
        }],
    }
}

fn ids(features: &[Value]) -> Vec<String> {
    let mut ids: Vec<String> = features
        .iter()
        .map(|feature| feature["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

#[test]
fn every_predicate_selects_exactly_the_oracle_ids_and_returns_features_unchanged() {
    let fixture = oracle();
    for (predicate, key) in [
        (RegionPredicate::Intersects, "intersects"),
        (RegionPredicate::Within, "within"),
        (RegionPredicate::Outside, "outside"),
    ] {
        let asked = request(fixture["selector"]["polygon"].clone(), predicate);
        let reply = select(&asked).unwrap();
        let decoded = decode_response(&asked, &serde_json::to_vec(&reply).unwrap()).unwrap();
        assert_eq!(decoded, reply);
        let mut expected: Vec<String> =
            serde_json::from_value(fixture["expected"][key].clone()).unwrap();
        expected.sort();
        assert_eq!(ids(&reply.layers[0].features), expected, "{key}");
        assert_eq!(reply.layers[0].input_count, 11);
        assert_eq!(reply.total_selected, reply.layers[0].selected_count);
        for feature in &reply.layers[0].features {
            assert!(
                asked.layers[0].features.contains(feature),
                "selected features are unchanged inputs"
            );
        }
    }
}

/// The retired export ran on maps that always iterate in sorted key order, so
/// its JSON reply listed every object's keys sorted at every level. The
/// Kernel's maps keep insertion order; the reply must still be those bytes,
/// or a host reading the first matching key or deriving column order would
/// silently answer differently.
#[test]
fn unsorted_feature_keys_return_the_retired_sorted_wire_bytes_with_a_sorted_control() {
    let unsorted = json!({
        "polygon": {"type": "Polygon", "coordinates": [[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]]},
        "layers": [{"name": "survey", "features": [
            {"type": "Feature", "properties": {"zone": "B", "meta": {"z": 2, "a": 1}, "alpha": {"x": "first-in-source"}, "Code": 7},
             "id": "in", "geometry": {"coordinates": [5, 5], "type": "Point"}},
            {"type": "Feature", "id": "out", "properties": {"b": 1}, "geometry": {"type": "Point", "coordinates": [20, 5]}}
        ]}]
    });
    let expected = r#"{"layers":[{"name":"survey","input_count":2,"selected_count":1,"features":[{"geometry":{"coordinates":[5,5],"type":"Point"},"id":"in","properties":{"Code":7,"alpha":{"x":"first-in-source"},"meta":{"a":1,"z":2},"zone":"B"},"type":"Feature"}]}],"total_selected":1}"#;
    let reply = select_json(&serde_json::to_vec(&unsorted).unwrap()).unwrap();
    assert_eq!(reply, expected);
    // Control: the same features already in sorted order give the same bytes.
    let mut sorted = unsorted.clone();
    sorted.sort_all_objects();
    assert_ne!(
        serde_json::to_string(&sorted).unwrap(),
        serde_json::to_string(&unsorted).unwrap(),
        "the fixture's keys are deliberately unsorted"
    );
    assert_eq!(
        select_json(&serde_json::to_vec(&sorted).unwrap()).unwrap(),
        expected
    );
    // The decoder accepts the sorted reply for the unsorted request: map
    // equality compares entries, never key order.
    let input = serde_json::to_vec(&unsorted).unwrap();
    assert_eq!(decode_response_json(&input, &reply).unwrap(), reply);
}

#[test]
fn the_default_predicate_and_payload_spelling_keep_the_retired_wire() {
    let fixture = oracle();
    let wire = json!({
        "polygon": fixture["selector"]["polygon"],
        "layers": [{"name": "oracle", "features": fixture["layer"]["features"]}],
    });
    let asked: AreaSelectionRequest = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(asked.predicate, RegionPredicate::Intersects);
    let reply: Value =
        serde_json::from_str(&select_json(&serde_json::to_vec(&wire).unwrap()).unwrap()).unwrap();
    assert_eq!(
        reply.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["layers", "total_selected"]
    );
    assert_eq!(
        reply["layers"][0]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["name", "input_count", "selected_count", "features"]
    );
}

#[test]
fn malformed_selectors_are_refused_by_name() {
    let fixture = oracle();
    let multipolygon = select(&request(
        fixture["selector"]["multipolygon_selector"].clone(),
        RegionPredicate::Intersects,
    ))
    .unwrap_err()
    .to_string();
    assert_eq!(
        multipolygon,
        format!(
            "area_selection_request_invalid: {}",
            fixture["expected"]["multipolygon_selector_error"]
                .as_str()
                .unwrap()
        )
    );
    let degenerate = select(&request(
        json!({"type":"Polygon","coordinates":[[[0,0],[1,0],[0,0]]]}),
        RegionPredicate::Within,
    ))
    .unwrap_err()
    .to_string();
    assert!(degenerate.starts_with(
        "area_selection_request_invalid: selector polygon must have at least three vertices"
    ));
    for malformed in [
        &b"{bad"[..],
        br#"{"polygon": {}, "predicate": "nearby"}"#,
        br#"{"polygon": {"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}, "layers": "all"}"#,
    ] {
        let refused = select_json(malformed).unwrap_err().to_string();
        assert!(
            refused.starts_with("area_selection_request_invalid:"),
            "{refused}"
        );
    }
    let huge = AreaSelectionRequest {
        polygon: fixture["selector"]["polygon"].clone(),
        predicate: RegionPredicate::Intersects,
        layers: vec![AreaSelectionLayer {
            name: "huge".into(),
            features: vec![Value::Null; 1_000_001],
        }],
    };
    assert!(select(&huge)
        .unwrap_err()
        .to_string()
        .contains("feature limit"));
}

#[test]
fn the_decoder_refuses_replies_that_do_not_answer_the_request() {
    let fixture = oracle();
    let asked = request(
        fixture["selector"]["polygon"].clone(),
        RegionPredicate::Intersects,
    );
    let input = serde_json::to_vec(&asked).unwrap();
    let reply: Value = serde_json::from_str(&select_json(&input).unwrap()).unwrap();
    let mut renamed = reply.clone();
    renamed["layers"][0]["name"] = json!("other_layer");
    let mut counted = reply.clone();
    counted["total_selected"] = json!(99);
    let mut altered = reply.clone();
    altered["layers"][0]["features"][0]["properties"] = json!({"injected": true});
    let mut invented = reply.clone();
    invented["layers"][0]["features"][0] =
        json!({"type":"Feature","id":"invented","geometry":null,"properties":{}});
    let mut reordered = reply.clone();
    reordered["layers"][0]["features"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let mut missing = reply.clone();
    missing["layers"] = json!([]);
    let mut shape = reply.clone();
    shape["layers"][0]["input_count"] = json!("eleven");
    for bad in [
        renamed, counted, altered, invented, reordered, missing, shape,
    ] {
        let refused =
            decode_response_json(&input, &serde_json::to_string(&bad).unwrap()).unwrap_err();
        assert!(
            refused
                .to_string()
                .starts_with("area_selection_response_invalid:"),
            "{refused}"
        );
    }
    assert!(decode_response_json(&input, "not json")
        .unwrap_err()
        .to_string()
        .starts_with("area_selection_response_invalid:"));
}

#[test]
fn compatible_additive_fields_are_accepted_on_both_ends() {
    let fixture = oracle();
    let input = serde_json::to_vec(&json!({
        "polygon": fixture["selector"]["polygon"],
        "predicate": "within",
        "layers": [{"name": "oracle", "features": fixture["layer"]["features"], "future": 1}],
        "trace": "additive",
    }))
    .unwrap();
    let reply = select_json(&input).unwrap();
    let mut extended: Value = serde_json::from_str(&reply).unwrap();
    extended["receipt"] = json!({"elapsed_ms": 1});
    extended["layers"][0]["future"] = json!(true);
    let extended = serde_json::to_string(&extended).unwrap();
    assert_eq!(decode_response_json(&input, &extended).unwrap(), extended);
}
