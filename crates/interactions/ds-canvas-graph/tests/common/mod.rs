//! Shared fixtures. Every test builds its graph from these so a change in one
//! expectation cannot quietly disagree with another test's idea of the same
//! scene.

#![allow(dead_code)]

use ds_canvas_graph::{
    EdgeInput, EdgeShape, EndpointInput, GraphInput, LayoutMetrics, NodeInput, Orientation,
    PortSide, PortSpec,
};

pub fn node(id: &str, column: i32, row: i32) -> NodeInput {
    NodeInput {
        id: id.to_string(),
        column,
        row,
        width: None,
        height: None,
        parent: None,
        ports: Vec::new(),
    }
}

pub fn contained(id: &str, column: i32, row: i32, parent: &str) -> NodeInput {
    NodeInput {
        parent: Some(parent.to_string()),
        ..node(id, column, row)
    }
}

pub fn port(name: &str, side: PortSide, offset: f64) -> PortSpec {
    PortSpec {
        name: name.to_string(),
        side,
        offset,
    }
}

pub fn edge(id: &str, from: &str, to: &str) -> EdgeInput {
    EdgeInput {
        id: id.to_string(),
        from: EndpointInput {
            node: from.to_string(),
            port: None,
        },
        to: EndpointInput {
            node: to.to_string(),
            port: None,
        },
        shape: EdgeShape::Orthogonal,
    }
}

pub fn shaped_edge(id: &str, from: &str, to: &str, shape: EdgeShape) -> EdgeInput {
    EdgeInput {
        shape,
        ..edge(id, from, to)
    }
}

pub fn ported_edge(id: &str, from: (&str, &str), to: (&str, &str)) -> EdgeInput {
    EdgeInput {
        id: id.to_string(),
        from: EndpointInput {
            node: from.0.to_string(),
            port: Some(from.1.to_string()),
        },
        to: EndpointInput {
            node: to.0.to_string(),
            port: Some(to.1.to_string()),
        },
        shape: EdgeShape::Orthogonal,
    }
}

/// Four nodes in two flow steps and two lanes, one flow edge, one cross edge,
/// one same-step edge, and one containment tether. Small enough to assert every
/// coordinate by hand; broad enough that all three route shapes appear.
pub fn chapter(orientation: Orientation) -> GraphInput {
    GraphInput {
        orientation,
        metrics: LayoutMetrics::default(),
        nodes: vec![
            node("a", 0, 0),
            node("b", 1, 0),
            node("c", 1, 1),
            contained("d", 0, 1, "a"),
        ],
        edges: vec![
            edge("a-b", "a", "b"),
            edge("a-c", "a", "c"),
            edge("a-d", "a", "d"),
        ],
    }
}

pub const FLOW_STEP_H: f64 = 212.0 + 92.0;
pub const CROSS_STEP_H: f64 = 64.0 + 26.0;
pub const FLOW_STEP_V: f64 = 64.0 + 92.0;
pub const CROSS_STEP_V: f64 = 212.0 + 26.0;

#[track_caller]
pub fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected {expected}, got {actual}"
    );
}
