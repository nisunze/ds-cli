//! The operation registry, as Rust sees it.
//!
//! One reviewed descriptor — `ds-web/routing/operations.json` — produces two
//! artefacts from the same bytes: the content-hashed TypeScript route manifest
//! the bundle loads, and the table in [`registry`] this module exposes. Contract
//! §3 of `ds-command-kernel/docs/contracts/ds-transport-authority.md`.
//!
//! **The registry is not yet the source of [`crate::profile`]'s constants.**
//! `ClientProfile::validate` hashes its route/method/action constants in a fixed
//! order into `profile_sha256`, and every issued profile is validated against
//! that digest — so making the registry the source would have to reproduce that
//! ordered digest input byte for byte or invalidate every profile in the field.
//! That unification is deliberately NOT done here. What is done instead is a
//! test (`registry_agrees_with_profile_constants`) that proves the two agree
//! today, plus `PROFILE_CONSTANTS_WITHOUT_REGISTRY_ENTRY`, which names every
//! profile-pinned route the registry does not yet carry and why. Adding a
//! profile constant without a registry row, or drifting one from the other,
//! fails that test.
//!
//! Nothing here performs IO. It is a lookup table over `&'static str`: the
//! vocabularies are validated by the generator, so a value that reached this
//! file was already checked against the contract's enums.

pub mod registry;

pub use registry::{DESCRIPTOR_SHA256, NATIVE_ONLY_OPERATIONS, OPERATIONS, SCHEMA_VERSION};

/// Crate-level aliases, so a consumer that only wants the registry does not
/// have to import three same-named constants from three modules.
pub use registry::DESCRIPTOR_SHA256 as GATEWAY_REGISTRY_DESCRIPTOR_SHA256;
pub use registry::OPERATIONS as GATEWAY_REGISTRY_OPERATIONS;
pub use registry::SCHEMA_VERSION as GATEWAY_REGISTRY_SCHEMA_VERSION;

/// One reviewed operation. Every field is exactly what the descriptor declares.
///
/// The vocabulary fields stay `&'static str` on purpose: the generator is the
/// validator, and a second enum here would be a second reading of the same
/// contract — the drift this whole mechanism exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GatewayOperation {
    /// Logical id, e.g. `domains.compute_artifacts.open`.
    pub id: &'static str,
    /// HTTP method, upper case.
    pub method: &'static str,
    /// Gateway path. May template `{name}` segments named in `path_params`.
    pub path: &'static str,
    /// The template parameters `path` expects, in declaration order.
    pub path_params: &'static [&'static str],
    /// Body key that selects the action when the path multiplexes.
    pub action_key: &'static str,
    /// The closed action vocabulary this operation may send. Empty when the
    /// path does not multiplex.
    pub actions: &'static [&'static str],
    /// `json` | `ndjson` | `blob` | `sse` | `resumable_upload` | `anonymous` | `native`.
    pub transport: &'static str,
    /// `gateway` | `release` | `storage_session`.
    pub authority: &'static str,
    /// `user` (a Firebase bearer is attached) | `anonymous` (never one).
    pub principal: &'static str,
    /// Whether `X-DS-Processing-Lane` applies to this operation.
    pub lane_header: bool,
    /// `idempotent` | `idempotent_with_key` | `unsafe`.
    pub idempotency: &'static str,
    /// `none` | `transport_only` | `transport_and_5xx`.
    pub retry_class: &'static str,
    /// Deadline owned by the registry, not by the caller.
    pub timeout_s: u32,
    /// Response ceiling owned by the registry, not by the caller.
    pub response_cap_bytes: u64,
    /// How the web placement resolves: `gateway` | `native` | `unavailable`.
    pub resolution_web: &'static str,
    /// How the desktop placement resolves: `gateway` | `native` | `unavailable`.
    pub resolution_desktop: &'static str,
    /// Native command name when one is declared, otherwise empty.
    pub native_command: &'static str,
}

