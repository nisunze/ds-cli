//! The project template's process rows, decided here for every host: whether
//! a run's settings find the configuration sheets they need, and the exact
//! `transformer_settings` / `project_settings` rows a settings surface writes
//! for a preset, its design sources and the default preset. Hosts save the
//! returned rows through the configuration owner; they never build them.
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Map, Value};

use super::{canonical_parameter_name, normalize, setting_value, sheet_rows, Preset2, Settings};

const TRANSFORMER_SETTINGS: &str = "transformer_settings";
const PROJECT_SETTINGS: &str = "project_settings";
const DEFAULT_PRESET: &str = "default_process_preset";
const CUSTOMER_LAYERS: &str = "customer_source_layers";
const POLE_LAYERS: &str = "pole_source_layers";
const INCLUDE_DESIGN_CUSTOMERS: &str = "include_design_customers";

pub const CONFIG_EMPTY: &str = "process_config_empty";
pub const CONFIG_MISSING: &str = "process_config_missing";

/// The typed verdict a surface renders before it starts a run: `ready` when
/// the settings find every sheet they need; `refusal` names the first reason
/// a surface that requires a configured project refuses (an empty
/// configuration, then missing sheets).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Readiness {
    pub has_config: bool,
    pub missing: Vec<&'static str>,
    pub ready: bool,
    pub refusal: Option<ReadinessRefusal>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReadinessRefusal {
    pub code: &'static str,
    pub message: String,
}

fn sheet_present(sheets: &Map<String, Value>, sheet: &str) -> bool {
    match sheets.get(sheet) {
        Some(Value::Array(rows)) => !rows.is_empty(),
        Some(Value::Object(sets)) if sets.is_empty() => false,
        // A rule-set-keyed sheet (lv_poles_rules) is present when any set has rows.
        Some(Value::Object(sets)) if sets.values().all(Value::is_array) => sets
            .values()
            .any(|rows| rows.as_array().is_some_and(|rows| !rows.is_empty())),
        Some(Value::Object(_)) => true,
        _ => false,
    }
}

/// Which configuration sheets `settings` need from this project's template.
pub fn readiness(settings: &Settings, project_config: Option<&Value>) -> Readiness {
    let empty = Map::new();
    let sheets = project_config
        .and_then(|config| config.get("sheets"))
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let normalized = normalize(settings);
    let is = |key: &str, value: bool| normalized.get(key) == Some(&Value::Bool(value));
    let present = |sheet: &str| sheet_present(sheets, sheet);
    let mut missing: Vec<&'static str> = Vec::new();
    let mut need = |sheet: &'static str| {
        if !missing.contains(&sheet) {
            missing.push(sheet);
        }
    };
    if is("delete_strange_cols", true) && !present("know_columns") {
        need("know_columns");
    }
    let spans_always_sized = !is("build_network", false);
    let sizing = spans_always_sized
        || is("size_transformer_kva", true)
        || is("size_spans", true)
        || (spans_always_sized && is("keep_lv_lines_sizes", false));
    if sizing || is("connect_unconnected_customers", true) {
        let required: &[&'static str] = if sizing {
            &[PROJECT_SETTINGS, "transfo_sizes"]
        } else {
            &[PROJECT_SETTINGS]
        };
        for sheet in required {
            if !present(sheet) {
                need(sheet);
            }
        }
    }
    if is("consider_cable_size_in_pole_placement", true) && !present("lv_lines") {
        need("lv_lines");
    }
    if spans_always_sized && is("keep_service_cables_sizes", false) {
        if !present("cust_category") && !present("customers_categories_info_df") {
            need("cust_category");
        }
        if !present("service_cable_sizes") {
            need("service_cable_sizes");
        }
    }
    if [
        "assign_assemblies",
        "assign_struct_types",
        "assign_earthing",
        "assign_stays",
    ]
    .iter()
    .any(|key| is(key, true))
        && !present("lv_poles_rules")
    {
        need("lv_poles_rules");
    }
    let has_config = !sheets.is_empty();
    let refusal = if !has_config {
        Some(ReadinessRefusal {
            code: CONFIG_EMPTY,
            message: "Project network config is required to process but came back empty.".into(),
        })
    } else if !missing.is_empty() {
        Some(ReadinessRefusal {
            code: CONFIG_MISSING,
            message: format!(
                "Project network config is missing {}; processing needs it for sizing and pole attribution.",
                missing.join(", ")
            ),
        })
    } else {
        None
    };
    Readiness {
        has_config,
        ready: missing.is_empty(),
        missing,
        refusal,
    }
}

fn preset_of(row: &Map<String, Value>) -> Option<Preset2> {
    row.get("preset")
        .and_then(Value::as_str)
        .and_then(|preset| Preset2::parse(&preset.trim().to_lowercase()))
}

fn row(parameter: &str, value: Value) -> Map<String, Value> {
    let mut row = Map::new();
    row.insert("parameter".into(), json!(parameter));
    row.insert("value".into(), value);
    row
}

/// `transformer_settings` with every row of `preset` replaced by `settings`
/// (the full map, so the sheet states what the preset resolves to). Other
/// rows keep their order; malformed rows (no string parameter) are not kept.
pub fn preset_rows(
    project_config: Option<&Value>,
    preset: Preset2,
    settings: &Settings,
) -> Vec<Map<String, Value>> {
    let mut rows: Vec<Map<String, Value>> = sheet_rows(project_config, TRANSFORMER_SETTINGS)
        .into_iter()
        .filter(|row| preset_of(row) != Some(preset))
        .collect();
    for (key, value) in settings {
        let parameter = canonical_parameter_name(key);
        let setting = value.is_boolean() || value.as_f64().is_some_and(f64::is_finite);
        if parameter.is_empty() || !setting {
            continue;
        }
        let mut added = row(&parameter, value.clone());
        added.insert("preset".into(), json!(preset));
        rows.push(added);
    }
    rows
}

