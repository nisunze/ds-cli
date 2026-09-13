//! Per-transformer print instructions. The named project layout remains the
//! default; a bounded explicit patch changes only the selected transformer's
//! composition. Canonical layout/output identities cannot be overridden.
use super::{validate, ElementKind, Layout, PrintStyleOverride, TableOverflow};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TableOverride {
    pub panels: Option<usize>,
    pub row_mm: Option<f64>,
    pub overflow: Option<TableOverflow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_mode: Option<super::table_headings::HeadingMode>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ElementOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet_anchor: Option<super::sheet_margin::SheetAnchor>,
    pub rect: Option<[f64; 4]>,
    pub font_pt: Option<f64>,
    pub table: Option<TableOverride>,
    pub legend: Option<super::LegendBinding>,
    /// Replacement text for a text element — a transformer's own drafter,
    /// revision line or drawing number where the project layout carries the
    /// default. Bindings such as `{date}` still apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PrintOverride {
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub elements: BTreeMap<String, ElementOverride>,
    #[serde(default)]
    pub style_overrides: BTreeMap<String, PrintStyleOverride>,
}
pub type TransformerOverrides = BTreeMap<String, BTreeMap<String, PrintOverride>>;
pub fn apply(layout: &Layout, patch: &PrintOverride) -> Result<Layout, String> {
    if patch.note.len() > 2000 || patch.elements.len() > 64 || patch.style_overrides.len() > 64 {
        return Err("Transformer print overrides exceed their bound.".into());
    }
    let mut result = layout.clone();
    for (id, change) in &patch.elements {
        let element = result
            .elements
            .iter_mut()
            .find(|e| &e.id == id)
            .ok_or_else(|| format!("Transformer print override names absent element {id}."))?;
        if let Some(anchor) = change.sheet_anchor {
            element.sheet_anchor = anchor;
        }
        if let Some(rect) = change.rect {
            element.rect = rect;
        }
        if let Some(font) = change.font_pt {
            element.font_pt = font;
        }
        if let Some(text) = &change.text {
            if element.kind != ElementKind::Text {
                return Err(format!("Print element {id} is not a text element."));
            }
            if text.len() > 4000 {
                return Err(format!(
                    "Print element {id} override text exceeds 4000 bytes."
                ));
            }
            element.text = text.clone();
        }
        if let Some(legend) = &change.legend {
            if element.kind != ElementKind::Legend {
                return Err(format!("Print element {id} is not a legend."));
            }
            element.legend = Some(legend.clone());
        }
        if let Some(table) = &change.table {
            if element.kind != ElementKind::Table {
                return Err(format!("Print element {id} is not a table."));
            }
            let binding = element
                .table
                .as_mut()
                .ok_or("Print table has no binding.")?;
            if let Some(panels) = table.panels {
                binding.panels = panels;
            }
            if let Some(row_mm) = table.row_mm {
                binding.row_mm = Some(row_mm);
            }
            if let Some(mode) = table.heading_mode {
                binding.heading_mode = mode;
            }
            if let Some(overflow) = table.overflow {
                binding.overflow = overflow;
            }
        }
    }
    for (id, value) in &patch.style_overrides {
        result.style_overrides.insert(id.clone(), value.clone());
    }
    validate(&result)?;
    Ok(result)
}
pub fn validate_inventory(
    overrides: &TransformerOverrides,
    layouts: &BTreeMap<String, Layout>,
) -> Result<(), String> {
    if overrides.len() > 10_000 {
        return Err("Too many transformer print overrides.".into());
    }
    for (transformer, instructions) in overrides {
        if transformer.is_empty()
            || transformer.len() > 121
            || !transformer
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || instructions.len() > 32
        {
            return Err("Invalid transformer print override identity or layout count.".into());
        }
        for (id, patch) in instructions {
            apply(
                layouts.get(id).ok_or_else(|| {
                    format!("Transformer {transformer} override needs layout {id}.")
                })?,
                patch,
            )?;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn an_outlier_changes_only_its_explicit_elements_and_retains_canonical_identity() {
        let mut base = super::super::default_layout();
        let element: super::super::Element = serde_json::from_value(json!({"id":"schedule","kind":"table","rect":[10,10,150,99],"font_pt":7,"table":{"layer":"lv_print_info","columns":["Description"],"headings":["Description"],"widths":[1]}})).unwrap();
        base.elements.push(element);
        let original = base.clone();
        let patch: PrintOverride = serde_json::from_value(json!({"note":"Dense transformer schedule","elements":{"schedule":{"rect":[10,10,180,120],"table":{"overflow":"truncate_warn","panels":2}}}})).unwrap();
        let changed = apply(&base, &patch).unwrap();
        assert_eq!(base, original);
        assert_eq!(base.id, changed.id);
        assert_eq!(base.paper, changed.paper);
        assert_eq!(
            changed
                .elements
                .last()
                .unwrap()
                .table
                .as_ref()
                .unwrap()
                .overflow,
            TableOverflow::TruncateWarn
        );
        let bad: PrintOverride =
            serde_json::from_value(json!({"elements":{"absent":{"font_pt":8}}})).unwrap();
        assert!(apply(&base, &bad).unwrap_err().contains("absent"));
        assert!(serde_json::from_value::<PrintOverride>(json!({"id":"renamed"})).is_err());
        let bad: PrintOverride =
            serde_json::from_value(json!({"elements":{"schedule":{"rect":[10,10,1000,120]}}}))
                .unwrap();
        assert!(apply(&base, &bad).is_err());
    }

    #[test]
    fn a_transformer_may_replace_a_text_elements_words_but_nothing_else_with_text() {
        // The project layout carries the default drafter line; one transformer
        // names its own. Bindings inside the replacement stay bindings.
        let base = super::super::default_layout();
        let text_id = base
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Text)
            .map(|e| e.id.clone())
            .expect("the default layout has a text element");
        let patch: PrintOverride = serde_json::from_value(
            json!({"elements":{text_id.clone():{"text":"Drawn by J. Doe · {date}"}}}),
        )
        .unwrap();
        let changed = apply(&base, &patch).unwrap();
        let element = changed.elements.iter().find(|e| e.id == text_id).unwrap();
        assert_eq!(element.text, "Drawn by J. Doe · {date}");
        let map_id = base
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Map)
            .map(|e| e.id.clone())
            .unwrap();
        let bad: PrintOverride =
            serde_json::from_value(json!({"elements":{map_id:{"text":"nope"}}})).unwrap();
        assert!(apply(&base, &bad)
            .unwrap_err()
            .contains("not a text element"));
        let long: PrintOverride =
            serde_json::from_value(json!({"elements":{text_id:{"text":"x".repeat(4001)}}}))
                .unwrap();
        assert!(apply(&base, &long).unwrap_err().contains("4000"));
    }
}

