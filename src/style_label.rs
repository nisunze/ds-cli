//! Label controls shared by native CLI/MCP and the desktop host.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LabelOptions {
    pub visible: Option<bool>,
    pub size: Option<f64>,
    pub font: Option<String>,
    pub color: Option<String>,
    /// Paper names, or the single value `all` to remove the paper restriction.
    pub papers: Option<Vec<String>>,
    /// Try the backend's non-central point anchors to avoid collisions.
    pub automatic_placement: Option<bool>,
}

pub fn apply_options(
    label: &mut Value,
    options: &LabelOptions,
    snapshot: &Value,
    editor: &Value,
) -> Result<(), String> {
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
            label["layout"]["text-allow-overlap"] = json!(false);
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
