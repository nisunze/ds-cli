use ds_command_kernel::{design_config, project_config};
use serde_json::{json, Value};
#[test]
fn captured_typescript_and_new_contract_corpus() {
    let corpus: Value =
        serde_json::from_str(include_str!("fixtures/settings-model-parity.json")).unwrap();
    for case in corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(corpus["contract_cases"].as_array().unwrap())
    {
        let result = design_config::evaluate(&serde_json::to_vec(&case["request"]).unwrap());
        if let Some(error) = case.get("expected_error") {
            assert_eq!(
                result.unwrap_err(),
                error.as_str().unwrap(),
                "{}",
                case["id"]
            );
            continue;
        }
        let result: Value =
            serde_json::from_str(&result.unwrap_or_else(|e| panic!("{}: {e}", case["id"])))
                .unwrap();
        let result = &result["result"];
        let expected = case.get("expected").unwrap_or(&case["expected_ts"]);
        if let Some(key) = case.get("projection").and_then(Value::as_str) {
            assert_eq!(result[key], expected[key], "{}", case["id"])
        } else {
            assert_eq!(result, expected, "{}", case["id"])
        }
    }
    for case in corpus["admission_cases"].as_array().unwrap() {
        let result: Value = serde_json::from_str(
            &project_config::evaluate(&serde_json::to_vec(&case["request"]).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(result["verdict"], case["verdict"], "{}", case["id"]);
    }
}
#[test]
fn closed_requests_and_mutations_refuse_invalid_inputs() {
    for input in [
        json!({"op":"compact","sheet_key":"tags","value":[],"surprise":true}),
        json!({"op":"anything"}),
        json!({"op":"save","document":{"sheets":{}},"sheet_key":"unknown","value":[]}),
    ] {
        assert!(design_config::evaluate(&serde_json::to_vec(&input).unwrap()).is_err());
    }
    assert!(design_config::evaluate(&vec![b' '; design_config::MAX_INPUT_BYTES + 1]).is_err());
    let document = json!({"sheets":{"project_settings":[]}});
    assert!(
        design_config::save_sheet(&document, "project_settings", &json!([{"parameter":"x"}]))
            .is_err()
    );
    assert!(design_config::save_sheet(
        &document,
        "project_settings",
        &json!([{"parameter":"---","value":1}])
    )
    .is_err());
}
#[test]
fn native_projection_pages_rule_rows_and_marks_truncated_cells() {
    let rows: Vec<_> = (0..1000)
        .map(|n| json!({"priority":n,"notes":"x".repeat(400)}))
        .collect();
    let doc = json!({"sheets":{"lv_poles_rules":{"rule_edcl":{"table":rows,"default_material":"wooden"}}}});
    let page =
        design_config::read_sheet(&doc, "lv_poles_rules", Some("rule_edcl"), 30, 10).unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 10);
    assert_eq!(page["items"][0]["priority"], 30);
    assert_eq!(page["more"], 960);
    assert_eq!(page["truncated"], true);
    assert_eq!(page["metadata"]["default_material"], "wooden");
    assert!(design_config::read_sheet(&doc, "lv_poles_rules", Some("missing"), 0, 10).is_err());
    assert!(design_config::read_sheet(&doc, "lv_poles_rules", None, 0, 0).is_err());
}

#[test]
fn semantic_numbers_do_not_erase_distinct_large_integer_identities() {
    assert!(design_config::settings_equal(&json!(1), &json!(1.0)));
    assert!(!design_config::settings_equal(
        &json!(9007199254740992u64),
        &json!(9007199254740993u64)
    ));
    assert!(!design_config::settings_equal(&json!("1"), &json!(1)));
}
