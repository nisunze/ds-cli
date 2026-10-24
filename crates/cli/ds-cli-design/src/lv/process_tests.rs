use super::*;

#[test]
fn contract_is_offline_local_file_compute() {
    assert_eq!(COMMAND.authority, Authority::None);
    assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
    assert_eq!(COMMAND.path, ["design", "lv", "process"]);
}

fn execute(input: &Path, output: &Path) -> Result<Value, Failure> {
    let parsed = ds_cli_contract::parse(
        &COMMAND,
        &[
            "--input".into(),
            input.to_string_lossy().into_owned(),
            "--out".into(),
            output.to_string_lossy().into_owned(),
        ],
    )
    .unwrap();
    run(
        &parsed,
        &Context {
            confirmed: false,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        },
    )
}

#[test]
fn missing_or_malformed_sources_refuse_without_an_engine_or_artifact() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("request.json");
    let output = directory.path().join("result.json");
    assert_eq!(
        execute(&source, &output).unwrap_err().code(),
        "fast_lv_source_not_found"
    );
    std::fs::write(&source, b"{\"schema\":\"unrecognized\",\"jobs\":[]}").unwrap();
    assert_eq!(
        execute(&source, &output).unwrap_err().code(),
        "fast_lv_schema_unsupported"
    );
    assert!(!output.exists());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
#[ignore = "requires an explicitly selected compatible Network executable"]
fn actual_network_process_retains_historical_cli_bytes_and_ordered_failures() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("request.json");
    let output = directory.path().join("result.json");
    let request = include_bytes!(
        "../../tests/fixtures/native-engine-boundary/v1/mixed.request.json"
    );
    let golden = include_bytes!(
        "../../tests/fixtures/native-engine-boundary/v1/mixed.insertion.result.json"
    );
    std::fs::write(&source, request).unwrap();
    let receipt = execute(&source, &output).unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), request);
    assert_eq!(std::fs::read(&output).unwrap(), golden);
    assert_eq!(receipt["input_sha256"], sha256(request));
    assert_eq!(receipt["result_sha256"], sha256(golden));
    assert_eq!(receipt["jobs"], 2);
    assert_eq!(receipt["succeeded"], 1);
    assert_eq!(receipt["failed"], 1);
    assert_eq!(receipt["results"][0]["transformer_name"], "T_success");
    assert_eq!(receipt["results"][1]["transformer_name"], "T_failure");
    assert_eq!(receipt["engine_identity"]["jsonObjectOrder"], "insertion");
    assert!(
        receipt["engine_identity"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "lv_processing_v1")
    );
    let executable =
        std::fs::read(std::env::var_os("DS_NETWORK_ENGINE_BIN").expect("explicit actual engine"))
            .unwrap();
    assert_eq!(receipt["executable_sha256"], sha256(&executable));
    assert_eq!(receipt["development_override"], true);
    assert_eq!(
        std::fs::read_dir(directory.path()).unwrap().count(),
        2,
        "private staging is removed"
    );
}

#[test]
fn renderer_never_projects_full_layers_or_errors() {
    let text = render(&json!({
        "jobs": 2,
        "succeeded": 1,
        "failed": 1,
        "out": "result.json",
        "result_sha256": "abc",
        "results": [{ "transformer_name": "T1", "ok": false }],
    }));
    assert!(text.contains("1 succeeded, 1 failed"));
    assert!(!text.contains("T1"));
    assert!(!text.contains("gdfs"));
}

#[test]
#[ignore = "requires an explicitly selected compatible Network executable"]
fn approved_rows_a_job_would_change_are_kept_and_the_analysis_still_returns() {
    let dir = tempfile::tempdir().unwrap();
    let input_path = dir.path().join("request.json");
    let output_path = dir.path().join("result.json");
    let mut source: Value = serde_json::from_slice(include_bytes!(
        "../../../../kernel/ds-client-core/tests/fixtures/native-lv-save-request.json"
    )).unwrap();
    source["jobs"][0]["gdfs"]["lv_poles"]["features"] = json!([{
        "type":"Feature", "id":"approved-missing-pole", "geometry":null,
        "properties":{"drafting_status":"approved", "struct_type":"manual"}
    }]);
    std::fs::write(&input_path, serde_json::to_vec(&source).unwrap()).unwrap();
    let parsed = ds_cli_contract::parse(
        &COMMAND,
        &[
            "--input".into(),
            input_path.to_string_lossy().into_owned(),
            "--out".into(),
            output_path.to_string_lossy().into_owned(),
        ],
    )
    .unwrap();
    let receipt = run(
        &parsed,
        &Context {
            confirmed: false,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        },
    )
    .unwrap();
    assert_eq!(receipt["failed"], 0);
    let row = &receipt["results"][0];
    assert!(row["error_code"].is_null());
    assert_eq!(
        row["preservation"]["effective_settings"]["calculate_voltage_drop"],
        true
    );
    let result: Value = serde_json::from_slice(&std::fs::read(output_path).unwrap()).unwrap();
    assert_eq!(result["jobs"][0]["preservation"], row["preservation"]);
    let output = &result["jobs"][0]["output"];
    let pole = &output["gdfs"]["lv_poles"]["features"][0];
    assert_eq!(pole["id"], "approved-missing-pole");
    assert_eq!(pole["properties"]["struct_type"], "manual");
    assert!(output["voltage_drop"]["status"].is_string());
    assert!(
        output["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| {
                warning["code"] == "approved_network_kept"
                    && warning["message"]
                        .as_str()
                        .unwrap()
                        .contains("lv_poles: @missing 1")
            })
    );
}

