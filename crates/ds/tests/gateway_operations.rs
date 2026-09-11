//! Do `ds` and the desktop select the SAME gateway operations?
//!
//! `bridge_parity.rs` next door answers a different question — that a `ds map`
//! or `ds work` command names exactly one typed operation the paired desktop's
//! CLI bridge owns. That is the *desktop bridge*. This file is about the
//! *gateway*: the reviewed operation registry generated from
//! `ds-web/routing/operations.json` into `ds_client_core::gateway`, which the
//! browser reaches through its generated TS manifest and the CLI reaches
//! through `ClientProfile`. Contract §3 of
//! `ds-command-kernel/docs/contracts/ds-transport-authority.md`.
//!
//! The parity here is proved rather than restated. `ClientProfile::validate`
//! refuses any route that is not byte-for-byte the CLI's own compiled
//! constant, and every issued profile is fenced by `profile_sha256`. So a
//! profile assembled ENTIRELY from registry values either validates — in which
//! case the registry and the CLI agree about that method, path and action — or
//! it does not, and the two have drifted. `a_drifted_registry_route_is_refused`
//! shows the check bites rather than passing structurally.
//!
//! Refusals are tested next to the successes, because "fail closed" is the
//! half that a happy-path test cannot establish: an unknown operation id, a
//! profile whose lane and origin disagree, a profile pointed at an origin
//! outside the gateway, and an action outside a closed vocabulary.
//!
//! Two boundaries this file deliberately does NOT own, so they are not tested
//! twice:
//!
//! * the byte transfer itself carries no DS credential — pinned where the
//!   bytes are, by `ds-cli-auth`'s
//!   `upload::tests::a_storage_session_carries_no_ds_credential`;
//! * a stored project context belonging to another account is refused — pinned
//!   by `ds-cli-auth`'s
//!   `state::tests::a_context_for_another_account_is_refused`.

use ds_client_core::gateway::{
    GatewayOperation, OPERATIONS, PROFILE_CONSTANTS_WITHOUT_REGISTRY_ENTRY, operation,
};
use ds_client_core::{
    CLIENT_PROFILE_SCHEMA, ClientProfile, ClientProfileInput, DATA_DISTRIBUTION_ACTIONS,
    DESIGN_SELECTIONS_ACTIONS, DeploymentLane, LAYERS_ACTIONS, PRINTING_ACTIONS,
    PROCESSING_LANE_HEADER, PROJECT_DATA_ACTIONS, PROJECT_FORM_EDITOR_ACTION, PROJECT_FORMS_ACTION,
    PROJECT_REPORT_ACTIONS, ProfileError, SOLAR_SNAPSHOT_ACTION, STYLES_ACTION,
    SURVEY_CONTROL_ROUTES, SURVEY_ENTRIES_CHANGES_METHOD, SURVEY_ENTRIES_CHANGES_PATH,
    SURVEY_ENTRIES_SELECT_METHOD, SURVEY_ENTRIES_SELECT_PATH, SURVEY_ENTRY_CREATE_OPERATION,
    SURVEY_QUERY_METHOD, SURVEY_QUERY_PATH, TILES_ACTIONS, TRANSFORMER_CONTEXT_ACTION,
    TRANSFORMER_CONTEXT_FIELDS, TRANSFORMER_CONTEXT_METHOD, TRANSFORMER_CONTEXT_PATH,
};

