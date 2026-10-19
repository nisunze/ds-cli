//! `--remote-context` asked ds-brain to compute Google buildings and elevation
//! contours for missing seed coverage. That was hosted compute, and the door is
//! retired (development philosophy §2): the switch stays declared only so a
//! caller who passes it gets a typed refusal naming the edge inputs, before any
//! credential, manifest or provider is touched.
use super::*;

pub(super) fn validate_source_mode(inputs: &Inputs) -> Result<(), Failure> {
    if inputs.switch("remote-context") {
        return Err(Failure::unavailable(
            REMOTE_CONTEXT_RETIRED.code,
            "buildings and contours are no longer computed in the cloud; nothing was sent",
        )
        .remedy(REMOTE_CONTEXT_RETIRED.remedy));
    }
    Ok(())
}

#[cfg(test)]
#[path = "remote_context_tests.rs"]
mod tests;
