//! The `persistent_cache` legacy source: the `persist:` keys and prefixes of
//! the retired `ds_web_store` `cache` object store that move into UI
//! preferences, and one reader per key. Deleted whole with the importer once
//! the recovery window closes.
//!
//! Each reader keeps the tolerance of the TypeScript reader it replaces and
//! answers a candidate; the import admits every candidate through its key's
//! contract and bound, so what the contract refuses is skipped and counted. A
//! project key's legacy name is its prefix followed by the project id.
use super::values::design::{lv_transformer, DESIGN_CUSTOMERS_ADDRESS};
use super::values::map::{
    earth_engine_layer, earth_engine_point, HILLSHADE_EXAGGERATIONS, MAP_MODES, MAX_SWITCHES,
};
use super::values::shape::{
    entry, finite_tuple, iso_from_ms, js_number, member, millis, number, ranged, truthy, MAX_SAFE,
};
use super::values::workbench::{admin_selection, boundary, date_bound, surveyor};
use super::Key;
use serde_json::{json, Map, Value};

/// The account keys, read by their exact legacy name.
pub(super) const EXACT: [(&str, Key); 15] = [
    ("ui:layer_group_collapse:v1", Key::LayerGroupCollapse),
    ("map_mode_state", Key::MapMode),
    ("map_defaults", Key::MapDefaults),
    ("tool-layout:map-editing:v3", Key::ToolLayout),
    ("temp_tile_layers_v1", Key::TileLayers),
    ("application_settings:v1", Key::ApplicationSettings),
    ("edit_projection_choice", Key::EditProjectionChoice),
    ("edit_projection_custom_proj", Key::EditProjectionCustom),
    ("design:pinned_group_visible:v1", Key::PinnedGroupVisible),
    // One record of every project's pins; it becomes one row per project.
    ("design:pinned_transformers:v1", Key::PinnedTransformers),
    ("profile:occupancy:v1", Key::ProfileOccupancy),
    (
        "design:create-transformer-map-data-sources:v1",
        Key::CreateTransformerMapSources,
    ),
    (
        "design_layer_upload_dialog_layout:v1",
        Key::UploadDialogLayout,
    ),
    ("properties:dock-width:v1", Key::PropertiesDockWidth),
    ("style_center_layout", Key::StyleCenterLayout),
];

/// The project keys, read by prefix; the rest of the legacy name is the project.
pub(super) const PREFIXES: [(&str, Key); 11] = [
    ("ui:layer_visibility:", Key::LayerVisibility),
    ("ui:popup_layers_disabled:", Key::PopupLayersDisabled),
    ("earth_engine_explorer_state:v1:", Key::EarthEngineExplorer),
    ("working_area_query:v1:", Key::WorkingArea),
    ("ds_design_process_preset:", Key::ProcessPreset),
    ("design:active_lv_transformer:v1:", Key::ActiveLvTransformer),
    ("design_status_warning_types:", Key::StatusWarningTypes),
    ("design_status_sort:", Key::StatusSort),
    ("viewport:", Key::MapViewport),
    (
        "ds_fast_mode_customer_source:v1:",
        Key::FastModeCustomerSource,
    ),
    ("ds_fast_mode_pole_source:v1:", Key::FastModePoleSource),
];

/// The exact legacy names and the prefixes the host reads, read-only.
pub(super) fn keys() -> (Vec<&'static str>, Vec<&'static str>) {
    (
        EXACT.iter().map(|(name, _)| *name).collect(),
        PREFIXES.iter().map(|(prefix, _)| *prefix).collect(),
    )
}

/// The candidate rows one legacy entry becomes: none for a name this source
/// does not own, an unusable value or a project id the Kernel refuses.
pub(super) fn read(legacy_key: &str, value: &Value, now: u64) -> Vec<(Key, Option<String>, Value)> {
    if let Some((_, key)) = EXACT.iter().find(|(name, _)| *name == legacy_key) {
        if *key == Key::PinnedTransformers {
            return pinned_transformers(value);
        }
        return candidate(*key, value, now)
            .map(|value| (*key, None, value))
            .into_iter()
            .collect();
    }
    for (prefix, key) in PREFIXES {
        let Some(project) = legacy_key.strip_prefix(prefix) else {
            continue;
        };
        // `viewport:_global` was named for a project-less camera that was
        // never written; it is no project.
        if !crate::execution_context::valid_project(project)
            || (key == Key::MapViewport && project == "_global")
        {
            return Vec::new();
        }
        return candidate(key, value, now)
            .map(|value| (key, Some(project.to_owned()), value))
            .into_iter()
            .collect();
    }
    Vec::new()
}

