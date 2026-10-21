use ds_profile_view::{model::*, preview::*};
fn scene() -> Scene {
    serde_json::from_value(serde_json::json!({"schema_version":19,"model_revision":"r1","bounds":{"min_x":0,"max_x":100,"min_y":0,"max_y":100},"axis_pin":{"digest":"axis"},"bands":[{"alignment_id":"a","structures":[{"entity_id":"old","ground":{"scene_x":10,"scene_y":30}}]}]})).unwrap()
}
fn preview() -> GeometryPreview {
    serde_json::from_value(serde_json::json!({"patch":{"outcome":"patched","schema_version":19,"model_revision":"r1","axis_pin":{"digest":"axis"},"provisional":true,"geometry_only":true,"structures":[{"alignment_id":"a","structure":{"entity_id":"old","ground":{"scene_x":20,"scene_y":30}}}]}})).unwrap()
}
#[test]
fn exact_native_patch_replaces_only_view_geometry() {
    let s = scene();
    let next = project(&s, Some(&preview())).unwrap();
    assert_eq!(s.bands[0].structures[0].ground.scene_x, 10.);
    assert_eq!(next.bands[0].structures[0].ground.scene_x, 20.);
    assert!(next.provisional_geometry);
    assert!(!next.current());
}
#[test]
fn every_native_patch_pin_is_required() {
    let s = scene();
    for case in 0..5 {
        let mut p = preview();
        let patch = p.patch.as_mut().unwrap();
        match case {
            0 => patch.schema_version += 1,
            1 => patch.model_revision = "r2".into(),
            2 => patch.axis_pin.digest = "different".into(),
            3 => patch.provisional = false,
            _ => patch.geometry_only = false,
        };
        assert!(project(&s, Some(&p)).is_none(), "case {case}");
    }
}
#[test]
fn preview_may_not_carry_solved_engineering() {
    let s = scene();
    for case in 0..4 {
        let mut p = preview();
        let patch = p.patch.as_mut().unwrap();
        match case {
            0 => patch.spans.push(Span::default()),
            1 => patch.evidence.curve_unavailable.push(serde_json::json!({})),
            2 => patch.evidence.section_clearance.push(serde_json::json!({})),
            _ => patch.evidence.section_basis.push(serde_json::json!({})),
        };
        assert!(project(&s, Some(&p)).is_none());
    }
}
#[test]
fn absent_support_is_not_silently_inserted() {
    let s = scene();
    let mut p = preview();
    p.patch.as_mut().unwrap().structures[0].structure.entity_id = "new".into();
    let next = project(&s, Some(&p)).unwrap();
    assert_eq!(next.bands[0].structures.len(), 1);
    assert_eq!(next.bands[0].structures[0].entity_id, "old");
}
#[test]
fn explicit_created_support_is_inserted_only_on_its_native_band() {
    let s = scene();
    let mut p = preview();
    p.patch.as_mut().unwrap().structures[0].structure.entity_id = "new".into();
    p.created_structure_id = Some("new".into());
    let next = project(&s, Some(&p)).unwrap();
    assert_eq!(next.bands[0].structures.len(), 2);
    p.patch.as_mut().unwrap().structures[0].alignment_id = "unknown".into();
    assert!(project(&s, Some(&p)).is_none());
}
#[test]
fn reconcile_required_is_an_ordinary_control() {
    let s = scene();
    let p: GeometryPreview = serde_json::from_value(
        serde_json::json!({"patch":{"outcome":"reconcile_required","reason":"new axis"}}),
    )
    .unwrap();
    assert!(project(&s, Some(&p)).is_none());
}
