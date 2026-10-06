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
//!
//! [`execute`] is the command-shaped entry over [`transformer_page`]: the one
//! execution of `report.project.export --preview-layout` both doors reach —
//! `registry::dispatch` in the `ds` process and `ds_invoke` in the desktop
//! process — with the held context resolved once, by
//! `ds_project_data::held::preview_context`. [`answer`] is the one `data`
//! projection both doors wrap in the DS envelope (`ds_command_kernel::envelope`);
//! [`PreviewRefusal`] carries every refusal with its envelope class.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ds_command_kernel::envelope::{self, Class};
use ds_command_kernel::printing::context::{printed_layer_ids, RetainedContext};
use ds_command_kernel::printing::held_context::HeldCapture;
use ds_command_kernel::printing::table_rows::PrintWarning;
use ds_command_kernel::printing::{self, PrintContextLayer};
use ds_command_kernel::project_dataset_cache::Scope;
use ds_command_kernel::report_export::{self, EngineIdentity, InputReceipt};
use ds_command_kernel::report_formats::PrintArtifactFormat;
use ds_project_data::held::{preview_context, HeldPreviewState, PreviewContextRefusal};
use ds_project_data::{PrintContext, ReferenceResource};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    random_run_id, run_transformer_report_with, AdminBoundsAsset, HostFailure, PrintContextBytes,
    RunExtras, RunSettings, Staging, TransformerReportInputs, CODE_INPUTS_INVALID,
    CODE_RESULT_INVALID,
};

/// The largest page a preview hands back in memory.
pub const MAX_PREVIEW_BYTES: u64 = 96 * 1024 * 1024;

/// The command [`execute`] is the execution of. Its descriptor — arguments,
/// help, the MCP projection — is declared once in ds-cli; every door that
/// executes it wraps [`answer`] or a [`PreviewRefusal`] in the DS envelope
/// under this id.
pub const COMMAND_ID: &str = "report.project.export";
/// The command's contract version, as the envelope carries it.
pub const CONTRACT: u32 = 1;

/// What every preview of one project shares: the governed receipt as the
/// service minted it (never a local recipe), the Rwanda reference when the
/// country needs it, and private scratch.
#[derive(Debug, Clone)]
pub struct PreviewSettings<'a> {
    pub project_id: &'a str,
    pub project_crs: Option<&'a ds_command_kernel::printing::project_crs::Capture>,
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
    /// The survey forms the project appends to delivered reports that this
    /// proof was rendered without (`report_export::preview::Facts`).
    pub survey_forms_omitted: Vec<String>,
    /// Digest of the staged `ds.print-context/v1` document, when one was.
    pub context_sha256: Option<String>,
    /// The page's filename as the engine named it, so a filesystem host
    /// places the page under the same name a delivery batch would.
    pub filename: String,
    /// The run receipt exactly as the run wrote its `report-run.json`, so a
    /// filesystem host can place it beside the page under that name.
    pub receipt: Value,
}

/// The diagnostic code a preview page carries when the project's policy
/// appends survey layers the local proof was rendered without — the kernel's
/// own, so a preview and a delivery name the omission with one word.
pub use ds_command_kernel::report_export::{
    survey_layers_omitted_message, SURVEY_LAYERS_OMITTED, SURVEY_LAYER_OMISSION_REASON,
};

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
/// omits. A draft layer is carried when the document holds any of the ids it
/// prints as (`printed_layer_ids`); otherwise it is named, with the reason
/// the host or the document gives.
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
        .filter_map(|layer| {
            let ids = printed_layer_ids(layer);
            if ids.iter().any(|id| context.layers.contains(id)) {
                return None;
            }
            let reason = if ids.iter().any(|id| context.omitted.contains(id)) {
                "not supplied from what this computer holds"
            } else {
                "not carried by the print context document"
            };
            Some(ContextOmission {
                layer: layer.id.clone(),
                reason: reason.into(),
            })
        })
        .collect();
    (context.layers.clone(), omitted)
}

/// Render one transformer's preview page for a draft layout and return it in
/// memory. The draft is taken as the host holds it and upgraded to its
/// current form before the recipe. `print_context` receives exactly the
/// context layers the draft names and answers with what the host holds for
/// them, or `None` to print without external context; it must not acquire
/// anything. Nothing is left under `settings.staging_root` on any path.
pub fn transformer_page(
    engine: &dyn crate::ReportEngine,
    settings: &PreviewSettings<'_>,
    room: &HeldRoom,
    layout: &printing::Layout,
    print_context: impl FnOnce(&[PrintContextLayer]) -> Result<Option<PrintContextBytes>, HostFailure>,
) -> Result<PreviewPage, HostFailure> {
    transformer_page_with_styles(engine, settings, room, layout, None, print_context)
}

/// A local preview may carry a kernel-produced exact style capture. It is
/// admitted only after the governed source receipt becomes a local recipe;
/// the original source digest and layout provenance stay unchanged.
pub fn transformer_page_with_styles(
    engine: &dyn crate::ReportEngine,
    settings: &PreviewSettings<'_>,
    room: &HeldRoom,
    layout: &printing::Layout,
    captured: Option<&Value>,
    print_context: impl FnOnce(&[PrintContextLayer]) -> Result<Option<PrintContextBytes>, HostFailure>,
) -> Result<PreviewPage, HostFailure> {
    // The recipe is over the draft's current form: a draft a host still holds
    // under the legacy schema, or without its completed bindings, is upgraded
    // here, so every host stages the same document for the same draft.
    let layout = printing::upgrade(layout.clone()).map_err(inputs_invalid)?;
    let mut preview =
        report_export::preview::with_layout(settings.receipt, &layout).map_err(inputs_invalid)?;
    if let Some(captured) = captured {
        preview.receipt = ds_command_kernel::style_resolution::overlay_local_preview(
            settings.project_id,
            &layout.style_refs,
            &preview.receipt,
            captured,
        )
        .map_err(|refusal| inputs_invalid(refusal.to_string()))?;
    }
    let contexts = draft_context_layers(&preview)?;
    let context = print_context(&contexts)?;
    let context = settings
        .project_crs
        .map(|capture| crate::bind_project_crs(context.clone(), capture, settings.project_id))
        .transpose()?
        .or(context);
    let (context_layers, context_omitted) = context_summary(&contexts, context.as_ref());
    let context_sha256 = context.as_ref().map(|context| context.sha256.clone());

    // The page root is private scratch of its own; the run's staging lives
    // beside it and goes with the run, the page root goes with this guard.
    crate::create_staging_root(settings.staging_root)?;
    let page_root = Staging {
        path: settings
            .staging_root
            .join(format!("{}-page", random_run_id())),
    };
    crate::create_private_dir(&page_root.path)?;
    let inputs = TransformerReportInputs {
        transformer: room.transformer.clone(),
        server_version: room.server_version,
        layers: room.layers.clone(),
        content_digest: room.content_digest.clone(),
        transformer_natures: None,
        selection: Some(preview.selection.clone()),
        print_context: context,
        sheet: room.sheet,
        survey: None,
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
            ));
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
    let mut print_diagnostics = outcome.receipt["print_diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // The survey forms the delivered report appends that this proof was
    // rendered without: a named omission in the diagnostics, and one named
    // omission per form beside the context layers, so a sheet's status line
    // and an agent reading the receipt both see it.
    let mut context_omitted = context_omitted;
    if !preview.facts.survey_forms_omitted.is_empty() {
        print_diagnostics.push(json!({
            "code": SURVEY_LAYERS_OMITTED,
            "forms": preview.facts.survey_forms_omitted,
            "message": survey_layers_omitted_message(&preview.facts.survey_forms_omitted),
        }));
        for form in &preview.facts.survey_forms_omitted {
            context_omitted.push(ContextOmission {
                layer: form.clone(),
                reason: SURVEY_LAYER_OMISSION_REASON.into(),
            });
        }
    }
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
        survey_forms_omitted: preview.facts.survey_forms_omitted.clone(),
        context_sha256,
        filename: artifact.filename.clone(),
        receipt: outcome.receipt,
    })
}

