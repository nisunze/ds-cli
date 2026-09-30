//! Exact global member reads through the existing protected native catalog lane.

use ds_cli_contract::{Failure, spec::Refusal};
use ds_client_core::{ClientError, ErrorKind, grid_catalog::Command};
use serde_json::Value;

pub const REFUSALS: [Refusal; 2] = [
    Refusal {
        code: "catalog_member_pin_invalid",
        when: "a pin violates the native contract, or the server refuses an invalid or ambiguous inventory",
        remedy: "use the exact library/release id, case-sensitive inventory path and lowercase SHA-256 from library global read",
    },
    Refusal {
        code: "catalog_member_not_found",
        when: "the exact pin is absent, hidden, not ready, stale or belongs to an unindexed legacy release",
        remedy: "inspect the authorized exact release inventory with library global read; never substitute its head",
    },
];

/// No project or saved selection: the catalog gateway authorizes the restored
/// user/device in this explicit lane. Core validates the response's four pins.
pub fn resolve(
    lane: &str,
    library_id: &str,
    release_id: &str,
    relative_path: &str,
    expected_digest: &str,
) -> Result<Value, Failure> {
    let command = Command::ResolveLibraryMember {
        library_id: library_id.into(),
        release_id: release_id.into(),
        relative_path: relative_path.into(),
        expected_digest: expected_digest.into(),
    };
    // Invalid requests fail before restoring credentials or reaching a socket.
    command.validate().map_err(map_error)?;
    super::run_grid_catalog(lane, &command, map_error)
}

fn map_error(error: ClientError) -> Failure {
    match error.kind() {
        ErrorKind::InvalidInput => Failure::invalid(
            "catalog_member_pin_invalid",
            "the native catalog refused the exact member pin or stored inventory",
        )
        .remedy(REFUSALS[0].remedy),
        ErrorKind::ResourceNotFound => Failure::invalid(
            "catalog_member_not_found",
            "the authorized catalog has no readable indexed member for this exact pin",
        )
        .remedy(REFUSALS[1].remedy),
        _ => super::map_client(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FixtureTransport, NOW, SIGN_IN, signed_in};
    use ds_client_core::TransportResponse;
    use serde_json::json;

    fn command() -> Command {
        Command::ResolveLibraryMember {
            library_id: "library_1".into(),
            release_id: "release_1".into(),
            relative_path: "pls-cadd/criteria/Base.CRI".into(),
            expected_digest: "a".repeat(64),
        }
    }

    fn data() -> Value {
        json!({
            "library_id": "library_1", "release_id": "release_1",
            "member": {
                "relative_path": "pls-cadd/criteria/Base.CRI", "class": "criteria",
                "artifact": {"digest": "a".repeat(64), "byte_length": 17,
                    "object": format!("ds-grid/objects/sha256/{}", "a".repeat(64))},
                "provenance": {"kind": "source_project", "reference": "approved-source"}
            },
            "download_url": "https://storage.googleapis.com/bucket/object?generation=7&signature=opaque",
            "expires_at": "2030-03-17T17:46:40Z"
        })
    }

    fn response(status: u16, data: Value) -> TransportResponse {
        TransportResponse::new(
            status,
            serde_json::to_vec(&json!({"success": true, "data": data})).unwrap(),
        )
    }

    #[test]
    fn exact_member_uses_existing_authenticated_gateway_without_project_context() {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport
            .lock()
            .grid_catalog
            .push_back(response(200, data()));
        let mut client = signed_in(transport.clone());
        assert_eq!(client.grid_catalog(&command(), NOW).unwrap(), data());
        let script = transport.lock();
        assert_eq!(
            script.grid_catalog_bodies,
            vec![json!({
                "action": "resolve_library_member", "library_id": "library_1",
                "release_id": "release_1", "relative_path": "pls-cadd/criteria/Base.CRI",
                "expected_digest": "a".repeat(64)
            })]
        );
        assert!(
            script
                .calls
                .iter()
                .any(|call| call.starts_with("grid_catalog ") && call.ends_with(" user"))
        );
    }

    #[test]
    fn exact_member_uses_the_linked_device_context_without_a_project() {
        let transport = FixtureTransport::default();
        transport
            .lock()
            .grid_catalog
            .push_back(response(200, data()));
        let mut device = crate::test_support::linked_device(transport.clone(), crate::now());
        assert_eq!(device.grid_catalog(&command()).unwrap(), data());
        assert_eq!(
            transport.calls(),
            vec![format!(
                "grid_catalog {} device-1",
                crate::test_support::DEVICE_ACCESS_TOKEN
            )]
        );
        assert!(
            transport.lock().grid_catalog_bodies[0]
                .get("project_id")
                .is_none()
        );
    }

    #[test]
    fn stale_unauthorized_and_invalid_pins_keep_actionable_refusals() {
        for (status, code) in [
            (404, "catalog_member_not_found"),
            (403, "auth_rejected"),
            (401, "auth_rejected"),
            (400, "catalog_member_pin_invalid"),
            (409, "catalog_member_pin_invalid"),
        ] {
            let transport = FixtureTransport::with_sign_in(SIGN_IN);
            transport
                .lock()
                .grid_catalog
                .push_back(response(status, Value::Null));
            let mut client = signed_in(transport);
            assert_eq!(
                map_error(client.grid_catalog(&command(), NOW).unwrap_err()).code(),
                code
            );
        }
    }

    #[test]
    fn different_release_path_digest_and_size_cannot_be_accepted_as_this_member() {
        for (pointer, replacement) in [
            ("/release_id", json!("release_2")),
            ("/member/relative_path", json!("pls-cadd/criteria/base.cri")),
            ("/member/artifact/digest", json!("b".repeat(64))),
            ("/member/artifact/byte_length", json!(0)),
        ] {
            let mut wrong = data();
            *wrong.pointer_mut(pointer).unwrap() = replacement;
            let transport = FixtureTransport::with_sign_in(SIGN_IN);
            transport
                .lock()
                .grid_catalog
                .push_back(response(200, wrong));
            let mut client = signed_in(transport);
            assert_eq!(
                map_error(client.grid_catalog(&command(), NOW).unwrap_err()).code(),
                "auth_response_unreadable"
            );
        }
    }

    #[test]
    fn malformed_digest_or_ambiguous_missing_release_refuses_before_authentication() {
        for (release, digest) in [("", "a".repeat(64)), ("release_1", "A".repeat(64))] {
            assert_eq!(
                resolve("stable", "library_1", release, "pls-cadd/Base.CRI", &digest)
                    .unwrap_err()
                    .code(),
                "catalog_member_pin_invalid"
            );
        }
    }
}
