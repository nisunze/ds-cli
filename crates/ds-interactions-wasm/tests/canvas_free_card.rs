#![cfg(not(target_arch = "wasm32"))]
//! Numeric wrapper parity and exact native refusal envelopes.
use ds_canvas_graph::{
    Point,
    free_hit::{FreeCards, PortEnd},
};
use ds_interactions_wasm::CanvasFreeCardGeometry;

#[test]
fn packed_free_card_results_are_the_native_answers() {
    let rects = [10., 20., 192., 68., 20., 30., 40., 20.];
    let parents = [-1, 0];
    let native = FreeCards::new(&rects, &parents).unwrap();
    let wire = CanvasFreeCardGeometry::new(&rects, &parents).unwrap();
    for zoom in [0.02, 0.5, 1., 8.] {
        let port = native.port(0, PortEnd::Successor, zoom).unwrap();
        assert_eq!(
            wire.port(0, 0, zoom).unwrap(),
            vec![
                port.anchor.x,
                port.anchor.y,
                port.radius,
                if port.visible { 1. } else { 0. }
            ]
        );
    }
    let boundary = native.boundary(0, Point::new(500., 400.)).unwrap();
    assert_eq!(
        wire.boundary(0, 500., 400.).unwrap(),
        vec![boundary.x, boundary.y]
    );
    assert_eq!(
        wire.hit(30., 40.).unwrap(),
        native.hit(Point::new(30., 40.)).unwrap().unwrap() as i32
    );
    assert_eq!(wire.hit_port(202., 54., 1., Some(6.)).unwrap(), vec![0, 0]);
    assert_eq!(
        wire.hit_port(-100., -100., 1., None).unwrap(),
        Vec::<i32>::new()
    );
    assert_eq!(wire.drop_target(30., 40., 0).unwrap(), -1);
    assert_eq!(wire.drop_target(30., 40., 1).unwrap(), 0);
}

#[test]
fn malformed_numeric_inputs_keep_the_native_error_code() {
    let error = CanvasFreeCardGeometry::new(&[0., 0., f64::NAN, 10.], &[-1])
        .err()
        .unwrap();
    let envelope: serde_json::Value = serde_json::from_str(&error.to_json()).unwrap();
    assert_eq!(envelope["code"], "canvas_geometry_non_finite_input");
    assert_eq!(envelope["details"]["field"], "cards[0].width");
    let wire = CanvasFreeCardGeometry::new(&[0., 0., 100., 100.], &[-1]).unwrap();
    let error = wire.port(0, 9, 1.).unwrap_err();
    let envelope: serde_json::Value = serde_json::from_str(&error.to_json()).unwrap();
    assert_eq!(envelope["code"], "canvas_geometry_invalid_metric");
}
