//! Negative cases.
//!
//! Every refusal here is a case where a silent correction would produce a
//! drawable-looking picture of a graph nobody authored. A clamped `NaN` is
//! still a wrong position; it is just a wrong position nobody notices.

mod common;

use common::*;
use ds_canvas_graph::{
    EdgeShape, GeometrySession, GraphInput, LayoutMetrics, Orientation, PortSide, build_scene,
    input_from_json,
};

fn horizontal(nodes: Vec<ds_canvas_graph::NodeInput>) -> GraphInput {
    GraphInput {
        orientation: Orientation::Horizontal,
        metrics: LayoutMetrics::default(),
        nodes,
        edges: Vec::new(),
    }
}

#[test]
fn a_non_finite_node_size_is_refused_not_clamped() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut input = horizontal(vec![node("a", 0, 0)]);
        input.nodes[0].width = Some(bad);
        let error = build_scene(&input).unwrap_err();
        assert_eq!(error.code(), "canvas_geometry_non_finite_input");
    }
}

#[test]
fn a_zero_or_negative_node_size_is_refused() {
    for bad in [0.0, -1.0] {
        let mut input = horizontal(vec![node("a", 0, 0)]);
        input.nodes[0].height = Some(bad);
        assert_eq!(
            build_scene(&input).unwrap_err().code(),
            "canvas_geometry_invalid_metric"
        );
    }
}

