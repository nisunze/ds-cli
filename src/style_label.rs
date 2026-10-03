//! Label controls shared by native CLI/MCP and the desktop host.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LabelOptions {
    pub visible: Option<bool>,
    pub min_zoom: Option<f64>,
    pub max_zoom: Option<f64>,
    pub size: Option<f64>,
    pub font: Option<String>,
    pub color: Option<String>,
    #[serde(default)]
    pub numerics: std::collections::BTreeMap<String, f64>,
    #[serde(default)]
    pub append_fields: Vec<String>,
    pub separator: Option<String>,
    pub suffix: Option<String>,
    pub format: Option<String>,
    pub halo_color: Option<String>,
    pub allow_overlap: Option<bool>,
    pub alignment: Option<String>,
    /// Paper names, or the single value `all` to remove the paper restriction.
    pub papers: Option<Vec<String>>,
    /// Try the backend's non-central point anchors to avoid collisions.
    pub automatic_placement: Option<bool>,
}

/// Layer zoom bounds live beside paint/layout, including on label and boundary
/// children. Validate the merged range before changing either bound.
pub(crate) fn apply_zoom_range(
    layer: &mut Value,
    min_zoom: Option<f64>,
    max_zoom: Option<f64>,
) -> Result<(), String> {
    if min_zoom.is_none() && max_zoom.is_none() {
        return Ok(());
    }
    let bound = |key: &str, requested: Option<f64>, default: f64| {
        let value = match requested {
            Some(value) => value,
            None => match layer.get(key) {
                Some(value) => value
                    .as_f64()
                    .ok_or_else(|| format!("saved {key} must be numeric"))?,
                None => default,
            },
        };
        if !value.is_finite() || !(0.0..=24.0).contains(&value) {
            return Err(format!("{key} must be between 0 and 24"));
        }
        Ok(value)
    };
    let min = bound("minzoom", min_zoom, 0.0)?;
    let max = bound("maxzoom", max_zoom, 24.0)?;
    if min > max {
        return Err("minzoom must not exceed maxzoom, including a retained bound".into());
    }
    if let Some(value) = min_zoom {
        layer["minzoom"] = json!(value);
    }
    if let Some(value) = max_zoom {
        layer["maxzoom"] = json!(value);
    }
    Ok(())
}

