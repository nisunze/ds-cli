//! Guided authoring decisions over a bounded backend style snapshot.
use crate::style_authoring::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StyleInstruction {
    /// Persist one backend-declared print-context source style exactly once.
    /// The backend snapshot owns its palette, schema and save routing.
    Seed,
    /// Clone the authenticated screen style into its predictable `_print`
    /// variant. The clone is create-only; later guided edits address the new
    /// ref through the ordinary Style Center path.
    PrintVariant,
    Appearance {
        color: Option<String>,
        icon: Option<String>,
        size: Option<f64>,
    },
    /// Point a style's governed label at one exact declared data field. The
    /// backend-provided label model supplies defaults only when the style has
    /// no label yet; existing label authorship is otherwise preserved.
    Label {
        field: String,
        #[serde(default)]
        options: crate::style_label::LabelOptions,
    },
    Dimension {
        field: String,
        channel: String,
        values: Vec<DimensionValue>,
        other: Option<f64>,
        color: Option<String>,
        field_type: Option<String>,
        #[serde(default)]
        keep_other_channels: bool,
    },
    ClearDimension,
    Cartography {
        #[serde(flatten)]
        change: CartographyChange,
    },
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DimensionValue {
    pub value: String,
    pub amount: Option<f64>,
    pub color: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CartographyChange {
    pub visible: Option<bool>,
    pub opacity: Option<f64>,
    pub boundary_color: Option<String>,
    pub boundary_width: Option<f64>,
    pub boundary_opacity: Option<f64>,
    pub boundary_line_type: Option<String>,
    pub line_type: Option<String>,
    pub line_type_scale: Option<f64>,
    pub boundary_line_type_scale: Option<f64>,
    pub direction_size: Option<f64>,
    pub direction_spacing: Option<f64>,
    pub casing_color: Option<String>,
    pub casing_width: Option<f64>,
    pub fill_pattern: Option<String>,
    pub pattern_color: Option<String>,
    pub pattern_background: Option<String>,
    pub pattern_spacing: Option<u32>,
    pub pattern_stroke: Option<f64>,
}
#[derive(Debug, Clone, Serialize)]
pub struct StylePlan {
    pub document: Value,
    pub receipt: Value,
    pub payload: Value,
}

/// Shared native/WASM request boundary. The host acquires the authenticated
/// snapshot and publishes the returned payload; planning has no map dependency.
pub fn plan_style_request(input: &[u8]) -> Result<String, String> {
    if input.is_empty() || input.len() > 16 * 1024 * 1024 {
        return Err("style plan input must contain 1 byte through 16 MiB".into());
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        snapshot: Value,
        reference: String,
        instruction: StyleInstruction,
    }
    let request: Request = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    let plan = plan_style_edit(&request.snapshot, &request.reference, &request.instruction)?;
    serde_json::to_string(&plan).map_err(|e| e.to_string())
}

pub fn style_catalog_request(input: &[u8]) -> Result<String, String> {
    if input.is_empty() || input.len() > 16 * 1024 * 1024 {
        return Err("style catalog input must contain 1 byte through 16 MiB".into());
    }
    #[derive(Deserialize)]
    #[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
    enum Request {
        List {
            snapshot: Value,
            query: Option<String>,
            limit: usize,
        },
        Read {
            snapshot: Value,
            reference: String,
        },
    }
    let request: Request = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    let result = match request {
        Request::List {
            snapshot,
            query,
            limit,
        } => list_styles(&snapshot, query.as_deref(), limit)?,
        Request::Read {
            snapshot,
            reference,
        } => describe_style(&snapshot, &reference)?,
    };
    serde_json::to_string(&result).map_err(|e| e.to_string())
}

fn field(value: &Value) -> Option<String> {
    let expr = value.as_array()?;
    if expr.first()?.as_str()? != "match" {
        return None;
    }
    let lookup = expr.get(1)?.as_array()?;
    match lookup.first()?.as_str()? {
        "get" => Some(lookup.get(1)?.as_str()?.trim().into()),
        "coalesce" => Some(lookup.get(1)?.get(1)?.as_str()?.trim().into()),
        _ => None,
    }
}
pub fn primary_style_field(style: &Value) -> Option<String> {
    [
        "circle-color",
        "line-color",
        "fill-color",
        "icon-color",
        "fill-extrusion-color",
    ]
    .iter()
    .find_map(|key| field(&style["paint"][key]))
}
pub fn second_style_channel(style: &Value) -> Option<Channel> {
    style_channels(style["type"].as_str()?)
        .into_iter()
        .find(|c| {
            c.number_prop
                .as_ref()
                .is_some_and(|p| field(&style[&c.number_group][p]).is_some())
                || c.color_prop
                    .as_ref()
                    .is_some_and(|p| field(&style["paint"][p]).is_some())
        })
}
fn checked_color(color: &str) -> Result<String, String> {
    normalize_style_color(color).ok_or_else(|| "colour must be #RRGGBB or #RRGGBBAA".into())
}
fn bounded(value: f64, min: f64, max: f64, name: &str) -> Result<f64, String> {
    if !value.is_finite() || value < min || value > max {
        Err(format!("{name} is outside {min}..{max}"))
    } else {
        Ok(value)
    }
}
fn style_editor<'a>(snapshot: &'a Value, reference: &str) -> Result<&'a Value, String> {
    snapshot["style_editors"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["style_ref"] == reference))
        .ok_or_else(|| "backend style editor is unavailable for this ref".into())
}
fn icon_names(snapshot: &Value) -> Vec<String> {
    let catalog = &snapshot["symbol_catalog"];
    let mut names = Vec::new();
    for name in catalog["built_ins"].as_array().into_iter().flatten() {
        if let Some(name) = name.as_str() {
            names.push(name.into());
        }
    }
    for group in catalog["groups"].as_array().into_iter().flatten() {
        for name in group["icons"].as_array().into_iter().flatten() {
            if let Some(name) = name.as_str() {
                names.push(name.into());
            }
        }
    }
    for icon in catalog["custom_icons"].as_array().into_iter().flatten() {
        if let Some(name) = icon["name"].as_str() {
            names.push(name.into());
        }
    }
    names.sort();
    names.dedup();
    names
}
fn size_property(kind: &str) -> Option<(&str, &str)> {
    match kind {
        "circle" => Some(("paint", "circle-radius")),
        "line" => Some(("paint", "line-width")),
        "symbol" => Some(("layout", "icon-size")),
        _ => None,
    }
}
fn numeric_baseline(value: &Value) -> Option<f64> {
    if value.is_null() {
        return Some(1.);
    }
    if let Some(v) = value.as_f64() {
        return Some(v);
    }
    let widened = widen_line_width(value, 0.).ok()?;
    let n = widened.as_array()?.len();
    let index = match widened[0].as_str()? {
        "interpolate" => 4,
        "step" | "case" => 2,
        "match" if n >= 5 => 3,
        _ => return None,
    };
    widened[index].as_f64()
}
fn dash_table(snapshot: &Value) -> Map<String, Value> {
    let mut result = Map::new();
    for row in snapshot["platform_config"]["dash_presets"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(key) = row["key"].as_str().filter(|v| !v.is_empty()) {
            let pattern = row["pattern"]
                .as_array()
                .filter(|v| !v.is_empty())
                .map_or(Value::Null, |v| json!(v));
            result.insert(key.into(), pattern);
        }
    }
    if result.is_empty() {
        result=serde_json::from_value(json!({"solid":null,"dashed":[2,2],"dotted":[0.5,1.5],"dash-dot":[3,1,0.5,1],"long-dash":[5,2],"dash-dot-dot":[3,1,0.5,1,0.5,1]})).expect("fixed table");
    }
    result
}
fn current_line_type(style: &Value, table: &Map<String, Value>) -> String {
    if let Some(name) = style["metadata"]["line_type_name"]
        .as_str()
        .filter(|name| table.contains_key(*name) || *name == "directional")
    {
        return name.into();
    }
    if style["metadata"]["line_marker"] == "arrow" {
        "directional".into()
    } else {
        table
            .iter()
            .find(|(_, pattern)| {
                pattern.is_array() && *pattern == &style["paint"]["line-dasharray"]
            })
            .map(|(key, _)| key.clone())
            .unwrap_or_else(|| "solid".into())
    }
}
fn cartography(
    mut style: Value,
    change: &CartographyChange,
    snapshot: &Value,
) -> Result<Value, String> {
    let kind = style["type"]
        .as_str()
        .ok_or("missing style type")?
        .to_owned();
    let wants_line = change.line_type.is_some()
        || change.line_type_scale.is_some()
        || change.direction_size.is_some()
        || change.direction_spacing.is_some();
    let wants_casing = change.casing_color.is_some() || change.casing_width.is_some();
    let wants_pattern = change.fill_pattern.is_some()
        || change.pattern_color.is_some()
        || change.pattern_background.is_some()
        || change.pattern_spacing.is_some()
        || change.pattern_stroke.is_some();
    let wants_boundary = change.boundary_color.is_some()
        || change.boundary_width.is_some()
        || change.boundary_opacity.is_some()
        || change.boundary_line_type.is_some()
        || change.boundary_line_type_scale.is_some();
    if !wants_line
        && !wants_casing
        && !wants_pattern
        && !wants_boundary
        && change.visible.is_none()
        && change.opacity.is_none()
    {
        return Err("choose a line type, casing or hatch".into());
    }
    if let Some(visible) = change.visible {
        style["layout"]["visibility"] = json!(if visible { "visible" } else { "none" });
    }
    if let Some(opacity) = change.opacity {
        let key = match kind.as_str() {
            "fill" => "fill-opacity",
            "line" => "line-opacity",
            "circle" => "circle-opacity",
            "symbol" => "icon-opacity",
            _ => return Err("opacity requires a supported geometry style".into()),
        };
        style["paint"][key] = json!(bounded(opacity, 0., 1., "opacity")?);
    }
    if wants_boundary {
        if kind != "fill" {
            return Err("polygon boundary controls require a fill layer".into());
        }
        if !style["boundary"].is_object() {
            style["boundary"] = json!({"paint":{"line-color":"#64748B","line-width":1,"line-opacity":1},"layout":{"visibility":"visible"}});
        }
        if let Some(color) = &change.boundary_color {
            style["boundary"]["paint"]["line-color"] = json!(checked_color(color)?);
        }
        if let Some(width) = change.boundary_width {
            style["boundary"]["paint"]["line-width"] =
                json!(bounded(width, 0., 20., "boundary width")?);
        }
        if let Some(opacity) = change.boundary_opacity {
            style["boundary"]["paint"]["line-opacity"] =
                json!(bounded(opacity, 0., 1., "boundary opacity")?);
        }
        if change.boundary_line_type.is_some() || change.boundary_line_type_scale.is_some() {
            let table = dash_table(snapshot);
            let requested = change
                .boundary_line_type
                .clone()
                .unwrap_or_else(|| current_line_type(&style["boundary"], &table));
            if !table.contains_key(&requested) {
                return Err(
                    "boundary line type is absent from the published dash vocabulary".into(),
                );
            }
            let mut boundary = style["boundary"].clone();
            boundary["type"] = json!("line");
            let mut scale = Map::new();
            if let Some(value) = change.boundary_line_type_scale {
                scale.insert(
                    "dash".into(),
                    json!(bounded(value, 0.1, 100., "boundary line type scale")?),
                );
            }
            boundary = apply_line_type(boundary, &requested, &table, &scale)?;
            boundary.as_object_mut().unwrap().remove("type");
            style["boundary"] = boundary;
        }
    }
    if wants_line {
        if kind != "line" {
            return Err("line type requires a line layer".into());
        }
        let table = dash_table(snapshot);
        let requested = change
            .line_type
            .clone()
            .unwrap_or_else(|| current_line_type(&style, &table));
        if requested != "directional" && !table.contains_key(&requested) {
            return Err("line type is absent from the published dash vocabulary".into());
        }
        if requested != "directional"
            && (change.direction_size.is_some() || change.direction_spacing.is_some())
        {
            return Err("direction size/spacing require a directional line".into());
        }
        let mut scale = Map::new();
        if let Some(value) = change.line_type_scale {
            scale.insert(
                "dash".into(),
                json!(bounded(value, 0.1, 100., "line type scale")?),
            );
        }
        if let Some(size) = change.direction_size {
            scale.insert(
                "size".into(),
                json!(bounded(size, 6., 48., "direction size")?),
            );
        }
        if let Some(spacing) = change.direction_spacing {
            scale.insert(
                "spacing".into(),
                json!(bounded(spacing, 20., 1000., "direction spacing")?),
            );
        }
        style = apply_line_type(style, &requested, &table, &scale)?;
    }
    if wants_casing {
        if kind != "line" {
            return Err("casing requires a line layer".into());
        }
        if change.casing_width == Some(0.) {
            if change.casing_color.is_some() {
                return Err("clearing a casing takes no colour".into());
            }
            style
                .as_object_mut()
                .ok_or("invalid style")?
                .remove("boundary");
        } else {
            let color = change
                .casing_color
                .as_deref()
                .or_else(|| style["boundary"]["paint"]["line-color"].as_str())
                .ok_or("new casing requires colour and width")?;
            let color = checked_color(color)?;
            let width = change
                .casing_width
                .or_else(|| {
                    Some(
                        (numeric_baseline(&style["boundary"]["paint"]["line-width"])?
                            - numeric_baseline(&style["paint"]["line-width"])?)
                            / 2.,
                    )
                })
                .ok_or("new casing requires colour and width")?;
            style = apply_casing(style, &color, width)?;
        }
    }
    if wants_pattern {
        if kind != "fill" {
            return Err("hatching requires a fill layer".into());
        }
        if change.fill_pattern.as_deref() == Some("solid") {
            if change.pattern_color.is_some()
                || change.pattern_background.is_some()
                || change.pattern_spacing.is_some()
                || change.pattern_stroke.is_some()
            {
                return Err("solid clears the hatch and takes no recipe".into());
            }
            style = apply_pattern(style, None)?;
        } else {
            let metadata = &style["metadata"];
            let pattern = change
                .fill_pattern
                .as_deref()
                .or_else(|| metadata["fill_pattern"].as_str())
                .ok_or("hatch requires a pattern")?;
            let color = checked_color(
                change
                    .pattern_color
                    .as_deref()
                    .or_else(|| metadata["fill_pattern_color"].as_str())
                    .ok_or("hatch requires a colour")?,
            )?;
            let background = change
                .pattern_background
                .as_deref()
                .or_else(|| metadata["fill_pattern_background"].as_str())
                .unwrap_or("");
            let recipe = PatternRecipe {
                pattern: pattern.into(),
                color,
                background: if background.is_empty() {
                    String::new()
                } else {
                    checked_color(background)?
                },
                spacing: change
                    .pattern_spacing
                    .or_else(|| {
                        metadata["fill_pattern_spacing_px"]
                            .as_u64()
                            .and_then(|v| v.try_into().ok())
                    })
                    .ok_or("hatch requires spacing")?,
                stroke: change
                    .pattern_stroke
                    .or_else(|| metadata["fill_pattern_stroke_px"].as_f64())
                    .ok_or("hatch requires stroke")?,
            };
            style = apply_pattern(style, Some(&recipe))?;
        }
    }
    Ok(style)
}
fn normalize_icon(value: &Value, images: &[Value]) -> Value {
    if let Some(name) = value.as_str() {
        return images
            .iter()
            .find(|image| image["id"] == name)
            .and_then(|image| image.get("source"))
            .cloned()
            .unwrap_or_else(|| value.clone());
    }
    let Some(expr) = value.as_array() else {
        return value.clone();
    };
    let n = expr.len();
    let indices: Vec<usize> = match expr.first().and_then(Value::as_str) {
        Some("match") if n >= 5 && n % 2 == 1 => (3..n - 1)
            .step_by(2)
            .chain(std::iter::once(n - 1))
            .collect(),
        Some("case") if n >= 4 && n % 2 == 0 => (2..n - 1)
            .step_by(2)
            .chain(std::iter::once(n - 1))
            .collect(),
        _ => return value.clone(),
    };
    let mut next = value.clone();
    for index in &indices {
        next[*index] = normalize_icon(&value[*index], images);
    }
    // Collapse only if EVERY output is the same literal; preserve nested choices.
    if let Some(first) = indices.first().and_then(|i| next[*i].as_str()) {
        if indices.iter().all(|i| next[*i].as_str() == Some(first)) {
            return json!(first);
        }
    }
    next
}
pub fn style_save_payload(
    snapshot: &Value,
    reference: &str,
    document: &Value,
) -> Result<Value, String> {
    let editor = style_editor(snapshot, reference)?;
    let target = editor["style_target"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("backend style target is missing")?;
    if editor["layer_name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .is_none()
        && editor["slug"].as_str().filter(|s| !s.is_empty()).is_none()
    {
        return Err("backend style target identity is missing".into());
    }
    let mut document = document.clone();
    if document["type"] == "symbol" {
        if let Some(icon) = document["layout"].get("icon-image").cloned() {
            let images = snapshot["required_images"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let normalized = normalize_icon(&icon, &images);
            if normalized.is_string()
                && (document["paint"]["icon-color"].is_null()
                    || document["paint"]["icon-color"] == "")
            {
                if let Some(color) = images
                    .iter()
                    .find(|v| v["id"] == icon)
                    .and_then(|v| v.get("color"))
                    .filter(|v| v.is_string())
                {
                    if !document["paint"].is_object() {
                        document["paint"] = json!({});
                    }
                    document["paint"]["icon-color"] = color.clone();
                }
            }
            document["layout"]["icon-image"] = normalized;
        }
    }
    let mut payload = json!({"style_ref":reference,"style_data":document});
    for key in [
        "style_target",
        "slug",
        "template_name",
        "styling_template_id",
        "layer_name",
        "available_fields",
        "field_values",
    ] {
        if let Some(value) = editor.get(key) {
            payload[key] = value.clone();
        }
    }
    if ![
        "design",
        "design_vt",
        "user_data",
        "project_data",
        "global_tile",
        "ds_grid",
        "print_context",
        "print",
    ]
    .contains(&target)
    {
        payload["project_id"] = snapshot["project_id"].clone();
    }
    Ok(payload)
}
pub fn plan_style_edit(
    snapshot: &Value,
    reference: &str,
    instruction: &StyleInstruction,
) -> Result<StylePlan, String> {
    if reference.is_empty() || reference.len() > 500 {
        return Err("invalid style reference".into());
    }
    let editor = style_editor(snapshot, reference)?;
    let style = snapshot["styles"]
        .get(reference)
        .filter(|v| v.is_object())
        .ok_or("no backend document for this style ref")?;
    let kind = style["type"].as_str().ok_or("missing style type")?;

    if matches!(instruction, StyleInstruction::Seed) {
        if editor["style_target"] != "print_context" {
            return Err("style seed is limited to declared print context sources".into());
        }
        let mut payload = style_save_payload(snapshot, reference, style)?;
        payload["create_only"] = json!(true);
        let document = payload["style_data"].clone();
        return Ok(StylePlan {
            document: document.clone(),
            receipt: json!({
                "project": snapshot["project_id"], "ref": reference,
                "type": kind, "document": document,
                "published": false, "dryRun": true, "createOnly": true,
            }),
            payload,
        });
    }

    if matches!(instruction, StyleInstruction::PrintVariant) {
        if reference.ends_with("_print") {
            return Err("source style is already a print variant".into());
        }
        let print_ref = format!("{reference}_print");
        if print_ref.len() > 500 {
            return Err("print style ref exceeds 500 characters".into());
        }
        // Reuse the existing authored-save normalization so a runtime sprite
        // id is converted back to its catalog icon name before cloning.
        let source_payload = style_save_payload(snapshot, reference, style)?;
        let document = crate::print_scaling::apply_print_scaling(
            source_payload["style_data"].clone(),
            crate::print_scaling::PrintScaling::default(),
        )?;
        let mut fields = editor["available_fields"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if !fields.contains(&json!("print_paper_size")) {
            fields.push(json!("print_paper_size"));
        }
        let mut field_values = editor["field_values"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        field_values.insert(
            "print_paper_size".into(),
            json!([
                "print_a0",
                "print_a1",
                "print_a2",
                "print_a3",
                "print_a4",
                "print_a5",
                "print_custom"
            ]),
        );
        let payload = json!({
            "style_ref": print_ref,
            "style_target": "print",
            "layer_name": print_ref,
            "style_data": document,
            "available_fields": fields,
            "field_values": field_values,
            "create_only": true,
        });
        return Ok(StylePlan {
            document: document.clone(),
            receipt: json!({
                "project": snapshot["project_id"], "ref": print_ref,
                "sourceRef": reference, "type": kind, "paperField": "print_paper_size",
                "paperValues": ["print_a0", "print_a1", "print_a2", "print_a3", "print_a4", "print_a5", "print_custom"],
                "document": document, "published": false, "dryRun": true,
            }),
            payload,
        });
    }
    let mut next = style.clone();
    let mut receipt = json!({"project":snapshot["project_id"],"ref":reference,"type":kind,"published":false,"dryRun":true,"onMap":null});
    match instruction {
        StyleInstruction::Appearance { color, icon, size } => {
            if color.is_none() && icon.is_none() && size.is_none() {
                return Err("choose colour, icon or size".into());
            }
            let mut change = json!({});
            if let Some(color) = color {
                if !["circle", "symbol", "line", "fill", "fill-extrusion"].contains(&kind) {
                    return Err("unsupported guided appearance".into());
                }
                change["color"] = json!(checked_color(color)?);
            }
            if let Some(icon) = icon {
                if kind != "symbol" || !icon_names(snapshot).contains(icon) {
                    return Err("icon is not in the published catalog".into());
                }
                change["icon"] = json!(icon);
            }
            let second = second_style_channel(style);
            let size_is_second = size.is_some() && second.as_ref().is_some_and(|c| c.key == "size");
            if let Some(size) = size {
                let (group, property) =
                    size_property(kind).ok_or("this layer has no guided size")?;
                let schema = snapshot["style_properties"][kind]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|r| r["key"] == property))
                    .filter(|v| v["type"] == "number")
                    .ok_or("numeric property schema is missing")?;
                bounded(
                    *size,
                    schema["min"].as_f64().ok_or("minimum missing")?,
                    schema["max"].as_f64().ok_or("maximum missing")?,
                    "size",
                )?;
                if size_is_second {
                    let expr = next[group][property]
                        .as_array_mut()
                        .ok_or("invalid size expression")?;
                    *expr.last_mut().ok_or("empty size expression")? = json!(size);
                } else {
                    change["size"] = json!(size);
                }
            }
            next = apply_appearance(next, &change)?;
            receipt["requested"] = json!({"color":color,"icon":icon,"size":size});
            receipt["baseSizeUpdatedAsFallback"] = json!(size_is_second);
        }
        StyleInstruction::Label {
            field: requested_field,
            options,
        } => {
            if requested_field.is_empty()
                || requested_field.len() > 120
                || requested_field.trim() != requested_field
            {
                return Err("label needs one bounded field without surrounding whitespace".into());
            }
            let supports_field = snapshot["label_editor"]["field_part_types"]
                .as_array()
                .is_some_and(|parts| parts.contains(&json!("field")));
            if !supports_field {
                return Err("backend label editor does not publish field labels".into());
            }
            if !editor["available_fields"]
                .as_array()
                .is_some_and(|fields| fields.contains(&json!(requested_field)))
            {
                return Err("label field is absent from the published vocabulary".into());
            }
            let mut label = if style["label"].is_object() {
                style["label"].clone()
            } else {
                editor["label_model"]
                    .as_object()
                    .map(|_| editor["label_model"].clone())
                    .ok_or("backend label model is unavailable for this ref")?
            };
            if !label["layout"].is_object() {
                return Err("backend label model has no layout".into());
            }
            label["layout"]["text-field"] = json!(["get", requested_field]);
            crate::style_label::apply_options(&mut label, options, snapshot, editor)?;
            next["label"] = label;
            receipt["labelField"] = json!(requested_field);
            receipt["labelOptions"] = json!(options);
            receipt["changed"] = json!(["label"]);
        }
        StyleInstruction::Dimension {
            field: requested_field,
            channel,
            values,
            other,
            color,
            field_type,
            keep_other_channels,
        } => {
            if requested_field.trim().is_empty()
                || requested_field.len() > 120
                || values.is_empty()
                || values.len() > 50
            {
                return Err("dimension needs a bounded field and 1..50 values".into());
            }
            if primary_style_field(style).as_deref() == Some(requested_field) {
                return Err("field already drives primary colour; choose a second field".into());
            }
            if editor["available_fields"]
                .as_array()
                .is_some_and(|fields| !fields.contains(&json!(requested_field)))
            {
                return Err("field is absent from the published vocabulary".into());
            }
            let def = style_channels(kind)
                .into_iter()
                .find(|c| &c.key == channel)
                .ok_or("unsupported channel")?;
            let ring = color
                .as_deref()
                .map(checked_color)
                .transpose()?
                .unwrap_or_else(|| def.default_color.clone());
            let mut state = DimensionState {
                channel: channel.clone(),
                field: requested_field.clone(),
                aliases: vec![],
                values: vec![],
                raw_values: Map::new(),
                numbers: Map::new(),
                colors: Map::new(),
                fallback_number: bounded(
                    other.unwrap_or(def.fallback),
                    def.min,
                    def.max,
                    "fallback",
                )?,
                fallback_color: ring.clone(),
            };
            if other.is_some_and(|v| !v.is_finite()) {
                return Err("fallback must be finite".into());
            }
            // Declared domain typing may replace a host map scan; observed values never define a domain.
            let declared_type = editor["field_domains"][requested_field]["type"]
                .as_str()
                .map(|kind| match kind {
                    "categorical_string" => "string",
                    "categorical_number" | "numeric" => "number",
                    other => other,
                });
            if field_type
                .as_deref()
                .zip(declared_type)
                .is_some_and(|(requested, declared)| {
                    requested != declared && !(requested == "number" && declared == "numeric")
                })
            {
                return Err("field type conflicts with published domain".into());
            }
            let value_type = declared_type.or(field_type.as_deref()).or(Some("string"));
            if !matches!(
                value_type,
                Some("string" | "number" | "numeric" | "boolean")
            ) {
                return Err("unsupported match label type".into());
            }
            let mut labels = std::collections::HashSet::new();
            for value in values {
                if value.value.is_empty()
                    || value.value.len() > 200
                    || state.values.contains(&value.value)
                {
                    return Err("dimension values must be nonempty, bounded and unique".into());
                }
                state.values.push(value.value.clone());
                let raw = match value_type {
                    Some("number" | "numeric") => value
                        .value
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .map(|v| json!(v))
                        .ok_or("dimension value must be a finite number")?,
                    Some("boolean") => match value.value.as_str() {
                        "true" => json!(true),
                        "false" => json!(false),
                        _ => return Err("dimension value must be true or false".into()),
                    },
                    _ => json!(value.value),
                };
                if !labels.insert(raw.to_string()) {
                    return Err("duplicate typed match label".into());
                }
                state.raw_values.insert(value.value.clone(), raw);
                if def.number_prop.is_some() {
                    state.numbers.insert(
                        value.value.clone(),
                        json!(bounded(
                            value.amount.unwrap_or(def.off),
                            def.min,
                            def.max,
                            "dimension amount"
                        )?),
                    );
                }
                if def.color_prop.is_some() {
                    state.colors.insert(
                        value.value.clone(),
                        json!(value
                            .color
                            .as_deref()
                            .map(checked_color)
                            .transpose()?
                            .unwrap_or_else(|| ring.clone())),
                    );
                }
            }
            if let Some(old) =
                second_style_channel(style).filter(|c| c.key != def.key && !keep_other_channels)
            {
                next = clear_dimension(next, &old.key)?;
            }
            next = apply_dimension(next, channel, &state)?;
            receipt["field"] = json!(requested_field);
            receipt["fieldType"] = json!(value_type);
            receipt["channel"] = json!(channel);
            receipt["second"] = json!(format!(
                "{} → {} ({} explicit)",
                requested_field,
                def.label.to_lowercase(),
                values.len()
            ));
            let mut expressions = Map::new();
            if let Some(prop) = def.number_prop {
                expressions.insert(prop.clone(), next[&def.number_group][&prop].clone());
            }
            if let Some(prop) = def.color_prop {
                expressions.insert(prop.clone(), next["paint"][&prop].clone());
            }
            receipt["expressions"] = json!(expressions);
        }
        StyleInstruction::ClearDimension => {
            if let Some(def) = second_style_channel(style) {
                next = clear_dimension(next, &def.key)?;
                receipt["removed"] = json!(def.key);
            } else {
                receipt["removed"] = Value::Null;
            }
        }
        StyleInstruction::Cartography { change } => {
            next = cartography(next, change, snapshot)?;
            receipt["changed"] = json!(["cartography"]);
            receipt["requested"] = json!(change);
        }
        StyleInstruction::Seed | StyleInstruction::PrintVariant => {
            unreachable!("handled before guided edits")
        }
    }
    normalize_json_numbers(&mut next);
    let payload = style_save_payload(snapshot, reference, &next)?;
    let document = payload["style_data"].clone();
    receipt["document"] = document.clone();
    Ok(StylePlan {
        document,
        receipt,
        payload,
    })
}

/// Bounded headless description. Runtime presence is not inferred from the catalog.
pub fn describe_style(snapshot: &Value, reference: &str) -> Result<Value, String> {
    let editor = style_editor(snapshot, reference)?;
    let document = snapshot["styles"]
        .get(reference)
        .ok_or("style document is missing")?;
    if serde_json::to_vec(document)
        .map_err(|_| "invalid document")?
        .len()
        > MAX_STYLE_REQUEST_BYTES
    {
        return Err("style document exceeds 2 MiB".into());
    }
    let kind = document["type"].as_str().ok_or("style type is missing")?;
    let channels:Vec<Value>=style_channels(kind).into_iter().map(|c|json!({"channel":c.key,"numberProperty":c.number_prop,"colorProperty":c.color_prop,"min":c.min,"max":c.max,"on":c.on,"off":c.off})).collect();
    let fields: Vec<&Value> = editor["available_fields"]
        .as_array()
        .into_iter()
        .flatten()
        .take(200)
        .collect();
    let mut values = Map::new();
    let mut more = editor["available_fields"]
        .as_array()
        .is_some_and(|a| a.len() > 200);
    for (key, row) in editor["field_values"]
        .as_object()
        .into_iter()
        .flatten()
        .take(200)
    {
        let entries = row.as_array().ok_or("invalid field value domain")?;
        more |= entries.len() > 50;
        values.insert(
            key.clone(),
            json!(entries.iter().take(50).collect::<Vec<_>>()),
        );
    }
    more |= editor["field_values"]
        .as_object()
        .is_some_and(|a| a.len() > 200);
    let second = second_style_channel(document).map(|c| {
        let f = c
            .number_prop
            .as_ref()
            .and_then(|p| field(&document[&c.number_group][p]))
            .or_else(|| {
                c.color_prop
                    .as_ref()
                    .and_then(|p| field(&document["paint"][p]))
            });
        json!({"field":f,"channel":c.key})
    });
    Ok(
        json!({"project":snapshot["project_id"],"ref":reference,"type":kind,"target":editor["style_target"],"document":document,"fields":fields,"fieldValues":values,"channels":channels,"propertySchema":snapshot["style_properties"][kind],"labelSchema":snapshot["label_editor"],"icons":icon_names(snapshot).into_iter().take(200).collect::<Vec<_>>(),"fieldDomains":editor["field_domains"],"second":second,"colorField":primary_style_field(document),"onMap":null,"more":more}),
    )
}
pub fn list_styles(snapshot: &Value, query: Option<&str>, limit: usize) -> Result<Value, String> {
    if !(1..=200).contains(&limit) {
        return Err("style limit must be 1..200".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut rows = Vec::new();
    for editor in snapshot["style_editors"]
        .as_array()
        .ok_or("style editors are missing")?
    {
        let reference = editor["style_ref"].as_str().ok_or("style ref is missing")?;
        if !seen.insert(reference) {
            continue;
        }
        if query.is_some_and(|q| !reference.to_lowercase().contains(&q.to_lowercase())) {
            continue;
        }
        let Some(style) = snapshot["styles"].get(reference) else {
            continue;
        };
        rows.push(json!({"ref":reference,"type":style["type"],"target":editor["style_target"],"colorField":primary_style_field(style)}));
    }
    let total = rows.len();
    rows.truncate(limit);
    Ok(json!({"project":snapshot["project_id"],"total":total,"styles":rows,"more":total>limit}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygon_boundary_is_independent_and_opacity_is_bounded() {
        let style = json!({"type":"fill","layout":{"visibility":"none"},"paint":{"fill-color":"#ffffff","fill-opacity":0.3}});
        let change = CartographyChange {
            visible: Some(true),
            opacity: Some(0.),
            boundary_color: Some("#7C8792".into()),
            boundary_width: Some(0.7),
            boundary_opacity: Some(0.5),
            boundary_line_type: Some("dashed".into()),
            ..Default::default()
        };
        let out = cartography(style, &change, &json!({})).unwrap();
        assert_eq!(out["paint"]["fill-opacity"], 0.);
        assert_eq!(out["boundary"]["paint"]["line-width"], 0.7);
        assert_eq!(out["layout"]["visibility"], "visible");
        assert_eq!(out["boundary"]["paint"]["line-dasharray"], json!([2, 2]));
        let solid = cartography(
            out.clone(),
            &CartographyChange {
                boundary_line_type: Some("solid".into()),
                ..Default::default()
            },
            &json!({}),
        )
        .unwrap();
        assert!(solid["boundary"]["paint"].get("line-dasharray").is_none());
        assert!(cartography(
            out.clone(),
            &CartographyChange {
                boundary_line_type: Some("directional".into()),
                ..Default::default()
            },
            &json!({})
        )
        .is_err());
        assert!(cartography(json!({"type":"line"}), &change, &json!({})).is_err());
        assert!(cartography(
            out,
            &CartographyChange {
                opacity: Some(1.1),
                ..Default::default()
            },
            &json!({})
        )
        .is_err());
    }

    #[test]
    fn held_request_matches_native_plan_without_map_state() {
        let state = snapshot(json!({"type":"line","paint":{"line-width":3}}));
        let request = json!({"snapshot":state,"reference":"master/poles","instruction":{"kind":"print_variant"}});
        let held: Value = serde_json::from_str(
            &plan_style_request(&serde_json::to_vec(&request).unwrap()).unwrap(),
        )
        .unwrap();
        let native =
            plan_style_edit(&state, "master/poles", &StyleInstruction::PrintVariant).unwrap();
        assert_eq!(held, serde_json::to_value(native).unwrap());
        let mut invalid = request;
        invalid["map"] = json!({});
        assert!(plan_style_request(&serde_json::to_vec(&invalid).unwrap()).is_err());
        assert!(plan_style_request(&[]).is_err());
    }

    #[test]
    fn held_catalog_matches_native_without_map_or_silent_extra_arguments() {
        let state = snapshot(json!({"type":"line","paint":{"line-width":3}}));
        for (request, expected) in [
            (
                json!({"operation":"list","snapshot":state,"limit":10}),
                list_styles(&state, None, 10).unwrap(),
            ),
            (
                json!({"operation":"read","snapshot":state,"reference":"master/poles"}),
                describe_style(&state, "master/poles").unwrap(),
            ),
        ] {
            let output: Value = serde_json::from_str(
                &style_catalog_request(&serde_json::to_vec(&request).unwrap()).unwrap(),
            )
            .unwrap();
            assert_eq!(output, expected);
            let mut invalid = request;
            invalid["map"] = json!({});
            assert!(style_catalog_request(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
    }
    fn snapshot(style: Value) -> Value {
        json!({
            "project_id":"p",
            "styles":{"master/poles":style},
            "style_editors":[{
                "style_ref":"master/poles",
                "style_target":"design",
                "layer_name":"poles",
                "available_fields":["status","kind","road_no"],
                "field_domains":{"status":{"type":"boolean"}},
                "label_model":{
                    "type":"symbol",
                    "paint":{"text-color":"#333333","text-halo-width":1},
                    "layout":{"text-field":["get","status"],"text-size":12,"visibility":"visible"}
                }
            }],
            "label_editor":{"field_part_types":["literal","field"]},
            "style_properties":{"circle":[{"key":"circle-radius","type":"number","min":1,"max":30}]}
        })
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn guided_dimension_uses_domain_type_and_keeps_unrelated_authorship() {
        let before = json!({"type":"circle","paint":{"circle-color":["match",["get","kind"],"wood","#FFFFFF","#000000"]},"label":{"custom":true},"filter":["has","id"],"minzoom":9,"metadata":{"custom":"retained"}});
        let request = StyleInstruction::Dimension {
            field: "status".into(),
            channel: "halo".into(),
            values: vec![DimensionValue {
                value: "true".into(),
                amount: Some(2.),
                color: None,
            }],
            other: None,
            color: Some("#ff0000".into()),
            field_type: None,
            keep_other_channels: false,
        };
        let plan = plan_style_edit(&snapshot(before.clone()), "master/poles", &request).unwrap();
        assert_eq!(plan.document["paint"]["circle-stroke-width"][2], true);
        for key in ["label", "filter", "minzoom", "metadata"] {
            assert_eq!(plan.document[key], before[key]);
        }
        assert!(plan.payload.get("project_id").is_none());
        assert_eq!(plan.payload["style_target"], "design");
        assert_eq!(plan.receipt["published"], false);
    }

    #[test]
    fn opacity_can_preserve_an_existing_halo_channel() {
        let style = json!({"type":"circle","paint":{"circle-color":"#123456","circle-stroke-width":["match",["get","status"],true,2,1],"circle-stroke-color":"#000000"}});
        let request = StyleInstruction::Dimension {
            field: "status".into(),
            channel: "opacity".into(),
            values: vec![DimensionValue {
                value: "true".into(),
                amount: Some(0.5),
                color: None,
            }],
            other: Some(1.),
            color: None,
            field_type: None,
            keep_other_channels: true,
        };
        let plan = plan_style_edit(&snapshot(style.clone()), "master/poles", &request).unwrap();
        assert_eq!(
            plan.document["paint"]["circle-stroke-width"],
            style["paint"]["circle-stroke-width"]
        );
        assert_eq!(
            plan.document["paint"]["circle-opacity"],
            json!(["match", ["get", "status"], true, 0.5, 1])
        );
    }

    #[test]
    fn categorical_string_domains_accept_black_draft_halos() {
        let mut source =
            snapshot(json!({"type":"circle","paint":{"circle-color":"#123456"},"layout":{}}));
        source["style_editors"][0]["field_domains"]["status"]["type"] = json!("categorical_string");
        let plan = plan_style_edit(
            &source,
            "master/poles",
            &StyleInstruction::Dimension {
                field: "status".into(),
                channel: "halo".into(),
                values: vec![DimensionValue {
                    value: "draft".into(),
                    amount: Some(1.5),
                    color: Some("#000000".into()),
                }],
                other: Some(0.),
                color: Some("#000000".into()),
                field_type: Some("string".into()),
                keep_other_channels: false,
            },
        )
        .unwrap();
        assert_eq!(plan.document["paint"]["circle-stroke-color"][3], "#000000");
        assert_eq!(plan.document["paint"]["circle-stroke-width"][2], "draft");
    }

    #[test]
    fn guided_label_changes_only_the_field_and_preserves_print_authorship() {
        let before = json!({
            "type":"line",
            "paint":{"line-color":"#7F3C8D","line-width":3},
            "layout":{"line-cap":"round","visibility":"visible"},
            "filter":["has","road_no"],
            "metadata":{
                "geometry_type":"line",
                "print_scaling":{"reference_paper":"A3","units":"css_px","factors":{"A0":2.8284271247461903,"A3":1}}
            },
            "label":{
                "type":"symbol",
                "minzoom":15,
                "paint":{"text-color":"#333333","text-halo-width":1},
                "layout":{
                    "symbol-placement":"line",
                    "text-field":["get","name"],
                    "text-font":["Open Sans Regular"],
                    "text-size":12,
                    "visibility":"visible"
                }
            }
        });
        let plan = plan_style_edit(
            &snapshot(before.clone()),
            "master/poles",
            &StyleInstruction::Label {
                field: "road_no".into(),
                options: Default::default(),
            },
        )
        .unwrap();
        let mut expected = before.clone();
        expected["label"]["layout"]["text-field"] = json!(["get", "road_no"]);
        assert_eq!(plan.document, expected);
        assert_eq!(plan.receipt["labelField"], "road_no");
        assert_eq!(plan.receipt["changed"], json!(["label"]));
        assert_eq!(plan.payload["style_data"], expected);
    }

    #[test]
    fn guided_label_uses_backend_model_when_absent_and_refuses_unknown_fields() {
        let state = snapshot(json!({
            "type":"line",
            "paint":{"line-color":"#7F3C8D"},
            "metadata":{"custom":"retained"}
        }));
        let plan = plan_style_edit(
            &state,
            "master/poles",
            &StyleInstruction::Label {
                field: "road_no".into(),
                options: Default::default(),
            },
        )
        .unwrap();
        assert_eq!(
            plan.document["label"]["layout"]["text-field"],
            json!(["get", "road_no"])
        );
        assert_eq!(plan.document["label"]["layout"]["text-size"], 12);
        assert_eq!(plan.document["label"]["paint"]["text-halo-width"], 1);
        assert_eq!(plan.document["metadata"]["custom"], "retained");

        for field in ["name", " road_no", ""] {
            assert!(plan_style_edit(
                &state,
                "master/poles",
                &StyleInstruction::Label {
                    field: field.into(),
                    options: Default::default(),
                },
            )
            .is_err());
        }
        let held = json!({
            "snapshot":state,
            "reference":"master/poles",
            "instruction":{"kind":"label","field":"road_no"}
        });
        let output: Value =
            serde_json::from_str(&plan_style_request(&serde_json::to_vec(&held).unwrap()).unwrap())
                .unwrap();
        assert_eq!(
            output["document"]["label"]["layout"]["text-field"],
            json!(["get", "road_no"])
        );
    }

    #[test]
    fn print_variant_is_create_only_and_keeps_catalog_icon_authorship() {
        let mut state = snapshot(json!({
            "type":"symbol", "paint":{"icon-color":"#123456"},
            "layout":{"icon-image":"runtime/icon/school"}
        }));
        state["required_images"] = json!([{
            "id":"runtime/icon/school", "source":"school", "color":"#123456"
        }]);
        let plan =
            plan_style_edit(&state, "master/poles", &StyleInstruction::PrintVariant).unwrap();
        assert_eq!(plan.receipt["ref"], "master/poles_print");
        assert_eq!(plan.receipt["paperField"], "print_paper_size");
        assert_eq!(plan.payload["style_target"], "print");
        assert_eq!(plan.payload["layer_name"], "master/poles_print");
        assert_eq!(plan.payload["create_only"], true);
        assert_eq!(plan.document["layout"]["icon-image"], "school");
        assert!(plan.payload["available_fields"]
            .as_array()
            .unwrap()
            .contains(&json!("print_paper_size")));
        assert!(plan_style_edit(
            &state,
            "master/poles_print",
            &StyleInstruction::PrintVariant
        )
        .is_err());
    }
    #[test]
    fn seed_is_create_only_and_limited_to_backend_declared_print_context() {
        let mut state = snapshot(json!({
            "type":"line", "paint":{"line-color":"#725c48","line-width":1.058},
            "metadata":{"geometry_type":"line"}
        }));
        state["style_editors"][0]["style_ref"] = json!("print_context/contours_index");
        state["style_editors"][0]["style_target"] = json!("print_context");
        state["style_editors"][0]["layer_name"] = json!("contours_index");
        let style = state["styles"]["master/poles"].take();
        state["styles"] = json!({"print_context/contours_index":style});

        let plan = plan_style_edit(
            &state,
            "print_context/contours_index",
            &StyleInstruction::Seed,
        )
        .unwrap();
        assert_eq!(plan.payload["style_target"], "print_context");
        assert_eq!(plan.payload["layer_name"], "contours_index");
        assert_eq!(plan.payload["create_only"], true);
        assert_eq!(plan.receipt["createOnly"], true);
        assert!(plan.payload.get("project_id").is_none());

        assert!(plan_style_edit(
            &snapshot(json!({"type":"line"})),
            "master/poles",
            &StyleInstruction::Seed
        )
        .unwrap_err()
        .contains("limited to declared print context"));
    }
    #[test]
    fn guided_appearance_changes_only_size_fallback_and_checks_server_bounds() {
        let state = snapshot(
            json!({"type":"circle","paint":{"circle-radius":["match",["get","status"],"built",8,4]}}),
        );
        let edit = StyleInstruction::Appearance {
            color: None,
            icon: None,
            size: Some(6.),
        };
        let plan = plan_style_edit(&state, "master/poles", &edit).unwrap();
        assert_eq!(plan.document["paint"]["circle-radius"][3], 8);
        assert_eq!(
            plan.document["paint"]["circle-radius"][4].as_f64(),
            Some(6.)
        );
        assert_eq!(plan.receipt["baseSizeUpdatedAsFallback"], true);
        assert!(plan_style_edit(
            &state,
            "master/poles",
            &StyleInstruction::Appearance {
                color: None,
                icon: None,
                size: Some(100.)
            }
        )
        .is_err());
        assert!(plan_style_edit(&state, "other", &edit).is_err());
    }
    #[test]
    fn typed_cartography_grammar_has_no_arbitrary_document_keys() {
        let request: StyleInstruction = serde_json::from_value(
            json!({"kind":"cartography","lineType":"directional","directionSize":20}),
        )
        .unwrap();
        let state = snapshot(json!({"type":"line","paint":{"line-width":3}}));
        let plan = plan_style_edit(&state, "master/poles", &request).unwrap();
        assert_eq!(plan.document["metadata"]["line_marker"], "arrow");
        assert!(serde_json::from_value::<StyleInstruction>(
            json!({"kind":"cartography","paint":{"anything":true}})
        )
        .is_err());
    }
    #[test]
    fn runtime_icon_normalization_preserves_nested_choices() {
        let expr = json!([
            "match",
            ["get", "status"],
            "a",
            "baked-a",
            ["match", ["get", "kind"], "x", "baked-b", "baked-c"]
        ]);
        let images = vec![
            json!({"id":"baked-a","source":"school"}),
            json!({"id":"baked-b","source":"hospital"}),
            json!({"id":"baked-c","source":"town"}),
        ];
        let normalized = normalize_icon(&expr, &images);
        assert!(normalized.is_array());
        assert_eq!(normalized[3], "school");
        assert!(normalized[4].is_array());
    }
}
