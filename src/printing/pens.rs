//! Template-local pens: the print layers a layout binds to governed print
//! styles, what each prints with, and the bounded edits an editor makes to
//! them. A host's pen panel shows these rows and forwards one choice per
//! input; it derives nothing from a style document itself.
//!
//! The governed `_print` document stays the base. An override
//! (`Layout::style_overrides`, applied by `style_overrides::apply` at render
//! and read by the legend) changes colour, size, opacity, visibility and label
//! treatment for this template only; a delivery and a preview consume the same
//! override, so what the editor shows is what the sheet prints.
use super::{Layout, PrintLabelOverride, PrintStyleOverride};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// One resolved governed print document, as the host holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PenDocument {
    pub style_ref: String,
    pub document: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PenGroup {
    /// The transformer's own design layers (`master/…` print styles).
    Design,
    /// Everything printed around the design: catalogue, models, buildings.
    Context,
}

/// What a pen prints with, before or after its override.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct PenValues {
    /// A flat colour; `None` when the document colours by category.
    pub color: Option<String>,
    pub size: Option<f64>,
    pub opacity: Option<f64>,
    pub visible: bool,
}

/// One category of a data-driven colour, from the document's `match`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PenCategory {
    pub value: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Pen {
    pub layer_id: String,
    pub label: String,
    pub style_ref: String,
    pub group: PenGroup,
    /// `line`, `circle`, `fill`, `symbol` — or `None` when the host holds no
    /// document for the reference, in which case nothing can be shown.
    pub geometry: Option<String>,
    pub held: bool,
    /// The unit a size is authored in, read from the document's print
    /// scaling: `css_px` at its reference paper, or `mm`.
    pub size_unit: String,
    pub base: PenValues,
    pub effective: PenValues,
    #[serde(rename = "override")]
    pub override_: Option<PrintStyleOverride>,
    /// The document colours by a category field; `categories` lists them.
    pub categorical: bool,
    pub category_field: Option<String>,
    pub categories: Vec<PenCategory>,
    /// The document carries a governed label; label overrides apply.
    pub labelled: bool,
    pub label_visible: bool,
}

/// The properties a pen panel edits, one per input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PenProperty {
    Color,
    Size,
    Opacity,
    Visible,
    LabelVisible,
    LabelSizePt,
    LabelColor,
    LabelHaloMm,
    LabelHaloColor,
}

const MAX_CATEGORIES: usize = 48;

fn paint_keys(kind: &str) -> Option<(&'static str, &'static str, &'static str, &'static str)> {
    // (colour, size, opacity, size group)
    match kind {
        "line" => Some(("line-color", "line-width", "line-opacity", "paint")),
        "fill" => Some(("fill-color", "", "fill-opacity", "paint")),
        "circle" => Some(("circle-color", "circle-radius", "circle-opacity", "paint")),
        "symbol" => Some(("icon-color", "icon-size", "icon-opacity", "layout")),
        _ => None,
    }
}

/// The category table of a `["match", ["get", field], v, c, …, fallback]`
/// colour, with its fallback as the flat colour when it is one.
fn categories(expression: &Value) -> (Option<String>, Option<String>, Vec<PenCategory>) {
    let Some(items) = expression.as_array() else {
        return (None, None, Vec::new());
    };
    if items.first().and_then(Value::as_str) != Some("match") || items.len() < 3 {
        return (None, None, Vec::new());
    }
    let field = items[1]
        .as_array()
        .filter(|get| get.first().and_then(Value::as_str) == Some("get"))
        .and_then(|get| get.get(1))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let body = &items[2..];
    let (pairs, fallback) = if body.len() % 2 == 1 {
        (&body[..body.len() - 1], body.last())
    } else {
        (body, None)
    };
    let mut rows = Vec::new();
    for pair in pairs.chunks(2) {
        let Some(color) = pair[1].as_str() else {
            continue;
        };
        // A label may be one value or a list of values sharing a colour.
        let values: Vec<String> = match &pair[0] {
            Value::Array(many) => many
                .iter()
                .filter_map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .or_else(|| v.as_f64().map(|n| n.to_string()))
                })
                .collect(),
            Value::String(one) => vec![one.clone()],
            Value::Number(n) => vec![n.to_string()],
            _ => Vec::new(),
        };
        for value in values {
            if rows.len() == MAX_CATEGORIES {
                break;
            }
            rows.push(PenCategory {
                value,
                color: color.to_owned(),
            });
        }
    }
    (
        field,
        fallback.and_then(Value::as_str).map(str::to_owned),
        rows,
    )
}

