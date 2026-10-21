//! Deterministic layout and routing.
//!
//! Placement comes from the authored cell. Routing comes from geometry alone —
//! anchor faces, the gap between them, and the metrics. There is no solver, no
//! randomness and no clock, so the same input always produces bit-identical
//! coordinates, which is the only reason a fixture can pin a route at all.
//!
//! ## Route shapes
//!
//! Four cases, chosen by the resolved anchor faces and the authored cells:
//!
//! 1. **facing across the flow gap** — a Z elbow turning at the mid-gap;
//! 2. **same flow step, cross-axis faces** — a sidestep to an outward rail, so
//!    a route between two nodes in one column never lies over the nodes
//!    between them;
//! 3. **mixed axes** — an L with a stub at each end;
//! 4. **facing the same way or away** — around an outward rail with stubs.
//!
//! Cases 1 and 2 are the shapes the reviewed TypeScript manifest already draws.
//! Cases 3 and 4 exist because named ports can point anywhere, and a route that
//! ignored its own anchor face would leave a box through its side.

use std::collections::BTreeMap;

use crate::curve::{Curve, CurveSegment, Point, polyline_midpoint, simplify_polyline};
use crate::error::GeometryError;
use crate::input::{
    Axis, EdgeInput, EdgeShape, EndpointInput, GraphInput, LayoutMetrics, NodeInput, Orientation,
    PortSide,
};
use crate::scene::{Bounds, EdgeRoute, NodeRect, PortAnchor, Rect, Scene, TetherRoute};

/// Validate, place, route, and bound one graph.
pub fn build_scene(input: &GraphInput) -> Result<Scene, GeometryError> {
    input.validate()?;

    let metrics = &input.metrics;
    let orientation = input.orientation;

    let nodes: Vec<NodeRect> = input
        .nodes
        .iter()
        .map(|node| place_node(node, orientation, metrics))
        .collect();
    let node_index: BTreeMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), index))
        .collect();

    let mut ports: Vec<PortAnchor> = Vec::new();
    let mut port_index: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (node_position, node) in input.nodes.iter().enumerate() {
        for port in &node.ports {
            port_index.insert((node.id.clone(), port.name.clone()), ports.len());
            ports.push(PortAnchor {
                node_index: node_position,
                node_id: node.id.clone(),
                name: port.name.clone(),
                side: port.side,
                point: nodes[node_position]
                    .rect
                    .point_on_side(port.side, port.offset),
            });
        }
    }

    let mut edges: Vec<EdgeRoute> = Vec::with_capacity(input.edges.len());
    let mut edge_index: BTreeMap<String, usize> = BTreeMap::new();
    for edge in &input.edges {
        edge_index.insert(edge.id.clone(), edges.len());
        edges.push(route_edge(
            edge,
            orientation,
            metrics,
            &nodes,
            &node_index,
            &ports,
            &port_index,
        ));
    }

    let tethers: Vec<TetherRoute> = input
        .nodes
        .iter()
        .filter_map(|node| route_tether(node, orientation, metrics, &nodes, &node_index))
        .collect();

    let bounds = compute_bounds(&nodes, &edges, &tethers, metrics.scene_padding);

    Ok(Scene {
        orientation,
        nodes,
        ports,
        edges,
        tethers,
        bounds,
        node_index,
        port_index,
        edge_index,
    })
}

fn place_node(node: &NodeInput, orientation: Orientation, metrics: &LayoutMetrics) -> NodeRect {
    let flow = f64::from(node.column) * metrics.flow_step(orientation);
    let cross = f64::from(node.row) * metrics.cross_step(orientation);
    let (x, y) = match orientation {
        Orientation::Horizontal => (flow, cross),
        Orientation::Vertical => (cross, flow),
    };
    let rect = Rect {
        x,
        y,
        width: node.width.unwrap_or(metrics.node_width),
        height: node.height.unwrap_or(metrics.node_height),
    };
    NodeRect {
        id: node.id.clone(),
        column: node.column,
        row: node.row,
        rect,
        primary_label: Point::new(
            rect.x + rect.width * metrics.label_x_ratio,
            rect.y + rect.height * metrics.primary_label_y_ratio,
        ),
        secondary_label: Point::new(
            rect.x + rect.width * metrics.label_x_ratio,
            rect.y + rect.height * metrics.secondary_label_y_ratio,
        ),
    }
}

/// A resolved end of a route: where it starts and which way it must leave.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Anchor {
    pub(crate) point: Point,
    pub(crate) side: PortSide,
}

