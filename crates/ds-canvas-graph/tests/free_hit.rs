use ds_canvas_graph::{
    GeometryError, Point,
    free_hit::{FreeCards, PortEnd},
};

fn cards(rects: &[f64], parents: &[i32]) -> FreeCards {
    FreeCards::new(rects, parents).unwrap()
}

#[test]
fn body_hit_is_inclusive_and_last_painted_wins() {
    let scene = cards(&[0., 0., 100., 60., 100., 0., 100., 60.], &[-1, -1]);
    assert_eq!(scene.hit(Point::new(0., 0.)).unwrap(), Some(0));
    assert_eq!(scene.hit(Point::new(100., 60.)).unwrap(), Some(1));
    assert_eq!(scene.hit(Point::new(200., 60.)).unwrap(), Some(1));
    assert_eq!(scene.hit(Point::new(200.0001, 60.)).unwrap(), None);
}

#[test]
fn port_size_and_visibility_share_the_exact_screen_threshold() {
    let scene = cards(&[10., 20., 18., 12.], &[-1]);
    let successor = scene.port(0, PortEnd::Successor, 1.).unwrap();
    assert_eq!(successor.anchor, Point::new(28., 26.));
    assert_eq!(successor.radius, 6.);
    assert!(successor.visible);
    assert_eq!(
        scene.port(0, PortEnd::Predecessor, 1.).unwrap().anchor,
        Point::new(10., 26.)
    );
    assert!(scene.port(0, PortEnd::Successor, 0.5).unwrap().visible);
    assert!(
        !scene
            .port(0, PortEnd::Successor, 0.499_999)
            .unwrap()
            .visible
    );
    assert_eq!(scene.port(0, PortEnd::Successor, 2.).unwrap().radius, 3.);
    for zoom in [0., -1., f64::NAN, f64::INFINITY] {
        let port = scene.port(0, PortEnd::Successor, zoom).unwrap();
        assert_eq!(port.radius, 0.);
        assert!(!port.visible);
        assert_eq!(scene.hit_port(port.anchor, zoom, 6.).unwrap(), None);
    }
}

#[test]
fn hidden_ports_never_steal_cards_and_coarse_targets_only_expand_visible_ports() {
    let scene = cards(&[0., 0., 192., 68.], &[-1]);
    let near = Point::new(178., 34.);
    assert_eq!(scene.hit_port(near, 1., 6.).unwrap(), None);
    assert_eq!(
        scene.hit_port(near, 1., 16.).unwrap().unwrap().end,
        PortEnd::Successor
    );
    assert_eq!(
        scene.hit_port(Point::new(192., 34.), 0.02, 100.).unwrap(),
        None
    );
    assert_eq!(scene.hit(Point::new(96., 34.)).unwrap(), Some(0));
    let short = cards(&[0., 0., 18., 12., 0., 0., 18., 12.], &[-1, -1]);
    let hit = short
        .hit_port(Point::new(9., 6.), 1., 10.)
        .unwrap()
        .unwrap();
    assert_eq!(hit.index, 1);
    assert_eq!(
        hit.end,
        PortEnd::Successor,
        "right port wins an equal-distance tie"
    );
    assert_eq!(
        short
            .hit_port(Point::new(28., 6.), 1., 10.)
            .unwrap()
            .unwrap()
            .index,
        1
    );
    assert_eq!(
        short.hit_port(Point::new(28.001, 6.), 1., 10.).unwrap(),
        None
    );
}

#[test]
fn boundaries_keep_direction_center_fallback_and_small_card_floor() {
    let scene = cards(&[10., 20., 100., 40., 0., 0., 1., 1.], &[-1, -1]);
    assert_eq!(
        scene.boundary(0, Point::new(60., 40.)).unwrap(),
        Point::new(110., 40.)
    );
    assert_eq!(
        scene.boundary(0, Point::new(60., 500.)).unwrap(),
        Point::new(60., 60.)
    );
    assert_eq!(
        scene.boundary(0, Point::new(-500., 40.)).unwrap(),
        Point::new(10., 40.)
    );
    assert_eq!(
        scene.boundary(0, Point::new(160., 140.)).unwrap(),
        Point::new(80., 60.)
    );
    assert_eq!(
        scene.boundary(1, Point::new(2., 0.5)).unwrap(),
        Point::new(1.5, 0.5)
    );
}