fn document_values(
    document: &Value,
    kind: &str,
) -> (PenValues, bool, Option<String>, Vec<PenCategory>) {
    let Some((color_key, size_key, opacity_key, size_group)) = paint_keys(kind) else {
        return (PenValues::default(), false, None, Vec::new());
    };
    let paint = &document["paint"];
    let (categorical, category_field, color, rows) = match &paint[color_key] {
        Value::String(flat) => (false, None, Some(flat.clone()), Vec::new()),
        expression @ Value::Array(_) => {
            let (field, fallback, rows) = categories(expression);
            (true, field, fallback, rows)
        }
        _ => (false, None, None, Vec::new()),
    };
    let size = if size_key.is_empty() {
        None
    } else {
        document[size_group][size_key].as_f64()
    };
    let opacity = paint[opacity_key].as_f64().or(Some(1.));
    let visible = document["layout"]["visibility"].as_str() != Some("none");
    (
        PenValues {
            color,
            size,
            opacity,
            visible,
        },
        categorical,
        category_field,
        rows,
    )
}

fn size_unit(document: &Value) -> String {
    let scaling = &document["metadata"]["print_scaling"];
    match (
        scaling["units"].as_str(),
        scaling["reference_paper"].as_str(),
    ) {
        (Some(units), Some(paper)) => format!("{units} @ {paper}"),
        (Some(units), None) => units.to_owned(),
        _ => "mm".into(),
    }
}

/// `lv_lines` → `LV lines`; `buildings_context` → `Buildings context`.
pub fn label(layer_id: &str) -> String {
    if layer_id == "tr" {
        return "Transformer".into();
    }
    let mut words = Vec::new();
    for (index, word) in layer_id.split('_').filter(|w| !w.is_empty()).enumerate() {
        let word = match word {
            "lv" | "mv" | "hv" | "mv2" | "id" | "upi" => word.to_ascii_uppercase(),
            _ if index == 0 => {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
            _ => word.to_owned(),
        };
        words.push(word);
    }
    words.join(" ")
}

/// Design layers print in engineering order; the rest follow by label.
const DESIGN_ORDER: &[&str] = &[
    "lv_lines",
    "non_snapping_lv_lines",
    "spans",
    "service_cables",
    "lv_poles",
    "tapping_poles",
    "dropped_lv_poles",
    "customers",
    "drafting_errors",
];

fn group(style_ref: &str) -> PenGroup {
    if style_ref.starts_with("master/") {
        PenGroup::Design
    } else {
        PenGroup::Context
    }
}

/// Every pen of the layout: one row per bound print layer, base values from
/// the document the host holds, effective values with the override applied.
pub fn pens(layout: &Layout, documents: &[PenDocument]) -> Result<Vec<Pen>, String> {
    super::validate(layout)?;
    if documents.len() > 500 {
        return Err("pen documents exceed their bound".into());
    }
    let held: BTreeMap<&str, &Value> = documents
        .iter()
        .map(|d| (d.style_ref.as_str(), &d.document))
        .collect();
    let context_labels: BTreeMap<&str, &str> = layout
        .context_layers
        .iter()
        .map(|c| (c.id.as_str(), c.label.as_str()))
        .collect();
    let mut rows = Vec::with_capacity(layout.style_refs.len());
    for (layer_id, style_ref) in &layout.style_refs {
        let document = held.get(style_ref.as_str()).copied();
        let kind = document.and_then(|d| d["type"].as_str()).map(str::to_owned);
        let (base, categorical, category_field, categories) = match (document, kind.as_deref()) {
            (Some(document), Some(kind)) => document_values(document, kind),
            _ => (PenValues::default(), false, None, Vec::new()),
        };
        let override_ = layout.style_overrides.get(layer_id).cloned();
        let effective = match &override_ {
            None => base.clone(),
            Some(change) => PenValues {
                color: change.color.clone().or_else(|| base.color.clone()),
                size: change.size.or(base.size),
                opacity: change.opacity.or(base.opacity),
                visible: change.visible.unwrap_or(base.visible),
            },
        };
        let labelled = document.is_some_and(|d| d["label"].is_object());
        let label_visible = override_
            .as_ref()
            .and_then(|o| o.label.as_ref())
            .and_then(|l| l.visible)
            .unwrap_or_else(|| {
                labelled
                    && document.is_some_and(|d| {
                        d["label"]["layout"]["visibility"].as_str() != Some("none")
                    })
            });
        rows.push(Pen {
            layer_id: layer_id.clone(),
            label: context_labels
                .get(layer_id.as_str())
                .map(|l| (*l).to_owned())
                .unwrap_or_else(|| label(layer_id)),
            style_ref: style_ref.clone(),
            group: group(style_ref),
            geometry: kind,
            held: document.is_some(),
            size_unit: document.map(size_unit).unwrap_or_else(|| "mm".into()),
            base,
            effective,
            override_,
            categorical,
            category_field,
            categories,
            labelled,
            label_visible,
        });
    }
    rows.sort_by_key(|pen| {
        let rank = DESIGN_ORDER
            .iter()
            .position(|id| *id == pen.layer_id)
            .unwrap_or(DESIGN_ORDER.len());
        (
            pen.group == PenGroup::Context,
            rank,
            pen.label.to_lowercase(),
            pen.layer_id.clone(),
        )
    });
    Ok(rows)
}

fn colour(value: &Value) -> Result<Option<String>, String> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => crate::style_authoring::normalize_style_color(text)
            .map(Some)
            .ok_or_else(|| format!("{text:?} is not a #RRGGBB or #RRGGBBAA colour")),
        _ => Err("a colour is a string".into()),
    }
}