#[test]
#[ignore = "requires an explicitly selected compatible Network executable"]
fn a_supplied_network_kept_as_drawn_comes_back_row_for_row() {
    let dir = tempfile::tempdir().unwrap();
    let input_path = dir.path().join("request.json");
    let output_path = dir.path().join("result.json");
    let mut source: Value = serde_json::from_slice(include_bytes!(
        "../../../../kernel/ds-client-core/tests/fixtures/native-lv-save-request.json"
    )).unwrap();
    source["jobs"][0]["settings"]["keep_network_as_drawn"] = json!(true);
    std::fs::write(&input_path, serde_json::to_vec(&source).unwrap()).unwrap();
    let parsed = ds_cli_contract::parse(
        &COMMAND,
        &[
            "--input".into(),
            input_path.to_string_lossy().into_owned(),
            "--out".into(),
            output_path.to_string_lossy().into_owned(),
        ],
    )
    .unwrap();
    let receipt = run(
        &parsed,
        &Context {
            confirmed: false,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        },
    )
    .unwrap();
    assert_eq!(receipt["failed"], 0, "{receipt}");
    let result: Value = serde_json::from_slice(&std::fs::read(output_path).unwrap()).unwrap();
    let output = &result["jobs"][0]["output"];
    assert!(
        output["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["code"] == "network_kept_as_drawn"),
        "{}",
        output["warnings"]
    );
    for (layer, supplied) in source["jobs"][0]["gdfs"].as_object().unwrap() {
        let supplied = supplied["features"].as_array().unwrap();
        let kept = output["gdfs"][layer]["features"].as_array().unwrap();
        assert_eq!(kept.len(), supplied.len(), "{layer}");
        for (row, authored) in kept.iter().zip(supplied) {
            assert_eq!(row["id"], authored["id"], "{layer}");
            assert_eq!(row["geometry"], authored["geometry"], "{layer}");
        }
    }
}

fn execute_page(input: &Path, output: &Path, project: &str) -> Result<Value, Failure> {
    let parsed = ds_cli_contract::parse(
        &COMMAND,
        &[
            "--input".into(),
            input.to_string_lossy().into_owned(),
            "--out".into(),
            output.to_string_lossy().into_owned(),
            "--project".into(),
            project.into(),
        ],
    )
    .unwrap();
    run(
        &parsed,
        &Context {
            confirmed: false,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        },
    )
}

#[test]
fn a_page_batch_that_is_malformed_or_names_another_project_refuses_before_any_engine() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("batch.json");
    let output = directory.path().join("result.json");
    std::fs::write(&source, br#"{"jobs":"not-a-list"}"#).unwrap();
    let malformed = execute_page(&source, &output, "example_rwanda").unwrap_err();
    assert_eq!(malformed.code(), "fast_lv_input_invalid");
    let crossed = json!({"jobs": [{"ds_project": "example_tchad", "transformer_name": "tx_a",
        "gdfs": {}, "settings": {}}]});
    std::fs::write(&source, serde_json::to_vec(&crossed).unwrap()).unwrap();
    let refused = execute_page(&source, &output, "example_rwanda").unwrap_err();
    assert_eq!(refused.code(), "fast_lv_input_invalid");
    assert!(refused.to_string().contains("--project"), "{refused}");
    assert!(!output.exists());
}

fn status(name: &str, ok: bool, error: Option<&str>) -> Value {
    json!({"transformer_name": name, "ok": ok, "error_code": null, "error": error})
}

#[test]
fn a_batch_where_every_job_failed_is_a_failure_that_names_the_cause_and_keeps_the_receipt() {
    let receipt = json!({
        "out": "r.json", "jobs": 2, "succeeded": 0, "failed": 2, "warning_count": 2,
        "results": [
            status("a", false, Some("project_settings.sc_real_length is required; the network_config template is outdated")),
            status("b", false, Some("other")),
        ],
    });
    let failure = all_jobs_failed(&receipt);
    assert_eq!(failure.code(), "lv_process_all_jobs_failed");
    assert!(failure.message().contains("all 2 job(s)"), "{failure}");
    assert!(
        failure.remedy_text().unwrap().contains("--project-config"),
        "{failure}"
    );
    assert_eq!(failure.detail_value().unwrap()["out"], "r.json");
    assert_eq!(
        failure.detail_value().unwrap()["results"][1]["error"],
        "other"
    );
    assert!(
        COMMAND
            .refusals
            .iter()
            .any(|refusal| refusal.code == "lv_process_all_jobs_failed")
    );
}

#[test]
fn an_unrelated_failure_gets_the_generic_remedy() {
    let receipt = json!({"jobs": 1, "results": [status("a", false, Some("boom"))]});
    let remedy = all_jobs_failed(&receipt).remedy_text().unwrap().to_owned();
    assert!(remedy.contains("detail.results"), "{remedy}");
}
