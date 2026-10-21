//! The packed wire format, read back through the decoder a host must mirror.

mod common;

use common::*;
use ds_canvas_graph::abi::{
    EDGE_HEAD_LEN, NO_PORT, NODE_STRIDE, PATCH_EDGE_HEAD_LEN, PATCH_HEADER_LEN, PATCH_NODE_STRIDE,
    PATCH_TETHER_HEAD_LEN, PORT_STRIDE, SCENE_HEADER_LEN, SEGMENT_STRIDE, TETHER_HEAD_LEN,
    abi_descriptor, decode_drag, decode_scene,
};
use ds_canvas_graph::{
    CURVE_OP_CUBIC, CURVE_OP_LINE, CURVE_OP_QUADRATIC, EdgeShape, GeometrySession, Orientation,
    PortSide, SCENE_BUFFER_VERSION, encode_drag, encode_scene,
};

/// The shared chapter plus one explicitly ported edge, so every buffer test
/// sees both a geometry-chosen face and a named port.
fn session(orientation: Orientation) -> GeometrySession {
    let mut input = chapter(orientation);
    input.nodes[0].ports = vec![port("out", PortSide::Right, 0.25)];
    input.nodes[1].ports = vec![port("in", PortSide::Left, 0.75)];
    input
        .edges
        .push(ported_edge("a-b2", ("a", "out"), ("b", "in")));
    let mut session = GeometrySession::open(input).unwrap();
    session.set_host(1600.0, 1200.0, 2.0).unwrap();
    session
}

#[test]
fn the_scene_buffer_header_states_the_version_revision_and_counts() {
    let session = session(Orientation::Horizontal);
    let buffer = session.scene_buffer();
    let scene = session.scene();

    close(buffer[0], SCENE_BUFFER_VERSION);
    close(buffer[1], 1.0);
    close(buffer[2], 0.0);
    close(buffer[3], scene.nodes.len() as f64);
    close(buffer[4], scene.ports.len() as f64);
    close(buffer[5], scene.edges.len() as f64);
    close(buffer[6], scene.tethers.len() as f64);
    close(buffer[7], scene.bounds.min_x);
    close(buffer[10], scene.bounds.max_y);
}

#[test]
fn a_vertical_scene_is_tagged_with_its_orientation() {
    let session = session(Orientation::Vertical);
    close(session.scene_buffer()[2], 1.0);
}

#[test]
fn the_buffer_decodes_back_to_exactly_the_scene_it_encoded() {
    let session = session(Orientation::Horizontal);
    let decoded = session.decoded_scene_buffer().unwrap();
    let scene = session.scene();

    assert_eq!(decoded.revision, 1);
    assert_eq!(decoded.orientation, Orientation::Horizontal);
    assert_eq!(
        decoded.bounds,
        [
            scene.bounds.min_x,
            scene.bounds.min_y,
            scene.bounds.max_x,
            scene.bounds.max_y
        ]
    );

    assert_eq!(decoded.nodes.len(), scene.nodes.len());
    for (decoded, node) in decoded.nodes.iter().zip(&scene.nodes) {
        assert_eq!(
            *decoded,
            [
                node.rect.x,
                node.rect.y,
                node.rect.width,
                node.rect.height,
                node.primary_label.x,
                node.primary_label.y,
                node.secondary_label.x,
                node.secondary_label.y,
            ]
        );
    }

    assert_eq!(decoded.ports.len(), scene.ports.len());
    for (decoded, anchor) in decoded.ports.iter().zip(&scene.ports) {
        assert_eq!(decoded.node_index, anchor.node_index);
        assert_eq!(decoded.side, anchor.side.code());
        assert_eq!(decoded.point, anchor.point);
    }

    assert_eq!(decoded.edges.len(), scene.edges.len());
    for (decoded, route) in decoded.edges.iter().zip(&scene.edges) {
        assert_eq!(decoded.from_node_index, route.from_node_index);
        assert_eq!(decoded.to_node_index, route.to_node_index);
        assert_eq!(decoded.from_port_index, route.from_port_index);
        assert_eq!(decoded.to_port_index, route.to_port_index);
        assert_eq!(decoded.label, route.label);
        assert_eq!(decoded.start, route.curve.start);
        assert_eq!(decoded.segments.len(), route.curve.segments.len());
        for (decoded, segment) in decoded.segments.iter().zip(&route.curve.segments) {
            assert_eq!(decoded.op, segment.op_code());
            assert_eq!(decoded.to, segment.end());
        }
    }

    assert_eq!(decoded.tethers.len(), scene.tethers.len());
    for (decoded, tether) in decoded.tethers.iter().zip(&scene.tethers) {
        assert_eq!(decoded.child_node_index, tether.child_node_index);
        assert_eq!(decoded.parent_node_index, tether.parent_node_index);
        assert_eq!(decoded.start, tether.curve.start);
    }
}

