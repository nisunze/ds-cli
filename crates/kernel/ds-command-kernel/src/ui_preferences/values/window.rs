//! The account's window presentation: theme and language, the Web page's
//! offline isolation, the hints and Desktop recommendation already shown, the
//! layer drawer's groups and width, the command shortcuts, the canvas
//! connector style, the Tools panel and PDF preview geometry, and the Profile
//! surface's display choices and per-model cameras.
use super::shape::{entry, fields, identity, js_round, member, millis, number, ranged, text};
use serde_json::{json, Map, Value};

pub(in crate::ui_preferences) const THEMES: [&str; 3] = ["light", "dark", "system"];
pub(in crate::ui_preferences) const LANGUAGES: [&str; 2] = ["en", "fr"];

/// One member of a closed vocabulary, stored as given.
fn named(value: &Value, members: &[&'static str]) -> Option<Value> {
    member(value, members).map(Value::from)
}

/// `theme`: light, dark or the system's choice.
pub(in crate::ui_preferences) fn theme(value: &Value) -> Option<Value> {
    named(value, &THEMES)
}

/// `language`: the interface language.
pub(in crate::ui_preferences) fn language(value: &Value) -> Option<Value> {
    named(value, &LANGUAGES)
}

/// A time a JavaScript clock wrote (`desktop_recommendation_shown_at`).
pub(in crate::ui_preferences) fn instant(value: &Value) -> Option<Value> {
    millis(value).map(Value::from)
}

const MAX_HINTS: usize = 128;
const MAX_HINT_ID_BYTES: usize = 128;

/// `activity_hints`: the hints already shown, each once, oldest first (the
/// latest kept), and when the last one was shown.
pub(in crate::ui_preferences) fn activity_hints(value: &Value) -> Option<Value> {
    let map = fields(value, &["seen", "lastShownAt"])?;
    let mut seen: Vec<&str> = Vec::new();
    for id in map.get("seen")?.as_array()? {
        let id = identity(id, MAX_HINT_ID_BYTES)?;
        if !seen.contains(&id) {
            seen.push(id);
        }
    }
    let skip = seen.len().saturating_sub(MAX_HINTS);
    let last = match map.get("lastShownAt").filter(|at| !at.is_null()) {
        Some(at) => Value::from(millis(at)?),
        None => Value::Null,
    };
    Some(json!({"seen": seen[skip..], "lastShownAt": last}))
}

/// The layer drawer's groups, in drawer order.
pub(in crate::ui_preferences) const LAYER_GROUPS: [&str; 6] = [
    "survey",
    "design",
    "project-work",
    "local",
    "tiles",
    "basemap",
];

/// `layer_group_settings`: whether each drawer group is shown and keeps its
/// contents visible. Every group is answered; one not named takes both on.
pub(in crate::ui_preferences) fn layer_group_settings(value: &Value) -> Option<Value> {
    let map = fields(value, &LAYER_GROUPS)?;
    let mut out = Map::new();
    for group in LAYER_GROUPS {
        let row = match map.get(group) {
            Some(row) => Some(fields(row, &["showInDrawer", "keepContentsVisible"])?),
            None => None,
        };
        let flag = |name: &str| match row.and_then(|row| row.get(name)) {
            Some(flag) => flag.as_bool(),
            None => Some(true),
        };
        out.insert(
            group.into(),
            json!({"showInDrawer": flag("showInDrawer")?, "keepContentsVisible": flag("keepContentsVisible")?}),
        );
    }
    Some(Value::Object(out))
}

/// `command_shortcuts_visible`: which command shortcuts the drawer shows, as
/// the navigation owner normalises them (retired shortcuts dropped, every
/// current one answered).
pub(in crate::ui_preferences) fn command_shortcuts(value: &Value) -> Option<Value> {
    let map = value.as_object()?;
    if !map
        .iter()
        .all(|(key, flag)| entry(key) && flag.is_boolean())
    {
        return None;
    }
    let normalized = crate::application_navigation::shortcut_preferences(&value.to_string(), None);
    serde_json::from_str(&normalized.serialized).ok()
}

pub(in crate::ui_preferences) const MIN_DRAWER_WIDTH: f64 = 320.0;
pub(in crate::ui_preferences) const MAX_DRAWER_WIDTH: f64 = 600.0;

/// `drawer_width`: the application drawer's width in whole pixels.
pub(in crate::ui_preferences) fn drawer_width(value: &Value) -> Option<Value> {
    let width = value.as_f64()?;
    Some(number(
        js_round(width).clamp(MIN_DRAWER_WIDTH, MAX_DRAWER_WIDTH),
    ))
}

pub(in crate::ui_preferences) const CONNECTOR_SHAPES: [&str; 4] =
    ["angled", "straight", "quadratic", "curved"];
pub(in crate::ui_preferences) const CONNECTOR_MARKERS: [&str; 4] =
    ["end", "middle", "dots", "none"];

/// `connector_style`: how every canvas graph draws its connectors.
pub(in crate::ui_preferences) fn connector_style(value: &Value) -> Option<Value> {
    let map = fields(value, &["shape", "marker"])?;
    Some(json!({
        "shape": member(map.get("shape")?, &CONNECTOR_SHAPES)?,
        "marker": member(map.get("marker")?, &CONNECTOR_MARKERS)?,
    }))
}

const MAX_PANEL_PIXELS: f64 = 100_000.0;
pub(in crate::ui_preferences) const MIN_TOOLS_PANEL_WIDTH: f64 = 320.0;
pub(in crate::ui_preferences) const MIN_TOOLS_PANEL_HEIGHT: f64 = 280.0;

/// `tools_panel_size`: the Tools panel's size in whole pixels, never below its
/// usable minimum. The panel clamps it into the viewport it opens in.
pub(in crate::ui_preferences) fn tools_panel_size(value: &Value) -> Option<Value> {
    let map = fields(value, &["width", "height"])?;
    let width = ranged(map.get("width")?, 0.0, MAX_PANEL_PIXELS)?;
    let height = ranged(map.get("height")?, 0.0, MAX_PANEL_PIXELS)?;
    Some(json!({
        "width": number(js_round(width).max(MIN_TOOLS_PANEL_WIDTH)),
        "height": number(js_round(height).max(MIN_TOOLS_PANEL_HEIGHT)),
    }))
}

/// `pdf_screen_scale`: the PDF preview's calibration of this screen's 100 %.
pub(in crate::ui_preferences) fn pdf_screen_scale(value: &Value) -> Option<Value> {
    ranged(value, 0.5, 2.0).map(number)
}

pub(in crate::ui_preferences) const LABEL_FIELDS: [&str; 8] = [
    "number",
    "station",
    "type",
    "height",
    "alignment",
    "comment1",
    "comment2",
    "comment3",
];
pub(in crate::ui_preferences) const LABEL_ORIENTATIONS: [&str; 6] =
    ["auto", "right", "left", "above", "below", "vertical"];

/// `profile_workspace`: the Profile surface's fit and zoom controls and the
/// structure labels the map draws, each field once in the chosen order.
pub(in crate::ui_preferences) fn profile_workspace(value: &Value) -> Option<Value> {
    let map = fields(
        value,
        &["showFit", "showZoom", "mapLabels", "mapLabelOrientation"],
    )?;
    let mut labels: Vec<&str> = Vec::new();
    for field in map.get("mapLabels")?.as_array()? {
        let field = member(field, &LABEL_FIELDS)?;
        if !labels.contains(&field) {
            labels.push(field);
        }
    }
    Some(json!({
        "showFit": map.get("showFit")?.as_bool()?,
        "showZoom": map.get("showZoom")?.as_bool()?,
        "mapLabels": labels,
        "mapLabelOrientation": member(map.get("mapLabelOrientation")?, &LABEL_ORIENTATIONS)?,
    }))
}

/// The Profile cameras kept, the latest last.
pub(in crate::ui_preferences) const MAX_PROFILE_VIEWPORTS: usize = 64;
const MAX_VIEWPORT_ID_BYTES: usize = 2048;
const MAX_CURSOR_BYTES: usize = 512;
const MAX_PROFILE_PAN: f64 = 1.0e12;

fn profile_viewport(value: &Value) -> Option<(String, Value)> {
    let map = fields(value, &["id", "zoom", "panX", "panY", "cursorEntityId"])?;
    let id = text(map.get("id")?, MAX_VIEWPORT_ID_BYTES)?.to_owned();
    let zoom = ranged(map.get("zoom")?, f64::MIN_POSITIVE, MAX_PROFILE_PAN)?;
    let pan_x = ranged(map.get("panX")?, -MAX_PROFILE_PAN, MAX_PROFILE_PAN)?;
    let pan_y = ranged(map.get("panY")?, -MAX_PROFILE_PAN, MAX_PROFILE_PAN)?;
    let cursor = match map.get("cursorEntityId").filter(|cursor| !cursor.is_null()) {
        Some(cursor) => Value::from(text(cursor, MAX_CURSOR_BYTES)?),
        None => Value::Null,
    };
    let camera = json!({"id": id, "zoom": number(zoom), "panX": number(pan_x), "panY": number(pan_y), "cursorEntityId": cursor});
    Some((id, camera))
}

/// `profile_viewports`: each model's Profile camera and cursor, keyed by the
/// immutable project revision or the mutable local model. A camera named
/// twice keeps its later entry; past the bound the oldest leave first.
pub(in crate::ui_preferences) fn profile_viewports(value: &Value) -> Option<Value> {
    let mut kept: Vec<(String, Value)> = Vec::new();
    for camera in value.as_array()? {
        let (id, camera) = profile_viewport(camera)?;
        kept.retain(|(held, _)| *held != id);
        kept.push((id, camera));
    }
    let skip = kept.len().saturating_sub(MAX_PROFILE_VIEWPORTS);
    Some(Value::Array(
        kept.into_iter()
            .skip(skip)
            .map(|(_, camera)| camera)
            .collect(),
    ))
}

#[cfg(test)]
#[path = "window_tests.rs"]
mod tests;