impl Anchor {
    fn stub_point(&self, distance: f64) -> Point {
        let (nx, ny) = self.side.normal();
        Point::new(self.point.x + nx * distance, self.point.y + ny * distance)
    }
}

/// The faces a route uses when no port names one. Mirrors the reviewed
/// manifest's three cases, generalized over orientation.
fn default_sides(orientation: Orientation, from: &NodeRect, to: &NodeRect) -> (PortSide, PortSide) {
    let from_center = from.rect.center();
    let to_center = to.rect.center();
    if from.column == to.column {
        // Same flow step: leave on a cross-axis face, toward the other node.
        return match orientation {
            Orientation::Horizontal => {
                if to_center.y > from_center.y {
                    (PortSide::Bottom, PortSide::Top)
                } else {
                    (PortSide::Top, PortSide::Bottom)
                }
            }
            Orientation::Vertical => {
                if to_center.x > from_center.x {
                    (PortSide::Right, PortSide::Left)
                } else {
                    (PortSide::Left, PortSide::Right)
                }
            }
        };
    }
    let forward = to.column > from.column;
    let exit = orientation.flow_exit_side();
    if forward {
        (exit, exit.opposite())
    } else {
        (exit.opposite(), exit)
    }
}

#[allow(clippy::too_many_arguments)]
fn route_edge(
    edge: &EdgeInput,
    orientation: Orientation,
    metrics: &LayoutMetrics,
    nodes: &[NodeRect],
    node_index: &BTreeMap<String, usize>,
    ports: &[PortAnchor],
    port_index: &BTreeMap<(String, String), usize>,
) -> EdgeRoute {
    // Validation already proved both endpoints and any named ports exist.
    let from_node = node_index[edge.from.node.as_str()];
    let to_node = node_index[edge.to.node.as_str()];
    let from_port = resolve_port(&edge.from, port_index);
    let to_port = resolve_port(&edge.to, port_index);
    let (default_from_side, default_to_side) =
        default_sides(orientation, &nodes[from_node], &nodes[to_node]);

    let from_anchor = anchor_for(from_port, ports, &nodes[from_node], default_from_side);
    let to_anchor = anchor_for(to_port, ports, &nodes[to_node], default_to_side);

    let orthogonal = orthogonal_points(
        orientation,
        metrics,
        &nodes[from_node],
        &nodes[to_node],
        from_anchor,
        to_anchor,
    );
    let curve = shape_curve(edge.shape, from_anchor, to_anchor, &orthogonal, metrics);
    let polyline = curve.flatten(metrics.curve_flatten_steps);
    let label = polyline_midpoint(&polyline);

    EdgeRoute {
        id: edge.id.clone(),
        from_node_index: from_node,
        to_node_index: to_node,
        from_port_index: from_port,
        to_port_index: to_port,
        shape: edge.shape,
        curve,
        polyline,
        label,
    }
}

fn resolve_port(
    endpoint: &EndpointInput,
    port_index: &BTreeMap<(String, String), usize>,
) -> Option<usize> {
    endpoint.port.as_ref().and_then(|port| {
        port_index
            .get(&(endpoint.node.clone(), port.clone()))
            .copied()
    })
}

fn anchor_for(
    port: Option<usize>,
    ports: &[PortAnchor],
    node: &NodeRect,
    default_side: PortSide,
) -> Anchor {
    match port {
        Some(index) => Anchor {
            point: ports[index].point,
            side: ports[index].side,
        },
        None => Anchor {
            point: node.rect.point_on_side(default_side, 0.5),
            side: default_side,
        },
    }
}

/// The orthogonal polyline for one pair of anchors. Every shape is derived
/// from this, so a quadratic and an orthogonal route of the same edge agree
/// about where the connector turns.
pub(crate) fn orthogonal_points(
    orientation: Orientation,
    metrics: &LayoutMetrics,
    from: &NodeRect,
    to: &NodeRect,
    start: Anchor,
    end: Anchor,
) -> Vec<Point> {
    let cross_axis = match orientation.flow_axis() {
        Axis::X => Axis::Y,
        Axis::Y => Axis::X,
    };
    let same_flow_step = from.column == to.column;
    let both_cross_faces = start.side.axis() == cross_axis && end.side.axis() == cross_axis;

    let points = if same_flow_step && both_cross_faces {
        sidestep_route(orientation, metrics, from, to, start, end)
    } else if start.side.axis() == end.side.axis() {
        same_axis_route(metrics, from, to, start, end)
    } else {
        mixed_axis_route(metrics, start, end)
    };
    simplify_polyline(&points)
}

