use super::*;

#[test]
fn lv_domain_has_id_lv() {
    assert_eq!(LvDomain::new().id(), "lv");
}

#[test]
fn mv_domain_has_id_mv() {
    assert_eq!(MvDomain::new().id(), "mv");
}

#[test]
fn temp_domain_has_id_temp() {
    assert_eq!(TempDomain::new().id(), "temp");
}

#[test]
fn lv_pick_priority_prefers_point_over_line() {
    let d = LvDomain::new();
    let pt = d.pick_priority().score(
        &Value::Null,
        &PickContext {
            layer_id: "lv_poles",
            geometry_kind: GeometryKind::Point,
        },
    );
    let ln = d.pick_priority().score(
        &Value::Null,
        &PickContext {
            layer_id: "lv_lines",
            geometry_kind: GeometryKind::LineString,
        },
    );
    assert!(pt > ln);
}
