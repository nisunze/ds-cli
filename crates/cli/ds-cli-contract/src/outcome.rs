//! Canonical command envelopes belong to the shared Kernel contract leaf.
pub use ds_core_contracts::envelope::{
    error_envelope, success_envelope, ErrorBody, ErrorEnvelope, SuccessEnvelope, ENVELOPE_VERSION,
};
pub use ds_core_contracts::{ExitClass, Failure, Refusal};
