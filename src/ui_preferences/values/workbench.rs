//! Workbench preferences of the account: the map-editing toolbar, panel and
//! dialog geometry, the operator's application settings and the Profile
//! surface's restart record; per project, the applied survey working area.
use super::shape::{
    entry, fields, finite_tuple, identity, js_round, label, millis, number, ranged, stored_len,
    text,
};
use serde_json::{json, Map, Value};

/// The toolbar's modify tools in their default order; `settings` always leads.
pub(in crate::ui_preferences) const MODIFY_TOOLS: [&str; 11] = [
    "settings",
    "move",
    "vertex",
    "trim",
    "extend",
    "split",
    "splitWithCutter",
    "join",
    "offset",
    "buffer",
    "delete",
];
/// Every modify tool's default pin, including `calculateProperties`, which is
/// kept pinned-off while it has no place in the order.
const MODIFY_PINS: [(&str, bool); 12] = [
    ("settings", true),
    ("move", true),
    ("vertex", false),
    ("trim", false),
    ("extend", false),
    ("split", false),
    ("splitWithCutter", false),
    ("join", false),
    ("calculateProperties", false),
    ("offset", false),
    ("buffer", false),
    ("delete", false),
];
pub(in crate::ui_preferences) const ACTION_TOOLS: [&str; 3] = ["process", "cloudSave", "reports"];
const ACTION_PINS: [(&str, bool); 3] = [("process", true), ("cloudSave", true), ("reports", true)];

/// One toolbar group: known tools once each in the chosen order, unnamed
/// tools appended in default order, `settings` first, and a pin for every
/// tool (a chosen pin counts only for a tool in the order).
fn tool_group(raw: Option<&Value>, tools: &[&'static str], pins: &[(&'static str, bool)]) -> Value {
    let mut order: Vec<&'static str> = Vec::with_capacity(tools.len());
    let chosen = raw
        .and_then(|group| group.get("order"))
        .and_then(Value::as_array);
    for tool in chosen.into_iter().flatten().filter_map(Value::as_str) {
        if let Some(known) = tools.iter().copied().find(|known| *known == tool) {
            if !order.contains(&known) {
                order.push(known);
            }
        }
    }
    for tool in tools {
        if !order.contains(tool) {
            order.push(tool);
        }
    }
    if let Some(at) = order.iter().position(|tool| *tool == "settings") {
        let settings = order.remove(at);
        order.insert(0, settings);
    }
    let chosen_pins = raw
        .and_then(|group| group.get("pinned"))
        .and_then(Value::as_object);
    let mut pinned = Map::new();
    for (tool, default) in pins {
        let chosen = order
            .contains(tool)
            .then(|| {
                chosen_pins
                    .and_then(|pins| pins.get(*tool))
                    .and_then(Value::as_bool)
            })
            .flatten();
        pinned.insert((*tool).into(), chosen.unwrap_or(*default).into());
    }
    json!({"order": order, "pinned": pinned})
}

/// `tool_layout`: the map-editing toolbar's order and pins for its two groups.
/// Any object reads as a complete layout; what it does not name takes the
/// default.
pub(in crate::ui_preferences) fn tool_layout(value: &Value) -> Option<Value> {
    let groups = value.as_object()?.get("groups");
    Some(json!({
        "position": "top-right",
        "groups": {
            "cad.modify": tool_group(groups.and_then(|groups| groups.get("cad.modify")), &MODIFY_TOOLS, &MODIFY_PINS),
            "design.actions": tool_group(groups.and_then(|groups| groups.get("design.actions")), &ACTION_TOOLS, &ACTION_PINS),
        },
    }))
}

pub(in crate::ui_preferences) const MIN_DOCK_WIDTH: f64 = 280.0;
pub(in crate::ui_preferences) const MAX_DOCK_WIDTH: f64 = 900.0;

/// `properties_dock_width`: the right dock's width in whole pixels.
pub(in crate::ui_preferences) fn properties_dock_width(value: &Value) -> Option<Value> {
    let width = value.as_f64()?;
    Some(number(
        js_round(width).clamp(MIN_DOCK_WIDTH, MAX_DOCK_WIDTH),
    ))
}

pub(in crate::ui_preferences) const MIN_UPLOAD_SPLIT: f64 = 45.0;
pub(in crate::ui_preferences) const MAX_UPLOAD_SPLIT: f64 = 80.0;

/// `upload_dialog_layout`: the upload dialog's left pane, in whole percent.
pub(in crate::ui_preferences) fn upload_dialog_layout(value: &Value) -> Option<Value> {
    let split = fields(value, &["splitLeftPercent"])?
        .get("splitLeftPercent")?
        .as_f64()?;
    Some(
        json!({"splitLeftPercent": number(js_round(split).clamp(MIN_UPLOAD_SPLIT, MAX_UPLOAD_SPLIT))}),
    )
}

const MAX_PANEL_COORDINATE: f64 = 100_000.0;

