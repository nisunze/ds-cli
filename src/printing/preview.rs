//! May a live transformer preview be produced right now, and how it is shown.
//!
//! The editor reports facts — the draft, the transformer it chose (as the
//! printable inventory described it), which governed print styles it holds,
//! whether it is online, whether its native reporter is ready and how many
//! device pixels one paper millimetre currently spans — and receives one
//! plan: ready or the refusals by message key, the page the engine will
//! produce, and the raster the host should draw it at. Rendering is a host
//! effect (`report_export::preview` is the recipe both hosts run); the page
//! itself is the engine's SVG, which the host rasterises for its canvas at
//! the size decided here and re-rasterises, without the engine, when the
//! zoom changes.
use super::Layout;
use crate::report_formats::PrintArtifactFormat;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The transformer the editor chose, as `printable_transformers` listed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transformer {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    /// Whether this device holds the room; `None` when unknown.
    #[serde(default)]
    pub cached: Option<bool>,
    /// Whether the held room carries unsaved edits; `None` when unknown.
    #[serde(default)]
    pub dirty: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    pub layout: Layout,
    #[serde(default)]
    pub transformer: Option<Transformer>,
    #[serde(default)]
    pub available_style_refs: Vec<String>,
    #[serde(default)]
    pub online: bool,
    /// The host can run the report engine locally (desktop-native placement).
    #[serde(default)]
    pub native_ready: bool,
    /// Device pixels per paper millimetre at the editor's current zoom; the
    /// raster is chosen to match it, so the page is sharp at the zoom it is
    /// looked at and no larger than the budget allows.
    #[serde(default)]
    pub screen_px_per_mm: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Refusal {
    pub code: String,
    pub message_key: String,
    #[serde(default)]
    pub params: std::collections::BTreeMap<String, String>,
}

/// The bitmap a host draws the page at: whole device pixels, and the scale
/// the SVG's millimetre units are multiplied by to reach them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Raster {
    pub width_px: u32,
    pub height_px: u32,
    pub px_per_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Plan {
    pub ready: bool,
    pub refusals: Vec<Refusal>,
    /// Facts the host should show beside a ready preview, by message key.
    pub notes: Vec<Refusal>,
    pub transformer: Option<String>,
    pub output_id: String,
    pub format: PrintArtifactFormat,
    pub raster: Raster,
    /// Every governed print style the render needs, in layout order.
    pub print_style_refs: Vec<String>,
}

/// The least a page is rasterised at: legible on a screen when the whole
/// sheet is fitted to a small window.
pub const MIN_PX_PER_MM: f64 = 1.5;
/// The most: the delivery raster (300 dpi) — nothing is gained past it.
pub const MAX_PX_PER_MM: f64 = 300. / 25.4;
/// The bitmap budget a webview holds comfortably; an A0 at 300 dpi would be
/// 140 megapixels, which no preview needs.
pub const MAX_RASTER_PIXELS: f64 = 40_000_000.;

/// The raster for a page of `page_mm` looked at through `screen_px_per_mm`:
/// the screen scale within bounds, then shrunk to the pixel budget.
pub fn raster(page_mm: [f64; 2], screen_px_per_mm: Option<f64>) -> Raster {
    let wanted = match screen_px_per_mm {
        Some(scale) if scale.is_finite() && scale > 0. => scale,
        _ => 96. / 25.4,
    }
    .clamp(MIN_PX_PER_MM, MAX_PX_PER_MM);
    let area = page_mm[0] * page_mm[1];
    let budget = (MAX_RASTER_PIXELS / area).sqrt();
    let px_per_mm = wanted.min(budget).max(MIN_PX_PER_MM);
    Raster {
        width_px: (page_mm[0] * px_per_mm).round().max(1.) as u32,
        height_px: (page_mm[1] * px_per_mm).round().max(1.) as u32,
        px_per_mm,
    }
}

fn refusal(code: &str, key: &str, params: &[(&str, &str)]) -> Refusal {
    Refusal {
        code: code.into(),
        message_key: key.into(),
        params: params
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

pub fn plan(facts: &Facts) -> Result<Plan, String> {
    super::validate(&facts.layout)?;
    let mut refusals = Vec::new();
    let mut notes = Vec::new();
    let mut refs: Vec<String> = Vec::new();
    for reference in facts.layout.style_refs.values() {
        if !refs.contains(reference) {
            refs.push(reference.clone());
        }
        if !facts.available_style_refs.contains(reference) {
            refusals.push(refusal(
                "print_style_unavailable",
                "printing_style_unavailable",
                &[("reference", reference)],
            ));
        }
    }
    match &facts.transformer {
        None => refusals.push(refusal(
            "print_preview_no_transformer",
            "printing_preview_no_transformer",
            &[],
        )),
        Some(transformer) => {
            if crate::report_export::reportable_transformer(&transformer.name).is_err()
                || (!transformer.kind.is_empty() && transformer.kind != "transformer")
            {
                refusals.push(refusal(
                    "print_preview_not_a_transformer",
                    "printing_preview_not_a_transformer",
                    &[("name", &transformer.name)],
                ));
            }
            if transformer.cached == Some(false) && !facts.online {
                refusals.push(refusal(
                    "print_preview_room_unavailable",
                    "printing_preview_room_unavailable",
                    &[("name", &transformer.name)],
                ));
            }
            if transformer.dirty == Some(true) {
                notes.push(refusal(
                    "print_preview_dirty_room",
                    "printing_preview_dirty_room",
                    &[("name", &transformer.name)],
                ));
            }
        }
    }
    if !facts.native_ready {
        refusals.push(refusal(
            "print_preview_native_unavailable",
            "printing_preview_native_unavailable",
            &[],
        ));
    }
    Ok(Plan {
        ready: refusals.is_empty(),
        refusals,
        notes,
        transformer: facts.transformer.as_ref().map(|t| t.name.clone()),
        output_id: format!("svg__{}", facts.layout.id),
        format: crate::report_export::preview::FORMAT,
        raster: raster(facts.layout.page_mm, facts.screen_px_per_mm),
        print_style_refs: refs,
    })
}

pub fn evaluate(facts: Facts) -> Result<Value, String> {
    serde_json::to_value(plan(&facts)?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn facts() -> Facts {
        let mut layout = super::super::default_layout();
        layout
            .style_refs
            .insert("lv_lines".into(), "master/lv_lines_print".into());
        Facts {
            layout,
            transformer: Some(Transformer {
                name: "agahehe".into(),
                kind: "transformer".into(),
                cached: Some(true),
                dirty: Some(false),
            }),
            available_style_refs: vec!["master/lv_lines_print".into()],
            online: true,
            native_ready: true,
            screen_px_per_mm: None,
        }
    }

    #[test]
    fn a_held_transformer_with_its_styles_previews_as_the_shared_page() {
        let plan = plan(&facts()).unwrap();
        assert!(plan.ready && plan.refusals.is_empty() && plan.notes.is_empty());
        assert_eq!(plan.output_id, format!("svg__{}", facts().layout.id));
        assert_eq!(json!(plan.format), json!("svg"));
        assert_eq!(plan.print_style_refs, ["master/lv_lines_print"]);
        assert_eq!(plan.transformer.as_deref(), Some("agahehe"));
        // A3 landscape at the 96 dpi default: 1587 × 1123.
        assert_eq!((plan.raster.width_px, plan.raster.height_px), (1587, 1123));
    }

    #[test]
    fn the_raster_follows_the_screen_scale_within_its_budget() {
        let a0 = [1189., 841.];
        // Fit-to-page on a laptop is below the floor: the floor wins.
        let fitted = raster(a0, Some(1.1));
        assert_eq!(fitted.px_per_mm, MIN_PX_PER_MM);
        assert_eq!((fitted.width_px, fitted.height_px), (1784, 1262));
        let zoomed = raster(a0, Some(5.));
        assert_eq!(zoomed.px_per_mm, 5.);
        assert_eq!((zoomed.width_px, zoomed.height_px), (5945, 4205));
        // Far in on an A0: the pixel budget, not the delivery raster, caps it.
        let deep = raster(a0, Some(40.));
        assert!(deep.px_per_mm < MAX_PX_PER_MM);
        assert!(f64::from(deep.width_px) * f64::from(deep.height_px) <= MAX_RASTER_PIXELS * 1.01);
        // A small sheet reaches the delivery raster before the budget.
        let a4 = raster([297., 210.], Some(40.));
        assert_eq!(a4.px_per_mm, MAX_PX_PER_MM);
        assert_eq!(raster(a0, Some(f64::NAN)).px_per_mm, 96. / 25.4);
        assert_eq!(raster(a0, None).px_per_mm, 96. / 25.4);
    }

    #[test]
    fn every_missing_fact_is_named_by_key() {
        let mut f = facts();
        f.transformer = None;
        f.available_style_refs.clear();
        f.native_ready = false;
        let first = plan(&f).unwrap();
        let codes: Vec<&str> = first.refusals.iter().map(|r| r.code.as_str()).collect();
        assert_eq!(
            codes,
            [
                "print_style_unavailable",
                "print_preview_no_transformer",
                "print_preview_native_unavailable"
            ]
        );
        assert_eq!(
            first.refusals[0].params["reference"],
            "master/lv_lines_print"
        );
        assert!(!first.ready);

        let mut f = facts();
        f.transformer.as_mut().unwrap().cached = Some(false);
        f.online = false;
        assert_eq!(
            plan(&f).unwrap().refusals[0].code,
            "print_preview_room_unavailable"
        );
        f.online = true;
        assert!(plan(&f).unwrap().ready);

        let mut f = facts();
        f.transformer.as_mut().unwrap().kind = "project_document".into();
        assert_eq!(
            plan(&f).unwrap().refusals[0].code,
            "print_preview_not_a_transformer"
        );
        let mut f = facts();
        f.transformer.as_mut().unwrap().name = "combined_transformer".into();
        assert_eq!(
            plan(&f).unwrap().refusals[0].code,
            "print_preview_not_a_transformer"
        );

        let mut f = facts();
        f.transformer.as_mut().unwrap().dirty = Some(true);
        let noted = plan(&f).unwrap();
        assert!(noted.ready);
        assert_eq!(noted.notes[0].code, "print_preview_dirty_room");
    }
}