/// One legacy value as its key's candidate. The contract canonicalises it.
fn candidate(key: Key, value: &Value, now: u64) -> Option<Value> {
    match key {
        Key::LayerGroupCollapse | Key::LayerVisibility => switches(value),
        Key::MapMode => map_mode(value),
        Key::MapDefaults | Key::ToolLayout | Key::StyleCenterLayout => {
            value.is_object().then(|| value.clone())
        }
        Key::TileLayers | Key::MapViewport | Key::EditProjectionChoice => Some(value.clone()),
        Key::ApplicationSettings => application_settings(value),
        Key::EditProjectionCustom => value.is_string().then(|| value.clone()),
        Key::PinnedGroupVisible => value.is_boolean().then(|| value.clone()),
        Key::ProfileOccupancy => profile_occupancy(value, now),
        Key::CreateTransformerMapSources => map_sources(value),
        Key::UploadDialogLayout => {
            let split = js_number(value.get("splitLeftPercent")?)?;
            Some(json!({"splitLeftPercent": split}))
        }
        Key::PropertiesDockWidth => value.is_number().then(|| value.clone()),
        Key::PopupLayersDisabled => popup_layers_disabled(value),
        Key::EarthEngineExplorer => earth_engine_explorer(value, now),
        Key::WorkingArea => working_area(value, now),
        Key::ProcessPreset => value.get("preset").cloned(),
        Key::ActiveLvTransformer => active_lv_transformer(value),
        Key::StatusWarningTypes => {
            let types = value.as_array()?;
            let kept: Vec<&Value> = types.iter().filter(|kind| kind.is_string()).collect();
            kept.iter()
                .any(|kind| kind.as_str().is_some_and(|kind| !kind.trim().is_empty()))
                .then(|| json!(kept))
        }
        Key::StatusSort => {
            let key = value.get("key")?;
            (key.is_null() || key.is_string())
                .then(|| json!({"key": key, "asc": truthy(value.get("asc"))}))
        }
        Key::FastModeCustomerSource => fast_mode_customers(value),
        Key::FastModePoleSource => fast_mode_poles(value),
        Key::PinnedTransformers | Key::ActiveProject => None,
    }
}

/// Boolean entries only, first chosen first; nothing chosen is no row.
fn switches(value: &Value) -> Option<Value> {
    let kept: Map<String, Value> = value
        .as_object()?
        .iter()
        .filter(|(key, value)| entry(key) && value.is_boolean())
        .take(MAX_SWITCHES)
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    (!kept.is_empty()).then_some(Value::Object(kept))
}

/// The nearest relief preset, the first of two equally near.
fn closest_exaggeration(value: f64) -> f64 {
    let mut best = HILLSHADE_EXAGGERATIONS[0];
    for preset in HILLSHADE_EXAGGERATIONS {
        if (value - preset).abs() < (value - best).abs() {
            best = preset;
        }
    }
    best
}

/// The camera mode as `loadMapModeState` restored it: an unknown provider is
/// `auto`, a missing 3D layer switch is on, an off-preset intensity snaps to
/// the nearest preset and relief itself never restores.
fn map_mode(value: &Value) -> Option<Value> {
    let saved = value.as_object()?;
    let mode = member(saved.get("mode")?, &MAP_MODES)?;
    let provider = match saved.get("provider").and_then(Value::as_str) {
        Some(provider @ ("aws-terrain" | "google")) => provider,
        _ => "auto",
    };
    let on_unless_off = |name: &str| match saved.get(name) {
        None | Some(Value::Null) => true,
        flag => truthy(flag),
    };
    let mut out = json!({
        "mode": mode,
        "rotationLocked": truthy(saved.get("rotationLocked")),
        "provider": provider,
        "google3dBuildings": on_unless_off("google3dBuildings"),
        "layer3DShapes": on_unless_off("layer3DShapes"),
        "hillshade": false,
    });
    if let Some(intensity) = saved.get("hillshadeExaggeration").and_then(Value::as_f64) {
        out["hillshadeExaggeration"] = number(closest_exaggeration(intensity));
    }
    Some(out)
}

