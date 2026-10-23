use super::*;
use serde_json::json;

fn point(props: Value) -> Value {
    json!({"type": "Feature", "geometry": {"type": "Point", "coordinates": [30.06, -1.95]}, "properties": props})
}

fn config() -> Value {
    json!({
        "know_columns": [
            {"lv_poles": "struct_type", "alias": ["struct typ", "Structure Type"]},
            {"lv_lines": "cable_size"},
            {"common": "village"},
            {"common_points": "x"}
        ],
        "lv_poles_struct_type": [
            {"clean_name": "9m 150daN", "alias": ["9m150", "9M 150 DAN"]}
        ]
    })
}

fn apply_input(features: Vec<Value>, config_dfs: Option<Value>) -> Value {
    let mut input = json!({
        "inspection": {"fileName": "poles.geojson", "layers": [{
            "name": "poles", "featureCount": features.len(), "geometryType": "Point",
            "columns": ["struct typ", "stay", "village"], "features": features
        }]},
        "mapping": [{"parsedName": "poles", "targetKey": "lv_poles"}],
        "importKey": "batch-1",
        "columnPlans": {"poles": {"cleanData": true, "valueReplacements": {"village": {"string:Gisgara": "Gisagara"}}}}
    });
    if let Some(config) = config_dfs {
        input["configDfs"] = config;
    }
    input
}

fn bytes(request: Value) -> Vec<u8> {
    serde_json::to_vec(&request).unwrap()
}

fn run(request: Value) -> UploadCleaningResponse {
    execute_bytes(&bytes(request)).unwrap()
}

fn engine_apply(input: Value) -> Value {
    let config = input.get("configDfs").cloned().unwrap_or(Value::Null);
    let upload: ds_cleaning::UploadCleaningInput = serde_json::from_value(input).unwrap();
    let plan = LvMvDomainPlan::from_value(&config);
    serde_json::to_value(
        ds_cleaning::apply_upload_cleaning_with_domain_plan(upload, &plan).unwrap(),
    )
    .unwrap()
}

#[test]
fn apply_matches_the_engine_owner_with_and_without_project_sheets() {
    let features = vec![
        point(json!({"struct typ": "9m150", "stay": "1", "village": "Gisgara"})),
        point(json!({"struct typ": "9M 150 DAN", "stay": 0, "village": "Huye"})),
    ];
    for config in [Some(config()), None] {
        let input = apply_input(features.clone(), config);
        let reply = run(json!({"operation": "apply", "input": input.clone()}));
        let UploadCleaningResponse::Apply(UploadCleaningOutcome::Cleaned(output)) = &reply else {
            panic!("expected a cleaned upload, received {reply:?}");
        };
        assert_eq!(
            serde_json::to_value(output).unwrap(),
            engine_apply(input.clone())
        );
        let request =
            decode_request(&bytes(json!({"operation": "apply", "input": input}))).unwrap();
        assert_eq!(
            decode_response(&request, &serde_json::to_vec(&reply).unwrap()).unwrap(),
            reply
        );
    }
    // The project's header alias renamed the column and its domain cleaned the value.
    let reply = run(json!({"operation": "apply", "input": apply_input(features, Some(config()))}));
    let value = serde_json::to_value(reply).unwrap();
    let props = &value["result"]["layers"]["lv_poles"]["features"][0]["properties"];
    assert_eq!(props["struct_type"], json!("9m 150daN"));
    assert_eq!(props["village"], json!("Gisagara"));
    assert_eq!(value["result"]["status"], json!("cleaned"));
}

#[test]
fn apply_refuses_whole_layer_json_above_the_admission_bound_as_a_conflict() {
    let features = (0..=APPLY_MAX_FEATURES)
        .map(|index| json!({"type": "Feature", "geometry": null, "properties": {"id": index}}))
        .collect::<Vec<_>>();
    let reply = run(json!({"operation": "apply", "input": apply_input(features, None)}));
    let UploadCleaningResponse::Apply(UploadCleaningOutcome::Conflict(conflict)) = reply else {
        panic!("expected a too-large conflict");
    };
    assert_eq!(conflict.code, APPLY_TOO_LARGE_CODE);
    assert_eq!(
        conflict.details["featureCount"],
        json!(APPLY_MAX_FEATURES + 1)
    );
    assert_eq!(conflict.details["maxFeatures"], json!(APPLY_MAX_FEATURES));
    assert_eq!(conflict.details["reason"], json!("feature_count"));
}

