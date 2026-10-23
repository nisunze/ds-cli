use super::*;
fn item_with_nature(nature: &str) -> TransformerSaveItem {
    let bytes: &[u8] = match nature {
        "new_transformer" => {
            include_bytes!("../tests/fixtures/native-lv-save-new.publication.json")
        }
        "fill_in" => {
            include_bytes!("../tests/fixtures/native-lv-save-reserved.publication.json")
        }
        _ => panic!("no captured owning fixture for transformer nature"),
    };
    let projection: ds_network_contracts::lv_processing::NativeLvPublication =
        serde_json::from_slice(bytes).unwrap();
    TransformerSaveItem {
        transformer_name: "T1".into(),
        base_version: 7,
        source_content_digest: digest(&projection.source_layers).unwrap(),
        source_layers: projection.source_layers,
        gdfs: projection.gdfs,
        config_dfs: projection.config_dfs,
        process_metadata: projection.process_metadata,
        process_input: projection.process_input,
        voltage_drop: projection.voltage_drop,
        operation_id: "save-T1-7".into(),
    }
}
fn item() -> TransformerSaveItem {
    item_with_nature("new_transformer")
}
fn analysis_metadata(item: &TransformerSaveItem) -> Value {
    let a = &item.voltage_drop;
    json!({"state":"ready","schema":a.schema,"method":a.method,"status":a.status,
            "content_digest":digest(&item.gdfs).unwrap(),"analysis_sha256":analysis_sha(item).unwrap(),
            "inputs_digest":a.inputs_digest,"analysis_inputs_digest":a.analysis_inputs_digest,
            "processed_digest":a.processed_digest,"summary":a.summary})
}
fn snapshot(
    item: &TransformerSaveItem,
    layers: &BTreeMap<String, Value>,
    version: u64,
) -> crate::TransformerContext {
    let body = serde_json::to_vec(&json!({"success":true,"data":{"total":1,"found_count":1,"failed_count":0,"results":[{
            "transformer_name":item.transformer_name,"ok":true,"data":{"layers":layers,"voltage_drop_metadata":analysis_metadata(item),"metadata":{"version":version,"content_digest":digest(layers).unwrap()}}
        }]}})).unwrap();
    crate::transformers::decode_context(
        crate::TransportResponse { status: 200, body },
        1024 * 1024,
        "project",
        &item.transformer_name,
    )
    .unwrap()
}
#[test]
fn save_fences_source_and_retries_already_published_result() {
    let item = item();
    let result_digest = digest(&item.gdfs).unwrap();
    assert!(!preflight(
        &item,
        &snapshot(&item, &item.source_layers, 7),
        &result_digest
    )
    .unwrap());
    assert!(preflight(
        &item,
        &snapshot(&item, &item.source_layers, 8),
        &result_digest
    )
    .is_err());
    assert!(preflight(&item, &snapshot(&item, &item.gdfs, 8), &result_digest).unwrap());
    assert!(preflight(&item, &snapshot(&item, &item.gdfs, 6), &result_digest).is_err());
}
#[test]
#[ignore = "requires an explicitly retained native Network save-validation executable"]
fn save_binds_project_config_and_result_digest_without_source_or_force_fields_on_wire() {
    let item = item();
    let mut batch = TransformerSaveBatch {
        project_id: "project".into(),
        items: vec![item.clone()],
    };
    batch.validate("project", DeploymentLane::Stable).unwrap();
    assert!(batch.validate("other", DeploymentLane::Stable).is_err());
    check_config(&item, &serde_json::to_value(&item.config_dfs).unwrap()).unwrap();
    assert!(check_config(
        &item,
        &json!({"lv_poles_struct_type":[{"material":"concrete"}]})
    )
    .is_err());
    let wire: Value = serde_json::from_slice(&batch.wire_body().unwrap()).unwrap();
    let saved = &wire["transformer_saves"][0];
    assert_eq!(saved["content_digest"], digest(&item.gdfs).unwrap());
    assert_ne!(saved["content_digest"], item.source_content_digest);
    assert_eq!(saved["voltage_drop"], analysis_value(&item).unwrap());
    assert_eq!(saved.as_object().unwrap().len(), 7);
    assert!(saved["process_metadata"].get("voltage_drop").is_none());
    assert!(saved.get("process_input").is_none());
    assert!(saved.get("force_version_conflict").is_none());
    assert!(saved.get("source_layers").is_none());
    batch.items[0].source_layers = item.gdfs;
    assert!(batch.validate("project", DeploymentLane::Stable).is_err());
}
#[test]
fn save_refuses_duplicate_items_and_oversized_payloads() {
    let item = item();
    let mut batch = TransformerSaveBatch {
        project_id: "project".into(),
        items: vec![item.clone(), item.clone()],
    };
    assert!(batch.validate("project", DeploymentLane::Stable).is_err());
    batch.items = vec![item];
    let held = "x".repeat(3 * 1024 * 1024);
    batch.items[0].gdfs.get_mut("lv_poles").unwrap()["features"][0]["properties"]["retained"] =
        json!(held);
    assert!(batch.wire_body().unwrap().len() < MAX_SAVE_BYTES);
    let body: Value = serde_json::from_slice(&batch.wire_body().unwrap()).unwrap();
    assert_eq!(
        body["transformer_saves"][0]["gdfs"]["lv_poles"]["features"][0]["properties"]["retained"],
        held
    );
    batch.items[0].gdfs.get_mut("lv_poles").unwrap()["features"][0]["properties"]["oversized"] =
        json!("x".repeat(MAX_SAVE_BYTES));
    assert!(batch.wire_body().unwrap().len() > MAX_SAVE_BYTES);
    assert!(batch.validate("project", DeploymentLane::Stable).is_err());
}
#[test]
#[ignore = "requires an explicitly retained native Network save-validation executable"]
fn native_calculated_analysis_is_required_and_bound_and_a_reservation_is_refused() {
    let item = item();
    assert_eq!(
        serde_json::to_value(item.voltage_drop.status).unwrap(),
        "calculated"
    );
    let batch = TransformerSaveBatch {
        project_id: "project".into(),
        items: vec![item.clone()],
    };
    batch.validate("project", DeploymentLane::Stable).unwrap();
    let mut raw = serde_json::to_value(&item).unwrap();
    raw.as_object_mut().unwrap().remove("voltage_drop");
    assert!(serde_json::from_value::<TransformerSaveItem>(raw).is_err());
    for mutate in 0..9 {
        let mut changed = item.clone();
        match mutate {
            0 => changed.voltage_drop.inputs_digest = "0".repeat(64),
            1 => changed.voltage_drop.processed_digest = "0".repeat(64),
            2 => changed.voltage_drop.analysis_inputs_digest = "g".repeat(64),
            3 => changed.voltage_drop.schema = "other".into(),
            4 => {
                changed.process_input.settings.renumber_poles =
                    !changed.process_input.settings.renumber_poles
            }
            5 => {
                changed
                    .config_dfs
                    .insert("project_settings".into(), json!([]));
            }
            6 => {
                changed.gdfs.get_mut("tr").unwrap()["features"][0]["properties"]["changed"] =
                    json!(true)
            }
            7 => changed.process_metadata["voltage_drop"] = analysis_value(&item).unwrap(),
            _ => {
                changed.voltage_drop.status =
                    ds_network_contracts::voltage_drop::AnalysisStatus::Incomplete
            }
        }
        assert!(
            TransformerSaveBatch {
                project_id: "project".into(),
                items: vec![changed]
            }
            .validate("project", DeploymentLane::Stable)
            .is_err(),
            "mutation {mutate}"
        );
    }
    // The fill-in fixture is a reservation an engine older than 2026-10-19
    // returned. Every transformer is calculated now, so the Network save
    // validation refuses to publish it; a reprocess calculates it.
    let reserved = item_with_nature("fill_in");
    assert_eq!(
        serde_json::to_value(reserved.voltage_drop.status).unwrap(),
        "reserved"
    );
    let error = TransformerSaveBatch {
        project_id: "project".into(),
        items: vec![reserved],
    }
    .validate("project", DeploymentLane::Stable)
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "transformer_save_analysis_binding_invalid"
    );
}
#[test]
#[ignore = "requires an explicitly retained native Network save-validation executable"]
fn hydrated_nature_preserves_the_source_fence_but_other_source_changes_fail() {
    let mut item = item();
    item.source_layers.get_mut("tr").unwrap()["features"][0]["properties"]
        .as_object_mut()
        .unwrap()
        .remove("transformer_nature");
    item.source_content_digest = digest(&item.source_layers).unwrap();
    let mut batch = TransformerSaveBatch {
        project_id: "project".into(),
        items: vec![item],
    };
    batch.validate("project", DeploymentLane::Stable).unwrap();
    batch.items[0].process_input.gdfs.get_mut("tr").unwrap()["features"][0]["properties"]
        ["transfo_size"] = json!(50);
    assert!(batch.validate("project", DeploymentLane::Stable).is_err());
}
#[test]
fn oversized_detailed_analysis_is_refused_without_truncation() {
    let mut item = item();
    item.voltage_drop.run.as_mut().unwrap()["report"]["padding"] =
        json!("x".repeat(MAX_SAVE_BYTES));
    assert!(TransformerSaveBatch {
        project_id: "project".into(),
        items: vec![item]
    }
    .validate("project", DeploymentLane::Stable)
    .is_err());
}
#[test]
fn retry_refuses_stale_or_replaced_analysis_even_when_network_is_identical() {
    let item = item();
    let result_digest = digest(&item.gdfs).unwrap();
    for key in [
        "state",
        "schema",
        "method",
        "status",
        "content_digest",
        "inputs_digest",
        "processed_digest",
        "analysis_inputs_digest",
        "analysis_sha256",
    ] {
        let mut meta = analysis_metadata(&item);
        meta[key] = json!("replaced");
        let body = serde_json::to_vec(&json!({"success":true,"data":{"total":1,"found_count":1,"failed_count":0,"results":[{
                "transformer_name":"T1","ok":true,"data":{"layers":item.gdfs,"metadata":{"version":8,"content_digest":result_digest},"voltage_drop_metadata":meta}
            }]}})).unwrap();
        let replaced = crate::transformers::decode_context(
            crate::TransportResponse { status: 200, body },
            MAX_SAVE_BYTES,
            "project",
            "T1",
        )
        .unwrap();
        assert!(
            preflight(&item, &replaced, &result_digest).is_err(),
            "{key}"
        );
    }
}
