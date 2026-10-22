//! Command envelopes and application refusal builders have one shared owner.
pub use ds_core_contracts::envelope::ENVELOPE_VERSION as VERSION;
pub use ds_core_contracts::envelope::{
    error, success, Class, CommandEnvelope, CommandRefusal, Refusal,
};

#[cfg(test)]
#[path = "envelope_tests.rs"]
mod tests;
