//! The typed, domain-neutral graph input.
//!
//! Nothing here knows what a node *is*. There is no lane, kind, confidence,
//! evidence, delivery state, task, or milestone: those belong to the surface
//! that authored the graph, and two surfaces sharing this boundary must not be
//! able to see each other's business objects through it. What crosses is an
//! identity, an authored cell, an optional size, an optional container, and
//! named ports.
//!
//! ## Authored placement, and what orientation does to it
//!
//! `column` is the position along the flow axis and `row` the position across
//! it. Orientation decides which screen axis each one becomes:
//!
//! | | flow axis (`column`) | cross axis (`row`) |
//! |---|---|---|
//! | `Horizontal` | +x | +y |
//! | `Vertical` | +y | +x |
//!
//! So one authored graph draws both ways without re-authoring, and `column_gap`
//! always means "between successive flow steps" regardless of orientation.
//!
//! Placement is authored, never solved. A force layout looks different every
//! time the graph is edited, so a reader cannot tell a content change from a
//! jitter change — and the page they learned last week is gone.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::error::{GeometryError, finite, finite_non_negative, finite_positive};

pub type NodeId = String;
pub type EdgeId = String;
pub type PortName = String;

/// Bound on one input graph. Past these counts the caller is asking the canvas
/// to draw something no reader can read, and a browser tab pays for it.
pub const MAX_NODES: usize = 20_000;
pub const MAX_EDGES: usize = 60_000;
pub const MAX_PORTS_PER_NODE: usize = 64;

/// Which screen axis the flow runs along.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    #[default]
    Horizontal,
    Vertical,
}

impl Orientation {
    /// The face a forward edge leaves from when no port is named.
    pub(crate) fn flow_exit_side(self) -> PortSide {
        match self {
            Self::Horizontal => PortSide::Right,
            Self::Vertical => PortSide::Bottom,
        }
    }

    /// The axis `column` advances along.
    pub(crate) fn flow_axis(self) -> Axis {
        match self {
            Self::Horizontal => Axis::X,
            Self::Vertical => Axis::Y,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Axis {
    X,
    Y,
}

/// Which face of a node rectangle a port sits on.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl PortSide {
    /// Wire code, shared with the packed buffer.
    pub fn code(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Top => 2,
            Self::Bottom => 3,
        }
    }

    pub(crate) fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::X,
            Self::Top | Self::Bottom => Axis::Y,
        }
    }

    /// Unit outward normal. A route's stub always leaves along this.
    pub(crate) fn normal(self) -> (f64, f64) {
        match self {
            Self::Left => (-1.0, 0.0),
            Self::Right => (1.0, 0.0),
            Self::Top => (0.0, -1.0),
            Self::Bottom => (0.0, 1.0),
        }
    }

    pub(crate) fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
        }
    }
}

/// A named anchor on a node face. `offset` is the fraction along that face,
/// `0.0` at the left/top end and `1.0` at the right/bottom end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortSpec {
    pub name: PortName,
    pub side: PortSide,
    #[serde(default = "default_port_offset")]
    pub offset: f64,
}

fn default_port_offset() -> f64 {
    0.5
}

/// One node. `width`/`height` override the metric defaults for this node only;
/// the authored grid step does not change with them, so an oversized node
/// overlaps its neighbours' cells deliberately rather than silently reflowing
/// every other node on the page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeInput {
    pub id: NodeId,
    pub column: i32,
    pub row: i32,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub height: Option<f64>,
    /// Structural containment, drawn as a tether. Containment is not a flow:
    /// an edge answers "what moves", a tether answers "what is inside what".
    #[serde(default)]
    pub parent: Option<NodeId>,
    #[serde(default)]
    pub ports: Vec<PortSpec>,
}

/// What an edge is drawn as. All four produce the same [`crate::Curve`] type;
/// they differ only in which segments and control points it holds.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeShape {
    #[default]
    Orthogonal,
    Straight,
    Quadratic,
    Cubic,
}

/// One end of an edge. Omitting `port` asks geometry to choose the face, which
/// is what keeps an authored graph readable without authoring every anchor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EndpointInput {
    pub node: NodeId,
    #[serde(default)]
    pub port: Option<PortName>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EdgeInput {
    pub id: EdgeId,
    pub from: EndpointInput,
    pub to: EndpointInput,
    #[serde(default)]
    pub shape: EdgeShape,
}