fn number(value: &Value, min: f64, max: f64, what: &str) -> Result<Option<f64>, String> {
    match value {
        Value::Null => Ok(None),
        Value::Number(n) => {
            let n = n.as_f64().ok_or("a number is finite")?;
            if !n.is_finite() || n < min || n > max {
                return Err(format!("{what} must be between {min} and {max}"));
            }
            Ok(Some(n))
        }
        _ => Err(format!("{what} is a number")),
    }
}

fn flag(value: &Value) -> Result<Option<bool>, String> {
    match value {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(*b)),
        _ => Err("a switch is true or false".into()),
    }
}

/// Set (or, with `null`, clear) one property of a layer's pen. An override
/// that no longer changes anything is removed, so an untouched template
/// serialises exactly as before.
pub fn edit(
    mut layout: Layout,
    layer_id: &str,
    property: PenProperty,
    value: &Value,
) -> Result<Layout, String> {
    if !layout.style_refs.contains_key(layer_id) {
        return Err(format!(
            "printing_pen_unbound: `{layer_id}` is not bound to a governed print style; bind one first"
        ));
    }
    let mut change = layout.style_overrides.remove(layer_id).unwrap_or_default();
    match property {
        PenProperty::Color => change.color = colour(value)?,
        PenProperty::Size => {
            change.size = number(value, 0., 100., "size")?;
            if change.size.is_none() {
                // A weight policy needs its base size; clearing one clears both.
                change.scale_weight = None;
            }
        }
        PenProperty::Opacity => change.opacity = number(value, 0., 1., "opacity")?,
        PenProperty::Visible => change.visible = flag(value)?,
        PenProperty::LabelVisible
        | PenProperty::LabelSizePt
        | PenProperty::LabelColor
        | PenProperty::LabelHaloMm
        | PenProperty::LabelHaloColor => {
            let mut label = change.label.take().unwrap_or_default();
            match property {
                PenProperty::LabelVisible => label.visible = flag(value)?,
                PenProperty::LabelSizePt => label.size_pt = number(value, 1., 60., "label size")?,
                PenProperty::LabelColor => label.color = colour(value)?,
                PenProperty::LabelHaloMm => label.halo_mm = number(value, 0., 5., "label halo")?,
                PenProperty::LabelHaloColor => label.halo_color = colour(value)?,
                _ => unreachable!(),
            }
            change.label = (label != PrintLabelOverride::default()).then_some(label);
        }
    }
    if change != PrintStyleOverride::default() {
        layout.style_overrides.insert(layer_id.to_owned(), change);
    }
    super::validate(&layout)?;
    Ok(layout)
}

