use ds_profile_view::{model::*, selection::*};
#[test]
fn range_uses_published_native_order() {
    let mut s = Scene::default();
    s.selection_order.structures = vec!["z".into(), "a".into(), "b".into()];
    assert_eq!(range(&s, "b", "z"), vec!["z", "a", "b"]);
}
#[test]
fn additive_range_preserves_existing_ids() {
    let (ids, primary) = choose(
        &["old".into()],
        Some("b"),
        Some(&["a".into(), "b".into()]),
        Modifiers {
            toggle: true,
            range: true,
            cycle: false,
        },
    );
    assert_eq!(ids, vec!["old", "a", "b"]);
    assert_eq!(primary.as_deref(), Some("b"));
}
#[test]
fn toggle_removes_only_clicked_identity() {
    let (ids, primary) = choose(
        &["a".into(), "b".into()],
        Some("b"),
        None,
        Modifiers {
            toggle: true,
            ..Modifiers::default()
        },
    );
    assert_eq!(ids, vec!["a"]);
    assert_eq!(primary.as_deref(), Some("a"));
}
#[test]
fn ground_drag_uses_native_vertical_axis_and_clamps() {
    let axis = Axis {
        vertical_px_per_m: 4.,
        domain_min_m: 100.,
        domain_max_m: 200.,
        ..Axis::default()
    };
    assert_eq!(ground_candidate(150., -80., 2., &axis), Some((160., false)));
    assert_eq!(ground_candidate(150., -800., 2., &axis), Some((200., true)));
}
#[test]
fn invalid_axis_does_not_emit_move() {
    assert!(ground_candidate(10., 2., 0., &Axis::default()).is_none());
    assert!(station_candidate(10., 2., 1., &StationAxis::default()).is_none());
}
#[test]
fn station_drag_converts_delta_without_model_placement() {
    let axis = StationAxis {
        minimum_chain_station_m: 300.,
        maximum_chain_station_m: 800.,
        minimum_scene_x: 50.,
        maximum_scene_x: 150.,
    };
    assert_eq!(station_candidate(100., 40., 2., &axis), Some(200.));
}
