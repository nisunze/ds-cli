//! Placement, routing, tethers and bounds, in both orientations.
//!
//! The numbers below are computed by hand from the authored cell and the
//! default metrics. That is the point of an authored layout: a reviewer can
//! check the picture arithmetically instead of running it.

mod common;

use common::*;
use ds_canvas_graph::{
    Curve, CurveSegment, EdgeShape, GraphInput, LayoutMetrics, Orientation, Point, PortSide,
    build_scene,
};

fn polyline(curve: &Curve) -> Vec<Point> {
    curve.flatten(16)
}

#[test]
fn horizontal_places_columns_along_x_and_rows_along_y() {
    let scene = build_scene(&chapter(Orientation::Horizontal)).unwrap();

    let a = scene.node("a").unwrap();
    close(a.rect.x, 0.0);
    close(a.rect.y, 0.0);
    close(a.rect.width, 212.0);
    close(a.rect.height, 64.0);

    close(scene.node("b").unwrap().rect.x, FLOW_STEP_H);
    close(scene.node("b").unwrap().rect.y, 0.0);
    close(scene.node("c").unwrap().rect.x, FLOW_STEP_H);
    close(scene.node("c").unwrap().rect.y, CROSS_STEP_H);
    close(scene.node("d").unwrap().rect.x, 0.0);
    close(scene.node("d").unwrap().rect.y, CROSS_STEP_H);
}

#[test]
fn vertical_swaps_the_axes_without_re_authoring_the_graph() {
    let scene = build_scene(&chapter(Orientation::Vertical)).unwrap();

    close(scene.node("a").unwrap().rect.x, 0.0);
    close(scene.node("a").unwrap().rect.y, 0.0);
    // column advances down, row advances across.
    close(scene.node("b").unwrap().rect.x, 0.0);
    close(scene.node("b").unwrap().rect.y, FLOW_STEP_V);
    close(scene.node("c").unwrap().rect.x, CROSS_STEP_V);
    close(scene.node("c").unwrap().rect.y, FLOW_STEP_V);
    close(scene.node("d").unwrap().rect.x, CROSS_STEP_V);
    close(scene.node("d").unwrap().rect.y, 0.0);

    // Node size is not rotated: a box is a box in both orientations.
    close(scene.node("c").unwrap().rect.width, 212.0);
    close(scene.node("c").unwrap().rect.height, 64.0);
}

#[test]
fn a_same_lane_flow_edge_collapses_to_one_straight_segment() {
    let scene = build_scene(&chapter(Orientation::Horizontal)).unwrap();
    let route = &scene.edges[scene.edge_index_of("a-b").unwrap()];

    assert_eq!(route.polyline.len(), 2, "collinear corners are removed");
    close(route.polyline[0].x, 212.0);
    close(route.polyline[0].y, 32.0);
    close(route.polyline[1].x, FLOW_STEP_H);
    close(route.polyline[1].y, 32.0);
    close(route.label.x, (212.0 + FLOW_STEP_H) / 2.0);
    close(route.label.y, 32.0);
}

#[test]
fn a_cross_lane_flow_edge_is_a_z_elbow_at_the_mid_gap() {
    let scene = build_scene(&chapter(Orientation::Horizontal)).unwrap();
    let route = &scene.edges[scene.edge_index_of("a-c").unwrap()];

    let mid = (212.0 + FLOW_STEP_H) / 2.0;
    let expected = [
        Point::new(212.0, 32.0),
        Point::new(mid, 32.0),
        Point::new(mid, CROSS_STEP_H + 32.0),
        Point::new(FLOW_STEP_H, CROSS_STEP_H + 32.0),
    ];
    assert_eq!(route.polyline.len(), expected.len());
    for (actual, expected) in route.polyline.iter().zip(expected) {
        close(actual.x, expected.x);
        close(actual.y, expected.y);
    }
    // The label lands on the middle of the connecting leg.
    close(route.label.x, mid);
    close(route.label.y, (32.0 + CROSS_STEP_H + 32.0) / 2.0);
}