/// `style_center_layout`: the floating Style Center's position and size. The
/// window clamps it into the viewport it opens in.
pub(in crate::ui_preferences) fn style_center_layout(value: &Value) -> Option<Value> {
    let map = fields(value, &["x", "y", "w", "h"])?;
    ranged(map.get("x")?, -MAX_PANEL_COORDINATE, MAX_PANEL_COORDINATE)?;
    ranged(map.get("y")?, -MAX_PANEL_COORDINATE, MAX_PANEL_COORDINATE)?;
    ranged(map.get("w")?, 1.0, MAX_PANEL_COORDINATE)?;
    ranged(map.get("h")?, 1.0, MAX_PANEL_COORDINATE)?;
    Some(json!({"x": map["x"], "y": map["y"], "w": map["w"], "h": map["h"]}))
}

/// The two settings the first launch fills from the device; while unset the
/// application-settings owner keeps deciding them.
const DEVICE_SETTINGS: [&str; 2] = ["popupMode", "showMyLocationControl"];

/// `application_settings`: the operator's record as the application-settings
/// owner normalises it. Unknown and retired keys are dropped, never refused;
/// a device setting never chosen stays unset.
pub(in crate::ui_preferences) fn application_settings(value: &Value) -> Option<Value> {
    let saved = value.as_object()?;
    let mut record = crate::application_settings::normalize_settings(saved);
    for key in DEVICE_SETTINGS {
        if saved.get(key).map_or(true, Value::is_null) {
            record.remove(key);
        }
    }
    Some(Value::Object(record))
}

pub(in crate::ui_preferences) const WORKING_AREA_MODES: [&str; 3] = ["full", "admin", "draw"];
/// The survey read refuses a larger drawn boundary (`survey::hold`).
pub(in crate::ui_preferences) const MAX_BOUNDARY_BYTES: usize = 1024 * 1024;
const MAX_SURVEYORS: usize = 200;
const MAX_ADMIN_TEXT: usize = 256;
const MAX_HIERARCHY_LEVELS: usize = 16;
const WORKING_AREA_FIELDS: [&str; 12] = [
    "mode",
    "admin",
    "drawBoundary",
    "fullProjectLoad",
    "dateFrom",
    "dateTo",
    "surveyors",
    "applied",
    "appliedAt",
    "appliedCenter",
    "appliedZoom",
    "appliedBbox",
];

/// An administrative unit picked in the working-area dialog.
pub(in crate::ui_preferences) fn admin_selection(value: &Value) -> Option<Value> {
    let map = fields(value, &["code", "name", "level", "hierarchy"])?;
    let code = identity(map.get("code")?, MAX_ADMIN_TEXT)?;
    let name = label(map.get("name")?, MAX_ADMIN_TEXT)?;
    let level = label(map.get("level")?, MAX_ADMIN_TEXT)?;
    let hierarchy = map
        .get("hierarchy")?
        .as_object()
        .filter(|levels| levels.len() <= MAX_HIERARCHY_LEVELS)?;
    if !hierarchy
        .iter()
        .all(|(level, unit)| entry(level) && label(unit, MAX_ADMIN_TEXT).is_some())
    {
        return None;
    }
    Some(json!({"code": code, "name": name, "level": level, "hierarchy": hierarchy}))
}

/// A drawn or loaded GeoJSON polygon within the survey read's bound.
pub(in crate::ui_preferences) fn boundary(value: &Value) -> Option<Value> {
    let kind = value.get("type").and_then(Value::as_str)?;
    (matches!(kind, "Polygon" | "MultiPolygon")
        && value.get("coordinates").is_some_and(Value::is_array)
        && stored_len(value) <= MAX_BOUNDARY_BYTES)
        .then(|| value.clone())
}

/// `YYYY-MM-DD`, or empty for no bound.
pub(in crate::ui_preferences) fn date_bound(value: &str) -> bool {
    value.is_empty()
        || time::format_description::parse("[year]-[month]-[day]")
            .is_ok_and(|format| time::Date::parse(value, &format).is_ok())
}

/// A surveyor named by the filter.
pub(in crate::ui_preferences) fn surveyor(value: &Value) -> Option<&str> {
    identity(value, MAX_ADMIN_TEXT)
}

