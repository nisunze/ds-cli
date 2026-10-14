//! The packed `f64` wire format.
//!
//! A pointer-move loop must not stringify a JSON tree. The scene is encoded
//! once per revision into one flat `Float64Array`, a drag answers with a patch
//! of only what changed, and a hover answers with a single packed `u32`. JSON
//! remains available for durable interchange and for debugging, which is the
//! only place it is cheap enough to be honest.
//!
//! Identity does not travel in the buffer. Ids are strings, strings are
//! expensive at the boundary, and the graph's membership does not change
//! between revisions — so the host fetches the id tables once per revision via
//! [`SceneIndex`] and speaks dense indices for everything after that.
//!
//! The format is versioned and self-describing: [`abi_descriptor`] emits every
//! offset and stride below, and a native test asserts the descriptor matches
//! what the encoder actually writes. A host that reads the descriptor cannot
//! drift from the encoder without the test failing first.

use serde::Serialize;

use crate::curve::{Curve, Point};
use crate::drag::{DragOutcome, ScenePatch};
use crate::error::GeometryError;
use crate::hit::{
    HIT_INDEX_BITS, HIT_KIND_EDGE, HIT_KIND_NODE, HIT_KIND_NONE, HIT_KIND_PORT, HIT_KIND_TETHER,
};
use crate::input::{Orientation, PortSide};
use crate::scene::Scene;

/// Bump when any offset, stride or opcode below changes.
pub const SCENE_BUFFER_VERSION: f64 = 1.0;

pub const SCENE_HEADER_LEN: usize = 11;
pub const NODE_STRIDE: usize = 8;
pub const PORT_STRIDE: usize = 4;
pub const EDGE_HEAD_LEN: usize = 9;
pub const TETHER_HEAD_LEN: usize = 5;
pub const SEGMENT_STRIDE: usize = 7;

pub const PATCH_HEADER_LEN: usize = 14;
pub const PATCH_NODE_STRIDE: usize = 9;
pub const PATCH_EDGE_HEAD_LEN: usize = 10;
pub const PATCH_TETHER_HEAD_LEN: usize = 6;

/// Written where an endpoint uses a geometry-chosen face instead of a port.
pub const NO_PORT: f64 = -1.0;

fn push_point(out: &mut Vec<f64>, point: Point) {
    out.push(point.x);
    out.push(point.y);
}

/// One curve as `start_x, start_y, segment_count` followed by fixed-stride
/// segments. Every segment is the same width whatever its op, so a reader
/// advances by a constant and never parses to find the next record.
fn push_curve(out: &mut Vec<f64>, curve: &Curve) {
    push_point(out, curve.start);
    out.push(curve.segments.len() as f64);
    for segment in &curve.segments {
        let [control_a, control_b] = segment.control_slots();
        out.push(f64::from(segment.op_code()));
        push_point(out, control_a);
        push_point(out, control_b);
        push_point(out, segment.end());
    }
}

fn orientation_code(orientation: Orientation) -> f64 {
    match orientation {
        Orientation::Horizontal => 0.0,
        Orientation::Vertical => 1.0,
    }
}

fn port_slot(index: Option<usize>) -> f64 {
    index.map_or(NO_PORT, |index| index as f64)
}