#[test]
fn replacement_conflicts_are_answers_on_apply_and_candidate_validation() {
    let plan = json!({
        "cleanData": true,
        "valueReplacements": {"material": {"string:Wood": "Plastic"}},
        "domains": {"material": {"type": "categorical_string", "values": ["Wood", "Concrete"]}}
    });
    let mut input = apply_input(vec![point(json!({"material": "Wood"}))], None);
    input["columnPlans"]["poles"] = plan.clone();
    let reply = run(json!({"operation": "apply", "input": input.clone()}));
    let UploadCleaningResponse::Apply(UploadCleaningOutcome::Conflict(conflict)) = &reply else {
        panic!("expected a replacement conflict, received {reply:?}");
    };
    assert_eq!(conflict.code, "UPLOAD_CLEANING_CONFLICT");
    let request = decode_request(&bytes(json!({"operation": "apply", "input": input}))).unwrap();
    assert!(decode_response(&request, &serde_json::to_vec(&reply).unwrap()).is_ok());

    let candidates = |raw: &str| {
        json!({"operation": "validate_replacement_candidates", "input": {
            "fileName": "poles.geojson", "parsedName": "poles", "targetKey": "lv_poles",
            "columns": ["material"], "columnPlan": plan, "candidates": {"material": [raw]}
        }})
    };
    let UploadCleaningResponse::ValidateReplacementCandidates(verdict) = run(candidates("Wood"))
    else {
        panic!("wrong operation");
    };
    let engine = ds_cleaning::validate_value_replacement_candidates(
        "poles.geojson",
        "poles",
        "lv_poles",
        vec!["material".into()],
        Some(&serde_json::from_value(plan.clone()).unwrap()),
        &BTreeMap::from([("material".to_string(), vec![json!("Wood")])]),
    );
    assert_eq!(verdict.conflict, engine.err());
    assert!(verdict.conflict.is_some());
    let UploadCleaningResponse::ValidateReplacementCandidates(verdict) =
        run(candidates("Concrete"))
    else {
        panic!("wrong operation");
    };
    assert_eq!(verdict.conflict, None);
}

#[test]
fn preview_stamp_columns_and_merge_match_their_owners() {
    let features = vec![
        point(json!({"material": "wood"})),
        point(json!({"material": "Wood", "_ds_source_file": "kept.geojson"})),
        point(json!({"material": null})),
    ];
    let preview = json!({"features": features, "column": "material", "plan": {"cleanData": true}, "sampleLimit": 2});
    let UploadCleaningResponse::PreviewColumn(reply) =
        run(json!({"operation": "preview_column", "input": preview.clone()}))
    else {
        panic!("wrong operation");
    };
    assert_eq!(
        reply,
        ds_cleaning::preview_upload_column_values(serde_json::from_value(preview).unwrap())
    );
    assert!(reply.samples.len() <= 2);

    let UploadCleaningResponse::StampFeatures(stamped) =
        run(json!({"operation": "stamp_features", "input": {
            "features": features, "file": "upload.geojson", "layer": "poles"
        }}))
    else {
        panic!("wrong operation");
    };
    assert_eq!(
        stamped.features[0]["properties"]["_ds_source_file"],
        json!("upload.geojson")
    );
    assert_eq!(
        stamped.features[1]["properties"]["_ds_source_file"],
        json!("kept.geojson")
    );
    assert_eq!(
        stamped.features[2]["properties"]["_ds_source_layer"],
        json!("poles")
    );

    let config = config();
    for (vocabulary, owner) in [
        (
            "known",
            known_columns_for_layer as fn(&Map<String, Value>, &str) -> Vec<String>,
        ),
        ("upload_mapping", upload_mapping_columns_for_layer),
    ] {
        let UploadCleaningResponse::LayerColumns(columns) =
            run(json!({"operation": "layer_columns", "input": {
                "configDfs": config, "layer": "lv_poles", "vocabulary": vocabulary
            }}))
        else {
            panic!("wrong operation");
        };
        assert_eq!(
            columns.columns,
            owner(config.as_object().unwrap(), "lv_poles")
        );
        assert!(columns.columns.starts_with(&[
            "struct_type".to_string(),
            "village".to_string(),
            "x".to_string()
        ]));
    }

    let merge = json!({
        "layer": "mv_structures", "keyed_layer": true, "label": "Staking", "import_key": "70",
        "existing": [point(json!({"id": "old-70", "import_key": "70"})), point(json!({"id": "keep-120", "import_key": "120"}))],
        "incoming": [point(json!({"id": "new-70"}))]
    });
    let UploadCleaningResponse::ImportKeyMerge(merged) =
        run(json!({"operation": "import_key_merge", "input": merge.clone()}))
    else {
        panic!("wrong operation");
    };
    assert_eq!(
        merged,
        resolve_import_key_merge(serde_json::from_value(merge).unwrap())
    );
    assert_eq!(merged.mode, "replace_key");
    let ids: Vec<_> = merged
        .merged
        .iter()
        .map(|feature| feature["properties"]["id"].clone())
        .collect();
    assert_eq!(ids, vec![json!("keep-120"), json!("new-70")]);
}

