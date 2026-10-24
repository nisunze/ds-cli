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
    /// One transformer's value key where the project layout carries the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_key: Option<super::table_values::ValueKey>,
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
/// Read the same explicit project overrides consumed by capture and rendering.
pub fn project_overrides(sheets: &serde_json::Value) -> Result<TransformerOverrides, String> {
    let rows: Vec<_> = sheets["project_settings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["parameter"] == "printing_overrides")
        .collect();
    if rows.len() > 1 {
        return Err("Project settings contain duplicate printing_overrides parameters.".into());
    }
    serde_json::from_value(
        sheets
            .get("printing_overrides")
            .cloned()
            .or_else(|| rows.first().map(|row| row["value"].clone()))
            .unwrap_or_else(|| serde_json::json!({})),
    )
    .map_err(|e| e.to_string())
}
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
            if let Some(key) = &table.value_key {
                binding.value_key = Some(key.clone());
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

#[cfg(test)]
mod scoped_tests {
    use super::*;
    #[test]
    fn saved_scoped_title_is_revision_content_and_cannot_change_a_peer() {
        let mut layout = super::super::default_layout();
        let patch: PrintOverride = serde_json::from_value(
            serde_json::json!({"elements":{"title":{"text":"Zihari · Version 1"}}}),
        )
        .unwrap();
        layout
            .transformer_overrides
            .insert("zihari".into(), patch.clone());
        super::super::validate(&layout).unwrap();
        let saved = serde_json::to_vec(&layout).unwrap();
        let restored: Layout = serde_json::from_slice(&saved).unwrap();
        let focused = apply(&restored, &restored.transformer_overrides["zihari"]).unwrap();
        assert_eq!(
            focused
                .elements
                .iter()
                .find(|e| e.id == "title")
                .unwrap()
                .text,
            "Zihari · Version 1"
        );
        assert_eq!(
            restored
                .elements
                .iter()
                .find(|e| e.id == "title")
                .unwrap()
                .text,
            "{title}"
        );
        assert!(!restored.transformer_overrides.contains_key("peer"));
        assert_eq!(restored, layout);
    }
}

/// The shared effective-layout precedence used by capture and final rendering.
pub fn effective(
    layout: &Layout,
    transformer: &str,
    project: &TransformerOverrides,
) -> Result<Layout, String> {
    let mut result = layout.clone();
    if let Some(patch) = layout.transformer_overrides.get(transformer) {
        result = apply(&result, patch)?;
    }
    if let Some(patch) = project
        .get(transformer)
        .and_then(|layouts| layouts.get(&layout.id))
    {
        result = apply(&result, patch)?;
    }
    Ok(result)
}
