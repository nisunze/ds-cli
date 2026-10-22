use super::*;

fn source() -> DesignFeatureSourceContext {
    DesignFeatureSourceContext::TransformerSnapshot {
        ds_project: "project-a".into(),
        transformer: "T-1".into(),
        version: Some(4),
        content_digest: Some("source-digest".into()),
    }
}
fn select_encoded(input: &str) -> Result<String, DesignFeaturesSelectFault> {
    let output = execute(input.as_bytes());
    decode_response(&output, &source())?;
    Ok(output)
}

fn parity_input() -> String {
    serde_json::json!({
        "source": source(),
        "layers": {
            "poles": {
                "type": "FeatureCollection",
                "features": [
                    {
                        "type": "Feature",
                        "id": "pole-1",
                        "properties": {"kind": "pole", "phase": null},
                        "geometry": {"type": "Point", "coordinates": [30.0, -2.0]}
                    },
                    {
                        "type": "Feature",
                        "properties": {"fid": "unsafe", "kind": "pole"},
                        "geometry": {"type": "Point", "coordinates": [31.0, -3.0]}
                    }
                ]
            }
        },
        "selector": {
            "layers": ["poles"],
            "where": {"kind": "pole", "phase": null},
            "bbox": [29.0, -2.5, 30.5, -1.5],
            "ids": ["pole-1"],
            "sample": 1
        }
    })
    .to_string()
}

#[test]
fn exact_selector_and_result_wire_contract_is_preserved() {
    let output: Value = serde_json::from_str(&select_encoded(&parity_input()).unwrap()).unwrap();
    assert_eq!(
        output["selection"],
        serde_json::json!({
            "receipt": {
                "selectedLayers": ["poles"],
                "scannedFeatures": 2,
                "matchedFeatures": 1,
                "matchedByLayer": {"poles": 1},
                "missingIdentityFeatures": 1,
                "identifiedMatches": 1,
                "projectedIdCount": 1,
                "projectedIdsTruncated": false,
                "sampleCount": 1,
                "sampleWithGeometry": 1,
                "sampleWithoutGeometry": 0,
                "sampleGeometryOversizeOmissions": 0
            },
            "matches": [{"layer": "poles", "index": 0, "id": "pole-1"}],
            "ids": ["pole-1"],
            "sample": [{
                "layer": "poles",
                "index": 0,
                "id": "pole-1",
                "properties": {"kind": "pole", "phase": null},
                "geometry": {"type": "Point", "coordinates": [30.0, -2.0]},
                "geometryState": "included"
            }]
        })
    );
}