#[test]
fn a_geometry_chosen_face_is_marked_with_the_no_port_sentinel() {
    let session = session(Orientation::Horizontal);
    let decoded = session.decoded_scene_buffer().unwrap();

    let plain = decoded
        .edges
        .iter()
        .find(|edge| edge.from_port_index.is_none())
        .expect("at least one edge uses a geometry-chosen face");
    assert_eq!(plain.to_port_index, None);
    // The sentinel must stay out of the index space it shares a slot with.
    const { assert!(NO_PORT < 0.0) };

    let ported = decoded
        .edges
        .iter()
        .find(|edge| edge.from_port_index.is_some())
        .expect("the ported edge survives the round trip");
    assert_eq!(
        ported.from_port_index,
        session.scene().port_index_of("a", "out")
    );
}

#[test]
fn every_curve_kind_encodes_its_control_points() {
    for (shape, op) in [
        (EdgeShape::Orthogonal, CURVE_OP_LINE),
        (EdgeShape::Straight, CURVE_OP_LINE),
        (EdgeShape::Quadratic, CURVE_OP_QUADRATIC),
        (EdgeShape::Cubic, CURVE_OP_CUBIC),
    ] {
        let mut input = chapter(Orientation::Horizontal);
        input.edges = vec![shaped_edge("a-c", "a", "c", shape)];
        let session = GeometrySession::open(input).unwrap();
        let decoded = session.decoded_scene_buffer().unwrap();
        let segment = decoded.edges[0].segments[0];
        assert_eq!(segment.op, op, "{shape:?}");

        let route = &session.scene().edges[0];
        match route.curve.segments[0] {
            ds_canvas_graph::CurveSegment::Quadratic { control, .. } => {
                assert_eq!(segment.control_a, control);
            }
            ds_canvas_graph::CurveSegment::Cubic {
                control_a,
                control_b,
                ..
            } => {
                assert_eq!(segment.control_a, control_a);
                assert_eq!(segment.control_b, control_b);
            }
            ds_canvas_graph::CurveSegment::Line { to } => {
                // Unused control slots repeat the endpoint, never the origin.
                assert_eq!(segment.control_a, to);
                assert_eq!(segment.control_b, to);
            }
        }
    }
}

#[test]
fn buffer_length_matches_the_documented_strides() {
    let session = session(Orientation::Horizontal);
    let scene = session.scene();
    let buffer = session.scene_buffer();

    let segments: usize = scene
        .edges
        .iter()
        .map(|edge| edge.curve.segments.len())
        .chain(scene.tethers.iter().map(|t| t.curve.segments.len()))
        .sum();
    let expected = SCENE_HEADER_LEN
        + scene.nodes.len() * NODE_STRIDE
        + scene.ports.len() * PORT_STRIDE
        + scene.edges.len() * EDGE_HEAD_LEN
        + scene.tethers.len() * TETHER_HEAD_LEN
        + segments * SEGMENT_STRIDE;
    assert_eq!(buffer.len(), expected);
}

