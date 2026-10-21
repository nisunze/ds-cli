//! The native client profiles this build was compiled with.
//!
//! Every release channel the Kernel declares (`releaseTargets`) has a profile
//! when the build environment named its gateway, keys and Firebase audience
//! (`ds_command_kernel::gateway_target`). Nothing is read from disk: every
//! Core build carries its catalog, and a lane it was not given refuses with
//! the variable it lacked.

use ds_command_kernel::gateway_target;
use sha2::{Digest, Sha256};

use crate::profile::{ClientProfile, DeploymentLane, ProfileError, ProfileParts};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledProfileError {
    /// The build environment did not name this variable for the lane.
    Unconfigured(&'static str),
    /// A named value is malformed.
    Invalid(ProfileError),
}

/// `lane`'s profile, as compiled.
pub fn compiled_profile(lane: DeploymentLane) -> Result<ClientProfile, CompiledProfileError> {
    let gateway = gateway_target::release_gateway(lane.token())
        .map_err(CompiledProfileError::Unconfigured)?;
    ClientProfile::from_parts(ProfileParts {
        lane,
        source_revision: gateway_target::SOURCE_REVISION,
        descriptor_sha256: gateway_target::ROUTING_SHA256,
        firebase_project_id: gateway.firebase_project,
        firebase_api_key: gateway.firebase_api_key,
        gateway_api_key: gateway.api_key,
        gateway_origin: gateway.origin,
    })
    .map_err(CompiledProfileError::Invalid)
}

/// The digest of the whole compiled catalog, when the build serves every
/// lane: what a packaged Core reports as its identity.
pub fn compiled_catalog_sha256() -> Option<String> {
    let mut hasher = Sha256::new();
    for lane in [DeploymentLane::Stable, DeploymentLane::Canary] {
        let profile = compiled_profile(lane).ok()?;
        for value in [lane.token(), profile.profile_sha256()] {
            hasher.update((value.len() as u64).to_be_bytes());
            hasher.update(value.as_bytes());
        }
    }
    Some(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
#[path = "compiled_profile_tests.rs"]
mod tests;
