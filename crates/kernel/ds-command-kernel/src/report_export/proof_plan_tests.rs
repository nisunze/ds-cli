//! Local review recipes planned through the Kernel's transformer export plan.
use super::proof::*;
use super::*;
use crate::printing::Layout;

fn review_source() -> (InputReceipt, Vec<Layout>) {
    let layouts = [
        ("saved-a0", crate::printing::Paper::A0),
        ("saved-a3", crate::printing::Paper::A3),
    ]
    .into_iter()
    .map(|(id, paper)| {
        crate::printing::named_layout(
            Some(id.into()),
            None,
            paper,
            crate::printing::Orientation::Landscape,
        )
        .unwrap()
    })
    .collect::<Vec<_>>();
    let sheets = json!({
        "printing_setups":layouts.iter().map(|layout| json!({
            "id":layout.id,"revision":sha256_hex(&serde_json::to_vec(layout).unwrap()),"layout":layout
        })).collect::<Vec<_>>(),
        "project_settings":[{"parameter":"design_export_format","value":["xlsx"]}]
    });
    let sheets_json = sheets.to_string();
    (
        InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: "Chad".into(),
            sheets_sha256: sha256_hex(sheets_json.as_bytes()),
            sheets_json,
            reference_semantic_sha256: "a".repeat(64),
        },
        layouts,
    )
}

fn review_selection(tokens: &[&str]) -> DesignOutputSelection {
    crate::report_formats::normalize_output_selection(
        None,
        Some(tokens.iter().map(|token| (*token).into()).collect()),
        None,
    )
    .unwrap()
}

#[test]
fn local_review_plans_a0_a3_a4_and_mixed_without_delivery_inventory() {
    let (source, layouts) = review_source();
    let sheets = source.sheets().unwrap();
    let unchanged = source.clone();
    let layers = BTreeMap::from([(
        "lv_poles".into(),
        json!({"type":"FeatureCollection","features":[{
            "type":"Feature","properties":{"pole_number":"P1"},
            "geometry":{"type":"Point","coordinates":[30.06,-1.94]}
        }]}),
    )]);
    for (tokens, expected) in [
        (vec!["pdf__saved-a0"], vec!["pdf__saved-a0"]),
        (vec!["pdf__saved-a3"], vec!["pdf__saved-a3"]),
        (
            vec!["voltage_drop_pdf"],
            vec!["voltage_drop", "voltage_drop_pdf"],
        ),
        (
            vec!["pdf__saved-a0", "pdf__saved-a3", "voltage_drop_pdf"],
            vec![
                "voltage_drop",
                "voltage_drop_pdf",
                "pdf__saved-a0",
                "pdf__saved-a3",
            ],
        ),
    ] {
        let selection = review_selection(&tokens);
        let outputs = local_review_plan(&sheets, false, Some(&selection)).unwrap();
        assert_eq!(
            outputs
                .iter()
                .map(|output| output.format.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        let mut review = source.clone();
        let selected_layouts = layouts
            .iter()
            .filter(|layout| tokens.contains(&format!("pdf__{}", layout.id).as_str()))
            .cloned()
            .collect::<Vec<_>>();
        review.local_print_recipe = Some(local_review(&source, &selected_layouts).unwrap());
        let plan = plan_transformer_export("proj-1", &review, "tx_a", 7, &layers, Some(&selection))
            .unwrap();
        assert_eq!(plan.outputs, outputs);
        assert_eq!(plan.formats, expected);
        assert_eq!(review.sheets_json, source.sheets_json);
        assert_eq!(review.sheets_sha256, source.sheets_sha256);
        let main = plan_transformer_export("proj-1", &source, "tx_a", 7, &layers, Some(&selection))
            .unwrap();
        assert_eq!(
            main.formats
                .iter()
                .filter(|format| *format == "voltage_drop")
                .count(),
            usize::from(tokens.contains(&"voltage_drop_pdf")),
            "only selected A4 analysis has a saved JSON companion for {tokens:?}"
        );
        assert!(plan_transformer_export(
            "proj-1",
            &review,
            "combined_transformer",
            7,
            &layers,
            Some(&selection),
        )
        .is_err());
    }
    assert_eq!(source, unchanged);
}
