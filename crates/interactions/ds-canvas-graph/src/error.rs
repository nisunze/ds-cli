//! Typed refusals.
//!
//! Every failure carries a stable machine `code()` plus exact operands. The
//! host never receives a bare string it has to pattern-match on, and it never
//! receives a silently corrected value: a non-finite coordinate is refused, not
//! clamped to zero. Clamping a bad number is how a canvas ends up drawing a
//! plausible picture of an impossible scene.
//!
//! `value` fields are pre-formatted strings on purpose — `NaN` and `Infinity`
//! have no JSON number representation, so an error that reported them as `f64`
//! could not survive the interchange it exists to describe.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Every way canvas geometry refuses an input.
#[derive(Clone, Debug, PartialEq, Eq, Error, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "code")]
pub enum GeometryError {
    #[error("{field} must be finite; got {value}")]
    #[serde(rename = "canvas_geometry_non_finite_input")]
    NonFiniteInput { field: String, value: String },

    #[error("{field} is out of range: {value} ({expected})")]
    #[serde(rename = "canvas_geometry_invalid_metric")]
    InvalidMetric {
        field: String,
        value: String,
        expected: String,
    },

    #[error("{field} must not be empty")]
    #[serde(rename = "canvas_geometry_empty_identifier")]
    EmptyIdentifier { field: String },

    #[error("duplicate node id {id:?}")]
    #[serde(rename = "canvas_geometry_duplicate_node")]
    DuplicateNode { id: String },

    #[error("duplicate edge id {id:?}")]
    #[serde(rename = "canvas_geometry_duplicate_edge")]
    DuplicateEdge { id: String },

    #[error("duplicate port {port:?} on node {node:?}")]
    #[serde(rename = "canvas_geometry_duplicate_port")]
    DuplicatePort { node: String, port: String },

    #[error("{reference} references unknown node {node:?}")]
    #[serde(rename = "canvas_geometry_unknown_node")]
    UnknownNode { reference: String, node: String },

    #[error("{reference} references unknown port {port:?} on node {node:?}")]
    #[serde(rename = "canvas_geometry_unknown_port")]
    UnknownPort {
        reference: String,
        node: String,
        port: String,
    },

    #[error("node {node:?} cell ({column}, {row}) is outside the authored grid")]
    #[serde(rename = "canvas_geometry_cell_out_of_range")]
    CellOutOfRange { node: String, column: i32, row: i32 },

    #[error("node {node:?} cannot contain itself")]
    #[serde(rename = "canvas_geometry_self_containment")]
    SelfContainment { node: String },

    #[error("containment cycle through node {node:?}")]
    #[serde(rename = "canvas_geometry_containment_cycle")]
    ContainmentCycle { node: String },

    #[error("{field} holds {count} entries; the bound is {max}")]
    #[serde(rename = "canvas_geometry_input_too_large")]
    InputTooLarge {
        field: String,
        count: usize,
        max: usize,
    },

    #[error("drag delta ({dx}, {dy}) exceeds the bound {max}")]
    #[serde(rename = "canvas_geometry_drag_too_large")]
    DragTooLarge { dx: String, dy: String, max: String },

    #[error("invalid canvas geometry JSON: {detail}")]
    #[serde(rename = "canvas_geometry_invalid_json")]
    InvalidJson { detail: String },

    #[error("hit code {hit_code} does not describe a target in this scene")]
    #[serde(rename = "canvas_geometry_invalid_hit_code")]
    InvalidHitCode { hit_code: u32 },

    #[error("scene buffer is not decodable: {detail}")]
    #[serde(rename = "canvas_geometry_invalid_buffer")]
    InvalidBuffer { detail: String },
}

impl GeometryError {
    /// Stable machine code. Hosts branch on this, never on the message.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NonFiniteInput { .. } => "canvas_geometry_non_finite_input",
            Self::InvalidMetric { .. } => "canvas_geometry_invalid_metric",
            Self::EmptyIdentifier { .. } => "canvas_geometry_empty_identifier",
            Self::DuplicateNode { .. } => "canvas_geometry_duplicate_node",
            Self::DuplicateEdge { .. } => "canvas_geometry_duplicate_edge",
            Self::DuplicatePort { .. } => "canvas_geometry_duplicate_port",
            Self::UnknownNode { .. } => "canvas_geometry_unknown_node",
            Self::UnknownPort { .. } => "canvas_geometry_unknown_port",
            Self::CellOutOfRange { .. } => "canvas_geometry_cell_out_of_range",
            Self::SelfContainment { .. } => "canvas_geometry_self_containment",
            Self::ContainmentCycle { .. } => "canvas_geometry_containment_cycle",
            Self::InputTooLarge { .. } => "canvas_geometry_input_too_large",
            Self::DragTooLarge { .. } => "canvas_geometry_drag_too_large",
            Self::InvalidJson { .. } => "canvas_geometry_invalid_json",
            Self::InvalidHitCode { .. } => "canvas_geometry_invalid_hit_code",
            Self::InvalidBuffer { .. } => "canvas_geometry_invalid_buffer",
        }
    }

    pub(crate) fn non_finite(field: impl Into<String>, value: f64) -> Self {
        Self::NonFiniteInput {
            field: field.into(),
            value: format!("{value}"),
        }
    }
}

/// Guard one host-supplied scalar. Returns the value so call sites read as
/// `let w = finite("hostWidth", width)?;`.
pub(crate) fn finite(field: &str, value: f64) -> Result<f64, GeometryError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(GeometryError::non_finite(field, value))
    }
}

/// Guard a scalar that must additionally be strictly positive.
pub(crate) fn finite_positive(field: &str, value: f64) -> Result<f64, GeometryError> {
    let value = finite(field, value)?;
    if value > 0.0 {
        Ok(value)
    } else {
        Err(GeometryError::InvalidMetric {
            field: field.to_string(),
            value: format!("{value}"),
            expected: "> 0".to_string(),
        })
    }
}

/// Guard a scalar that must be finite and non-negative.
pub(crate) fn finite_non_negative(field: &str, value: f64) -> Result<f64, GeometryError> {
    let value = finite(field, value)?;
    if value >= 0.0 {
        Ok(value)
    } else {
        Err(GeometryError::InvalidMetric {
            field: field.to_string(),
            value: format!("{value}"),
            expected: ">= 0".to_string(),
        })
    }
}
