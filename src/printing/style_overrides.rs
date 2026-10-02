//! Template-local pens inherit symbols and categories from governed shared styles.
use super::{Layout, PrintStyleOverride};
use serde_json::{Value, json};

pub fn apply(document: &Value, change: &PrintStyleOverride, paper: &str) -> Result<Value, String> {
    if let Some(policy) = &change.scale_weight {
        policy.factor(policy.reference_denominator)?;
        if document["type"] != "line" || change.size.is_none() {
            return Err("scale_weight requires a line style and an explicit base size".into());
        }
    }
    let mut style = document.clone();
    if let Some(visible) = change.visible {
        style["layout"]["visibility"] = json!(if visible { "visible" } else { "none" });
        if style["boundary"].is_object() {
            style["boundary"]["layout"]["visibility"] =
                json!(if visible { "visible" } else { "none" });
        }
    }

    let (color, size, opacity) = match style["type"].as_str() {
        Some("line") => ("line-color", "line-width", "line-opacity"),
        Some("fill") => ("fill-color", "", "fill-opacity"),
        Some("circle") => ("circle-color", "circle-radius", "circle-opacity"),
        Some("symbol") => ("icon-color", "icon-size", "icon-opacity"),
        _ => return Err("template override needs a supported governed geometry style".into()),
    };
    if let Some(value) = &change.color {
        style["paint"][color] = json!(value);
    }
    if !change.palette.is_empty() {
        let original = style["paint"][color].clone();
        let field = if let Some(field) = &change.field {
            json!(["get", field])
        } else if original[0] == "match" && original[1][0] == "get" && original[1][1].is_string() {
            original[1].clone()
        } else {
            return Err("categorical template overrides need a category field or a primary match/get color expression".into());
        };
        let mut expression = vec![json!("match"), field];
        for (value, color) in &change.palette {
            expression.extend([json!(value), json!(color)]);
        }
        expression.push(
            change
                .palette_fallback
                .as_ref()
                .map_or(original, |c| json!(c)),
        );
        style["paint"][color] = json!(expression);
    }
    if let Some(value) = change.size {
        if size.is_empty() {
            return Err("fill layers have no base size; use polygon boundary controls".into());
        }
        let group = if size == "icon-size" {
            "layout"
        } else {
            "paint"
        };
        style[group][size] = json!(value);
    }
    if !change.size_by_value.is_empty() {
        if size.is_empty() {
            return Err("fill layers have no base size".into());
        }
        let field = change
            .field
            .as_ref()
            .ok_or("categorical sizes require a field")?;
        let group = if size == "icon-size" {
            "layout"
        } else {
            "paint"
        };
        let mut expression = vec![json!("match"), json!(["get", field])];
        for (value, amount) in &change.size_by_value {
            expression.extend([json!(value), json!(amount)]);
        }
        expression.push(style[group][size].clone());
        style[group][size] = json!(expression);
    }
    if document["type"] == "line" && (change.size.is_some() || !change.size_by_value.is_empty()) {
        if let (Some(inner), Some(outer), Some(color)) = (
            document["paint"]["line-width"].as_f64(),
            document["boundary"]["paint"]["line-width"].as_f64(),
            document["boundary"]["paint"]["line-color"].as_str(),
        ) {
            let casing = (outer - inner) / 2.;
            if casing > 0. {
                style = crate::style_authoring::apply_casing(style, color, casing)?;
            }
        }
    }
    if let Some(value) = change.opacity {
        style["paint"][opacity] = json!(value);
    }
    if change.color_scope == super::PrintColorScope::BodyAndBoundary
        && style["boundary"].is_object()
    {
        let boundary_type = style["boundary"]["type"].as_str().unwrap_or("line");
        let (boundary_color, boundary_opacity) = match boundary_type {
            "line" => ("line-color", "line-opacity"),
            "circle" => ("circle-color", "circle-opacity"),
            "fill" => ("fill-color", "fill-opacity"),
            "symbol" => ("icon-color", "icon-opacity"),
            _ => {
                return Err("boundary recolouring needs a supported governed geometry style".into());
            }
        };
        if change.color.is_some() || !change.palette.is_empty() {
            style["boundary"]["paint"][boundary_color] = style["paint"][color].clone();
        }
        if let Some(value) = change.opacity {
            style["boundary"]["paint"][boundary_opacity] = json!(value);
        }
    }
    if let Some(label) = &change.label {
        use crate::print_scaling::{PrintUnits, scaling_from_style};
        if !style["label"].is_object() {
            if label.visible == Some(false) {
                return Ok(style);
            }
            return Err("label override requires a governed label document".into());
        }
        let scaling = scaling_from_style(document)?;
        let factor = scaling
            .as_ref()
            .map(|p| p.factor(paper))
            .transpose()?
            .unwrap_or(1.);
        let css = scaling
            .as_ref()
            .is_some_and(|p| p.units == PrintUnits::CssPx);
        if let Some(visible) = label.visible {
            style["label"]["layout"]["visibility"] =
                json!(if visible { "visible" } else { "none" });
        }
        if let Some(size) = label.size_pt {
            style["label"]["layout"]["text-size"] =
                json!(size / factor * if css { 96. / 72. } else { 1. });
        }
        if let Some(halo) = label.halo_mm {
            style["label"]["paint"]["text-halo-width"] =
                json!(halo / factor * if css { 96. / 25.4 } else { 1. });
        }
        if let Some(color) = &label.color {
            style["label"]["paint"]["text-color"] = json!(color);
        }
        if let Some(color) = &label.halo_color {
            style["label"]["paint"]["text-halo-color"] = json!(color);
        }
        if let Some(overlap) = label.allow_overlap {
            style["label"]["layout"]["text-allow-overlap"] = json!(overlap);
        }
        if let Some(priority) = label.priority {
            style["label"]["layout"]["symbol-sort-key"] = json!(priority);
        }
    }
    Ok(style)
}