/// Encode a whole scene. Fetched when [`crate::GeometrySession::revision`]
/// changes, not per frame.
pub fn encode_scene(scene: &Scene, revision: u32) -> Vec<f64> {
    let mut out = Vec::with_capacity(
        SCENE_HEADER_LEN
            + scene.nodes.len() * NODE_STRIDE
            + scene.ports.len() * PORT_STRIDE
            + scene.edges.len() * (EDGE_HEAD_LEN + 4 * SEGMENT_STRIDE),
    );
    out.push(SCENE_BUFFER_VERSION);
    out.push(f64::from(revision));
    out.push(orientation_code(scene.orientation));
    out.push(scene.nodes.len() as f64);
    out.push(scene.ports.len() as f64);
    out.push(scene.edges.len() as f64);
    out.push(scene.tethers.len() as f64);
    out.push(scene.bounds.min_x);
    out.push(scene.bounds.min_y);
    out.push(scene.bounds.max_x);
    out.push(scene.bounds.max_y);

    for node in &scene.nodes {
        out.push(node.rect.x);
        out.push(node.rect.y);
        out.push(node.rect.width);
        out.push(node.rect.height);
        push_point(&mut out, node.primary_label);
        push_point(&mut out, node.secondary_label);
    }
    for port in &scene.ports {
        out.push(port.node_index as f64);
        out.push(f64::from(port.side.code()));
        push_point(&mut out, port.point);
    }
    for edge in &scene.edges {
        out.push(edge.from_node_index as f64);
        out.push(edge.to_node_index as f64);
        out.push(port_slot(edge.from_port_index));
        out.push(port_slot(edge.to_port_index));
        push_point(&mut out, edge.label);
        push_curve(&mut out, &edge.curve);
    }
    for tether in &scene.tethers {
        out.push(tether.child_node_index as f64);
        out.push(tether.parent_node_index as f64);
        push_curve(&mut out, &tether.curve);
    }
    out
}

/// Encode a drag answer: the resolved cell plus only the geometry that moved.
pub fn encode_drag(outcome: &DragOutcome) -> Vec<f64> {
    let patch: &ScenePatch = &outcome.patch;
    let mut out = Vec::with_capacity(PATCH_HEADER_LEN + patch.nodes.len() * PATCH_NODE_STRIDE);
    out.push(SCENE_BUFFER_VERSION);
    out.push(f64::from(patch.revision));
    out.push(f64::from(u8::from(outcome.moved)));
    out.push(f64::from(u8::from(outcome.clamped)));
    out.push(outcome.node_index as f64);
    out.push(f64::from(outcome.to_column));
    out.push(f64::from(outcome.to_row));
    out.push(patch.nodes.len() as f64);
    out.push(patch.edges.len() as f64);
    out.push(patch.tethers.len() as f64);
    out.push(patch.bounds.min_x);
    out.push(patch.bounds.min_y);
    out.push(patch.bounds.max_x);
    out.push(patch.bounds.max_y);

    for patched in &patch.nodes {
        out.push(patched.index as f64);
        out.push(patched.node.rect.x);
        out.push(patched.node.rect.y);
        out.push(patched.node.rect.width);
        out.push(patched.node.rect.height);
        push_point(&mut out, patched.node.primary_label);
        push_point(&mut out, patched.node.secondary_label);
    }
    for patched in &patch.edges {
        out.push(patched.index as f64);
        out.push(patched.route.from_node_index as f64);
        out.push(patched.route.to_node_index as f64);
        out.push(port_slot(patched.route.from_port_index));
        out.push(port_slot(patched.route.to_port_index));
        push_point(&mut out, patched.route.label);
        push_curve(&mut out, &patched.route.curve);
    }
    for patched in &patch.tethers {
        out.push(patched.index as f64);
        out.push(patched.route.child_node_index as f64);
        out.push(patched.route.parent_node_index as f64);
        push_curve(&mut out, &patched.route.curve);
    }
    out
}

