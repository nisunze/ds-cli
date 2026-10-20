//! The read-only A4 voltage-drop print of one locally calculated transformer.
//!
//! `design.lv.project-export --project-config` captures the project's raw
//! process sheets and, beside them, the server's sealed printing receipt.
//! Together they hold everything the governed A4 needs: its document, the
//! canonical caption, the renderer policy and the project's report format.
//! This decides the exact `render_voltage_drop_result` request for a local
//! `ds.lv-voltage-drop.result/v1` from that capture alone: nothing is read,
//! saved or published. Hosts read the files, run the Reporter and verify it.

use serde_json::{json, Value};

use super::{admitted_printing_label, InputReceipt};
use crate::printing::renderer_defaults;
use crate::report_formats::project_voltage_drop_report_format;
use crate::style_resolution::Binding;

/// The Reporter request this module decides.
pub const RENDER_REQUEST_SCHEMA: &str = "ds.voltage-drop-pdf.render-request/v1";
/// The governed A4 document a printable capture owns.
const A4_DOCUMENT_SCHEMA: &str = "ds.print-a4-document/v1";
/// The only raw sheet that differs from the sealed receipt: the receipt adds
/// the canonical caption to it.
const CAPTION_SHEET: &str = "printing_context";

/// What one export capture decides for its A4 print.
#[derive(Debug, Clone)]
pub struct Capture {
    pub project: String,
    pub project_label: String,
    pub report_format: &'static str,
    pub renderer_defaults: Binding,
}

/// Prove that `receipt` was sealed with these raw process sheets and that
/// they own the project's governed A4, then decide its caption, renderer
/// policy and report format. Refuses before anything is calculated.
pub fn capture(network_config: &Value, receipt: &InputReceipt) -> Result<Capture, String> {
    let sheets = network_config
        .as_object()
        .filter(|sheets| !sheets.is_empty())
        .ok_or("A4 printing needs the project's process sheets: export with --project-config")?;
    let project = network_config[CAPTION_SHEET]["project_id"]
        .as_str()
        .filter(|project| !project.is_empty())
        .ok_or("The capture has no project printing context: export with --project-config")?;
    let document = &network_config["printing_a4"];
    if document["schema"] != A4_DOCUMENT_SCHEMA || document["project_id"] != project {
        return Err("The capture lacks the project's governed A4 document".into());
    }
    let context = receipt.printing_context_for_project(project)?;
    let sealed = receipt.sheets()?;
    if sheets
        .iter()
        .any(|(name, sheet)| name != CAPTION_SHEET && sealed.get(name) != Some(sheet))
    {
        return Err("The printing receipt was not sealed with these process sheets".into());
    }
    let project_label = admitted_printing_label(project, context["project"].as_str())?;
    let renderer_defaults: Binding =
        serde_json::from_value(network_config["printing_renderer_defaults"].clone())
            .map_err(|error| format!("print_renderer_defaults_missing: {error}"))?;
    renderer_defaults::admit(&renderer_defaults)?;
    Ok(Capture {
        project: project.into(),
        project_label,
        report_format: project_voltage_drop_report_format(network_config)?.token(),
        renderer_defaults,
    })
}

/// The exact Reporter request for one transformer of a local result whose
/// bytes hash to `source_sha256`. The Reporter never recalculates it.
pub fn request(
    capture: &Capture,
    network_config: &Value,
    transformer: &str,
    source_document: &str,
    source_sha256: &str,
    out_pdf: &str,
) -> Value {
    json!({
        "schema": RENDER_REQUEST_SCHEMA,
        "network_config": network_config,
        "renderer_defaults": capture.renderer_defaults,
        "source_document": source_document,
        "source_sha256": source_sha256,
        "report_format": capture.report_format,
        "transformer": transformer,
        "project_label": capture.project_label,
        "out_pdf": out_pdf,
    })
}

#[cfg(test)]
#[path = "voltage_drop_a4_tests.rs"]
mod tests;
