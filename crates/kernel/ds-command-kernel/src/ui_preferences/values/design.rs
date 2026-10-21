//! Design preferences of one project: pinned and active transformers, the
//! remembered process preset, the status register's sort and warning filter,
//! and the Fast-mode customer and pole sources; for the account, the pinned
//! group's visibility and the map-data sources a new transformer starts from.
use super::shape::{entry, fields, millis};
use serde_json::{json, Value};

/// The most transformers one project keeps pinned.
pub(in crate::ui_preferences) const MAX_PINNED: usize = 4096;
/// The most map-data sources a new transformer preselects.
pub(in crate::ui_preferences) const MAX_MAP_SOURCES: usize = 512;

/// An LV transformer box name the Kernel's design owner admits.
pub(in crate::ui_preferences) fn lv_transformer(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|name| crate::design::transformer_name(name).is_ok())
}

/// `pinned_transformers`: the project's pinned LV transformers, each once in
/// the order given; the window sorts them for display.
pub(in crate::ui_preferences) fn pinned_transformers(value: &Value) -> Option<Value> {
    let names = value.as_array().filter(|names| names.len() <= MAX_PINNED)?;
    let mut kept: Vec<&str> = Vec::with_capacity(names.len());
    for name in names {
        let name = lv_transformer(name)?;
        if !kept.contains(&name) {
            kept.push(name);
        }
    }
    Some(json!(kept))
}

/// `active_lv_transformer`: the LV transformer the project last opened.
pub(in crate::ui_preferences) fn active_lv_transformer(value: &Value) -> Option<Value> {
    lv_transformer(value).map(Value::from)
}

/// `process_preset`: the operator's last explicit preset for the project, in
/// the process-settings owner's vocabulary.
pub(in crate::ui_preferences) fn process_preset(value: &Value) -> Option<Value> {
    let preset: crate::process_settings::Preset2 = serde_json::from_value(value.clone()).ok()?;
    serde_json::to_value(preset).ok()
}

/// `status_sort`: the register's sort column (or none) and direction, in the
/// status query owner's vocabulary.
pub(in crate::ui_preferences) fn status_sort(value: &Value) -> Option<Value> {
    let map = fields(value, &["key", "asc"])?;
    let key = match map.get("key")? {
        Value::Null => Value::Null,
        Value::String(key) if crate::design_status_query::SORT_KEYS.contains(&key.as_str()) => {
            key.as_str().into()
        }
        _ => return None,
    };
    Some(json!({"key": key, "asc": map.get("asc")?.as_bool()?}))
}

/// `status_warning_types`: the warning filter, as the status query owner
/// spells a persisted selection (trimmed, unique, sorted).
pub(in crate::ui_preferences) fn status_warning_types(value: &Value) -> Option<Value> {
    if !value.as_array()?.iter().all(Value::is_string) {
        return None;
    }
    let request = json!({
        "op": "warning_keys",
        "schema": crate::design_status_query::SCHEMA,
        "value": value,
    });
    let answer = crate::design_status_query::evaluate(request.to_string().as_bytes()).ok()?;
    serde_json::from_str::<Value>(&answer)
        .ok()?
        .get("warning_types")
        .filter(|types| types.is_array())
        .cloned()
}

/// `pinned_group_visible` and other plain switches.
pub(in crate::ui_preferences) fn switch(value: &Value) -> Option<Value> {
    value.as_bool().map(Value::from)
}

/// `create_transformer_map_sources`: the map-data sources a new transformer
/// preselects, each once in the order chosen.
pub(in crate::ui_preferences) fn create_transformer_map_sources(value: &Value) -> Option<Value> {
    let keys = fields(value, &["selectedKeys"])?
        .get("selectedKeys")?
        .as_array()
        .filter(|keys| keys.len() <= MAX_MAP_SOURCES)?;
    let mut kept: Vec<&str> = Vec::with_capacity(keys.len());
    for key in keys {
        let key = key.as_str().filter(|key| entry(key))?;
        if !kept.contains(&key) {
            kept.push(key);
        }
    }
    Some(json!({"selectedKeys": kept}))
}

/// What a sentinel for "the design's own customers" can never name: a pole table.
pub(in crate::ui_preferences) const DESIGN_CUSTOMERS_ADDRESS: &str = "design://layer/customers";

/// Which Fast-mode source a selection names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui_preferences) enum SourceKind {
    Customers,
    Poles,
}

