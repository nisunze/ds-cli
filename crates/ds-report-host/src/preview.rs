//! The one print preview entry both hosts reach.
//!
//! A preview is one transformer rendered through the delivery engine with a
//! draft layout, as the engine's own SVG page, returned in memory and never
//! placed under an artifact root. The desktop shell paints it over its
//! Canvas2D sheet; `ds report project export --preview-layout` writes the
//! same page headlessly. Both compose the same steps, all of them here:
//!
//! 1. the kernel rewrites the governed receipt for the draft
//!    (`report_export::preview::with_layout`) — a receipt that is already a
//!    recipe is refused;
//! 2. the kernel decides which context layers the draft prints with
//!    (`report::evaluate` `context_preparation` over the rewritten sheets,
//!    offline) and the host's closure supplies what it holds for exactly
//!    those layers — a preview reads held state and never acquires;
//! 3. the room, its media grant and the context are staged and run through
//!    [`run_transformer_report_with`], the same run a delivery makes, with
//!    the artifact root under private scratch;
//! 4. the one page is proven — exactly one artifact answering the draft's
//!    output id, within [`MAX_PREVIEW_BYTES`], re-hashed, UTF-8, an `<svg`
//!    document — and read into memory;
//! 5. the scratch goes, whatever the outcome.
//!
//! The host decides nothing here: what it hands over is data (its receipt,
//! its held room, its cached grant, the captures it holds), and what it gets
//! back is the page with the run's diagnostics, warnings and context summary.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use ds_command_kernel::printing::table_rows::PrintWarning;
use ds_command_kernel::printing::{self, PrintContextLayer};
use ds_command_kernel::report_export::{self, EngineIdentity, InputReceipt};
use ds_command_kernel::report_formats::PrintArtifactFormat;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    random_run_id, run_transformer_report_with, AdminBoundsAsset, HostFailure, PrintContextBytes,
    RunExtras, RunSettings, Staging, TransformerReportInputs, CODE_INPUTS_INVALID,
    CODE_RESULT_INVALID,
};

/// The largest page a preview hands back in memory.
pub const MAX_PREVIEW_BYTES: u64 = 96 * 1024 * 1024;

/// What every preview of one project shares: the governed receipt as the
/// service minted it (never a local recipe), the Rwanda reference when the
/// country needs it, and private scratch.
#[derive(Debug, Clone)]
pub struct PreviewSettings<'a> {
    pub project_id: &'a str,
    /// The governed project receipt; `local_print_recipe` must be `None`.
    pub receipt: &'a InputReceipt,
    pub admin_bounds: Option<&'a AdminBoundsAsset>,
    /// Private scratch; a preview creates its directories below it and
    /// removes every one of them before returning.
    pub staging_root: &'a Path,
}

/// The transformer room a host holds, handed over as data.
#[derive(Debug, Clone)]
pub struct HeldRoom {
    pub transformer: String,
    /// The saved document's server version — its publication revision.
    pub server_version: i64,
    pub layers: BTreeMap<String, Value>,
    /// ds-brain's `metadata.content_digest` when the room came from the
    /// service; recorded against the kernel's own digest of the same bytes.
    pub content_digest: Option<String>,
    /// The engine-schema media grant document the host holds for this
    /// room's photos, at most [`crate::MAX_MEDIA_GRANT_BYTES`]; `None` when
    /// the host has none, which the engine accepts for a room without media.
    pub media_grant: Option<Vec<u8>>,
    /// One-based position in the drawing set and the set's size, for the
    /// sheet numbering a title block prints; `None` is a set of one.
    pub sheet: Option<(u32, u32)>,
}

/// A context layer the draft names that the page was printed without.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextOmission {
    pub layer: String,
    /// Plain prose a sheet's status line can show beside the layer.
    pub reason: String,
}

