//! The one gateway route the sync loop may address — the shared record's
//! door — and no other. Publication asks the session's own authority there;
//! there is no grant route to reach.
//!
//! `domains.compute_artifacts.*` is transcribed from the reviewed descriptor
//! (`ds-web/routing/operations.json`, as `ds-client-core/src/gateway/registry.rs`
//! carries it; a test in this crate holds the two equal).
//!
//! A gateway adapter is only ever handed a [`SyncRoute`]. It cannot be asked
//! for a path, a method or a URL, which is what keeps the seam a typed door
//! rather than an HTTP tunnel.

/// One gateway route, exactly as the reviewed descriptor declares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub path: &'static str,
    pub method: &'static str,
    pub timeout_s: u64,
    pub response_cap_bytes: u64,
}

/// `POST /api/v1/compute_artifacts` — actions `list`, `read`, `download`,
/// and a report publication's `open` and `finalize`.
pub const COMPUTE_ARTIFACTS: Route = Route {
    path: "/api/v1/compute_artifacts",
    method: "POST",
    timeout_s: 30,
    response_cap_bytes: 4 * 1024 * 1024,
};

/// The closed set of routes the loop addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncRoute {
    /// The shared record: list heads, mint a download ticket, open and
    /// finalize a publication.
    ComputeArtifacts,
}

impl SyncRoute {
    /// The descriptor row this route stands for.
    pub const fn declared(self) -> Route {
        match self {
            SyncRoute::ComputeArtifacts => COMPUTE_ARTIFACTS,
        }
    }

    /// The `action` values the multiplexed record route may carry.
    pub const fn actions(self) -> &'static [&'static str] {
        match self {
            SyncRoute::ComputeArtifacts => &["list", "read", "download", "open", "finalize"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(id: &str) -> &'static ds_client_core::GatewayOperation {
        ds_client_core::gateway_operation(id).unwrap_or_else(|| panic!("registry has no {id}"))
    }

    /// The transcription and the reviewed registry must not disagree.
    #[test]
    fn the_record_route_is_the_registrys_row() {
        let list = registry("domains.compute_artifacts.list");
        assert_eq!(COMPUTE_ARTIFACTS.path, list.path);
        assert_eq!(COMPUTE_ARTIFACTS.method, list.method);
        assert_eq!(COMPUTE_ARTIFACTS.timeout_s, u64::from(list.timeout_s));
        assert_eq!(
            COMPUTE_ARTIFACTS.response_cap_bytes,
            list.response_cap_bytes
        );
        assert!(list.carries_user_bearer());
        for action in SyncRoute::ComputeArtifacts.actions() {
            let entry = registry(&format!("domains.compute_artifacts.{action}"));
            assert_eq!(entry.path, COMPUTE_ARTIFACTS.path);
            assert!(entry.allows_action(action), "{action} is declared");
            assert!(
                entry.response_cap_bytes <= COMPUTE_ARTIFACTS.response_cap_bytes,
                "{action} answers within the ceiling this loop enforces"
            );
        }
    }
}
