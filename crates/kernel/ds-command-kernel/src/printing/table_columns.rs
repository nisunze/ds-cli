//! Plain schedule column selection shared by CLI, WASM and native rendering.
use super::{Element, Layout, TableBinding, TablePresentation, WorkbookSheet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SchedulePhase {
    #[default]
    Design,
    AsBuilt,
}

pub fn available_in_phase(key: &str, phase: SchedulePhase) -> bool {
    let key = key.to_ascii_lowercase();
    phase == SchedulePhase::AsBuilt
        || (!matches!(
            key.as_str(),
            "meter_number" | "meter_numbers" | "meter_numb" | "meter_type" | "nid"
        ) && !key.contains("phone"))
}

/// Rendering and authoring enforce the same optional schedule phase.
pub fn validate_phase(binding: &TableBinding) -> Result<(), String> {
    if let Some(phase) = binding.phase {
        if binding.workbook.is_some() || binding.presentation != TablePresentation::Plain {
            return Err("Schedule phase requires a plain engineering table".into());
        }
        if binding
            .columns
            .iter()
            .any(|key| !available_in_phase(key, phase))
        {
            return Err("Installed-connection columns require an as-built schedule".into());
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct Column {
    pub key: String,
    pub heading: String,
    pub selected: bool,
    pub available: bool,
    pub reason: Option<&'static str>,
}
#[derive(Debug, Serialize)]
pub struct Catalog {
    pub phase: SchedulePhase,
    pub columns: Vec<Column>,
}

fn schedule(element: &Element) -> Result<TableBinding, String> {
    let mut binding = element.table.clone().ok_or("Select a data table first")?;
    if binding.presentation == TablePresentation::WorkbookInfo {
        return Err("Network Summary retains its workbook presentation".into());
    }
    if let Some(workbook) = binding.workbook.take() {
        let (layer, companions) = match workbook.sheet {
            WorkbookSheet::Poles => ("lv_poles", vec!["tapping_poles".into()]),
            WorkbookSheet::Customers => ("customers", vec![]),
            WorkbookSheet::LvLines => ("lv_lines", vec![]),
            WorkbookSheet::ServiceCables => ("service_cables", vec![]),
        };
        binding.layer = layer.into();
        binding.additional_layers = companions;
        binding.phase = Some(if workbook.as_built {
            SchedulePhase::AsBuilt
        } else {
            SchedulePhase::Design
        });
    }
    binding.presentation = TablePresentation::Plain;
    Ok(binding)
}
fn table<'a>(layout: &'a Layout, id: &str) -> Result<&'a Element, String> {
    layout
        .elements
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("Unknown table {id}"))
}
fn heading(key: &str) -> String {
    let words = key.replace('_', " ");
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |c| {
        c.to_uppercase().collect::<String>() + chars.as_str()
    })
}
fn populated(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(s) => !s.trim().is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        _ => true,
    }
}

pub fn catalog(
    layout: &Layout,
    id: &str,
    layers: &BTreeMap<String, Value>,
) -> Result<Catalog, String> {
    super::validate(layout)?;
    let binding = schedule(table(layout, id)?)?;
    let phase = binding.phase.unwrap_or_default();
    let mut keys: BTreeMap<String, bool> = binding
        .columns
        .iter()
        .map(|key| (key.clone(), false))
        .collect();
    for id in std::iter::once(&binding.layer).chain(&binding.additional_layers) {
        let Some(layer) = layers.get(id) else {
            continue;
        };
        let features = layer["features"]
            .as_array()
            .ok_or("Schedule source must be a FeatureCollection")?;
        if features.len() > 1_000_000 {
            return Err("Schedule source exceeds one million rows".into());
        }
        for feature in features {
            if let Some(properties) = feature["properties"].as_object() {
                for (key, value) in properties {
                    if key.starts_with('_') {
                        continue;
                    }
                    if key.is_empty() || key.len() > 160 {
                        return Err("Invalid schedule property key".into());
                    }
                    *keys.entry(key.clone()).or_default() |= populated(value);
                    if keys.len() > 512 {
                        return Err("Schedule source exceeds 512 properties".into());
                    }
                }
            }
        }
    }
    let columns = keys
        .into_iter()
        .map(|(key, has_data)| {
            let allowed = available_in_phase(&key, phase);
            let index = binding.columns.iter().position(|c| c == &key);
            Column {
                heading: index
                    .and_then(|i| binding.headings.get(i))
                    .cloned()
                    .unwrap_or_else(|| heading(&key)),
                selected: index.is_some(),
                available: allowed && has_data,
                reason: if !allowed {
                    Some("as_built_only")
                } else if !has_data {
                    Some("no_data")
                } else {
                    None
                },
                key,
            }
        })
        .collect();
    Ok(Catalog { phase, columns })
}