/// The settings record, unless it names no setting at all. The retired
/// `profileTerrainPointRadiusPx` is still read by the settings owner.
fn application_settings(value: &Value) -> Option<Value> {
    let saved = value.as_object()?;
    saved
        .keys()
        .any(|key| {
            crate::application_settings::SETTING_KEYS.contains(&key.as_str())
                || key == "profileTerrainPointRadiusPx"
        })
        .then(|| value.clone())
}

/// A restart record `hydrateGridProfileOccupancy` would restore: schema 1,
/// and a generic profile only with its scene.
fn profile_occupancy(value: &Value, now: u64) -> Option<Value> {
    let saved = value.as_object()?;
    if saved.get("schemaVersion")?.as_u64()? != 1 {
        return None;
    }
    let generic = saved.get("occupant")?.get("kind")?.as_str()? == "generic";
    if generic && saved.get("genericScene").map_or(true, Value::is_null) {
        return None;
    }
    let mut out = value.clone();
    if saved.get("updatedAt").and_then(millis).is_none() {
        out["updatedAt"] = now.into();
    }
    Some(out)
}

fn map_sources(value: &Value) -> Option<Value> {
    let keys: Vec<&str> = value
        .get("selectedKeys")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter(|key| entry(key))
        .collect();
    (!keys.is_empty()).then(|| json!({"selectedKeys": keys}))
}

/// The disabled popup keys from either retired shape: a list, or an object
/// whose `true` members are the disabled keys.
fn popup_layers_disabled(value: &Value) -> Option<Value> {
    let keys: Vec<String> = match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::String(key) => Some(key.trim().to_owned()),
                Value::Number(key) => Some(key.to_string()),
                _ => None,
            })
            .collect(),
        Value::Object(members) => members
            .iter()
            .filter(|(_, enabled)| **enabled == Value::Bool(true))
            .map(|(key, _)| key.trim().to_owned())
            .collect(),
        _ => return None,
    };
    let keys: Vec<String> = keys.into_iter().filter(|key| entry(key)).collect();
    (!keys.is_empty()).then(|| json!(keys))
}

fn clamp_opacity(value: Option<f64>) -> f64 {
    value
        .filter(|opacity| opacity.is_finite())
        .unwrap_or(0.72)
        .clamp(0.0, 1.0)
}

/// `validOverlay`: an id and a tile URL with something in them.
fn usable_overlay(value: &Value) -> bool {
    let filled = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
    };
    filled("id") && filled("tileUrl")
}

/// `layerFromPersisted`, kept only when the layer contract admits it.
fn earth_engine_layer_from(
    value: &Value,
    fallback_opacity: Option<&Value>,
    fallback_visible: bool,
    now_iso: &str,
) -> Option<Value> {
    let record = value.as_object()?;
    let overlay = record
        .get("overlay")
        .filter(|overlay| usable_overlay(overlay))?;
    let id = record
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .or_else(|| overlay.get("id").and_then(Value::as_str))?;
    let visible = match record.get("visible") {
        None => fallback_visible,
        Some(flag) => *flag == Value::Bool(true),
    };
    let opacity = record
        .get("opacity")
        .filter(|opacity| !opacity.is_null())
        .or(fallback_opacity)
        .and_then(js_number);
    let stamp = |name: &str| {
        record
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or(now_iso)
            .to_owned()
    };
    earth_engine_layer(&json!({
        "id": id,
        "overlay": overlay,
        "visible": visible,
        "opacity": number(clamp_opacity(opacity)),
        "addedAt": stamp("addedAt"),
        "updatedAt": stamp("updatedAt"),
    }))
}