#[test]
fn a_same_flow_step_edge_sidesteps_to_a_rail_outside_both_boxes() {
    let scene = build_scene(&chapter(Orientation::Horizontal)).unwrap();
    let route = &scene.edges[scene.edge_index_of("a-d").unwrap()];

    let rail = 212.0 + 92.0 / 3.0;
    let expected = [
        Point::new(106.0, 64.0),
        Point::new(106.0, 78.0),
        Point::new(rail, 78.0),
        Point::new(rail, CROSS_STEP_H - 14.0),
        Point::new(106.0, CROSS_STEP_H - 14.0),
        Point::new(106.0, CROSS_STEP_H),
    ];
    assert_eq!(route.polyline.len(), expected.len());
    for (actual, expected) in route.polyline.iter().zip(expected) {
        close(actual.x, expected.x);
        close(actual.y, expected.y);
    }
    assert!(
        route.polyline.iter().all(|point| point.x >= 106.0),
        "the rail must stay outside the boxes it routes between"
    );
}

#[test]
fn vertical_same_flow_step_edges_sidestep_on_the_other_axis() {
    let mut input = chapter(Orientation::Vertical);
    input.edges.retain(|edge| edge.id == "a-d");
    let scene = build_scene(&input).unwrap();
    let route = &scene.edges[0];

    let rail = 64.0 + 92.0 / 3.0;
    let expected = [
        Point::new(212.0, 32.0),
        Point::new(226.0, 32.0),
        Point::new(226.0, rail),
        Point::new(CROSS_STEP_V - 14.0, rail),
        Point::new(CROSS_STEP_V - 14.0, 32.0),
        Point::new(CROSS_STEP_V, 32.0),
    ];
    assert_eq!(route.polyline.len(), expected.len());
    for (actual, expected) in route.polyline.iter().zip(expected) {
        close(actual.x, expected.x);
        close(actual.y, expected.y);
    }
}

#[test]
fn a_tether_hugs_the_outward_rail_in_each_orientation() {
    let horizontal = build_scene(&chapter(Orientation::Horizontal)).unwrap();
    assert_eq!(horizontal.tethers.len(), 1);
    let tether = &horizontal.tethers[0];
    assert_eq!(
        tether.child_node_index,
        horizontal.node_index_of("d").unwrap()
    );
    assert_eq!(
        tether.parent_node_index,
        horizontal.node_index_of("a").unwrap()
    );
    let expected = [
        Point::new(0.0, 32.0),
        Point::new(-12.0, 32.0),
        Point::new(-12.0, CROSS_STEP_H + 32.0),
        Point::new(0.0, CROSS_STEP_H + 32.0),
    ];
    for (actual, expected) in tether.polyline.iter().zip(expected) {
        close(actual.x, expected.x);
        close(actual.y, expected.y);
    }

    let vertical = build_scene(&chapter(Orientation::Vertical)).unwrap();
    let tether = &vertical.tethers[0];
    let expected = [
        Point::new(106.0, 0.0),
        Point::new(106.0, -12.0),
        Point::new(CROSS_STEP_V + 106.0, -12.0),
        Point::new(CROSS_STEP_V + 106.0, 0.0),
    ];
    for (actual, expected) in tether.polyline.iter().zip(expected) {
        close(actual.x, expected.x);
        close(actual.y, expected.y);
    }
}