#[test]
fn malformed_requests_are_refused_by_name() {
    for request in [
        b"{".to_vec(),
        bytes(json!({"operation": "rewrite_everything", "input": {}})),
        bytes(json!({"operation": "preview_column", "input": {"features": [], "column": " "}})),
        bytes(
            json!({"operation": "layer_columns", "input": {"configDfs": {}, "layer": "", "vocabulary": "known"}}),
        ),
        bytes(
            json!({"operation": "layer_columns", "input": {"configDfs": {}, "layer": "tr", "vocabulary": "every"}}),
        ),
        bytes(json!({"operation": "import_key_merge", "input": {"layer": " "}})),
        bytes(
            json!({"operation": "stamp_features", "input": {"features": [], "file": 3, "layer": "x"}}),
        ),
        // A non-object feature is refused before the owner could trap on it.
        bytes(
            json!({"operation": "stamp_features", "input": {"features": ["x"], "file": "f", "layer": "l"}}),
        ),
    ] {
        let error = execute_bytes(&request).unwrap_err();
        assert!(
            matches!(error, UploadCleaningError::InvalidRequest(_)),
            "{error}"
        );
        assert!(error
            .to_string()
            .starts_with("upload_cleaning_request_invalid: "));
    }
}

#[test]
fn incompatible_replies_are_refused_by_the_decoder() {
    let apply = bytes(
        json!({"operation": "apply", "input": apply_input(vec![point(json!({"village": "Huye"}))], None)}),
    );
    let request = decode_request(&apply).unwrap();
    let good = serde_json::to_value(execute_bytes(&apply).unwrap()).unwrap();
    let mut extra_layer = good.clone();
    extra_layer["result"]["layers"]["customers"] = json!({"features": []});
    let mut miscounted = good.clone();
    miscounted["result"]["featureCounts"]["lv_poles"] = json!(7);
    let mut foreign_source = good.clone();
    foreign_source["result"]["sourceLayers"]["lv_poles"] = json!("not-uploaded");
    for reply in [
        json!({"operation": "preview_column", "result": good["result"].clone()}),
        extra_layer,
        miscounted,
        foreign_source,
        json!({"operation": "apply", "result": {"status": "conflict", "code": "", "message": "x"}}),
        json!({"operation": "apply", "result": {"status": "cleaned"}}),
    ] {
        let error = decode_response(&request, &serde_json::to_vec(&reply).unwrap()).unwrap_err();
        assert!(
            matches!(error, UploadCleaningError::InvalidResponse(_)),
            "{reply}: {error}"
        );
    }

    let merge = decode_request(&bytes(json!({"operation": "import_key_merge", "input": {
        "layer": "mv_lines", "existing": [point(json!({"id": "a"}))], "incoming": []
    }})))
    .unwrap();
    for result in [
        json!({"merged": [], "mode": "replace_some", "stats": {"keys": [], "unkeyed_count": 0, "unkeyed_source_files": [], "has_source_file_map": false}}),
        json!({"merged": [], "mode": "append", "error": "refused", "stats": {"keys": [], "unkeyed_count": 0, "unkeyed_source_files": [], "has_source_file_map": false}}),
    ] {
        let reply = json!({"operation": "import_key_merge", "result": result});
        assert!(decode_response(&merge, &serde_json::to_vec(&reply).unwrap()).is_err());
    }
    let columns = decode_request(&bytes(json!({"operation": "layer_columns", "input": {"configDfs": {}, "layer": "tr", "vocabulary": "known"}}))).unwrap();
    let duplicate = json!({"operation": "layer_columns", "result": {"columns": ["a", "a"]}});
    assert!(decode_response(&columns, &serde_json::to_vec(&duplicate).unwrap()).is_err());
    let stamp = decode_request(&bytes(json!({"operation": "stamp_features", "input": {"features": [point(json!({}))], "file": "f", "layer": "l"}}))).unwrap();
    for features in [json!([]), json!([point(json!({"_ds_source_file": "f"}))])] {
        let reply = json!({"operation": "stamp_features", "result": {"features": features}});
        assert!(decode_response(&stamp, &serde_json::to_vec(&reply).unwrap()).is_err());
    }
}