/// The rendered page and everything the run said about it.
#[derive(Debug, Clone)]
pub struct PreviewPage {
    pub transformer: String,
    pub server_version: i64,
    /// The page as text: an SVG document a host hands to its rasteriser or
    /// viewer and never re-encodes.
    pub page: String,
    /// Lowercase hex SHA-256 of the page's exact bytes, as the engine
    /// declared and this crate re-hashed.
    pub sha256: String,
    pub size_bytes: u64,
    pub content_type: &'static str,
    /// `svg__<layout id>`: the engine's output id.
    pub output_id: String,
    pub layout_id: String,
    /// SHA-256 of the draft layout's canonical bytes, as the recipe names it.
    pub layout_sha256: String,
    pub format: PrintArtifactFormat,
    pub warnings: Vec<PrintWarning>,
    /// The page's collision assessment (kernel `printing::diagnostics`):
    /// typed findings with rectangles in millimetres and suggested moves.
    pub print_diagnostics: Vec<Value>,
    /// Wall time of the engine run, as the run receipt records it.
    pub elapsed_ms: u64,
    pub engine: EngineIdentity,
    /// The context layer ids the page was printed with, in document order.
    pub context_layers: Vec<String>,
    /// The draft's context layers the page was printed without, each named.
    pub context_omitted: Vec<ContextOmission>,
    /// Digest of the staged `ds.print-context/v1` document, when one was.
    pub context_sha256: Option<String>,
}

fn inputs_invalid(message: impl Into<String>) -> HostFailure {
    HostFailure::new(CODE_INPUTS_INVALID, message)
}

fn page_invalid(message: impl Into<String>) -> HostFailure {
    HostFailure::new(CODE_RESULT_INVALID, message)
}

/// The context layers the draft prints with, as the kernel decides them from
/// the rewritten sheets: every layer of every selected print output, once,
/// which for a preview is exactly the draft's own list.
fn draft_context_layers(
    preview: &report_export::preview::Preview,
) -> Result<Vec<PrintContextLayer>, HostFailure> {
    let sheets: Value = serde_json::from_str(&preview.receipt.sheets_json)
        .map_err(|error| inputs_invalid(error.to_string()))?;
    let request = json!({
        "command": "context_preparation",
        "sheets": sheets,
        "online": false,
        "selection": preview.selection,
    });
    let bytes = serde_json::to_vec(&request).map_err(|error| inputs_invalid(error.to_string()))?;
    let answer: Value =
        serde_json::from_str(&ds_command_kernel::report::evaluate(&bytes).map_err(inputs_invalid)?)
            .map_err(|error| inputs_invalid(error.to_string()))?;
    serde_json::from_value(answer["result"]["contexts"].clone())
        .map_err(|error| inputs_invalid(format!("selected context layers: {error}")))
}

/// Which of the draft's layers the staged context carries and which it
/// omits, each omission with the reason the host or the document gives.
fn context_summary(
    wanted: &[PrintContextLayer],
    context: Option<&PrintContextBytes>,
) -> (Vec<String>, Vec<ContextOmission>) {
    let Some(context) = context else {
        let omitted = wanted
            .iter()
            .map(|layer| ContextOmission {
                layer: layer.id.clone(),
                reason: "no print context was supplied for this preview".into(),
            })
            .collect();
        return (Vec::new(), omitted);
    };
    let omitted = wanted
        .iter()
        .filter(|layer| !context.layers.contains(&layer.id))
        .map(|layer| ContextOmission {
            layer: layer.id.clone(),
            reason: if context.omitted.contains(&layer.id) {
                "not held on this computer".into()
            } else {
                "not carried by the print context document".into()
            },
        })
        .collect();
    (context.layers.clone(), omitted)
}

