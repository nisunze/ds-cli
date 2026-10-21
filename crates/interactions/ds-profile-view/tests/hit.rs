use ds_profile_view::{
    camera::Camera,
    hit::{distance, HitIndex},
    model::*,
};
fn scene() -> Scene {
    serde_json::from_value(serde_json::json!({"bounds":{"min_x":0,"max_x":100,"min_y":0,"max_y":100},"bands":[{"alignment_id":"a","ground":[{"points":[{"scene_x":-1000,"scene_y":80},{"scene_x":1000,"scene_y":80}]}],"structures":[{"entity_id":"p","base":{"scene_x":50,"scene_y":80},"top":{"scene_x":50,"scene_y":30},"attachment_points":[{"attachment_point_id":"same","point":{"scene_x":50,"scene_y":30}}]},{"entity_id":"q","base":{"scene_x":80,"scene_y":80},"top":{"scene_x":80,"scene_y":30},"attachment_points":[{"attachment_point_id":"same","point":{"scene_x":80,"scene_y":30}}]}]}]})).unwrap()
}
fn camera(s: &Scene) -> Camera {
    Camera {
        bounds: s.bounds,
        width: 300.,
        height: 300.,
        viewport: Viewport::default(),
    }
}
#[test]
fn distance_handles_degenerate_and_crossing_segments() {
    assert_eq!(
        distance(Point::xy(1., 2.), Point::xy(1., 2.), Point::xy(1., 2.)),
        0.
    );
    assert_eq!(
        distance(Point::xy(1., 2.), Point::xy(-100., 2.), Point::xy(100., 2.)),
        0.
    );
}
#[test]
fn picks_structure_between_endpoints() {
    let s = scene();
    let c = camera(&s);
    let hits = HitIndex::new(&s).pick(
        &c,
        c.project(Point::xy(50., 55.)),
        &Visibility::default(),
        12.,
    );
    assert_eq!(hits[0].entity_id, "p");
}
#[test]
fn crossing_ground_remains_pickable_when_endpoints_are_offscreen() {
    let s = scene();
    let c = camera(&s);
    let hits = HitIndex::new(&s).pick(
        &c,
        c.project(Point::xy(20., 80.)),
        &Visibility::default(),
        12.,
    );
    assert_eq!(hits[0].entity_id, "a");
}
#[test]
fn same_attachment_point_id_is_scoped_to_structure() {
    let s = scene();
    let c = camera(&s);
    let index = HitIndex::new(&s);
    let a = index.pick(
        &c,
        c.project(Point::xy(50., 30.)),
        &Visibility::default(),
        12.,
    );
    let b = index.pick(
        &c,
        c.project(Point::xy(80., 30.)),
        &Visibility::default(),
        12.,
    );
    assert_eq!(a[0].entity_id, "p\0same");
    assert_eq!(b[0].entity_id, "q\0same");
    assert_eq!(a[0].attachment_ref.as_ref().unwrap().structure_id, "p");
}
#[test]
fn hidden_entities_are_excluded() {
    let s = scene();
    let c = camera(&s);
    let visibility = Visibility {
        show_ground: false,
        show_structures: false,
        show_attachments: false,
        ..Visibility::default()
    };
    assert!(HitIndex::new(&s)
        .pick(&c, c.project(Point::xy(50., 55.)), &visibility, 12.)
        .is_empty());
}
#[test]
fn empty_profile_is_an_admitted_control() {
    let s = Scene::default();
    assert!(s.validate().is_ok());
    assert!(HitIndex::new(&s)
        .pick(
            &camera(&s),
            Screen { x: 0., y: 0. },
            &Visibility::default(),
            12.
        )
        .is_empty());
}
#[test]
fn malformed_bounds_refuse() {
    let mut s = scene();
    s.bounds.min_x = 200.;
    assert_eq!(s.validate().unwrap_err(), "profile_view_invalid_bounds");
}
