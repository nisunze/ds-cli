//! The Kernel's printing command evaluator over the shared print contract
//! (ds-report-contracts). These tests moved out of the contract's modules
//! because they drive `printing::evaluate`, which the Kernel owns.
use super::catalogue::*;

use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn fixture() -> (Layout, BTreeMap<String, Value>) {
    let mut layout = super::default_layout();
    let mut element =
        super::elements::new_element(super::ElementKind::Table, "customers-table".into(), "");
    element.table = Some(TableBinding {
        presentation: TablePresentation::Plain,
        layer: "customers".into(),
        columns: vec!["house_number".into(), "names".into()],
        headings: vec!["House".into(), "Customer".into()],
        widths: vec![2., 5.],
        min_widths_mm: vec![7., 10.],
        max_widths_mm: vec![17., 45.],
        ..Default::default()
    });
    layout.elements.push(element);
    let layers = BTreeMap::from([(
        "customers".into(),
        json!({"type":"FeatureCollection","features":[{"properties":{"house_number":0,"names":"Name","meter_number":"M-123","phone_number":"123","empty":" ","flag":false}}]}),
    )]);
    (layout, layers)
}

#[test]
fn classification_edit_preserves_body_and_clear_restores_legacy_shape() {
    let original = serde_json::to_value(super::default_layout()).unwrap();
    assert!(original.get("classification").is_none());
    let run = |request| -> Result<serde_json::Value, String> {
        let bytes = serde_json::to_vec(&request).unwrap();
        let output = super::evaluate(&bytes)?;
        serde_json::from_str(&output).map_err(|e| e.to_string())
    };
    let classified = run(json!({"op":"set_classification","layout":original,"classification":{"purpose":"lv_transformer","role":"network"}})).unwrap();
    let mut expected = original.clone();
    expected["classification"] = json!({"purpose":"lv_transformer","role":"network"});
    assert_eq!(classified["layout"], expected);
    assert_eq!(
        run(json!({"op":"set_classification","layout":classified["layout"],"classification":null}))
            .unwrap()["layout"],
        original
    );
    assert!(run(json!({"op":"set_classification","layout":original,"classification":{"purpose":"a4","role":"report"}})).is_err());
    assert!(run(json!({"op":"set_classification","layout":original,"classification":{"purpose":"project","role":"guess","paper":"A0"}})).is_err());
}

#[test]
fn mv_sheets_hold_only_the_a3_landscape_plan_and_profile_sheet() {
    let rows = serde_json::from_value(json!([
            {"id":"district","name":"Nyaruguru MV district map","paper":{"size":"A3","orientation":"landscape"},"classification":{"purpose":"mv_model","role":"network"}},
            {"id":"portrait","name":"Nyamagabe MV district map","paper":{"size":"A3","orientation":"portrait"},"classification":{"purpose":"mv_model","role":"plan_profile"}},
            {"id":"pp","name":"MV plan and profile","paper":{"size":"A3","orientation":"landscape"},"classification":{"purpose":"mv_model","role":"plan_profile"}},
            {"id":"overview","name":"Project overview","paper":{"size":"A3","orientation":"landscape"},"classification":{"purpose":"project","role":"overview"}}
        ]))
        .unwrap();
    let out = serde_json::to_value(project(rows, None, None).unwrap()).unwrap();
    let mv = &out["groups"][0];
    assert_eq!(mv["purpose"], "mv_model");
    assert_eq!(mv["roles"].as_array().unwrap().len(), 1);
    assert_eq!(mv["roles"][0]["role"], "plan_profile");
    let sheets = mv["roles"][0]["templates"].as_array().unwrap();
    assert_eq!(sheets.len(), 1);
    assert_eq!(sheets[0]["id"], "pp");
    assert_eq!(sheets[0]["subject"], "mv_models");
    assert!(sheets[0].get("mismatch").is_none());
    assert_eq!(
        out["groups"][1]["roles"][0]["templates"][0]["subject"],
        "extent"
    );
    // Stored mismatches stay listed, flagged, as Unclassified.
    let unclassified = &out["groups"][2];
    assert_eq!(unclassified["label"], "Unclassified");
    let flagged = unclassified["roles"][0]["templates"].as_array().unwrap();
    assert_eq!(flagged.len(), 2);
    for template in flagged {
        assert_eq!(
            template["mismatch"]["code"],
            "print_classification_mv_requires_plan_profile"
        );
        assert_eq!(template["mismatch"]["stored"]["purpose"], "mv_model");
        assert!(template.get("subject").is_none());
    }
    // Authoring admits MV Sheets only for a layout carrying an MV setup.
    let set = |layout: &Value, classification: Value| {
        super::evaluate(
            &serde_json::to_vec(
                &json!({"op":"set_classification","layout":layout,"classification":classification}),
            )
            .unwrap(),
        )
    };
    let plain = serde_json::to_value(super::default_layout()).unwrap();
    for role in ["network", "plan_profile"] {
        assert!(set(&plain, json!({"purpose":"mv_model","role":role}))
            .unwrap_err()
            .starts_with("print_classification_invalid: MV Sheets hold only"));
    }
    assert!(set(&plain, json!({"purpose":"project","role":"overview"})).is_ok());
    let mv_layout: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/mv-global-canonical.json"
    ))
    .unwrap();
    let classified: Value = serde_json::from_str(
        &set(
            &mv_layout,
            json!({"purpose":"mv_model","role":"plan_profile"}),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        classified["layout"]["classification"],
        json!({"purpose":"mv_model","role":"plan_profile"})
    );
    assert!(set(&mv_layout, json!({"purpose":"mv_model","role":"network"})).is_err());
}