/// Identity tables for one revision. Fetched with the scene buffer, never per
/// frame: this is the only place strings cross the boundary.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneIndex {
    pub revision: u32,
    pub orientation: Orientation,
    pub nodes: Vec<String>,
    pub ports: Vec<SceneIndexPort>,
    pub edges: Vec<String>,
    pub tethers: Vec<SceneIndexTether>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneIndexPort {
    pub node: String,
    pub name: String,
    pub side: PortSide,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneIndexTether {
    pub child: String,
    pub parent: String,
}

pub fn scene_index(scene: &Scene, revision: u32) -> SceneIndex {
    SceneIndex {
        revision,
        orientation: scene.orientation,
        nodes: scene.nodes.iter().map(|node| node.id.clone()).collect(),
        ports: scene
            .ports
            .iter()
            .map(|port| SceneIndexPort {
                node: port.node_id.clone(),
                name: port.name.clone(),
                side: port.side,
            })
            .collect(),
        edges: scene.edges.iter().map(|edge| edge.id.clone()).collect(),
        tethers: scene
            .tethers
            .iter()
            .map(|tether| SceneIndexTether {
                child: scene.nodes[tether.child_node_index].id.clone(),
                parent: scene.nodes[tether.parent_node_index].id.clone(),
            })
            .collect(),
    }
}

/// Machine-readable description of everything above.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AbiDescriptor {
    pub version: f64,
    pub scene_header_len: usize,
    pub node_stride: usize,
    pub port_stride: usize,
    pub edge_head_len: usize,
    pub tether_head_len: usize,
    pub segment_stride: usize,
    pub patch_header_len: usize,
    pub patch_node_stride: usize,
    pub patch_edge_head_len: usize,
    pub patch_tether_head_len: usize,
    pub no_port: f64,
    pub curve_ops: CurveOpCodes,
    pub port_sides: PortSideCodes,
    pub orientations: OrientationCodes,
    pub hit_kinds: HitKindCodes,
    pub hit_index_bits: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurveOpCodes {
    pub line: u8,
    pub quadratic: u8,
    pub cubic: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortSideCodes {
    pub left: u8,
    pub right: u8,
    pub top: u8,
    pub bottom: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrientationCodes {
    pub horizontal: f64,
    pub vertical: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HitKindCodes {
    pub none: u32,
    pub node: u32,
    pub port: u32,
    pub edge: u32,
    pub tether: u32,
}

pub fn abi_descriptor() -> AbiDescriptor {
    AbiDescriptor {
        version: SCENE_BUFFER_VERSION,
        scene_header_len: SCENE_HEADER_LEN,
        node_stride: NODE_STRIDE,
        port_stride: PORT_STRIDE,
        edge_head_len: EDGE_HEAD_LEN,
        tether_head_len: TETHER_HEAD_LEN,
        segment_stride: SEGMENT_STRIDE,
        patch_header_len: PATCH_HEADER_LEN,
        patch_node_stride: PATCH_NODE_STRIDE,
        patch_edge_head_len: PATCH_EDGE_HEAD_LEN,
        patch_tether_head_len: PATCH_TETHER_HEAD_LEN,
        no_port: NO_PORT,
        curve_ops: CurveOpCodes {
            line: crate::curve::CURVE_OP_LINE,
            quadratic: crate::curve::CURVE_OP_QUADRATIC,
            cubic: crate::curve::CURVE_OP_CUBIC,
        },
        port_sides: PortSideCodes {
            left: PortSide::Left.code(),
            right: PortSide::Right.code(),
            top: PortSide::Top.code(),
            bottom: PortSide::Bottom.code(),
        },
        orientations: OrientationCodes {
            horizontal: orientation_code(Orientation::Horizontal),
            vertical: orientation_code(Orientation::Vertical),
        },
        hit_kinds: HitKindCodes {
            none: HIT_KIND_NONE,
            node: HIT_KIND_NODE,
            port: HIT_KIND_PORT,
            edge: HIT_KIND_EDGE,
            tether: HIT_KIND_TETHER,
        },
        hit_index_bits: HIT_INDEX_BITS,
    }
}

/// A decoded scene buffer. This exists so the reader the host must write is
/// executed by native tests: if a JS consumer can be written against the
/// documented offsets, this decoder proves it, and if it cannot, this decoder
/// fails first.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedScene {
    pub version: f64,
    pub revision: u32,
    pub orientation: Orientation,
    pub bounds: [f64; 4],
    pub nodes: Vec<[f64; 8]>,
    pub ports: Vec<DecodedPort>,
    pub edges: Vec<DecodedEdge>,
    pub tethers: Vec<DecodedTether>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedPort {
    pub node_index: usize,
    pub side: u8,
    pub point: Point,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedEdge {
    pub from_node_index: usize,
    pub to_node_index: usize,
    pub from_port_index: Option<usize>,
    pub to_port_index: Option<usize>,
    pub label: Point,
    pub start: Point,
    pub segments: Vec<DecodedSegment>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedTether {
    pub child_node_index: usize,
    pub parent_node_index: usize,
    pub start: Point,
    pub segments: Vec<DecodedSegment>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecodedSegment {
    pub op: u8,
    pub control_a: Point,
    pub control_b: Point,
    pub to: Point,
}

struct Cursor<'a> {
    data: &'a [f64],
    at: usize,
}

impl<'a> Cursor<'a> {
    /// The returned slice borrows the underlying data, not the cursor, so a
    /// decoded header stays readable while later records are being taken.
    fn take(&mut self, count: usize) -> Result<&'a [f64], GeometryError> {
        let end = self.at + count;
        if end > self.data.len() {
            return Err(GeometryError::InvalidBuffer {
                detail: format!("needed {count} values at offset {}", self.at),
            });
        }
        let slice = &self.data[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn count(&mut self, field: &str) -> Result<usize, GeometryError> {
        let value = self.take(1)?[0];
        if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
            return Err(GeometryError::InvalidBuffer {
                detail: format!("{field} is not a whole count: {value}"),
            });
        }
        Ok(value as usize)
    }

    fn segments(&mut self) -> Result<(Point, Vec<DecodedSegment>), GeometryError> {
        let head = self.take(2)?;
        let start = Point::new(head[0], head[1]);
        let count = self.count("segmentCount")?;
        let mut segments = Vec::with_capacity(count);
        for _ in 0..count {
            let raw = self.take(SEGMENT_STRIDE)?;
            segments.push(DecodedSegment {
                op: raw[0] as u8,
                control_a: Point::new(raw[1], raw[2]),
                control_b: Point::new(raw[3], raw[4]),
                to: Point::new(raw[5], raw[6]),
            });
        }
        Ok((start, segments))
    }
}

fn port_from_slot(value: f64) -> Option<usize> {
    if value < 0.0 {
        None
    } else {
        Some(value as usize)
    }
}

/// Read a buffer produced by [`encode_scene`].
pub fn decode_scene(data: &[f64]) -> Result<DecodedScene, GeometryError> {
    let mut cursor = Cursor { data, at: 0 };
    let header = cursor.take(SCENE_HEADER_LEN)?;
    if header[0] != SCENE_BUFFER_VERSION {
        return Err(GeometryError::InvalidBuffer {
            detail: format!("unsupported scene buffer version {}", header[0]),
        });
    }
    let orientation = match header[2] {
        0.0 => Orientation::Horizontal,
        1.0 => Orientation::Vertical,
        other => {
            return Err(GeometryError::InvalidBuffer {
                detail: format!("unknown orientation code {other}"),
            });
        }
    };
    let node_count = header[3] as usize;
    let port_count = header[4] as usize;
    let edge_count = header[5] as usize;
    let tether_count = header[6] as usize;
    let bounds = [header[7], header[8], header[9], header[10]];

    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        let raw = cursor.take(NODE_STRIDE)?;
        nodes.push([
            raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
        ]);
    }
    let mut ports = Vec::with_capacity(port_count);
    for _ in 0..port_count {
        let raw = cursor.take(PORT_STRIDE)?;
        ports.push(DecodedPort {
            node_index: raw[0] as usize,
            side: raw[1] as u8,
            point: Point::new(raw[2], raw[3]),
        });
    }
    let mut edges = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        let raw = cursor.take(EDGE_HEAD_LEN - 3)?;
        let (start, segments) = cursor.segments()?;
        edges.push(DecodedEdge {
            from_node_index: raw[0] as usize,
            to_node_index: raw[1] as usize,
            from_port_index: port_from_slot(raw[2]),
            to_port_index: port_from_slot(raw[3]),
            label: Point::new(raw[4], raw[5]),
            start,
            segments,
        });
    }
    let mut tethers = Vec::with_capacity(tether_count);
    for _ in 0..tether_count {
        let raw = cursor.take(TETHER_HEAD_LEN - 3)?;
        let (start, segments) = cursor.segments()?;
        tethers.push(DecodedTether {
            child_node_index: raw[0] as usize,
            parent_node_index: raw[1] as usize,
            start,
            segments,
        });
    }
    if cursor.at != data.len() {
        return Err(GeometryError::InvalidBuffer {
            detail: format!("{} trailing values", data.len() - cursor.at),
        });
    }
    Ok(DecodedScene {
        version: header[0],
        revision: header[1] as u32,
        orientation,
        bounds,
        nodes,
        ports,
        edges,
        tethers,
    })
}

/// A decoded drag buffer.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedDrag {
    pub version: f64,
    pub revision: u32,
    pub moved: bool,
    pub clamped: bool,
    pub node_index: usize,
    pub to_column: i32,
    pub to_row: i32,
    pub bounds: [f64; 4],
    pub nodes: Vec<(usize, [f64; 8])>,
    pub edges: Vec<(usize, DecodedEdge)>,
    pub tethers: Vec<(usize, DecodedTether)>,
}

/// Read a buffer produced by [`encode_drag`].
pub fn decode_drag(data: &[f64]) -> Result<DecodedDrag, GeometryError> {
    let mut cursor = Cursor { data, at: 0 };
    let header = cursor.take(PATCH_HEADER_LEN)?;
    if header[0] != SCENE_BUFFER_VERSION {
        return Err(GeometryError::InvalidBuffer {
            detail: format!("unsupported patch buffer version {}", header[0]),
        });
    }
    let node_count = header[7] as usize;
    let edge_count = header[8] as usize;
    let tether_count = header[9] as usize;

    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        let raw = cursor.take(PATCH_NODE_STRIDE)?;
        nodes.push((
            raw[0] as usize,
            [
                raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7], raw[8],
            ],
        ));
    }
    let mut edges = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        let raw = cursor.take(PATCH_EDGE_HEAD_LEN - 3)?;
        let (start, segments) = cursor.segments()?;
        edges.push((
            raw[0] as usize,
            DecodedEdge {
                from_node_index: raw[1] as usize,
                to_node_index: raw[2] as usize,
                from_port_index: port_from_slot(raw[3]),
                to_port_index: port_from_slot(raw[4]),
                label: Point::new(raw[5], raw[6]),
                start,
                segments,
            },
        ));
    }
    let mut tethers = Vec::with_capacity(tether_count);
    for _ in 0..tether_count {
        let raw = cursor.take(PATCH_TETHER_HEAD_LEN - 3)?;
        let (start, segments) = cursor.segments()?;
        tethers.push((
            raw[0] as usize,
            DecodedTether {
                child_node_index: raw[1] as usize,
                parent_node_index: raw[2] as usize,
                start,
                segments,
            },
        ));
    }
    if cursor.at != data.len() {
        return Err(GeometryError::InvalidBuffer {
            detail: format!("{} trailing values", data.len() - cursor.at),
        });
    }
    Ok(DecodedDrag {
        version: header[0],
        revision: header[1] as u32,
        moved: header[2] != 0.0,
        clamped: header[3] != 0.0,
        node_index: header[4] as usize,
        to_column: header[5] as i32,
        to_row: header[6] as i32,
        bounds: [header[10], header[11], header[12], header[13]],
        nodes,
        edges,
        tethers,
    })
}