/// `working_area`: the survey working area a project last applied. Only an
/// applied area with a scope is kept: a whole-project load or at least one
/// criterion. The admin unit and drawn boundary are kept only in their own
/// mode, and a whole-project load only in full mode.
pub(in crate::ui_preferences) fn working_area(value: &Value) -> Option<Value> {
    let map = fields(value, &WORKING_AREA_FIELDS)?;
    if map.get("applied")?.as_bool()? != true {
        return None;
    }
    let mode = super::shape::member(map.get("mode")?, &WORKING_AREA_MODES)?;
    let optional = |name: &str| map.get(name).filter(|value| !value.is_null());
    let admin = match optional("admin") {
        Some(admin) => Some(admin_selection(admin)?),
        None => None,
    };
    let drawn = match optional("drawBoundary") {
        Some(polygon) => Some(boundary(polygon)?),
        None => None,
    };
    let full_load = match optional("fullProjectLoad") {
        Some(flag) => flag.as_bool()?,
        None => false,
    };
    let date_from = map
        .get("dateFrom")?
        .as_str()
        .filter(|date| date_bound(date))?;
    let date_to = map
        .get("dateTo")?
        .as_str()
        .filter(|date| date_bound(date))?;
    let surveyors = map
        .get("surveyors")?
        .as_array()
        .filter(|list| list.len() <= MAX_SURVEYORS)?;
    let surveyors = surveyors.iter().map(surveyor).collect::<Option<Vec<_>>>()?;
    let applied_at = millis(map.get("appliedAt")?)?;
    let center = match optional("appliedCenter") {
        Some(center) => {
            finite_tuple::<2>(center)?;
            center.clone()
        }
        None => Value::Null,
    };
    let zoom = match optional("appliedZoom") {
        Some(zoom) => {
            ranged(zoom, -2.0, 30.0)?;
            zoom.clone()
        }
        None => Value::Null,
    };
    let bbox = match optional("appliedBbox") {
        Some(bbox) => {
            finite_tuple::<4>(bbox)?;
            bbox.clone()
        }
        None => Value::Null,
    };
    let admin = if mode == "admin" { admin } else { None };
    let drawn = if mode == "draw" { drawn } else { None };
    let full_load = mode == "full" && full_load;
    let scoped = full_load
        || admin.is_some()
        || drawn.is_some()
        || !date_from.is_empty()
        || !date_to.is_empty()
        || !surveyors.is_empty();
    scoped.then(|| {
        json!({
            "mode": mode,
            "admin": admin,
            "drawBoundary": drawn,
            "fullProjectLoad": full_load,
            "dateFrom": date_from,
            "dateTo": date_to,
            "surveyors": surveyors,
            "applied": true,
            "appliedAt": applied_at,
            "appliedCenter": center,
            "appliedZoom": zoom,
            "appliedBbox": bbox,
        })
    })
}

/// The Profile surface's restart record, as large as a retained generic scene
/// may make it. A scene that would exceed it is not retained.
pub(in crate::ui_preferences) const PROFILE_OCCUPANCY_MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_PROFILE_ID_BYTES: usize = 512;

fn profile_id(value: &Value) -> Option<&str> {
    text(value, MAX_PROFILE_ID_BYTES).filter(|id| !id.trim().is_empty())
}

/// Which camera a model Profile keeps: the mutable local model's, or one
/// immutable project revision's.
fn viewport_identity(value: &Value) -> Option<Value> {
    match value.get("kind")?.as_str()? {
        "local" => fields(value, &["kind"]).map(|_| json!({"kind": "local"})),
        "project-version" => {
            let map = fields(
                value,
                &["kind", "dsProject", "projectModelId", "revisionId"],
            )?;
            Some(json!({
                "kind": "project-version",
                "dsProject": profile_id(map.get("dsProject")?)?,
                "projectModelId": profile_id(map.get("projectModelId")?)?,
                "revisionId": profile_id(map.get("revisionId")?)?,
            }))
        }
        _ => None,
    }
}

/// The one owner of the Profile surface: a DS Grid model or a generic profile.
pub(in crate::ui_preferences) fn profile_occupant(value: &Value) -> Option<Value> {
    match value.get("kind")?.as_str()? {
        "model" => {
            let map = fields(value, &["kind", "modelId", "viewportIdentity"])?;
            let mut out = json!({"kind": "model", "modelId": profile_id(map.get("modelId")?)?});
            if let Some(identity) = map
                .get("viewportIdentity")
                .filter(|identity| !identity.is_null())
            {
                out["viewportIdentity"] = viewport_identity(identity)?;
            }
            Some(out)
        }
        "generic" => {
            let map = fields(value, &["kind", "profileId"])?;
            Some(json!({"kind": "generic", "profileId": profile_id(map.get("profileId")?)?}))
        }
        _ => None,
    }
}

/// `profile_occupancy` (schema 1): who owns the Profile surface, restored at
/// the next start. A generic profile has no session to rebuild from, so its
/// Rust scene projection is retained with it while it fits the bound; a model
/// occupant never carries one.
pub(in crate::ui_preferences) fn profile_occupancy(value: &Value) -> Option<Value> {
    let map = fields(
        value,
        &["schemaVersion", "occupant", "genericScene", "updatedAt"],
    )?;
    if map.get("schemaVersion")?.as_u64()? != 1 {
        return None;
    }
    let occupant = profile_occupant(map.get("occupant")?)?;
    let updated = millis(map.get("updatedAt")?)?;
    let mut out = Map::new();
    out.insert("schemaVersion".into(), 1.into());
    let generic = occupant["kind"] == "generic";
    out.insert("occupant".into(), occupant);
    out.insert("updatedAt".into(), updated.into());
    if let Some(scene) = map.get("genericScene").filter(|scene| !scene.is_null()) {
        if !scene.is_object() {
            return None;
        }
        if generic {
            out.insert("genericScene".into(), scene.clone());
            if stored_len(&Value::Object(out.clone())) > PROFILE_OCCUPANCY_MAX_BYTES {
                out.remove("genericScene");
            }
        }
    }
    Some(Value::Object(out))
}

#[cfg(test)]
#[path = "workbench_tests.rs"]
mod tests;
