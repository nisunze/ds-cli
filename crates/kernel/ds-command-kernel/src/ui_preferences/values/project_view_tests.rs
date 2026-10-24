use super::*;
use serde_json::json;

#[test]
fn layer_workspace_names_its_chapter_and_tabs() {
    assert_eq!(
        layer_workspace(&json!({"workspace": "design", "design": "mv", "local": "ds-grid"})),
        Some(json!({"workspace": "design", "design": "mv", "local": "ds-grid"}))
    );
    assert_eq!(
        layer_workspace(&json!({"workspace": null, "design": "lv", "local": "gis"})),
        Some(json!({"workspace": null, "design": "lv", "local": "gis"}))
    );
    assert!(
        layer_workspace(&json!({"workspace": "solar", "design": "lv", "local": "gis"})).is_none()
    );
    assert!(layer_workspace(&json!({"design": "hv", "local": "gis"})).is_none());
}

#[test]
fn project_workspace_names_a_current_view() {
    let held = json!({"workspace": "project-work", "workView": "gantt"});
    assert_eq!(project_workspace(&held), Some(held));
    assert!(project_workspace(&json!({"workspace": "map", "workView": "dashboard"})).is_none());
    assert!(project_workspace(&json!({"workspace": "solar", "workView": "plan"})).is_none());
}

#[test]
fn board_view_folds_each_column_once() {
    assert_eq!(
        board_view(
            &json!({"grouping": "phase", "density": "compact", "collapsedColumns": ["done", "done", "todo"]})
        ),
        Some(
            json!({"grouping": "phase", "density": "compact", "collapsedColumns": ["done", "todo"]})
        )
    );
    assert!(board_view(
        &json!({"grouping": "phase", "density": "compact", "collapsedColumns": [" "]})
    )
    .is_none());
    assert!(board_view(
        &json!({"grouping": "colour", "density": "compact", "collapsedColumns": []})
    )
    .is_none());
}

#[test]
fn paper_placements_are_finite_points() {
    assert_eq!(
        paper_placements(&json!({"n1": {"x": 10.5, "y": -4}})),
        Some(json!({"n1": {"x": 10.5, "y": -4}}))
    );
    assert!(paper_placements(&json!({"n1": {"x": 1}})).is_none());
    assert!(paper_placements(&json!({"n1": {"x": "1", "y": 2}})).is_none());
    assert!(paper_placements(&json!({"": {"x": 1, "y": 2}})).is_none());
    let many: serde_json::Map<String, Value> = (0..=MAX_PAPER_PLACEMENTS)
        .map(|index| (format!("n{index}"), json!({"x": 0, "y": 0})))
        .collect();
    assert!(paper_placements(&Value::Object(many)).is_none());
}

#[test]
fn paper_choices_are_closed() {
    assert_eq!(paper_note_style(&json!("sticky")), Some(json!("sticky")));
    assert!(paper_note_style(&json!("bold")).is_none());
    assert_eq!(
        paper_collapsed_groups(&json!(["g1", "g2", "g1"])),
        Some(json!(["g1", "g2"]))
    );
    assert!(paper_collapsed_groups(&json!("g1")).is_none());
    let dock = json!({"side": "left", "pinned": false});
    assert_eq!(paper_tool_dock(&dock), Some(dock));
    assert!(paper_tool_dock(&json!({"side": "top", "pinned": true})).is_none());
}
