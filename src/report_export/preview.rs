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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    pub format: PrintArtifactFormat,
    /// The survey forms the project's policy appends to a delivered report
    /// that this preview page was rendered without. A preview is a local
    /// proof of a sheet and survey layers live in the cloud, so the preview
    /// recipe switches the append off in its in-memory settings and names the
    /// omission here. A delivery reaches the same page from the other side:
    /// it leaves the policy alone and names the omission on its plan
    /// ([`super::ExportPlan::survey_layers_omitted`]) under the same code.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub survey_forms_omitted: Vec<String>,
}

/// Switch a survey-append policy off in the preview's in-memory settings and
/// return the forms the page will be rendered without. The reading is the
/// shared one ([`super::survey_append_forms`]), so a preview and a delivery
/// never disagree about which forms a project appends. A policy with no
/// forms appends nothing, so nothing is omitted and nothing is changed.
fn omit_survey_append(rows: &mut [Value]) -> Vec<String> {
    let forms = super::survey_append_forms(rows);
    if forms.is_empty() {
        return Vec::new();
    }
    for row in rows.iter_mut() {
        if super::include_survey_row(row) {
            row["value"] = Value::Bool(false);
        }
    }
    forms
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

/// A proven local authoring preview remains one page. A caller-supplied output
/// list cannot select this purpose; the receipt must carry the existing
/// nonpublishable preview provenance and its exact one-layout byte pin.
pub(super) fn plan_outputs(
    sheets: &Value,
    selection: Option<&DesignOutputSelection>,
    provenance: &proof::Provenance,
) -> Result<Vec<report_formats::Output>, String> {
    if !provenance
        .preview
        .as_ref()
        .is_some_and(|facts| facts.format == FORMAT)
        || provenance.layouts.len() != 1
    {
        return Err("An authoring preview requires its exact one-page provenance".into());
    }
    let stored;
    let selection = match selection {
        Some(selection) => selection,
        None => {
            let row = report_formats::project_output_setting(sheets)?
                .ok_or("An authoring preview has no page selection")?;
            stored = report_formats::stored_output_selection(&row["value"])?;
            &stored
        }
    };
    let outputs = report_formats::selected_authoring_preview_plan(sheets, selection)?;
    let (_, id) = report_formats::named_print_output(&outputs[0].format)
        .ok_or("An authoring preview has no named page")?;
    let layouts = report_formats::named_layouts(sheets)?;
    let digest = sha256_hex(&serde_json::to_vec(&layouts[id]).map_err(|e| e.to_string())?);
    if provenance.layouts.get(id) != Some(&digest) {
        return Err("An authoring preview page differs from its pinned layout".into());
    }
    Ok(outputs)
}

/// Admit the engine transport of a local preview without passing its SVG
/// through the delivery-only output normalizer. The staged policy, exact
/// requested format and pinned layout must describe the same one page.
pub fn engine_outputs(
    sheets: &Value,
    formats: &[String],
    provenance: &proof::Provenance,
) -> Result<Vec<report_formats::Output>, String> {
    if !exact_lower_sha256(&provenance.source_sheets_sha256) {
        return Err("An authoring preview requires its source receipt digest".into());
    }
    let outputs = plan_outputs(sheets, None, provenance)?;
    if outputs
        .iter()
        .map(|output| &output.format)
        .ne(formats.iter())
    {
        return Err("An authoring preview request differs from its exact page selection".into());
    }
    Ok(outputs)
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
        voltage_drop_report: Default::default(),
    };
    // The in-memory policy names the preview page so the engine accepts it;
    // this receipt is never saved, which is what `require_deliverable`
    // protects on the host's save paths.
    report_formats::apply_output_selection(&mut rows, &selection)?;
    // A proof renders with the survey layers absent and says so, rather than
    // refusing a project whose delivered reports append them from the cloud.
    let survey_forms_omitted = omit_survey_append(&mut rows);
    sheets["project_settings"] = Value::Array(rows);
    let outputs = report_formats::selected_authoring_preview_plan(&sheets, &selection)?;
    let output_id = match outputs.as_slice() {
        [one] => one.output_id.clone(),
        _ => return Err("A print preview is exactly one output".into()),
    };
    let facts = Facts {
        format: FORMAT,
        survey_forms_omitted,
    };
    let mut result = source.clone();
    result.sheets_json = serde_json::to_string(&sheets).map_err(|e| e.to_string())?;
    result.sheets_sha256 = sha256_hex(result.sheets_json.as_bytes());
    result.local_print_recipe = Some(proof::Provenance {
        source_sheets_sha256: source.sheets_sha256.clone(),
        layouts: BTreeMap::from([(layout.id.clone(), digest.clone())]),
        preview: Some(facts.clone()),
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
        let facts = recipe.preview.clone().unwrap();
        assert_eq!(facts.format, PrintArtifactFormat::Svg);
        assert!(facts.survey_forms_omitted.is_empty());
        assert_eq!(recipe.layouts["draft-a3"], preview.layout_sha256);
        let sheets: Value = serde_json::from_str(&preview.receipt.sheets_json).unwrap();
        assert_eq!(
            sheets["pole_types"],
            json!([{"height":9,"material":"steel"}])
        );
        let named = report_formats::named_layouts(&sheets).unwrap();
        assert_eq!(named["draft-a3"], layout);
        let plan = plan_outputs(&sheets, Some(&preview.selection), recipe).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].output_id, "svg__draft-a3");
        assert_eq!(plan[0].content_type, "image/svg+xml");
        assert_eq!(plan[0].suffix, ".draft-a3.svg");
        assert_eq!(
            engine_outputs(&sheets, &[preview.output_id.clone()], recipe).unwrap(),
            plan
        );
        assert!(engine_outputs(&sheets, &["xlsx".into()], recipe).is_err());
        let mut changed = recipe.clone();
        changed.source_sheets_sha256 = "A".repeat(64);
        assert!(
            engine_outputs(&sheets, &[preview.output_id.clone()], &changed)
                .is_err_and(|error| error.contains("source receipt digest"))
        );
        // A delivery planner cannot claim this caller-authored SVG list is an
        // authoring preview: delivery still includes the saved analysis.
        let policy = report_formats::plan(&sheets, false).unwrap();
        assert_eq!(policy.len(), 2);
        assert_eq!(policy[0].output_id, "voltage_drop");
        assert_eq!(policy[1].output_id, "svg__draft-a3");
    }

    #[test]
    fn a_preview_requires_its_pinned_page_and_delivery_receipt_refuses_svg() {
        let layout = crate::printing::default_layout();
        let preview = with_layout(&source(json!([])), &layout).unwrap();
        let sheets: Value = serde_json::from_str(&preview.receipt.sheets_json).unwrap();
        let provenance = preview.receipt.local_print_recipe.as_ref().unwrap();
        let mut changed = provenance.clone();
        changed.layouts.insert(layout.id.clone(), "b".repeat(64));
        assert!(plan_outputs(&sheets, None, &changed)
            .is_err_and(|error| error.contains("pinned layout")));
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
            assert!(report_formats::normalize_output_selection(
                Some(selection.clone()),
                None,
                None
            )
            .is_err());
        }
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

    /// 4c193e6d: a project whose policy appends survey layers to delivered
    /// reports could publish a layout but never proof it — the delivery
    /// engine refuses a local room under that policy. A preview is a proof of
    /// a sheet, not a delivery: it renders with the survey layers absent and
    /// names the omission.
    #[test]
    fn a_preview_renders_without_the_survey_layers_and_names_them() {
        let layout = crate::printing::default_layout();
        let sheets = json!({
            "printing_setups": [],
            "project_settings": [
                {"parameter":"design_export_format","value":["xlsx","shp"]},
                {"parameter":"include_survey_data_in_transformer_exports","value":true},
                {"parameter":"survey_forms_in_transformer_exports","value":["field_notes","lv_poles_as_built","transformer_survey"]},
                {"parameter":"minimum_tr_size","value":"25"},
            ],
        });
        let bytes = serde_json::to_string(&sheets).unwrap();
        let source = InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: "Rwanda".into(),
            sheets_sha256: sha256_hex(bytes.as_bytes()),
            sheets_json: bytes,
            reference_semantic_sha256: "a".repeat(64),
        };
        let preview = with_layout(&source, &layout).unwrap();
        assert_eq!(
            preview.facts.survey_forms_omitted,
            ["field_notes", "lv_poles_as_built", "transformer_survey"]
        );
        let recipe = preview.receipt.local_print_recipe.as_ref().unwrap();
        assert_eq!(
            recipe.preview.as_ref().unwrap().survey_forms_omitted,
            preview.facts.survey_forms_omitted
        );
        let rewritten: Value = serde_json::from_str(&preview.receipt.sheets_json).unwrap();
        let rows = rewritten["project_settings"].as_array().unwrap();
        let include = rows
            .iter()
            .find(|row| row["parameter"] == "include_survey_data_in_transformer_exports")
            .unwrap();
        assert_eq!(include["value"], json!(false));
        // Every other row is as the project stored it; the governed receipt
        // is untouched.
        assert!(rows
            .iter()
            .any(|row| row["parameter"] == "minimum_tr_size" && row["value"] == "25"));
        assert!(rows.iter().any(
            |row| row["parameter"] == "survey_forms_in_transformer_exports"
                && row["value"].as_array().unwrap().len() == 3
        ));
        let original: Value = serde_json::from_str(&source.sheets_json).unwrap();
        assert_eq!(original["project_settings"][1]["value"], json!(true));
        // The hyphenated spelling and a text flag are read the same way.
        let mut rows = vec![
            json!({"parameter":"include-survey-data-in-transformer-exports","value":"yes"}),
            json!({"parameter":"transformer-export-survey-forms","value":"customer_survey"}),
        ];
        assert_eq!(omit_survey_append(&mut rows), ["customer_survey"]);
        assert_eq!(rows[0]["value"], json!(false));
        // Appending nothing omits nothing and changes nothing.
        let mut rows =
            vec![json!({"parameter":"include_survey_data_in_transformer_exports","value":true})];
        assert!(omit_survey_append(&mut rows).is_empty());
        assert_eq!(rows[0]["value"], json!(true));
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
}
