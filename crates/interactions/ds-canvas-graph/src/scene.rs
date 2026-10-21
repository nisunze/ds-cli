//! The computed scene — the authoritative answer.
//!
//! Everything here is a finite `f64` world coordinate. A renderer paints these
//! after applying one projection; it does not adjust, re-derive or persist
//! them. Identity is carried as both an id and a dense index: the index is what
//! the hot packed buffer speaks, and the id table is fetched once per revision
//! rather than re-sent on every pointer move.

use serde::Serialize;
use std::collections::BTreeMap;

use crate::curve::{Curve, Point};
use crate::input::{EdgeId, EdgeShape, NodeId, Orientation, PortName, PortSide};

/// An axis-aligned world rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x && point.x <= self.max_x() && point.y >= self.y && point.y <= self.max_y()
    }

    /// The anchor `offset` of the way along one face.
    pub fn point_on_side(&self, side: PortSide, offset: f64) -> Point {
        let offset = offset.clamp(0.0, 1.0);
        match side {
            PortSide::Left => Point::new(self.x, self.y + self.height * offset),
            PortSide::Right => Point::new(self.max_x(), self.y + self.height * offset),
            PortSide::Top => Point::new(self.x + self.width * offset, self.y),
            PortSide::Bottom => Point::new(self.x + self.width * offset, self.max_y()),
        }
    }
}

/// Scene extent in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Bounds {
    /// The extent an empty graph reports. Non-degenerate on purpose: a zero
    /// -size scene would divide by zero the first time a viewport fit it.
    pub const EMPTY: Self = Self {
        min_x: 0.0,
        min_y: 0.0,
        max_x: 1.0,
        max_y: 1.0,
    };

    pub(crate) fn seed(point: Point) -> Self {
        Self {
            min_x: point.x,
            min_y: point.y,
            max_x: point.x,
            max_y: point.y,
        }
    }

    pub(crate) fn expand_point(&mut self, point: Point) {
        self.min_x = self.min_x.min(point.x);
        self.min_y = self.min_y.min(point.y);
        self.max_x = self.max_x.max(point.x);
        self.max_y = self.max_y.max(point.y);
    }

    pub(crate) fn expand_rect(&mut self, rect: &Rect) {
        self.expand_point(Point::new(rect.x, rect.y));
        self.expand_point(Point::new(rect.max_x(), rect.max_y()));
    }

    pub(crate) fn pad(self, padding: f64) -> Self {
        Self {
            min_x: self.min_x - padding,
            min_y: self.min_y - padding,
            max_x: self.max_x + padding,
            max_y: self.max_y + padding,
        }
    }

    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }
}

/// One placed node.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRect {
    pub id: NodeId,
    pub column: i32,
    pub row: i32,
    pub rect: Rect,
    /// Baseline anchors for the node's primary and secondary host-rendered
    /// labels. Rust owns the coordinates; the host owns font and colour.
    pub primary_label: Point,
    pub secondary_label: Point,
}

/// One resolved port anchor. Present for every authored port whether or not an
/// edge uses it — a host draws and hit-tests ports it did not connect.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortAnchor {
    pub node_index: usize,
    pub node_id: NodeId,
    pub name: PortName,
    pub side: PortSide,
    pub point: Point,
}

/// One routed edge.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeRoute {
    pub id: EdgeId,
    pub from_node_index: usize,
    pub to_node_index: usize,
    /// Index into [`Scene::ports`], or `None` when geometry chose the face.
    pub from_port_index: Option<usize>,
    pub to_port_index: Option<usize>,
    pub shape: EdgeShape,
    pub curve: Curve,
    /// The curve flattened once, for bounds and hit testing. A host may draw
    /// from this instead of the curve if it does not want bezier support.
    pub polyline: Vec<Point>,
    /// Where this edge's label wants to sit — the arc-length midpoint.
    pub label: Point,
}

/// One containment tether: a child to the node it lives inside.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TetherRoute {
    pub child_node_index: usize,
    pub parent_node_index: usize,
    pub curve: Curve,
    pub polyline: Vec<Point>,
}

/// The authoritative scene.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub orientation: Orientation,
    pub nodes: Vec<NodeRect>,
    pub ports: Vec<PortAnchor>,
    pub edges: Vec<EdgeRoute>,
    pub tethers: Vec<TetherRoute>,
    pub bounds: Bounds,
    #[serde(skip)]
    pub(crate) node_index: BTreeMap<NodeId, usize>,
    #[serde(skip)]
    pub(crate) port_index: BTreeMap<(NodeId, PortName), usize>,
    #[serde(skip)]
    pub(crate) edge_index: BTreeMap<EdgeId, usize>,
}

impl Scene {
    pub fn node_index_of(&self, id: &str) -> Option<usize> {
        self.node_index.get(id).copied()
    }

    pub fn node(&self, id: &str) -> Option<&NodeRect> {
        self.node_index_of(id).map(|index| &self.nodes[index])
    }

    pub fn port_index_of(&self, node: &str, port: &str) -> Option<usize> {
        self.port_index
            .get(&(node.to_string(), port.to_string()))
            .copied()
    }

    pub fn edge_index_of(&self, id: &str) -> Option<usize> {
        self.edge_index.get(id).copied()
    }
}
