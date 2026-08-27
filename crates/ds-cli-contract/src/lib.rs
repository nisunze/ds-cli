//! The `ds` command-line contract.
//!
//! This crate holds everything that is true about the CLI *as a CLI* and
//! nothing about any engineering domain: how a command describes itself, how
//! help is tiered, how output is shaped, how a refusal is typed, and what the
//! process exits with. It links no domain crate on purpose — the contract has
//! to be testable without building an engine, and a domain must never be able
//! to reach into it and quietly widen the surface.

pub mod args;
pub mod help;
pub mod outcome;
pub mod output;
pub mod spec;

pub use args::{Inputs, parse};
pub use outcome::{ExitClass, Failure, SuccessEnvelope, success_envelope};
pub use output::{Format, Output};
pub use spec::{
    Arg, ArgKind, Authority, Availability, Chapter, Command, Domain, Effect, Example, Execution,
    Refusal,
};

/// The handler signature every command implements.
///
/// Handlers return JSON, not text. Human rendering is a projection applied
/// afterwards, so the machine contract cannot drift from what a person sees:
/// both are made from the same value.
pub type Handler = fn(&Inputs, &Context) -> Result<serde_json::Value, Failure>;

/// What a handler is allowed to know about its invocation. Deliberately
/// small: no argv, no environment access helper, no way to reach another
/// domain's engine.
pub struct Context {
    /// Whether the caller pre-confirmed an effectful command with `--yes`.
    /// Read only by commands whose effect requires confirmation; dispatch
    /// enforces the requirement so a handler cannot forget to.
    pub confirmed: bool,
    /// The output shape, for handlers that must bound a projection
    /// differently for a human than for a machine.
    pub output: Output,
}
