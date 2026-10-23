//! Typed replies of the browser `SessionHandle` JSON methods.
//!
//! The handle answers in JSON text. A host admits each reply through the
//! decoder of its family before reading it, so the reply shapes have one
//! owner: the types below, which also generate the browser declarations.
//! A reply that is not a JSON object of the owning type, or that lacks a
//! field the Kernel always writes, refuses as `design_session_response_invalid`.
//! Fields a newer Kernel adds are ignored, so an additive reply stays
//! compatible. Decoding never repairs or defaults a written field.

#[cfg(feature = "enable_dataframes")]
use crate::dataframe::{BulkCellEditResult, CellEditDiff, LayerRow};
use crate::mutation::SessionDiff;
use crate::projection::ProjectionMetadata;
use crate::session::{HoverResult, ScopeStatus, SelectionState, SnapResult};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

/// The reply families of the `SessionHandle` JSON methods.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplyFamily {
    /// `SessionDiff`: every mutation, undo and redo.
    Diff,
    /// `ScopeStatus`: `scopeStatusJson`.
    ScopeStatus,
    /// `ProjectionMetadata`: `projectionMetadata`.
    Projection,
    /// `SnapResult`: `snapJson`.
    Snap,
    /// `HoverResult`: `hoverJson`.
    Hover,
    /// `SelectionState`: `selection`, `select`, `selectByPolygonJson`,
    /// `hover`, `markEdited`, `clearSelection`.
    Selection,
    /// `LayerRow` or `null`: `getRowJson`.
    Row,
    /// `CellEditDiff` or `null`: `editCellJson`.
    CellEdit,
    /// `BulkCellEditResult`: `editCellsJson`.
    CellEdits,
}

impl ReplyFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Diff => "diff",
            Self::ScopeStatus => "scope_status",
            Self::Projection => "projection",
            Self::Snap => "snap",
            Self::Hover => "hover",
            Self::Selection => "selection",
            Self::Row => "row",
            Self::CellEdit => "cell_edit",
            Self::CellEdits => "cell_edits",
        }
    }
}

/// A reply the owning type does not admit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplyError {
    pub family: ReplyFamily,
    pub message: String,
}

impl ReplyError {
    pub fn new(family: ReplyFamily, message: impl Into<String>) -> Self {
        Self {
            family,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ReplyError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            out,
            "design_session_response_invalid: {} reply {}",
            self.family.as_str(),
            self.message
        )
    }
}

impl std::error::Error for ReplyError {}

/// serde also reads a struct from a positional JSON array; a reply is an
/// object (or, where the family allows absence, `null`).
fn decode<T: DeserializeOwned>(
    family: ReplyFamily,
    reply: &str,
    absent_allowed: bool,
) -> Result<T, ReplyError> {
    let start = reply.trim_start();
    if !(start.starts_with('{') || absent_allowed && start.starts_with("null")) {
        let expected = if absent_allowed {
            "is neither a JSON object nor null"
        } else {
            "is not a JSON object"
        };
        return Err(ReplyError::new(family, expected));
    }
    serde_json::from_str(reply).map_err(|error| ReplyError::new(family, error.to_string()))
}

pub fn decode_diff(reply: &str) -> Result<SessionDiff, ReplyError> {
    decode(ReplyFamily::Diff, reply, false)
}

pub fn decode_scope_status(reply: &str) -> Result<ScopeStatus, ReplyError> {
    decode(ReplyFamily::ScopeStatus, reply, false)
}

pub fn decode_projection(reply: &str) -> Result<ProjectionMetadata, ReplyError> {
    decode(ReplyFamily::Projection, reply, false)
}

pub fn decode_snap(reply: &str) -> Result<SnapResult, ReplyError> {
    decode(ReplyFamily::Snap, reply, false)
}

pub fn decode_hover(reply: &str) -> Result<HoverResult, ReplyError> {
    decode(ReplyFamily::Hover, reply, false)
}

pub fn decode_selection(reply: &str) -> Result<SelectionState, ReplyError> {
    decode(ReplyFamily::Selection, reply, false)
}

/// `null` is the Kernel's answer for an absent layer or feature.
#[cfg(feature = "enable_dataframes")]
pub fn decode_row(reply: &str) -> Result<Option<LayerRow>, ReplyError> {
    decode(ReplyFamily::Row, reply, true)
}

/// `null` is the Kernel's answer for an absent feature.
#[cfg(feature = "enable_dataframes")]
pub fn decode_cell_edit(reply: &str) -> Result<Option<CellEditDiff>, ReplyError> {
    decode(ReplyFamily::CellEdit, reply, true)
}

#[cfg(feature = "enable_dataframes")]
pub fn decode_cell_edits(reply: &str) -> Result<BulkCellEditResult, ReplyError> {
    decode(ReplyFamily::CellEdits, reply, false)
}

/// A field the Kernel always writes, possibly as `null`. serde would read a
/// missing `Option` field as `None`; with this reader a missing field refuses.
pub(crate) fn written_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[cfg(test)]
#[path = "reply_tests.rs"]
mod tests;