/// Apply a bounded transformer patch to one named layout in project settings.
/// Other transformers, layouts and unrelated parameters retain their values.
pub fn update_settings(
    layout: &Layout,
    current: &[serde_json::Value],
    instructions: &BTreeMap<String, Option<PrintOverride>>,
) -> Result<Vec<serde_json::Value>, String> {
    if instructions.len() > 10_000 {
        return Err("Too many transformer print instructions.".into());
    }
    let mut next = current.to_vec();
    let positions: Vec<_> = next
        .iter()
        .enumerate()
        .filter(|(_, row)| row["parameter"] == "printing_overrides")
        .map(|(i, _)| i)
        .collect();
    if positions.len() > 1 {
        return Err("Project settings contain duplicate printing_overrides parameters.".into());
    }
    let mut inventory: TransformerOverrides = serde_json::from_value(
        positions
            .first()
            .map(|i| next[*i]["value"].clone())
            .unwrap_or_else(|| serde_json::json!({})),
    )
    .map_err(|e| format!("Invalid stored print instructions: {e}"))?;
    for (transformer, patch) in instructions {
        let check = BTreeMap::from([(transformer.clone(), BTreeMap::new())]);
        validate_inventory(&check, &BTreeMap::new())?;
        if let Some(patch) = patch {
            apply(layout, patch)?;
            inventory
                .entry(transformer.clone())
                .or_default()
                .insert(layout.id.clone(), patch.clone());
        } else if let Some(layouts) = inventory.get_mut(transformer) {
            layouts.remove(&layout.id);
            if layouts.is_empty() {
                inventory.remove(transformer);
            }
        }
    }
    if inventory.len() > 10_000 || inventory.values().any(|layouts| layouts.len() > 32) {
        return Err("Print instructions exceed their inventory bound.".into());
    }
    let value = serde_json::to_value(inventory).map_err(|e| e.to_string())?;
    if let Some(i) = positions.first() {
        next[*i]["value"] = value;
    } else {
        next.push(serde_json::json!({"parameter":"printing_overrides","value":value}));
    }
    Ok(next)
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn outlier_edits_and_removal_preserve_other_transformers_and_parameters() {
        let layout = super::super::default_layout();
        let original = vec![json!({"parameter":"design_export_format","value":["xlsx"]})];
        let patch = PrintOverride {
            note: "Outlier schedule".into(),
            ..Default::default()
        };
        let first = update_settings(
            &layout,
            &original,
            &BTreeMap::from([
                ("agatare".into(), Some(patch.clone())),
                ("akaderege_kigoma".into(), Some(patch)),
            ]),
        )
        .unwrap();
        assert_eq!(first[0], original[0]);
        assert_eq!(original.len(), 1);
        let second =
            update_settings(&layout, &first, &BTreeMap::from([("agatare".into(), None)])).unwrap();
        assert!(second[1]["value"].get("agatare").is_none());
        assert_eq!(
            second[1]["value"]["akaderege_kigoma"],
            first[1]["value"]["akaderege_kigoma"]
        );
        assert!(update_settings(
            &layout,
            &original,
            &BTreeMap::from([("../other".into(), None)])
        )
        .is_err());
    }
}