/// The preview's argument set, as every door builds it from the command's
/// declared inputs: `--transformer X` and the `--preview-layout` document.
/// The CLI names the document by path and reads it; the desktop hands it
/// inline because the webview has no filesystem. Either way the draft is
/// taken as held and upgraded inside [`transformer_page`].
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewRequest {
    pub transformer: String,
    pub layout: printing::Layout,
}

/// The project's geographic holdings a host can read rooms from for the
/// catalogue, building and unretained contour layers a draft names: the
/// shared root, the principal/project scope, and the catalogue rows the
/// rooms were held under. Reads only; a preview never acquires.
#[derive(Debug, Clone)]
pub struct Holdings {
    pub root: PathBuf,
    /// Project print roster and saved rooms live under the machine layer root,
    /// independently of the geographic cache's configured storage location.
    pub room_root: Option<PathBuf>,
    pub scope: Scope,
    pub catalog: Vec<ReferenceResource>,
}

/// What a host holds for the draft's context layers, read from its own
/// stores without a decision: survey and local-layer captures (the browser's
/// IndexedDB rows, typed by the kernel), the derived preparations it retained
/// by context layer id, and its holdings root when it has one. A layer with
/// nothing here is a named omission on the sheet, never a refusal.
#[derive(Debug, Clone, Default)]
pub struct HeldContext {
    pub captures: Vec<HeldCapture>,
    pub retained: BTreeMap<String, RetainedContext>,
    pub holdings: Option<Holdings>,
}

/// Everything a host holds for one preview: the admitted room and the
/// context. The room was admitted before this — by
/// `printing::preview::room_admission` over the desktop's row, by the
/// service answer's own admission in the CLI — and both hand the same shape.
#[derive(Debug, Clone)]
pub struct HeldState {
    pub room: HeldRoom,
    pub context: HeldContext,
}

/// Why a preview did not produce its page: a kernel refusal (a room a door
/// admitted, a capture `held_context::admit` refused — code, message key
/// and params travel unchanged so a localised surface renders them), or a
/// host failure of the run itself.
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewRefusal {
    Kernel(printing::preview::Refusal),
    Host(HostFailure),
}

/// The envelope class of every refusal code a preview can end with, keyed by
/// code alone so both doors classify identically and ds-cli's own table can
/// be proven equal to this one. A code not listed here is a defect in this
/// table and classifies as `internal`.
pub const CLASS_TABLE: &[(&str, Class)] = &[
    // The kernel's room admission, over the row a door holds.
    ("print_preview_room_unavailable", Class::Unavailable),
    ("print_preview_room_incomplete", Class::Conflict),
    // The kernel's capture admission and the omission it may name.
    ("print_context_capture_invalid", Class::InvalidInput),
    ("print_context_survey_empty", Class::InvalidInput),
    ("print_context_too_large", Class::InvalidInput),
    ("print_context_capture_changed", Class::InvalidInput),
    ("print_context_foreign_project", Class::InvalidInput),
    ("print_context_capture_incomplete", Class::InvalidInput),
    ("print_context_not_held_source", Class::InvalidInput),
    ("print_context_omitted", Class::InvalidInput),
    // The host's run, this crate's own codes and the ones its adapters raise.
    ("report_inputs_invalid", Class::InvalidInput),
    ("print_context_unsupported", Class::InvalidInput),
    ("report_output_exists", Class::Conflict),
    ("admin_bounds_unavailable", Class::Unavailable),
    ("reporter_engine_missing", Class::Unavailable),
    ("catalog_unavailable", Class::Unavailable),
    ("reference_bundle_unavailable", Class::Unavailable),
    ("report_staging_failed", Class::Failed),
    ("report_result_invalid", Class::Failed),
    ("engine_refused", Class::Failed),
    ("export_blocked", Class::Failed),
    ("callee_contract_mismatch", Class::Failed),
    ("callee_timed_out", Class::Failed),
    ("report_engine_identity_changed", Class::Failed),
    ("print_context_invalid", Class::Failed),
    ("project_dataset_store_failed", Class::Failed),
];

/// One sentence for a kernel refusal, from its code and params; the
/// localised surface renders `detail.message_key` instead.
fn kernel_message(refusal: &printing::preview::Refusal) -> String {
    let param = |key: &str| refusal.params.get(key).cloned().unwrap_or_default();
    let name = param("name");
    match refusal.code.as_str() {
        "print_preview_room_unavailable" => {
            format!("the design of transformer {name} is not held on this host")
        }
        "print_preview_room_incomplete" => {
            format!("the held design of transformer {name} has unsaved incomplete edits")
        }
        "print_context_capture_invalid" => {
            format!("the capture handed for {name} is not a feature collection")
        }
        "print_context_survey_empty" => format!("the survey capture for {name} has no entries"),
        "print_context_too_large" => format!(
            "the capture for {name} exceeds {} features or is a display preview",
            param("bound")
        ),
        "print_context_capture_changed" => {
            format!("the capture for {name} changed while it was handed over")
        }
        "print_context_foreign_project" => {
            format!("the capture for {name} belongs to another project")
        }
        "print_context_capture_incomplete" => {
            format!("the capture for {name} is not completely loaded")
        }
        "print_context_not_held_source" => {
            format!("{name} is not a source a host captures")
        }
        "print_context_omitted" => format!("{}: {}", param("label"), param("reason")),
        other => {
            let params: Vec<String> = refusal
                .params
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            format!("{other} ({})", params.join(", "))
        }
    }
}

impl PreviewRefusal {
    /// The stable snake_case code a script branches on.
    pub fn code(&self) -> &str {
        match self {
            PreviewRefusal::Kernel(refusal) => &refusal.code,
            PreviewRefusal::Host(failure) => &failure.code,
        }
    }