/// One named way to poison a metric field.
type MetricPoison = (&'static str, fn(&mut LayoutMetrics));

#[test]
fn non_finite_metrics_are_refused_field_by_field() {
    let cases: [MetricPoison; 6] = [
        ("nodeWidth", |m| m.node_width = f64::NAN),
        ("nodeHeight", |m| m.node_height = f64::INFINITY),
        ("columnGap", |m| m.column_gap = f64::NAN),
        ("rowGap", |m| m.row_gap = f64::NEG_INFINITY),
        ("scenePadding", |m| m.scene_padding = f64::NAN),
        ("maxDragWorld", |m| m.max_drag_world = f64::NAN),
    ];
    for (label, mutate) in cases {
        let mut input = horizontal(vec![node("a", 0, 0)]);
        mutate(&mut input.metrics);
        let error = build_scene(&input).unwrap_err();
        assert_eq!(
            error.code(),
            "canvas_geometry_non_finite_input",
            "{label} must be refused"
        );
    }
}

#[test]
fn a_negative_gap_is_refused_because_it_would_invert_the_grid() {
    let mut input = horizontal(vec![node("a", 0, 0)]);
    input.metrics.column_gap = -10.0;
    assert_eq!(
        build_scene(&input).unwrap_err().code(),
        "canvas_geometry_invalid_metric"
    );
}

#[test]
fn flatten_steps_outside_the_supported_band_are_refused() {
    for bad in [0u32, 257] {
        let mut input = horizontal(vec![node("a", 0, 0)]);
        input.metrics.curve_flatten_steps = bad;
        assert_eq!(
            build_scene(&input).unwrap_err().code(),
            "canvas_geometry_invalid_metric"
        );
    }
}

#[test]
fn duplicate_and_empty_identities_are_refused() {
    let duplicate = horizontal(vec![node("a", 0, 0), node("a", 1, 0)]);
    assert_eq!(
        build_scene(&duplicate).unwrap_err().code(),
        "canvas_geometry_duplicate_node"
    );

    let empty = horizontal(vec![node("   ", 0, 0)]);
    assert_eq!(
        build_scene(&empty).unwrap_err().code(),
        "canvas_geometry_empty_identifier"
    );

    let mut duplicate_edges = horizontal(vec![node("a", 0, 0), node("b", 1, 0)]);
    duplicate_edges.edges = vec![edge("e", "a", "b"), edge("e", "b", "a")];
    assert_eq!(
        build_scene(&duplicate_edges).unwrap_err().code(),
        "canvas_geometry_duplicate_edge"
    );
}

#[test]
fn a_dangling_edge_endpoint_is_refused_rather_than_dropped() {
    let mut input = horizontal(vec![node("a", 0, 0)]);
    input.edges = vec![edge("e", "a", "ghost")];
    let error = build_scene(&input).unwrap_err();
    assert_eq!(error.code(), "canvas_geometry_unknown_node");
    assert!(
        error.to_string().contains("ghost"),
        "the refusal names the missing node: {error}"
    );
}

#[test]
fn an_unknown_or_duplicate_port_is_refused() {
    let mut input = horizontal(vec![node("a", 0, 0), node("b", 1, 0)]);
    input.nodes[0].ports = vec![port("out", PortSide::Right, 0.5)];
    input.edges = vec![ported_edge("e", ("a", "out"), ("b", "in"))];
    assert_eq!(
        build_scene(&input).unwrap_err().code(),
        "canvas_geometry_unknown_port"
    );

    let mut duplicate = horizontal(vec![node("a", 0, 0)]);
    duplicate.nodes[0].ports = vec![
        port("out", PortSide::Right, 0.5),
        port("out", PortSide::Left, 0.5),
    ];
    assert_eq!(
        build_scene(&duplicate).unwrap_err().code(),
        "canvas_geometry_duplicate_port"
    );
}

#[test]
fn a_port_offset_outside_its_face_is_refused() {
    for bad in [-0.001, 1.001, f64::NAN] {
        let mut input = horizontal(vec![node("a", 0, 0)]);
        input.nodes[0].ports = vec![port("out", PortSide::Right, bad)];
        let error = build_scene(&input).unwrap_err();
        assert!(
            error.code() == "canvas_geometry_invalid_metric"
                || error.code() == "canvas_geometry_non_finite_input",
            "unexpected code {} for offset {bad}",
            error.code()
        );
    }
}

#[test]
fn containment_must_be_a_forest() {
    let mut self_parent = horizontal(vec![node("a", 0, 0)]);
    self_parent.nodes[0].parent = Some("a".to_string());
    assert_eq!(
        build_scene(&self_parent).unwrap_err().code(),
        "canvas_geometry_self_containment"
    );

    let mut missing = horizontal(vec![contained("a", 0, 0, "ghost")]);
    missing.edges.clear();
    assert_eq!(
        build_scene(&missing).unwrap_err().code(),
        "canvas_geometry_unknown_node"
    );

    let cycle = horizontal(vec![
        contained("a", 0, 0, "b"),
        contained("b", 1, 0, "c"),
        contained("c", 2, 0, "a"),
    ]);
    assert_eq!(
        build_scene(&cycle).unwrap_err().code(),
        "canvas_geometry_containment_cycle"
    );
}

#[test]
fn a_cell_outside_the_authored_grid_is_refused() {
    for (column, row) in [(-1, 0), (0, -1), (5_000, 0), (0, 5_000)] {
        let input = horizontal(vec![node("a", column, row)]);
        assert_eq!(
            build_scene(&input).unwrap_err().code(),
            "canvas_geometry_cell_out_of_range",
            "cell ({column}, {row}) must be refused"
        );
    }
}

#[test]
fn oversized_inputs_are_refused_with_the_exact_bound() {
    let mut input = horizontal(
        (0..3)
            .map(|index| node(&format!("n{index}"), index, 0))
            .collect(),
    );
    input.nodes[0].ports = (0..65)
        .map(|index| port(&format!("p{index}"), PortSide::Right, 0.5))
        .collect();
    let error = build_scene(&input).unwrap_err();
    assert_eq!(error.code(), "canvas_geometry_input_too_large");
    assert!(error.to_string().contains("65"), "{error}");
}

#[test]
fn host_window_facts_must_be_finite_and_positive() {
    let mut session = GeometrySession::open(chapter(Orientation::Horizontal)).unwrap();
    for (width, height, dpr) in [
        (f64::NAN, 600.0, 1.0),
        (800.0, f64::INFINITY, 1.0),
        (800.0, 600.0, f64::NAN),
        (0.0, 600.0, 1.0),
        (800.0, -600.0, 1.0),
        (800.0, 600.0, 0.0),
    ] {
        assert!(
            session.set_host(width, height, dpr).is_err(),
            "({width}, {height}, {dpr}) must be refused"
        );
    }
    // A refused call leaves the previous host facts in place.
    session.set_host(800.0, 600.0, 2.0).unwrap();
    assert!(session.set_host(f64::NAN, 600.0, 1.0).is_err());
    close(session.host().width, 800.0);
    close(session.host().device_pixel_ratio, 2.0);
}

#[test]
fn camera_intent_must_be_finite() {
    let mut session = GeometrySession::open(chapter(Orientation::Horizontal)).unwrap();
    session.set_host(800.0, 600.0, 1.0).unwrap();

    assert!(session.set_viewport(f64::NAN, 0.0, 0.0).is_err());
    assert!(session.set_viewport(1.0, f64::INFINITY, 0.0).is_err());
    assert!(session.set_viewport(1.0, 0.0, f64::NAN).is_err());
    assert!(session.pan(f64::NAN, 0.0).is_err());
    assert!(session.zoom_centered(f64::NAN).is_err());
    assert!(session.zoom_at(f64::NAN, 0.0, 2.0).is_err());
    assert!(session.zoom_at(0.0, 0.0, f64::INFINITY).is_err());
    assert!(session.center_on(f64::NAN, 0.0).is_err());
    assert!(session.hit_test_screen(f64::NAN, 0.0).is_err());
    assert!(session.world_from_screen(0.0, f64::NAN).is_err());
    assert!(session.screen_from_world(f64::NAN, 0.0).is_err());

    // Nothing above committed anything.
    assert_eq!(session.viewport(), ds_canvas_graph::Viewport::fitted());
}

#[test]
fn json_input_refuses_unknown_fields_and_malformed_text() {
    let unknown = r#"{"nodes":[{"id":"a","column":0,"row":0,"colour":"red"}]}"#;
    assert_eq!(
        input_from_json(unknown).unwrap_err().code(),
        "canvas_geometry_invalid_json"
    );

    // A misspelled collection would otherwise become a silently empty one.
    let misspelled = r#"{"nodes":[],"edge":[]}"#;
    assert_eq!(
        input_from_json(misspelled).unwrap_err().code(),
        "canvas_geometry_invalid_json"
    );

    assert_eq!(
        input_from_json("not json").unwrap_err().code(),
        "canvas_geometry_invalid_json"
    );

    assert_eq!(
        GeometrySession::open_json(r#"{"nodes":[{"id":"","column":0,"row":0}]}"#)
            .unwrap_err()
            .code(),
        "canvas_geometry_empty_identifier"
    );
}

#[test]
fn json_input_accepts_the_documented_camel_case_shape() {
    let input = input_from_json(
        r#"{
            "orientation": "vertical",
            "metrics": { "nodeWidth": 100, "curveFlattenSteps": 8 },
            "nodes": [
                { "id": "a", "column": 0, "row": 0, "ports": [{ "name": "out", "side": "bottom" }] },
                { "id": "b", "column": 1, "row": 0, "parent": "a" }
            ],
            "edges": [
                { "id": "a-b", "from": { "node": "a", "port": "out" }, "to": { "node": "b" },
                  "shape": "cubic" }
            ]
        }"#,
    )
    .unwrap();

    assert_eq!(input.orientation, Orientation::Vertical);
    close(input.metrics.node_width, 100.0);
    assert_eq!(input.metrics.curve_flatten_steps, 8);
    // Unspecified metrics keep their defaults.
    close(input.metrics.node_height, 64.0);
    close(input.nodes[0].ports[0].offset, 0.5);
    assert_eq!(input.edges[0].shape, EdgeShape::Cubic);
    assert!(build_scene(&input).is_ok());
}

#[test]
fn every_refusal_carries_a_distinct_stable_code() {
    let mut codes = vec![
        "canvas_geometry_non_finite_input",
        "canvas_geometry_invalid_metric",
        "canvas_geometry_empty_identifier",
        "canvas_geometry_duplicate_node",
        "canvas_geometry_duplicate_edge",
        "canvas_geometry_duplicate_port",
        "canvas_geometry_unknown_node",
        "canvas_geometry_unknown_port",
        "canvas_geometry_cell_out_of_range",
        "canvas_geometry_self_containment",
        "canvas_geometry_containment_cycle",
        "canvas_geometry_input_too_large",
        "canvas_geometry_drag_too_large",
        "canvas_geometry_invalid_json",
        "canvas_geometry_invalid_hit_code",
        "canvas_geometry_invalid_buffer",
    ];
    let count = codes.len();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), count, "codes must be unique");
}

#[test]
fn a_refusal_serializes_with_its_code_and_operands() {
    let mut input = horizontal(vec![node("a", 0, 0)]);
    input.edges = vec![edge("e", "a", "ghost")];
    let error = build_scene(&input).unwrap_err();
    let json = ds_canvas_graph::error_to_json(&error);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["code"], "canvas_geometry_unknown_node");
    assert_eq!(value["error"]["code"], "canvas_geometry_unknown_node");
    assert_eq!(value["error"]["node"], "ghost");
    assert_eq!(value["error"]["reference"], "edges[e].to");
    assert!(value["message"].as_str().unwrap().contains("ghost"));
}