/// Replace (or, with `None`, remove) a layer's whole override.
pub fn set_override(
    mut layout: Layout,
    layer_id: &str,
    change: Option<PrintStyleOverride>,
) -> Result<Layout, String> {
    match change {
        Some(change) if change != PrintStyleOverride::default() => {
            if !layout.style_refs.contains_key(layer_id) {
                return Err(format!(
                    "printing_pen_unbound: `{layer_id}` is not bound to a governed print style; bind one first"
                ));
            }
            layout.style_overrides.insert(layer_id.to_owned(), change);
        }
        _ => {
            layout.style_overrides.remove(layer_id);
        }
    }
    super::validate(&layout)?;
    Ok(layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn layout() -> Layout {
        let mut layout = super::super::default_layout();
        layout
            .style_refs
            .insert("lv_lines".into(), "master/lv_lines_print".into());
        layout
            .style_refs
            .insert("lv_poles".into(), "master/lv_poles_print".into());
        layout
            .style_refs
            .insert("roads".into(), "gt/roads_print".into());
        layout
    }

    fn documents() -> Vec<PenDocument> {
        vec![
            PenDocument {
                style_ref: "master/lv_lines_print".into(),
                document: json!({
                    "type":"line",
                    "paint":{"line-color":["match",["get","cable_size"],"3x25","#22D3EE",["3x35","3x50"],"#EBE424","#0EA5E9"],"line-width":2.3,"line-opacity":0.9},
                    "layout":{"visibility":"visible"},
                    "label":{"type":"symbol","layout":{"visibility":"none","text-field":["get","cable_size"]}},
                    "metadata":{"print_scaling":{"reference_paper":"A3","units":"css_px","factors":{"A0":2.83,"A3":1}}}
                }),
            },
            PenDocument {
                style_ref: "gt/roads_print".into(),
                document: json!({"type":"line","paint":{"line-color":"#888888","line-width":0.6},"layout":{"visibility":"none"}}),
            },
        ]
    }

    #[test]
    fn pens_list_every_bound_layer_design_first_with_held_facts() {
        let rows = pens(&layout(), &documents()).unwrap();
        let ids: Vec<&str> = rows.iter().map(|p| p.layer_id.as_str()).collect();
        assert_eq!(ids, ["lv_lines", "lv_poles", "roads"]);
        let lines = &rows[0];
        assert_eq!(lines.label, "LV lines");
        assert_eq!(lines.group, PenGroup::Design);
        assert_eq!(lines.geometry.as_deref(), Some("line"));
        assert!(lines.held && lines.categorical && lines.labelled && !lines.label_visible);
        assert_eq!(lines.category_field.as_deref(), Some("cable_size"));
        assert_eq!(lines.categories.len(), 3);
        assert_eq!(lines.categories[1].value, "3x35");
        assert_eq!(lines.categories[2].color, "#EBE424");
        assert_eq!(lines.base.color.as_deref(), Some("#0EA5E9"));
        assert_eq!(lines.base.size, Some(2.3));
        assert_eq!(lines.size_unit, "css_px @ A3");
        let poles = &rows[1];
        assert!(!poles.held && poles.geometry.is_none());
        let roads = &rows[2];
        assert_eq!(roads.group, PenGroup::Context);
        assert!(!roads.base.visible && roads.base.opacity == Some(1.));
    }

    #[test]
    fn a_pen_edit_layers_over_the_document_and_an_empty_override_disappears() {
        let edited = edit(layout(), "lv_lines", PenProperty::Size, &json!(0.8)).unwrap();
        let edited = edit(edited, "lv_lines", PenProperty::Color, &json!("#ff0000")).unwrap();
        let edited = edit(edited, "roads", PenProperty::Visible, &json!(true)).unwrap();
        let edited = edit(edited, "lv_lines", PenProperty::LabelVisible, &json!(true)).unwrap();
        let rows = pens(&edited, &documents()).unwrap();
        assert_eq!(rows[0].effective.size, Some(0.8));
        assert_eq!(rows[0].effective.color.as_deref(), Some("#FF0000"));
        assert_eq!(rows[0].base.size, Some(2.3));
        assert!(rows[0].label_visible);
        assert!(rows[2].effective.visible && !rows[2].base.visible);
        let serialized = serde_json::to_value(&rows[0]).unwrap();
        assert_eq!(serialized["override"]["size"], json!(0.8));
        assert_eq!(serialized["override"]["label"]["visible"], json!(true));

        let cleared = edit(edited, "lv_lines", PenProperty::Size, &Value::Null).unwrap();
        let cleared = edit(cleared, "lv_lines", PenProperty::Color, &Value::Null).unwrap();
        let cleared = edit(cleared, "lv_lines", PenProperty::LabelVisible, &Value::Null).unwrap();
        assert!(!cleared.style_overrides.contains_key("lv_lines"));
        assert!(cleared.style_overrides.contains_key("roads"));
        let reset = set_override(cleared, "roads", None).unwrap();
        assert!(reset.style_overrides.is_empty());
    }

    #[test]
    fn pen_edits_refuse_what_the_layout_would_refuse() {
        assert!(edit(layout(), "rivers", PenProperty::Color, &json!("#ff0000")).is_err());
        assert!(edit(layout(), "lv_lines", PenProperty::Color, &json!("red")).is_err());
        assert!(edit(layout(), "lv_lines", PenProperty::Size, &json!(101)).is_err());
        assert!(edit(layout(), "lv_lines", PenProperty::Opacity, &json!(1.5)).is_err());
        assert!(edit(layout(), "lv_lines", PenProperty::LabelSizePt, &json!(0.2)).is_err());
        assert!(edit(layout(), "lv_lines", PenProperty::Visible, &json!("yes")).is_err());
        assert!(set_override(
            layout(),
            "rivers",
            Some(PrintStyleOverride {
                size: Some(1.),
                ..Default::default()
            })
        )
        .is_err());
    }

    #[test]
    fn labels_read_as_engineering_names() {
        assert_eq!(label("lv_poles"), "LV poles");
        assert_eq!(label("buildings_context"), "Buildings context");
        assert_eq!(label("dsgrid_mv_lines"), "Dsgrid MV lines");
        assert_eq!(label("tr"), "Transformer");
    }
}