/// Every dimension the layout uses. Hosts may override them — a browser knows
/// its font metrics and this crate does not — but they are geometry inputs,
/// not results: nothing here is measured from a canvas at draw time.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct LayoutMetrics {
    pub node_width: f64,
    pub node_height: f64,
    /// Domain-neutral text anchors inside each node. The host supplies font
    /// metrics and paint, but it never invents label coordinates.
    pub label_x_ratio: f64,
    pub primary_label_y_ratio: f64,
    pub secondary_label_y_ratio: f64,
    /// Gap between successive flow steps (`column`).
    pub column_gap: f64,
    /// Gap between cross-axis neighbours (`row`).
    pub row_gap: f64,
    pub scene_padding: f64,
    /// How far a route leaves a face before it is allowed to turn.
    pub stub: f64,
    /// How far outside both boxes a containment tether's rail sits.
    pub tether_rail_gap: f64,
    /// Screen-pixel radius in which a port wins a hit over its node.
    pub port_hit_radius_px: f64,
    /// Screen-pixel tolerance for hitting a route.
    pub edge_hit_tolerance_px: f64,
    /// Subdivisions per curved segment when flattening for bounds and hits.
    pub curve_flatten_steps: u32,
    pub max_column: i32,
    pub max_row: i32,
    /// Bound on one drag intent, in world units.
    pub max_drag_world: f64,
}

impl Default for LayoutMetrics {
    fn default() -> Self {
        Self {
            node_width: 212.0,
            node_height: 64.0,
            label_x_ratio: 9.0 / 212.0,
            primary_label_y_ratio: 23.0 / 64.0,
            secondary_label_y_ratio: 45.0 / 64.0,
            column_gap: 92.0,
            row_gap: 26.0,
            scene_padding: 40.0,
            stub: 14.0,
            tether_rail_gap: 12.0,
            port_hit_radius_px: 7.0,
            edge_hit_tolerance_px: 6.0,
            curve_flatten_steps: 16,
            max_column: 4_096,
            max_row: 4_096,
            max_drag_world: 1.0e6,
        }
    }
}

impl LayoutMetrics {
    /// Distance between the origins of successive flow steps.
    pub(crate) fn flow_step(&self, orientation: Orientation) -> f64 {
        match orientation {
            Orientation::Horizontal => self.node_width + self.column_gap,
            Orientation::Vertical => self.node_height + self.column_gap,
        }
    }

    /// Distance between the origins of cross-axis neighbours.
    pub(crate) fn cross_step(&self, orientation: Orientation) -> f64 {
        match orientation {
            Orientation::Horizontal => self.node_height + self.row_gap,
            Orientation::Vertical => self.node_width + self.row_gap,
        }
    }

    fn validate(&self) -> Result<(), GeometryError> {
        finite_positive("metrics.nodeWidth", self.node_width)?;
        finite_positive("metrics.nodeHeight", self.node_height)?;
        finite_non_negative("metrics.labelXRatio", self.label_x_ratio)?;
        finite_non_negative("metrics.primaryLabelYRatio", self.primary_label_y_ratio)?;
        finite_non_negative("metrics.secondaryLabelYRatio", self.secondary_label_y_ratio)?;
        if self.label_x_ratio > 1.0
            || self.primary_label_y_ratio > 1.0
            || self.secondary_label_y_ratio > 1.0
        {
            return Err(GeometryError::InvalidMetric {
                field: "metrics.labelXRatio/primaryLabelYRatio/secondaryLabelYRatio".to_string(),
                value: format!(
                    "{}, {}, {}",
                    self.label_x_ratio, self.primary_label_y_ratio, self.secondary_label_y_ratio
                ),
                expected: "0..=1".to_string(),
            });
        }
        finite_non_negative("metrics.columnGap", self.column_gap)?;
        finite_non_negative("metrics.rowGap", self.row_gap)?;
        finite_non_negative("metrics.scenePadding", self.scene_padding)?;
        finite_non_negative("metrics.stub", self.stub)?;
        finite_non_negative("metrics.tetherRailGap", self.tether_rail_gap)?;
        finite_non_negative("metrics.portHitRadiusPx", self.port_hit_radius_px)?;
        finite_non_negative("metrics.edgeHitTolerancePx", self.edge_hit_tolerance_px)?;
        finite_positive("metrics.maxDragWorld", self.max_drag_world)?;
        if !(1..=256).contains(&self.curve_flatten_steps) {
            return Err(GeometryError::InvalidMetric {
                field: "metrics.curveFlattenSteps".to_string(),
                value: self.curve_flatten_steps.to_string(),
                expected: "1..=256".to_string(),
            });
        }
        if self.max_column < 0 || self.max_row < 0 {
            return Err(GeometryError::InvalidMetric {
                field: "metrics.maxColumn/maxRow".to_string(),
                value: format!("{}, {}", self.max_column, self.max_row),
                expected: ">= 0".to_string(),
            });
        }
        Ok(())
    }
}

/// One complete graph to lay out.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphInput {
    #[serde(default)]
    pub orientation: Orientation,
    #[serde(default)]
    pub metrics: LayoutMetrics,
    #[serde(default)]
    pub nodes: Vec<NodeInput>,
    #[serde(default)]
    pub edges: Vec<EdgeInput>,
}