fn replace_columns(binding: &mut TableBinding, columns: &[String]) {
    let previous = binding.clone();
    let indices: Vec<_> = columns
        .iter()
        .map(|key| previous.columns.iter().position(|c| c == key))
        .collect();
    binding.columns = columns.to_vec();
    if let Some(key) = binding.value_key.as_mut() {
        key.retain_bound(columns);
        // A chosen column list that the selection emptied is no choice any
        // more: the table's own default applies again.
        if key.mode == super::table_values::ValueKeyMode::Columns && key.columns.is_empty() {
            binding.value_key = None;
        }
    }
    binding.headings = columns
        .iter()
        .zip(&indices)
        .map(|(key, i)| {
            i.and_then(|i| previous.headings.get(i))
                .cloned()
                .unwrap_or_else(|| heading(key))
        })
        .collect();
    binding.widths = indices
        .iter()
        .map(|i| {
            i.and_then(|i| previous.widths.get(i))
                .copied()
                .unwrap_or(1.)
        })
        .collect();
    binding.min_widths_mm = if previous.min_widths_mm.is_empty() {
        vec![]
    } else {
        indices
            .iter()
            .map(|i| {
                i.and_then(|i| previous.min_widths_mm.get(i))
                    .copied()
                    .unwrap_or(3.)
            })
            .collect()
    };
    binding.max_widths_mm = if previous.max_widths_mm.is_empty() {
        vec![]
    } else {
        indices
            .iter()
            .map(|i| {
                i.and_then(|i| previous.max_widths_mm.get(i))
                    .copied()
                    .unwrap_or(60.)
            })
            .collect()
    };
}
pub fn select_columns(
    layout: &Layout,
    id: &str,
    columns: &[String],
    layers: &BTreeMap<String, Value>,
) -> Result<Layout, String> {
    let choices = catalog(layout, id, layers)?;
    let mut seen = BTreeSet::new();
    for key in columns {
        if !seen.insert(key) {
            return Err("Schedule columns must be unique".into());
        }
        let choice = choices
            .columns
            .iter()
            .find(|c| &c.key == key)
            .ok_or_else(|| format!("Schedule column {key} is unavailable"))?;
        if !available_in_phase(key, choices.phase) || (!choice.available && !choice.selected) {
            return Err(format!(
                "Schedule column {key} is unavailable for this phase or data"
            ));
        }
    }
    let mut result = layout.clone();
    let element = result
        .elements
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or("Unknown table")?;
    let mut binding = schedule(element)?;
    binding.phase = Some(choices.phase);
    replace_columns(&mut binding, columns);
    element.table = Some(binding);
    super::validate(&result)?;
    Ok(result)
}
pub fn set_phase(layout: &Layout, id: &str, phase: SchedulePhase) -> Result<Layout, String> {
    super::validate(layout)?;
    let mut result = layout.clone();
    let element = result
        .elements
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or("Unknown table")?;
    let mut binding = schedule(element)?;
    binding.phase = Some(phase);
    let columns = binding
        .columns
        .iter()
        .filter(|key| available_in_phase(key, phase))
        .cloned()
        .collect::<Vec<_>>();
    replace_columns(&mut binding, &columns);
    element.table = Some(binding);
    super::validate(&result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (Layout, BTreeMap<String, Value>) {
        let mut layout = super::super::default_layout();
        let mut element = super::super::elements::new_element(
            super::super::ElementKind::Table,
            "customers-table".into(),
            "",
        );
        element.table = Some(TableBinding {
            presentation: TablePresentation::Plain,
            layer: "customers".into(),
            columns: vec!["house_number".into(), "names".into()],
            headings: vec!["House".into(), "Customer".into()],
            widths: vec![2., 5.],
            min_widths_mm: vec![7., 10.],
            max_widths_mm: vec![17., 45.],
            ..Default::default()
        });
        layout.elements.push(element);
        let layers = BTreeMap::from([(
            "customers".into(),
            json!({"type":"FeatureCollection","features":[{"properties":{"house_number":0,"names":"Name","meter_number":"M-123","phone_number":"123","empty":" ","flag":false}}]}),
        )]);
        (layout, layers)
    }
    #[test]
    fn requests_expose_the_same_column_policy_and_validated_layout() {
        let (layout, layers) = fixture();
        let request = json!({"op":"table_columns","layout":layout,"element_id":"customers-table","layers":layers});
        let result: Value = serde_json::from_str(
            &super::super::evaluate(&serde_json::to_vec(&request).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(result["phase"], "design");
        assert!(result["columns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["key"] == "meter_number" && c["reason"] == "as_built_only"));
        let request = json!({"op":"table_column_selection","layout":layout,"element_id":"customers-table","layers":layers,"columns":["names"]});
        let result: Value = serde_json::from_str(
            &super::super::evaluate(&serde_json::to_vec(&request).unwrap()).unwrap(),
        )
        .unwrap();
        let selected: Layout = serde_json::from_value(result["layout"].clone()).unwrap();
        assert_eq!(
            selected
                .elements
                .last()
                .unwrap()
                .table
                .as_ref()
                .unwrap()
                .columns,
            ["names"]
        );
        super::super::validate(&selected).unwrap();
    }
    #[test]
    fn phase_and_real_values_determine_availability() {
        let (layout, layers) = fixture();
        let c = catalog(&layout, "customers-table", &layers).unwrap();
        assert!(
            c.columns
                .iter()
                .find(|c| c.key == "house_number")
                .unwrap()
                .available
        );
        assert!(
            c.columns
                .iter()
                .find(|c| c.key == "flag")
                .unwrap()
                .available
        );
        assert_eq!(
            c.columns
                .iter()
                .find(|c| c.key == "meter_number")
                .unwrap()
                .reason,
            Some("as_built_only")
        );
        assert_eq!(
            c.columns.iter().find(|c| c.key == "empty").unwrap().reason,
            Some("no_data")
        );
        let built = set_phase(&layout, "customers-table", SchedulePhase::AsBuilt).unwrap();
        assert!(
            catalog(&built, "customers-table", &layers)
                .unwrap()
                .columns
                .iter()
                .find(|c| c.key == "meter_number")
                .unwrap()
                .available
        );
    }
    #[test]
    fn visibility_retains_authored_column_geometry_and_phase_change_removes_installed_fields() {
        let (layout, layers) = fixture();
        assert!(select_columns(
            &layout,
            "customers-table",
            &["meter_number".into()],
            &layers
        )
        .is_err());
        let built = set_phase(&layout, "customers-table", SchedulePhase::AsBuilt).unwrap();
        let selected = select_columns(
            &built,
            "customers-table",
            &["names".into(), "meter_number".into()],
            &layers,
        )
        .unwrap();
        let b = selected.elements.last().unwrap().table.as_ref().unwrap();
        assert_eq!(b.headings, ["Customer", "Meter number"]);
        assert_eq!(b.widths, [5., 1.]);
        assert_eq!(b.max_widths_mm, [45., 60.]);
        let design = set_phase(&selected, "customers-table", SchedulePhase::Design).unwrap();
        assert_eq!(
            design
                .elements
                .last()
                .unwrap()
                .table
                .as_ref()
                .unwrap()
                .columns,
            ["names"]
        );
        assert!(select_columns(&design, "customers-table", &[], &layers).is_err());
        assert!(select_columns(
            &design,
            "customers-table",
            &["names".into(), "names".into()],
            &layers
        )
        .is_err());
    }
    #[test]
    fn phase_is_optional_for_legacy_layouts_and_native_validation_enforces_authored_phase() {
        let (mut layout, _) = fixture();
        let binding = layout.elements.last_mut().unwrap().table.as_mut().unwrap();
        binding.columns.push("meter_number".into());
        binding.headings.push("Meter".into());
        binding.widths.push(1.);
        binding.min_widths_mm.push(3.);
        binding.max_widths_mm.push(60.);
        super::super::validate(&layout).unwrap();
        layout
            .elements
            .last_mut()
            .unwrap()
            .table
            .as_mut()
            .unwrap()
            .phase = Some(SchedulePhase::Design);
        assert!(super::super::validate(&layout).is_err());
        layout
            .elements
            .last_mut()
            .unwrap()
            .table
            .as_mut()
            .unwrap()
            .phase = Some(SchedulePhase::AsBuilt);
        super::super::validate(&layout).unwrap();
        let encoded = serde_json::to_vec(&layout).unwrap();
        assert_eq!(serde_json::from_slice::<Layout>(&encoded).unwrap(), layout);
    }
    #[test]
    fn missing_values_cannot_be_added_but_existing_project_columns_can_be_retained() {
        let (layout, layers) = fixture();
        assert!(select_columns(
            &layout,
            "customers-table",
            &["house_number".into(), "empty".into()],
            &layers
        )
        .is_err());
        assert!(select_columns(
            &layout,
            "customers-table",
            &["names".into()],
            &BTreeMap::new()
        )
        .is_ok());
    }
}