#[test]
fn malformed_json_or_missing_context_return_typed_details_without_echoing_input() {
    let malformed = select_encoded("{").unwrap_err();
    assert_eq!(malformed.code, "invalid_json");
    assert_eq!(malformed.details["category"], "eof");
    assert!(!malformed
        .to_json()
        .contains("feature-selection input is not the closed JSON contract: {"));

    let unknown = select_encoded(r#"{"layers":{},"path":"/escape"}"#).unwrap_err();
    assert_eq!(unknown.code, "invalid_json");
    assert_eq!(unknown.details["category"], "data");
    assert!(!unknown.to_json().contains("/escape"));
}

#[test]
fn kernel_bounds_keep_their_exact_codes_and_machine_details() {
    let input = serde_json::json!({
        "source": source(),
        "layers": {},
        "selector": {"sample": 201}
    })
    .to_string();
    let error = select_encoded(&input).unwrap_err();
    assert_eq!(error.code, "sample_too_large");
    assert_eq!(error.details, serde_json::json!({"count": 201}));

    let layers = (0..=ds_geo::feature_selection::MAX_SELECTION_LAYERS)
        .map(|index| {
            (
                format!("layer-{index}"),
                serde_json::json!({"type":"FeatureCollection", "features":[]}),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let error = select_encoded(
        &serde_json::json!({"source":source(),"layers": layers, "selector": {}}).to_string(),
    )
    .unwrap_err();
    assert_eq!(error.code, "too_many_layers");
    assert_eq!(error.details["count"], 65);
}

#[test]
fn one_shot_payload_bound_is_checked_before_json_parsing() {
    let input = " ".repeat(MAX_INPUT_BYTES + 1);
    let error = select_encoded(&input).unwrap_err();
    assert_eq!(error.code, "input_too_large");
    assert_eq!(
        error.details,
        serde_json::json!({
            "payloadBytes": MAX_INPUT_BYTES + 1,
            "maxPayloadBytes": MAX_INPUT_BYTES,
        })
    );
}

#[test]
fn native_and_browser_encoded_directions_use_the_same_typed_decision() {
    let request: DesignFeaturesSelectRequest = serde_json::from_str(&parity_input()).unwrap();
    let layers_before = request.layers.clone();
    let typed = select(
        request.source.clone(),
        &request.layers,
        request.selector.clone(),
    )
    .unwrap();
    let encoded = execute(&encode_request(&request).unwrap());
    let decoded = decode_response(&encoded, &request.source).unwrap();
    assert_eq!(typed, decoded);
    assert_eq!(
        request.layers, layers_before,
        "selection never mutates captured input"
    );
    assert_eq!(decoded.source, source());
}

#[test]
fn unrelated_projects_and_authored_bases_cannot_cross_a_response() {
    let output = select_encoded(&parity_input()).unwrap();
    let other_project = DesignFeatureSourceContext::TransformerSnapshot {
        ds_project: "project-b".into(),
        transformer: "T-1".into(),
        version: Some(4),
        content_digest: Some("source-digest".into()),
    };
    assert_eq!(
        decode_response(&output, &other_project).unwrap_err().code,
        "selection_response_incompatible"
    );
    let authored = DesignFeatureSourceContext::TransformerWorkingCopy {
        ds_project: "project-a".into(),
        transformer: "T-1".into(),
        base_version: Some(4),
        dirty: true,
        server_known: true,
    };
    assert_eq!(
        decode_response(&output, &authored).unwrap_err().code,
        "selection_response_incompatible"
    );
    let request: DesignFeaturesSelectRequest = serde_json::from_str(&parity_input()).unwrap();
    let typed = select(authored.clone(), &request.layers, request.selector).unwrap();
    let value = serde_json::to_value(&typed).unwrap();
    assert_eq!(value["source"]["baseVersion"], 4);
    assert!(
        value["source"].get("contentDigest").is_none(),
        "no digest is invented for edited bytes"
    );
    assert!(decode_response(&encode_response(typed), &authored).is_ok());
}

#[test]
fn additive_fields_are_compatible_but_required_fields_and_types_are_enforced() {
    let mut input: Value = serde_json::from_str(&parity_input()).unwrap();
    input["optionalFutureInput"] = json!({"hint":true});
    input["source"]["optionalFutureSource"] = json!(17);
    let output = select_encoded(&input.to_string()).unwrap();
    let mut response: Value = serde_json::from_str(&output).unwrap();
    response["optionalFutureResult"] = json!(true);
    response["selection"]["receipt"]["optionalFutureCount"] = json!(3);
    assert!(decode_response(&response.to_string(), &source()).is_ok());
    response["selection"]
        .as_object_mut()
        .unwrap()
        .remove("matches");
    let missing = decode_response(&response.to_string(), &source()).unwrap_err();
    assert_eq!(missing.code, "selection_response_incompatible");
    assert!(missing.message.contains("required fields"));
    let mut response: Value = serde_json::from_str(&output).unwrap();
    response["selection"]["receipt"]["matchedFeatures"] = json!("one");
    assert_eq!(
        decode_response(&response.to_string(), &source())
            .unwrap_err()
            .code,
        "selection_response_incompatible"
    );
    response["selection"]["receipt"]["matchedFeatures"] = json!(0);
    assert_eq!(
        decode_response(&response.to_string(), &source())
            .unwrap_err()
            .code,
        "selection_response_incompatible"
    );
}

#[test]
fn malformed_context_and_selector_refusals_preserve_input_without_partial_result() {
    let request: DesignFeaturesSelectRequest = serde_json::from_str(&parity_input()).unwrap();
    let layers_before = request.layers.clone();
    let mut invalid_source = source();
    if let DesignFeatureSourceContext::TransformerSnapshot { ds_project, .. } = &mut invalid_source
    {
        *ds_project = "../foreign".into();
    }
    assert_eq!(
        select(invalid_source, &request.layers, request.selector.clone())
            .unwrap_err()
            .code,
        "context_corrupt"
    );
    let mut bad_selector = request.selector;
    bad_selector.sample = MAX_SAMPLE_FEATURES + 1;
    let error = select(source(), &request.layers, bad_selector).unwrap_err();
    assert_eq!(error.code, "sample_too_large");
    assert_eq!(error.details, json!({"count":201}));
    assert_eq!(request.layers, layers_before);
    assert!(!error.to_json().contains("pole-1"));
}

#[test]
fn legacy_source_metadata_stays_absent_and_cannot_become_a_write_fence() {
    let request: DesignFeaturesSelectRequest = serde_json::from_str(&parity_input()).unwrap();
    let legacy = DesignFeatureSourceContext::TransformerSnapshot {
        ds_project: "project-a".into(),
        transformer: "T-1".into(),
        version: None,
        content_digest: None,
    };
    let output = select(legacy.clone(), &request.layers, request.selector).unwrap();
    let value = serde_json::to_value(&output).unwrap();
    assert!(value["source"].get("version").is_none());
    assert!(value["source"].get("contentDigest").is_none());
    assert!(decode_response(&encode_response(output), &legacy).is_ok());
}

#[test]
fn malformed_request_refusal_roundtrips_as_data_without_worker_code_erasure() {
    let output = execute(b"{");
    let envelope: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(envelope["status"], "refused");
    let refusal = decode_response(&output, &source()).unwrap_err();
    assert_eq!(refusal.code, "invalid_json");
    assert_eq!(refusal.details["category"], "eof");
    assert_eq!(serde_json::to_value(&refusal).unwrap(), envelope["refusal"]);
    let mut compatible = envelope;
    compatible["optionalOutcomeHint"] = json!(true);
    assert_eq!(
        decode_response(&compatible.to_string(), &source()).unwrap_err(),
        refusal
    );
    compatible["status"] = json!("other_semantics");
    let incompatible = decode_response(&compatible.to_string(), &source()).unwrap_err();
    assert_eq!(incompatible.code, "selection_response_incompatible");
    assert!(incompatible.message.contains("status"));
}
