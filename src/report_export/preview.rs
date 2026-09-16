//! A print preview: one transformer rendered through the delivery engine with
//! a draft layout — any id, any paper, saved or not — as the engine's own page.
//!
//! The page is the SVG every delivered format is derived from (PDF by
//! conversion, PNG and JPEG by rasterisation), with its text flattened to
//! paths so a viewer needs no font. One preview therefore serves every format
//! a template may later be delivered in, and a host paints it wherever it
//! likes: the desktop over its Canvas2D page, an agent through any SVG viewer.
//!
//! The governed inputs are untouched. The receipt is rewritten in memory
//! only: the draft replaces (or joins) the project's printing setups, the
//! output policy becomes exactly that one page, and the provenance names the
//! run a preview. `publication_facts` already reads a local recipe as
//! `local_only`, so a preview can never be promoted or published; hosts also
//! never place its file under an artifact root.
//!
//! Both hosts run this recipe: `ds report project export --preview-layout`
//! headlessly and the desktop shell against its held room, so a sheet looks
//! the same in the editor as it will in the delivery.
use super::*;
use crate::printing::Layout;
use crate::report_formats::{NamedPrintSelection, PrintArtifactFormat, OUTPUT_SELECTION_SCHEMA};

/// The one page format a preview renders.
pub const FORMAT: PrintArtifactFormat = PrintArtifactFormat::Svg;

/// What a preview run rendered, recorded in its receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    pub format: PrintArtifactFormat,
}

/// The receipt and selection a host runs, and the one output it will find.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    pub receipt: InputReceipt,
    pub selection: DesignOutputSelection,
    /// `svg__<layout id>`: the engine's output id.
    pub output_id: String,
    pub layout_id: String,
    pub layout_sha256: String,
    pub facts: Facts,
}

