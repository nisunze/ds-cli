//! Guided style document transformations, shared by browser WASM and native CLI.
//! No rendering, category completion, transport, or independent save sanitizer.
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub const MAX_STYLE_REQUEST_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub key: String,
    pub label: String,
    pub hint: String,
    pub number_prop: Option<String>,
    pub number_group: String,
    pub color_prop: Option<String>,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub unit: String,
    pub on: f64,
    pub off: f64,
    pub fallback: f64,
    pub default_color: String,
}
pub fn style_channels(kind: &str) -> Vec<Channel> {
    let all: std::collections::BTreeMap<String, Vec<Channel>> =
        serde_json::from_str(include_str!("../style-channels.json"))
            .expect("compiled channel vocabulary");
    all.get(kind).cloned().unwrap_or_default()
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DimensionState {
    pub channel: String,
    pub field: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub values: Vec<String>,
    #[serde(default)]
    pub raw_values: Map<String, Value>,
    #[serde(default)]
    pub numbers: Map<String, Value>,
    #[serde(default)]
    pub colors: Map<String, Value>,
    pub fallback_number: f64,
    pub fallback_color: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum StyleTransform {
    PrintScaling {
        style: Value,
        #[serde(default)]
        scaling: crate::print_scaling::PrintScaling,
    },
    Appearance {
        style: Value,
        change: Value,
    },
    Dimension {
        style: Value,
        channel: String,
        state: DimensionState,
    },
    ClearDimension {
        style: Value,
        channel: String,
    },
    LineType {
        style: Value,
        line_type: String,
        table: Map<String, Value>,
        #[serde(default)]
        scale: Map<String, Value>,
    },
    Casing {
        style: Value,
        color: String,
        width: f64,
    },
    ClearCasing {
        style: Value,
    },
    FillPattern {
        style: Value,
        recipe: Option<PatternRecipe>,
    },
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PatternRecipe {
    pub pattern: String,
    pub color: String,
    #[serde(default)]
    pub background: String,
    pub spacing: u32,
    pub stroke: f64,
}

pub fn normalize_style_color(value: &str) -> Option<String> {
    let value = value.trim();
    ((value.len() == 7 || value.len() == 9)
        && value.starts_with('#')
        && value[1..].bytes().all(|c| c.is_ascii_hexdigit()))
    .then(|| value.to_ascii_uppercase())
}
fn object(value: &Value) -> Result<&Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| "style must be an object".into())
}
fn bucket<'a>(style: &'a mut Value, key: &str) -> Result<&'a mut Map<String, Value>, String> {
    let root = style.as_object_mut().ok_or("style must be an object")?;
    let value = root.entry(key).or_insert_with(|| json!({}));
    value
        .as_object_mut()
        .ok_or_else(|| format!("{key} must be an object"))
}
fn kind(style: &Value) -> Result<&str, String> {
    object(style)?;
    style["type"]
        .as_str()
        .ok_or_else(|| "style type is required".into())
}
fn channel_for(style: &Value, key: &str) -> Result<Channel, String> {
    style_channels(kind(style)?)
        .into_iter()
        .find(|c| c.key == key)
        .ok_or_else(|| "unsupported style channel".into())
}
fn field_lookup(field: &str, aliases: &[String]) -> Value {
    let canonical = field.trim();
    let mut seen = HashSet::from([canonical]);
    let mut names = vec![json!(["get", canonical])];
    for alias in aliases {
        let alias = alias.trim();
        if !alias.is_empty() && seen.insert(alias) {
            names.push(json!(["get", alias]));
        }
    }
    if names.len() == 1 {
        names.remove(0)
    } else {
        let mut out = vec![json!("coalesce")];
        out.extend(names);
        Value::Array(out)
    }
}
fn label_kind(v: &Value) -> &'static str {
    if v.is_number() {
        "number"
    } else if v.is_boolean() {
        "boolean"
    } else {
        "string"
    }
}
fn categorical(
    state: &DimensionState,
    map: &Map<String, Value>,
    fallback: Value,
    color: bool,
) -> Result<Value, String> {
    let labels: Vec<Value> = state
        .values
        .iter()
        .filter(|v| map.contains_key(*v))
        .map(|v| state.raw_values.get(v).cloned().unwrap_or_else(|| json!(v)))
        .collect();
    let strings = labels.iter().filter(|v| v.is_string()).count();
    let numbers = labels.iter().filter(|v| v.is_number()).count();
    let booleans = labels.iter().filter(|v| v.is_boolean()).count();
    let dominant = if numbers > strings && numbers >= booleans {
        "number"
    } else if booleans > strings && booleans > numbers {
        "boolean"
    } else {
        "string"
    };
    let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
    let mut seen = HashSet::new();
    for key in &state.values {
        if !seen.insert(key) {
            continue;
        }
        let Some(output) = map.get(key) else { continue };
        let label = state
            .raw_values
            .get(key)
            .cloned()
            .unwrap_or_else(|| json!(key));
        if !(label.is_string() || label.is_number() || label.is_boolean()) {
            return Err("invalid categorical label".into());
        }
        if label_kind(&label) != dominant {
            continue;
        }
        let output = if color {
            let c = output.as_str().ok_or("categorical color must be text")?;
            json!(normalize_style_color(c).unwrap_or_else(|| c.into()))
        } else {
            if !output.is_number() {
                continue;
            }
            output.clone()
        };
        if let Some((_, labels)) = groups.iter_mut().find(|(v, _)| v == &output) {
            labels.push(label);
        } else {
            groups.push((output, vec![label]));
        }
    }
    if groups.is_empty() {
        return Ok(fallback);
    }
    let mut expression = vec![json!("match"), field_lookup(&state.field, &state.aliases)];
    for (output, mut labels) in groups {
        expression.push(if labels.len() == 1 {
            labels.remove(0)
        } else {
            json!(labels)
        });
        expression.push(output);
    }
    expression.push(fallback);
    Ok(Value::Array(expression))
}
pub fn apply_dimension(
    mut style: Value,
    channel: &str,
    state: &DimensionState,
) -> Result<Value, String> {
    let def = channel_for(&style, channel)?;
    if state.channel != channel
        || state.field.trim().is_empty()
        || state.field.len() > 500
        || state.values.len() > 10_000
        || state.aliases.len() > 100
    {
        return Err("invalid dimension state".into());
    }
    if let Some(prop) = def.number_prop {
        let expr = categorical(state, &state.numbers, json!(state.fallback_number), false)?;
        bucket(&mut style, &def.number_group)?.insert(prop, expr);
    }
    if let Some(prop) = def.color_prop {
        let fallback = if state.fallback_color.is_empty() {
            if def.default_color.is_empty() {
                "#ffffff"
            } else {
                &def.default_color
            }
        } else {
            &state.fallback_color
        };
        let expr = categorical(
            state,
            &state.colors,
            json!(normalize_style_color(fallback).unwrap_or_else(|| fallback.into())),
            true,
        )?;
        bucket(&mut style, "paint")?.insert(prop, expr);
    }
    Ok(style)
}
pub fn clear_dimension(mut style: Value, channel: &str) -> Result<Value, String> {
    let def = channel_for(&style, channel)?;
    if let Some(prop) = def.number_prop {
        bucket(&mut style, &def.number_group)?.remove(&prop);
    }
    if let Some(prop) = def.color_prop {
        bucket(&mut style, "paint")?.remove(&prop);
    }
    Ok(style)
}
pub fn apply_appearance(mut style: Value, change: &Value) -> Result<Value, String> {
    let kind = kind(&style)?.to_owned();
    let change = object(change)?;
    if change
        .keys()
        .any(|k| !matches!(k.as_str(), "color" | "icon" | "size"))
    {
        return Err("unknown appearance property".into());
    }
    let (color, size, icon) = match kind.as_str() {
        "circle" => (Some("circle-color"), Some("circle-radius"), None),
        "line" => (Some("line-color"), Some("line-width"), None),
        "fill" => (Some("fill-color"), None, None),
        "symbol" => (Some("icon-color"), Some("icon-size"), Some("icon-image")),
        "fill-extrusion" => (Some("fill-extrusion-color"), None, None),
        _ => return Ok(style),
    };
    for (name, property, group) in [
        ("color", color, "paint"),
        (
            "size",
            size,
            if kind == "symbol" { "layout" } else { "paint" },
        ),
        ("icon", icon, "layout"),
    ] {
        if let (Some(value), Some(prop)) = (change.get(name), property) {
            bucket(&mut style, group)?.insert(prop.into(), value.clone());
        }
    }
    Ok(style)
}
pub fn apply_line_type(
    mut style: Value,
    line_type: &str,
    table: &Map<String, Value>,
    scale: &Map<String, Value>,
) -> Result<Value, String> {
    kind(&style)?;
    if line_type == "directional" {
        bucket(&mut style, "paint")?.remove("line-dasharray");
        bucket(&mut style, "metadata")?.insert("line_marker".into(), json!("arrow"));
        for (input, key, fallback) in [
            ("size", "line_marker_size_px", 14.),
            ("spacing", "line_marker_spacing_px", 140.),
        ] {
            if let Some(value) = scale.get(input) {
                let metadata = bucket(&mut style, "metadata")?;
                if value.as_f64().is_some_and(|v| v > 0. && v != fallback) {
                    metadata.insert(key.into(), value.clone());
                } else {
                    metadata.remove(key);
                }
            }
        }
    } else {
        let paint = bucket(&mut style, "paint")?;
        if let Some(pattern) = table.get(line_type).filter(|v| !v.is_null()) {
            paint.insert("line-dasharray".into(), pattern.clone());
        } else {
            paint.remove("line-dasharray");
        }
        bucket(&mut style, "metadata")?.remove("line_marker");
    }
    Ok(style)
}
fn output_indices(value: &Value) -> Result<Vec<usize>, String> {
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
    boundary["type"] = json!("line");
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
pub fn apply_pattern(mut style: Value, recipe: Option<&PatternRecipe>) -> Result<Value, String> {
    kind(&style)?;
    let metadata = bucket(&mut style, "metadata")?;
    for key in [
        "fill_pattern",
        "fill_pattern_color",
        "fill_pattern_background",
        "fill_pattern_spacing_px",
        "fill_pattern_stroke_px",
    ] {
        metadata.remove(key);
    }
    if let Some(recipe) = recipe {
        if !["diagonal-forward", "diagonal-back", "crosshatch", "dots"]
            .contains(&recipe.pattern.as_str())
            || ![4, 8, 16, 32].contains(&recipe.spacing)
            || !(1.0..=6.0).contains(&recipe.stroke)
        {
            return Err("invalid hatch recipe".into());
        }
        metadata.insert("fill_pattern".into(), json!(recipe.pattern));
        metadata.insert("fill_pattern_color".into(), json!(recipe.color));
        if !recipe.background.is_empty() {
            metadata.insert("fill_pattern_background".into(), json!(recipe.background));
        }
        metadata.insert("fill_pattern_spacing_px".into(), json!(recipe.spacing));
        metadata.insert("fill_pattern_stroke_px".into(), json!(recipe.stroke));
    }
    if metadata.is_empty() {
        style.as_object_mut().expect("object").remove("metadata");
    }
    Ok(style)
}
pub fn transform_style(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_STYLE_REQUEST_BYTES {
        return Err("style request exceeds 2 MiB".into());
    }
    let request: StyleTransform =
        serde_json::from_slice(input).map_err(|e| format!("invalid style request: {e}"))?;
    let mut style = match request {
        StyleTransform::PrintScaling { style, scaling } => {
            crate::print_scaling::apply_print_scaling(style, scaling)?
        }
        StyleTransform::Appearance { style, change } => apply_appearance(style, &change)?,
        StyleTransform::Dimension {
            style,
            channel,
            state,
        } => apply_dimension(style, &channel, &state)?,
        StyleTransform::ClearDimension { style, channel } => clear_dimension(style, &channel)?,
        StyleTransform::LineType {
            style,
            line_type,
            table,
            scale,
        } => apply_line_type(style, &line_type, &table, &scale)?,
        StyleTransform::Casing {
            style,
            color,
            width,
        } => apply_casing(style, &color, width)?,
        StyleTransform::ClearCasing { mut style } => {
            kind(&style)?;
            style.as_object_mut().expect("checked").remove("boundary");
            style
        }
        StyleTransform::FillPattern { style, recipe } => apply_pattern(style, recipe.as_ref())?,
    };
    normalize_json_numbers(&mut style);
    serde_json::to_string(&style).map_err(|_| "cannot serialize style".into())
}

pub(crate) fn normalize_json_numbers(value: &mut Value) {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn native_and_wasm_match_captured_style_center_documents() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../tests/fixtures/style-authoring.json")).unwrap();
        for (index, case) in cases.iter().enumerate() {
            let result = transform_style(&serde_json::to_vec(&case["request"]).unwrap())
                .unwrap_or_else(|e| panic!("fixture {index}: {e}"));
            assert_eq!(
                serde_json::from_str::<Value>(&result).unwrap(),
                case["expected"],
                "fixture {index}"
            );
        }
    }
    #[test]
    fn casing_refuses_unsupported_arithmetic_instead_of_flattening() {
        assert!(widen_line_width(&json!(["*", ["get", "width"], 2]), 3.).is_err());
        assert!(widen_line_width(&json!(["match", ["get", "kind"], "a", "red", 2]), 3.).is_err());
        assert!(transform_style(
            br#"{"operation":"clear_casing","style":{"type":"line"},"extra":true}"#
        )
        .is_err());
    }
}

// JSON.stringify emits integral JS numbers without a decimal suffix. Keep
// native/WASM fixture documents identical while retaining existing integer data.
