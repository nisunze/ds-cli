use super::*;
use serde_json::json;

fn features() -> Vec<Value> {
    vec![
        json!({"type": "Feature", "id": "a", "geometry": {"type": "Point", "coordinates": [30.06, -1.95]},
               "properties": {"Material": "wood", "structure_number": 3, "village": "Gisgara"}}),
        json!({"type": "Feature", "id": "b", "geometry": {"type": "Point", "coordinates": [30.07, -1.96]},
               "properties": {"Material": "Concrete", "structure_number": "4", "_ds_source_file": "kept.xml"}}),
    ]
}
fn columns() -> Vec<String> {
    vec![
        "Material".into(),
        "structure_number".into(),
        "village".into(),
        "_ds_source_file".into(),
    ]
}
fn chunk() -> Vec<u8> {
    arrow_ipc::features_to_ipc(&features(), &columns()).unwrap()
}
fn plan() -> ColumnPlan {
    serde_json::from_value(json!({
        "cleanData": true,
        "renames": {"Material": "material"},
        "valueReplacements": {"village": {"string:Gisgara": "Gisagara"}}
    }))
    .unwrap()
}
fn clean_request() -> UploadBatchRequest {
    serde_json::from_value(json!({"operation": "clean", "input": {
        "fileName": "poles.geojson", "parsedName": "poles", "targetKey": "lv_poles",
        "columns": columns(), "columnPlan": plan(), "importKey": " ", "baseRowIndex": 1024
    }}))
    .unwrap()
}
fn round_trip(request: &UploadBatchRequest, reply: &UploadBatchResponse) -> UploadBatchResponse {
    decode_response(request, &serde_json::to_vec(reply).unwrap()).unwrap()
}

#[test]
fn clean_scan_and_annotate_return_the_owner_bytes() {
    let ipc = chunk();
    let request = clean_request();
    let reply = execute(&ipc, &request).unwrap();
    let (engine, count) = arrow_ipc::clean_feature_batch_ipc(
        "poles.geojson",
        "poles",
        "lv_poles",
        columns(),
        &ipc,
        Some(plan()),
        None,
        None,
        1024,
    )
    .unwrap();
    assert_eq!(
        reply,
        UploadBatchResponse::Clean(CleanBatchOutcome::Cleaned(BatchIpc {
            ipc: engine,
            feature_count: count
        }))
    );
    assert_eq!(round_trip(&request, &reply), reply);
    let UploadBatchResponse::Clean(CleanBatchOutcome::Cleaned(batch)) = &reply else {
        unreachable!()
    };
    let cleaned = arrow_ipc::ipc_to_features(&batch.ipc).unwrap();
    assert_eq!(cleaned[0]["properties"]["material"], json!("wood"));
    assert_eq!(cleaned[0]["properties"]["village"], json!("Gisagara"));

    let scan: UploadBatchRequest = serde_json::from_value(
        json!({"operation": "scan_replacement_candidates", "input": {"columnPlan": plan()}}),
    )
    .unwrap();
    let reply = execute(&ipc, &scan).unwrap();
    assert_eq!(
        reply,
        UploadBatchResponse::ScanReplacementCandidates(ReplacementCandidates {
            candidates: arrow_ipc::replacement_candidates_ipc(&ipc, &plan()).unwrap()
        })
    );
    assert_eq!(round_trip(&scan, &reply), reply);

    let annotate: UploadBatchRequest =
        serde_json::from_value(json!({"operation": "annotate", "input": {
            "sourceFile": "upload.xml", "sourceLayer": "", "structureNumberOffset": 10
        }}))
        .unwrap();
    let reply = execute(&ipc, &annotate).unwrap();
    let (engine, count) =
        arrow_ipc::annotate_feature_batch_ipc(&ipc, Some("upload.xml"), None, 10).unwrap();
    assert_eq!(
        reply,
        UploadBatchResponse::Annotate(BatchIpc {
            ipc: engine,
            feature_count: count
        })
    );
    assert_eq!(round_trip(&annotate, &reply), reply);
    let UploadBatchResponse::Annotate(batch) = reply else {
        unreachable!()
    };
    let annotated = arrow_ipc::ipc_to_features(&batch.ipc).unwrap();
    assert_eq!(
        annotated[0]["properties"]["_ds_source_file"],
        json!("upload.xml")
    );
    assert_eq!(
        annotated[1]["properties"]["_ds_source_file"],
        json!("kept.xml")
    );
    // A mixed number/text column is stored as text; the offset keeps that type.
    assert_eq!(annotated[0]["properties"]["structure_number"], json!("13"));
    assert_eq!(annotated[1]["properties"]["structure_number"], json!("14"));
}

#[test]
fn an_undecodable_chunk_is_a_cleaning_conflict_as_before_and_a_refusal_elsewhere() {
    let reply = execute(&[1, 2, 3], &clean_request()).unwrap();
    let UploadBatchResponse::Clean(CleanBatchOutcome::Conflict(conflict)) = &reply else {
        panic!("expected a conflict, received {reply:?}");
    };
    assert_eq!(conflict.code, "UPLOAD_CLEANING_CONFLICT");
    let annotate: UploadBatchRequest =
        serde_json::from_value(json!({"operation": "annotate", "input": {}})).unwrap();
    let error = execute(&[1, 2, 3], &annotate).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("upload_batch_request_invalid: "),
        "{error}"
    );
}

