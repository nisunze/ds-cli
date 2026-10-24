//! One project's presentation choices: the layer drawer's chapter, the
//! primary workspace and Project Work view a restart returns to, the board's
//! grouping and density, and the Plan sheet's placements, note style, snap,
//! folded groups and tool rail.
use super::shape::{entry, fields, member, number, ranged};
use super::window::LAYER_GROUPS;
use serde_json::{json, Map, Value};

pub(in crate::ui_preferences) const DESIGN_WORKSPACES: [&str; 3] = ["lv", "mv", "pm"];
pub(in crate::ui_preferences) const LOCAL_WORKSPACES: [&str; 2] = ["gis", "ds-grid"];

/// `layer_workspace`: the drawer chapter open in this project (none until one
/// is chosen) and the Design and Local tab inside it.
pub(in crate::ui_preferences) fn layer_workspace(value: &Value) -> Option<Value> {
    let map = fields(value, &["workspace", "design", "local"])?;
    let workspace = match map.get("workspace").filter(|chapter| !chapter.is_null()) {
        Some(chapter) => Value::from(member(chapter, &LAYER_GROUPS)?),
        None => Value::Null,
    };
    Some(json!({
        "workspace": workspace,
        "design": member(map.get("design")?, &DESIGN_WORKSPACES)?,
        "local": member(map.get("local")?, &LOCAL_WORKSPACES)?,
    }))
}

pub(in crate::ui_preferences) const MAIN_WORKSPACES: [&str; 2] = ["map", "project-work"];
pub(in crate::ui_preferences) const WORK_VIEWS: [&str; 7] = [
    "plan", "board", "gantt", "records", "messages", "notes", "help",
];

/// `project_workspace`: the primary workspace and Project Work view a new
/// window of this project returns to.
pub(in crate::ui_preferences) fn project_workspace(value: &Value) -> Option<Value> {
    let map = fields(value, &["workspace", "workView"])?;
    Some(json!({
        "workspace": member(map.get("workspace")?, &MAIN_WORKSPACES)?,
        "workView": member(map.get("workView")?, &WORK_VIEWS)?,
    }))
}

pub(in crate::ui_preferences) const BOARD_GROUPINGS: [&str; 4] =
    ["status", "phase", "assignee", "priority"];
pub(in crate::ui_preferences) const DENSITIES: [&str; 2] = ["compact", "comfortable"];
const MAX_FOLDED_COLUMNS: usize = 256;
pub(in crate::ui_preferences) const MAX_FOLDED_GROUPS: usize = 2000;

/// Distinct non-blank entries in their first order, at most `max`.
fn distinct(value: &Value, max: usize) -> Option<Vec<&str>> {
    let mut kept: Vec<&str> = Vec::new();
    for item in value.as_array()? {
        let item = item
            .as_str()
            .filter(|item| entry(item) && !item.trim().is_empty())?;
        if !kept.contains(&item) {
            kept.push(item);
        }
    }
    (kept.len() <= max).then_some(kept)
}

/// `board_view`: the board's grouping, density and folded columns.
pub(in crate::ui_preferences) fn board_view(value: &Value) -> Option<Value> {
    let map = fields(value, &["grouping", "density", "collapsedColumns"])?;
    Some(json!({
        "grouping": member(map.get("grouping")?, &BOARD_GROUPINGS)?,
        "density": member(map.get("density")?, &DENSITIES)?,
        "collapsedColumns": distinct(map.get("collapsedColumns")?, MAX_FOLDED_COLUMNS)?,
    }))
}

pub(in crate::ui_preferences) const MAX_PAPER_PLACEMENTS: usize = 5000;
const MAX_PAPER_COORDINATE: f64 = 1.0e9;

/// `paper_placements`: where each root note sits on the Plan sheet.
pub(in crate::ui_preferences) fn paper_placements(value: &Value) -> Option<Value> {
    let map = value
        .as_object()
        .filter(|map| map.len() <= MAX_PAPER_PLACEMENTS)?;
    let mut out = Map::new();
    for (id, point) in map {
        if !entry(id) {
            return None;
        }
        let point = fields(point, &["x", "y"])?;
        let x = ranged(point.get("x")?, -MAX_PAPER_COORDINATE, MAX_PAPER_COORDINATE)?;
        let y = ranged(point.get("y")?, -MAX_PAPER_COORDINATE, MAX_PAPER_COORDINATE)?;
        out.insert(id.clone(), json!({"x": number(x), "y": number(y)}));
    }
    Some(Value::Object(out))
}

pub(in crate::ui_preferences) const NOTE_STYLES: [&str; 3] = ["card", "sticky", "plain"];

/// `paper_note_style`: how the Plan sheet draws its notes.
pub(in crate::ui_preferences) fn paper_note_style(value: &Value) -> Option<Value> {
    member(value, &NOTE_STYLES).map(Value::from)
}

/// `paper_collapsed_groups`: the Plan sheet's folded groups.
pub(in crate::ui_preferences) fn paper_collapsed_groups(value: &Value) -> Option<Value> {
    distinct(value, MAX_FOLDED_GROUPS).map(|ids| json!(ids))
}

/// `paper_tool_dock`: the Plan sheet's tool rail side and pin.
pub(in crate::ui_preferences) fn paper_tool_dock(value: &Value) -> Option<Value> {
    let map = fields(value, &["side", "pinned"])?;
    Some(json!({
        "side": member(map.get("side")?, &["left", "right"])?,
        "pinned": map.get("pinned")?.as_bool()?,
    }))
}

#[cfg(test)]
#[path = "project_view_tests.rs"]
mod tests;
