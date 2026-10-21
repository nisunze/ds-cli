//! Guided primary channels and screen scale instructions. Hosts only bind inputs.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CategoryValue {
    pub value: String,
    pub output: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ColorStop {
    pub value: f64,
    pub color: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ZoomStop {
    pub zoom: f64,
    pub value: Value,
}

fn put(style: &mut Value, group: &str, property: &str, value: Value) -> Result<(), String> {
    let root = style.as_object_mut().ok_or("style must be an object")?;
    root.entry(group)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("{group} must be an object"))?
        .insert(property.into(), value);
    Ok(())
}
pub(crate) fn primary_property(kind: &str) -> Result<&'static str, String> {
    match kind {
        "circle" => Ok("circle-color"),
        "line" => Ok("line-color"),
        "fill" => Ok("fill-color"),
        "symbol" => Ok("icon-color"),
        "fill-extrusion" => Ok("fill-extrusion-color"),
        _ => Err("this layer has no primary colour channel".into()),
    }
}
fn color(raw: &str) -> Result<String, String> {
    crate::style_authoring::normalize_style_color(raw)
        .ok_or_else(|| "colour must be #RRGGBB or #RRGGBBAA".into())
}
fn declared_field<'a>(editor: &'a Value, field: &str) -> Result<&'a Value, String> {
    if field.is_empty()
        || field.len() > 120
        || field.trim() != field
        || !editor["available_fields"]
            .as_array()
            .is_some_and(|fields| fields.contains(&json!(field)))
    {
        return Err("field is absent from the published vocabulary".into());
    }
    Ok(&editor["field_domains"][field])
}
fn lookup(field: &str, domain: &Value) -> Value {
    let mut expressions = vec![json!(["get", field])];
    for alias in domain["aliases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if alias != field && !alias.is_empty() && !expressions.contains(&json!(["get", alias])) {
            expressions.push(json!(["get", alias]));
        }
    }
    if expressions.len() == 1 {
        expressions.remove(0)
    } else {
        let mut coalesce = vec![json!("coalesce")];
        coalesce.extend(expressions);
        json!(coalesce)
    }
}
fn value_type<'a>(domain: &'a Value, requested: Option<&'a str>) -> Result<&'a str, String> {
    let declared = domain["type"].as_str().map(|kind| match kind {
        "categorical_string" => "string",
        "categorical_number" | "categorical_int" | "numeric" => "number",
        other => other,
    });
    if requested.zip(declared).is_some_and(|(a, b)| a != b) {
        return Err("field type conflicts with published domain".into());
    }
    match declared.or(requested).unwrap_or("string") {
        "string" => Ok("string"),
        "number" => Ok("number"),
        _ => Err("categorical match needs string or integer labels".into()),
    }
}
fn label(raw: &str, kind: &str) -> Result<Value, String> {
    if raw.is_empty() || raw.len() > 200 {
        return Err("category labels must be nonempty and bounded".into());
    }
    if kind == "number" {
        raw.parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && n.fract() == 0.)
            .map(|n| json!(if n == 0. { 0. } else { n }))
            .ok_or_else(|| "numeric match labels must be finite integers".into())
    } else {
        Ok(json!(raw))
    }
}