/// Every registry operation `ds` can issue, and which of its typed calls does.
///
/// This list is the CLI's half of the answer: if a `Transport` method exists
/// for it, it is here. A row that is not here is an operation `ds` never sends.
const CLI_ISSUED: &[(&str, &str)] = &[
    ("domains.auth.device.begin", "ds auth login"),
    ("domains.auth.device.status", "ds auth login"),
    ("domains.auth.device.complete", "ds auth login"),
    ("domains.auth.device.refresh", "every authenticated call"),
    ("domains.auth.device.list", "ds auth device list"),
    ("domains.auth.device.read", "ds auth device read"),
    ("domains.auth.device.revoke", "ds auth device revoke"),
    ("domains.projects.read", "ds project list"),
    ("domains.project_data.action", "ds map data"),
    ("domains.project_forms.action", "ds survey forms"),
    ("domains.project_report.action", "ds report"),
    ("domains.styles.update", "ds style"),
    ("domains.tiles.action", "ds tile"),
    ("domains.solar.desktop_snapshot", "ds solar"),
    ("domains.printing.list", "ds map print"),
    (
        "domains.data_distribution.action",
        "ds-cli-auth data_distribution (headless seeding host; its commands land in a later slice)",
    ),
];

fn op(id: &str) -> &'static GatewayOperation {
    operation(id).unwrap_or_else(|| panic!("the registry declares no operation {id}"))
}

/// The registry's method and path for one operation, as owned strings the
/// profile input wants.
fn route(id: &str) -> (String, String) {
    let entry = op(id);
    (entry.method.to_owned(), entry.path.to_owned())
}

/// Routes the registry cannot supply yet, each with the reason, so the honest
/// remainder is asserted rather than quietly filled in from the CLI constant.
const CLI_ONLY: &[(&str, &str)] = &[
    (
        "POST /api/v1/survey/query",
        "No matching path exists in ds-apis-tf, so the gateway contract test could not validate a \
         declaration. Native client only.",
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
        "Six form-factory / template routes pinned as one profile string.",
    ),
];

