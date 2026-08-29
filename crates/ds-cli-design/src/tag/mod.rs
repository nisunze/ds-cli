//! `ds design tag` — the project's typed vocabulary, and its values on an object.
//!
//! ```text
//!   list → define | set
//! ```
//!
//! A definition is a closed value type, compatible input control, constraints,
//! cardinality and lifecycle. Choice definitions additionally own an allowed
//! vocabulary; free-form definitions do not pretend observed values are one. A
//! value is one definition applied to one object, optionally anchored to one
//! exact object version so historical context survives a later edit. ds-brain
//! validates every value against that definition and this domain never infers
//! or coerces a type.

pub mod define;
pub mod list;
pub mod query;
pub mod set;