impl GraphInput {
    /// Refuse everything that cannot produce an exact scene. Called by
    /// [`crate::build_scene`]; every later stage may assume it passed.
    pub fn validate(&self) -> Result<(), GeometryError> {
        self.metrics.validate()?;
        if self.nodes.len() > MAX_NODES {
            return Err(GeometryError::InputTooLarge {
                field: "nodes".to_string(),
                count: self.nodes.len(),
                max: MAX_NODES,
            });
        }
        if self.edges.len() > MAX_EDGES {
            return Err(GeometryError::InputTooLarge {
                field: "edges".to_string(),
                count: self.edges.len(),
                max: MAX_EDGES,
            });
        }

        let mut ports_by_node: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for node in &self.nodes {
            if node.id.trim().is_empty() {
                return Err(GeometryError::EmptyIdentifier {
                    field: "nodes[].id".to_string(),
                });
            }
            if ports_by_node.contains_key(node.id.as_str()) {
                return Err(GeometryError::DuplicateNode {
                    id: node.id.clone(),
                });
            }
            if node.column < 0
                || node.row < 0
                || node.column > self.metrics.max_column
                || node.row > self.metrics.max_row
            {
                return Err(GeometryError::CellOutOfRange {
                    node: node.id.clone(),
                    column: node.column,
                    row: node.row,
                });
            }
            if let Some(width) = node.width {
                finite_positive("nodes[].width", width)?;
            }
            if let Some(height) = node.height {
                finite_positive("nodes[].height", height)?;
            }
            if node.ports.len() > MAX_PORTS_PER_NODE {
                return Err(GeometryError::InputTooLarge {
                    field: format!("nodes[{}].ports", node.id),
                    count: node.ports.len(),
                    max: MAX_PORTS_PER_NODE,
                });
            }
            let mut names: BTreeSet<&str> = BTreeSet::new();
            for port in &node.ports {
                if port.name.trim().is_empty() {
                    return Err(GeometryError::EmptyIdentifier {
                        field: format!("nodes[{}].ports[].name", node.id),
                    });
                }
                if !names.insert(port.name.as_str()) {
                    return Err(GeometryError::DuplicatePort {
                        node: node.id.clone(),
                        port: port.name.clone(),
                    });
                }
                let offset = finite("nodes[].ports[].offset", port.offset)?;
                if !(0.0..=1.0).contains(&offset) {
                    return Err(GeometryError::InvalidMetric {
                        field: format!("nodes[{}].ports[{}].offset", node.id, port.name),
                        value: format!("{offset}"),
                        expected: "0.0..=1.0".to_string(),
                    });
                }
            }
            ports_by_node.insert(node.id.as_str(), names);
        }

        for node in &self.nodes {
            let Some(parent) = node.parent.as_deref() else {
                continue;
            };
            if parent == node.id {
                return Err(GeometryError::SelfContainment {
                    node: node.id.clone(),
                });
            }
            if !ports_by_node.contains_key(parent) {
                return Err(GeometryError::UnknownNode {
                    reference: format!("nodes[{}].parent", node.id),
                    node: parent.to_string(),
                });
            }
        }
        self.check_containment_acyclic()?;

        let mut edge_ids: BTreeSet<&str> = BTreeSet::new();
        for edge in &self.edges {
            if edge.id.trim().is_empty() {
                return Err(GeometryError::EmptyIdentifier {
                    field: "edges[].id".to_string(),
                });
            }
            if !edge_ids.insert(edge.id.as_str()) {
                return Err(GeometryError::DuplicateEdge {
                    id: edge.id.clone(),
                });
            }
            for (label, endpoint) in [("from", &edge.from), ("to", &edge.to)] {
                let Some(ports) = ports_by_node.get(endpoint.node.as_str()) else {
                    return Err(GeometryError::UnknownNode {
                        reference: format!("edges[{}].{label}", edge.id),
                        node: endpoint.node.clone(),
                    });
                };
                if let Some(port) = endpoint.port.as_deref()
                    && !ports.contains(port)
                {
                    return Err(GeometryError::UnknownPort {
                        reference: format!("edges[{}].{label}", edge.id),
                        node: endpoint.node.clone(),
                        port: port.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    fn check_containment_acyclic(&self) -> Result<(), GeometryError> {
        let parents: BTreeMap<&str, &str> = self
            .nodes
            .iter()
            .filter_map(|node| {
                node.parent
                    .as_deref()
                    .map(|parent| (node.id.as_str(), parent))
            })
            .collect();
        for start in parents.keys() {
            let mut seen: BTreeSet<&str> = BTreeSet::new();
            let mut cursor = *start;
            while let Some(parent) = parents.get(cursor) {
                if !seen.insert(cursor) {
                    return Err(GeometryError::ContainmentCycle {
                        node: start.to_string(),
                    });
                }
                if *parent == *start {
                    return Err(GeometryError::ContainmentCycle {
                        node: start.to_string(),
                    });
                }
                cursor = parent;
            }
        }
        Ok(())
    }
}