/// A source selection as the design owner's resolver admits an operator's
/// stored choice: bounded, unique, labelled, and for customers never empty.
pub(in crate::ui_preferences) fn resolved_sources(
    kind: SourceKind,
    addresses: &[Value],
    labels: &[Value],
) -> Option<(Value, Value)> {
    let request = json!({
        "schema": "ds.design.source-resolution/v1",
        "kind": match kind {
            SourceKind::Customers => "customers",
            SourceKind::Poles => "poles",
        },
        "project": null,
        "user": {"addresses": addresses, "labels": labels},
    });
    let answer = crate::design::resolve_sources(request.to_string().as_bytes()).ok()?;
    Some((
        answer.get("addresses")?.clone(),
        answer.get("labels")?.clone(),
    ))
}

/// `fast_mode_customer_source` and `fast_mode_pole_source`: the operator's
/// Fast-mode source choice for the project, `{addresses, labels, updated_at}`.
/// A pole selection never keeps the design-customers sentinel.
pub(in crate::ui_preferences) fn fast_mode_sources(
    kind: SourceKind,
    value: &Value,
) -> Option<Value> {
    let map = fields(value, &["addresses", "labels", "updated_at"])?;
    let mut addresses = map.get("addresses")?.as_array()?.clone();
    let mut labels = map.get("labels")?.as_array()?.clone();
    if kind == SourceKind::Poles && addresses.len() == labels.len() {
        let kept: Vec<(Value, Value)> = addresses
            .into_iter()
            .zip(labels)
            .filter(|(address, _)| address.as_str() != Some(DESIGN_CUSTOMERS_ADDRESS))
            .collect();
        (addresses, labels) = kept.into_iter().unzip();
    }
    let (addresses, labels) = resolved_sources(kind, &addresses, &labels)?;
    let updated = millis(map.get("updated_at")?)?;
    Some(json!({"addresses": addresses, "labels": labels, "updated_at": updated}))
}

const PROCESS_SETUP_FIELDS: [&str; 9] = [
    "enabled",
    "sourceAddress",
    "sourceLabel",
    "processPreset",
    "processPresetExplicit",
    "processSettings",
    "autoProcessCadence",
    "differentialAutoProcess",
    "updatedAt",
];
const MAX_PROCESS_SETTINGS: usize = 64;
const MAX_SOURCE_TEXT: usize = 1024;

/// `process_setup`: the project's processing setup — its customer source,
/// preset, settings and AutoProcess cadence. Booleans, bounded text, a preset
/// of `sketch` or `drafting`, settings that are booleans or finite numbers, a
/// cadence of 1–25 pending edits and 5–600 seconds.
pub(in crate::ui_preferences) fn process_setup(value: &Value) -> Option<Value> {
    let map = fields(value, &PROCESS_SETUP_FIELDS)?;
    for flag in ["enabled", "processPresetExplicit", "differentialAutoProcess"] {
        if map.get(flag).is_some_and(|flag| !flag.is_boolean()) {
            return None;
        }
    }
    for name in ["sourceAddress", "sourceLabel"] {
        if let Some(text) = map.get(name) {
            text.as_str().filter(|text| text.len() <= MAX_SOURCE_TEXT)?;
        }
    }
    if let Some(preset) = map.get("processPreset") {
        matches!(preset.as_str(), Some("sketch" | "drafting")).then_some(())?;
    }
    if let Some(settings) = map.get("processSettings") {
        let settings = settings
            .as_object()
            .filter(|settings| settings.len() <= MAX_PROCESS_SETTINGS)?;
        let valid = settings.iter().all(|(key, value)| {
            entry(key) && (value.is_boolean() || value.as_f64().is_some_and(f64::is_finite))
        });
        valid.then_some(())?;
    }
    if let Some(cadence) = map.get("autoProcessCadence") {
        let cadence = fields(cadence, &["minPendingEdits", "maxPendingSeconds"])?;
        let within = |key: &str, min: u64, max: u64| {
            cadence
                .get(key)
                .map_or(true, |count| count.as_u64().is_some_and(|count| (min..=max).contains(&count)))
        };
        (within("minPendingEdits", 1, 25) && within("maxPendingSeconds", 5, 600)).then_some(())?;
    }
    if let Some(at) = map.get("updatedAt") {
        millis(at)?;
    }
    Some(value.clone())
}

#[cfg(test)]
#[path = "design_tests.rs"]
mod tests;
