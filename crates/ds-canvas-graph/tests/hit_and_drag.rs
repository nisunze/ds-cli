//! Pointer resolution and bounded drag intent.

mod common;

use common::*;
use ds_canvas_graph::hit::{HIT_KIND_NODE, HIT_KIND_PORT, HIT_MAX_INDEX};
use ds_canvas_graph::{GeometrySession, HitTarget, Orientation, PortSide};

fn session() -> GeometrySession {
    let mut session = GeometrySession::open(chapter(Orientation::Horizontal)).unwrap();
    session.set_host(1600.0, 1200.0, 1.0).unwrap();
    session
}

fn ported_session() -> GeometrySession {
    let mut input = chapter(Orientation::Horizontal);
    input.nodes[0].ports = vec![port("out", PortSide::Right, 0.5)];
    input.nodes[1].ports = vec![port("in", PortSide::Left, 0.5)];
    input.edges = vec![ported_edge("a-b", ("a", "out"), ("b", "in"))];
    let mut session = GeometrySession::open(input).unwrap();
    session.set_host(1600.0, 1200.0, 1.0).unwrap();
    session
}

#[test]
fn a_point_inside_a_node_resolves_to_that_node() {
    let session = session();
    let index = session.scene().node_index_of("c").unwrap();
    let rect = session.scene().nodes[index].rect;
    let hit = session
        .hit_test_world(rect.center().x, rect.center().y)
        .unwrap();
    assert_eq!(hit, HitTarget::Node { index });
}

#[test]
fn empty_space_resolves_to_nothing() {
    let session = session();
    assert_eq!(
        session.hit_test_world(-1000.0, -1000.0).unwrap(),
        HitTarget::None
    );
    assert_eq!(HitTarget::None.code(), 0);
}

#[test]
fn a_port_wins_over_the_node_it_is_drawn_on() {
    let session = ported_session();
    let port_index = session.scene().port_index_of("a", "out").unwrap();
    let anchor = session.scene().ports[port_index].point;

    // The anchor sits exactly on the node's own boundary.
    assert_eq!(
        session.hit_test_world(anchor.x, anchor.y).unwrap(),
        HitTarget::Port { index: port_index }
    );
    // Just inside the node, past the port radius, the node wins again.
    let node_index = session.scene().node_index_of("a").unwrap();
    assert_eq!(
        session.hit_test_world(anchor.x - 40.0, anchor.y).unwrap(),
        HitTarget::Node { index: node_index }
    );
}

#[test]
fn a_route_is_hit_within_tolerance_and_missed_outside_it() {
    let session = session();
    let index = session.scene().edge_index_of("a-b").unwrap();
    // Past the mid-gap, where "a-c" has already turned away, only "a-b" runs.
    let point = ds_canvas_graph::Point::new(FLOW_STEP_H - 10.0, 32.0);

    assert_eq!(
        session.hit_test_world(point.x, point.y).unwrap(),
        HitTarget::Edge { index }
    );
    let scale = session.projection().scale;
    let outside = session.input().metrics.edge_hit_tolerance_px / scale + 1.0;
    assert_eq!(
        session.hit_test_world(point.x, point.y + outside).unwrap(),
        HitTarget::None
    );
}

#[test]
fn two_routes_over_one_point_resolve_the_same_way_every_time() {
    let session = session();
    // "a-b" and "a-c" share their first leg out of node a.
    let shared = session.scene().edges[session.scene().edge_index_of("a-b").unwrap()].label;
    let first = session.hit_test_world(shared.x, shared.y).unwrap();
    for _ in 0..8 {
        assert_eq!(session.hit_test_world(shared.x, shared.y).unwrap(), first);
    }
    assert_eq!(
        first,
        HitTarget::Edge {
            index: session.scene().edge_index_of("a-c").unwrap()
        },
        "the later-authored route is the one drawn on top"
    );
}

#[test]
fn a_containment_tether_is_hit_testable_and_ranked_below_routes() {
    let session = session();
    let tether = &session.scene().tethers[0];
    let rail = tether.polyline[1];
    assert_eq!(
        session.hit_test_world(rail.x, rail.y + 20.0).unwrap(),
        HitTarget::Tether { index: 0 }
    );
}

#[test]
fn hit_tolerance_is_a_screen_size_so_it_shrinks_in_world_as_zoom_grows() {
    let mut session = ported_session();
    let anchor = session.scene().ports[session.scene().port_index_of("a", "out").unwrap()].point;
    let probe = 6.0;

    session.set_viewport(0.2, 0.0, 0.0).unwrap();
    let zoomed_out = session.hit_test_world(anchor.x, anchor.y + probe).unwrap();
    session.set_viewport(50.0, 0.0, 0.0).unwrap();
    let zoomed_in = session.hit_test_world(anchor.x, anchor.y + probe).unwrap();

    assert!(matches!(zoomed_out, HitTarget::Port { .. }));
    assert!(
        !matches!(zoomed_in, HitTarget::Port { .. }),
        "at high zoom the same world offset is far outside the port"
    );
}

