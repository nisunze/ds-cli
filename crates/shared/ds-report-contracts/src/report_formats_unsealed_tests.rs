//! Regression (24 Oct): a local `--selection` naming a project setup outside
//! the saved selection (Gisagara's A3 beside its saved A0) refused "Prepare
//! printing setup … before selecting it" with a remedy pointing at project
//! settings, which never named the cause. ds-brain seals only the saved
//! selection's setups; the refusal now says so and names what can print.
use super::*;
use serde_json::json;

#[test]
fn a_setup_outside_the_saved_selection_names_the_cause_and_the_printable_setups() {
    let mut saved = crate::printing::default_layout();
    saved.id = "a0-landscape-gisagara-cjic".into();
    let sheets = json!({"project_settings":[{"parameter":"design_export_format","value":["pdf__a0-landscape-gisagara-cjic"]}],
        "printing_setups":[{"id":saved.id,"revision":"a".repeat(64),"layout":saved}]});
    let selection: DesignOutputSelection = serde_json::from_value(json!({
        "schema": OUTPUT_SELECTION_SCHEMA,
        "prints": [{"layout_id": "a3-landscape-gisagara-cjic", "formats": ["pdf"]}]
    }))
    .unwrap();
    let error = selected_plan(&sheets, false, Some(&selection)).unwrap_err();
    assert!(
        error.starts_with("print_setup_not_in_saved_selection: "),
        "{error}"
    );
    assert!(error.contains("a3-landscape-gisagara-cjic"), "{error}");
    assert!(
        error.contains("printable here: a0-landscape-gisagara-cjic"),
        "{error}"
    );
    assert!(error.contains("ds report project outputs set"), "{error}");
    let saved_selection: DesignOutputSelection = serde_json::from_value(json!({
        "schema": OUTPUT_SELECTION_SCHEMA,
        "prints": [{"layout_id": "a0-landscape-gisagara-cjic", "formats": ["pdf"]}]
    }))
    .unwrap();
    assert_eq!(
        selected_plan(&sheets, false, Some(&saved_selection)).unwrap()[0].output_id,
        "pdf__a0-landscape-gisagara-cjic"
    );
}

#[test]
fn without_any_sealed_setup_the_refusal_says_none_can_print() {
    let selection: DesignOutputSelection = serde_json::from_value(json!({
        "schema": OUTPUT_SELECTION_SCHEMA,
        "prints": [{"layout_id": "detail", "formats": ["pdf"]}]
    }))
    .unwrap();
    let error = selected_plan(&json!({}), false, Some(&selection)).unwrap_err();
    assert!(error.contains("printable here: none"), "{error}");
}