impl GatewayOperation {
    /// True when this operation may send `action` under `action_key`.
    ///
    /// An operation with no declared vocabulary multiplexes nothing, so it
    /// accepts no action at all — that is a refusal, not a wildcard.
    pub fn allows_action(&self, action: &str) -> bool {
        self.actions.contains(&action)
    }

    /// True when a DS user bearer may be attached to this operation.
    pub fn carries_user_bearer(&self) -> bool {
        self.principal == "user" && self.authority != "storage_session"
    }
}

/// Look one operation up by its logical id.
///
/// There is deliberately no "resolve a URL" or "build a request" here: this
/// crate's typed calls own that, and an arbitrary-path door is exactly what the
/// registry exists to close.
pub fn operation(id: &str) -> Option<&'static GatewayOperation> {
    OPERATIONS.iter().find(|entry| entry.id == id)
}

/// Every declared id, in table order (sorted by the generator).
pub fn operation_ids() -> impl Iterator<Item = &'static str> {
    OPERATIONS.iter().map(|entry| entry.id)
}

/// Profile-pinned routes the registry does NOT carry yet, each with its reason.
///
/// This list is asserted, not decorative: it is the honest count of how far the
/// unification has got. Removing an entry means declaring that operation in
/// `routing/operations.json`.
pub const PROFILE_CONSTANTS_WITHOUT_REGISTRY_ENTRY: &[(&str, &str)] = &[
    (
        "POST /api/v1/survey/query",
        "No matching path exists in ds-apis-tf, so the gateway contract test could not validate a \
         declaration. The route is reached by the native client only.",
    ),
    (
        "POST /api/v1/survey/entries/select",
        "No matching path in ds-apis-tf; native client only.",
    ),
    (
        "POST /api/v1/survey/entries/changes",
        "No matching path in ds-apis-tf; native client only.",
    ),
    (
        "SURVEY_CONTROL_ROUTES",
        "Six form-factory / template routes pinned as one profile string. They need six reviewed \
         descriptor entries, not one.",
    ),
    (
        "POST /v1/projects/{firebase_project}/databases/(default)/documents:commit",
        "Not a gateway route by design: Survey direct create commits to Firestore on \
         firestore.googleapis.com under the user's Firebase session (survey-entry-create contract).",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// The exact descriptor bytes this crate was generated from.
    const DESCRIPTOR_SOURCE: &str = include_str!("../../../routing/operations.json");

    fn op(id: &str) -> &'static GatewayOperation {
        operation(id).unwrap_or_else(|| panic!("registry has no operation {id}"))
    }

    /// The coupling point. The generator hashes the descriptor source and
    /// compiles that digest into BOTH artefacts; if this table were regenerated
    /// from other bytes, or the descriptor edited without regenerating, the two
    /// stop matching here.
    #[test]
    fn registry_was_generated_from_these_descriptor_bytes() {
        let digest = format!("{:x}", Sha256::digest(DESCRIPTOR_SOURCE.as_bytes()));
        assert_eq!(
            digest, DESCRIPTOR_SHA256,
            "crates/ds-client-core/src/gateway/registry.rs is stale. Regenerate it:\n  \
             cd ds-web && npm run routes:generate -- --target web-canary --gateway <origin>"
        );
        assert_eq!(SCHEMA_VERSION, "ds-web.gateway-registry/v1");
    }

    #[test]
    fn ids_are_unique_and_sorted() {
        let ids: Vec<&str> = operation_ids().collect();
        assert!(!ids.is_empty());
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "the generated table must stay sorted by id");
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            ids.len(),
            "duplicate operation id in the registry"
        );
    }

    #[test]
    fn every_row_carries_the_full_reviewed_field_set() {
        for entry in OPERATIONS {
            assert!(
                entry.path.starts_with('/'),
                "{} has a non-absolute path",
                entry.id
            );
            assert!(!entry.method.is_empty(), "{} has no method", entry.id);
            assert!(
                matches!(
                    entry.transport,
                    "json"
                        | "ndjson"
                        | "blob"
                        | "sse"
                        | "resumable_upload"
                        | "anonymous"
                        | "native"
                ),
                "{} declares transport {}",
                entry.id,
                entry.transport
            );
            assert!(
                matches!(entry.authority, "gateway" | "release" | "storage_session"),
                "{} declares authority {}",
                entry.id,
                entry.authority
            );
            assert!(
                matches!(entry.principal, "user" | "anonymous"),
                "{} declares principal {}",
                entry.id,
                entry.principal
            );
            assert!(
                matches!(
                    entry.idempotency,
                    "idempotent" | "idempotent_with_key" | "unsafe"
                ),
                "{} declares idempotency {}",
                entry.id,
                entry.idempotency
            );
            assert!(
                matches!(
                    entry.retry_class,
                    "none" | "transport_only" | "transport_and_5xx"
                ),
                "{} declares retry_class {}",
                entry.id,
                entry.retry_class
            );
            assert!(entry.timeout_s > 0, "{} has no deadline", entry.id);
            assert!(
                entry.response_cap_bytes > 0,
                "{} has no response cap",
                entry.id
            );
            let templated = entry.path.matches('{').count();
            assert_eq!(
                templated,
                entry.path_params.len(),
                "{} templates {templated} segments but declares {} parameters",
                entry.id,
                entry.path_params.len()
            );
        }
    }

    /// No operation may carry a DS bearer to a storage session: the session URI
    /// is the whole credential (contract §3).
    #[test]
    fn storage_session_operations_never_carry_a_bearer() {
        for entry in OPERATIONS {
            if entry.authority == "storage_session" {
                assert!(
                    !entry.carries_user_bearer(),
                    "{} would attach a bearer to a storage session",
                    entry.id
                );
            }
        }
    }

    #[test]
    fn anonymous_operations_are_recognisable_as_such() {
        let examples = op("domains.grid.examples.list");
        assert_eq!(examples.principal, "anonymous");
        assert!(!examples.carries_user_bearer());
        assert_eq!(op("domains.grid.examples.get").principal, "anonymous");
        assert!(op("domains.grid.catalog.action").carries_user_bearer());
    }

    #[test]
    fn action_multiplexed_operations_declare_a_closed_vocabulary() {
        let catalog = op("domains.grid.catalog.action");
        assert_eq!(catalog.action_key, "action");
        assert!(catalog.allows_action("publish_library_release"));
        assert!(catalog.allows_action("resolve_library_member"));
        assert!(!catalog.allows_action("delete_everything"));
        // A single-action operation still refuses anything else.
        let open = op("domains.compute_artifacts.open");
        assert_eq!(open.actions, &["open"]);
        assert!(!open.allows_action("finalize"));
    }

    /// The registry and `profile.rs` must not disagree about a route.
    ///
    /// This is the safe half of the unification: equality is asserted, the
    /// profile digest is untouched. See the module docs.
    #[test]
    fn registry_agrees_with_profile_constants() {
        use crate::profile::{
            ADMIN_BOUNDS_ACTIONS, ADMIN_BOUNDS_METHOD, ADMIN_BOUNDS_PATH,
            DATA_DISTRIBUTION_ACTIONS, DATA_DISTRIBUTION_METHOD, DATA_DISTRIBUTION_PATH,
            DESIGN_SELECTIONS_ACTIONS, DESIGN_SELECTIONS_METHOD, DESIGN_SELECTIONS_PATH,
            SRE_EVENTS_ACTIONS, SRE_EVENTS_METHOD, SRE_EVENTS_PATH, SRE_OVERVIEW_METHOD,
            SRE_OVERVIEW_PATH,
        };
        use crate::profile::{
            AUTH_DEVICE_LIST_METHOD, AUTH_DEVICE_LIST_PATH, AUTH_DEVICE_READ_METHOD,
            AUTH_DEVICE_READ_PATH_TEMPLATE, AUTH_DEVICE_REFRESH_METHOD, AUTH_DEVICE_REFRESH_PATH,
            AUTH_DEVICE_REVOKE_METHOD, AUTH_DEVICE_REVOKE_PATH_TEMPLATE, AUTH_LINK_BEGIN_METHOD,
            AUTH_LINK_BEGIN_PATH, AUTH_LINK_COMPLETE_METHOD, AUTH_LINK_COMPLETE_PATH,
            AUTH_LINK_STATUS_METHOD, AUTH_LINK_STATUS_PATH, LAYERS_ACTIONS, LAYERS_METHOD,
            LAYERS_PATH, PROJECT_DATA_ACTIONS, PROJECT_DATA_METHOD, PROJECT_DATA_PATH,
            PROJECT_FORMS_ACTION, PROJECT_FORMS_METHOD, PROJECT_FORMS_PATH,
            PROJECT_FORM_EDITOR_ACTION, PROJECT_LIST_METHOD, PROJECT_LIST_PATH,
            PROJECT_REPORT_ACTIONS, PROJECT_REPORT_METHOD, PROJECT_REPORT_PATH,
            SOLAR_SNAPSHOT_ACTION, SOLAR_SNAPSHOT_METHOD, SOLAR_SNAPSHOT_PATH, STYLES_ACTION,
            STYLES_METHOD, STYLES_PATH, TILES_ACTIONS, TILES_METHOD, TILES_PATH,
        };

        let route_pairs: &[(&str, &str, &str)] = &[
            (
                "domains.auth.device.begin",
                AUTH_LINK_BEGIN_METHOD,
                AUTH_LINK_BEGIN_PATH,
            ),
            (
                "domains.auth.device.status",
                AUTH_LINK_STATUS_METHOD,
                AUTH_LINK_STATUS_PATH,
            ),
            (
                "domains.auth.device.complete",
                AUTH_LINK_COMPLETE_METHOD,
                AUTH_LINK_COMPLETE_PATH,
            ),
            (
                "domains.auth.device.refresh",
                AUTH_DEVICE_REFRESH_METHOD,
                AUTH_DEVICE_REFRESH_PATH,
            ),
            (
                "domains.auth.device.list",
                AUTH_DEVICE_LIST_METHOD,
                AUTH_DEVICE_LIST_PATH,
            ),
            (
                "domains.auth.device.read",
                AUTH_DEVICE_READ_METHOD,
                AUTH_DEVICE_READ_PATH_TEMPLATE,
            ),
            (
                "domains.auth.device.revoke",
                AUTH_DEVICE_REVOKE_METHOD,
                AUTH_DEVICE_REVOKE_PATH_TEMPLATE,
            ),
            (
                "domains.projects.read",
                PROJECT_LIST_METHOD,
                PROJECT_LIST_PATH,
            ),
            (
                "domains.project_data.action",
                PROJECT_DATA_METHOD,
                PROJECT_DATA_PATH,
            ),
            (
                "domains.project_forms.action",
                PROJECT_FORMS_METHOD,
                PROJECT_FORMS_PATH,
            ),
            (
                "domains.project_report.action",
                PROJECT_REPORT_METHOD,
                PROJECT_REPORT_PATH,
            ),
            ("domains.styles.update", STYLES_METHOD, STYLES_PATH),
            ("domains.tiles.action", TILES_METHOD, TILES_PATH),
            (
                "domains.solar.desktop_snapshot",
                SOLAR_SNAPSHOT_METHOD,
                SOLAR_SNAPSHOT_PATH,
            ),
            (
                "domains.printing.list",
                crate::PRINTING_METHOD,
                crate::PRINTING_PATH,
            ),
            (
                "domains.printing.get",
                crate::PRINTING_METHOD,
                crate::PRINTING_PATH,
            ),
            (
                "domains.printing.save",
                crate::PRINTING_METHOD,
                crate::PRINTING_PATH,
            ),
            (
                "domains.design_collaboration.selections",
                DESIGN_SELECTIONS_METHOD,
                DESIGN_SELECTIONS_PATH,
            ),
            (
                "domains.data_distribution.action",
                DATA_DISTRIBUTION_METHOD,
                DATA_DISTRIBUTION_PATH,
            ),
            (
                "domains.admin_bounds.action",
                ADMIN_BOUNDS_METHOD,
                ADMIN_BOUNDS_PATH,
            ),
            (
                "domains.sre.overview",
                SRE_OVERVIEW_METHOD,
                SRE_OVERVIEW_PATH,
            ),
            ("domains.sre.events", SRE_EVENTS_METHOD, SRE_EVENTS_PATH),
        ];
        for (id, method, path) in route_pairs {
            let entry = op(id);
            assert_eq!(entry.method, *method, "{id} method");
            assert_eq!(entry.path, *path, "{id} path");
        }

        // Action vocabularies, where both sides declare one.
        assert_eq!(
            op("domains.project_data.action").actions,
            &PROJECT_DATA_ACTIONS[..]
        );
        assert_eq!(op("domains.tiles.action").actions, &TILES_ACTIONS[..]);
        assert_eq!(op("domains.layers.action").actions, &LAYERS_ACTIONS[..]);
        assert_eq!(op("domains.layers.action").path, LAYERS_PATH);
        assert_eq!(op("domains.layers.action").method, LAYERS_METHOD);
        assert_eq!(
            op("domains.project_report.action").actions,
            &PROJECT_REPORT_ACTIONS[..]
        );
        assert_eq!(op("domains.styles.update").actions, &[STYLES_ACTION]);
        assert_eq!(
            op("domains.solar.desktop_snapshot").actions,
            &[SOLAR_SNAPSHOT_ACTION]
        );
        assert_eq!(
            op("domains.design_collaboration.selections").actions,
            &DESIGN_SELECTIONS_ACTIONS[..]
        );
        // The seeding door: exactly the catalogue read and the derived
        // acquisition, on the bulk lane with the gateway's own deadline.
        let distribution = op("domains.data_distribution.action");
        assert_eq!(distribution.actions, &DATA_DISTRIBUTION_ACTIONS[..]);
        assert_eq!(distribution.timeout_s, 180);
        assert_eq!(distribution.response_cap_bytes, 32 * 1024 * 1024);
        assert!(!distribution.allows_action("query_dataset"));
        assert!(!distribution.allows_action("publish_dataset"));
        // The national hierarchy: two exact reads, and none of the unbounded
        // or duplicate actions the same path also answers.
        let admin = op("domains.admin_bounds.action");
        assert_eq!(admin.actions, &ADMIN_BOUNDS_ACTIONS[..]);
        assert!(admin.allows_action("children"));
        assert!(!admin.allows_action("descendants"));
        assert!(!admin.allows_action("enrich"));
        assert!(!admin.allows_action("locate"));
        // Platform reliability: one read with no action at all, and one that
        // borrows the reusable tabular stream for exactly one of its actions.
        let overview = op("domains.sre.overview");
        assert!(overview.actions.is_empty());
        assert_eq!(overview.transport, "json");
        let events = op("domains.sre.events");
        assert_eq!(events.actions, &SRE_EVENTS_ACTIONS[..]);
        assert!(events.allows_action("query_table"));
        assert!(!events.allows_action("query_entries"));
        assert!(!events.allows_action("save_transformers_data"));
        // The window is a stream, and a stream that is silently retried bills a
        // second scan for an answer nobody has been told is incomplete.
        assert_eq!(events.transport, "ndjson");
        assert_eq!(events.retry_class, "none");
        let forms = op("domains.project_forms.action");
        assert!(forms.allows_action(PROJECT_FORMS_ACTION));
        assert!(forms.allows_action(PROJECT_FORM_EDITOR_ACTION));
    }

    /// The honest remainder. If this count moves, the report was wrong.
    #[test]
    fn the_unfinished_half_is_named_not_hidden() {
        assert_eq!(PROFILE_CONSTANTS_WITHOUT_REGISTRY_ENTRY.len(), 5);
        for (route, reason) in PROFILE_CONSTANTS_WITHOUT_REGISTRY_ENTRY {
            assert!(!route.is_empty());
            assert!(
                reason.len() > 40,
                "{route} needs a real reason, not a label"
            );
            assert!(
                !OPERATIONS
                    .iter()
                    .any(|entry| format!("{} {}", entry.method, entry.path) == *route),
                "{route} IS in the registry now — remove it from the unfinished list"
            );
        }
    }
}
