//! Preview receipts planned and requested through the Kernel's export plan.
//! The preview recipe is the shared contract (ds-report-contracts); the plan,
//! engine request and publication facts are the Kernel's.
use super::preview::*;
use super::*;
use crate::report_formats::PrintArtifactFormat;

fn source(setups: Value) -> InputReceipt {
    let sheets = json!({
        "printing_setups": setups,
        "project_settings": [{"parameter":"design_export_format","value":["xlsx","shp"]}],
        "pole_types": [{"height":9,"material":"steel"}],
    });
    let bytes = serde_json::to_string(&sheets).unwrap();
    InputReceipt {
        local_print_recipe: None,
        schema: 1,
        country: "Rwanda".into(),
        sheets_sha256: sha256_hex(bytes.as_bytes()),
        sheets_json: bytes,
        reference_semantic_sha256: "a".repeat(64),
    }
}

#[test]
fn a_preview_requires_its_pinned_page_and_delivery_receipt_refuses_svg() {
    let layout = crate::printing::default_layout();
    let preview = with_layout(&source(json!([])), &layout).unwrap();
    let sheets: Value = serde_json::from_str(&preview.receipt.sheets_json).unwrap();
    let provenance = preview.receipt.local_print_recipe.as_ref().unwrap();
    let mut changed = provenance.clone();
    changed.layouts.insert(layout.id.clone(), "b".repeat(64));
    assert!(
        plan_outputs(&sheets, None, &changed).is_err_and(|error| error.contains("pinned layout"))
    );
    changed = provenance.clone();
    changed.preview.as_mut().unwrap().format = PrintArtifactFormat::Pdf;
    assert!(plan_outputs(&sheets, None, &changed)
        .is_err_and(|error| error.contains("one-page provenance")));
    let mut mixed = preview.selection.clone();
    mixed.tabular.push("xlsx".into());
    assert!(plan_outputs(&sheets, Some(&mixed), provenance)
        .is_err_and(|error| error.contains("exactly one named SVG")));
    let mut delivery = preview.receipt.clone();
    delivery.local_print_recipe = None;
    let layers = BTreeMap::from([(
        "lv_poles".into(),
        json!({"type":"FeatureCollection",
            "features":[{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[30.,-2.]}}]}),
    )]);
    for selection in [&preview.selection, &mixed] {
        let error = crate::report_export::plan_transformer_export(
            "proj-1",
            &delivery,
            "tx_a",
            1,
            &layers,
            Some(selection),
        )
        .unwrap_err();
        assert!(error.contains("svg is the shared print preview"));
        assert!(
            report_formats::normalize_output_selection(Some(selection.clone()), None, None)
                .is_err()
        );
    }
}

/// 4c193e6d: a project whose policy appends survey layers to delivered
/// reports could publish a layout but never proof it — the delivery
/// engine refuses a local room under that policy. A preview is a proof of
/// a sheet, not a delivery: it renders with the survey layers absent and
/// names the omission.

#[test]
fn the_engine_request_names_the_page_and_the_run_stays_local() {
    let layout = crate::printing::default_layout();
    let governed = source(json!([]));
    let preview = with_layout(&governed, &layout).unwrap();
    let layers = BTreeMap::from([(
        "lv_poles".to_string(),
        json!({"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{}}]}),
    )]);
    let plan = plan_transformer_export(
        "czgmdwth_gisagara",
        &preview.receipt,
        "agahehe",
        3,
        &layers,
        None,
    )
    .unwrap();
    assert_eq!(plan.formats, vec![format!("svg__{}", layout.id)]);
    let inputs = StagedInputs {
        transformer_document: Path::new("/t/transformer.json"),
        network_config: Path::new("/t/config.json"),
        out_dir: Path::new("/t/out"),
        admin_bounds: Some((Path::new("/t/asset.dsab"), &"b".repeat(64))),
        media_grant: None,
        print_context: None,
        sheet: None,
        survey_layers: None,
        transformer_natures: None,
    };
    assert_eq!(
        engine_request(&plan, &inputs).unwrap()["formats"],
        json!([format!("svg__{}", layout.id)])
    );
    assert_eq!(
        engine_request(&plan, &inputs).unwrap()["local_print_recipe"],
        json!(preview.receipt.local_print_recipe)
    );
    assert_eq!(
        publication_facts(
            &plan,
            &EngineIdentity {
                engine_version: "ds-network-reporter@0.1.0+abc".into(),
                build_manifest_sha256: "c".repeat(64),
                package_version: "0.1.0".into(),
                source_sha: "abc".into(),
                profile: "release".into(),
            }
        )
        .unwrap()["state"],
        json!("local_only")
    );
}
