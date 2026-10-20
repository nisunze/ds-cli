//! Map preferences: the camera mode and cartography defaults of the account,
//! and per project the camera, layer switches, disabled popups and Earth
//! Engine imagery; the browser's raster overlay registry; the editing
//! projection.
use super::shape::{entry, fields, finite_tuple, label, member, number, ranged, text};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub(in crate::ui_preferences) const MAP_MODES: [&str; 2] = ["2d", "3d"];
pub(in crate::ui_preferences) const PROVIDERS_3D: [&str; 3] = ["auto", "aws-terrain", "google"];
/// The relief intensities the hillshade control cycles through.
pub(in crate::ui_preferences) const HILLSHADE_EXAGGERATIONS: [f64; 5] = [0.3, 0.6, 1.0, 1.5, 2.5];
const MAP_MODE_FIELDS: [&str; 7] = [
    "mode",
    "rotationLocked",
    "provider",
    "google3dBuildings",
    "layer3DShapes",
    "hillshade",
    "hillshadeExaggeration",
];

/// `map_mode`: the 2D/3D camera choices. Relief visibility lasts one session,
/// so the stored record always says `hillshade: false`; an intensity is kept
/// only when it is one of the cycle's presets, and an absent one leaves the
/// cartography default in charge.
pub(in crate::ui_preferences) fn map_mode(value: &Value) -> Option<Value> {
    let map = fields(value, &MAP_MODE_FIELDS)?;
    if map.get("hillshade").is_some_and(|flag| !flag.is_boolean()) {
        return None;
    }
    let mut out = Map::new();
    out.insert("mode".into(), member(map.get("mode")?, &MAP_MODES)?.into());
    out.insert(
        "rotationLocked".into(),
        map.get("rotationLocked")?.as_bool()?.into(),
    );
    out.insert(
        "provider".into(),
        member(map.get("provider")?, &PROVIDERS_3D)?.into(),
    );
    out.insert(
        "google3dBuildings".into(),
        map.get("google3dBuildings")?.as_bool()?.into(),
    );
    out.insert(
        "layer3DShapes".into(),
        map.get("layer3DShapes")?.as_bool()?.into(),
    );
    out.insert("hillshade".into(), false.into());
    if let Some(exaggeration) = map.get("hillshadeExaggeration") {
        let preset = exaggeration
            .as_f64()
            .filter(|value| HILLSHADE_EXAGGERATIONS.contains(value))?;
        out.insert("hillshadeExaggeration".into(), number(preset));
    }
    Some(Value::Object(out))
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

pub(in crate::ui_preferences) const MAX_EARTH_ENGINE_LAYERS: usize = 32;
const MAX_OVERLAY_ID_BYTES: usize = 512;
const MAX_OVERLAY_TEXT: usize = 1024;
const MAX_TILE_URL_BYTES: usize = 4096;
const MAX_TIMESTAMP_TEXT: usize = 64;
const OVERLAY_TEXT_FIELDS: [&str; 7] = [
    "name",
    "dataset",
    "acquiredAt",
    "attribution",
    "expiresAt",
    "provider",
    "license",
];
const OVERLAY_NUMBER_FIELDS: [&str; 3] = ["nativeGsdM", "minZoom", "maxZoom"];
const CREDENTIAL_QUERY_KEYS: [&str; 8] = [
    "access_token",
    "authorization",
    "code",
    "id_token",
    "mapid",
    "oauth",
    "refresh_token",
    "token",
];

/// A tile template the imagery broker hands out: never a credential-bearing
/// query, and a direct Earth Engine URL only as its public tiles path.
pub(in crate::ui_preferences) fn safe_tile_template(raw: &str) -> bool {
    let raw = raw.trim();
    if raw.is_empty()
        || !["{z}", "{x}", "{y}"]
            .iter()
            .all(|token| raw.contains(token))
    {
        return false;
    }
    if raw.to_lowercase().contains("googleapis.com/earthengine") {
        return false;
    }
    let Ok(url) = url::Url::parse("https://ds-web.local").and_then(|base| base.join(raw)) else {
        return false;
    };
    if url
        .query_pairs()
        .any(|(key, _)| CREDENTIAL_QUERY_KEYS.contains(&key.to_lowercase().as_str()))
    {
        return false;
    }
    let path = url.path().to_lowercase();
    if url.host_str() == Some("earthengine.googleapis.com") {
        return url.query().map_or(true, str::is_empty)
            && url.scheme() == "https"
            && path.starts_with("/v1/projects/")
            && path.contains("/maps/")
            && path.contains("/tiles/");
    }
    (path.contains("/api/v1/earth-engine/tiles/") || path.contains("/api/v1/imagery/tiles/"))
        && matches!(url.scheme(), "https" | "http")
}

/// An id with something in it, kept exactly as written.
fn named(value: &Value) -> Option<&str> {
    text(value, MAX_OVERLAY_ID_BYTES).filter(|id| !id.trim().is_empty())
}

/// One Earth Engine overlay as the broker described it; absent optional facts
/// stay absent.
pub(in crate::ui_preferences) fn earth_engine_overlay(value: &Value) -> Option<Value> {
    let map = value.as_object()?;
    let mut out = Map::new();
    out.insert("id".into(), named(map.get("id")?)?.into());
    for (key, field) in map {
        match key.as_str() {
            "id" => {}
            "tileUrl" => {
                let url = text(field, MAX_TILE_URL_BYTES).filter(|url| safe_tile_template(url))?;
                out.insert(key.clone(), url.into());
            }
            _ if field.is_null() => {}
            name if OVERLAY_TEXT_FIELDS.contains(&name) => {
                out.insert(key.clone(), label(field, MAX_OVERLAY_TEXT)?.into());
            }
            name if OVERLAY_NUMBER_FIELDS.contains(&name) => {
                field.as_f64()?;
                out.insert(key.clone(), field.clone());
            }
            "bounds" => {
                finite_tuple::<4>(field)?;
                out.insert(key.clone(), field.clone());
            }
            _ => return None,
        }
    }
    out.contains_key("tileUrl").then_some(Value::Object(out))
}

/// The picked imagery point, or none.
pub(in crate::ui_preferences) fn earth_engine_point(value: &Value) -> Option<Value> {
    if value.is_null() {
        return Some(Value::Null);
    }
    let map = fields(value, &["lng", "lat", "label"])?;
    ranged(map.get("lng")?, -540.0, 540.0)?;
    ranged(map.get("lat")?, -90.0, 90.0)?;
    let mut out = json!({"lng": map["lng"], "lat": map["lat"]});
    match map.get("label") {
        None | Some(Value::Null) => {}
        Some(name) => out["label"] = label(name, MAX_OVERLAY_TEXT)?.into(),
    }
    Some(out)
}

/// One imagery layer of the explorer.
pub(in crate::ui_preferences) fn earth_engine_layer(value: &Value) -> Option<Value> {
    let map = fields(
        value,
        &[
            "id",
            "overlay",
            "visible",
            "opacity",
            "addedAt",
            "updatedAt",
        ],
    )?;
    let id = named(map.get("id")?)?;
    let overlay = earth_engine_overlay(map.get("overlay")?)?;
    let visible = map.get("visible")?.as_bool()?;
    ranged(map.get("opacity")?, 0.0, 1.0)?;
    let added = label(map.get("addedAt")?, MAX_TIMESTAMP_TEXT)?;
    let updated = label(map.get("updatedAt")?, MAX_TIMESTAMP_TEXT)?;
    Some(json!({
        "id": id,
        "overlay": overlay,
        "visible": visible,
        "opacity": map["opacity"],
        "addedAt": added,
        "updatedAt": updated,
    }))
}

const EARTH_ENGINE_FIELDS: [&str; 8] = [
    "version",
    "point",
    "overlay",
    "overlays",
    "activeOverlayId",
    "visible",
    "opacity",
    "savedAt",
];

/// `earth_engine_explorer` (version 2): the project's picked point and
/// imagery layers. `overlay`, `visible` and `opacity` describe the active
/// layer; `opacity` is also the opacity the next layer opens with.
pub(in crate::ui_preferences) fn earth_engine_explorer(value: &Value) -> Option<Value> {
    let map = fields(value, &EARTH_ENGINE_FIELDS)?;
    if map.get("version")?.as_u64()? != 2 {
        return None;
    }
    let point = earth_engine_point(map.get("point").unwrap_or(&Value::Null))?;
    let overlay = match map.get("overlay") {
        None | Some(Value::Null) => Value::Null,
        Some(overlay) => earth_engine_overlay(overlay)?,
    };
    let layers = map
        .get("overlays")?
        .as_array()
        .filter(|layers| layers.len() <= MAX_EARTH_ENGINE_LAYERS)?
        .iter()
        .map(earth_engine_layer)
        .collect::<Option<Vec<_>>>()?;
    let active = match map.get("activeOverlayId") {
        None | Some(Value::Null) => Value::Null,
        Some(id) => named(id)?.into(),
    };
    let visible = map.get("visible")?.as_bool()?;
    ranged(map.get("opacity")?, 0.0, 1.0)?;
    let saved_at = label(map.get("savedAt")?, MAX_TIMESTAMP_TEXT)?;
    Some(json!({
        "version": 2,
        "point": point,
        "overlay": overlay,
        "overlays": layers,
        "activeOverlayId": active,
        "visible": visible,
        "opacity": map["opacity"],
        "savedAt": saved_at,
    }))
}

#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
