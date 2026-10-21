use ds_profile_view::{
    camera::Camera,
    model::{Bounds, Point, Screen, Viewport},
};
fn camera() -> Camera {
    Camera {
        bounds: Bounds {
            min_x: 0.,
            min_y: 0.,
            max_x: 10000.,
            max_y: 100.,
        },
        width: 1200.,
        height: 600.,
        viewport: Viewport::default(),
    }
}
#[test]
fn projected_point_roundtrips_on_long_profile() {
    let c = camera();
    let p = Point::xy(7234.125, 53.25);
    let p2 = c.unproject(c.project(p));
    assert!((p2.scene_x - p.scene_x).abs() < 1e-9);
    assert!((p2.scene_y - p.scene_y).abs() < 1e-9);
    assert!(c.projection().scale < 1.);
}
#[test]
fn anchor_zoom_preserves_world_point() {
    let mut c = camera();
    let anchor = Screen { x: 800., y: 450. };
    let point = c.unproject(anchor);
    c.viewport = c.zoom(anchor, 7.);
    let p = c.project(point);
    assert!((p.x - anchor.x).abs() < 1e-9);
    assert!((p.y - anchor.y).abs() < 1e-9);
}
#[test]
fn offscreen_camera_rebases_to_selected() {
    let mut c = camera();
    c.viewport.pan_x = 1e6;
    let point = Point::xy(4000., 20.);
    c.viewport = c.rebase(Some(point));
    let p = c.project(point);
    assert!((p.x - c.width / 2.).abs() < 1e-9);
    assert!((p.y - c.height / 2.).abs() < 1e-9);
}
#[test]
fn visible_camera_is_retained() {
    let c = camera();
    assert_eq!(c.rebase(None), c.viewport);
}
#[test]
fn reversed_zoom_window_has_same_camera() {
    let c = camera();
    let a = Screen { x: 20., y: 30. };
    let b = Screen { x: 300., y: 400. };
    assert_eq!(c.window(a, b).unwrap(), c.window(b, a).unwrap());
}
#[test]
fn zero_area_zoom_window_refuses() {
    let c = camera();
    let p = Screen { x: 20., y: 30. };
    assert!(c.window(p, p).is_err());
}
#[test]
fn nan_zoom_does_not_poison_camera() {
    let c = camera();
    assert_eq!(c.zoom(Screen { x: 30., y: 20. }, f64::NAN), c.viewport);
}