pub(crate) fn categorical(
    style: &mut Value,
    snapshot: &Value,
    editor: &Value,
    channel: &str,
    field: &str,
    field_type: Option<&str>,
    values: &[CategoryValue],
    other: &str,
    merge: bool,
) -> Result<Value, String> {
    if values.is_empty() || values.len() > 50 {
        return Err("categorical styling needs 1..50 values".into());
    }
    let domain = declared_field(editor, field)?;
    let kind = value_type(domain, field_type)?;
    let style_kind = style["type"].as_str().ok_or("style type missing")?;
    let (group, property) = match channel {
        "color" => ("paint", primary_property(style_kind)?),
        "icon" if style_kind == "symbol" => ("layout", "icon-image"),
        "label" => ("layout", "text-field"),
        _ => return Err("channel must be color, icon on a symbol, or label".into()),
    };
    let output = |raw: &str| -> Result<Value, String> {
        match channel {
            "color" => color(raw).map(|v| json!(v)),
            "icon" => {
                if !crate::style_plan::icon_names(snapshot)
                    .iter()
                    .any(|icon| icon == raw)
                {
                    return Err("icon is not in the published catalog".into());
                }
                Ok(json!(raw))
            }
            _ => {
                if raw.len() > 500 || raw.chars().any(char::is_control) {
                    Err("label text exceeds the bounded text vocabulary".into())
                } else {
                    Ok(json!(raw))
                }
            }
        }
    };
    let fallback = output(other)?;
    let current = if channel == "label" {
        &style["label"][group][property]
    } else {
        &style[group][property]
    };
    let mut pairs: Vec<(Value, Value)> = Vec::new();
    if merge && !current.is_null() && current.is_array() {
        let old = current.as_array().expect("array");
        if old.first() != Some(&json!("match"))
            || old.get(1) != Some(&lookup(field, domain))
            || old.len() < 5
            || old.len() % 2 != 1
        {
            return Err("merge requires an existing match on the same field".into());
        }
        for index in (2..old.len() - 1).step_by(2) {
            for raw in old[index]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_else(|| std::slice::from_ref(&old[index]))
            {
                pairs.push((raw.clone(), old[index + 1].clone()));
            }
        }
    } else if merge && !current.is_null() {
        return Err("merge requires an existing categorical expression".into());
    }
    let mut seen = BTreeSet::new();
    for row in values {
        let key = label(&row.value, kind)?;
        if !seen.insert(key.to_string()) {
            return Err("duplicate typed category label".into());
        }
        let result = output(&row.output)?;
        if let Some(pair) = pairs.iter_mut().find(|(raw, _)| {
            raw == &key || raw.as_f64().zip(key.as_f64()).is_some_and(|(a, b)| a == b)
        }) {
            pair.1 = result;
        } else {
            pairs.push((key, result));
        }
    }
    if pairs.len() > 50 {
        return Err("merged categories exceed 50 values".into());
    }
    let mut expression = vec![json!("match"), lookup(field, domain)];
    for (key, value) in pairs {
        expression.push(key);
        expression.push(value);
    }
    expression.push(fallback);
    let expression = json!(expression);
    if channel == "label" {
        if !style["label"].is_object() {
            style["label"] = editor["label_model"]
                .as_object()
                .map(|_| editor["label_model"].clone())
                .ok_or("backend label model is unavailable for this ref")?;
        }
        put(&mut style["label"], group, property, expression.clone())?;
    } else {
        put(style, group, property, expression.clone())?;
    }
    crate::style_validation::validate(style)?;
    Ok(
        json!({"channel":channel,"field":field,"fieldType":kind,"expression":expression,"merge":merge}),
    )
}

pub(crate) fn color_range(
    style: &mut Value,
    editor: &Value,
    field: &str,
    stops: &[ColorStop],
) -> Result<Value, String> {
    if style["type"] == "symbol" {
        return Err("interpolated colour cannot tint raster catalog symbols continuously; use categorical colour with finite icon recipes".into());
    }
    let domain = declared_field(editor, field)?;
    if !matches!(
        domain["type"].as_str(),
        Some("numeric" | "categorical_number" | "categorical_int" | "number")
    ) {
        return Err("interpolated colour requires a declared numeric field".into());
    }
    if !(2..=50).contains(&stops.len()) {
        return Err("interpolated colour needs 2..50 stops".into());
    }
    let mut expression = vec![
        json!("interpolate"),
        json!(["linear"]),
        json!(["to-number", lookup(field, domain), stops[0].value]),
    ];
    let mut previous = f64::NEG_INFINITY;
    for stop in stops {
        if !stop.value.is_finite() || stop.value <= previous {
            return Err("colour stop inputs must be finite and strictly increasing".into());
        }
        previous = stop.value;
        expression.push(json!(stop.value));
        expression.push(json!(color(&stop.color)?));
    }
    let property = primary_property(style["type"].as_str().ok_or("style type missing")?)?;
    let expression = json!(expression);
    put(style, "paint", property, expression.clone())?;
    Ok(json!({"field":field,"expression":expression}))
}