/// A version 1 or 2 explorer record as version 2: a version 1 single overlay
/// becomes the one layer, and the active layer is the chosen visible one or
/// else the last visible one (`withCompatibilityFields`).
fn earth_engine_explorer(value: &Value, now: u64) -> Option<Value> {
    let saved = value.as_object()?;
    if !matches!(saved.get("version")?.as_u64()?, 1 | 2) {
        return None;
    }
    let now_iso = iso_from_ms(now);
    let saved_at = saved
        .get("savedAt")
        .and_then(Value::as_str)
        .filter(|at| !at.is_empty())
        .map_or_else(|| now_iso.clone(), str::to_owned);
    let fallback_opacity = saved.get("opacity");
    let fallback_visible = saved.get("visible") != Some(&Value::Bool(false));
    let mut layers: Vec<Value> = saved
        .get("overlays")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|layer| {
            earth_engine_layer_from(layer, fallback_opacity, fallback_visible, &now_iso)
        })
        .collect();
    let single = saved
        .get("overlay")
        .filter(|overlay| usable_overlay(overlay));
    if layers.is_empty() {
        if let Some(overlay) = single {
            layers.extend(earth_engine_layer_from(
                &json!({"overlay": overlay, "addedAt": saved_at, "updatedAt": saved_at}),
                fallback_opacity,
                fallback_visible,
                &now_iso,
            ));
        }
    }
    let chosen = saved
        .get("activeOverlayId")
        .and_then(Value::as_str)
        .or_else(|| {
            single
                .and_then(|overlay| overlay.get("id"))
                .and_then(Value::as_str)
        });
    let visible = |layer: &&Value| layer["visible"] == true;
    let active = chosen
        .and_then(|id| {
            layers
                .iter()
                .filter(visible)
                .find(|layer| layer["id"] == id)
        })
        .or_else(|| layers.iter().rev().find(visible));
    let point = saved
        .get("point")
        .and_then(earth_engine_point)
        .unwrap_or(Value::Null);
    Some(json!({
        "version": 2,
        "point": point,
        "overlay": active.map_or(Value::Null, |layer| layer["overlay"].clone()),
        "overlays": layers,
        "activeOverlayId": active.map(|layer| layer["id"].clone()).or_else(|| chosen.map(Value::from)).unwrap_or(Value::Null),
        "visible": active.is_some(),
        "opacity": active.map_or_else(|| number(clamp_opacity(fallback_opacity.and_then(js_number))), |layer| layer["opacity"].clone()),
        "savedAt": saved_at,
    }))
}

