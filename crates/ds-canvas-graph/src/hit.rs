//! Hit testing.
//!
//! One index answers "what is under the pointer", so a click and a hover can
//! never disagree, and the answer is packed into a single `u32` because the
//! pointer-move path must not allocate a JSON object per event. The host
//! resolves that code to an identity only when the hovered target actually
//! changes.
//!
//! Priority is ports, then nodes, then edges, then tethers: a port is a small
//! deliberate target drawn on top of its node, and a route passing beneath a
//! node is not what the operator is pointing at.

use serde::Serialize;

use crate::curve::{Point, polyline_distance};
use crate::error::GeometryError;
use crate::scene::Scene;

/// Wire codes for the packed hit result.
pub const HIT_KIND_NONE: u32 = 0;
pub const HIT_KIND_NODE: u32 = 1;
pub const HIT_KIND_PORT: u32 = 2;
pub const HIT_KIND_EDGE: u32 = 3;
pub const HIT_KIND_TETHER: u32 = 4;

/// Bits reserved for the index inside a packed hit code.
pub const HIT_INDEX_BITS: u32 = 28;
/// Largest index a packed hit code can carry.
pub const HIT_MAX_INDEX: u32 = (1 << HIT_INDEX_BITS) - 1;

/// What the pointer is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum HitTarget {
    None,
    Node { index: usize },
    Port { index: usize },
    Edge { index: usize },
    Tether { index: usize },
}

impl HitTarget {
    /// Pack into one `u32`: kind in the high bits, dense index in the low 28.
    pub fn code(self) -> u32 {
        let (kind, index) = match self {
            Self::None => (HIT_KIND_NONE, 0usize),
            Self::Node { index } => (HIT_KIND_NODE, index),
            Self::Port { index } => (HIT_KIND_PORT, index),
            Self::Edge { index } => (HIT_KIND_EDGE, index),
            Self::Tether { index } => (HIT_KIND_TETHER, index),
        };
        // Input bounds keep every scene far below 2^28 entries.
        (kind << HIT_INDEX_BITS) | (index as u32 & HIT_MAX_INDEX)
    }

    /// Unpack a code, refusing anything that does not address this scene.
    pub fn from_code(code: u32, scene: &Scene) -> Result<Self, GeometryError> {
        let kind = code >> HIT_INDEX_BITS;
        let index = (code & HIT_MAX_INDEX) as usize;
        let invalid = || GeometryError::InvalidHitCode { hit_code: code };
        let bounded = |len: usize| {
            if index < len {
                Ok(index)
            } else {
                Err(invalid())
            }
        };
        match kind {
            HIT_KIND_NONE if index == 0 => Ok(Self::None),
            HIT_KIND_NODE => Ok(Self::Node {
                index: bounded(scene.nodes.len())?,
            }),
            HIT_KIND_PORT => Ok(Self::Port {
                index: bounded(scene.ports.len())?,
            }),
            HIT_KIND_EDGE => Ok(Self::Edge {
                index: bounded(scene.edges.len())?,
            }),
            HIT_KIND_TETHER => Ok(Self::Tether {
                index: bounded(scene.tethers.len())?,
            }),
            _ => Err(invalid()),
        }
    }
}

/// World-space tolerances for one hit query. The session derives these from
/// screen pixels and the current scale, so a target stays the same physical
/// size on screen at every zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitTolerance {
    pub port_radius: f64,
    pub edge_tolerance: f64,
}

/// Resolve the topmost target at a world point.
///
/// Later entries win within a kind: the last node authored is the one drawn on
/// top, so it is the one the operator sees and therefore the one they mean.
pub fn hit_test(scene: &Scene, point: Point, tolerance: HitTolerance) -> HitTarget {
    if !point.is_finite() {
        return HitTarget::None;
    }

    let mut best_port: Option<(usize, f64)> = None;
    for (index, port) in scene.ports.iter().enumerate() {
        let distance = port.point.distance_to(point);
        if distance <= tolerance.port_radius && best_port.is_none_or(|(_, best)| distance <= best) {
            best_port = Some((index, distance));
        }
    }
    if let Some((index, _)) = best_port {
        return HitTarget::Port { index };
    }

    if let Some(index) = scene
        .nodes
        .iter()
        .rposition(|node| node.rect.contains(point))
    {
        return HitTarget::Node { index };
    }

    let mut best_edge: Option<(usize, f64)> = None;
    for (index, edge) in scene.edges.iter().enumerate() {
        let distance = polyline_distance(&edge.polyline, point);
        if distance <= tolerance.edge_tolerance
            && best_edge.is_none_or(|(_, best)| distance <= best)
        {
            best_edge = Some((index, distance));
        }
    }
    if let Some((index, _)) = best_edge {
        return HitTarget::Edge { index };
    }

    let mut best_tether: Option<(usize, f64)> = None;
    for (index, tether) in scene.tethers.iter().enumerate() {
        let distance = polyline_distance(&tether.polyline, point);
        if distance <= tolerance.edge_tolerance
            && best_tether.is_none_or(|(_, best)| distance <= best)
        {
            best_tether = Some((index, distance));
        }
    }
    match best_tether {
        Some((index, _)) => HitTarget::Tether { index },
        None => HitTarget::None,
    }
}