/// One part of a saved template override that the render could not use.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SkippedPart {
    /// `palette`, `size`, `size_by_value`, `scale_weight` or `label`; or
    /// `override` when the governed style takes no part of it at all.
    pub part: &'static str,
    /// The refusal [`apply`] gives for that part.
    pub reason: String,
}

/// The render-time rule for an already-saved layout. [`apply`] refuses an
/// unusable override outright, which is right while a person authors it and
/// wrong once the layout is saved: one layer's bad choice would refuse the
/// whole sheet. Here each independent part is tried with only what it reads;
/// a part the governed style cannot take is dropped and named, and the rest
/// still applies. The returned override always passes [`apply`].
pub fn usable(
    document: &Value,
    change: &PrintStyleOverride,
    paper: &str,
) -> (PrintStyleOverride, Vec<SkippedPart>) {
    if apply(document, change, paper).is_ok() {
        return (change.clone(), Vec::new());
    }
    let whole = |reason: String| {
        let part = "override";
        (
            PrintStyleOverride::default(),
            vec![SkippedPart { part, reason }],
        )
    };
    if let Err(reason) = apply(document, &PrintStyleOverride::default(), paper) {
        return whole(reason);
    }
    let refused = |part: &'static str, present: bool, probe: PrintStyleOverride| {
        present
            .then(|| apply(document, &probe, paper).err())
            .flatten()
            .map(|reason| SkippedPart { part, reason })
    };
    let only = PrintStyleOverride::default;
    let mut kept = change.clone();
    let mut skipped = Vec::new();
    let palette = PrintStyleOverride {
        color: kept.color.clone(),
        palette: kept.palette.clone(),
        palette_fallback: kept.palette_fallback.clone(),
        field: kept.field.clone(),
        ..only()
    };
    if let Some(part) = refused("palette", !kept.palette.is_empty(), palette) {
        kept.palette.clear();
        kept.palette_fallback = None;
        skipped.push(part);
    }
    let size = PrintStyleOverride {
        size: kept.size,
        ..only()
    };
    if let Some(part) = refused("size", kept.size.is_some(), size) {
        kept.size = None;
        skipped.push(part);
    }
    let sizes = PrintStyleOverride {
        size_by_value: kept.size_by_value.clone(),
        field: kept.field.clone(),
        ..only()
    };
    if let Some(part) = refused("size_by_value", !kept.size_by_value.is_empty(), sizes) {
        kept.size_by_value.clear();
        skipped.push(part);
    }
    // After `size`: a scale policy needs the explicit size that remains.
    let weight = PrintStyleOverride {
        scale_weight: kept.scale_weight.clone(),
        size: kept.size,
        ..only()
    };
    if let Some(part) = refused("scale_weight", kept.scale_weight.is_some(), weight) {
        kept.scale_weight = None;
        skipped.push(part);
    }
    let label = PrintStyleOverride {
        label: kept.label.clone(),
        ..only()
    };
    if let Some(part) = refused("label", kept.label.is_some(), label) {
        kept.label = None;
        skipped.push(part);
    }
    match apply(document, &kept, paper) {
        Ok(_) => (kept, skipped),
        // Parts that refuse only together: the governed style prints as is.
        Err(reason) => whole(reason),
    }
}