/// Two nodes in one flow step: step out to a rail beyond both of them, travel
/// along it, and come back. The reviewed manifest's same-column shape.
fn sidestep_route(
    orientation: Orientation,
    metrics: &LayoutMetrics,
    from: &NodeRect,
    to: &NodeRect,
    start: Anchor,
    end: Anchor,
) -> Vec<Point> {
    let start_stub = start.stub_point(metrics.stub);
    let end_stub = end.stub_point(metrics.stub);
    let rail_offset = metrics.column_gap / 3.0;
    match orientation {
        Orientation::Horizontal => {
            let rail = from.rect.max_x().max(to.rect.max_x()) + rail_offset;
            vec![
                start.point,
                Point::new(start.point.x, start_stub.y),
                Point::new(rail, start_stub.y),
                Point::new(rail, end_stub.y),
                Point::new(end.point.x, end_stub.y),
                end.point,
            ]
        }
        Orientation::Vertical => {
            let rail = from.rect.max_y().max(to.rect.max_y()) + rail_offset;
            vec![
                start.point,
                Point::new(start_stub.x, start.point.y),
                Point::new(start_stub.x, rail),
                Point::new(end_stub.x, rail),
                Point::new(end_stub.x, end.point.y),
                end.point,
            ]
        }
    }
}

/// Both anchors leave along the same axis. Facing each other gives the Z elbow
/// at the mid-gap; anything else routes around an outward rail.
fn same_axis_route(
    metrics: &LayoutMetrics,
    from: &NodeRect,
    to: &NodeRect,
    start: Anchor,
    end: Anchor,
) -> Vec<Point> {
    let facing = start.side == end.side.opposite() && leaves_toward(start, end.point);
    match start.side.axis() {
        Axis::X => {
            if facing {
                let mid = (start.point.x + end.point.x) / 2.0;
                vec![
                    start.point,
                    Point::new(mid, start.point.y),
                    Point::new(mid, end.point.y),
                    end.point,
                ]
            } else {
                let start_stub = start.stub_point(metrics.stub);
                let end_stub = end.stub_point(metrics.stub);
                let rail = if to.rect.center().y >= from.rect.center().y {
                    from.rect.max_y().max(to.rect.max_y()) + metrics.tether_rail_gap
                } else {
                    from.rect.y.min(to.rect.y) - metrics.tether_rail_gap
                };
                vec![
                    start.point,
                    Point::new(start_stub.x, start.point.y),
                    Point::new(start_stub.x, rail),
                    Point::new(end_stub.x, rail),
                    Point::new(end_stub.x, end.point.y),
                    end.point,
                ]
            }
        }
        Axis::Y => {
            if facing {
                let mid = (start.point.y + end.point.y) / 2.0;
                vec![
                    start.point,
                    Point::new(start.point.x, mid),
                    Point::new(end.point.x, mid),
                    end.point,
                ]
            } else {
                let start_stub = start.stub_point(metrics.stub);
                let end_stub = end.stub_point(metrics.stub);
                let rail = if to.rect.center().x >= from.rect.center().x {
                    from.rect.max_x().max(to.rect.max_x()) + metrics.tether_rail_gap
                } else {
                    from.rect.x.min(to.rect.x) - metrics.tether_rail_gap
                };
                vec![
                    start.point,
                    Point::new(start.point.x, start_stub.y),
                    Point::new(rail, start_stub.y),
                    Point::new(rail, end_stub.y),
                    Point::new(end.point.x, end_stub.y),
                    end.point,
                ]
            }
        }
    }
}

/// Anchors on perpendicular faces: leave on each stub, then one corner.
fn mixed_axis_route(metrics: &LayoutMetrics, start: Anchor, end: Anchor) -> Vec<Point> {
    let start_stub = start.stub_point(metrics.stub);
    let end_stub = end.stub_point(metrics.stub);
    match start.side.axis() {
        Axis::X => vec![
            start.point,
            Point::new(start_stub.x, start.point.y),
            Point::new(start_stub.x, end_stub.y),
            Point::new(end.point.x, end_stub.y),
            end.point,
        ],
        Axis::Y => vec![
            start.point,
            Point::new(start.point.x, start_stub.y),
            Point::new(end_stub.x, start_stub.y),
            Point::new(end_stub.x, end.point.y),
            end.point,
        ],
    }
}

