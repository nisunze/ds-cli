//! Template-local pens inherit symbols and categories from governed shared styles.
use super::{Layout, PrintStyleOverride};
use serde_json::{json, Value};

pub fn apply(document: &Value, change: &PrintStyleOverride, paper: &str) -> Result<Value, String> {
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
        expression.push(original);
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
    if let Some(label) = &change.label {
        use crate::print_scaling::{scaling_from_style, PrintUnits};
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

/// Validate template choices before the host acquires geographic context.
/// Rendering calls the same effective-style function, so there is one rule.
pub fn preflight(layout: &Layout, documents: &Value) -> Result<(), String> {
    for (layer, change) in &layout.style_overrides {
        let reference = layout
            .style_refs
            .get(layer)
            .ok_or_else(|| format!("layer {layer}: no governed style reference"))?;
        let document = documents
            .get(reference)
            .filter(|value| value.is_object())
            .ok_or_else(|| format!("layer {layer}: governed style {reference} is not prepared"))?;
        apply(document, change, layout.paper.expression_value())
            .map_err(|error| format!("layer {layer} ({reference}): {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hiding_absent_labels_is_a_no_op_but_authoring_needs_a_definition() {
        let base = json!({"type":"symbol", "layout":{"icon-image":"school"}, "paint":{}});
        let hidden: PrintStyleOverride =
            serde_json::from_value(json!({"label":{"visible":false,"size_pt":8}})).unwrap();
        assert_eq!(apply(&base, &hidden, "print_a3").unwrap(), base);
        let visible: PrintStyleOverride =
            serde_json::from_value(json!({"label":{"visible":true}})).unwrap();
        assert!(apply(&base, &visible, "print_a3")
            .unwrap_err()
            .contains("governed label"));
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
}