/// Capture the governed document's optical policy into the same declarative
/// override used by map strokes and legend samples. Explicit template policy
/// wins; no geometry, style identity or pen value is selected here.
pub fn inherit_scale_policy(
    document: &Value,
    change: &PrintStyleOverride,
) -> Result<PrintStyleOverride, String> {
    let Some(value) = document["metadata"].get("scale_weight") else {
        return Ok(change.clone());
    };
    let policy: super::PrintPenScale = serde_json::from_value(value.clone())
        .map_err(|e| format!("invalid governed scale_weight: {e}"))?;
    policy.factor(policy.reference_denominator)?;
    if change.scale_weight.is_some() {
        return Ok(change.clone());
    }
    if document["type"] != "line" {
        return Err("governed scale_weight requires a line document".into());
    }
    let mut resolved = change.clone();
    resolved.size = resolved
        .size
        .or_else(|| document["paint"]["line-width"].as_f64());
    if resolved.size.is_none() {
        return Err("governed scale_weight needs a scalar line-width or an explicit declarative base-size override; categorical or zoom width expressions are preserved".into());
    }
    resolved.scale_weight = Some(policy);
    Ok(resolved)
}

/// Resolve once at the fitted/explicit map scale, shared by map strokes and legend samples.
pub fn at_scale(
    change: &PrintStyleOverride,
    denominator: f64,
) -> Result<PrintStyleOverride, String> {
    let mut resolved = change.clone();
    if let Some(policy) = resolved.scale_weight.take() {
        let factor = policy.factor(denominator)?;
        resolved.size = resolved.size.map(|size| size * factor);
        for size in resolved.size_by_value.values_mut() {
            *size *= factor;
        }
    }
    Ok(resolved)
}

