//! Preview one MV printing layout against held DS Grid engine projections.
//!
//! A template is seen before it is adopted: the previewed layout, saved or
//! draft, is resolved as if it were the project's selection, under the
//! project's effective issue identity (its fixed external version and issue
//! date), and bound to the project's style table. The answer is one complete
//! `render-grid-plan-profile` request for a `preview_only`, `svg_only` sample sheet. A
//! preview reads no approved preserved page and writes no selection,
//! publication or model state; model-specific preserved pages belong to an
//! adopted setup and a print.

use super::{mv, project_crs, Layout};
use crate::style_resolution::Snapshot;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// The canonical command a host previews MV templates through.
pub const COMMAND_ID: &str = "report.plan-profile";
/// The previewed layout, by the command's declared input name.
pub const PREVIEW_LAYOUT_ARG: &str = "preview-layout";
/// A preview proves the template on one representative sheet, never the booklet.
pub const SAMPLE_PAGES: usize = 1;
/// The paper/renderer binding refusal, shared with `report project mv-setup`.
pub const STYLE_UNRESOLVED: &str = "mv_print_style_unresolved";

/// Where the host staged the projections and where the engine writes.
#[derive(Debug, Clone, Copy)]
pub struct Paths<'a> {
    pub scene: &'a Path,
    pub plan: &'a Path,
    pub out_dir: &'a Path,
}

/// The resolved previewed setup and the exact engine request for it.
#[derive(Debug, Clone)]
pub struct Preview {
    pub setup: mv::Resolved,
    pub request: Value,
}

/// The previewed content's identity: a draft has no server revision, so the
/// preview pins the layout bytes it renders.
pub fn revision(layout: &Layout) -> Result<String, mv::Refusal> {
    let bytes = serde_json::to_vec(layout).map_err(|_| {
        refuse(
            "mv_print_template_invalid",
            "layout",
            "Repair the layout document.",
        )
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// Resolve `layout` for `project_id` from the server-issued configuration
/// `sheets` and the project's style `table`, and compose the engine request.
pub fn request(
    project_id: &str,
    sheets: &Value,
    layout: &Layout,
    table: &Snapshot,
    project_crs: Option<&project_crs::Capture>,
    model_crs: Option<&str>,
    paths: Paths<'_>,
) -> Result<Preview, mv::Refusal> {
    if layout.mv.is_none() {
        return Err(refuse(
            "mv_print_template_invalid",
            "mv",
            "Preview a printing layout carrying an MV plan and profile setup.",
        ));
    }
    if !sheets.is_object() {
        return Err(refuse(
            "mv_print_setup_missing",
            "sheets",
            "Read the project configuration, then preview again.",
        ));
    }
    // The issue identity is the project's: never today's date, never a guess.
    let issue = mv::effective_selection(sheets)?;
    let selection = mv::Selection {
        schema: mv::SELECTION_SCHEMA.into(),
        layout_id: layout.id.clone(),
        revision: revision(layout)?,
        external_version: issue.external_version,
        issue_date: issue.issue_date,
    };
    let mut previewed = sheets.clone();
    let mut rows: Vec<Value> = previewed["project_settings"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|row| {
            crate::report_formats::setting_parameter_key(row).as_deref() != Some(mv::SETTING)
        })
        .collect();
    rows.push(json!({"parameter": mv::SETTING, "value": selection}));
    previewed["project_settings"] = json!(rows);
    previewed["printing_setups"] = json!([{
        "id": layout.id,
        "revision": selection.revision,
        "layout": layout,
        "inherited_from": null,
    }]);
    let setup = mv::resolve(&previewed, project_id, BTreeMap::new())?;
    if !setup.preserved_pages.is_empty() {
        return Err(refuse(
            "mv_print_preview_preserved_pages",
            "pages",
            "Preview names no model identity; print an adopted setup to include approved model pages.",
        ));
    }
    let (paper, renderer) = mv::resolve_print_bindings(&setup, table).map_err(|error| {
        refuse(
            STYLE_UNRESOLVED,
            "style_resolution",
            &format!("Adopt the governed MV paper and renderer styles for this project: {error}"),
        )
    })?;
    let request = json!({
        "project_crs": project_crs,
        "report_config": previewed,
        "preview_only": true,
        "svg_only": true,
        "project_id": project_id,
        "scene_path": paths.scene,
        "plan_path": paths.plan,
        "side_profiles_path": null,
        "notes_path": null,
        "structure_descriptions_path": null,
        "out_dir": paths.out_dir,
        "settings": setup.settings,
        "style_resolution": paper,
        "renderer_defaults": renderer,
        "mv_setup": setup,
        "publication_assets": {},
        "sample_pages": SAMPLE_PAGES,
        "model_crs": model_crs,
        "context_page_files": [],
        "logo_files": [],
    });
    Ok(Preview { setup, request })
}

fn refuse(code: &str, field: &str, remedy: &str) -> mv::Refusal {
    mv::Refusal {
        code: code.into(),
        message_key: code.into(),
        field: field.into(),
        remedy: remedy.into(),
    }
}

/// The same admitted template over every native station sheet. Only an exact
/// native progress target enables this mode; ordinary samples stay bounded.
pub fn all_pages(
    mut preview: Preview,
    target: &super::preview_progress::Target,
) -> Result<Preview, mv::Refusal> {
    target.validate().map_err(|_| {
        refuse(
            "mv_print_template_invalid",
            "preview_progress",
            "Use the native preview job's exact private target.",
        )
    })?;
    preview.request["sample_pages"] = Value::Null;
    preview.request["preview_progress"] = serde_json::to_value(target).map_err(|_| {
        refuse(
            "mv_print_template_invalid",
            "preview_progress",
            "Repair the native preview target.",
        )
    })?;
    Ok(preview)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Sample,
    Start,
    Read {
        input_sha256: String,
        page: Option<usize>,
    },
}

/// One closed control policy shared by headless and paired preview doors.
pub fn action(
    has_layout: bool,
    all: bool,
    job: Option<&str>,
    page: Option<usize>,
    retention: bool,
) -> Result<Action, mv::Refusal> {
    let invalid = || {
        refuse("request_mode_invalid","preview-job",
        "Use --preview-layout with optional --preview-all/--preview-retention, or --preview-job with optional --preview-page.")
    };
    if let Some(job) = job {
        if has_layout
            || all
            || retention
            || !super::preview_progress::digest(job)
            || page.is_some_and(|page| page == 0 || page > super::preview_progress::MAX_PAGES)
        {
            return Err(invalid());
        }
        return Ok(Action::Read {
            input_sha256: job.into(),
            page,
        });
    }
    if page.is_some() || (!has_layout && (all || retention)) {
        return Err(invalid());
    }
    Ok(if !has_layout {
        Action::None
    } else if all {
        Action::Start
    } else {
        Action::Sample
    })
}
