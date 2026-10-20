use super::*;
use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
use serde_json::json;

fn snapshot(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

fn admin() -> Map<String, Value> {
    snapshot(json!({
        "user": {"email": "Editor@test.com"},
        "app": {"roles": ["platform_admin"]},
        "active_project": {"project_id": "proj1", "roles": ["gis_design_editor"]},
        "capabilities": {"styles.edit": true, "forms.edit": true, "design.edit": true,
            "pipeline.run": true, "reports.export": true, "ui.dev": true}
    }))
}

fn collapsed() -> Map<String, Value> {
    snapshot(json!({
        "user": {"email": "editor@test.com "},
        "app": {"roles": ["standard_user"]},
        "active_project": {"project_id": "proj1", "roles": ["survey_viewer"]},
        "capabilities": {"styles.edit": false, "entries.create": true}
    }))
}

#[test]
fn a_collapse_to_the_baseline_for_the_same_person_is_suspicious() {
    assert!(suspicious_downgrade(&admin(), &collapsed(), Some("proj1")));
    let mut other = collapsed();
    other.insert("user".into(), json!({"email": "someone@test.com"}));
    assert!(!suspicious_downgrade(&admin(), &other, Some("proj1")));
    let mut refreshed = admin();
    refreshed["capabilities"]["ui.dev"] = json!(false);
    assert!(
        !suspicious_downgrade(&admin(), &refreshed, Some("proj1")),
        "fewer grants with the same roles is a real change"
    );
}

#[test]
fn losing_the_project_roles_with_an_edit_right_is_suspicious() {
    let mut dropped = admin();
    dropped["active_project"]["roles"] = json!([]);
    dropped["capabilities"]["design.edit"] = json!(false);
    assert!(suspicious_downgrade(&admin(), &dropped, Some("proj1")));
    assert!(!suspicious_downgrade(&admin(), &dropped, Some("proj2")), "another project");
}

fn answer(capabilities: Map<String, Value>) -> Value {
    json!({"success": true, "data": capabilities})
}

#[test]
fn a_fresh_snapshot_is_held_and_a_collapse_keeps_the_held_one() {
    let edge = TestEdge::new();
    assert_eq!(block_on(held(&edge, Some("proj1"))).unwrap(), None);
    edge.json(answer(admin()));
    assert_eq!(block_on(refresh(&edge, Some("proj1"))).unwrap(), admin());
    assert_eq!(block_on(held(&edge, Some("proj1"))).unwrap(), Some(admin()));
    assert_eq!(block_on(held(&edge, None)).unwrap(), None, "the platform snapshot is apart");

    edge.json(answer(collapsed()));
    assert_eq!(block_on(refresh(&edge, Some("proj1"))).unwrap(), admin());
    assert_eq!(block_on(held(&edge, Some("proj1"))).unwrap(), Some(admin()));

    edge.sign_in("someone_else");
    assert_eq!(block_on(held(&edge, Some("proj1"))).unwrap(), None, "never another account's");
}