/// Render one transformer's preview page for a draft layout and return it in
/// memory. `print_context` receives exactly the context layers the draft
/// names and answers with what the host holds for them, or `None` to print
/// without external context; it must not acquire anything. Nothing is left
/// under `settings.staging_root` on any path.
pub fn transformer_page(
    engine: &dyn crate::ReportEngine,
    settings: &PreviewSettings<'_>,
    room: &HeldRoom,
    layout: &printing::Layout,
    print_context: impl FnOnce(&[PrintContextLayer]) -> Result<Option<PrintContextBytes>, HostFailure>,
) -> Result<PreviewPage, HostFailure> {
    let preview =
        report_export::preview::with_layout(settings.receipt, layout).map_err(inputs_invalid)?;
    let contexts = draft_context_layers(&preview)?;
    let context = print_context(&contexts)?;
    let (context_layers, context_omitted) = context_summary(&contexts, context.as_ref());
    let context_sha256 = context.as_ref().map(|context| context.sha256.clone());

    // The page root is private scratch of its own; the run's staging lives
    // beside it and goes with the run, the page root goes with this guard.
    fs::create_dir_all(settings.staging_root).map_err(|error| {
        crate::staging_failed(format!(
            "could not create {}: {error}",
            settings.staging_root.display()
        ))
    })?;
    let page_root = Staging {
        path: settings
            .staging_root
            .join(format!("{}-page", random_run_id())),
    };
    let inputs = TransformerReportInputs {
        transformer: room.transformer.clone(),
        server_version: room.server_version,
        layers: room.layers.clone(),
        content_digest: room.content_digest.clone(),
        selection: Some(preview.selection.clone()),
        print_context: context,
        sheet: room.sheet,
    };
    let outcome = run_transformer_report_with(
        engine,
        &RunSettings {
            project_id: settings.project_id,
            receipt: &preview.receipt,
            admin_bounds: settings.admin_bounds,
            staging_root: settings.staging_root,
            out_root: &page_root.path,
        },
        &inputs,
        &RunExtras {
            media_grant: room.media_grant.as_deref(),
        },
    )?;
    drop(inputs);

    let artifact = match outcome.artifacts.as_slice() {
        [one] if one.output_id == preview.output_id => one,
        [_] => return Err(page_invalid("the print preview answered another output")),
        [] => return Err(page_invalid("the print preview produced no page")),
        _ => {
            return Err(page_invalid(
                "the print preview produced more than its one page",
            ))
        }
    };
    if artifact.size_bytes > MAX_PREVIEW_BYTES {
        return Err(page_invalid(format!(
            "the print preview page exceeds {MAX_PREVIEW_BYTES} bytes"
        )));
    }
    let bytes = fs::read(outcome.artifact_dir.join(&artifact.filename))
        .map_err(|error| page_invalid(format!("could not read the print preview: {error}")))?;
    if bytes.len() as u64 != artifact.size_bytes
        || format!("{:x}", Sha256::digest(&bytes)) != artifact.sha256
    {
        return Err(page_invalid(
            "the print preview bytes differ from the engine's receipt",
        ));
    }
    let page = String::from_utf8(bytes)
        .map_err(|_| page_invalid("the print preview page is not UTF-8 SVG"))?;
    if !page.trim_start().starts_with("<svg") {
        return Err(page_invalid(
            "the print preview page is not an SVG document",
        ));
    }
    let print_diagnostics = outcome.receipt["print_diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let elapsed_ms = outcome.receipt["elapsed_ms"].as_u64().unwrap_or_default();
    drop(page_root);
    Ok(PreviewPage {
        transformer: outcome.transformer,
        server_version: room.server_version,
        page,
        sha256: artifact.sha256.clone(),
        size_bytes: artifact.size_bytes,
        content_type: artifact.content_type,
        output_id: preview.output_id,
        layout_id: preview.layout_id,
        layout_sha256: preview.layout_sha256,
        format: preview.facts.format,
        warnings: outcome.warnings,
        print_diagnostics,
        elapsed_ms,
        engine: outcome.engine,
        context_layers,
        context_omitted,
        context_sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{receipt, FakeEngine};
    use ds_command_kernel::printing::PrintContextSource;
    use std::sync::atomic::Ordering;
    use std::sync::Mutex;

    fn room(name: &str) -> HeldRoom {
        let mut layers = BTreeMap::new();
        layers.insert(
            "lv_poles".to_string(),
            json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":{"pole_number":"P1"},"geometry":{"type":"Point","coordinates":[30.06,-1.94]}}]}),
        );
        HeldRoom {
            transformer: name.into(),
            server_version: 7,
            layers,
            content_digest: None,
            media_grant: None,
            sheet: None,
        }
    }

    fn context_layer(id: &str, layer: &str) -> PrintContextLayer {
        PrintContextLayer {
            id: id.into(),
            label: id.into(),
            source: PrintContextSource::Catalog {
                country: "Rwanda".into(),
                layer: layer.into(),
            },
        }
    }

    fn draft() -> printing::Layout {
        let mut layout = printing::default_layout();
        layout.id = "draft-a3".into();
        layout.name = "Draft A3".into();
        layout.context_layers = vec![
            context_layer("roads", "roads"),
            context_layer("rivers", "rivers"),
        ];
        layout
    }

    /// A governed receipt whose saved setup names other context than the
    /// draft, so a preview that read the saved list would show.
    fn governed() -> InputReceipt {
        let mut saved = printing::default_layout();
        saved.id = "saved-a3".into();
        saved.name = "Saved A3".into();
        saved.context_layers = vec![context_layer("buildings", "buildings")];
        let sheets = json!({
            "project_settings": [{"parameter":"design_export_format","value":["shp","xlsx","pdf__saved-a3"]}],
            "printing_setups": [{"id": saved.id, "revision": "a".repeat(64), "layout": saved}],
        });
        let bytes = serde_json::to_string(&sheets).unwrap();
        InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: "Benin".into(),
            sheets_sha256: format!("{:x}", Sha256::digest(bytes.as_bytes())),
            sheets_json: bytes,
            reference_semantic_sha256: "7".repeat(64),
        }
    }

    fn settings<'a>(receipt: &'a InputReceipt, staging: &'a Path) -> PreviewSettings<'a> {
        PreviewSettings {
            project_id: "proj-1",
            receipt,
            admin_bounds: None,
            staging_root: staging,
        }
    }

    fn nothing_left(staging: &Path) -> bool {
        !staging.exists() || fs::read_dir(staging).unwrap().next().is_none()
    }

    #[test]
    fn a_preview_returns_the_one_page_and_leaves_nothing_under_the_staging_root() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let receipt = receipt("Benin");
        let engine = FakeEngine::release();
        let mut room = room("tx_a");
        room.media_grant =
            Some(br#"{"schema":"ds.media-grant/v1","bucket":"b","prefix":"p/"}"#.to_vec());
        let page = transformer_page(
            &engine,
            &settings(&receipt, &staging),
            &room,
            &draft(),
            |_| Ok(None),
        )
        .unwrap();
        assert_eq!(page.output_id, "svg__draft-a3");
        assert_eq!(page.layout_id, "draft-a3");
        assert_eq!(page.format, PrintArtifactFormat::Svg);
        assert_eq!(page.content_type, "image/svg+xml");
        assert_eq!(page.transformer, "tx_a");
        assert_eq!(page.server_version, 7);
        assert!(page.page.trim_start().starts_with("<svg"));
        assert!(page.page.starts_with('\n'), "the page is the exact bytes");
        assert_eq!(page.size_bytes, page.page.len() as u64);
        assert_eq!(
            page.sha256,
            format!("{:x}", Sha256::digest(page.page.as_bytes()))
        );
        assert_eq!(page.print_diagnostics.len(), 1);
        assert_eq!(page.print_diagnostics[0]["output_id"], "svg__draft-a3");
        assert!(page.warnings.is_empty());
        assert_eq!(page.elapsed_ms, 5);
        assert_eq!(page.engine.source_sha, "a".repeat(40));
        assert_eq!(page.layout_sha256.len(), 64);
        // Without a context document every draft layer is a named omission.
        assert!(page.context_layers.is_empty());
        assert_eq!(
            page.context_omitted
                .iter()
                .map(|o| o.layer.as_str())
                .collect::<Vec<_>>(),
            ["roads", "rivers"]
        );
        assert!(page.context_sha256.is_none());
        // The held grant reached the engine; the page root and the run's
        // staging are both gone.
        assert!(engine.media_grant_seen.lock().unwrap().is_some());
        assert!(nothing_left(&staging));
        assert!(!temp.path().join("out").exists());
    }

    #[test]
    fn a_preview_never_starts_from_a_recipe_receipt_and_reads_only_the_drafts_contexts() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let engine = FakeEngine::release();
        let governed = governed();
        let recipe = report_export::preview::with_layout(&governed, &draft())
            .unwrap()
            .receipt;
        let asked = Mutex::new(None);
        let refused = transformer_page(
            &engine,
            &settings(&recipe, &staging),
            &room("tx_a"),
            &draft(),
            |contexts| {
                *asked.lock().unwrap() = Some(contexts.to_vec());
                Ok(None)
            },
        )
        .unwrap_err();
        assert_eq!(refused.code, CODE_INPUTS_INVALID);
        assert!(asked.lock().unwrap().is_none(), "no context is asked for");
        assert_eq!(engine.calls.load(Ordering::SeqCst), 0);
        assert!(nothing_left(&staging));

        // The governed receipt names a saved setup with other context; the
        // closure sees exactly the draft's list and its answer is staged.
        let document = br#"{"schema":"ds.print-context/v1","layers":{"roads":{}}}"#.to_vec();
        let sha256 = format!("{:x}", Sha256::digest(&document));
        let page = transformer_page(
            &engine,
            &settings(&governed, &staging),
            &room("tx_a"),
            &draft(),
            |contexts| {
                *asked.lock().unwrap() = Some(contexts.to_vec());
                Ok(Some(PrintContextBytes {
                    bytes: document.clone(),
                    sha256: sha256.clone(),
                    layers: vec!["roads".into()],
                    omitted: vec!["rivers".into()],
                }))
            },
        )
        .unwrap();
        assert_eq!(
            asked.lock().unwrap().clone().unwrap(),
            draft().context_layers
        );
        assert_eq!(page.context_layers, ["roads"]);
        assert_eq!(
            page.context_omitted,
            [ContextOmission {
                layer: "rivers".into(),
                reason: "not held on this computer".into(),
            }]
        );
        assert_eq!(page.context_sha256.as_deref(), Some(sha256.as_str()));
        assert_eq!(page.output_id, "svg__draft-a3");
        assert!(nothing_left(&staging));
    }

    #[test]
    fn a_page_that_is_not_svg_or_more_than_one_or_over_the_bound_is_refused_and_removed() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let receipt = receipt("Benin");
        let engine = FakeEngine::release();
        {
            let mut behaviour = engine.behaviour.lock().unwrap();
            behaviour.insert("tx_pdf".into(), "not_svg");
            behaviour.insert("tx_twins".into(), "two_pages");
            behaviour.insert("tx_huge".into(), "oversized");
        }
        let run = |name: &str| {
            transformer_page(
                &engine,
                &settings(&receipt, &staging),
                &room(name),
                &draft(),
                |_| Ok(None),
            )
            .unwrap_err()
        };
        let not_svg = run("tx_pdf");
        assert_eq!(not_svg.code, CODE_RESULT_INVALID);
        assert!(not_svg.message.contains("not an SVG"));
        assert!(nothing_left(&staging));
        let twins = run("tx_twins");
        assert_eq!(twins.code, CODE_RESULT_INVALID);
        assert!(nothing_left(&staging));
        let huge = run("tx_huge");
        assert_eq!(huge.code, CODE_RESULT_INVALID);
        assert!(huge.message.contains("exceeds"), "{}", huge.message);
        assert!(nothing_left(&staging));
        assert_eq!(engine.calls.load(Ordering::SeqCst), 3);
    }
}