/// Check template choices before the host acquires geographic context. Every
/// overridden layer must name a governed style the host prepared; that is
/// the host's own integrity and still refuses. An override part the style
/// cannot take does not: the render follows [`usable`], printing the rest and
/// naming what it skipped, so the plan and the page keep one rule.
pub fn preflight(layout: &Layout, documents: &Value) -> Result<(), String> {
    for layer in layout.style_overrides.keys() {
        let reference = layout
            .style_refs
            .get(layer)
            .ok_or_else(|| format!("layer {layer}: no governed style reference"))?;
        documents
            .get(reference)
            .filter(|value| value.is_object())
            .ok_or_else(|| format!("layer {layer}: governed style {reference} is not prepared"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optical_line_weight_scales_by_square_root_and_stops_at_legibility_bounds() {
        let change:PrintStyleOverride=serde_json::from_value(json!({"size":2.,"field":"class","size_by_value":{"primary":4.},"scale_weight":{"reference_denominator":10000.,"min_factor":0.7,"max_factor":1.5}})).unwrap();
        let distant = at_scale(&change, 40000.).unwrap();
        assert_eq!(distant.size, Some(1.4));
        assert_eq!(distant.size_by_value["primary"], 2.8);
        assert!(distant.scale_weight.is_none());
        assert_eq!(at_scale(&change, 2500.).unwrap().size, Some(3.));
        assert!(apply(&json!({"type":"circle","paint":{}}), &change, "print_a3").is_err());
    }
    #[test]
    fn governed_scale_policy_reaches_shared_stroke_and_legend_path() {
        let document = json!({"type":"line","paint":{"line-width":2.5,"line-color":"#123456"},"metadata":{"scale_weight":{"reference_denominator":10000.,"min_factor":0.7,"max_factor":1.5}}});
        let inherited = inherit_scale_policy(&document, &PrintStyleOverride::default()).unwrap();
        let distant = at_scale(&inherited, 40000.).unwrap();
        assert_eq!(
            apply(&document, &distant, "A3").unwrap()["paint"]["line-width"],
            1.75
        );
        assert_eq!(document["paint"]["line-width"], 2.5);
        let explicit:PrintStyleOverride=serde_json::from_value(json!({"size":4.,"scale_weight":{"reference_denominator":10000.,"min_factor":1.,"max_factor":1.}})).unwrap();
        assert_eq!(
            inherit_scale_policy(&document, &explicit).unwrap(),
            explicit
        );
        let mut categorical = document.clone();
        categorical["paint"]["line-width"] = json!(["match", ["get", "cable"], "a", 2, 1]);
        assert!(
            inherit_scale_policy(&categorical, &PrintStyleOverride::default())
                .unwrap_err()
                .contains("scalar")
        );
    }
    #[test]
    fn hiding_absent_labels_is_a_no_op_but_authoring_needs_a_definition() {
        let base = json!({"type":"symbol", "layout":{"icon-image":"school"}, "paint":{}});
        let hidden: PrintStyleOverride =
            serde_json::from_value(json!({"label":{"visible":false,"size_pt":8}})).unwrap();
        assert_eq!(apply(&base, &hidden, "print_a3").unwrap(), base);
        let visible: PrintStyleOverride =
            serde_json::from_value(json!({"label":{"visible":true}})).unwrap();
        assert!(
            apply(&base, &visible, "print_a3")
                .unwrap_err()
                .contains("governed label")
        );
    }

    #[test]
    fn road_categories_override_flat_pens_without_mutating_the_shared_style() {
        let base = json!({"type":"line","paint":{"line-color":"#F4EEE2","line-width":4},"boundary":{"paint":{"line-color":"#999999","line-width":6}}});
        let change: PrintStyleOverride = serde_json::from_value(json!({"field":"type","size":2.8,"palette":{"District":"#D6D5D1"},"size_by_value":{"National":4.4,"District":3.4}})).unwrap();
        let out = apply(&base, &change, "print_a3").unwrap();
        assert_eq!(
            out["paint"]["line-color"],
            json!(["match", ["get", "type"], "District", "#D6D5D1", "#F4EEE2"])
        );
        assert_eq!(
            out["paint"]["line-width"],
            json!([
                "match",
                ["get", "type"],
                "District",
                3.4,
                "National",
                4.4,
                2.8
            ])
        );
        assert_eq!(
            out["boundary"]["paint"]["line-width"],
            json!([
                "match",
                ["get", "type"],
                "District",
                5.4,
                "National",
                6.4,
                4.8
            ])
        );
        assert_eq!(base["paint"]["line-width"], 4);
    }
    #[test]
    fn a_project_palette_keeps_shared_symbols_and_unmodified_categories() {
        let base = json!({"type":"symbol","layout":{"icon-image":"square","icon-size":1},"paint":{"icon-color":["match",["get","type"],"A","#ffff00","B","#ee33ff","#999999"]}});
        let mut change = PrintStyleOverride::default();
        change.palette.insert("A".into(), "#A88D00".into());
        let overridden = apply(&base, &change, "print_a3").unwrap();
        assert_eq!(overridden["layout"], base["layout"]);
        assert_eq!(
            overridden["paint"]["icon-color"][4],
            base["paint"]["icon-color"]
        );
        assert_eq!(base["paint"]["icon-color"][3], "#ffff00");
    }
    #[test]
    fn per_template_labels_have_constant_physical_size_without_changing_defaults() {
        let base = json!({"type":"line","paint":{},"label":{"layout":{"text-size":10}},"metadata":{"print_scaling":{"reference_paper":"A3","units":"css_px","factors":{"A0":2.,"A1":1.,"A2":1.,"A3":1.,"A4":1.,"A5":1.,"Custom":1.}}}});
        let change: PrintStyleOverride =
            serde_json::from_value(json!({"label":{"size_pt":8,"allow_overlap":false}})).unwrap();
        let a0 = apply(&base, &change, "print_a0").unwrap();
        let a3 = apply(&base, &change, "print_a3").unwrap();
        assert!(
            (a0["label"]["layout"]["text-size"].as_f64().unwrap() * 2. * 0.75 - 8.).abs() < 1e-8
        );
        assert!((a3["label"]["layout"]["text-size"].as_f64().unwrap() * 0.75 - 8.).abs() < 1e-8);
        assert_eq!(base["label"]["layout"]["text-size"], 10);
    }

    /// Gisagara A0 `lv_lines`, 2026-09-25: a saved palette with no field over a
    /// flat governed colour refused the whole PDF. The render now drops that
    /// palette alone, names it, and keeps the template's other choices.
    #[test]
    fn a_saved_palette_without_a_category_is_skipped_and_named_not_refused() {
        let lv_lines = json!({"type":"line","paint":{"line-color":"#1F4E79","line-width":1.5}});
        let change: PrintStyleOverride = serde_json::from_value(
            json!({"palette":{"Existing":"#555555"},"size":2.5,"opacity":0.9}),
        )
        .unwrap();
        let refusal = apply(&lv_lines, &change, "print_a0").unwrap_err();
        assert!(refusal.contains("need a category field"), "{refusal}");

        let (kept, skipped) = usable(&lv_lines, &change, "print_a0");
        assert_eq!(
            skipped,
            vec![SkippedPart {
                part: "palette",
                reason: refusal
            }]
        );
        assert!(kept.palette.is_empty());
        let printed = apply(&lv_lines, &kept, "print_a0").unwrap();
        assert_eq!(printed["paint"]["line-color"], "#1F4E79");
        assert_eq!(printed["paint"]["line-width"], 2.5);
        assert_eq!(printed["paint"]["line-opacity"], 0.9);
    }

    #[test]
    fn usable_overrides_pass_whole_and_unstyleable_documents_skip_everything() {
        let base = json!({"type":"line","paint":{"line-color":"#F4EEE2","line-width":4}});
        let change: PrintStyleOverride =
            serde_json::from_value(json!({"field":"type","palette":{"District":"#D6D5D1"}}))
                .unwrap();
        assert_eq!(usable(&base, &change, "print_a3"), (change.clone(), vec![]));

        // Each part is judged alone: a fill has no size, the rest applies.
        let fill = json!({"type":"fill","paint":{"fill-color":"#EEEEEE"}});
        let sized: PrintStyleOverride =
            serde_json::from_value(json!({"size":2.,"color":"#123456","visible":false})).unwrap();
        let (kept, skipped) = usable(&fill, &sized, "print_a3");
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].part, "size");
        let printed = apply(&fill, &kept, "print_a3").unwrap();
        assert_eq!(printed["paint"]["fill-color"], "#123456");
        assert_eq!(printed["layout"]["visibility"], "none");

        let raster = json!({"type":"raster","paint":{}});
        let (kept, skipped) = usable(&raster, &sized, "print_a3");
        assert_eq!(kept, PrintStyleOverride::default());
        assert_eq!(skipped[0].part, "override");
        assert!(
            skipped[0]
                .reason
                .contains("supported governed geometry style")
        );
    }

    #[test]
    fn map_preflight_refuses_only_unprepared_styles_not_unusable_parts() {
        let mut layout = crate::printing::default_layout();
        layout
            .style_refs
            .insert("lv_lines".into(), "master/lv_lines_print".into());
        let change: PrintStyleOverride =
            serde_json::from_value(json!({"palette":{"Existing":"#555555"}})).unwrap();
        layout.style_overrides.insert("lv_lines".into(), change);
        let documents = json!({"master/lv_lines_print":
            {"type":"line","paint":{"line-color":"#1F4E79","line-width":1.5}}});
        preflight(&layout, &documents).unwrap();
        assert!(
            preflight(&layout, &json!({}))
                .unwrap_err()
                .contains("is not prepared")
        );
    }
}
