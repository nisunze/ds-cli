//! Map preferences: the relief intensity and cartography defaults of the account,
//! and per project the camera, layer switches and disabled popups; the
//! browser's raster overlay registry; the editing projection.
use super::shape::{entry, fields, finite_tuple, member, number, ranged};
use crate::gateway_exchange::map_defaults::RELIEF_PRESETS;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

/// `hillshade_exaggeration`: the relief intensity, one of the cycle's
/// presets. Relief visibility itself lasts one session and is never stored.
pub(in crate::ui_preferences) fn hillshade_exaggeration(value: &Value) -> Option<Value> {
    value
        .as_f64()
        .filter(|value| RELIEF_PRESETS.contains(value))
        .map(number)
}

const MAP_DEFAULT_SECTIONS: [&str; 6] = [
    "hillshade",
    "scale_bar",
    "measure",
    "gps_accuracy",
    "zoom_level",
    "camera",
];
const MAX_SECTION_FIELDS: usize = 32;
const MAX_SECTION_TEXT: usize = 256;

/// `map_defaults`: the cartography defaults this edge last read or saved. Each
/// section is a flat record of scalars and hillshade never starts enabled.
/// The basemap catalogue is the frontend's own and is never stored, and a
/// section this Kernel does not know is not kept.
pub(in crate::ui_preferences) fn map_defaults(value: &Value) -> Option<Value> {
    let map = value.as_object()?;
    if map
        .get("basemaps")
        .is_some_and(|catalogue| !catalogue.is_array())
    {
        return None;
    }
    let scalar = |value: &Value| match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => true,
        Value::String(text) => text.len() <= MAX_SECTION_TEXT,
        _ => false,
    };
    let mut out = Map::new();
    for name in MAP_DEFAULT_SECTIONS {
        let Some(section) = map.get(name) else {
            continue;
        };
        let section = section
            .as_object()
            .filter(|section| section.len() <= MAX_SECTION_FIELDS)?;
        if !section
            .iter()
            .all(|(key, value)| entry(key) && scalar(value))
        {
            return None;
        }
        let mut section = section.clone();
        if name == "hillshade" {
            section.insert("enabled".into(), false.into());
        }
        out.insert(name.into(), Value::Object(section));
    }
    Some(Value::Object(out))
}

/// `map_viewport`: the project's last settled camera.
pub(in crate::ui_preferences) fn map_viewport(value: &Value) -> Option<Value> {
    let map = fields(value, &["center", "zoom", "bearing", "pitch"])?;
    let [longitude, latitude] = finite_tuple::<2>(map.get("center")?)?;
    if longitude.abs() > 540.0 || !(-90.0..=90.0).contains(&latitude) {
        return None;
    }
    ranged(map.get("zoom")?, -2.0, 30.0)?;
    ranged(map.get("bearing")?, -720.0, 720.0)?;
    ranged(map.get("pitch")?, 0.0, 90.0)?;
    Some(json!({
        "center": map["center"],
        "zoom": map["zoom"],
        "bearing": map["bearing"],
        "pitch": map["pitch"],
    }))
}

/// `tile_layers`: the browser's raster overlay registry, admitted by the
/// Kernel's overlay owner. The Desktop keeps its own native registry.
pub(in crate::ui_preferences) fn tile_layers(value: &Value) -> Option<Value> {
    let registry: crate::overlays::OverlayRegistry = serde_json::from_value(value.clone()).ok()?;
    registry.validate().ok()?;
    serde_json::to_value(&registry).ok()
}

pub(in crate::ui_preferences) const PROJECTION_CHOICES: [&str; 5] =
    ["auto", "tm_rwanda", "utm", "web_mercator", "custom"];
pub(in crate::ui_preferences) const MAX_CUSTOM_PROJECTION_BYTES: usize = 16 * 1024;

/// `edit_projection_choice`: the projection policy of the editing plane.
pub(in crate::ui_preferences) fn edit_projection_choice(value: &Value) -> Option<Value> {
    member(value, &PROJECTION_CHOICES).map(Value::from)
}

/// `edit_projection_custom`: a PROJ string or PROJ file text, kept verbatim.
pub(in crate::ui_preferences) fn edit_projection_custom(value: &Value) -> Option<Value> {
    value
        .as_str()
        .filter(|proj| proj.len() <= MAX_CUSTOM_PROJECTION_BYTES && !proj.contains('\0'))
        .map(Value::from)
}

/// The most groups or layers one switch record names.
pub(in crate::ui_preferences) const MAX_SWITCHES: usize = 4096;

/// `layer_group_collapse` and `layer_visibility`: one boolean per group or
/// layer key, in the order they were first chosen.
pub(in crate::ui_preferences) fn switches(value: &Value) -> Option<Value> {
    let map = value.as_object().filter(|map| map.len() <= MAX_SWITCHES)?;
    map.iter()
        .all(|(key, value)| entry(key) && value.is_boolean())
        .then(|| value.clone())
}

/// `popup_layers_disabled`: the popup keys switched off, as a sorted set.
/// Popups are on unless named here.
pub(in crate::ui_preferences) fn popup_layers_disabled(value: &Value) -> Option<Value> {
    let mut keys = BTreeSet::new();
    for item in value.as_array()? {
        let key = item.as_str()?.trim();
        if key.is_empty() {
            continue;
        }
        if !entry(key) {
            return None;
        }
        keys.insert(key);
    }
    (keys.len() <= MAX_SWITCHES).then(|| json!(keys))
}

#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
