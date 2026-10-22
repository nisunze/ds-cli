use ds_command_kernel::report;
use serde_json::{json, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/report-inventory/held-six-old-legacy-three.json"
    ))
    .unwrap()
}

fn project(mut input: Value) -> Value {
    input["command"] = json!("inventory");
    let result = report::evaluate(&serde_json::to_vec(&input).unwrap()).unwrap();
    serde_json::from_str::<Value>(&result).unwrap()["result"].take()
}

fn refused(mut input: Value) {
    input["command"] = json!("inventory");
    assert!(report::evaluate(&serde_json::to_vec(&input).unwrap()).is_err());
}

fn cloud(input: &Value) -> Value {
    let set = &input["snapshot"]["local_sets"][0];
    let outputs = set["artifacts"].as_array().unwrap().iter().map(|artifact| json!({
        "filename":ds_command_kernel::report_formats::report_filename(set["transformer"].as_str().unwrap(),artifact["format"].as_str().unwrap()).unwrap(), "output_id":artifact["outputId"], "format":artifact["format"], "content_type":artifact["contentType"],
        "sha256":artifact["sha256"], "size_bytes":artifact["sizeBytes"],
        "paper_size":artifact.get("paperSize"),"presentation":artifact.get("presentation")
    })).collect::<Vec<_>>();
    let origins = outputs.iter().map(|output| (output["output_id"].as_str().unwrap().to_string(), json!({
        "work_id":"verified-native-work", "input_base_fingerprint":set["input_base_fingerprint"],
        "engine_version":set["engine_release"], "engine_build_manifest_sha256":set["engine_build_manifest_sha256"]
    }))).collect::<serde_json::Map<String,Value>>();
    json!({"owner":set["owner"],"project":set["project"],"head":{
        "work_id":"verified-native-work","engine":"network_reporter","operation":set["operation"],
        "variant":"default","input_base_fingerprint":set["input_base_fingerprint"],"head_revision":42,
        "updated_at":"2026-09-29T01:00:00Z","engine_version":set["engine_release"],
        "engine_build_manifest_sha256":set["engine_build_manifest_sha256"],"outputs":outputs,"output_origins":origins
    }})
}

#[test]
fn six_held_outputs_survive_old_cloud_status_without_timestamp_regression() {
    let input = fixture();
    let before = input.clone();
    let output = project(input.clone());
    assert_eq!(input, before);
    assert_eq!(output["schema"], "ds.report.inventory/v1");
    assert_eq!(output["outputs"].as_array().unwrap().len(), 6);
    assert_eq!(output["local_output_count"], 6);
    assert_eq!(output["published_output_count"], 0);
    assert_eq!(output["active_client_publish_id"], "publish-sep29");
    assert_eq!(output["generated_at_ms"], 1790640000000_u64);
    assert_eq!(output["legacy_cloud"]["reported_at_ms"], 1781481600000_u64);
    assert_eq!(output["missing_outputs"], json!([]));
    for file in output["outputs"].as_array().unwrap() {
        assert_eq!(file["publication_phase"], "queued");
        assert_eq!(file["client_publish_id"], "publish-sep29");
        assert!(file["cloud_binding"].is_null());
    }
}