/// A profile input whose every registry-backed field comes from the registry.
///
/// `lane` and the gateway origin are the caller's, so the same builder serves
/// the successful case and both lane/origin refusals.
fn profile_from_registry(lane: DeploymentLane, gateway_origin: &str) -> ClientProfileInput {
    let (auth_link_begin_method, auth_link_begin_path) = route("domains.auth.device.begin");
    let (auth_link_status_method, auth_link_status_path) = route("domains.auth.device.status");
    let (auth_link_complete_method, auth_link_complete_path) =
        route("domains.auth.device.complete");
    let (auth_device_refresh_method, auth_device_refresh_path) =
        route("domains.auth.device.refresh");
    let (auth_device_list_method, auth_device_list_path) = route("domains.auth.device.list");
    let (auth_device_read_method, auth_device_read_path_template) =
        route("domains.auth.device.read");
    let (auth_device_revoke_method, auth_device_revoke_path_template) =
        route("domains.auth.device.revoke");
    let (project_list_method, project_list_path) = route("domains.projects.read");
    let (project_forms_method, project_forms_path) = route("domains.project_forms.action");
    let (solar_snapshot_method, solar_snapshot_path) = route("domains.solar.desktop_snapshot");
    let (project_data_method, project_data_path) = route("domains.project_data.action");
    let (styles_method, styles_path) = route("domains.styles.update");
    let (printing_method, printing_path) = route("domains.printing.list");
    let (tiles_method, tiles_path) = route("domains.tiles.action");
    let (layers_method, layers_path) = route("domains.layers.action");
    let (project_report_method, project_report_path) = route("domains.project_report.action");
    let (survey_entry_create_method, survey_entry_create_path) =
        route("domains.survey_entries.create");
    let (design_selections_method, design_selections_path) =
        route("domains.design_collaboration.selections");
    let (data_distribution_method, data_distribution_path) =
        route("domains.data_distribution.action");

    ClientProfileInput {
        schema_version: CLIENT_PROFILE_SCHEMA.to_owned(),
        lane,
        source_revision: "registry-parity".to_owned(),
        descriptor_sha256: "a".repeat(64),
        firebase_project_id: "parity-project".to_owned(),
        firebase_api_key: "firebase-public".to_owned(),
        gateway_api_key: "gateway-public".to_owned(),
        gateway_origin: gateway_origin.to_owned(),
        auth_link_begin_method,
        auth_link_begin_path,
        auth_link_status_method,
        auth_link_status_path,
        auth_link_complete_method,
        auth_link_complete_path,
        auth_device_refresh_method,
        auth_device_refresh_path,
        auth_device_list_method,
        auth_device_list_path,
        auth_device_read_method,
        auth_device_read_path_template,
        auth_device_revoke_method,
        auth_device_revoke_path_template,
        project_list_method,
        project_list_path,
        // CLI-only: see CLI_ONLY.
        transformer_context_method: TRANSFORMER_CONTEXT_METHOD.to_owned(),
        transformer_context_path: TRANSFORMER_CONTEXT_PATH.to_owned(),
        transformer_context_action: TRANSFORMER_CONTEXT_ACTION.to_owned(),
        transformer_context_fields: TRANSFORMER_CONTEXT_FIELDS.to_owned(),
        project_forms_method,
        project_forms_path,
        project_forms_action: PROJECT_FORMS_ACTION.to_owned(),
        project_form_editor_action: PROJECT_FORM_EDITOR_ACTION.to_owned(),
        solar_snapshot_method,
        solar_snapshot_path,
        solar_snapshot_action: SOLAR_SNAPSHOT_ACTION.to_owned(),
        survey_query_method: SURVEY_QUERY_METHOD.to_owned(),
        survey_query_path: SURVEY_QUERY_PATH.to_owned(),
        survey_entries_select_method: SURVEY_ENTRIES_SELECT_METHOD.to_owned(),
        survey_entries_select_path: SURVEY_ENTRIES_SELECT_PATH.to_owned(),
        survey_entries_changes_method: SURVEY_ENTRIES_CHANGES_METHOD.to_owned(),
        survey_entries_changes_path: SURVEY_ENTRIES_CHANGES_PATH.to_owned(),
        survey_entry_create_method,
        survey_entry_create_path,
        survey_entry_create_operation: SURVEY_ENTRY_CREATE_OPERATION.to_owned(),
        project_data_method,
        project_data_path,
        // The registry declares this vocabulary too; it is asserted separately
        // in `the_action_vocabularies_the_cli_may_send_are_declared`.
        project_data_actions: PROJECT_DATA_ACTIONS.map(str::to_owned).to_vec(),
        survey_control_routes: SURVEY_CONTROL_ROUTES.map(str::to_owned).to_vec(),
        styles_method,
        styles_path,
        styles_action: STYLES_ACTION.to_owned(),
        printing_method,
        printing_path,
        // The registry splits printing into one id per action, so it has no
        // single vocabulary to copy. Named in `printing_is_one_profile_route`.
        printing_actions: PRINTING_ACTIONS.map(str::to_owned).to_vec(),
        layers_method,
        layers_path,
        layers_actions: LAYERS_ACTIONS.map(str::to_owned).to_vec(),
        tiles_method,
        tiles_path,
        tiles_actions: TILES_ACTIONS.map(str::to_owned).to_vec(),
        project_report_method,
        project_report_path,
        project_report_actions: PROJECT_REPORT_ACTIONS.map(str::to_owned).to_vec(),
        design_selections_method,
        design_selections_path,
        // The registry declares this vocabulary too; it is asserted separately
        // in `the_action_vocabularies_the_cli_may_send_are_declared`.
        design_selections_actions: DESIGN_SELECTIONS_ACTIONS.map(str::to_owned).to_vec(),
        data_distribution_method,
        data_distribution_path,
        // The registry declares this vocabulary too; it is asserted separately
        // in `the_action_vocabularies_the_cli_may_send_are_declared`.
        data_distribution_actions: DATA_DISTRIBUTION_ACTIONS.map(str::to_owned).to_vec(),
    }
}

const STABLE_ORIGIN: &str = "https://ds-stable.ue.gateway.dev";
const CANARY_ORIGIN: &str = "https://ds-canary.ue.gateway.dev";

