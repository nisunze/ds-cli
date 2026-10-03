//! One portable, digest-pinned page inventory for native and WASM consumers.
//! Hosts render each page once and use those exact bytes for preview and PDF.
use super::mv::{valid_digest, Resolved};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PageRole {
    Cover,
    Naming,
    SheetIndex,
    KeyPlan,
    LegendNotes,
    PlanProfile,
    Context,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub id: String,
    pub role: PageRole,
    /// Exact laid-out SVG or approved PDF component bytes.
    pub content_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: String,
    pub project_id: String,
    pub model_revision: String,
    pub setup_sha256: String,
    pub external_version: String,
    pub issue_date: String,
    pub page_mm: [f64; 2],
    pub preview_dpi: u16,
    pub pages: Vec<Page>,
    pub sha256: String,
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub fn plan(setup: &Resolved, revision: &str, pages: Vec<Page>) -> Result<Plan, String> {
    setup
        .validate(&setup.project_id)
        .map_err(|e| e.to_string())?;
    if revision.is_empty()
        || revision.len() > 200
        || revision.chars().any(char::is_control)
        || pages.is_empty()
        || pages.len() > 10_000
    {
        return Err(
            "mv_booklet_plan_invalid: bounded model revision and page inventory required".into(),
        );
    }
    let mut ids = BTreeSet::new();
    if pages.iter().any(|page| {
        !super::identifier(&page.id) || !ids.insert(&page.id) || !valid_digest(&page.content_sha256)
    }) {
        return Err(
            "mv_booklet_plan_invalid: unique page ids and exact content digests required".into(),
        );
    }
    let mut result = Plan {
        schema: "ds.mv-booklet-plan/v1".into(),
        project_id: setup.project_id.clone(),
        model_revision: revision.into(),
        setup_sha256: setup.sha256.clone(),
        external_version: setup.selection.external_version.clone(),
        issue_date: setup.selection.issue_date.clone(),
        page_mm: setup.layout.page_mm,
        preview_dpi: 96,
        pages,
        sha256: String::new(),
    };
    result.sha256 = sha256(&serde_json::to_vec(&result).map_err(|e| e.to_string())?);
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub page_id: String,
    pub png_sha256: String,
    /// Decoded RGBA bytes; independent of PNG compression or PDF timestamps.
    pub raster_sha256: String,
    pub png_data_url: String,
}

/// The browser and native CLI admit the same ordered PNG set against the
/// plan digest. No DOM state, active project, filename or PDF timestamp participates.
pub fn preview_document(plan: &Plan, previews: &[Preview]) -> Result<serde_json::Value, String> {
    use base64::Engine as _;
    if plan.schema != "ds.mv-booklet-plan/v1"
        || previews.len() != plan.pages.len()
        || plan.preview_dpi != 96
        || plan
            .page_mm
            .iter()
            .any(|mm| !mm.is_finite() || !(10.0..=1500.0).contains(mm))
    {
        return Err(
            "mv_booklet_preview_invalid: preview inventory differs from layout plan".into(),
        );
    }
    let mut checked = plan.clone();
    checked.sha256.clear();
    if plan.sha256 != sha256(&serde_json::to_vec(&checked).map_err(|e| e.to_string())?) {
        return Err("mv_booklet_preview_invalid: layout plan digest changed".into());
    }
    for (page, preview) in plan.pages.iter().zip(previews) {
        let data = preview
            .png_data_url
            .strip_prefix("data:image/png;base64,")
            .ok_or("mv_booklet_preview_invalid: PNG data URL required")?;
        if data.len() > 40_000_000 {
            return Err("mv_booklet_preview_invalid: PNG exceeds limit".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| e.to_string())?;
        let expected = plan
            .page_mm
            .map(|mm| (mm * f64::from(plan.preview_dpi) / 25.4).round() as u32);
        let dimensions = imagesize::blob_size(&bytes).map_err(|e| e.to_string())?;
        if [dimensions.width as u32, dimensions.height as u32] != expected
            || u64::from(expected[0]) * u64::from(expected[1]) > 20_000_000
        {
            return Err("mv_booklet_preview_invalid: bounded paper dimensions required".into());
        }
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?
            .to_rgba8();
        if preview.page_id != page.id
            || sha256(&bytes) != preview.png_sha256
            || sha256(decoded.as_raw()) != preview.raster_sha256
            || [decoded.width(), decoded.height()] != expected
        {
            return Err(
                "mv_booklet_preview_invalid: page identity, dimensions or raster digest differs"
                    .into(),
            );
        }
    }
    Ok(serde_json::json!({"schema":"ds.mv-booklet-preview/v1", "plan":plan, "pages":previews}))
}