#[test]
fn malformed_requests_are_refused() {
    let mut request = clean_request();
    if let UploadBatchRequest::Clean(control) = &mut request {
        control.target_key = " ".into();
    }
    assert!(matches!(
        execute(&chunk(), &request),
        Err(UploadBatchError::InvalidRequest(_))
    ));
    for wire in [
        json!({"operation": "rewrite", "input": {}}),
        json!({"operation": "scan_replacement_candidates", "input": {}}),
        json!({"operation": "annotate", "input": {"structureNumberOffset": "ten"}}),
    ] {
        assert!(serde_json::from_value::<UploadBatchRequest>(wire).is_err());
    }
}

#[test]
fn incompatible_replies_are_refused_by_the_decoder() {
    let ipc = chunk();
    let request = clean_request();
    let scan: UploadBatchRequest = serde_json::from_value(
        json!({"operation": "scan_replacement_candidates", "input": {"columnPlan": plan()}}),
    )
    .unwrap();
    for (request, reply) in [
        (
            &request,
            json!({"operation": "annotate", "result": {"ipc": ipc, "featureCount": 2}}),
        ),
        (
            &request,
            json!({"operation": "clean", "result": {"status": "cleaned", "ipc": [1, 2, 3], "featureCount": 2}}),
        ),
        (
            &request,
            json!({"operation": "clean", "result": {"status": "conflict", "code": "", "message": "m"}}),
        ),
        (
            &request,
            json!({"operation": "clean", "result": {"status": "cleaned", "ipc": ipc}}),
        ),
        (
            &scan,
            json!({"operation": "scan_replacement_candidates", "result": {"candidates": {"other": {}}}}),
        ),
    ] {
        let error = decode_response(request, &serde_json::to_vec(&reply).unwrap()).unwrap_err();
        assert!(
            matches!(error, UploadBatchError::InvalidResponse(_)),
            "{reply}: {error}"
        );
    }
}

#[test]
fn compatible_additive_fields_are_accepted_on_both_sides() {
    let request: UploadBatchRequest = serde_json::from_value(json!({
        "operation": "annotate", "input": {"sourceFile": "a.xml", "futureFlag": true}, "trace": "x"
    }))
    .unwrap();
    let reply = execute(&chunk(), &request).unwrap();
    let mut extended = serde_json::to_value(&reply).unwrap();
    extended["result"]["futureDigest"] = json!("sha256:…");
    extended["receipt"] = json!({});
    assert_eq!(
        decode_response(&request, &serde_json::to_vec(&extended).unwrap()).unwrap(),
        reply
    );
}

/// Chunk columns follow the engine's BTreeMap order — each feature's keys in
/// alphanumerical order, first seen first — even for unsorted property maps.
#[test]
fn chunk_columns_follow_the_engine_key_order_for_unsorted_properties() {
    let features = vec![
        json!({"type": "Feature", "geometry": null, "properties": {"zeta": "1", "Alpha": "2", "mid": "3"}}),
        json!({"type": "Feature", "geometry": null, "properties": {"beta": "4", "zeta": "5"}}),
    ];
    // Decoded rows omit null cells, so each row lists its non-null columns in
    // chunk (schema) order; together they pin the whole column order.
    let columns = |ipc: &[u8]| -> Vec<Vec<String>> {
        arrow_ipc::ipc_to_features(ipc)
            .unwrap()
            .iter()
            .map(|feature| {
                feature["properties"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .cloned()
                    .collect()
            })
            .collect()
    };
    let ipc = arrow_ipc::features_to_ipc(&features, &[]).unwrap();
    assert_eq!(
        columns(&ipc),
        [vec!["Alpha", "mid", "zeta"], vec!["zeta", "beta"]]
    );
    let annotate: UploadBatchRequest = serde_json::from_value(
        json!({"operation": "annotate", "input": {"sourceFile": "f.xml", "sourceLayer": "l"}}),
    )
    .unwrap();
    let UploadBatchResponse::Annotate(batch) = execute(&ipc, &annotate).unwrap() else {
        unreachable!()
    };
    assert_eq!(
        columns(&batch.ipc),
        [
            vec![
                "Alpha",
                "_ds_source_file",
                "_ds_source_layer",
                "mid",
                "zeta"
            ],
            vec!["_ds_source_file", "_ds_source_layer", "zeta", "beta"],
        ]
    );
    let clean: UploadBatchRequest = serde_json::from_value(json!({"operation": "clean", "input": {
        "fileName": "f.xml", "parsedName": "p", "targetKey": "lv_poles", "columns": ["zeta", "Alpha", "mid", "beta"],
        "columnPlan": {"renames": {"mid": "middle"}}
    }}))
    .unwrap();
    let UploadBatchResponse::Clean(CleanBatchOutcome::Cleaned(batch)) =
        execute(&ipc, &clean).unwrap()
    else {
        panic!("expected a cleaned chunk")
    };
    assert_eq!(
        columns(&batch.ipc),
        [vec!["Alpha", "middle", "zeta"], vec!["zeta", "beta"]]
    );
}