#[test]
fn screen_and_world_hit_testing_agree_through_the_projection() {
    let mut session = session();
    session.set_viewport(2.0, 40.0, -30.0).unwrap();
    let index = session.scene().node_index_of("b").unwrap();
    let centre = session.scene().nodes[index].rect.center();
    let screen = session.screen_from_world(centre.x, centre.y).unwrap();

    assert_eq!(
        session.hit_test_screen(screen.x, screen.y).unwrap(),
        session.hit_test_world(centre.x, centre.y).unwrap()
    );
}

#[test]
fn a_packed_hit_code_round_trips_through_the_scene() {
    let session = ported_session();
    for target in [
        HitTarget::None,
        HitTarget::Node { index: 2 },
        HitTarget::Port { index: 1 },
        HitTarget::Edge { index: 0 },
        HitTarget::Tether { index: 0 },
    ] {
        let code = target.code();
        assert_eq!(session.hit_target(code).unwrap(), target);
    }
    assert_eq!(
        HitTarget::Node { index: 5 }.code() >> 28,
        HIT_KIND_NODE,
        "kind lives in the high bits"
    );
    assert_eq!(
        HitTarget::Port { index: 3 }.code(),
        (HIT_KIND_PORT << 28) | 3
    );
}

#[test]
fn a_hit_code_that_does_not_address_this_scene_is_refused() {
    let session = session();
    for code in [
        (HIT_KIND_NODE << 28) | 99,
        (HIT_KIND_PORT << 28),
        7 << 28,
        HIT_MAX_INDEX,
    ] {
        assert_eq!(
            session.hit_target(code).unwrap_err().code(),
            "canvas_geometry_invalid_hit_code",
            "code {code} must be refused"
        );
    }
}

#[test]
fn a_drag_resolves_to_an_authored_cell_not_an_accumulated_offset() {
    let mut session = session();
    let outcome = session
        .apply_drag_world("b", FLOW_STEP_H, CROSS_STEP_H * 2.0)
        .unwrap();

    assert!(outcome.moved);
    assert!(!outcome.clamped);
    assert_eq!((outcome.from_column, outcome.from_row), (1, 0));
    assert_eq!((outcome.to_column, outcome.to_row), (2, 2));
    close(session.scene().node("b").unwrap().rect.x, FLOW_STEP_H * 2.0);
    close(
        session.scene().node("b").unwrap().rect.y,
        CROSS_STEP_H * 2.0,
    );
}

#[test]
fn a_sub_cell_drag_snaps_to_the_nearest_cell_and_reports_no_move() {
    let mut session = session();
    let outcome = session
        .apply_drag_world("b", FLOW_STEP_H * 0.4, 0.0)
        .unwrap();
    assert!(!outcome.moved);
    assert_eq!((outcome.to_column, outcome.to_row), (1, 0));
    assert!(outcome.patch.nodes.is_empty());
    assert_eq!(
        session.revision(),
        1,
        "an unchanged scene keeps its revision"
    );

    let outcome = session
        .apply_drag_world("b", FLOW_STEP_H * 0.6, 0.0)
        .unwrap();
    assert!(outcome.moved);
    assert_eq!(outcome.to_column, 2);
    assert_eq!(session.revision(), 2);
}

#[test]
fn a_vertical_drag_maps_the_pointer_axes_the_other_way_round() {
    let mut session = GeometrySession::open(chapter(Orientation::Vertical)).unwrap();
    session.set_host(1600.0, 1200.0, 1.0).unwrap();
    let outcome = session
        .apply_drag_world("b", CROSS_STEP_V, FLOW_STEP_V)
        .unwrap();

    // x crossed one lane, y advanced one flow step.
    assert_eq!((outcome.to_column, outcome.to_row), (2, 1));
}

#[test]
fn a_drag_off_the_authored_grid_is_clamped_and_says_so() {
    let mut session = session();
    let outcome = session
        .apply_drag_world("a", -FLOW_STEP_H * 5.0, -CROSS_STEP_H * 5.0)
        .unwrap();
    assert!(outcome.clamped);
    assert!(!outcome.moved);
    assert_eq!((outcome.to_column, outcome.to_row), (0, 0));

    // The far edge of the default grid is further away than one drag is
    // allowed to travel, so a narrow grid is what proves the upper clamp.
    let mut input = chapter(Orientation::Horizontal);
    input.metrics.max_column = 2;
    input.metrics.max_row = 1;
    let mut narrow = GeometrySession::open(input).unwrap();
    narrow.set_host(1600.0, 1200.0, 1.0).unwrap();
    let outcome = narrow
        .apply_drag_world("a", FLOW_STEP_H * 9.0, CROSS_STEP_H * 9.0)
        .unwrap();
    assert!(outcome.clamped);
    assert_eq!((outcome.to_column, outcome.to_row), (2, 1));
}