#[test]
fn front_matter_admits_only_front_matter_roles() {
    let layout = serde_json::to_value(super::default_layout()).unwrap();
    let run = |classification: Value| {
        super::evaluate(
            &serde_json::to_vec(
                &json!({"op":"set_classification","layout":layout,"classification":classification}),
            )
            .unwrap(),
        )
    };
    assert!(run(json!({"purpose":"front_matter","role":"key_map"})).is_ok());
    assert!(run(json!({"purpose":"front_matter","role":"plan_profile"}))
        .unwrap_err()
        .starts_with("print_classification_invalid"));
}

#[test]
fn requests_author_the_same_policy_and_prepare_only_selected_draft_context() {
    use crate::printing::{context, default_layout, evaluate};
    use serde_json::{json, Value};
    let layout = default_layout();
    let run = |request: Value| -> Result<Value, String> {
        serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap())?)
            .map_err(|e| e.to_string())
    };
    let result = run(json!({"op":"set_fit_policy","layout":layout,"network_fit":{"source_layers":["poles","lv_lines"],"clearance_mm":3},"scale_limits":{"min_denominator":1000,"max_denominator":10000}})).unwrap();
    assert_eq!(
        result["layout"]["network_fit"]["clearance_mm"].as_f64(),
        Some(3.)
    );
    assert!(run(json!({"op":"set_fit_policy","layout":layout,"network_fit":null,"scale_limits":{"min_denominator":10000,"max_denominator":1000}})).is_err());
    let selected =
        context::toggle(layout, context::derived_layers().remove(2), true, None).unwrap();
    for online in [false, true] {
        let result =
            run(json!({"op":"preview_context_preparation","layout":selected,"online":online}))
                .unwrap();
        assert_eq!(result["acquire_missing"], false);
        assert_eq!(result["contexts"].as_array().unwrap().len(), 1);
        assert_eq!(
            result["contexts"][0]["source"]["kind"],
            "elevation_contours"
        );
    }
}

#[test]
fn requests_expose_the_same_column_policy_and_validated_layout() {
    let (layout, layers) = fixture();
    let request = json!({"op":"table_columns","layout":layout,"element_id":"customers-table","layers":layers});
    let result: Value =
        serde_json::from_str(&super::evaluate(&serde_json::to_vec(&request).unwrap()).unwrap())
            .unwrap();
    assert_eq!(result["phase"], "design");
    assert!(result["columns"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["key"] == "meter_number" && c["reason"] == "as_built_only"));
    let request = json!({"op":"table_column_selection","layout":layout,"element_id":"customers-table","layers":layers,"columns":["names"]});
    let result: Value =
        serde_json::from_str(&super::evaluate(&serde_json::to_vec(&request).unwrap()).unwrap())
            .unwrap();
    let selected: Layout = serde_json::from_value(result["layout"].clone()).unwrap();
    assert_eq!(
        selected
            .elements
            .last()
            .unwrap()
            .table
            .as_ref()
            .unwrap()
            .columns,
        ["names"]
    );
    super::validate(&selected).unwrap();
}
