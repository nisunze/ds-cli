//! Style-document rules the print contract applies on both sides: the
//! canonical style digest, number normalisation, colour spelling, the casing a
//! physical print line gains and the fields a style reads. The Kernel's style
//! authoring, plan and pinned-context modules re-export them.

use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

/// Canonical authored JSON digest; catalogue-provided digests take precedence
/// where a seeded document and its stored head have separate provenance.
pub fn style_digest(document: &Value) -> String {
    use sha2::{Digest, Sha256};
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(object) => Value::Object(
                object
                    .iter()
                    .map(|(k, v)| (k.clone(), canonical(v)))
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            Value::Array(values) => Value::Array(values.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }
    let mut document = document.clone();
    normalize_json_numbers(&mut document);
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&canonical(&document)).expect("JSON style"))
    )
}

pub fn normalize_style_color(value: &str) -> Option<String> {
    let value = value.trim();
    ((value.len() == 7 || value.len() == 9)
        && value.starts_with('#')
        && value[1..].bytes().all(|c| c.is_ascii_hexdigit()))
    .then(|| value.to_ascii_uppercase())
}
pub fn object(value: &Value) -> Result<&Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| "style must be an object".into())
}
pub fn bucket<'a>(style: &'a mut Value, key: &str) -> Result<&'a mut Map<String, Value>, String> {
    let root = style.as_object_mut().ok_or("style must be an object")?;
    let value = root.entry(key).or_insert_with(|| json!({}));
    value
        .as_object_mut()
        .ok_or_else(|| format!("{key} must be an object"))
}
pub fn kind(style: &Value) -> Result<&str, String> {
    object(style)?;
    style["type"]
        .as_str()
        .ok_or_else(|| "style type is required".into())
}

pub fn output_indices(value: &Value) -> Result<Vec<usize>, String> {
    let expr = value
        .as_array()
        .ok_or("line-width is not a widenable expression")?;
    let n = expr.len();
    match expr.first().and_then(Value::as_str) {
        Some("interpolate") if n >= 5 && (n - 3) % 2 == 0 => Ok((4..n).step_by(2).collect()),
        Some("step") if n >= 5 && n % 2 == 1 => Ok((2..n).step_by(2).collect()),
        Some("match") if n >= 5 && n % 2 == 1 => Ok((3..n - 1)
            .step_by(2)
            .chain(std::iter::once(n - 1))
            .collect()),
        Some("case") if n >= 4 && n % 2 == 0 => Ok((2..n - 1)
            .step_by(2)
            .chain(std::iter::once(n - 1))
            .collect()),
        _ => Err("line-width operator or branch list is not widenable without guessing".into()),
    }
}
pub fn widen_line_width(value: &Value, delta: f64) -> Result<Value, String> {
    if !delta.is_finite() {
        return Err("width delta must be finite".into());
    }
    if value.is_null() {
        return Ok(json!(1. + delta));
    }
    if let Some(v) = value.as_f64() {
        let widened = v + delta;
        if !widened.is_finite() {
            return Err("line width is not finite".into());
        }
        return Ok(json!(widened));
    }
    let indices = output_indices(value)?;
    let mut next = value.clone();
    for index in indices {
        let v = value[index]
            .as_f64()
            .ok_or("line-width branch is not a number")?;
        if !(v + delta).is_finite() {
            return Err("line-width overflow".into());
        }
        next[index] = json!(v + delta);
    }
    Ok(next)
}
pub fn apply_casing(mut style: Value, color: &str, width: f64) -> Result<Value, String> {
    if kind(&style)? != "line" {
        return Err("a casing requires a line style".into());
    }
    if !width.is_finite() || width <= 0. || width > 20. {
        return Err("casing width must be greater than 0 and no more than 20".into());
    }
    let widened = widen_line_width(&style["paint"]["line-width"], width * 2.)?;
    let mut boundary = style
        .get("boundary")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    // Boundaries have an implicit line type in the governed style document.
    boundary.as_object_mut().expect("object").remove("type");
    boundary["paint"] = json!({"line-color":color,"line-width":widened});
    let layout = bucket(&mut boundary, "layout")?;
    let visibility = if layout.get("visibility") == Some(&json!("none")) {
        "none"
    } else {
        "visible"
    };
    layout.insert("visibility".into(), json!(visibility));
    layout.insert("line-cap".into(), json!("round"));
    layout.insert("line-join".into(), json!("round"));
    for key in ["filter", "minzoom", "maxzoom"] {
        if let Some(value) = style.get(key).filter(|v| key == "filter" || v.is_number()) {
            boundary[key] = value.clone();
        } else {
            boundary.as_object_mut().expect("object").remove(key);
        }
    }
    style["boundary"] = boundary;
    Ok(style)
}

pub fn normalize_json_numbers(value: &mut Value) {
    match value {
        Value::Number(n) if n.is_f64() => {
            let v = n.as_f64().expect("float");
            if v.fract() == 0. && v.abs() <= 9_007_199_254_740_991. {
                *value = json!(v as i64);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(normalize_json_numbers),
        Value::Object(values) => values.values_mut().for_each(normalize_json_numbers),
        _ => {}
    }
}

/// Every property name a style document reads, anywhere in it: `["get", f]`
/// and `["has", f]`, inside a `coalesce`, a `match`, a `case`, a label
/// `concat`, an `icon-image`, a second categorical dimension's halo or a
/// filter. The walk is generic on purpose — parsing only the shapes today's
/// emitters produce would silently drop a field and paint the layer wrong.
/// Fields read by a resolved style; shared by screen and print projections.
pub fn referenced_fields(document: &Value) -> BTreeSet<String> {
    let mut fields = BTreeSet::new();
    collect_fields(document, &mut fields);
    fields
}

pub fn collect_fields(document: &Value, into: &mut BTreeSet<String>) {
    match document {
        Value::Array(items) => {
            if let Some(Value::String(operator)) = items.first() {
                if (operator == "get" || operator == "has") && items.len() >= 2 {
                    if let Some(Value::String(field)) = items.get(1) {
                        into.insert(field.clone());
                    }
                }
            }
            for item in items {
                collect_fields(item, into);
            }
        }
        Value::Object(map) => {
            for value in map.values() {
                collect_fields(value, into);
            }
        }
        _ => {}
    }
}
