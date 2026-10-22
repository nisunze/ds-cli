use ds_profile_view::{
    draw_geometry::span_label_anchor,
    labels::{station_rows, station_text},
    model::{Point, Scene},
};
use serde_json::json;

#[test]
fn measured_station_rows_reserve_exact_ends_without_inventing_ticks() {
    let measured: Vec<_> = [0., 18., 36., 54., 72., 90.]
        .into_iter()
        .map(|x| (x, 42., false))
        .collect();
    assert_eq!(station_rows(&measured), [(0, 0), (1, 1), (4, 1), (5, 0)]);
    assert!(station_rows(&[]).is_empty());
    assert_eq!(
        station_rows(&[(0., 42., false), (48., 42., false)]),
        [(0, 0), (1, 0)]
    );
}

#[test]
fn exact_kilometre_labels_round_before_splitting() {
    assert_eq!(station_text(0.), "0+000.0");
    assert_eq!(station_text(4999.999999999), "5+000.0");
    assert_eq!(station_text(-1.), "0+000.0");
    assert_eq!(station_text(18.25), "0+018.3");
}

#[test]
fn span_annotations_use_only_covering_native_guides() {
    let authored = Point::xy(50., 20.);
    let ground = json!({"kind":"ground_offset","segments":[{"points":[
        {"scene_x":0.,"scene_y":80.},{"scene_x":100.,"scene_y":100.}
    ]}]});
    let wire = json!({"kind":"lowest_wire_drop","segments":[{"points":[
        {"scene_x":0.,"scene_y":40.},{"scene_x":100.,"scene_y":60.}
    ]}]});
    for (guides, expected) in [
        (json!([ground.clone(), wire]), 70.),
        (json!([ground]), 55.),
        (json!([]), 20.),
    ] {
        let scene: Scene = serde_json::from_value(json!({"clearance_guides":guides})).unwrap();
        let before = scene.clone();
        let anchor = span_label_anchor(&scene, authored);
        assert_eq!((anchor.scene_x, anchor.scene_y), (50., expected));
        assert_eq!(scene, before);
    }
    let gap: Scene = serde_json::from_value(json!({"clearance_guides":[{
        "kind":"ground_offset","segments":[{"points":[{"scene_x":0.,"scene_y":80.},{"scene_x":10.,"scene_y":100.}]}]
    }]})).unwrap();
    assert_eq!(span_label_anchor(&gap, authored), authored);
}
