use ds_command_kernel::{
    design,
    geojson_comparison::{compare_geojson_layers, decode_comparison, GeoJsonLayerComparisonInput},
};
use serde_json::{json, Value};

fn feature(id: &str, x: f64, value: &str) -> Value {
    json!({"type":"Feature", "id":id, "geometry":{"type":"Point","coordinates":[x,-2.0]},"properties":{"value":value}})
}
fn request() -> GeoJsonLayerComparisonInput {
    let mut missing = feature("missing", 30.0, "missing");
    missing.as_object_mut().unwrap().remove("id");
    serde_json::from_value(json!({"localLayers":{"poles":{"type":"FeatureCollection","features":[feature("u",30.0,"same"),feature("a",30.0,"before"),feature("g",30.0,"same"),feature("b",30.0,"before"),feature("left",30.0,"left"),feature("d",30.0,"duplicate"),feature("d",30.0,"duplicate"),missing.clone()]}},"cloudLayers":{"poles":{"type":"FeatureCollection","features":[feature("u",30.0,"same"),feature("a",30.0,"after"),feature("g",30.1,"same"),feature("b",30.1,"after"),feature("right",30.0,"right"),feature("d",30.0,"duplicate"),missing]}},"ignoredPropertyKeys":["v_first","v_last"],"maxDetailFeatures":500})).unwrap()
}
#[test]
fn real_native_design_comparison_and_typed_caller_agree_for_every_change_class() {
    let input = request();
    let reply = compare_geojson_layers(input.clone()).unwrap();
    let decoded = decode_comparison(&input, &serde_json::to_vec(&reply).unwrap()).unwrap();
    assert_eq!(decoded, reply);
    assert_eq!(
        (
            decoded.totals.unchanged,
            decoded.totals.attribute_only_changed,
            decoded.totals.geometry_only_changed,
            decoded.totals.attribute_and_geometry_changed
        ),
        (1, 1, 1, 1)
    );
    assert_eq!(
        (
            decoded.totals.local_only,
            decoded.totals.cloud_only,
            decoded.totals.ambiguous_identity_features,
            decoded.totals.unmatchable_features
        ),
        (1, 1, 3, 2)
    );
    let snapshot = |layers| design::Snapshot {
        schema: design::SCHEMA.into(),
        transformer: "comparison-fixture".into(),
        crs: "EPSG:4326".into(),
        layers,
        settings: Default::default(),
        network_config: design::NetworkConfig {
            sheets: Default::default(),
            groups: Default::default(),
            models: Default::default(),
        },
        include_design_customers: true,
        sources: vec![],
    };
    let left = snapshot(input.local_layers.clone());
    let right = snapshot(input.cloud_layers.clone());
    let native = design::compare_snapshots(&left, &right, 500).unwrap();
    assert_eq!(
        decode_comparison(&input, &serde_json::to_vec(&native["comparison"]).unwrap()).unwrap(),
        reply
    );
}
#[test]
fn invalid_requests_and_incompatible_responses_refuse_without_mutation() {
    let input = request();
    let before = serde_json::to_value(&input).unwrap();
    let mut invalid = input.clone();
    invalid
        .local_layers
        .insert("poles".into(), json!({"type":"Feature"}));
    assert!(compare_geojson_layers(invalid)
        .unwrap_err()
        .to_string()
        .contains("comparison_request_invalid"));
    let reply = serde_json::to_value(compare_geojson_layers(input.clone()).unwrap()).unwrap();
    for field in ["layer", "counts", "variant", "shape"] {
        let mut bad = reply.clone();
        match field {
            "layer" => bad["layers"][0]["layerName"] = json!("other_project"),
            "counts" => bad["totals"]["matched"] = json!(99),
            "variant" => bad["layers"][0]["details"][0]["changeClass"] = json!("unknown_variant"),
            _ => bad["layers"] = json!("invalid"),
        };
        assert!(
            decode_comparison(&input, &serde_json::to_vec(&bad).unwrap())
                .unwrap_err()
                .to_string()
                .contains("comparison_response_invalid"),
            "{field}"
        );
    }
    assert_eq!(serde_json::to_value(input).unwrap(), before);
}
#[test]
fn compatible_additions_and_bounded_details_survive_the_real_decoder() {
    let mut input = request();
    input.max_detail_features = 2;
    let reply = compare_geojson_layers(input.clone()).unwrap();
    assert!(reply.details_truncated);
    assert_eq!(reply.returned_detail_count, 2);
    let mut wire = serde_json::to_value(&reply).unwrap();
    wire["future_hint"] = json!(true);
    wire["layers"][0]["future_label"] = json!("compatible");
    assert_eq!(
        decode_comparison(&input, &serde_json::to_vec(&wire).unwrap()).unwrap(),
        reply
    );
}