pub fn apply_options(
    label: &mut Value,
    options: &LabelOptions,
    snapshot: &Value,
    editor: &Value,
) -> Result<(), String> {
    apply_zoom_range(label, options.min_zoom, options.max_zoom)?;
    if options.size.is_some() && options.numerics.contains_key("text-size") {
        return Err("use size or numeric text-size, not both".into());
    }
    if options.append_fields.len() > 8
        || options.separator.as_ref().is_some_and(|s| s.len() > 80)
        || options.suffix.as_ref().is_some_and(|s| s.len() > 80)
    {
        return Err("composed labels exceed the bounded part length".into());
    }
    if options.separator.is_some() && options.append_fields.is_empty() {
        return Err("a label separator requires append fields".into());
    }
    for field in &options.append_fields {
        if !editor["available_fields"]
            .as_array()
            .is_some_and(|a| a.contains(&json!(field)))
        {
            return Err("appended label field is absent from the published vocabulary".into());
        }
    }
    let format = options.format.as_deref().unwrap_or("raw");
    if options.format.is_some()
        && !snapshot["label_editor"]["field_formats"]
            .as_array()
            .is_some_and(|a| a.contains(&json!(format)))
    {
        return Err("label format is not published by the backend".into());
    }
    let mut primary = label["layout"]["text-field"].clone();
    if format != "raw" {
        primary = match format {
            "round" => json!(["round", ["to-number", primary, 0]]),
            "int" => json!(["floor", ["to-number", primary, 0]]),
            _ => return Err("unsupported published field format".into()),
        };
    }
    if !options.append_fields.is_empty() || options.suffix.is_some() {
        if !snapshot["label_editor"]["field_part_types"]
            .as_array()
            .is_some_and(|a| a.contains(&json!("literal")))
        {
            return Err("backend does not support literal label parts".into());
        }
        let mut parts = vec![json!("concat"), json!(["to-string", primary])];
        for field in &options.append_fields {
            parts.extend([
                json!(options.separator.as_deref().unwrap_or(" · ")),
                json!(["to-string", ["coalesce", ["get", field], ""]]),
            ]);
        }
        if let Some(suffix) = &options.suffix {
            parts.push(json!(suffix));
        }
        primary = json!(parts);
    }
    if options.format.is_some() || !options.append_fields.is_empty() || options.suffix.is_some() {
        label["layout"]["text-field"] = primary;
    }
    if let Some(alignment) = &options.alignment {
        if !snapshot["label_editor"]["placements"]
            .as_array()
            .is_some_and(|a| a.contains(&json!(alignment)))
        {
            return Err("label alignment must be a published placement".into());
        }
        label["layout"]["symbol-placement"] = json!(alignment);
        if alignment != "point" {
            label["layout"]["text-rotation-alignment"] = json!("map");
            label["layout"]["text-keep-upright"] = json!(true);
        }
    }
    if let Some(allow) = options.allow_overlap {
        label["layout"]["text-allow-overlap"] = json!(allow);
    }
    if let Some(color) = &options.halo_color {
        if color.len() != 7
            || !color.starts_with('#')
            || !color[1..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("label halo color must be a six-digit hexadecimal color".into());
        }
        if !label["paint"].is_object() {
            label["paint"] = json!({});
        }
        label["paint"]["text-halo-color"] = json!(color);
    }
    if let Some(visible) = options.visible {
        label["layout"]["visibility"] = json!(if visible { "visible" } else { "none" });
    }
    if let Some(size) = options.size {
        let range = &snapshot["label_editor"]["numerics"]["text-size"];
        let low = range["min"]
            .as_f64()
            .ok_or("backend label size bounds are missing")?;
        let high = range["max"]
            .as_f64()
            .ok_or("backend label size bounds are missing")?;
        if !size.is_finite() || !(low..=high).contains(&size) {
            return Err(format!("label size must be between {low} and {high}"));
        }
        label["layout"]["text-size"] = json!(size);
    }
    if let Some(font) = &options.font {
        let fonts = snapshot["label_editor"]["fonts"]
            .as_array()
            .ok_or("backend label font vocabulary is missing")?;
        if !fonts.contains(&json!(font)) {
            return Err("label font must be from the published font vocabulary".into());
        }
        label["layout"]["text-font"] = json!([font]);
    }
    if let Some(color) = &options.color {
        if color.len() != 7
            || !color.starts_with('#')
            || !color[1..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("label color must be a six-digit hexadecimal color".into());
        }
        if !label["paint"].is_object() {
            label["paint"] = json!({});
        }
        label["paint"]["text-color"] = json!(color);
    }
    for (key, value) in &options.numerics {
        let range = &snapshot["label_editor"]["numerics"][key];
        let low = range["min"]
            .as_f64()
            .ok_or("label numeric property is not published")?;
        let high = range["max"]
            .as_f64()
            .ok_or("label numeric bounds are missing")?;
        if !value.is_finite() || !(low..=high).contains(value) {
            return Err(format!("label {key} must be between {low} and {high}"));
        }
        let group = if key == "text-opacity" || key.starts_with("text-halo-") {
            "paint"
        } else {
            "layout"
        };
        if !label[group].is_object() {
            label[group] = json!({});
        }
        label[group][key] = json!(value);
    }
    if let Some(automatic) = options.automatic_placement {
        if automatic {
            if label["layout"]["symbol-placement"]
                .as_str()
                .is_some_and(|v| v != "point")
            {
                return Err("automatic point anchors require point label placement".into());
            }
            let anchors: Vec<_> = snapshot["label_editor"]["anchors"]
                .as_array()
                .ok_or("backend label anchors are missing")?
                .iter()
                .filter(|v| v.as_str().is_some_and(|s| s != "center"))
                .cloned()
                .collect();
            if anchors.is_empty() || anchors.len() > 8 {
                return Err("backend point label anchors are invalid".into());
            }
            label["layout"]["text-variable-anchor"] = json!(anchors);
            label["layout"]["text-allow-overlap"] = json!(options.allow_overlap.unwrap_or(false));
            label["layout"]["text-ignore-placement"] = json!(false);
        } else {
            label["layout"]
                .as_object_mut()
                .ok_or("label layout is missing")?
                .remove("text-variable-anchor");
        }
    }
    if let Some(papers) = &options.papers {
        if editor["style_target"] != "print" {
            return Err("paper-specific labels require a governed print style".into());
        }
        let all = papers == &["all".to_string()];
        let domain = editor["field_values"]["print_paper_size"]
            .as_array()
            .ok_or("backend print paper vocabulary is missing")?;
        let mut tokens = Vec::new();
        for paper in papers.iter().filter(|_| !all) {
            let token = format!("print_{}", paper.to_lowercase());
            if !domain.contains(&json!(token)) || tokens.contains(&token) {
                return Err(
                    "label papers must be unique names from the published print paper vocabulary"
                        .into(),
                );
            }
            tokens.push(token);
        }
        if papers.is_empty() || papers.len() > 7 {
            return Err("label papers must contain one through seven paper names, or all".into());
        }
        if !label["paint"].is_object() {
            label["paint"] = json!({});
        }
        let authored = label["paint"]
            .get("text-opacity")
            .cloned()
            .unwrap_or(json!(1));
        // Recognize only our exact wrapper. Updating the paper list or clearing
        // it keeps the original opacity, without accumulating nested masks.
        let opacity = if authored.as_array().is_some_and(|v| {
            v.len() == 5
                && v[0] == "match"
                && v[1] == json!(["get", "print_paper_size"])
                && v[2].is_array()
                && v[4] == 0
        }) {
            authored[3].clone()
        } else {
            authored
        };
        label["paint"]["text-opacity"] = if all {
            opacity
        } else {
            json!(["match", ["get", "print_paper_size"], tokens, opacity, 0])
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_zoom_preserves_content_and_validates_retained_bounds() {
        let original = json!({"minzoom":4,"maxzoom":20,
            "layout":{"text-field":["get","cell"],"text-size":12},
            "paint":{"text-opacity":0.8},"filter":["has","cell"]});
        let mut label = original.clone();
        apply_options(
            &mut label,
            &LabelOptions {
                min_zoom: Some(15.),
                ..Default::default()
            },
            &json!({}),
            &json!({}),
        )
        .unwrap();
        assert_eq!(label["minzoom"], 15.);
        for key in ["maxzoom", "layout", "paint", "filter"] {
            assert_eq!(label[key], original[key]);
        }
        for value in [-1., 25., f64::NAN, f64::INFINITY, 21.] {
            let mut candidate = original.clone();
            assert!(apply_zoom_range(&mut candidate, Some(value), None).is_err());
            assert_eq!(candidate, original);
        }
        assert!(apply_zoom_range(&mut label, None, Some(14.)).is_err());
        apply_zoom_range(&mut label, Some(0.), Some(24.)).unwrap();
        assert_eq!(label["minzoom"], 0.);
        assert_eq!(label["maxzoom"], 24.);
    }

    #[test]
    fn composed_labels_use_declared_fields_and_line_alignment() {
        let schema = json!({"label_editor":{"field_part_types":["literal","field"],"field_formats":["round","raw"],"placements":["point","line"]}});
        let editor = json!({"available_fields":["size"]});
        let mut label = json!({"layout":{"text-field":["get","name"]}});
        let mut options = LabelOptions {
            append_fields: vec!["size".into()],
            suffix: Some(" kVA".into()),
            alignment: Some("line".into()),
            allow_overlap: Some(true),
            ..Default::default()
        };
        apply_options(&mut label, &options, &schema, &editor).unwrap();
        assert_eq!(label["layout"]["text-field"][0], "concat");
        assert_eq!(
            label["layout"]["text-field"][3],
            json!(["to-string", ["coalesce", ["get", "size"], ""]])
        );
        assert_eq!(label["layout"]["text-rotation-alignment"], "map");
        options.append_fields = vec!["unknown".into()];
        assert!(apply_options(&mut label, &options, &schema, &editor).is_err());
    }

    #[test]
    fn numeric_label_controls_use_the_backend_bounds_and_property_groups() {
        let schema = json!({"label_editor":{"numerics":{"symbol-sort-key":{"min":-1000,"max":1000},"text-halo-width":{"min":0,"max":10}}}});
        let mut label = json!({"layout":{},"paint":{}});
        let mut options = LabelOptions::default();
        options.numerics.insert("symbol-sort-key".into(), -1000.);
        options.numerics.insert("text-halo-width".into(), 1.5);
        apply_options(&mut label, &options, &schema, &json!({})).unwrap();
        assert_eq!(label["layout"]["symbol-sort-key"], -1000.);
        assert_eq!(label["paint"]["text-halo-width"], 1.5);
        options.numerics.insert("arbitrary".into(), 1.);
        assert!(apply_options(&mut label, &options, &schema, &json!({})).is_err());
    }

    #[test]
    fn fonts_are_governed_and_text_color_is_validated() {
        let schema = json!({"label_editor":{"fonts":["Open Sans Italic"]}});
        let mut label = json!({"layout":{}});
        let mut options = LabelOptions {
            font: Some("Open Sans Italic".into()),
            color: Some("#52646B".into()),
            ..Default::default()
        };
        apply_options(&mut label, &options, &schema, &json!({})).unwrap();
        assert_eq!(label["layout"]["text-font"], json!(["Open Sans Italic"]));
        assert_eq!(label["paint"]["text-color"], "#52646B");
        options.color = Some("white".into());
        assert!(apply_options(&mut label, &options, &schema, &json!({})).is_err());
        options.color = None;
        options.font = Some("Unknown".into());
        assert!(apply_options(&mut label, &options, &schema, &json!({})).is_err());
    }

    #[test]
    fn paper_scope_round_trips_without_losing_opacity_or_filter() {
        let original = json!({"layout":{"visibility":"none"},"paint":{"text-opacity":0.85},"filter":["has","pole_number"]});
        let mut label = original.clone();
        let editor = json!({"style_target":"print","field_values":{"print_paper_size":["print_a0","print_a3"]}});
        let mut options = LabelOptions {
            papers: Some(vec!["A0".into()]),
            ..Default::default()
        };
        apply_options(&mut label, &options, &json!({}), &editor).unwrap();
        assert_eq!(
            label["paint"]["text-opacity"],
            json!(["match", ["get", "print_paper_size"], ["print_a0"], 0.85, 0])
        );
        options.papers = Some(vec!["A3".into()]);
        apply_options(&mut label, &options, &json!({}), &editor).unwrap();
        assert_eq!(label["paint"]["text-opacity"][3], 0.85);
        options.papers = Some(vec!["all".into()]);
        apply_options(&mut label, &options, &json!({}), &editor).unwrap();
        assert_eq!(label, original);
        assert!(apply_options(
            &mut label,
            &options,
            &json!({}),
            &json!({"style_target":"design"})
        )
        .is_err());
    }

    #[test]
    fn labels_enable_with_backend_bounded_sizes_and_collision_anchors() {
        let mut label = json!({"layout":{"visibility":"none"},"paint":{}});
        let schema = json!({"label_editor":{"anchors":["center","top","bottom"],"numerics":{"text-size":{"min":8,"max":48}}}});
        let mut options = LabelOptions {
            visible: Some(true),
            size: Some(8.),
            automatic_placement: Some(true),
            ..Default::default()
        };
        apply_options(&mut label, &options, &schema, &json!({})).unwrap();
        assert_eq!(label["layout"]["visibility"], "visible");
        assert_eq!(
            label["layout"]["text-variable-anchor"],
            json!(["top", "bottom"])
        );
        assert_eq!(label["layout"]["text-ignore-placement"], false);
        options.size = Some(7.);
        assert!(apply_options(&mut label, &options, &schema, &json!({})).is_err());
    }
}