    /// One sentence in plain prose.
    pub fn message(&self) -> String {
        match self {
            PreviewRefusal::Kernel(refusal) => kernel_message(refusal),
            PreviewRefusal::Host(failure) => failure.message.clone(),
        }
    }

    /// The envelope class, from [`CLASS_TABLE`].
    pub fn class(&self) -> Class {
        let code = self.code();
        CLASS_TABLE
            .iter()
            .find(|(known, _)| *known == code)
            .map(|(_, class)| *class)
            .unwrap_or(Class::Internal)
    }

    /// The envelope `detail`: a kernel refusal's `{message_key, params}` for
    /// the localised surface; a host failure's own detail when it has one.
    pub fn detail(&self) -> Option<Value> {
        match self {
            PreviewRefusal::Kernel(refusal) => Some(json!({
                "message_key": refusal.message_key,
                "params": refusal.params,
            })),
            PreviewRefusal::Host(failure) => failure.detail.clone(),
        }
    }

    /// The refusal as the envelope carries it, so both doors write the same
    /// `error` member: `envelope::error(COMMAND_ID, CONTRACT, &refusal)`.
    pub fn envelope_refusal(&self) -> envelope::Refusal {
        let refusal = envelope::Refusal::new(self.class(), self.code(), self.message());
        match self.detail() {
            Some(detail) => refusal.detail(detail),
            None => refusal,
        }
    }
}

/// The context document as the run stages it, from what the resolver
/// assembled: `None` when there is no document (the engine prints without
/// external context and every draft layer is a named omission).
fn context_bytes(context: Option<PrintContext>) -> Option<PrintContextBytes> {
    let context = context?;
    let bytes = context.document?;
    Some(PrintContextBytes {
        bytes,
        sha256: context.sha256.unwrap_or_default(),
        layers: context.layers,
        omitted: context
            .omitted
            .iter()
            .map(|omission| omission.layer.clone())
            .collect(),
    })
}

/// Execute `report.project.export --preview-layout` over what a host holds:
/// the one place both doors reach, and the only one that runs the engine.
///
/// The room must be the requested transformer's (a door hands the room it
/// admitted for that name; another name is `report_inputs_invalid`). The
/// held context is resolved by `ds_project_data::held::preview_context` for
/// exactly the context layers the draft names, as [`transformer_page`]
/// decides them: a capture the kernel refuses ends the preview with that
/// refusal unchanged; a layer the host has nothing for is a named omission
/// with the resolver's reason, never an acquisition; without holdings every
/// room-read layer is omitted as unreadable here. The page comes back in
/// memory and nothing is left under the staging root.
pub fn execute(
    engine: &dyn crate::ReportEngine,
    settings: &PreviewSettings<'_>,
    request: &PreviewRequest,
    held: &HeldState,
) -> Result<PreviewPage, PreviewRefusal> {
    execute_with_styles(engine, settings, request, held, None)
}

/// Carry exact immutable style data into the shared local-preview execution.
/// Delivery paths continue to use their governed source receipts unchanged.
pub fn execute_with_styles(
    engine: &dyn crate::ReportEngine,
    settings: &PreviewSettings<'_>,
    request: &PreviewRequest,
    held: &HeldState,
    captured: Option<&Value>,
) -> Result<PreviewPage, PreviewRefusal> {
    if held.room.transformer != request.transformer {
        return Err(PreviewRefusal::Host(inputs_invalid(format!(
            "the held room is {}'s, not {}'s",
            held.room.transformer, request.transformer
        ))));
    }
    let layers = serde_json::to_value(&held.room.layers)
        .map_err(|error| PreviewRefusal::Host(inputs_invalid(error.to_string())))?;
    let state = HeldPreviewState {
        captures: held.context.captures.clone(),
        retained: held.context.retained.clone(),
    };
    // Without holdings the resolver reads no room, so the scope it is handed
    // names the project alone and is never used for a read.
    let unread = Scope {
        principal: String::new(),
        project: settings.project_id.to_owned(),
    };
    let (root, scope, catalog): (Option<&Path>, &Scope, &[ReferenceResource]) =
        match &held.context.holdings {
            Some(holdings) => (
                Some(holdings.root.as_path()),
                &holdings.scope,
                holdings.catalog.as_slice(),
            ),
            None => (None, &unread, &[]),
        };
    let mut kernel_refusal: Option<printing::preview::Refusal> = None;
    let mut reasons: Vec<(String, String)> = Vec::new();
    let mut wanted_layers: Vec<PrintContextLayer> = Vec::new();
    let adjacent = printing::adjacent_networks::selected(&request.layout);
    let mut adjacent_omission = None;
    let outcome = transformer_page_with_styles(
        engine,
        settings,
        &held.room,
        &request.layout,
        captured,
        |wanted| {
            wanted_layers = wanted.to_vec();
            match preview_context(
                root,
                scope,
                settings.project_id,
                &request.transformer,
                &layers,
                wanted,
                catalog,
                &state,
            ) {
                Ok(context) => {
                    if let Some(context) = &context {
                        reasons.extend(
                            context
                                .omitted
                                .iter()
                                .map(|omission| (omission.layer.clone(), omission.reason.clone())),
                        );
                    }
                    let context = context_bytes(context);
                    if !adjacent {
                        return Ok(context);
                    }
                    // Both native hosts read the same account/project hold. A
                    // draft never fetches unheld rooms or silently prints a subset.
                    let rooms = match &held.context.holdings {
                        Some(holdings) if holdings.scope.project == settings.project_id => {
                            holdings
                                .room_root
                                .as_ref()
                                .ok_or_else(|| "no project print room root on this host".to_owned())
                                .and_then(|root| {
                                    ds_project_data::room_hold::active_print_rooms(
                                        root,
                                        &holdings.scope,
                                    )
                                })
                        }
                        _ => Err("no project holdings root on this host".into()),
                    };
                    let rooms = match rooms {
                        Ok(rooms) => rooms,
                        Err(reason) => {
                            adjacent_omission = Some(reason);
                            return Ok(context);
                        }
                    };
                    let document = context
                        .as_ref()
                        .map(|c| serde_json::from_slice::<Value>(&c.bytes))
                        .transpose()
                        .map_err(|e| inputs_invalid(e.to_string()))?;
                    let mut field_layout = request.layout.clone();
                    if let Some(capture) = captured {
                        field_layout.style_refs.extend(
                            serde_json::from_value::<BTreeMap<String, String>>(
                                capture["style_refs"].clone(),
                            )
                            .map_err(|e| inputs_invalid(e.to_string()))?,
                        );
                    }
                    let doc = printing::adjacent_networks::attach(
                        &request.transformer,
                        &rooms,
                        document.as_ref(),
                        &printing::adjacent_networks::required_fields(
                            std::slice::from_ref(&field_layout),
                            captured
                                .and_then(|capture| capture.get("print_styles"))
                                .unwrap_or(
                                    &settings.receipt.sheets().map_err(inputs_invalid)?
                                        ["printing_styles"],
                                ),
                        )
                        .map_err(inputs_invalid)?,
                    )
                    .map_err(inputs_invalid)?;
                    let Some(doc) = doc else {
                        return Ok(context);
                    };
                    let layers: BTreeMap<String, Value> =
                        serde_json::from_value(doc["layers"].clone())
                            .map_err(|e| inputs_invalid(e.to_string()))?;
                    let bytes = report_export::print_context_document(
                        &doc["coverage"],
                        &doc["sources"],
                        &layers,
                    )
                    .map_err(inputs_invalid)?;
                    Ok(Some(PrintContextBytes {
                        sha256: report_export::sha256_hex(&bytes),
                        bytes,
                        layers: layers.into_keys().collect(),
                        omitted: context.map_or_else(Vec::new, |c| c.omitted),
                    }))
                }
                Err(PreviewContextRefusal::Kernel(refusal)) => {
                    let failure = inputs_invalid(kernel_message(&refusal));
                    kernel_refusal = Some(refusal);
                    Err(failure)
                }
                Err(PreviewContextRefusal::Holdings(failure)) => {
                    Err(HostFailure::new(failure.code(), failure.message()))
                }
            }
        },
    );
    let mut page = match (outcome, kernel_refusal) {
        (Err(_), Some(refusal)) => return Err(PreviewRefusal::Kernel(refusal)),
        (Err(failure), None) => return Err(PreviewRefusal::Host(failure)),
        (Ok(page), _) => page,
    };
    if let Some(reason) = adjacent_omission {
        page.context_omitted
            .extend(
                printing::adjacent_networks::LAYERS
                    .into_iter()
                    .map(|(_, id)| ContextOmission {
                        layer: id.into(),
                        reason: reason.clone(),
                    }),
            );
    }
    // The resolver's reason for a layer it omitted is what the sheet shows
    // beside it: named by any of the ids the draft layer prints as.
    for omission in &mut page.context_omitted {
        let Some(layer) = wanted_layers
            .iter()
            .find(|layer| layer.id == omission.layer)
        else {
            continue;
        };
        let ids = printed_layer_ids(layer);
        if let Some((_, reason)) = reasons.iter().find(|(id, _)| ids.contains(id)) {
            omission.reason = reason.clone();
        }
    }
    Ok(page)
}

