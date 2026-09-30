//! The native default analysis through public discovery and the file CLI.

use ds_grid_engine::{DefaultAnalysisRequest, GridSession};
use serde_json::{Value, json};

mod common;

fn ok(args: &[&str]) -> Value {
    let (envelope, code) = common::json(args);
    assert_eq!(code, 0, "{envelope}");
    envelope["data"].clone()
}

#[test]
fn default_analysis_is_discoverable_with_the_exact_native_parameter_schema() {
    let index = ok(&[
        "dsgrid",
        "describe",
        "--kind",
        "operations",
        "--output",
        "json",
    ]);
    let entry = index["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "analyze_model_defaults")
        .expect("analysis appears in the operation index");
    assert_eq!(entry["effect"], "solve");
    assert_eq!(entry["journaled"], false);

    let descriptor = ok(&[
        "dsgrid",
        "describe",
        "--kind",
        "operations",
        "--id",
        "analyze_model_defaults",
        "--output",
        "json",
    ])["descriptor"]
        .clone();
    let native = ds_grid_engine::descriptor::operation_descriptors()
        .into_iter()
        .find(|op| op.operation_id == "analyze_model_defaults")
        .unwrap();
    assert_eq!(descriptor, serde_json::to_value(native).unwrap());
    assert_eq!(descriptor["params"][0]["name"], "request");
    assert_eq!(
        descriptor["params"][0]["value_type"],
        "DefaultAnalysisRequest"
    );
    assert_eq!(descriptor["params"][0]["required"], true);

    let request_type = ok(&[
        "dsgrid",
        "describe",
        "--kind",
        "types",
        "--id",
        "DefaultAnalysisRequest",
        "--output",
        "json",
    ])["descriptor"]
        .clone();
    let native_type = ds_grid_engine::descriptor::type_descriptors()
        .into_iter()
        .find(|entry| entry.id == "DefaultAnalysisRequest")
        .unwrap();
    assert_eq!(request_type, serde_json::to_value(native_type).unwrap());
    let schema = &request_type["schema"];
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["required"],
        json!(["expected_revision", "expected_engineering_input_root",])
    );
    let properties = schema["properties"].as_object().unwrap();
    assert_eq!(properties.len(), 3);
    assert!(properties.contains_key("expected_revision"));
    assert_eq!(
        properties["expected_engineering_input_root"]["type"],
        "string"
    );
    assert_eq!(properties["max_rows"]["type"], "integer");
    assert_eq!(properties["max_rows"]["minimum"], 1);
    assert_eq!(properties["max_rows"]["maximum"], 10_000);
    let native_default: DefaultAnalysisRequest = serde_json::from_value(json!({
        "expected_revision": "rev:test",
        "expected_engineering_input_root": "test-root",
    }))
    .unwrap();
    assert_eq!(properties["max_rows"]["default"], native_default.max_rows);
}

#[test]
fn default_analysis_cli_returns_native_evidence_with_bounds_and_revision_fences() {
    let model = common::fixture();
    let bytes = std::fs::read(&model).unwrap();
    let package = ds_grid_exchange::unpack(&bytes).unwrap();
    let session = GridSession::open(package.snapshot);
    // The operator obtains both fences from an existing read operation.
    let criteria = ok(&[
        "dsgrid",
        "run",
        "--model",
        &model,
        "--operation",
        "project_criteria_workbench",
        "--output",
        "json",
    ]);
    let request: DefaultAnalysisRequest = serde_json::from_value(json!({
        "expected_revision": criteria["result"]["model_revision"],
        "expected_engineering_input_root": criteria["result"]["engineering_input_root"],
        "max_rows": 2,
    }))
    .unwrap();
    let native = session.analyze_model_defaults(&request).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let params_path = directory.path().join("analysis.json");
    let params = json!({ "request": request });
    std::fs::write(&params_path, serde_json::to_vec(&params).unwrap()).unwrap();
    let args = |limit| {
        [
            "dsgrid",
            "run",
            "--model",
            &model,
            "--operation",
            "analyze_model_defaults",
            "--params",
            params_path.to_str().unwrap(),
            "--limit",
            limit,
            "--output",
            "json",
        ]
    };
    let data = ok(&args("10000"));
    assert_eq!(data["operation"]["result_type"], "DefaultAnalysisReport");
    assert_eq!(data["operation"]["effect"], "solve");
    assert_eq!(data["operation"]["journaled"], false);
    assert_eq!(data["staged"], false);
    assert_eq!(data["persisted"], false);
    assert_eq!(data["source"], criteria["source"]);
    assert_eq!(data["result"], serde_json::to_value(&native).unwrap());
    assert_eq!(
        data["result"]["model_revision"],
        data["source"]["authored_revision"]
    );
    assert!(
        native.cases.total_count > 2,
        "fixture has several bound named cases"
    );
    assert!(native.cases.truncated);

    let bounded = ok(&args("1"));
    assert_eq!(
        bounded["result"]["cases"]["rows"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        bounded["result"]["cases"]["total_count"],
        native.cases.total_count
    );
    assert_eq!(
        bounded["result"]["structure_status_counts"],
        data["result"]["structure_status_counts"]
    );
    let truncation = bounded["more"]["truncated"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["field"] == "result.cases.rows")
        .unwrap();
    assert_eq!(truncation["total"], 2);
    assert_eq!(truncation["shown"], 1);
    assert_eq!(truncation["withheld"], 1);

    for (field, stale, code) in [
        ("expected_revision", "rev:stale", "revision_mismatch"),
        (
            "expected_engineering_input_root",
            "stale-root",
            "engineering_input_root_mismatch",
        ),
    ] {
        let mut params = params.clone();
        params["request"][field] = json!(stale);
        std::fs::write(&params_path, serde_json::to_vec(&params).unwrap()).unwrap();
        let (refused, exit) = common::json(&args("50"));
        assert_ne!(exit, 0);
        assert_eq!(refused["error"]["code"], "operation_failed");
        assert_eq!(refused["error"]["detail"]["refusal"]["code"], code);
        assert_eq!(
            refused["error"]["detail"]["refusal"]["detail"]["expected"],
            stale
        );
        assert!(refused["data"].is_null());
    }
    assert_eq!(
        std::fs::read(&model).unwrap(),
        bytes,
        "source bytes are unchanged"
    );
}
