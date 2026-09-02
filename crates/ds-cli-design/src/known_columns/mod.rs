//! `ds design known-columns` — the project's external property authority.
//!
//! Internal model properties are preserved independently. This family only
//! edits the existing `know_columns` sheet used by reports, GIS exports and tiles.

pub mod list;
pub mod set;