#[test]
fn named_ports_resolve_to_face_anchors_whether_or_not_an_edge_uses_them() {
    let mut input = chapter(Orientation::Horizontal);
    input.nodes[0].ports = vec![
        port("out", PortSide::Right, 0.25),
        port("spare", PortSide::Top, 0.0),
    ];
    input.nodes[1].ports = vec![port("in", PortSide::Left, 0.75)];
    input.edges = vec![ported_edge("a-b", ("a", "out"), ("b", "in"))];

    let scene = build_scene(&input).unwrap();
    assert_eq!(scene.ports.len(), 3, "unconnected ports still exist");

    let out = &scene.ports[scene.port_index_of("a", "out").unwrap()];
    close(out.point.x, 212.0);
    close(out.point.y, 16.0);
    let spare = &scene.ports[scene.port_index_of("a", "spare").unwrap()];
    close(spare.point.x, 0.0);
    close(spare.point.y, 0.0);

    let route = &scene.edges[0];
    assert_eq!(route.from_port_index, scene.port_index_of("a", "out"));
    assert_eq!(route.to_port_index, scene.port_index_of("b", "in"));
    close(route.polyline[0].y, 16.0);
    close(route.polyline[route.polyline.len() - 1].y, 48.0);
}

#[test]
fn a_route_leaving_a_face_that_points_away_goes_around_instead_of_through() {
    let mut input = chapter(Orientation::Horizontal);
    input.nodes[0].ports = vec![port("back", PortSide::Left, 0.5)];
    input.nodes[1].ports = vec![port("in", PortSide::Left, 0.5)];
    input.edges = vec![ported_edge("a-b", ("a", "back"), ("b", "in"))];

    let scene = build_scene(&input).unwrap();
    let route = &scene.edges[0];
    assert_eq!(route.polyline.len(), 6, "an around-the-rail route");
    assert!(
        route.polyline.iter().any(|point| point.x < 0.0),
        "it must leave through the face it was anchored to"
    );
}

#[test]
fn perpendicular_anchors_route_as_an_l_with_a_stub_at_each_end() {
    let mut input = chapter(Orientation::Horizontal);
    input.nodes[0].ports = vec![port("out", PortSide::Right, 0.5)];
    input.nodes[2].ports = vec![port("top", PortSide::Top, 0.5)];
    input.edges = vec![ported_edge("a-c", ("a", "out"), ("c", "top"))];

    let scene = build_scene(&input).unwrap();
    let route = &scene.edges[0];
    close(route.polyline[0].x, 212.0);
    close(route.polyline[0].y, 32.0);
    let end = route.polyline[route.polyline.len() - 1];
    close(end.x, FLOW_STEP_H + 106.0);
    close(end.y, CROSS_STEP_H);
    // Every leg is axis-aligned.
    for pair in route.polyline.windows(2) {
        assert!(
            (pair[0].x - pair[1].x).abs() < 1e-9 || (pair[0].y - pair[1].y).abs() < 1e-9,
            "orthogonal routes never travel diagonally"
        );
    }
}

#[test]
fn every_shape_produces_the_same_endpoints_with_different_control_points() {
    let shapes = [
        EdgeShape::Orthogonal,
        EdgeShape::Straight,
        EdgeShape::Quadratic,
        EdgeShape::Cubic,
    ];
    for shape in shapes {
        let mut input = chapter(Orientation::Horizontal);
        input.edges = vec![shaped_edge("a-c", "a", "c", shape)];
        let scene = build_scene(&input).unwrap();
        let route = &scene.edges[0];

        close(route.curve.start.x, 212.0);
        close(route.curve.start.y, 32.0);
        close(route.curve.end().x, FLOW_STEP_H);
        close(route.curve.end().y, CROSS_STEP_H + 32.0);
        assert_eq!(route.shape, shape);

        match shape {
            EdgeShape::Orthogonal => assert_eq!(route.curve.segments.len(), 3),
            _ => assert_eq!(route.curve.segments.len(), 1),
        }
    }
}