/// `normalizePersistedWorkingArea`: an applied area keeps what its mode uses;
/// unusable facts read as absent; a missing apply time is now.
fn working_area(value: &Value, now: u64) -> Option<Value> {
    let saved = value.as_object()?;
    if !truthy(saved.get("applied")) {
        return None;
    }
    let mode = match saved.get("mode").and_then(Value::as_str) {
        Some(mode @ ("admin" | "draw")) => mode,
        _ => "full",
    };
    let admin = saved
        .get("admin")
        .and_then(admin_selection)
        .filter(|_| mode == "admin");
    let drawn = saved
        .get("drawBoundary")
        .and_then(boundary)
        .filter(|_| mode == "draw");
    let date = |name: &str| {
        saved
            .get(name)
            .and_then(Value::as_str)
            .filter(|date| date_bound(date))
            .unwrap_or("")
    };
    let surveyors: Vec<&str> = saved
        .get("surveyors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(surveyor)
        .take(200)
        .collect();
    let applied_at = saved
        .get("appliedAt")
        .and_then(Value::as_f64)
        .filter(|at| at.is_finite() && *at >= 0.0 && *at <= MAX_SAFE as f64)
        .map_or(now, |at| at.floor() as u64);
    let kept = |name: &str, usable: fn(&Value) -> bool| {
        saved
            .get(name)
            .filter(|value| usable(value))
            .cloned()
            .unwrap_or(Value::Null)
    };
    Some(json!({
        "mode": mode,
        "admin": admin,
        "drawBoundary": drawn,
        "fullProjectLoad": mode == "full" && saved.get("fullProjectLoad") == Some(&Value::Bool(true)),
        "dateFrom": date("dateFrom"),
        "dateTo": date("dateTo"),
        "surveyors": surveyors,
        "applied": true,
        "appliedAt": applied_at,
        "appliedCenter": kept("appliedCenter", |center| finite_tuple::<2>(center).is_some()),
        "appliedZoom": kept("appliedZoom", |zoom| ranged(zoom, -2.0, 30.0).is_some()),
        "appliedBbox": kept("appliedBbox", |bbox| finite_tuple::<4>(bbox).is_some()),
    }))
}

/// A bare name, or the `{transformerName}` record, trimmed.
fn active_lv_transformer(value: &Value) -> Option<Value> {
    let name = match value {
        Value::String(name) => name.as_str(),
        _ => value.get("transformerName")?.as_str()?,
    }
    .trim();
    lv_transformer(&Value::from(name)).map(Value::from)
}

/// `pinned_transformers`: one row per project id the Kernel admits.
fn pinned_transformers(value: &Value) -> Vec<(Key, Option<String>, Value)> {
    let Some(projects) = value.as_object() else {
        return Vec::new();
    };
    projects
        .iter()
        .filter(|(project, _)| crate::execution_context::valid_project(project))
        .filter_map(|(project, names)| {
            let names: Vec<Value> = names
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .map(|name| Value::from(name.trim()))
                .filter(|name| lv_transformer(name).is_some())
                .collect();
            (!names.is_empty()).then(|| {
                (
                    Key::PinnedTransformers,
                    Some(project.clone()),
                    Value::Array(names),
                )
            })
        })
        .collect()
}

fn updated_at(saved: &Map<String, Value>) -> u64 {
    saved
        .get("updatedAt")
        .and_then(Value::as_f64)
        .filter(|at| at.is_finite() && *at >= 0.0 && *at <= MAX_SAFE as f64)
        .map_or(0, |at| at.floor() as u64)
}

/// The customer choice `normalizePreference` read: the ordered lists, or the
/// singular fields of the first release.
fn fast_mode_customers(value: &Value) -> Option<Value> {
    let saved = value.as_object()?;
    let single = |current: &str, first: &str| {
        saved
            .get(current)
            .and_then(Value::as_str)
            .or_else(|| saved.get(first).and_then(Value::as_str))
            .map_or("", str::trim)
    };
    let address = single("customerSourceAddress", "sourceAddress");
    let label = single("customerSourceLabel", "sourceLabel");
    let listed = saved
        .get("customerSourceAddresses")
        .and_then(Value::as_array);
    if listed.is_none() && address.is_empty() {
        return None;
    }
    let addresses = listed.cloned().unwrap_or_else(|| vec![address.into()]);
    let labels = match saved.get("customerSourceLabels").and_then(Value::as_array) {
        Some(labels) => labels.clone(),
        None if label.is_empty() => Vec::new(),
        None => vec![label.into()],
    };
    Some(json!({"addresses": addresses, "labels": labels, "updated_at": updated_at(saved)}))
}

/// One stored list as trimmed text; a member that is not text is empty.
fn trimmed_texts<'a>(saved: &'a Map<String, Value>, name: &str) -> Vec<&'a str> {
    saved
        .get(name)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|item| item.as_str().unwrap_or("").trim())
        .collect()
}

/// The pole choice `resolveFastModePoleSource` read: trimmed, without empty
/// rows or the design-customers sentinel an early release could store.
fn fast_mode_poles(value: &Value) -> Option<Value> {
    let saved = value.as_object()?;
    let (addresses, labels) = (
        trimmed_texts(saved, "poleSourceAddresses"),
        trimmed_texts(saved, "poleSourceLabels"),
    );
    let mut kept = (Vec::new(), Vec::new());
    for (index, address) in addresses.iter().enumerate() {
        if address.is_empty() || *address == DESIGN_CUSTOMERS_ADDRESS {
            continue;
        }
        kept.0.push(*address);
        kept.1.push(labels.get(index).copied().unwrap_or(""));
    }
    Some(json!({"addresses": kept.0, "labels": kept.1, "updated_at": updated_at(saved)}))
}

#[cfg(test)]
#[path = "legacy_tests.rs"]
mod tests;