#[test]
fn drops_exclude_self_and_all_descendants_and_keep_topmost_other_card() {
    let scene = cards(
        &[
            0., 0., 200., 200., 20., 20., 100., 100., 30., 30., 50., 50., 300., 0., 40., 40.,
        ],
        &[-1, 0, 1, -1],
    );
    assert_eq!(
        scene.drop_target(Point::new(35., 35.), Some(0)).unwrap(),
        None
    );
    assert_eq!(
        scene.drop_target(Point::new(35., 35.), Some(1)).unwrap(),
        Some(0)
    );
    assert_eq!(
        scene.drop_target(Point::new(35., 35.), Some(3)).unwrap(),
        Some(2)
    );
    assert_eq!(
        scene.drop_target(Point::new(35., 35.), None).unwrap(),
        Some(2)
    );
    assert_eq!(
        scene.drop_target(Point::new(-1., 0.), Some(0)).unwrap(),
        None
    );
}

#[test]
fn cyclic_and_external_parent_ids_are_preserved_without_recursion() {
    let cyclic = cards(&[0., 0., 100., 100., 10., 10., 80., 80.], &[1, 0]);
    assert_eq!(
        cyclic.drop_target(Point::new(20., 20.), Some(0)).unwrap(),
        None
    );
    let orphan = cards(&[0., 0., 100., 100., 10., 10., 80., 80.], &[2, 0, -1]);
    assert_eq!(
        orphan.drop_target(Point::new(20., 20.), Some(2)).unwrap(),
        None
    );
    assert_eq!(orphan.hit(Point::new(20., 20.)).unwrap(), Some(1));
}

#[test]
fn degenerate_finite_rectangles_preserve_body_hit_without_visible_ports() {
    let scene = cards(&[0., 0., 0., 0.], &[-1]);
    assert_eq!(scene.hit(Point::new(0., 0.)).unwrap(), Some(0));
    assert!(!scene.port(0, PortEnd::Successor, 1.).unwrap().visible);
}

#[test]
fn malformed_geometry_queries_and_indices_carry_typed_refusals() {
    assert!(matches!(
        FreeCards::new(&[0., 0., 1.], &[-1]),
        Err(GeometryError::InvalidBuffer { .. })
    ));
    assert!(matches!(
        FreeCards::new(&[0., 0., 1., 1.], &[]),
        Err(GeometryError::InvalidBuffer { .. })
    ));
    assert!(matches!(
        FreeCards::new(&[0., 0., 1., 1.], &[2]),
        Err(GeometryError::InvalidBuffer { .. })
    ));
    assert!(matches!(
        FreeCards::new(&[f64::NAN, 0., 1., 1.], &[-1]),
        Err(GeometryError::NonFiniteInput { .. })
    ));
    assert!(matches!(
        FreeCards::new(&[f64::MAX, 0., f64::MAX, 1.], &[-1]),
        Err(GeometryError::NonFiniteInput { .. })
    ));
    assert!(matches!(
        FreeCards::new(&[0., 0., -1., 1.], &[-1]),
        Err(GeometryError::InvalidMetric { .. })
    ));
    let scene = cards(&[0., 0., 10., 10.], &[-1]);
    assert!(matches!(
        scene.hit(Point::new(f64::INFINITY, 0.)),
        Err(GeometryError::NonFiniteInput { .. })
    ));
    assert!(matches!(
        scene.boundary(0, Point::new(0., f64::NAN)),
        Err(GeometryError::NonFiniteInput { .. })
    ));
    assert!(matches!(
        scene.port(1, PortEnd::Successor, 1.),
        Err(GeometryError::UnknownNode { .. })
    ));
    assert!(matches!(
        scene.drop_target(Point::new(0., 0.), Some(1)),
        Err(GeometryError::UnknownNode { .. })
    ));
    assert!(matches!(
        scene.hit_port(Point::new(0., 0.), 1., -1.),
        Err(GeometryError::InvalidMetric { .. })
    ));
    assert!(matches!(
        PortEnd::try_from(2),
        Err(GeometryError::InvalidMetric { .. })
    ));
}