#[test]
fn compatible_additive_fields_are_accepted_on_both_sides() {
    let mut input = apply_input(vec![point(json!({"village": "Huye"}))], None);
    input["futureOption"] = json!({"enabled": true});
    let request = json!({"operation": "apply", "input": input, "requestedBy": "future-ui"});
    let reply = run(request.clone());
    let mut extended = serde_json::to_value(&reply).unwrap();
    extended["result"]["futureSummary"] = json!({"rows": 1});
    extended["receipt"] = json!("additive");
    let decoded = decode_response(
        &decode_request(&bytes(request)).unwrap(),
        &serde_json::to_vec(&extended).unwrap(),
    )
    .unwrap();
    assert_eq!(decoded, reply);
}

/// Replies carry objects in the engine's BTreeMap key order: unsorted request
/// objects answer exactly like their key-sorted control.
#[test]
fn unsorted_object_keys_answer_in_the_engine_key_order() {
    let unsorted = vec![point(json!({"zeta": 1, "alpha": "a", "Mid": null}))];
    let mut control = unsorted.clone();
    control.iter_mut().for_each(Value::sort_all_objects);
    let keys = |feature: &Value| -> Vec<String> {
        feature["properties"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    };
    for features in [&unsorted, &control] {
        let UploadCleaningResponse::StampFeatures(stamped) =
            run(json!({"operation": "stamp_features", "input": {
                "features": features, "file": "f.geojson", "layer": "l"
            }}))
        else {
            panic!("wrong operation")
        };
        assert_eq!(
            keys(&stamped.features[0]),
            [
                "Mid",
                "_ds_source_file",
                "_ds_source_layer",
                "alpha",
                "zeta"
            ]
        );
        assert_eq!(stamped.features[0]["properties"]["Mid"], Value::Null);
    }
    let apply = |features: &Vec<Value>| {
        serde_json::to_string(&run(json!({"operation": "apply", "input": {
            "inspection": {"fileName": "f.geojson", "layers": [{"name": "p", "columns": ["zeta", "alpha", "Mid"], "features": features}]},
            "mapping": [{"parsedName": "p", "targetKey": "lv_poles"}]
        }})))
        .unwrap()
    };
    assert_eq!(apply(&unsorted), apply(&control));
    let merge = |features: &Vec<Value>| {
        serde_json::to_string(&run(json!({"operation": "import_key_merge", "input": {
            "layer": "lv_poles", "existing": features, "incoming": features, "mode": "append"
        }})))
        .unwrap()
    };
    assert_eq!(merge(&unsorted), merge(&control));
    assert!(merge(&unsorted).contains(r#""properties":{"Mid":null,"alpha":"a","zeta":1}"#));
}