#[test]
fn a_quadratic_bends_through_the_elbow_its_orthogonal_route_turns_at() {
    let mut input = chapter(Orientation::Horizontal);
    input.edges = vec![shaped_edge("a-c", "a", "c", EdgeShape::Quadratic)];
    let scene = build_scene(&input).unwrap();

    let mid = (212.0 + FLOW_STEP_H) / 2.0;
    match scene.edges[0].curve.segments[0] {
        CurveSegment::Quadratic { control, .. } => {
            close(control.x, mid);
            close(control.y, 32.0);
        }
        other => panic!("expected a quadratic, got {other:?}"),
    }
}

#[test]
fn a_cubic_reaches_out_along_each_anchor_normal() {
    let mut input = chapter(Orientation::Horizontal);
    input.edges = vec![shaped_edge("a-c", "a", "c", EdgeShape::Cubic)];
    let scene = build_scene(&input).unwrap();

    match scene.edges[0].curve.segments[0] {
        CurveSegment::Cubic {
            control_a,
            control_b,
            ..
        } => {
            close(control_a.x, 212.0 + 46.0);
            close(control_a.y, 32.0);
            close(control_b.x, FLOW_STEP_H - 46.0);
            close(control_b.y, CROSS_STEP_H + 32.0);
        }
        other => panic!("expected a cubic, got {other:?}"),
    }
    assert!(
        polyline(&scene.edges[0].curve).len() > 4,
        "a curved segment flattens into many points"
    );
}

#[test]
fn scene_bounds_cover_route_geometry_that_leaves_every_box() {
    let scene = build_scene(&chapter(Orientation::Horizontal)).unwrap();
    // The tether rail at x = -12 is the leftmost geometry, and padding is 40.
    close(scene.bounds.min_x, -52.0);
    close(scene.bounds.min_y, -40.0);
    close(scene.bounds.max_x, FLOW_STEP_H + 212.0 + 40.0);
    close(scene.bounds.max_y, CROSS_STEP_H + 64.0 + 40.0);
}

#[test]
fn an_empty_graph_still_reports_a_drawable_extent() {
    let scene = build_scene(&GraphInput::default()).unwrap();
    assert!(scene.nodes.is_empty());
    assert!(scene.edges.is_empty());
    close(scene.bounds.min_x, 0.0);
    close(scene.bounds.max_x, 1.0);
    assert!(scene.bounds.width() > 0.0 && scene.bounds.height() > 0.0);
}

#[test]
fn per_node_size_overrides_do_not_move_any_other_node() {
    let mut input = chapter(Orientation::Horizontal);
    input.nodes[0].width = Some(400.0);
    input.nodes[0].height = Some(120.0);
    let scene = build_scene(&input).unwrap();

    close(scene.node("a").unwrap().rect.width, 400.0);
    close(scene.node("a").unwrap().rect.height, 120.0);
    // The authored grid is unchanged: neighbours keep their cells.
    close(scene.node("b").unwrap().rect.x, FLOW_STEP_H);
    close(scene.node("c").unwrap().rect.y, CROSS_STEP_H);
}

#[test]
fn host_supplied_metrics_are_geometry_inputs_not_a_second_layout() {
    let mut input = chapter(Orientation::Horizontal);
    input.metrics = LayoutMetrics {
        node_width: 100.0,
        node_height: 40.0,
        column_gap: 20.0,
        row_gap: 10.0,
        ..LayoutMetrics::default()
    };
    let scene = build_scene(&input).unwrap();
    close(scene.node("b").unwrap().rect.x, 120.0);
    close(scene.node("c").unwrap().rect.y, 50.0);
}

#[test]
fn node_label_anchors_are_rust_owned_in_both_orientations() {
    for orientation in [Orientation::Horizontal, Orientation::Vertical] {
        let scene = build_scene(&chapter(orientation)).unwrap();
        let node = &scene.nodes[0];
        assert_eq!(node.primary_label.x, node.rect.x + 9.0);
        assert_eq!(node.primary_label.y, node.rect.y + 23.0);
        assert_eq!(node.secondary_label.x, node.rect.x + 9.0);
        assert_eq!(node.secondary_label.y, node.rect.y + 45.0);
    }
}