/// The receipt for one draft layout as the shared preview page.
pub fn with_layout(source: &InputReceipt, layout: &Layout) -> Result<Preview, String> {
    source.validate()?;
    if source.local_print_recipe.is_some() {
        return Err("A print preview starts from the governed project receipt".into());
    }
    crate::printing::validate(layout)?;
    let mut sheets: Value = serde_json::from_str(&source.sheets_json).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(layout).map_err(|e| e.to_string())?;
    let digest = sha256_hex(&bytes);
    let row = json!({"id": layout.id, "revision": digest, "layout": layout});
    match sheets.get_mut("printing_setups") {
        None => sheets["printing_setups"] = json!([row]),
        Some(value) => {
            let setups = value
                .as_array_mut()
                .ok_or("printing_setups must be an array")?;
            match setups.iter().position(|s| s["id"] == layout.id) {
                Some(index) => setups[index] = row,
                None => setups.push(row),
            }
        }
    }
    crate::report_formats::named_layouts(&sheets)?;
    let rows = sheets["project_settings"]
        .as_array()
        .ok_or("Project settings must be a parameter sheet")?;
    let mut rows = rows.clone();
    let selection = DesignOutputSelection {
        schema: OUTPUT_SELECTION_SCHEMA.into(),
        prints: vec![NamedPrintSelection {
            layout_id: layout.id.clone(),
            enabled: true,
            formats: vec![FORMAT],
        }],
        geospatial: Vec::new(),
        tabular: Vec::new(),
        execution: BTreeMap::new(),
    };
    // The in-memory policy names the preview page so the engine accepts it;
    // this receipt is never saved, which is what `require_deliverable`
    // protects on the host's save paths.
    report_formats::apply_output_selection(&mut rows, &selection)?;
    sheets["project_settings"] = Value::Array(rows);
    let outputs = report_formats::selected_plan(&sheets, false, Some(&selection))?;
    let output_id = match outputs.as_slice() {
        [one] => one.output_id.clone(),
        _ => return Err("A print preview is exactly one output".into()),
    };
    let facts = Facts { format: FORMAT };
    let mut result = source.clone();
    result.sheets_json = serde_json::to_string(&sheets).map_err(|e| e.to_string())?;
    result.sheets_sha256 = sha256_hex(result.sheets_json.as_bytes());
    result.local_print_recipe = Some(proof::Provenance {
        source_sheets_sha256: source.sheets_sha256.clone(),
        layouts: BTreeMap::from([(layout.id.clone(), digest.clone())]),
        preview: Some(facts),
    });
    result.validate()?;
    Ok(Preview {
        receipt: result,
        selection,
        output_id,
        layout_id: layout.id.clone(),
        layout_sha256: digest,
        facts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_new_draft_becomes_the_only_output_and_touches_nothing_else() {
        let mut layout = crate::printing::default_layout();
        layout.id = "draft-a3".into();
        layout.name = "Draft A3".into();
        let source = source(json!([]));
        let original = source.clone();
        let preview = with_layout(&source, &layout).unwrap();
        assert_eq!(source, original);
        assert_eq!(preview.output_id, "svg__draft-a3");
        assert_eq!(preview.layout_id, "draft-a3");
        let recipe = preview.receipt.local_print_recipe.as_ref().unwrap();
        assert_eq!(recipe.source_sheets_sha256, source.sheets_sha256);
        assert_eq!(recipe.preview.unwrap().format, PrintArtifactFormat::Svg);
        assert_eq!(recipe.layouts["draft-a3"], preview.layout_sha256);
        let sheets: Value = serde_json::from_str(&preview.receipt.sheets_json).unwrap();
        assert_eq!(
            sheets["pole_types"],
            json!([{"height":9,"material":"steel"}])
        );
        let named = report_formats::named_layouts(&sheets).unwrap();
        assert_eq!(named["draft-a3"], layout);
        let plan = report_formats::selected_plan(&sheets, false, Some(&preview.selection)).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].output_id, "svg__draft-a3");
        assert_eq!(plan[0].content_type, "image/svg+xml");
        assert_eq!(plan[0].suffix, ".draft-a3.svg");
        // The rewritten in-memory policy names only the preview page.
        let policy = report_formats::plan(&sheets, false).unwrap();
        assert_eq!(policy.len(), 1);
        assert_eq!(policy[0].output_id, "svg__draft-a3");
    }

    #[test]
    fn a_draft_of_a_saved_setup_replaces_it_whatever_its_paper() {
        let saved = crate::printing::default_layout();
        let source = source(json!([{"id":saved.id,"revision":"a".repeat(64),"layout":saved}]));
        let mut draft = crate::printing::named_layout(
            Some(saved.id.clone()),
            None,
            crate::printing::Paper::A0,
            crate::printing::Orientation::Landscape,
        )
        .unwrap();
        draft.elements[1].text = "Preview title".into();
        let preview = with_layout(&source, &draft).unwrap();
        let sheets: Value = serde_json::from_str(&preview.receipt.sheets_json).unwrap();
        assert_eq!(sheets["printing_setups"].as_array().unwrap().len(), 1);
        assert_eq!(
            sheets["printing_setups"][0]["layout"]["elements"][1]["text"],
            "Preview title"
        );
        assert_eq!(preview.output_id, format!("svg__{}", draft.id));
    }

    #[test]
    fn a_preview_never_starts_from_a_proof_and_never_becomes_a_saved_selection() {
        let layout = crate::printing::default_layout();
        let governed = source(json!([]));
        let preview = with_layout(&governed, &layout).unwrap();
        assert!(with_layout(&preview.receipt, &layout).is_err());
        // The engine reads the in-memory policy as stored; the host save
        // paths refuse the preview page as a deliverable.
        let stored = serde_json::to_value(&preview.selection).unwrap();
        assert!(report_formats::stored_output_selection(&stored).is_ok());
        assert!(report_formats::normalize_output_selection(
            Some(preview.selection.clone()),
            None,
            None
        )
        .is_err());
        assert!(report_formats::normalize_output_selection(
            None,
            Some(vec![preview.output_id.clone()]),
            None
        )
        .is_err());
        assert!(preview.selection.require_deliverable().is_err());
    }

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
        };
        assert_eq!(
            engine_request(&plan, &inputs).unwrap()["formats"],
            json!([format!("svg__{}", layout.id)])
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
}
