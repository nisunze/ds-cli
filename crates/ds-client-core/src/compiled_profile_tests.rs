use super::*;
use crate::gateway::DESCRIPTOR_SHA256;
use ds_command_kernel::gateway_target::{release_gateway, ROUTING_SHA256};

#[test]
fn a_lane_is_served_exactly_when_its_gateway_was_compiled() {
    for lane in [DeploymentLane::Stable, DeploymentLane::Canary] {
        match (release_gateway(lane.token()), compiled_profile(lane)) {
            (Ok(gateway), Ok(profile)) => {
                assert_eq!(profile.gateway_origin(), gateway.origin);
                assert_eq!(profile.firebase_project_id(), gateway.firebase_project);
                assert_eq!(profile.descriptor_sha256(), ROUTING_SHA256);
                assert_eq!(profile.lane(), lane);
            }
            (Ok(_), Err(error)) => assert!(matches!(error, CompiledProfileError::Invalid(_))),
            (Err(variable), Err(error)) => {
                assert_eq!(error, CompiledProfileError::Unconfigured(variable));
            }
            (Err(_), Ok(_)) => panic!("a lane without a compiled gateway was served"),
        }
    }
}

#[test]
fn the_catalog_digest_names_both_lanes_or_nothing() {
    let both = [DeploymentLane::Stable, DeploymentLane::Canary]
        .into_iter()
        .all(|lane| compiled_profile(lane).is_ok());
    assert_eq!(compiled_catalog_sha256().is_some(), both);
}

#[test]
fn parts_build_the_profile_the_full_input_does() {
    let parts = ProfileParts {
        lane: DeploymentLane::Canary,
        source_revision: "unpinned",
        descriptor_sha256: DESCRIPTOR_SHA256,
        firebase_project_id: "fixture-project",
        firebase_api_key: "fixture-firebase-public-key",
        gateway_api_key: "fixture-gateway-public-key",
        gateway_origin: "https://fixture-canary.ue.gateway.dev/",
    };
    let profile = ClientProfile::from_parts(parts).unwrap();
    assert_eq!(profile.gateway_origin(), "https://fixture-canary.ue.gateway.dev");
    let stable = ClientProfile::from_parts(ProfileParts {
        lane: DeploymentLane::Stable,
        source_revision: "unpinned",
        descriptor_sha256: DESCRIPTOR_SHA256,
        firebase_project_id: "fixture-project",
        firebase_api_key: "fixture-firebase-public-key",
        gateway_api_key: "fixture-gateway-public-key",
        gateway_origin: "https://fixture-canary.ue.gateway.dev",
    });
    assert_eq!(stable.unwrap_err(), ProfileError::LaneMismatch);
}

#[test]
fn the_profile_names_the_routing_descriptor_the_registry_was_built_from() {
    assert_eq!(ROUTING_SHA256, DESCRIPTOR_SHA256);
}