pub(crate) fn halo(
    style: &mut Value,
    snapshot: &Value,
    halo_color: Option<&str>,
    width: Option<f64>,
) -> Result<(), String> {
    let kind = style["type"]
        .as_str()
        .ok_or("style type missing")?
        .to_owned();
    let (color_prop, width_prop) = match kind.as_str() {
        "circle" => ("circle-stroke-color", "circle-stroke-width"),
        "symbol" => ("icon-halo-color", "icon-halo-width"),
        _ => return Err("flat halos require a circle or symbol; lines use casing".into()),
    };
    if let Some(raw) = halo_color {
        put(style, "paint", color_prop, json!(color(raw)?))?;
    }
    if let Some(width) = width {
        // Raster symbols expose halo through the shared dimension vocabulary,
        // rather than through MapLibre's SDF-only property controls.
        let width = match numeric(snapshot, &kind, "paint", width_prop, width) {
            Ok(width) => width,
            Err(error) if error.starts_with("numeric property schema is missing") => {
                let channel = crate::style_authoring::style_channels(&kind)
                    .into_iter()
                    .find(|channel| channel.key == "halo")
                    .ok_or(error)?;
                if !width.is_finite() || width < channel.min || width > channel.max {
                    return Err(format!(
                        "halo width is outside {}..{}",
                        channel.min, channel.max
                    ));
                }
                width
            }
            Err(error) => return Err(error),
        };
        put(style, "paint", width_prop, json!(width))?;
    }
    Ok(())
}
fn numeric(
    snapshot: &Value,
    kind: &str,
    group: &str,
    property: &str,
    value: f64,
) -> Result<f64, String> {
    let schema = snapshot["style_properties"][kind]
        .as_array()
        .and_then(|rows| {
            rows.iter().find(|row| {
                row["key"] == property
                    && row["type"] == "number"
                    && (row["group"].is_null() || row["group"] == group)
            })
        })
        .ok_or_else(|| format!("numeric property schema is missing for {property}"))?;
    let min = schema["min"].as_f64().ok_or("minimum missing")?;
    let max = schema["max"].as_f64().ok_or("maximum missing")?;
    if !value.is_finite() || value < min || value > max {
        return Err(format!("{property} is outside {min}..{max}"));
    }
    Ok(value)
}
pub(crate) fn zoom(
    style: &mut Value,
    snapshot: &Value,
    channel: &str,
    base: &Value,
    stops: &[ZoomStop],
    interpolation: &str,
) -> Result<Value, String> {
    if stops.is_empty() || stops.len() > 50 {
        return Err("zoom styling needs 1..50 stops".into());
    }
    let kind = style["type"]
        .as_str()
        .ok_or("style type missing")?
        .to_owned();
    let (group, property) = match (channel, kind.as_str()) {
        ("size", "circle") => ("paint", "circle-radius"),
        ("size", "line") => ("paint", "line-width"),
        ("size", "symbol") => ("layout", "icon-size"),
        ("opacity", "circle") => ("paint", "circle-opacity"),
        ("opacity", "line") => ("paint", "line-opacity"),
        ("opacity", "symbol") => ("paint", "icon-opacity"),
        ("opacity", "fill") => ("paint", "fill-opacity"),
        ("opacity", "fill-extrusion") => ("paint", "fill-extrusion-opacity"),
        ("icon_overlap", "symbol") => ("layout", "icon-allow-overlap"),
        _ => return Err("unsupported zoom channel for this layer".into()),
    };
    if !matches!(interpolation, "step" | "linear")
        || (channel == "icon_overlap" && interpolation != "step")
    {
        return Err("overlap uses step; numeric channels use step or linear".into());
    }
    let check = |v: &Value| -> Result<Value, String> {
        if channel == "icon_overlap" {
            return v
                .as_bool()
                .map(|v| json!(v))
                .ok_or_else(|| "zoom overlap values must be boolean".into());
        }
        let n = v.as_f64().ok_or("zoom channel values must be numeric")?;
        if channel == "opacity" {
            if !(0.0..=1.0).contains(&n) {
                return Err("zoom opacity is outside 0..1".into());
            }
            Ok(json!(n))
        } else {
            numeric(snapshot, &kind, group, property, n).map(|n| json!(n))
        }
    };
    let base = check(base)?;
    let mut expression = if interpolation == "step" {
        vec![json!("step"), json!(["zoom"]), base]
    } else {
        vec![
            json!("interpolate"),
            json!(["linear"]),
            json!(["zoom"]),
            json!(0),
            base,
        ]
    };
    let mut previous = if interpolation == "step" { -1. } else { 0. };
    for stop in stops {
        if !stop.zoom.is_finite() || !(0.0..=24.0).contains(&stop.zoom) || stop.zoom <= previous {
            return Err(
                "zoom stops must be strictly increasing within 0..24 (linear starts at 0)".into(),
            );
        }
        previous = stop.zoom;
        expression.push(json!(stop.zoom));
        expression.push(check(&stop.value)?);
    }
    let expression = json!(expression);
    // Changing line scale must retain the authored casing's thickness.
    let casing = if channel == "size" && kind == "line" {
        crate::style_plan::describe_casing(style)
    } else {
        Value::Null
    };
    put(style, group, property, expression.clone())?;
    if channel == "icon_overlap" {
        put(style, "layout", "icon-ignore-placement", expression.clone())?;
    }
    if let (Some(color), Some(width)) = (casing["color"].as_str(), casing["width"].as_f64()) {
        *style = crate::style_authoring::apply_casing(style.clone(), color, width)?;
    }
    Ok(json!({"channel":channel,"expression":expression,"interpolation":interpolation}))
}