#[test]
fn published_requires_the_exact_native_pins_and_current_per_output_origins() {
    let mut input = fixture();
    input["snapshot"]["sync_artifacts"][0]["state"] = json!("published");
    for file in project(input.clone())["outputs"].as_array().unwrap() {
        assert_eq!(file["publication_phase"], "acknowledgement_unverified");
    }
    input["snapshot"]["cloud_head"] = cloud(&input);
    assert!(project(input.clone())["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|file| file["publication_phase"] == "acknowledgement_unverified"));
    input["snapshot"]["sync_artifacts"][0]["acknowledged_work_id"] = json!("verified-native-work");
    for file in project(input.clone())["outputs"].as_array().unwrap() {
        assert_eq!(file["publication_phase"], "published");
        assert_eq!(file["cloud_binding"]["head_revision"], 42);
        assert_eq!(file["cloud_binding"]["work_id"], "verified-native-work");
    }
    let mut missing_origin = input.clone();
    missing_origin["snapshot"]["cloud_head"]["head"]
        .as_object_mut()
        .unwrap()
        .remove("output_origins");
    assert!(project(missing_origin)["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|file| file["publication_phase"] == "acknowledgement_unverified"));
    let mut another_run = input.clone();
    another_run["snapshot"]["sync_artifacts"][0]["acknowledged_work_id"] =
        json!("another-acknowledged-work");
    assert!(project(another_run)["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|file| file["publication_phase"] == "acknowledgement_unverified"));
    let mut changed_bytes = input.clone();
    changed_bytes["snapshot"]["cloud_head"]["head"]["outputs"][0]["sha256"] = json!("e".repeat(64));
    let output = project(changed_bytes);
    assert_eq!(
        output["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["output_id"] == "shp")
            .unwrap()["publication_phase"],
        "acknowledgement_unverified"
    );
    let mut wrong_source = input;
    let head = &mut wrong_source["snapshot"]["cloud_head"]["head"];
    head["input_base_fingerprint"] = json!("e".repeat(64));
    for origin in head["output_origins"].as_object_mut().unwrap().values_mut() {
        origin["input_base_fingerprint"] = json!("e".repeat(64));
    }
    assert!(project(wrong_source)["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|file| file["publication_phase"] == "acknowledgement_unverified"));
}

#[test]
fn partial_iterations_keep_each_output_in_its_own_producing_generation() {
    let mut input = fixture();
    let mut old = input["snapshot"]["local_sets"][0].clone();
    old["batch_id"] = json!("f".repeat(64));
    old["client_publish_id"] = json!("publish-old");
    old["client_run_id"] = json!("report-old");
    // Timestamp order deliberately disagrees: native catalog order wins.
    old["generated_at_ms"] = json!(1790640009000_u64);
    input["snapshot"]["local_sets"][0]["artifacts"]
        .as_array_mut()
        .unwrap()
        .retain(|artifact| artifact["outputId"].as_str().unwrap().starts_with("pdf__"));
    input["snapshot"]["sync_artifacts"][0]["outputs"]
        .as_array_mut()
        .unwrap()
        .retain(|output| output["output_id"].as_str().unwrap().starts_with("pdf__"));
    input["snapshot"]["local_sets"]
        .as_array_mut()
        .unwrap()
        .push(old);
    let result = project(input);
    assert_eq!(result["outputs"].as_array().unwrap().len(), 6);
    assert_eq!(result["generated_at_ms"], 1790640000000_u64);
    for output in result["outputs"].as_array().unwrap() {
        assert_eq!(
            output["client_publish_id"],
            if output["output_id"].as_str().unwrap().starts_with("pdf__") {
                "publish-sep29"
            } else {
                "publish-old"
            }
        );
    }
}

#[test]
fn held_inventory_read_failure_retains_files_without_claiming_fresh_observation() {
    let mut input = fixture();
    input["snapshot"]["local_read"] =
        json!({"state":"held","error":"native inventory read failed"});
    input["snapshot"]["cloud_read"] =
        json!({"state":"unavailable","error":"remote status timed out"});
    let result = project(input);
    assert_eq!(result["outputs"].as_array().unwrap().len(), 6);
    assert_eq!(result["status"], "partial_observation");
    assert_eq!(result["complete_inventory_observed"], false);
    assert_eq!(result["generated_at_ms"], 1790640000000_u64);
}

#[test]
fn ambiguous_native_generations_do_not_choose_a_timestamp_winner() {
    let mut input = fixture();
    let mut old_set = input["snapshot"]["local_sets"][0].clone();
    let mut old_sync = input["snapshot"]["sync_artifacts"][0].clone();
    for row in [&mut old_set, &mut old_sync] {
        row["batch_id"] = json!("f".repeat(64));
        row["client_publish_id"] = json!("publish-old");
    }
    old_set["client_run_id"] = json!("report-old");
    old_set["generated_at_ms"] = json!(1790640009000_u64);
    old_set["artifacts"]
        .as_array_mut()
        .unwrap()
        .retain(|artifact| artifact["outputId"] == "xlsx");
    old_sync["outputs"]
        .as_array_mut()
        .unwrap()
        .retain(|output| output["output_id"] == "xlsx");
    input["snapshot"]["local_sets"][0]["artifacts"]
        .as_array_mut()
        .unwrap()
        .retain(|artifact| artifact["outputId"] != "xlsx");
    input["snapshot"]["sync_artifacts"][0]["outputs"]
        .as_array_mut()
        .unwrap()
        .retain(|output| output["output_id"] != "xlsx");
    input["snapshot"]["local_sets"]
        .as_array_mut()
        .unwrap()
        .push(old_set);
    input["snapshot"]["sync_artifacts"]
        .as_array_mut()
        .unwrap()
        .push(old_sync);
    let result = project(input);
    assert_eq!(result["outputs"].as_array().unwrap().len(), 6);
    assert_eq!(result["active_generation_ambiguous"], true);
    assert_eq!(result["status"], "ambiguous_generation");
    assert!(result["active_client_publish_id"].is_null());
    assert!(result["generated_at_ms"].is_null());
}

#[test]
fn changed_source_missing_outputs_and_refused_publication_remain_explicit() {
    let mut input = fixture();
    input["snapshot"]["expected_input_base_fingerprint"] = json!("e".repeat(64));
    input["snapshot"]["sync_artifacts"][0]["state"] = json!("refused");
    input["snapshot"]["sync_artifacts"][0]["reason"] = json!("service refused this run");
    let result = project(input.clone());
    assert_eq!(result["status"], "stale");
    assert_eq!(result["generated_at_ms"], 1790640000000_u64);
    assert!(result["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|output| output["publication_phase"] == "refused"
            && output["source_state"] == "changed"));
    input["snapshot"]["expected_input_base_fingerprint"] = json!("d".repeat(64));
    input["snapshot"]["local_sets"][0]["artifacts"]
        .as_array_mut()
        .unwrap()
        .pop();
    let result = project(input);
    assert_eq!(result["status"], "evidence_error");
    assert_eq!(result["missing_outputs"], json!(["pdf__a0l"]));
}

#[test]
fn foreign_scope_duplicate_identity_and_unbound_native_pins_never_launder_success() {
    for (field, value) in [
        ("owner", "another-user"),
        ("project", "another-project"),
        ("transformer", "another_transformer"),
        ("variant", "another-variant"),
    ] {
        let mut input = fixture();
        input["snapshot"]["local_sets"][0][field] = json!(value);
        refused(input);
    }
    let mut input = fixture();
    let duplicate = input["snapshot"]["local_sets"][0].clone();
    input["snapshot"]["local_sets"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    refused(input);
    let mut input = fixture();
    input["snapshot"]["sync_artifacts"][0]["batch_id"] = json!("e".repeat(64));
    let result = project(input);
    assert_eq!(result["status"], "evidence_error");
    assert!(result["generated_at_ms"].is_null());
    for target in ["sync_artifacts", "cloud_head"] {
        let mut input = fixture();
        if target == "cloud_head" {
            input["snapshot"][target] = cloud(&input);
            input["snapshot"][target]["owner"] = json!("another-user");
        } else {
            input["snapshot"][target][0]["project"] = json!("another-project");
        }
        refused(input);
    }
}

#[test]
fn normal_held_cloud_read_is_usable_without_fabricating_an_error_or_freshness() {
    let mut input = fixture();
    input["snapshot"]["cloud_head"] = cloud(&input);
    input["snapshot"]["local_sets"] = json!([]);
    input["snapshot"]["sync_artifacts"] = json!([]);
    input["snapshot"]["cloud_read"] = json!({"state":"held","error":null});
    let output = project(input.clone());
    assert_eq!(output["outputs"].as_array().unwrap().len(), 6);
    assert_eq!(output["cloud_read"]["state"], "held");
    assert!(output["cloud_read"]["error"].is_null());
    for artifact in output["outputs"].as_array().unwrap() {
        assert_eq!(artifact["publication_phase"], "held_cloud_observation");
        assert!(artifact["generated_at_ms"].is_null());
        assert!(artifact["local"].is_null());
    }
    input["snapshot"]["cloud_read"]["state"] = json!("unavailable");
    refused(input);
}

#[test]
fn the_sealed_combined_report_is_an_inventory_identity_but_other_aggregates_are_not() {
    let combined = |name: &str| {
        let mut input = fixture();
        let operation = format!("export-{name}");
        input["snapshot"]["transformer"] = json!(name);
        input["snapshot"]["local_sets"][0]["transformer"] = json!(name);
        input["snapshot"]["local_sets"][0]["operation"] = json!(operation);
        input["snapshot"]["sync_artifacts"][0]["operation"] = json!(operation);
        input
    };
    let output = project(combined("combined_transformer"));
    assert_eq!(output["transformer"], "combined_transformer");
    assert_eq!(output["local_output_count"], 6);
    assert_eq!(output["active_client_publish_id"], "publish-sep29");
    assert_eq!(output["outputs"].as_array().unwrap().len(), 6);
    for reserved in [
        "collisions",
        "all_transformers",
        "combined_transformers",
        "__all__",
    ] {
        refused(combined(reserved));
    }
}