/// Does this anchor's outward normal point toward `target`?
fn leaves_toward(anchor: Anchor, target: Point) -> bool {
    let (nx, ny) = anchor.side.normal();
    (target.x - anchor.point.x) * nx + (target.y - anchor.point.y) * ny >= 0.0
}

pub(crate) fn shape_curve(
    shape: EdgeShape,
    start: Anchor,
    end: Anchor,
    orthogonal: &[Point],
    metrics: &LayoutMetrics,
) -> Curve {
    match shape {
        EdgeShape::Orthogonal => Curve::polyline_curve(orthogonal),
        EdgeShape::Straight => Curve::new(start.point, vec![CurveSegment::Line { to: end.point }]),
        EdgeShape::Quadratic => Curve::new(
            start.point,
            vec![CurveSegment::Quadratic {
                control: quadratic_control(orthogonal, start.point, end.point),
                to: end.point,
            }],
        ),
        EdgeShape::Cubic => {
            let bow_a = bow(start, end.point, metrics);
            let bow_b = bow(end, start.point, metrics);
            Curve::new(
                start.point,
                vec![CurveSegment::Cubic {
                    control_a: start.stub_point(bow_a),
                    control_b: end.stub_point(bow_b),
                    to: end.point,
                }],
            )
        }
    }
}

/// The orthogonal route's middle corner. Using the elbow the orthogonal shape
/// already chose keeps a quadratic bending the same way its orthogonal
/// counterpart turns, instead of inventing a second opinion about the edge.
fn quadratic_control(orthogonal: &[Point], start: Point, end: Point) -> Point {
    if orthogonal.len() < 3 {
        return start.lerp(end, 0.5);
    }
    let interior = &orthogonal[1..orthogonal.len() - 1];
    interior[(interior.len() - 1) / 2]
}

/// How far a cubic's handle reaches: at least one stub, and otherwise half the
/// distance the route travels along that anchor's own normal.
fn bow(anchor: Anchor, target: Point, metrics: &LayoutMetrics) -> f64 {
    let (nx, ny) = anchor.side.normal();
    let along = ((target.x - anchor.point.x) * nx + (target.y - anchor.point.y) * ny).abs();
    metrics.stub.max(along / 2.0)
}

/// A tether hugs the outward side of both boxes so containment never crosses
/// the diagram it is describing.
fn route_tether(
    node: &NodeInput,
    orientation: Orientation,
    metrics: &LayoutMetrics,
    nodes: &[NodeRect],
    node_index: &BTreeMap<String, usize>,
) -> Option<TetherRoute> {
    let parent_id = node.parent.as_deref()?;
    let child_index = *node_index.get(node.id.as_str())?;
    let parent_index = *node_index.get(parent_id)?;
    let child = &nodes[child_index].rect;
    let parent = &nodes[parent_index].rect;

    let points = match orientation {
        Orientation::Horizontal => {
            let rail = child.x.min(parent.x) - metrics.tether_rail_gap;
            vec![
                Point::new(parent.x, parent.center().y),
                Point::new(rail, parent.center().y),
                Point::new(rail, child.center().y),
                Point::new(child.x, child.center().y),
            ]
        }
        Orientation::Vertical => {
            let rail = child.y.min(parent.y) - metrics.tether_rail_gap;
            vec![
                Point::new(parent.center().x, parent.y),
                Point::new(parent.center().x, rail),
                Point::new(child.center().x, rail),
                Point::new(child.center().x, child.y),
            ]
        }
    };
    let points = simplify_polyline(&points);
    let curve = Curve::polyline_curve(&points);
    Some(TetherRoute {
        child_node_index: child_index,
        parent_node_index: parent_index,
        curve,
        polyline: points,
    })
}

/// Scene extent covers node rectangles **and** route geometry: a sidestep rail
/// or a tether rail sits outside every box, and a bound that ignored it would
/// clip the drawing the first time a viewport fitted the scene.
pub(crate) fn compute_bounds(
    nodes: &[NodeRect],
    edges: &[EdgeRoute],
    tethers: &[TetherRoute],
    padding: f64,
) -> Bounds {
    let Some(first) = nodes.first() else {
        return Bounds::EMPTY;
    };
    let mut bounds = Bounds::seed(Point::new(first.rect.x, first.rect.y));
    for node in nodes {
        bounds.expand_rect(&node.rect);
    }
    for edge in edges {
        for point in &edge.polyline {
            bounds.expand_point(*point);
        }
    }
    for tether in tethers {
        for point in &tether.polyline {
            bounds.expand_point(*point);
        }
    }
    bounds.pad(padding)
}
