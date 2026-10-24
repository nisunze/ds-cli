//! The collision margin a layout document admits: furniture keeps inside the
//! paper less this margin (0 mm allowed, 1 mm the preferred least).
//!
//! The sheet assessment itself — furniture overlaps, design-area cover and
//! clipping, each a typed warning with its smallest clearing move — is the
//! Reporter's (`print_furniture`); only the document's bound lives here.

/// The preferred least collision margin: furniture inside 1 mm of the paper
/// edge is clipped for a printer, not merely close.
pub const DEFAULT_MARGIN_MM: f64 = 1.;
pub const MAX_MARGIN_MM: f64 = 20.;