/// Where a host put one page: on its filesystem, named by path in the
/// answer, or nowhere, the SVG text inline — the desktop's web limitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageRef<'a> {
    File(&'a Path),
    Inline,
}

/// What `data.preview.note` says: a preview is draft state, not an artifact.
pub const PREVIEW_NOTE: &str =
    "a preview is draft state rendered from what this host holds; the delivery is the artifact";

/// The one `data` member of a successful preview, for both doors: one row
/// per page with the page by path or inline, the run's diagnostics,
/// warnings and context summary, the draft's identity under `preview`, and
/// a completed batch summary. Every page was rendered for the same draft, so
/// `preview` is read from the first.
pub fn answer(project: &str, lane: &str, pages: &[(&PreviewPage, PageRef<'_>)]) -> Value {
    let first = pages.first().map(|(page, _)| *page);
    let results: Vec<Value> = pages
        .iter()
        .map(|(page, placed)| {
            let page_ref = match placed {
                PageRef::File(path) => json!({ "path": path.to_string_lossy() }),
                PageRef::Inline => json!({ "svg": page.page }),
            };
            json!({
                "transformer": page.transformer,
                "server_version": page.server_version,
                "status": "completed",
                "page": page_ref,
                "filename": page.filename,
                "sha256": page.sha256,
                "size_bytes": page.size_bytes,
                "content_type": page.content_type,
                "engine": {
                    "engine_version": page.engine.engine_version,
                    "build_manifest_sha256": page.engine.build_manifest_sha256,
                },
                "elapsed_ms": page.elapsed_ms,
                "warnings": page.warnings,
                "print_diagnostics": page.print_diagnostics,
                "print_context": {
                    "sha256": page.context_sha256,
                    "layers": page.context_layers,
                    "omitted": page
                        .context_omitted
                        .iter()
                        .map(|omission| json!({ "layer": omission.layer, "reason": omission.reason }))
                        .collect::<Vec<_>>(),
                },
            })
        })
        .collect();
    json!({
        "lane": lane,
        "project": project,
        "preview": {
            "layout_id": first.map(|page| page.layout_id.as_str()),
            "layout_sha256": first.map(|page| page.layout_sha256.as_str()),
            "output_id": first.map(|page| page.output_id.as_str()),
            "format": "svg",
            "note": PREVIEW_NOTE,
            "survey_forms_omitted": first.map_or_else(Vec::new, |page| page.survey_forms_omitted.clone()),
        },
        "results": results,
        "batch": {
            "status": "completed",
            "requested_count": pages.len(),
            "completed": pages.len(),
            "failed": 0,
        },
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
            project_crs: None,
            receipt,
            admin_bounds: None,
            staging_root: staging,
        }
    }

    fn nothing_left(staging: &Path) -> bool {
        !staging.exists() || fs::read_dir(staging).unwrap().next().is_none()
    }

    /// 4c193e6d: a survey-append policy no longer stops a proof. The engine
    /// receives the recipe with the append switched off, the page carries a
    /// `print_survey_layers_omitted` diagnostic naming the forms, and each
    /// form is a named omission beside the context layers.
    #[test]
    fn a_survey_policy_project_previews_with_the_survey_layers_named_absent() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let sheets = r#"{"project_settings":[{"parameter":"design_export_format","value":["shp","xlsx"]},{"parameter":"include_survey_data_in_transformer_exports","value":true},{"parameter":"survey_forms_in_transformer_exports","value":["field_notes","transformer_survey"]}]}"#;
        let receipt = InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: "Benin".into(),
            sheets_json: sheets.into(),
            sheets_sha256: format!("{:x}", Sha256::digest(sheets.as_bytes())),
            reference_semantic_sha256: "7".repeat(64),
        };
        let engine = FakeEngine::release();
        let page = transformer_page(
            &engine,
            &settings(&receipt, &staging),
            &room("tx_a"),
            &draft(),
            |_| Ok(None),
        )
        .unwrap();
        assert_eq!(
            page.survey_forms_omitted,
            ["field_notes", "transformer_survey"]
        );
        let named = page
            .print_diagnostics
            .iter()
            .find(|d| d["code"] == SURVEY_LAYERS_OMITTED)
            .expect("the omission is a diagnostic");
        assert_eq!(named["forms"], json!(["field_notes", "transformer_survey"]));
        assert!(named["message"].as_str().unwrap().contains("field_notes"));
        assert!(page
            .context_omitted
            .iter()
            .any(|o| o.layer == "transformer_survey" && o.reason.contains("survey layer")));
        let recipe = &page.receipt["local_print_recipe"];
        assert_eq!(
            recipe["preview"]["survey_forms_omitted"],
            json!(["field_notes", "transformer_survey"])
        );
        let answer = answer("proj-1", "stable", &[(&page, PageRef::Inline)]);
        assert_eq!(
            answer["preview"]["survey_forms_omitted"],
            json!(["field_notes", "transformer_survey"])
        );
        assert!(answer["results"][0]["print_diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == SURVEY_LAYERS_OMITTED));
        assert!(nothing_left(&staging));
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
    fn local_preview_carries_new_styles_and_preserves_governed_source_provenance() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let engine = FakeEngine::release();
        let source = governed();
        let mut layout = draft();
        layout.style_refs =
            BTreeMap::from([("roads".into(), "opaque/new-after-build_print".into())]);
        let document = json!({"type":"line","paint":{"line-color":"#123456","line-width":2}});
        let captured = json!({
            "project_id":"proj-1", "resolution_revision":"a".repeat(64),
            "style_refs": layout.style_refs,
            "print_styles":{"opaque/new-after-build_print":document},
            "style_revisions":{"opaque/new-after-build_print":{"revision_id":"b".repeat(64),"content_sha256":ds_command_kernel::style_plan::style_digest(&document)}}
        });
        let page = transformer_page_with_styles(
            &engine,
            &settings(&source, &staging),
            &room("tx_a"),
            &layout,
            Some(&captured),
            |_| Ok(None),
        )
        .unwrap();
        assert_eq!(
            page.receipt["local_print_recipe"]["source_sheets_sha256"],
            source.sheets_sha256
        );
        assert!(page.receipt["local_print_recipe"]["layouts"][&layout.id].is_string());
        assert!(source.local_print_recipe.is_none());
        assert!(source.sheets().unwrap()["printing_styles"].is_null());
        assert!(nothing_left(&staging));
        for (field, value, code) in [
            (
                "project_id",
                json!("another-project"),
                "project_context_changed",
            ),
            (
                "style_revisions",
                json!({"opaque/new-after-build_print":{"revision_id":"b".repeat(64),"content_sha256":"0".repeat(64)}}),
                "style_resolution_revision_mismatch",
            ),
        ] {
            let mut tampered = captured.clone();
            tampered[field] = value;
            let calls = engine.calls.load(Ordering::SeqCst);
            let refusal = transformer_page_with_styles(
                &engine,
                &settings(&source, &staging),
                &room("tx_a"),
                &layout,
                Some(&tampered),
                |_| panic!("refused before context reads"),
            )
            .unwrap_err();
            assert!(refusal.message.contains(code), "{}", refusal.message);
            assert_eq!(engine.calls.load(Ordering::SeqCst), calls);
        }
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
                reason: "not supplied from what this computer holds".into(),
            }]
        );
        assert_eq!(page.context_sha256.as_deref(), Some(sha256.as_str()));
        assert_eq!(page.output_id, "svg__draft-a3");
        assert!(nothing_left(&staging));
    }

    #[test]
    fn a_legacy_draft_is_upgraded_before_the_recipe() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let receipt = receipt("Benin");
        let engine = FakeEngine::release();
        let mut legacy = draft();
        legacy.schema = printing::LEGACY_SCHEMA.into();
        let upgraded = printing::upgrade(legacy.clone()).unwrap();
        assert_ne!(upgraded, legacy, "the legacy draft is another document");
        let as_held = report_export::preview::with_layout(&receipt, &legacy).unwrap();
        let as_upgraded = report_export::preview::with_layout(&receipt, &upgraded).unwrap();
        assert_ne!(as_held.layout_sha256, as_upgraded.layout_sha256);
        let page = transformer_page(
            &engine,
            &settings(&receipt, &staging),
            &room("tx_a"),
            &legacy,
            |_| Ok(None),
        )
        .unwrap();
        // The recipe was over the upgraded draft, never the legacy bytes.
        assert_eq!(page.layout_sha256, as_upgraded.layout_sha256);
        assert_eq!(page.output_id, "svg__draft-a3");
        assert!(nothing_left(&staging));
    }

    #[test]
    fn a_draft_layer_that_prints_as_expanded_document_layers_is_carried_not_omitted() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let receipt = receipt("Benin");
        let engine = FakeEngine::release();
        // Project MV, buildings and contours, then a catalogue layer: the
        // document names contours and MV by the ids their preparation emits.
        let mut layout = draft();
        layout.context_layers = printing::context::derived_layers();
        layout.context_layers.push(context_layer("roads", "roads"));
        let document = br#"{"schema":"ds.print-context/v1","layers":{}}"#.to_vec();
        let sha256 = format!("{:x}", Sha256::digest(&document));
        let page = transformer_page(
            &engine,
            &settings(&receipt, &staging),
            &room("tx_a"),
            &layout,
            |_| {
                Ok(Some(PrintContextBytes {
                    bytes: document.clone(),
                    sha256: sha256.clone(),
                    layers: vec![
                        "contours_index".into(),
                        "contours_intermediate".into(),
                        "dsgrid_mv_angle_points".into(),
                    ],
                    omitted: vec!["buildings_context".into(), "dsgrid_mv_lines".into()],
                }))
            },
        )
        .unwrap();
        assert_eq!(
            page.context_layers,
            [
                "contours_index",
                "contours_intermediate",
                "dsgrid_mv_angle_points"
            ]
        );
        // Contours print as two document layers, the MV projection as the
        // three `map::mv_layers` emits: neither draft layer is omitted. The
        // buildings the host named as omitted and the catalogue layer the
        // document does not carry are, each with its own reason.
        assert_eq!(
            page.context_omitted,
            [
                ContextOmission {
                    layer: "buildings_context".into(),
                    reason: "not supplied from what this computer holds".into(),
                },
                ContextOmission {
                    layer: "roads".into(),
                    reason: "not carried by the print context document".into(),
                },
            ]
        );
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

    fn survey_layer(id: &str, form: &str) -> PrintContextLayer {
        PrintContextLayer {
            id: id.into(),
            label: id.into(),
            source: PrintContextSource::Survey { form: form.into() },
        }
    }

    fn capture(context_id: &str, declared_count: u64) -> HeldCapture {
        HeldCapture {
            context_id: context_id.into(),
            features: json!({"type":"FeatureCollection","features":[{"type":"Feature","properties":{"name":"S1"},"geometry":{"type":"Point","coordinates":[30.0605,-1.9405]}}]}),
            declared_count,
            render_mode: None,
            source_project: None,
            features_loaded: true,
        }
    }

    fn held(name: &str, captures: Vec<HeldCapture>) -> HeldState {
        HeldState {
            room: room(name),
            context: HeldContext {
                captures,
                retained: BTreeMap::new(),
                holdings: None,
            },
        }
    }

    fn request(layout: printing::Layout) -> PreviewRequest {
        PreviewRequest {
            transformer: "tx_a".into(),
            layout,
        }
    }

    #[test]
    fn execute_returns_the_page_for_an_admitted_room_and_names_what_this_host_cannot_read() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let receipt = receipt("Benin");
        let engine = FakeEngine::release();
        let page = execute(
            &engine,
            &settings(&receipt, &staging),
            &request(draft()),
            &held("tx_a", Vec::new()),
        )
        .unwrap();
        assert_eq!(page.transformer, "tx_a");
        assert_eq!(page.server_version, 7);
        assert_eq!(page.output_id, "svg__draft-a3");
        assert!(page.page.trim_start().starts_with("<svg"));
        // The engine's own names travel so a filesystem host places the page
        // and its receipt as a delivery would.
        assert_eq!(page.filename, "tx_a.draft-a3.svg");
        assert_eq!(page.receipt["elapsed_ms"], 5);
        assert_eq!(page.receipt["transformer"], "tx_a");
        // Without holdings the catalogue layers are omitted with the
        // resolver's reason, not a generic one.
        assert!(page.context_layers.is_empty());
        assert_eq!(
            page.context_omitted,
            [
                ContextOmission {
                    layer: "roads".into(),
                    reason: "no holdings root on this host".into(),
                },
                ContextOmission {
                    layer: "rivers".into(),
                    reason: "no holdings root on this host".into(),
                },
            ]
        );
        assert!(page.context_sha256.is_none());
        assert_eq!(engine.calls.load(Ordering::SeqCst), 1);
        assert!(nothing_left(&staging));

        // A room handed for another transformer is refused before anything runs.
        let other = execute(
            &engine,
            &settings(&receipt, &staging),
            &PreviewRequest {
                transformer: "tx_b".into(),
                layout: draft(),
            },
            &held("tx_a", Vec::new()),
        )
        .unwrap_err();
        assert_eq!(other.code(), CODE_INPUTS_INVALID);
        assert_eq!(other.class(), Class::InvalidInput);
        assert_eq!(engine.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn execute_resolves_exactly_the_drafts_layers_from_the_handed_captures() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        // The governed receipt's saved setup names buildings; the draft names
        // a survey layer and a catalogue layer. Only the draft's list is
        // resolved: the survey capture is admitted and staged, the catalogue
        // layer is omitted here, buildings never appear.
        let governed = governed();
        let engine = FakeEngine::release();
        let mut layout = draft();
        layout.context_layers = vec![
            survey_layer("survey_x", "F1"),
            context_layer("roads", "roads"),
        ];
        let page = execute(
            &engine,
            &settings(&governed, &staging),
            &request(layout),
            &held("tx_a", vec![capture("survey_x", 1)]),
        )
        .unwrap();
        assert_eq!(page.context_layers, ["survey_x"]);
        assert_eq!(
            page.context_omitted,
            [ContextOmission {
                layer: "roads".into(),
                reason: "no holdings root on this host".into(),
            }]
        );
        assert_eq!(page.context_sha256.as_ref().map(String::len), Some(64));
        assert_eq!(page.receipt["print_context_layers"], json!(["survey_x"]));
        assert_eq!(page.receipt["print_context_omitted"], json!(["roads"]));
        assert_eq!(engine.calls.load(Ordering::SeqCst), 1);
        assert!(nothing_left(&staging));
    }

    #[test]
    fn adjacent_preview_uses_separate_room_root_and_keeps_account_scope() {
        use ds_project_data::room_hold::{self, Room};

        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let geographic_root = temp.path().join("geographic");
        let room_root = temp.path().join("layers");
        let scope = Scope {
            principal: "owner".into(),
            project: "proj-1".into(),
        };
        let mut held = held("tx_a", Vec::new());
        let focus = Room {
            transformer: held.room.transformer.clone(),
            version: Some(held.room.server_version),
            content_digest: Some(report_export::jcs::layers_content_digest(&held.room.layers).unwrap()),
            layers: held.room.layers.clone(),
        };
        let layers = BTreeMap::from([
            (
                "tr".into(),
                json!({"type":"FeatureCollection","features":[{
                    "type":"Feature","properties":{"transfo":"tx_b","existing_status":"new"},
                    "geometry":{"type":"Point","coordinates":[30.0601,-1.94]}
                }]}),
            ),
            (
                "lv_lines".into(),
                json!({"type":"FeatureCollection","features":[{
                    "type":"Feature","properties":{"transfo":"tx_b","existing_status":"new"},
                    "geometry":{"type":"LineString","coordinates":[[30.0601,-1.94],[30.0602,-1.94]]}
                }]}),
            ),
        ]);
        let neighbor = Room {
            transformer: "tx_b".into(),
            version: Some(3),
            content_digest: Some(report_export::jcs::layers_content_digest(&layers).unwrap()),
            layers,
        };
        room_hold::hold_inputs(
            &room_root,
            &scope,
            &json!({"rows":[
                {"name":"tx_a","kind":"transformer","state":"active"},
                {"name":"tx_b","kind":"transformer","state":"active"}
            ]}),
        )
        .unwrap();
        room_hold::hold_room(&room_root, &scope, &focus).unwrap();
        room_hold::hold_room(&room_root, &scope, &neighbor).unwrap();
        held.context.holdings = Some(Holdings {
            root: geographic_root.clone(),
            room_root: Some(room_root),
            scope,
            catalog: Vec::new(),
        });
        let mut layout = draft();
        layout.context_layers.clear();
        layout
            .style_refs
            .insert("neighbor_transformers".into(), "context/tr_print".into());
        layout
            .style_refs
            .insert("neighbor_lv_lines".into(), "context/lv_print".into());
        let mut receipt = receipt("Benin");
        let mut sheets = receipt.sheets().unwrap();
        sheets["printing_styles"] = json!({
            "context/tr_print":{"type":"circle","paint":{"circle-radius":1,"circle-opacity":0.65,
                "circle-color":["match",["get","existing_status"],"new","#0F766E","existing","#295B86","#228B83"]}},
            "context/lv_print":{"type":"line","paint":{"line-width":1,"line-opacity":0.65,
                "line-color":["match",["get","existing_status"],"new","#0F766E","existing","#295B86","#228B83"]}}
        });
        receipt.sheets_json = serde_json::to_string(&sheets).unwrap();
        receipt.sheets_sha256 = format!("{:x}", Sha256::digest(receipt.sheets_json.as_bytes()));
        let engine = FakeEngine::release();
        let request = request(layout);
        let page = execute(&engine, &settings(&receipt, &staging), &request, &held).unwrap();
        assert!(page
            .context_layers
            .iter()
            .any(|id| id == "neighbor_transformers"));
        assert!(page
            .context_layers
            .iter()
            .any(|id| id == "neighbor_lv_lines"));
        assert!(page.context_omitted.is_empty());
        assert_eq!(held.room.layers, focus.layers);
        assert_eq!(
            held.context.holdings.as_ref().unwrap().root,
            geographic_root
        );

        let mut wrong_root = held.clone();
        wrong_root.context.holdings.as_mut().unwrap().room_root = Some(geographic_root);
        let omitted = execute(
            &engine,
            &settings(&receipt, &staging),
            &request,
            &wrong_root,
        )
        .unwrap();
        assert!(omitted.context_layers.is_empty());
        assert!(omitted
            .context_omitted
            .iter()
            .any(|row| row.layer == "neighbor_transformers"
                && row.reason == "no project print inventory is held"));

        held.context.holdings.as_mut().unwrap().scope.principal = "another-owner".into();
        let omitted = execute(&engine, &settings(&receipt, &staging), &request, &held).unwrap();
        assert!(omitted.context_layers.is_empty());
        assert!(omitted
            .context_omitted
            .iter()
            .any(|row| row.layer == "neighbor_transformers"
                && row.reason == "no project print inventory is held"));
        assert!(nothing_left(&staging));
    }

    #[test]
    fn a_capture_the_kernel_refuses_ends_the_preview_with_that_refusal_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let receipt = receipt("Benin");
        let engine = FakeEngine::release();
        let mut layout = draft();
        layout.context_layers = vec![survey_layer("survey_x", "F1")];
        let refused = execute(
            &engine,
            &settings(&receipt, &staging),
            &request(layout),
            &held("tx_a", vec![capture("survey_x", 0)]),
        )
        .unwrap_err();
        let PreviewRefusal::Kernel(kernel) = &refused else {
            panic!("{refused:?}");
        };
        assert_eq!(kernel.code, "print_context_survey_empty");
        assert_eq!(kernel.message_key, "printing_context_survey_empty");
        assert_eq!(kernel.params["name"], "F1");
        assert_eq!(refused.code(), "print_context_survey_empty");
        assert_eq!(refused.class(), Class::InvalidInput);
        assert_eq!(
            refused.detail(),
            Some(
                json!({"message_key": "printing_context_survey_empty", "params": {"bound": "50000", "name": "F1"}})
            )
        );
        assert!(refused.message().contains("F1"));
        // The envelope both doors write, from the one refusal.
        let error = envelope::error(COMMAND_ID, CONTRACT, &refused.envelope_refusal());
        assert_eq!(error["v"], 1);
        assert_eq!(error["command"], "report.project.export");
        assert_eq!(error["contract"], 1);
        assert_eq!(error["status"], "error");
        assert_eq!(error["error"]["class"], "invalid_input");
        assert_eq!(error["error"]["code"], "print_context_survey_empty");
        assert_eq!(error["error"]["retryable"], false);
        assert_eq!(
            error["error"]["detail"]["message_key"],
            "printing_context_survey_empty"
        );
        assert_eq!(engine.calls.load(Ordering::SeqCst), 0);
        assert!(nothing_left(&staging));
    }

    #[test]
    fn a_retained_projection_the_kernel_omits_is_named_on_the_sheet() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let receipt = receipt("Benin");
        let engine = FakeEngine::release();
        let mut layout = draft();
        layout.context_layers = vec![printing::context::derived_layers().swap_remove(0)];
        assert_eq!(layout.context_layers[0].id, "project_dsgrid_mv");
        let page = execute(
            &engine,
            &settings(&receipt, &staging),
            &request(layout),
            &held("tx_a", Vec::new()),
        )
        .unwrap();
        // Nothing retained for the MV projection: the draft layer is a named
        // omission with the kernel's reason, and the page still renders.
        assert!(page.context_layers.is_empty());
        assert_eq!(
            page.context_omitted,
            [ContextOmission {
                layer: "project_dsgrid_mv".into(),
                reason: "not retained on this computer".into(),
            }]
        );
        assert_eq!(engine.calls.load(Ordering::SeqCst), 1);
        assert!(nothing_left(&staging));
    }

    #[test]
    fn answer_pins_the_one_data_shape_for_both_doors() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("staging");
        let receipt = receipt("Benin");
        let engine = FakeEngine::release();
        let page = execute(
            &engine,
            &settings(&receipt, &staging),
            &request(draft()),
            &held("tx_a", Vec::new()),
        )
        .unwrap();

        let inline = answer("proj-1", "stable", &[(&page, PageRef::Inline)]);
        let keys =
            |value: &Value| -> Vec<String> { value.as_object().unwrap().keys().cloned().collect() };
        assert_eq!(
            keys(&inline),
            ["lane", "project", "preview", "results", "batch"]
        );
        assert_eq!(inline["lane"], "stable");
        assert_eq!(inline["project"], "proj-1");
        assert_eq!(
            keys(&inline["preview"]),
            [
                "layout_id",
                "layout_sha256",
                "output_id",
                "format",
                "note",
                "survey_forms_omitted"
            ]
        );
        assert_eq!(inline["preview"]["survey_forms_omitted"], json!([]));
        assert_eq!(inline["preview"]["layout_id"], "draft-a3");
        assert_eq!(inline["preview"]["layout_sha256"], page.layout_sha256);
        assert_eq!(inline["preview"]["output_id"], "svg__draft-a3");
        assert_eq!(inline["preview"]["format"], "svg");
        assert_eq!(inline["preview"]["note"], PREVIEW_NOTE);
        assert_eq!(
            inline["batch"],
            json!({"status": "completed", "requested_count": 1, "completed": 1, "failed": 0})
        );
        let row = &inline["results"][0];
        assert_eq!(
            keys(row),
            [
                "transformer",
                "server_version",
                "status",
                "page",
                "filename",
                "sha256",
                "size_bytes",
                "content_type",
                "engine",
                "elapsed_ms",
                "warnings",
                "print_diagnostics",
                "print_context",
            ]
        );
        assert_eq!(row["transformer"], "tx_a");
        assert_eq!(row["server_version"], 7);
        assert_eq!(row["status"], "completed");
        assert_eq!(row["page"], json!({"svg": page.page}));
        assert_eq!(row["filename"], "tx_a.draft-a3.svg");
        assert_eq!(row["sha256"], page.sha256);
        assert_eq!(row["size_bytes"], page.size_bytes);
        assert_eq!(row["content_type"], "image/svg+xml");
        assert_eq!(
            row["engine"],
            json!({"engine_version": page.engine.engine_version, "build_manifest_sha256": "f".repeat(64)})
        );
        assert_eq!(row["elapsed_ms"], 5);
        assert_eq!(row["warnings"], json!([]));
        assert_eq!(row["print_diagnostics"][0]["output_id"], "svg__draft-a3");
        assert_eq!(
            row["print_context"],
            json!({
                "sha256": null,
                "layers": [],
                "omitted": [
                    {"layer": "roads", "reason": "no holdings root on this host"},
                    {"layer": "rivers", "reason": "no holdings root on this host"},
                ],
            })
        );
        let success = envelope::success(COMMAND_ID, CONTRACT, inline);
        assert_eq!(
            keys(&success),
            ["v", "command", "contract", "status", "data"]
        );
        assert_eq!(success["status"], "ok");

        // The CLI names the page it placed; every other member is the same.
        let placed = temp.path().join("out/tx_a/tx_a.draft-a3.svg");
        let by_path = answer("proj-1", "stable", &[(&page, PageRef::File(&placed))]);
        assert_eq!(
            by_path["results"][0]["page"],
            json!({"path": placed.to_string_lossy()})
        );
        let mut same = by_path.clone();
        same["results"][0]["page"] = json!({"svg": page.page});
        assert_eq!(
            same,
            envelope::success(COMMAND_ID, CONTRACT, same.clone())["data"]
        );
        assert_eq!(
            same,
            answer("proj-1", "stable", &[(&page, PageRef::Inline)])
        );
    }

    #[test]
    fn every_code_a_preview_can_end_with_has_its_envelope_class() {
        let kernel_codes = [
            ("print_preview_room_unavailable", Class::Unavailable),
            ("print_preview_room_incomplete", Class::Conflict),
            ("print_context_capture_invalid", Class::InvalidInput),
            ("print_context_survey_empty", Class::InvalidInput),
            ("print_context_too_large", Class::InvalidInput),
            ("print_context_capture_changed", Class::InvalidInput),
            ("print_context_foreign_project", Class::InvalidInput),
            ("print_context_capture_incomplete", Class::InvalidInput),
            ("print_context_not_held_source", Class::InvalidInput),
            ("print_context_omitted", Class::InvalidInput),
        ];
        for (code, class) in kernel_codes {
            // An omission names a label and a reason; every other kernel
            // refusal names the thing by `name`.
            let (key, value) = if code == "print_context_omitted" {
                ("label", "Proposed MV")
            } else {
                ("name", "TX-0421")
            };
            let refusal = PreviewRefusal::Kernel(printing::preview::Refusal {
                code: code.into(),
                message_key: format!("printing_{}", code.trim_start_matches("print_")),
                params: BTreeMap::from([(key.to_string(), value.to_string())]),
            });
            assert_eq!(refusal.class(), class, "{code}");
            assert!(
                refusal.message().contains(value),
                "{code}: {}",
                refusal.message()
            );
            assert_eq!(refusal.detail().unwrap()["params"][key], value, "{code}");
        }
        // Every code this crate raises, and the ones its adapters and the
        // holdings resolver raise, is classified — and never as internal.
        let host_codes = [
            (crate::CODE_INPUTS_INVALID, Class::InvalidInput),
            ("print_context_unsupported", Class::InvalidInput),
            ("print_context_too_large", Class::InvalidInput),
            (crate::CODE_OUTPUT_EXISTS, Class::Conflict),
            (crate::CODE_ADMIN_BOUNDS_UNAVAILABLE, Class::Unavailable),
            ("reporter_engine_missing", Class::Unavailable),
            ("catalog_unavailable", Class::Unavailable),
            ("reference_bundle_unavailable", Class::Unavailable),
            (crate::CODE_STAGING_FAILED, Class::Failed),
            (crate::CODE_RESULT_INVALID, Class::Failed),
            (crate::CODE_ENGINE_REFUSED, Class::Failed),
            (crate::CODE_EXPORT_BLOCKED, Class::Failed),
            (crate::CODE_ENGINE_IDENTITY_INVALID, Class::Failed),
            ("callee_timed_out", Class::Failed),
            (crate::CODE_ENGINE_IDENTITY_CHANGED, Class::Failed),
            ("print_context_invalid", Class::Failed),
            ("project_dataset_store_failed", Class::Failed),
        ];
        for (code, class) in host_codes {
            let refusal = PreviewRefusal::Host(HostFailure::new(code, "why"));
            assert_eq!(refusal.class(), class, "{code}");
            assert_eq!(refusal.message(), "why");
            assert_eq!(refusal.detail(), None);
        }
        let mut listed: Vec<&str> = CLASS_TABLE.iter().map(|(code, _)| *code).collect();
        let mut expected: Vec<&str> = kernel_codes
            .iter()
            .chain(host_codes.iter())
            .map(|(code, _)| *code)
            .collect();
        listed.sort_unstable();
        listed.dedup();
        expected.sort_unstable();
        expected.dedup();
        assert_eq!(listed, expected, "the table names exactly these codes once");
        assert_eq!(CLASS_TABLE.len(), listed.len(), "no code is listed twice");
        assert!(CLASS_TABLE
            .iter()
            .all(|(_, class)| *class != Class::Internal));
        // A code the table does not know is a defect, and says so.
        assert_eq!(
            PreviewRefusal::Host(HostFailure::new("something_new", "why")).class(),
            Class::Internal
        );
        // A host detail travels as the envelope detail.
        let blocked = PreviewRefusal::Host(
            HostFailure::new(crate::CODE_EXPORT_BLOCKED, "blocked")
                .with_detail(json!({"blockers": [{"code": "format_build_failed"}]})),
        );
        let carried = blocked.envelope_refusal();
        assert_eq!(carried.class, Class::Failed);
        assert_eq!(carried.code, "export_blocked");
        assert_eq!(
            carried.detail.unwrap()["blockers"][0]["code"],
            "format_build_failed"
        );
    }

    /// The envelope corpus ds-cli replays records refusals by code and
    /// class; a case that names a code of this table under another class
    /// would teach the other side a class the doors never give it.
    #[test]
    fn the_envelope_corpus_never_contradicts_the_class_table() {
        let corpus: Value = serde_json::from_str(
            &fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/fixtures/ds-envelope-cases.json"),
            )
            .expect("the kernel's envelope corpus"),
        )
        .expect("the corpus parses");
        let mut checked = 0;
        for case in corpus["cases"].as_array().expect("cases") {
            let refusal = &case["request"]["refusal"];
            let Some(code) = refusal["code"].as_str() else {
                continue;
            };
            let Some((_, class)) = CLASS_TABLE.iter().find(|(known, _)| *known == code) else {
                continue;
            };
            assert_eq!(
                refusal["class"].as_str(),
                Some(class.token()),
                "{}: {code} is recorded under another class than the table gives it",
                case["id"]
            );
            checked += 1;
            // A localised refusal's detail names the kernel's own key for its
            // code, never another code's.
            if let Some(key) = refusal["detail"].get("message_key") {
                assert_eq!(
                    key.as_str(),
                    Some(format!("printing_{}", code.trim_start_matches("print_")).as_str()),
                    "{}: the message key is the kernel's spelling of the code",
                    case["id"]
                );
            }
        }
        assert!(
            checked >= 3,
            "the corpus records codes of this table under their classes"
        );
    }
}
