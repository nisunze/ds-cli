use super::*;
use crate::report_export::{sha256_hex, INPUT_RECEIPT_SCHEMA};

fn binding() -> Binding {
    let value: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/governed-renderer-defaults.json"
    ))
    .unwrap();
    let document = json!({"type":"line","paint":{},"layout":{},
        "metadata":{"print_renderer_defaults":value}});
    Binding {
        key: renderer_defaults::key(),
        style_ref: "print_context/renderer_defaults_print".into(),
        revision_id: "a".repeat(64),
        content_sha256: crate::style_plan::style_digest(&document),
        scope: "global".into(),
        document,
        presentation_tier: None,
    }
}

/// The raw process sheets `design.lv.project-export --project-config` writes.
fn config() -> Value {
    json!({
        "printing_context": {"project_id": "project"},
        "printing_a4": {"schema": "ds.print-a4-document/v1", "project_id": "project",
            "revision_id": "r1", "content_sha256": "c".repeat(64), "html": "<html></html>",
            "status_layout": {}},
        "printing_renderer_defaults": binding(),
        "project_settings": [{"parameter": "report_locale", "value": "en"}],
    })
}

/// The server seal beside it: the same sheets, the caption and more.
fn receipt(config: &Value, label: &str) -> InputReceipt {
    let mut sheets = config.clone();
    sheets["printing_context"]["project"] = json!(label);
    sheets["printing_styles"] = json!({});
    let sheets_json = sheets.to_string();
    InputReceipt {
        local_print_recipe: None,
        schema: INPUT_RECEIPT_SCHEMA,
        country: "Rwanda".into(),
        sheets_sha256: sha256_hex(sheets_json.as_bytes()),
        sheets_json,
        reference_semantic_sha256: "d".repeat(64),
    }
}

#[test]
fn a_sealed_capture_decides_caption_policy_format_and_the_exact_request() {
    let config = config();
    let decided = capture(&config, &receipt(&config, "Rwanda Template")).unwrap();
    assert_eq!(decided.project, "project");
    assert_eq!(decided.project_label, "Rwanda Template");
    assert_eq!(decided.report_format, "extended");
    assert_eq!(
        serde_json::to_value(&decided.renderer_defaults).unwrap(),
        serde_json::to_value(binding()).unwrap()
    );
    let request = request(
        &decided,
        &config,
        "agahehe",
        "/out/agahehe.json",
        &"e".repeat(64),
        "/out/agahehe.pdf",
    );
    assert_eq!(
        request,
        json!({"schema": RENDER_REQUEST_SCHEMA, "network_config": config,
            "renderer_defaults": binding(), "source_document": "/out/agahehe.json",
            "source_sha256": "e".repeat(64), "report_format": "extended",
            "transformer": "agahehe", "project_label": "Rwanda Template",
            "out_pdf": "/out/agahehe.pdf"})
    );
}

#[test]
fn the_project_report_format_choice_reaches_the_request() {
    let mut config = config();
    config["project_settings"] = json!([{"parameter": "design_export_format", "value": {
        "schema": "ds.design-output-selection/v1", "tabular": ["voltage_drop"],
        "voltage_drop_report": "brief"}}]);
    let decided = capture(&config, &receipt(&config, "Gisagara, CJIC")).unwrap();
    assert_eq!(decided.report_format, "brief");
    assert_eq!(decided.project_label, "Gisagara, CJIC");
}

#[test]
fn an_unsealed_foreign_or_incomplete_capture_refuses_before_printing() {
    let config = config();
    let sealed = receipt(&config, "Rwanda Template");
    // A process-only export has no printing capture.
    assert!(capture(&json!({}), &sealed)
        .unwrap_err()
        .contains("--project-config"));
    // A sheet that moved after the seal, or a receipt sealed elsewhere.
    let mut moved = config.clone();
    moved["project_settings"][0]["value"] = json!("fr");
    assert!(capture(&moved, &sealed)
        .unwrap_err()
        .contains("not sealed with these process sheets"));
    let mut foreign = config.clone();
    foreign["printing_context"]["project_id"] = json!("other");
    foreign["printing_a4"]["project_id"] = json!("other");
    assert!(capture(&foreign, &sealed).is_err());
    // Tampered seal bytes, a caption equal to the id, a missing A4 document.
    let mut tampered = sealed.clone();
    tampered.sheets_json.push(' ');
    assert!(capture(&config, &tampered).is_err());
    assert!(capture(&config, &receipt(&config, "project")).is_err());
    let mut no_a4 = config.clone();
    no_a4.as_object_mut().unwrap().remove("printing_a4");
    assert!(capture(&no_a4, &receipt(&no_a4, "Rwanda Template"))
        .unwrap_err()
        .contains("governed A4"));
    // The renderer policy must be the complete governed binding.
    let mut no_policy = config.clone();
    no_policy
        .as_object_mut()
        .unwrap()
        .remove("printing_renderer_defaults");
    assert!(capture(&no_policy, &receipt(&no_policy, "Rwanda Template"))
        .unwrap_err()
        .starts_with("print_renderer_defaults_missing"));
}

#[test]
fn non_ascii_captions_survive_into_the_request_unchanged() {
    let config = config();
    let label = "Gisagara – Ndora 15° é × ≤ ²";
    let decided = capture(&config, &receipt(&config, label)).unwrap();
    let request = request(&decided, &config, "t", "/s.json", &"e".repeat(64), "/o.pdf");
    let bytes = serde_json::to_vec(&request).unwrap();
    let decoded: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded["project_label"], label);
}