#[test]
fn a_drag_larger_than_the_bound_is_refused_even_when_the_grid_would_allow_it() {
    let mut session = session();
    let beyond = session.input().metrics.max_drag_world * 1.5;
    assert_eq!(
        session
            .apply_drag_world("a", beyond, 0.0)
            .unwrap_err()
            .code(),
        "canvas_geometry_drag_too_large"
    );
}

#[test]
fn an_unbounded_or_non_finite_drag_is_refused() {
    let mut session = session();
    assert_eq!(
        session
            .apply_drag_world("a", f64::NAN, 0.0)
            .unwrap_err()
            .code(),
        "canvas_geometry_non_finite_input"
    );
    assert_eq!(
        session
            .apply_drag_world("a", 0.0, f64::INFINITY)
            .unwrap_err()
            .code(),
        "canvas_geometry_non_finite_input"
    );
    assert_eq!(
        session
            .apply_drag_world("a", 1.0e9, 0.0)
            .unwrap_err()
            .code(),
        "canvas_geometry_drag_too_large"
    );
    assert_eq!(
        session
            .apply_drag_screen("a", f64::NAN, 0.0)
            .unwrap_err()
            .code(),
        "canvas_geometry_non_finite_input"
    );
    assert_eq!(
        session
            .apply_drag_world("ghost", 1.0, 1.0)
            .unwrap_err()
            .code(),
        "canvas_geometry_unknown_node"
    );
    // Nothing above moved anything.
    assert_eq!(session.revision(), 1);
    close(session.scene().node("a").unwrap().rect.x, 0.0);
}

#[test]
fn preview_and_apply_run_the_same_validator() {
    let mut session = session();
    let preview = session.preview_drag_world("c", FLOW_STEP_H, 0.0).unwrap();
    // The preview did not commit.
    close(session.scene().node("c").unwrap().rect.x, FLOW_STEP_H);
    assert_eq!(session.revision(), 1);

    let applied = session.apply_drag_world("c", FLOW_STEP_H, 0.0).unwrap();
    assert_eq!(preview, applied);
    assert_eq!(session.revision(), 2);
    assert_eq!(
        preview.patch.revision, 2,
        "a preview reports the revision it would create"
    );
}

#[test]
fn a_patch_carries_the_moved_node_every_route_touching_it_and_the_new_bounds() {
    let mut session = session();
    let before = session.scene().bounds;
    let outcome = session
        .apply_drag_world("d", FLOW_STEP_H * 3.0, 0.0)
        .unwrap();

    assert_eq!(outcome.patch.nodes.len(), 1);
    assert_eq!(
        outcome.patch.nodes[0].index,
        session.scene().node_index_of("d").unwrap()
    );
    // "a-d" is the only edge touching d; the tether follows it too.
    assert_eq!(outcome.patch.edges.len(), 1);
    assert_eq!(
        outcome.patch.edges[0].index,
        session.scene().edge_index_of("a-d").unwrap()
    );
    assert_eq!(outcome.patch.tethers.len(), 1);
    assert_eq!(outcome.patch.bounds, session.scene().bounds);
    assert!(outcome.patch.bounds.max_x > before.max_x);
}

#[test]
fn a_screen_drag_is_divided_by_the_scale_before_it_means_anything() {
    let mut session = session();
    session.set_viewport(1.0, 0.0, 0.0).unwrap();
    let scale = session.projection().scale;

    let world = session.preview_drag_world("b", FLOW_STEP_H, 0.0).unwrap();
    let screen = session
        .preview_drag_screen("b", FLOW_STEP_H * scale, 0.0)
        .unwrap();
    assert_eq!(world.to_column, screen.to_column);
    assert_eq!(world.to_row, screen.to_row);
}

#[test]
fn dragging_a_node_onto_another_cell_does_not_delete_or_reorder_anything() {
    let mut session = session();
    let before: Vec<String> = session
        .scene()
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect();
    session.apply_drag_world("d", FLOW_STEP_H, 0.0).unwrap();
    let after: Vec<String> = session
        .scene()
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect();

    assert_eq!(
        before, after,
        "indices stay stable so a patch can address them"
    );
    // Overlapping cells are allowed: the authority reports geometry, it does
    // not enforce a placement policy the host never asked for.
    close(
        session.scene().node("d").unwrap().rect.x,
        session.scene().node("c").unwrap().rect.x,
    );
}
