//! Where a measured schedule or legend docks on its sheet, as the layout
//! document authors it. The placement itself is the Reporter's
//! (`print_frames`).
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SheetAnchor {
    /// Use table/legend alignment and the authored frame’s upper/lower page half.
    #[default]
    Auto,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    /// Explicit free composition, retaining authored coordinates.
    Authored,
}

pub fn is_auto(anchor: &SheetAnchor) -> bool {
    *anchor == SheetAnchor::Auto
}
