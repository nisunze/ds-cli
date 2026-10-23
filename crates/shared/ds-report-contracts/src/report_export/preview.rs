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
pub fn plan_outputs(
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
#[path = "preview_tests.rs"]
mod tests;