/// `project_settings` with the declared default preset upserted in place.
pub fn default_preset_rows(
    project_config: Option<&Value>,
    preset: Preset2,
) -> Vec<Map<String, Value>> {
    let mut out = Vec::new();
    let mut replaced = false;
    for current in sheet_rows(project_config, PROJECT_SETTINGS) {
        let name = current
            .get("parameter")
            .and_then(Value::as_str)
            .unwrap_or("");
        if canonical_parameter_name(name) != DEFAULT_PRESET {
            out.push(current);
            continue;
        }
        if !replaced {
            let mut kept = current;
            kept.insert("parameter".into(), json!(DEFAULT_PRESET));
            kept.insert("value".into(), json!(preset));
            out.push(kept);
        }
        replaced = true;
    }
    if !replaced {
        out.push(row(DEFAULT_PRESET, json!(preset)));
    }
    out
}

/// A design-source declaration: a present member is written, an absent one
/// is left as it is. An explicit empty layer list stays a declaration.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceUpdate {
    #[serde(default)]
    pub customer_layers: Option<Vec<String>>,
    #[serde(default)]
    pub pole_layers: Option<Vec<String>>,
    #[serde(default, deserialize_with = "present")]
    pub include_design_customers: Option<Option<bool>>,
}

fn present<'de, D: Deserializer<'de>>(value: D) -> Result<Option<Option<bool>>, D::Error> {
    Option::<bool>::deserialize(value).map(Some)
}

/// `transformer_settings` with the base (preset-less) source rows upserted;
/// a null include flag removes its row and duplicate aliases collapse.
pub fn source_rows(
    project_config: Option<&Value>,
    update: &SourceUpdate,
) -> Vec<Map<String, Value>> {
    let mut updates: Vec<(&'static str, Option<Value>)> = Vec::new();
    if let Some(layers) = &update.customer_layers {
        updates.push((CUSTOMER_LAYERS, Some(json!(layers.join(",")))));
    }
    if let Some(layers) = &update.pole_layers {
        updates.push((POLE_LAYERS, Some(json!(layers.join(",")))));
    }
    if let Some(include) = update.include_design_customers {
        updates.push((INCLUDE_DESIGN_CUSTOMERS, include.map(Value::Bool)));
    }
    let mut out = Vec::new();
    let mut written: Vec<&'static str> = Vec::new();
    for current in sheet_rows(project_config, TRANSFORMER_SETTINGS) {
        let key = canonical_parameter_name(
            current
                .get("parameter")
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
        let update = updates.iter().find(|(name, _)| *name == key);
        match update {
            Some((name, value)) if preset_of(&current).is_none() => {
                if written.contains(name) {
                    continue;
                }
                written.push(name);
                if let Some(value) = value {
                    let mut kept = current;
                    kept.insert("value".into(), value.clone());
                    out.push(kept);
                }
            }
            _ => out.push(current),
        }
    }
    for (name, value) in updates {
        if let (false, Some(value)) = (written.contains(&name), value) {
            out.push(row(name, value));
        }
    }
    out
}

/// The project's declared design sources, read from the base rows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sources {
    pub customer_layers: Vec<String>,
    pub pole_layers: Vec<String>,
    pub include_design_customers: Option<bool>,
    pub customer_layers_declared: bool,
    pub pole_layers_declared: bool,
}

fn layer_list(raw: &Value) -> Vec<String> {
    let text = |value: &Value| match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        _ => String::new(),
    };
    let parts: Vec<String> = match raw {
        Value::Array(items) => items.iter().map(text).collect(),
        other => text(other)
            .split(|ch: char| ch == ',' || ch == ';' || ch.is_whitespace())
            .map(str::to_owned)
            .collect(),
    };
    let mut out: Vec<String> = Vec::new();
    for part in parts {
        let slug = part.trim();
        if !slug.is_empty() && !out.iter().any(|seen| seen == slug) {
            out.push(slug.to_owned());
        }
    }
    out
}

pub fn sources(project_config: Option<&Value>) -> Sources {
    let mut out = Sources {
        customer_layers: Vec::new(),
        pole_layers: Vec::new(),
        include_design_customers: None,
        customer_layers_declared: false,
        pole_layers_declared: false,
    };
    for current in sheet_rows(project_config, TRANSFORMER_SETTINGS) {
        if preset_of(&current).is_some() {
            continue;
        }
        let value = current.get("value").unwrap_or(&Value::Null);
        match canonical_parameter_name(
            current
                .get("parameter")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )
        .as_str()
        {
            CUSTOMER_LAYERS => {
                out.customer_layers = layer_list(value);
                out.customer_layers_declared = true;
            }
            POLE_LAYERS => {
                out.pole_layers = layer_list(value);
                out.pole_layers_declared = true;
            }
            INCLUDE_DESIGN_CUSTOMERS => {
                out.include_design_customers =
                    setting_value(value).and_then(|value| value.as_bool());
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
#[path = "process_settings_rows_tests.rs"]
mod tests;
