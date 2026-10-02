//! Structural validation for direct JSON authorship. The service still owns
//! complete MapLibre property/schema validation when publishing.
use serde_json::Value;
use std::collections::BTreeSet;

fn expression(value: &Value, path: &str, depth: usize) -> Result<(), String> {
    if depth > 64 {
        return Err(format!("{path}: expression exceeds 64 levels"));
    }
    let Some(items) = value.as_array() else {
        return Ok(());
    };
    if items.first().and_then(Value::as_str) == Some("literal") {
        return Ok(());
    }
    if items.first().and_then(Value::as_str) == Some("match") {
        if items.len() < 5 || items.len() % 2 == 0 {
            return Err(format!(
                "{path}: match needs at least one label/output pair and a fallback"
            ));
        }
        let mut label_type = None;
        let mut seen = BTreeSet::new();
        for index in (2..items.len() - 1).step_by(2) {
            let labels = items[index]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_else(|| std::slice::from_ref(&items[index]));
            if labels.is_empty() {
                return Err(format!("{path}: match labels cannot be empty"));
            }
            for label in labels {
                let kind = match label {
                    Value::String(_) => "string",
                    Value::Number(n) if n.as_f64().is_some_and(|v| v.fract() == 0.) => "number",
                    _ => return Err(format!("{path}: match labels must be strings or integers")),
                };
                if label_type.is_some_and(|previous| previous != kind) {
                    return Err(format!("{path}: match labels must have one type"));
                }
                label_type = Some(kind);
                let identity = label.as_f64().map_or_else(
                    || label.to_string(),
                    |n| if n == 0. { "0".into() } else { n.to_string() },
                );
                if !seen.insert(identity) {
                    return Err(format!("{path}: duplicate match label {label}"));
                }
            }
            expression(&items[index + 1], path, depth + 1)?;
        }
        expression(&items[1], path, depth + 1)?;
        return expression(items.last().expect("fallback"), path, depth + 1);
    }
    for item in items {
        expression(item, path, depth + 1)?;
    }
    Ok(())
}

fn layer(
    document: &Value,
    path: &str,
    implicit_type: Option<&str>,
    depth: usize,
) -> Result<(), String> {
    if depth > 16 {
        return Err(format!("{path}: companion nesting exceeds 16 levels"));
    }
    let object = document
        .as_object()
        .ok_or_else(|| format!("{path}: style must be an object"))?;
    let kind = match object.get("type") {
        Some(value) => value
            .as_str()
            .ok_or_else(|| format!("{path}.type: expected a string"))?,
        None => implicit_type.ok_or_else(|| format!("{path}.type: layer type is required"))?,
    };
    if ![
        "background",
        "fill",
        "line",
        "symbol",
        "raster",
        "circle",
        "fill-extrusion",
        "heatmap",
        "hillshade",
        "color-relief",
    ]
    .contains(&kind)
    {
        return Err(format!("{path}.type: unknown MapLibre layer type {kind}"));
    }
    for bucket in ["paint", "layout", "metadata"] {
        if let Some(value) = object.get(bucket) {
            let properties = value
                .as_object()
                .ok_or_else(|| format!("{path}.{bucket}: expected an object"))?;
            if bucket != "metadata" {
                for (property, value) in properties {
                    if property.starts_with("fill_pattern") {
                        return Err(format!(
                            "{path}.{bucket}.{property}: hatch recipe belongs in metadata"
                        ));
                    }
                    expression(value, &format!("{path}.{bucket}.{property}"), 0)?;
                }
            }
        }
    }
    if let Some(visibility) = document["layout"].get("visibility") {
        if visibility != "visible" && visibility != "none" {
            return Err(format!(
                "{path}.layout.visibility: expected visible or none"
            ));
        }
    }
    let mut zoom = [0., 24.];
    for (index, key) in ["minzoom", "maxzoom"].into_iter().enumerate() {
        if let Some(value) = object.get(key) {
            zoom[index] = value
                .as_f64()
                .filter(|v| (0.0..=24.0).contains(v))
                .ok_or_else(|| format!("{path}.{key}: expected a number from 0 through 24"))?;
        }
    }
    if zoom[0] > zoom[1] {
        return Err(format!("{path}: minzoom must not exceed maxzoom"));
    }
    if let Some(filter) = object.get("filter") {
        expression(filter, &format!("{path}.filter"), 0)?;
    }
    for (key, implicit) in [
        ("label", "symbol"),
        ("boundary", "line"),
        ("d3", "fill-extrusion"),
    ] {
        if let Some(child) = object.get(key) {
            layer(child, &format!("{path}.{key}"), Some(implicit), depth + 1)?;
        }
    }
    Ok(())
}

pub fn validate(document: &Value) -> Result<(), String> {
    layer(document, "style", None, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn empty_label_and_icon_matches_are_refused_including_companions() {
        for property in ["text-field", "icon-image"] {
            for value in [
                json!(["match", ["get", "kind"], "fallback"]),
                json!(["match", ["get", "kind"], "a", "fallback"]),
            ] {
                let document = json!({"type": "circle", "label": {"layout": {property: value}}});
                assert!(validate(&document)
                    .unwrap_err()
                    .contains("match needs at least one"));
            }
        }
    }
    #[test]
    fn raw_json_preserves_expressions_outside_guided_authoring() {
        let document = json!({"type": "symbol", "paint": {"icon-color": ["interpolate", ["linear"], ["zoom"], 0, "red", 20, "blue"]},
            "layout": {"icon-image": ["match", ["get", "kind"], ["a", "b"], "ds-circle", "ds-transformer"]},
            "label": {"layout": {"text-field": ["match", ["get", "kind"], "a", "Label", ""]}}});
        assert!(validate(&document).is_ok());
        assert!(validate(&json!({"type": "symbol", "layout": {"text-field": "fallback"}})).is_ok());
    }
    #[test]
    fn malformed_structure_and_label_types_are_refused() {
        for document in [
            json!([]),
            json!({"type": "bogus"}),
            json!({"type": "line", "paint": []}),
            json!({"type": "line", "minzoom": 10, "maxzoom": 9}),
            json!({"type": "fill", "layout": {"fill_pattern_color": "red"}}),
            json!({"type": "line", "paint": {"line-color": ["match", ["get", "x"], [], "red", "black"]}}),
            json!({"type": "line", "paint": {"line-color": ["match", ["get", "x"], 1, "red", "a", "green", "black"]}}),
            json!({"type": "line", "paint": {"line-color": ["match", ["get", "x"], 1, "red", 1.0, "green", "black"]}}),
        ] {
            assert!(validate(&document).is_err(), "{document}");
        }
        assert!(validate(
            &json!({"type": "symbol", "layout": {"text-font": ["literal", ["match", "is data"]]}})
        )
        .is_ok());
    }
}
