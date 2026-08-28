//! `ds design tag` — the project's typed vocabulary, and its values on an object.
//!
//! ```text
//!   list → define | set
//! ```
//!
//! A definition is the vocabulary — allowed values, cardinality, lifecycle. A
//! value is one definition applied to one object, optionally anchored to one
//! exact object version so historical context survives a later edit. ds-brain
//! validates every value against the definition's own vocabulary; this domain
//! offers what the definition declares and never invents an allowed value.

pub mod define;
pub mod list;
pub mod set;