#[test]
fn a_truncated_or_over_long_buffer_is_refused() {
    let session = session(Orientation::Horizontal);
    let buffer = session.scene_buffer();

    assert_eq!(
        decode_scene(&buffer[..buffer.len() - 1])
            .unwrap_err()
            .code(),
        "canvas_geometry_invalid_buffer"
    );
    let mut extended = buffer.clone();
    extended.push(0.0);
    assert_eq!(
        decode_scene(&extended).unwrap_err().code(),
        "canvas_geometry_invalid_buffer"
    );
    assert_eq!(
        decode_scene(&[]).unwrap_err().code(),
        "canvas_geometry_invalid_buffer"
    );
}

#[test]
fn an_unknown_version_or_orientation_is_refused_rather_than_guessed() {
    let session = session(Orientation::Horizontal);

    let mut wrong_version = session.scene_buffer();
    wrong_version[0] = 99.0;
    assert_eq!(
        decode_scene(&wrong_version).unwrap_err().code(),
        "canvas_geometry_invalid_buffer"
    );

    let mut wrong_orientation = session.scene_buffer();
    wrong_orientation[2] = 7.0;
    assert_eq!(
        decode_scene(&wrong_orientation).unwrap_err().code(),
        "canvas_geometry_invalid_buffer"
    );
}

#[test]
fn a_non_integer_segment_count_is_refused() {
    let session = session(Orientation::Horizontal);
    let mut buffer = session.scene_buffer();
    let scene = session.scene();
    // The first edge's segment count sits right after its head.
    let at = SCENE_HEADER_LEN
        + scene.nodes.len() * NODE_STRIDE
        + scene.ports.len() * PORT_STRIDE
        + (EDGE_HEAD_LEN - 1);
    buffer[at] = 2.5;
    assert_eq!(
        decode_scene(&buffer).unwrap_err().code(),
        "canvas_geometry_invalid_buffer"
    );

    buffer[at] = f64::NAN;
    assert_eq!(
        decode_scene(&buffer).unwrap_err().code(),
        "canvas_geometry_invalid_buffer"
    );
}

#[test]
fn a_drag_buffer_carries_the_resolved_cell_and_only_the_changed_geometry() {
    let mut session = session(Orientation::Horizontal);
    let outcome = session
        .apply_drag_world("d", FLOW_STEP_H * 2.0, 0.0)
        .unwrap();
    let buffer = GeometrySession::drag_buffer(&outcome);
    let decoded = decode_drag(&buffer).unwrap();

    close(buffer[0], SCENE_BUFFER_VERSION);
    assert_eq!(decoded.revision, 2);
    assert!(decoded.moved);
    assert!(!decoded.clamped);
    assert_eq!(
        decoded.node_index,
        session.scene().node_index_of("d").unwrap()
    );
    assert_eq!((decoded.to_column, decoded.to_row), (2, 1));
    assert_eq!(decoded.nodes.len(), 1);
    assert_eq!(decoded.nodes[0].0, decoded.node_index);
    assert_eq!(decoded.bounds[2], session.scene().bounds.max_x);

    let expected = PATCH_HEADER_LEN
        + decoded.nodes.len() * PATCH_NODE_STRIDE
        + decoded
            .edges
            .iter()
            .map(|(_, edge)| PATCH_EDGE_HEAD_LEN + edge.segments.len() * SEGMENT_STRIDE)
            .sum::<usize>()
        + decoded
            .tethers
            .iter()
            .map(|(_, tether)| PATCH_TETHER_HEAD_LEN + tether.segments.len() * SEGMENT_STRIDE)
            .sum::<usize>();
    assert_eq!(buffer.len(), expected);
}

#[test]
fn an_unmoved_drag_encodes_an_empty_patch() {
    let mut session = session(Orientation::Horizontal);
    let outcome = session.apply_drag_world("d", 1.0, 1.0).unwrap();
    let decoded = decode_drag(&GeometrySession::drag_buffer(&outcome)).unwrap();

    assert!(!decoded.moved);
    assert!(decoded.nodes.is_empty());
    assert!(decoded.edges.is_empty());
    assert!(decoded.tethers.is_empty());
    assert_eq!(decoded.revision, 1);
}

