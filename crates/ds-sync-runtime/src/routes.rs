//! The three gateway routes the sync loop may address — and no other.
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

/// `POST /api/v1/compute_artifacts` — actions `list`, `read`, `download`.
pub const COMPUTE_ARTIFACTS: Route = Route {
    path: "/api/v1/compute_artifacts",
    method: "POST",
    timeout_s: 30,
    response_cap_bytes: 4 * 1024 * 1024,
};

/// `POST /api/v1/work/open` — §1.2. The response carries a snapshot manifest
/// ds-brain itself caps at 24 MiB.
pub const WORK_OPEN: Route = Route {
    path: "/api/v1/work/open",
    method: "POST",
    timeout_s: 120,
    response_cap_bytes: 24 * 1024 * 1024,
};

/// `POST /api/v1/work/publish` — §1.3.
pub const WORK_PUBLISH: Route = Route {
    path: "/api/v1/work/publish",
    method: "POST",
    timeout_s: 120,
    response_cap_bytes: 1024 * 1024,
};

/// The closed set of routes the loop addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncRoute {
    /// The shared record: list heads, mint a download ticket.
    ComputeArtifacts,
    /// Mint a work grant and pin a snapshot.
    WorkOpen,
    /// Declare a publication; restate it after the uploads.
    WorkPublish,
}

impl SyncRoute {
    /// The descriptor row this route stands for.
    pub const fn declared(self) -> Route {
        match self {
            SyncRoute::ComputeArtifacts => COMPUTE_ARTIFACTS,
            SyncRoute::WorkOpen => WORK_OPEN,
            SyncRoute::WorkPublish => WORK_PUBLISH,
        }
    }

    /// The `action` values the multiplexed record route may carry; grant
    /// routes multiplex nothing.
    pub const fn actions(self) -> &'static [&'static str] {
        match self {
            SyncRoute::ComputeArtifacts => &["list", "read", "download", "open", "finalize"],
            SyncRoute::WorkOpen | SyncRoute::WorkPublish => &[],
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

    /// Grant routes are descriptor-backed too, so runtime timeouts and body
    /// caps cannot quietly drift from their native transport declaration.
    #[test]
    fn the_grant_routes_match_their_descriptor_declarations() {
        for (route, id) in [
            (WORK_OPEN, "domains.work.open"),
            (WORK_PUBLISH, "domains.work.publish"),
        ] {
            let entry = registry(id);
            assert_eq!(route.path, entry.path);
            assert_eq!(route.method, entry.method);
            assert_eq!(route.timeout_s, u64::from(entry.timeout_s));
            assert_eq!(route.response_cap_bytes, entry.response_cap_bytes);
            assert!(entry.carries_user_bearer());
        }
    }
}