/// Published capability vocabulary used by the guided UI and headless readback.
pub(crate) fn vocabulary(kind: &str, document: &Value, snapshot: &Value) -> Value {
    let presets = snapshot["style_presets"]["authoring"]
        .as_object()
        .map(|recipes| recipes.keys().take(200).cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    json!({"presets":presets,"categorical":{"channels":if kind == "symbol" { vec!["color","icon","label"] } else { vec!["color","label"] },"maxValues":50,"fallbackRequired":true,"merge":true,"field_types":["string","number"],"current":current_categories(kind,document)},
        "colorRange":{"available":kind != "symbol","maxStops":50,"interpolation":"linear","inputType":"number"},
        "zoom":{"channels":if kind == "symbol" { vec!["size","opacity","icon_overlap"] } else if kind == "circle" || kind == "line" {vec!["size","opacity"]} else {vec!["opacity"]},"interpolations":["step","linear"],"min":0,"max":24,"maxStops":50},
        "flatHalo":kind == "symbol" || kind == "circle"})
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetInk {
    color: String,
    width: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetRecipe {
    #[serde(default)]
    palette: Vec<String>,
    missing_color: String,
    flat_color: String,
    #[serde(default)]
    flatten_categories: bool,
    #[serde(default)]
    halo: Option<PresetInk>,
    #[serde(default)]
    casing: Option<PresetInk>,
    #[serde(default = "preserve_halo")]
    preserve_secondary_halo: bool,
}
fn preserve_halo() -> bool {
    true
}

pub(crate) fn preset(
    style: &mut Value,
    snapshot: &Value,
    editor: &Value,
    reference: &str,
    name: &str,
) -> Result<(), String> {
    if editor["style_target"] == "print" {
        return Err(
            "screen presets address screen refs; print styles have independent standards".into(),
        );
    }
    let kind = style["type"]
        .as_str()
        .ok_or("style type missing")?
        .to_owned();
    let property = primary_property(&kind)?;
    let raw = &snapshot["style_presets"]["authoring"][name];
    if !raw.is_object() {
        return Err(format!("governed screen preset {name} is unavailable; initialize the declared seed manifest through the Styles API"));
    }
    let recipe: PresetRecipe = serde_json::from_value(raw.clone())
        .map_err(|error| format!("invalid governed preset {name}: {error}"))?;
    let flat = color(&recipe.flat_color)?;
    let missing = color(&recipe.missing_color)?;
    let palette = recipe
        .palette
        .iter()
        .map(|ink| color(ink))
        .collect::<Result<Vec<_>, _>>()?;
    if palette.len() > 200 {
        return Err("governed preset palette exceeds 200 colours".into());
    }
    if recipe.flatten_categories {
        put(style, "paint", property, json!(flat))?;
    } else {
        let current = style["paint"][property].clone();
        if current.as_array().is_some_and(|expr| {
            expr.first() == Some(&json!("match")) && expr.len() >= 5 && expr.len() % 2 == 1
        }) {
            let expr = current.as_array().expect("match");
            let mut next = vec![json!("match"), expr[1].clone()];
            let mut band = 0;
            for index in (2..expr.len() - 1).step_by(2) {
                for label in expr[index]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_else(|| std::slice::from_ref(&expr[index]))
                {
                    next.push(label.clone());
                    let ink = palette.get(band).ok_or_else(||format!("governed preset {name} needs a colour for every category; its declared palette is too short"))?;
                    next.push(json!(ink));
                    band += 1;
                }
            }
            next.push(json!(missing));
            put(style, "paint", property, json!(next))?;
        } else if !current.is_array() {
            put(style, "paint", property, json!(flat))?;
        }
    }
    if kind == "line" {
        if let Some(casing) = &recipe.casing {
            *style = crate::style_authoring::apply_casing(
                style.clone(),
                &color(&casing.color)?,
                casing.width,
            )?;
        }
    }
    if (kind == "symbol" || kind == "circle")
        && (!recipe.preserve_secondary_halo
            || !crate::style_plan::second_style_channel(style)
                .is_some_and(|channel| channel.key == "halo"))
    {
        if let Some(ink) = &recipe.halo {
            halo(style, snapshot, Some(&color(&ink.color)?), Some(ink.width))?;
        }
    }
    let _ = reference;
    Ok(())
}

fn current_categories(kind: &str, document: &Value) -> Value {
    let mut result = json!({});
    for (channel, expression) in [
        (
            "color",
            primary_property(kind).ok().map(|p| &document["paint"][p]),
        ),
        ("icon", Some(&document["layout"]["icon-image"])),
        ("label", Some(&document["label"]["layout"]["text-field"])),
    ] {
        let Some(expression) = expression.and_then(Value::as_array) else {
            continue;
        };
        if expression.len() < 5 || expression.len() % 2 != 1 || expression[0] != "match" {
            continue;
        }
        let Some(field) = expression[1][1].as_str() else {
            continue;
        };
        if expression[1][0] != "get" {
            continue;
        }
        let mut values = Vec::new();
        for index in (2..expression.len() - 1).step_by(2) {
            for raw in expression[index]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_else(|| std::slice::from_ref(&expression[index]))
            {
                values.push(json!({"value":raw.as_str().map(str::to_owned).unwrap_or_else(||raw.to_string()),"output":expression[index+1]}));
            }
        }
        result[channel] = json!({"field":field,"values":values,"other":expression.last()});
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style_plan::{plan_style_edit, StyleInstruction};
    fn snapshot(kind: &str) -> Value {
        json!({"project_id":"gisagara","styles":{"master/test":{"type":kind,"paint":{},"layout":{},"metadata":{"authored":true},"label":{"type":"symbol","layout":{"text-size":12},"paint":{"text-halo-width":1.8}}}},
        "style_editors":[{"style_ref":"master/test","style_target":"design","layer_name":"test","available_fields":["kind","tr_size"],"field_domains":{"kind":{"type":"categorical_string"},"tr_size":{"type":"categorical_number"}},"label_model":{"type":"symbol","layout":{},"paint":{}}}],
        "symbol_catalog":{"built_ins":["home","square"]},"style_properties":{kind:[
            {"key":"icon-size","group":"layout","type":"number","min":0.1,"max":5},
            {"key":"circle-radius","group":"paint","type":"number","min":0,"max":30},
            {"key":"line-width","group":"paint","type":"number","min":0,"max":30},
            {"key":"icon-halo-width","group":"paint","type":"number","min":0,"max":10},
            {"key":"circle-stroke-width","group":"paint","type":"number","min":0,"max":10}
        ]}})
    }
    fn plan(snapshot: &Value, request: Value) -> Result<Value, String> {
        let instruction: StyleInstruction =
            serde_json::from_value(request).map_err(|e| e.to_string())?;
        plan_style_edit(snapshot, "master/test", &instruction).map(|p| p.document)
    }
    #[test]
    fn categorical_primary_colour_covers_each_geometry_and_merges_without_flattening() {
        for kind in ["symbol", "circle", "line", "fill", "fill-extrusion"] {
            let mut snapshot = snapshot(kind);
            let document=plan(&snapshot,json!({"kind":"categorical","channel":"color","field":"kind","values":[{"value":"wood","output":"#f59e0b"},{"value":"steel","output":"#E11D48"}],"other":"#64748B"})).unwrap();
            let property = primary_property(kind).unwrap();
            assert_eq!(
                document["paint"][property],
                json!([
                    "match",
                    ["get", "kind"],
                    "wood",
                    "#F59E0B",
                    "steel",
                    "#E11D48",
                    "#64748B"
                ])
            );
            snapshot["styles"]["master/test"] = document;
            let updated=plan(&snapshot,json!({"kind":"categorical","channel":"color","field":"kind","values":[{"value":"wood","output":"#FFFFFF"}],"other":"#111827","merge":true})).unwrap();
            assert_eq!(updated["paint"][property][5], "#E11D48");
            assert_eq!(updated["metadata"]["authored"], true);
        }
    }
    #[test]
    fn categorical_icons_labels_numeric_categories_and_refusals_are_typed() {
        let snapshot = snapshot("symbol");
        let icon=plan(&snapshot,json!({"kind":"categorical","channel":"icon","field":"kind","values":[{"value":"house","output":"home"}],"other":"square"})).unwrap();
        assert_eq!(
            icon["layout"]["icon-image"],
            json!(["match", ["get", "kind"], "house", "home", "square"])
        );
        let label=plan(&snapshot,json!({"kind":"categorical","channel":"label","field":"tr_size","values":[{"value":"25","output":"25 kVA"}],"other":"Unknown"})).unwrap();
        assert!(label["label"]["layout"]["text-field"][2].is_number());
        assert_eq!(label["label"]["layout"]["text-size"], 12);
        for request in [
            json!({"kind":"categorical","channel":"icon","field":"kind","values":[{"value":"x","output":"unpublished"}],"other":"home"}),
            json!({"kind":"categorical","channel":"color","field":"unknown","values":[{"value":"x","output":"#FFFFFF"}],"other":"#000000"}),
            json!({"kind":"categorical","channel":"label","field":"tr_size","values":[{"value":"25","output":"a"},{"value":"25.0","output":"b"}],"other":""}),
        ] {
            assert!(plan(&snapshot, request).is_err());
        }
    }
    #[test]
    fn survey_multiscale_size_opacity_collision_and_white_halo_preserve_print_and_labels() {
        let mut snapshot = snapshot("symbol");
        let original = snapshot["styles"]["master/test"].clone();
        for request in [
            json!({"kind":"appearance","color":"#F59E0B","halo_color":"#FFFFFF","halo_width":0.9}),
            json!({"kind":"zoom","channel":"size","base":0.45,"stops":[{"zoom":14,"value":0.75},{"zoom":17,"value":1.15}]}),
            json!({"kind":"zoom","channel":"opacity","base":0.5,"stops":[{"zoom":14,"value":0.92}]}),
            json!({"kind":"zoom","channel":"icon_overlap","base":false,"stops":[{"zoom":16,"value":true}]}),
        ] {
            snapshot["styles"]["master/test"] = plan(&snapshot, request).unwrap();
        }
        let document = &snapshot["styles"]["master/test"];
        assert_eq!(document["paint"]["icon-halo-color"], "#FFFFFF");
        assert_eq!(
            document["layout"]["icon-size"],
            json!(["step", ["zoom"], 0.45, 14, 0.75, 17, 1.15])
        );
        assert_eq!(
            document["layout"]["icon-allow-overlap"],
            json!(["step", ["zoom"], false, 16, true])
        );
        assert_eq!(
            document["layout"]["icon-allow-overlap"],
            document["layout"]["icon-ignore-placement"]
        );
        assert_eq!(document["label"], original["label"]);
        for stops in [
            json!([{ "zoom":14,"value":1.0},{"zoom":14,"value":2.0}]),
            json!([{"zoom":25,"value":1.0}]),
            json!([{"zoom":14,"value":-1.0}]),
        ] {
            assert!(plan(
                &snapshot,
                json!({"kind":"zoom","channel":"size","base":0.5,"stops":stops})
            )
            .is_err());
        }
        assert!(plan(&snapshot,json!({"kind":"zoom","channel":"icon_overlap","base":false,"interpolation":"linear","stops":[{"zoom":14,"value":true}]})).is_err());
    }
    #[test]
    fn interpolated_colour_and_zoom_line_casing_are_retained_in_save_payload() {
        let mut snapshot = snapshot("line");
        let document=plan(&snapshot,json!({"kind":"color_range","field":"tr_size","stops":[{"value":25,"color":"#00FF00"},{"value":100,"color":"#FF0000"}]})).unwrap();
        assert_eq!(document["paint"]["line-color"][0], "interpolate");
        assert!(plan(&snapshot,json!({"kind":"color_range","field":"kind","stops":[{"value":0,"color":"#FFFFFF"},{"value":1,"color":"#000000"}]})).is_err());
        snapshot["styles"]["master/test"] = crate::style_authoring::apply_casing(
            json!({"type":"line","paint":{"line-width":2}}),
            "#FFFFFF",
            0.75,
        )
        .unwrap();
        let document = plan(
            &snapshot,
            json!({"kind":"zoom","channel":"size","base":1.4,"stops":[{"zoom":16,"value":4.2}]}),
        )
        .unwrap();
        assert_eq!(
            document["boundary"]["paint"]["line-width"],
            json!(["step", ["zoom"], 2.9, 16, 5.7])
        );
    }
    #[test]
    fn guarded_replay_uses_storage_digest_and_satellite_retains_secondary_authorship() {
        let mut snapshot = snapshot("symbol");
        snapshot["style_presets"]["authoring"]["satellite"] = json!({"palette":["#FACC15","#22D3EE"],"missing_color":"#94A3B8","flat_color":"#FACC15","flatten_categories":false,"halo":{"color":"#FFFFFF","width":0.9},"casing":{"color":"#0F172A","width":0.75}});
        snapshot["style_editors"][0]["content_sha256"] = json!("a".repeat(64));
        let instruction:StyleInstruction=serde_json::from_value(json!({"kind":"guarded","expected_digest":"a".repeat(64),"instruction":{"kind":"categorical","channel":"color","field":"kind","values":[{"value":"wood","output":"#FFFFFF"}],"other":"#64748B"}})).unwrap();
        let planned = plan_style_edit(&snapshot, "master/test", &instruction).unwrap();
        assert_eq!(planned.payload["expected_style_sha256"], "a".repeat(64));
        snapshot["style_editors"][0]["content_sha256"] = json!("b".repeat(64));
        assert!(plan_style_edit(&snapshot, "master/test", &instruction)
            .unwrap_err()
            .starts_with("style_digest_conflict:"));
        snapshot["styles"]["master/test"]["paint"] = json!({"icon-color":["match",["get","kind"],["wood","steel"],"#111111","#333333"],"icon-halo-width":["match",["get","kind"],"wood",1,"steel",2,0],"icon-halo-color":["match",["get","kind"],"wood","#FFFFFF","steel","#E11D48","#64748B"],"icon-opacity":["match",["get","kind"],"wood",0.5,1]});
        snapshot["styles"]["master/test"]["layout"]["icon-size"] = json!(0.85);
        let original = snapshot["styles"]["master/test"].clone();
        let styled = plan(&snapshot, json!({"kind":"preset","name":"satellite"})).unwrap();
        assert_ne!(
            styled["paint"]["icon-color"][3],
            styled["paint"]["icon-color"][5]
        );
        for property in ["icon-halo-width", "icon-halo-color", "icon-opacity"] {
            assert_eq!(styled["paint"][property], original["paint"][property]);
        }
        assert_eq!(
            styled["layout"]["icon-size"],
            original["layout"]["icon-size"]
        );
        assert!(plan(&snapshot,json!({"kind":"color_range","field":"tr_size","stops":[{"value":25,"color":"#FFFFFF"},{"value":100,"color":"#000000"}]})).unwrap_err().contains("raster"));
        // The real raster-symbol schema has no SDF halo row.
        snapshot["style_properties"]["symbol"]
            .as_array_mut()
            .unwrap()
            .retain(|row| row["key"] != "icon-halo-width");
        assert!(plan(
            &snapshot,
            json!({"kind":"appearance","halo_color":"#FFFFFF","halo_width":0.9})
        )
        .is_ok());
    }
}