#[test]
fn the_id_tables_are_fetched_once_per_revision_not_per_frame() {
    let mut session = session(Orientation::Horizontal);
    let index = session.scene_index();

    assert_eq!(index.revision, 1);
    assert_eq!(index.nodes, vec!["a", "b", "c", "d"]);
    assert_eq!(index.edges.len(), session.scene().edges.len());
    assert_eq!(index.ports[0].node, "a");
    assert_eq!(index.ports[0].name, "out");
    assert_eq!(index.tethers[0].child, "d");
    assert_eq!(index.tethers[0].parent, "a");

    session.apply_drag_world("d", FLOW_STEP_H, 0.0).unwrap();
    assert_eq!(session.scene_index().revision, 2);
    assert_eq!(
        session.scene_index().nodes,
        index.nodes,
        "a drag changes geometry, never membership"
    );
}

#[test]
fn the_published_descriptor_matches_what_the_encoder_writes() {
    let descriptor = abi_descriptor();
    assert_eq!(descriptor.version, SCENE_BUFFER_VERSION);
    assert_eq!(descriptor.scene_header_len, SCENE_HEADER_LEN);
    assert_eq!(descriptor.node_stride, NODE_STRIDE);
    assert_eq!(descriptor.port_stride, PORT_STRIDE);
    assert_eq!(descriptor.edge_head_len, EDGE_HEAD_LEN);
    assert_eq!(descriptor.tether_head_len, TETHER_HEAD_LEN);
    assert_eq!(descriptor.segment_stride, SEGMENT_STRIDE);
    assert_eq!(descriptor.patch_header_len, PATCH_HEADER_LEN);
    assert_eq!(descriptor.patch_node_stride, PATCH_NODE_STRIDE);
    assert_eq!(descriptor.curve_ops.line, CURVE_OP_LINE);
    assert_eq!(descriptor.curve_ops.cubic, CURVE_OP_CUBIC);
    assert_eq!(descriptor.port_sides.bottom, PortSide::Bottom.code());
    assert_eq!(descriptor.hit_index_bits, 28);

    // An empty scene is exactly one header — the leanest proof the offsets in
    // the descriptor are the offsets the encoder uses.
    let empty = GeometrySession::open(ds_canvas_graph::GraphInput::default()).unwrap();
    assert_eq!(empty.scene_buffer().len(), descriptor.scene_header_len);

    let json = GeometrySession::abi_descriptor_json().unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["sceneHeaderLen"], SCENE_HEADER_LEN);
    assert_eq!(value["segmentStride"], SEGMENT_STRIDE);
    assert_eq!(value["hitKinds"]["port"], 2);
}

#[test]
fn the_free_functions_and_the_session_encode_identically() {
    let mut session = session(Orientation::Horizontal);
    assert_eq!(
        session.scene_buffer(),
        encode_scene(session.scene(), session.revision())
    );
    let outcome = session.apply_drag_world("c", 0.0, CROSS_STEP_H).unwrap();
    assert_eq!(
        GeometrySession::drag_buffer(&outcome),
        encode_drag(&outcome)
    );
}

#[test]
fn scene_json_is_available_for_durable_interchange() {
    let session = session(Orientation::Horizontal);
    let value: serde_json::Value = serde_json::from_str(&session.scene_json().unwrap()).unwrap();

    assert_eq!(value["revision"], 1);
    assert_eq!(value["scene"]["orientation"], "horizontal");
    assert_eq!(value["scene"]["nodes"][0]["id"], "a");
    assert_eq!(value["scene"]["nodes"][0]["rect"]["width"], 212.0);
    assert_eq!(
        value["scene"]["edges"][0]["curve"]["segments"][0]["op"],
        "line"
    );
    assert_eq!(value["scene"]["ports"][0]["side"], "right");
    assert!(value["scene"]["bounds"]["minX"].is_number());
}
