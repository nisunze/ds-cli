//! Determinism.
//!
//! "Same input bytes → same output bytes" is what makes a pinned fixture
//! meaningful and what lets a reviewer tell a content change from a jitter
//! change. These tests assert bit equality, not approximate agreement.

mod common;

use common::*;
use ds_canvas_graph::{
    EdgeShape, GeometrySession, GraphInput, Orientation, PortSide, build_scene, input_from_json,
    scene_to_json,
};

fn rich(orientation: Orientation) -> GraphInput {
    let mut input = chapter(orientation);
    input.nodes[0].ports = vec![
        port("out", PortSide::Right, 0.25),
        port("under", PortSide::Bottom, 0.9),
    ];
    input.nodes[1].ports = vec![port("in", PortSide::Left, 0.75)];
    input.nodes[2].width = Some(180.0);
    input
        .edges
        .push(ported_edge("a-b2", ("a", "out"), ("b", "in")));
    input
        .edges
        .push(shaped_edge("a-c2", "a", "c", EdgeShape::Cubic));
    input
        .edges
        .push(shaped_edge("a-c3", "a", "c", EdgeShape::Quadratic));
    input
}

#[test]
fn building_the_same_graph_twice_produces_identical_scenes() {
    for orientation in [Orientation::Horizontal, Orientation::Vertical] {
        let first = build_scene(&rich(orientation)).unwrap();
        let second = build_scene(&rich(orientation)).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            scene_to_json(&first, 1).unwrap(),
            scene_to_json(&second, 1).unwrap()
        );
    }
}

#[test]
fn the_packed_buffer_is_byte_identical_across_builds() {
    let one = GeometrySession::open(rich(Orientation::Horizontal)).unwrap();
    let two = GeometrySession::open(rich(Orientation::Horizontal)).unwrap();
    assert_eq!(one.scene_buffer(), two.scene_buffer());
    assert_eq!(one.scene_index(), two.scene_index());
}

#[test]
fn a_json_round_trip_reproduces_the_same_scene() {
    let input = rich(Orientation::Vertical);
    let text = serde_json::to_string(&input).unwrap();
    let parsed = input_from_json(&text).unwrap();

    assert_eq!(parsed, input);
    assert_eq!(build_scene(&parsed).unwrap(), build_scene(&input).unwrap());
}

#[test]
fn repeating_a_query_never_changes_its_answer() {
    let mut session = GeometrySession::open(rich(Orientation::Horizontal)).unwrap();
    session.set_host(1440.0, 900.0, 2.0).unwrap();
    session.set_viewport(1.7, 22.0, -13.0).unwrap();

    let projection = session.projection_buffer();
    let hit = session.hit_test_screen(700.0, 420.0).unwrap();
    let preview = session.preview_drag_world("c", FLOW_STEP_H, 0.0).unwrap();
    for _ in 0..16 {
        assert_eq!(session.projection_buffer(), projection);
        assert_eq!(session.hit_test_screen(700.0, 420.0).unwrap(), hit);
        assert_eq!(
            session.preview_drag_world("c", FLOW_STEP_H, 0.0).unwrap(),
            preview
        );
    }
}

#[test]
fn a_drag_there_and_back_returns_the_scene_it_started_with() {
    let mut session = GeometrySession::open(rich(Orientation::Horizontal)).unwrap();
    let before = session.scene_buffer();

    session
        .apply_drag_world("c", FLOW_STEP_H * 2.0, CROSS_STEP_H)
        .unwrap();
    session
        .apply_drag_world("c", -FLOW_STEP_H * 2.0, -CROSS_STEP_H)
        .unwrap();

    let after = session.scene_buffer();
    // The revision differs — two commits happened — and every other value is
    // identical, which is the property a "cell, not an offset" drag must have.
    assert_eq!(session.revision(), 3);
    assert_eq!(before[2..], after[2..]);
}

#[test]
fn the_two_orientations_are_different_scenes_of_the_same_graph() {
    let horizontal = build_scene(&rich(Orientation::Horizontal)).unwrap();
    let vertical = build_scene(&rich(Orientation::Vertical)).unwrap();

    assert_ne!(horizontal.nodes[1].rect.x, vertical.nodes[1].rect.x);
    // Membership, identity and ordering are orientation-independent.
    assert_eq!(horizontal.nodes.len(), vertical.nodes.len());
    for (left, right) in horizontal.nodes.iter().zip(&vertical.nodes) {
        assert_eq!(left.id, right.id);
        assert_eq!((left.column, left.row), (right.column, right.row));
        assert_eq!(left.rect.width, right.rect.width);
    }
    assert_eq!(horizontal.edges.len(), vertical.edges.len());
    assert_eq!(horizontal.ports.len(), vertical.ports.len());
    assert_eq!(horizontal.tethers.len(), vertical.tethers.len());
}

#[test]
fn node_and_edge_order_follows_the_authored_order_not_a_hash() {
    let mut input = chapter(Orientation::Horizontal);
    input.nodes.reverse();
    let scene = build_scene(&input).unwrap();
    let ids: Vec<&str> = scene.nodes.iter().map(|node| node.id.as_str()).collect();
    assert_eq!(ids, ["d", "c", "b", "a"]);
    assert_eq!(scene.node_index_of("d"), Some(0));

    let edge_ids: Vec<&str> = scene.edges.iter().map(|edge| edge.id.as_str()).collect();
    assert_eq!(edge_ids, ["a-b", "a-c", "a-d"]);
}

#[test]
fn every_coordinate_the_authority_publishes_is_finite() {
    for orientation in [Orientation::Horizontal, Orientation::Vertical] {
        let mut session = GeometrySession::open(rich(orientation)).unwrap();
        session.set_host(37.0, 4000.0, 0.5).unwrap();
        session.set_viewport(1e6, -1e9, 1e9).unwrap();

        assert!(
            session.scene_buffer().iter().all(|value| value.is_finite()),
            "scene buffer must be finite"
        );
        assert!(
            session
                .projection_buffer()
                .iter()
                .all(|value| value.is_finite()),
            "projection buffer must be finite"
        );
        let outcome = session.apply_drag_world("b", 500.0, 500.0).unwrap();
        assert!(
            GeometrySession::drag_buffer(&outcome)
                .iter()
                .all(|value| value.is_finite()),
            "drag buffer must be finite"
        );
    }
}
