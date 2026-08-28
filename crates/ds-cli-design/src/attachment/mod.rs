//! `ds design attachment` — immutable, versioned files on a design object.
//!
//! ```text
//!   list → publish | download | retire
//! ```
//!
//! An attachment is OPAQUE: a PLS-CADD `.bak`, a native workspace, a report, a
//! photo, a client deliverable. Nothing here parses it. Publishing a revision
//! never mutates earlier bytes — each revision owns its own object, its own
//! server-verified digest and its own storage generation — and a download comes
//! back signed by ds-brain and pinned to that generation, so `ds` never composes
//! a storage URL.

pub mod download;
pub mod list;
pub mod publish;
pub mod retire;
