//! Governed semantic paper pens shared by native rendering and WASM preview.
use super::{
    self as printing,
    standards::{self, InkMode},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const CATEGORY_COLOURS: [&str; 12] = [
    "#173f69", "#b43a26", "#246e43", "#743d8c", "#946600", "#147a89", "#c05b91", "#677a21",
    "#60452f", "#49549c", "#b26817", "#375c5a",
];
fn category_field(id: &str) -> &str {
    match id {
        "lv_lines" | "service_cables" => "cable_size",
        "lv_poles" => "struct_type",
        _ => "",
    }
}
/// Pin the project-wide vocabulary before preparing individual sheets. Pens
/// then agree on every sheet, regardless of which categories occur locally.
pub fn seed_category_palette(
    layout: &mut printing::Layout,
    layers: &[VectorLayer],
) -> Result<(), String> {
    let mut vocabularies = BTreeMap::<String, BTreeSet<String>>::new();
    for layer in layers {
        let field = category_field(&layer.id);
        if field.is_empty() {
            continue;
        }
        for feature in features(&layer.geojson) {
            let value = feature["properties"][field]
                .as_str()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or("Unspecified")
                .to_owned();
            vocabularies
                .entry(layer.id.clone())
                .or_default()
                .insert(value);
        }
    }
    for (id, categories) in vocabularies {
        if categories.len() > 32 {
            return Err("print_palette_invalid: more than 32 categories in one paper layer".into());
        }
        let pen = layout.styles.entry(id).or_default();
        if pen.colors.is_empty() {
            pen.colors = categories
                .into_iter()
                .enumerate()
                .map(|(i, value)| (value, CATEGORY_COLOURS[i % CATEGORY_COLOURS.len()].into()))
                .collect();
        }
    }
    Ok(())
}
fn category_dash(index: usize, existing: bool) -> Vec<f64> {
    if index == 0 && !existing {
        return vec![];
    }
    // Sixteen short/long sequences plus a lifecycle gap. Unlike hashing into
    // four pens, distinct supplied cable sizes cannot silently share a pen.
    vec![
        2. + (index % 8) as f64 * 0.6,
        if existing { 1.8 } else { 0.8 },
        0.7 + (index / 8) as f64 * 0.5,
        0.8,
    ]
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VectorLayer {
    pub id: String,
    pub label: String,
    pub geojson: Value,
}
fn features(layer: &Value) -> Vec<Value> {
    layer["features"].as_array().cloned().unwrap_or_default()
}
fn vector(id: &str, label: &str, features: Vec<Value>) -> VectorLayer {
    VectorLayer {
        id: id.into(),
        label: label.into(),
        geojson: json!({"type":"FeatureCollection","features":features}),
    }
}
/// Materialise complete semantic pens so preview and PDF use the exact same
/// styles. Original focused layers remain table-only, never relabelled context.
pub fn materialize(
    layout: &mut super::Layout,
    layers: Vec<VectorLayer>,
) -> Result<Vec<VectorLayer>, String> {
    seed_category_palette(layout, &layers)?;
    let originals = layers;
    let mut result = Vec::new();
    let mut legend_ids = Vec::new();
    for layer in originals {
        let id = layer.id.as_str();
        if !matches!(
            id,
            "tr" | "lv_lines"
                | "lv_poles"
                | "customers"
                | "service_cables"
                | "tapping_poles"
                | "neighbor_transformers"
                | "neighbor_lv_lines"
                | "neighbor_lv_poles"
                | "neighbor_customers"
                | "neighbor_service_cables"
                | "mv_lines"
                | "mv_poles"
                | "existing_mv_lines"
                | "existing_mv_poles"
                | "contours_index"
                | "contours_intermediate"
                | "buildings"
                | "roads"
                | "rivers"
        ) {
            let mut table = layer;
            for f in table.geojson["features"]
                .as_array_mut()
                .into_iter()
                .flatten()
            {
                f["geometry"] = Value::Null;
            }
            result.push(table);
            continue;
        }
        let contextual = id.starts_with("neighbor_");
        let design = matches!(
            id,
            "tr" | "lv_lines" | "lv_poles" | "customers" | "service_cables" | "tapping_poles"
        );
        let category_field = category_field(id);
        let mut groups = BTreeMap::<(String, String), Vec<Value>>::new();
        for feature in features(&layer.geojson) {
            let existing = feature["properties"]["existing_status"]
                .as_str()
                .unwrap_or("unclassified")
                .to_ascii_lowercase();
            let status = if id.starts_with("existing_mv") {
                "existing".into()
            } else if matches!(existing.as_str(), "existing" | "new") {
                existing
            } else {
                "unclassified".into()
            };
            let category = feature["properties"][category_field]
                .as_str()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or(if category_field.is_empty() {
                    ""
                } else {
                    "Unspecified"
                })
                .to_string();
            groups.entry((status, category)).or_default().push(feature);
        }
        for (index, ((status, category), mut fs)) in groups.into_iter().enumerate() {
            let target = format!("paper-{}-{index}", id.replace('_', "-"));
            let is_line = fs.iter().any(|f| {
                f["geometry"]["type"]
                    .as_str()
                    .is_some_and(|k| k.contains("Line"))
            });
            let existing = status == "existing";
            let mut style = printing::PrintStyle {
                stroke: if existing { "#666666" } else { "#173f69" }.into(),
                width_mm: if existing { 0.3 } else { 0.6 },
                fill: if existing { "#ffffff" } else { "#173f69" }.into(),
                radius_mm: if design { 1.1 } else { 0.8 },
                dash_mm: if existing && is_line {
                    vec![2., 1.2]
                } else {
                    vec![]
                },
                label_pt: if layout.paper.size == printing::Paper::A0 {
                    9.
                } else {
                    7.
                },
                ..Default::default()
            };
            let category_index = layout
                .styles
                .get(id)
                .and_then(|s| s.colors.keys().position(|v| v == &category));
            if !category_field.is_empty() && !existing {
                style.stroke = layout.styles[id].colors[&category].clone();
                style.fill = style.stroke.clone();
            }
            match id {
                "mv_lines" | "mv_poles" => {
                    style.stroke = if existing { "#59616b" } else { "#9c3124" }.into();
                    style.fill = if existing {
                        "#ffffff".into()
                    } else {
                        style.stroke.clone()
                    };
                    style.width_mm = if existing { 0.35 } else { 0.75 };
                    style.dash_mm = if existing { vec![3., 1.5] } else { vec![] };
                }
                "existing_mv_lines" | "existing_mv_poles" => {
                    style.stroke = "#59616b".into();
                    style.width_mm = 0.35;
                    style.dash_mm = vec![3., 1.5];
                }
                "buildings" => {
                    style.stroke = "#658b61".into();
                    style.fill = "#bad2ad".into();
                    style.width_mm = 0.18;
                    style.fill_opacity = 0.5;
                }
                "contours_index" => {
                    style.stroke = "#8e745d".into();
                    style.width_mm = 0.3;
                    style.fill = "none".into();
                    style.label_field = "contour_label".into();
                    style.label_pt = 8.;
                }
                "contours_intermediate" => {
                    style.stroke = "#b09a83".into();
                    style.width_mm = 0.18;
                    style.fill = "none".into();
                }
                "roads" => {
                    style.stroke = "#999999".into();
                    style.width_mm = 0.4;
                }
                "rivers" => {
                    style.stroke = "#5686a1".into();
                    style.width_mm = 0.35;
                }
                _ => {}
            }
            if design {
                style.label_field = match id {
                    "tr" => "transfo",
                    "lv_poles" | "tapping_poles" => "pole_number",
                    "customers" => "house_number",
                    _ => "",
                }
                .into();
            }
            if contextual {
                style.stroke = if existing { "#888888" } else { "#758ca0" }.into();
                style.fill = style.stroke.clone();
                style.width_mm = 0.22;
                style.stroke_opacity = 0.45;
                style.fill_opacity = 0.45;
                style.label_field.clear();
            }
            if layout.ink_mode == InkMode::Monochrome {
                standards::monochrome_pen(&mut style, &fs[0]["properties"], None);
            }
            if let Some(index) = category_index {
                if is_line
                    && (layout.ink_mode == InkMode::Monochrome
                        || existing
                        || index >= CATEGORY_COLOURS.len())
                {
                    style.dash_mm = category_dash(index, existing);
                }
            }
            let label = if design || contextual || id.contains("mv_") {
                format!(
                    "{} {}{}{}",
                    if contextual {
                        "Context"
                    } else {
                        match status.as_str() {
                            "new" => "New",
                            "existing" => "Existing",
                            _ => "Unclassified",
                        }
                    },
                    id.trim_start_matches("neighbor_").replace('_', " "),
                    if category.is_empty() { "" } else { " · " },
                    category
                )
            } else {
                layer.label.replace('_', " ")
            };
            if id == "tr" {
                for f in &mut fs {
                    if let Some(v) = f["properties"]["transfo"].as_str() {
                        f["properties"]["transfo"] = json!(printing::names::transformer_title(v));
                    }
                }
            }
            layout.styles.insert(target.clone(), style);
            result.push(vector(&target, &label, fs));
            layout.layer_order.push(target.clone());
            legend_ids.push(target);
        }
        if design {
            let mut table = layer;
            for f in table.geojson["features"]
                .as_array_mut()
                .into_iter()
                .flatten()
            {
                f["geometry"] = Value::Null;
            }
            result.push(table);
        }
    }
    // Context precedes the focused circuits, irrespective of source ordering.
    layout.layer_order.sort_by_key(|id| {
        if id.starts_with("paper-neighbor") {
            0
        } else if id.starts_with("paper-lv-")
            || id.starts_with("paper-service")
            || id.starts_with("paper-tr-")
            || id.starts_with("paper-customers")
        {
            2
        } else {
            1
        }
    });
    for e in &mut layout.elements {
        if let Some(binding) = e.legend.as_mut() {
            binding.layers = legend_ids.clone();
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn four_cable_sizes_never_share_a_colour_or_monochrome_pen() {
        let sizes = ["4 x 16 mm2", "4 x 25 mm2", "4 x 35 mm2", "4 x 50 mm2"];
        let layer = vector("lv_lines", "LV", sizes.iter().map(|size| json!({"type":"Feature","geometry":{"type":"LineString","coordinates":[[30.,-2.],[30.01,-2.]]},"properties":{"existing_status":"new","cable_size":size}})).collect());
        for mode in [InkMode::Colour, InkMode::Monochrome] {
            let mut layout = printing::default_layout();
            layout.ink_mode = mode;
            let pens = materialize(&mut layout, vec![layer.clone()]).unwrap();
            let signatures = pens
                .iter()
                .filter(|l| l.id.starts_with("paper-"))
                .map(|l| {
                    serde_json::to_string(&(
                        &layout.styles[&l.id].stroke,
                        &layout.styles[&l.id].dash_mm,
                    ))
                    .unwrap()
                })
                .collect::<BTreeSet<_>>();
            assert_eq!(signatures.len(), sizes.len());
        }
    }
    #[test]
    fn paper_theme_separates_mv_lifecycle_and_keeps_buildings_green() {
        let mut layout = printing::default_layout();
        let f = |kind: &str, status: &str| json!({"type":"Feature","geometry":{"type":kind,"coordinates":if kind=="Point"{json!([30.,-2.])}else if kind=="Polygon"{json!([[[30.,-2.],[30.01,-2.],[30.01,-2.01],[30.,-2.]]])}else{json!([[30.,-2.],[30.01,-2.]])}},"properties":{"existing_status":status}});
        let layers = materialize(
            &mut layout,
            vec![
                vector(
                    "mv_lines",
                    "MV lines",
                    vec![f("LineString", "new"), f("LineString", "existing")],
                ),
                vector("buildings", "Buildings", vec![f("Polygon", "")]),
            ],
        )
        .unwrap();
        let new = layers.iter().find(|l| l.label.starts_with("New")).unwrap();
        let old = layers
            .iter()
            .find(|l| l.label.starts_with("Existing"))
            .unwrap();
        assert_ne!(layout.styles[&new.id].stroke, layout.styles[&old.id].stroke);
        assert!(layout.styles[&new.id].width_mm > layout.styles[&old.id].width_mm);
        assert_ne!(
            layout.styles[&new.id].dash_mm,
            layout.styles[&old.id].dash_mm
        );
        assert_eq!(layout.styles["paper-buildings-0"].stroke, "#658b61");
    }
    #[test]
    fn unspecified_categories_are_named_and_context_has_no_labels() {
        let mut layout = printing::default_layout();
        let feature = json!({"type":"Feature","geometry":{"type":"Point","coordinates":[30.,-2.]},"properties":{}});
        let layers = materialize(
            &mut layout,
            vec![
                vector("lv_poles", "Poles", vec![feature.clone()]),
                vector("neighbor_transformers", "Neighbours", vec![feature]),
            ],
        )
        .unwrap();
        assert!(layers.iter().any(|l| l.label.ends_with("Unspecified")));
        assert!(
            layout.styles["paper-neighbor-transformers-0"]
                .label_field
                .is_empty()
        );
    }
}