// ---------------------------------------------------------------------------
// Selection: the CLI and the GUI resolve the same operations
// ---------------------------------------------------------------------------

#[test]
fn a_profile_built_from_the_registry_is_the_profile_the_cli_accepts() {
    for (lane, origin) in [
        (DeploymentLane::Stable, STABLE_ORIGIN),
        (DeploymentLane::Canary, CANARY_ORIGIN),
    ] {
        let profile = ClientProfile::validate(profile_from_registry(lane, origin))
            .expect("every registry-backed route is the route the CLI compiles");
        assert_eq!(profile.lane(), lane);
        // The fence the CLI actually enforces at call time is this digest, and
        // it was computed over registry values.
        assert_eq!(profile.profile_sha256().len(), 64);
    }
}

/// One registry-backed field, made to disagree with the CLI's constant.
type Drift = (&'static str, fn(&mut ClientProfileInput));

#[test]
fn a_drifted_registry_route_is_refused() {
    // Proof that the parity check bites. Each mutation is one registry-backed
    // field made to disagree with the CLI's compiled constant.
    let cases: &[Drift] = &[
        ("project_list_path", |input| {
            input.project_list_path = "/api/v1/user/project".to_owned()
        }),
        ("project_list_method", |input| {
            input.project_list_method = "POST".to_owned()
        }),
        ("project_data_path", |input| {
            input.project_data_path = "/api/v1/project-data".to_owned()
        }),
        ("styles_path", |input| {
            input.styles_path = "/api/v1/style".to_owned()
        }),
        ("tiles_method", |input| {
            input.tiles_method = "GET".to_owned()
        }),
        ("project_report_path", |input| {
            input.project_report_path = "/reports".to_owned()
        }),
        ("survey_entry_create_path", |input| {
            input.survey_entry_create_path = "/api/v1/entries/create".to_owned()
        }),
        ("auth_device_revoke_path_template", |input| {
            input.auth_device_revoke_path_template = "/api/v1/auth/devices".to_owned()
        }),
        ("data_distribution_path", |input| {
            input.data_distribution_path = "/api/v1/data_distribution".to_owned()
        }),
    ];
    for (field, drift) in cases {
        let mut input = profile_from_registry(DeploymentLane::Stable, STABLE_ORIGIN);
        drift(&mut input);
        assert!(
            ClientProfile::validate(input).is_err(),
            "a drifted {field} must fail closed, not be accepted"
        );
    }
}

#[test]
fn every_operation_the_cli_issues_declares_its_authority_and_principal() {
    for (id, sender) in CLI_ISSUED {
        let entry = op(id);
        assert_eq!(
            entry.authority, "gateway",
            "{id} ({sender}) is reached through the gateway, not a storage session"
        );
        assert!(
            matches!(entry.principal, "user" | "anonymous"),
            "{id} declares principal {}",
            entry.principal
        );
        // The device-link handshake runs before there is a principal, so those
        // three plus the refresh exchange are the only anonymous ones the CLI
        // sends; everything else carries the user's bearer.
        let handshake = id.starts_with("domains.auth.device.")
            && !matches!(
                *id,
                "domains.auth.device.list"
                    | "domains.auth.device.read"
                    | "domains.auth.device.revoke"
            );
        assert_eq!(
            entry.principal == "anonymous",
            handshake,
            "{id} ({sender}) has the wrong principal for when the CLI sends it"
        );
        assert_eq!(entry.carries_user_bearer(), !handshake, "{id} bearer");
        assert!(entry.timeout_s > 0 && entry.response_cap_bytes > 0, "{id}");
    }
}

#[test]
fn the_lane_header_is_declared_exactly_where_the_cli_sends_one() {
    // `NativeTransport::send_project_report` is the one call that attaches
    // `X-DS-Processing-Lane`; ds-brain otherwise defaults a lane-aware action
    // to the retired Standard lane.
    assert_eq!(PROCESSING_LANE_HEADER, "X-DS-Processing-Lane");
    for (id, sender) in CLI_ISSUED {
        let entry = op(id);
        assert_eq!(
            entry.lane_header,
            *id == "domains.project_report.action",
            "{id} ({sender}) lane header"
        );
    }
}

#[test]
fn the_action_vocabularies_the_cli_may_send_are_declared() {
    assert_eq!(
        op("domains.project_data.action").actions,
        &PROJECT_DATA_ACTIONS[..]
    );
    assert_eq!(op("domains.tiles.action").actions, &TILES_ACTIONS[..]);
    assert_eq!(
        op("domains.project_report.action").actions,
        &PROJECT_REPORT_ACTIONS[..]
    );
    assert_eq!(
        op("domains.design_collaboration.selections").actions,
        &DESIGN_SELECTIONS_ACTIONS[..]
    );
    // The seeding door sends the catalogue read and the derived acquisition
    // and nothing else: the billed page query and publication stay browser
    // surfaces.
    let distribution = op("domains.data_distribution.action");
    assert_eq!(distribution.actions, &DATA_DISTRIBUTION_ACTIONS[..]);
    assert!(!distribution.allows_action("query_dataset"));
    assert!(!distribution.allows_action("publish_dataset"));
    assert_eq!(distribution.timeout_s, 180);
    assert_eq!(op("domains.styles.update").actions, &[STYLES_ACTION]);
    assert_eq!(
        op("domains.solar.desktop_snapshot").actions,
        &[SOLAR_SNAPSHOT_ACTION]
    );
    // The CLI sends two of the three declared project-form actions. A subset is
    // correct; a value outside the vocabulary is not.
    let forms = op("domains.project_forms.action");
    assert!(forms.allows_action(PROJECT_FORMS_ACTION));
    assert!(forms.allows_action(PROJECT_FORM_EDITOR_ACTION));
    assert!(!forms.allows_action("delete_everything"));
}

#[test]
fn printing_is_one_profile_route_and_four_declared_operations() {
    // Honest divergence, stated rather than asserted away: the profile pins one
    // printing route with a seven-action vocabulary, while the registry
    // declares one id per reviewed printing operation and carries no vocabulary
    // on any of them. Both agree on the method and the path, which is what a
    // request is built from.
    let (method, path) = route("domains.printing.list");
    for id in [
        "domains.printing.list",
        "domains.printing.get",
        "domains.printing.save",
        "domains.printing.render",
    ] {
        let entry = op(id);
        assert_eq!(entry.method, method, "{id} method");
        assert_eq!(entry.path, path, "{id} path");
        assert!(entry.actions.is_empty(), "{id} carries no vocabulary");
    }
    assert_eq!(PRINTING_ACTIONS.len(), 7);
}

// ---------------------------------------------------------------------------
// Refusals: everything undeclared fails closed
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_operation_id_is_refused() {
    for unknown in [
        "",
        "domains.compute_artifacts.upload_bytes",
        "domains.projects",
        "domains.projects.read ",
        "DOMAINS.PROJECTS.READ",
        "../domains.projects.read",
        "domains.projects.delete_everything",
    ] {
        assert!(
            operation(unknown).is_none(),
            "{unknown} must not resolve to an operation"
        );
    }
    // And an id that IS declared still refuses an action outside its closed
    // vocabulary — a known operation is not an open door.
    assert!(!op("domains.tiles.action").allows_action("drop"));
    assert!(!op("domains.projects.read").allows_action("list"));
}

#[test]
fn a_wrong_lane_profile_is_refused() {
    // A canary origin under the stable lane, and a stable origin under canary.
    // Either way the credential audience would be wrong, so neither is issued.
    for (lane, origin) in [
        (DeploymentLane::Stable, CANARY_ORIGIN),
        (DeploymentLane::Canary, STABLE_ORIGIN),
    ] {
        assert_eq!(
            ClientProfile::validate(profile_from_registry(lane, origin)).unwrap_err(),
            ProfileError::LaneMismatch,
            "{origin} must not be issued under {lane:?}"
        );
    }
}

#[test]
fn a_wrong_origin_profile_is_refused() {
    for origin in [
        "https://attacker.example",
        "https://ds-stable.ue.gateway.dev.attacker.example",
        "http://ds-stable.ue.gateway.dev",
        "https://user:secret@ds-stable.ue.gateway.dev",
        "https://ds-stable.ue.gateway.dev:8443",
        "https://ds-stable.ue.gateway.dev/api",
        "https://ds-stable.ue.gateway.dev?key=1",
        "not a uri",
        "",
    ] {
        assert_eq!(
            ClientProfile::validate(profile_from_registry(DeploymentLane::Stable, origin))
                .unwrap_err(),
            ProfileError::GatewayOrigin,
            "{origin} must not be issued as a gateway origin"
        );
    }
}

// ---------------------------------------------------------------------------
// The honest remainder
// ---------------------------------------------------------------------------

#[test]
fn a_storage_session_operation_never_carries_a_bearer() {
    let declared: Vec<&str> = OPERATIONS
        .iter()
        .filter(|entry| entry.authority == "storage_session")
        .map(|entry| entry.id)
        .collect();
    for entry in OPERATIONS {
        if entry.authority == "storage_session" {
            assert!(
                !entry.carries_user_bearer(),
                "{} would attach a DS bearer to a storage session",
                entry.id
            );
        }
    }
    // The descriptor declares no storage-session row yet: the byte transfer to
    // a DS-minted session is not modelled as a gateway operation, only the
    // calls that MINT and FINALIZE it are. Those are ordinary gateway
    // operations and do carry a bearer, which is the distinction the contract
    // draws. Until a row exists, the loop above is vacuous and the real
    // guarantee is pinned where the bytes are, by ds-cli-auth's
    // `upload::tests::a_storage_session_carries_no_ds_credential`.
    assert!(
        declared.is_empty(),
        "storage_session operations are declared now ({declared:?}) — delete this assertion and \
         keep the loop"
    );
    assert!(op("domains.compute_artifacts.open").carries_user_bearer());
    assert!(op("domains.compute_artifacts.finalize").carries_user_bearer());
}

#[test]
fn the_routes_the_registry_cannot_supply_are_named() {
    for (route, reason) in CLI_ONLY {
        assert!(
            reason.len() > 40,
            "{route} needs a real reason, not a label"
        );
        assert!(
            !OPERATIONS
                .iter()
                .any(|entry| format!("{} {}", entry.method, entry.path) == *route),
            "{route} IS in the registry now — take it from the registry in \
             profile_from_registry and drop it from CLI_ONLY"
        );
        assert!(
            PROFILE_CONSTANTS_WITHOUT_REGISTRY_ENTRY
                .iter()
                .any(|(known, _)| known == route),
            "{route} is not on ds-client-core's own unfinished list; the two must agree"
        );
    }
    // Same count on both sides, so neither list can quietly grow.
    assert_eq!(
        CLI_ONLY.len(),
        PROFILE_CONSTANTS_WITHOUT_REGISTRY_ENTRY.len()
    );
}

#[test]
fn the_cli_issues_only_operations_the_registry_declares() {
    for (id, sender) in CLI_ISSUED {
        assert!(
            operation(id).is_some(),
            "{id} ({sender}) is issued by the CLI but not declared"
        );
    }
    let mut ids: Vec<&str> = CLI_ISSUED.iter().map(|(id, _)| *id).collect();
    ids.sort_unstable();
    let unique = {
        let mut copy = ids.clone();
        copy.dedup();
        copy
    };
    assert_eq!(ids, unique, "one CLI sender per operation id");
}
