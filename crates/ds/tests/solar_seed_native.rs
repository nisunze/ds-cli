//! Governed Solar seeding uses explicit native device authority without Desktop.
//! The in-memory device and closed gateway transport follow the owner's
//! client_contract fixture; no ambient credential, selected project or network
//! endpoint participates in these tests.

use ds_client_core::solar_project::Command as SeedCommand;
use ds_client_core::{
    AuthorizedTransport, CLIENT_PROFILE_SCHEMA, ClientError, ClientProfile, ClientProfileInput,
    DeploymentLane, DeviceAccessSession, DeviceBinding, DeviceCredential, DevicePrivateKey,
    ErrorKind, SolarProjectCall, TransportError, TransportResponse,
};
use serde_json::{Value, json};
use std::process::Command;
const NOW: u64 = 1_900_000_000;

fn profile_input(lane: DeploymentLane) -> ClientProfileInput {
    let gateway_origin = match lane {
        DeploymentLane::Stable => "https://eds-gateway-3c0q477h.ue.gateway.dev",
        DeploymentLane::Canary => "https://eds-gateway-canary-3c0q477h.ue.gateway.dev",
    };
    ClientProfileInput {
        schema_version: CLIENT_PROFILE_SCHEMA.to_owned(),
        lane,
        source_revision: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        descriptor_sha256: "a".repeat(64),
        firebase_project_id: "firebase-project".to_owned(),
        firebase_api_key: "public-firebase-key".to_owned(),
        gateway_api_key: "public-gateway-key".to_owned(),
        gateway_origin: gateway_origin.to_owned(),
        auth_link_begin_method: "POST".to_owned(),
        auth_link_begin_path: "/api/v1/auth/device/begin".to_owned(),
        auth_link_status_method: "POST".to_owned(),
        auth_link_status_path: "/api/v1/auth/device/status".to_owned(),
        auth_link_complete_method: "POST".to_owned(),
        auth_link_complete_path: "/api/v1/auth/device/complete".to_owned(),
        auth_device_refresh_method: "POST".to_owned(),
        auth_device_refresh_path: "/api/v1/auth/device/refresh".to_owned(),
        auth_device_list_method: "GET".to_owned(),
        auth_device_list_path: "/api/v1/auth/devices".to_owned(),
        auth_device_read_method: "GET".to_owned(),
        auth_device_read_path_template: "/api/v1/auth/devices/{device_id}".to_owned(),
        auth_device_revoke_method: "DELETE".to_owned(),
        auth_device_revoke_path_template: "/api/v1/auth/devices/{device_id}".to_owned(),
        project_list_method: "GET".to_owned(),
        project_list_path: "/api/v1/user/projects".to_owned(),
        transformer_context_method: "POST".to_owned(),
        transformer_context_path: "/report".to_owned(),
        transformer_context_action: "get_transformers_data".to_owned(),
        transformer_context_fields: "full".to_owned(),
        project_forms_method: "POST".to_owned(),
        project_forms_path: "/api/v1/project-forms".to_owned(),
        project_forms_action: "activate".to_owned(),
        project_form_editor_action: "settings_editor".to_owned(),
        solar_snapshot_method: "POST".to_owned(),
        solar_snapshot_path: "/api/v1/solar".to_owned(),
        solar_snapshot_action: "desktop_snapshot".to_owned(),
        survey_query_method: "POST".to_owned(),
        survey_query_path: "/api/v1/survey/query".to_owned(),
        survey_entries_select_method: "POST".to_owned(),
        survey_entries_select_path: "/api/v1/survey/entries/select".to_owned(),
        survey_entries_changes_method: "POST".to_owned(),
        survey_entries_changes_path: "/api/v1/survey/entries/changes".to_owned(),
        survey_entry_create_method: "POST".to_owned(),
        survey_entry_create_path: "/api/v1/entries/mutate".to_owned(),
        survey_entry_create_operation: "create".to_owned(),
        project_data_method: "POST".to_owned(),
        project_data_path: "/api/v1/project_data".to_owned(),
        project_data_actions: ["list", "upload_start", "upload", "delete"]
            .map(str::to_owned)
            .to_vec(),
        survey_control_routes: ds_client_core::SURVEY_CONTROL_ROUTES
            .map(str::to_owned)
            .to_vec(),
        styles_method: "POST".to_owned(),
        styles_path: "/api/v1/styles".to_owned(),
        styles_action: "update_style".to_owned(),
        printing_method: "POST".into(),
        printing_path: "/api/v1/printing".into(),
        printing_actions: ds_client_core::PRINTING_ACTIONS.map(str::to_owned).to_vec(),
        layers_method: "POST".to_owned(),
        layers_path: "/api/v1/layers".to_owned(),
        layers_actions: ds_client_core::LAYERS_ACTIONS.map(str::to_owned).to_vec(),
        tiles_method: "POST".to_owned(),
        tiles_path: "/api/v1/tiles".to_owned(),
        tiles_actions: vec![
            "status".to_owned(),
            "preflight".to_owned(),
            "generate".to_owned(),
            "list".to_owned(),
            "add".to_owned(),
            "remove".to_owned(),
        ],
        project_report_method: "POST".to_owned(),
        project_report_path: "/report".to_owned(),
        project_report_actions: vec![
            "download_transfo".to_owned(),
            "list_compounded_reports".to_owned(),
            "transformer_inventory".to_owned(),
            "retire_transformer".to_owned(),
            "restore_transformer".to_owned(),
            "list_transformers_status".to_owned(),
        ],
        design_versions_method: "POST".into(),
        design_versions_path: "/api/v1/design/versions".into(),
        design_versions_actions: vec![
            "list_versions".into(),
            "get_version".into(),
            "get_head".into(),
            "create_version".into(),
            "restore_version".into(),
        ],
        design_selections_method: "POST".to_owned(),
        design_selections_path: "/api/v1/design/selections".to_owned(),
        design_selections_actions: vec![
            "list".to_owned(),
            "get".to_owned(),
            "save".to_owned(),
            "archive".to_owned(),
            "promote_task".to_owned(),
        ],
        data_distribution_method: "POST".to_owned(),
        data_distribution_path: "/api/v1/data-distribution".to_owned(),
        data_distribution_actions: vec![
            "list_datasets".to_owned(),
            "query_print_context".to_owned(),
        ],
    }
}

#[derive(Default)]
struct NativeGateway {
    response: Option<TransportResponse>,
    requests: Vec<Value>,
}
impl AuthorizedTransport for NativeGateway {
    fn list_projects(
        &mut self,
        _call: ds_client_core::ProjectListCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn transformer_context(
        &mut self,
        _call: ds_client_core::TransformerContextCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn project_forms(
        &mut self,
        _call: ds_client_core::ProjectFormsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn project_form_editor(
        &mut self,
        _call: ds_client_core::ProjectFormEditorCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn solar_snapshot(
        &mut self,
        _call: ds_client_core::SolarSnapshotCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn survey_query(
        &mut self,
        _call: ds_client_core::SurveyQueryCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn survey_entries_select(
        &mut self,
        _call: ds_client_core::SurveyEntriesSelectCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn survey_entries_changes(
        &mut self,
        _call: ds_client_core::SurveyEntriesChangesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn survey_entry_create(
        &mut self,
        _call: ds_client_core::SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn project_data(
        &mut self,
        _call: ds_client_core::ProjectDataCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn upload_bytes(
        &mut self,
        _call: ds_client_core::UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn survey_control(
        &mut self,
        _call: ds_client_core::SurveyControlCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn styles(
        &mut self,
        _call: ds_client_core::StylesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn layers(
        &mut self,
        _call: ds_client_core::LayersCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn tiles(
        &mut self,
        _call: ds_client_core::TileCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn project_report(
        &mut self,
        _call: ds_client_core::ProjectReportCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn solar_project(
        &mut self,
        call: SolarProjectCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.method(), "POST");
        assert_eq!(call.path(), "/api/v1/solar");
        assert!(!call.bearer_token().is_empty());
        self.requests
            .push(serde_json::from_slice(call.body().as_bytes()).unwrap());
        self.response.take().ok_or(TransportError::Unreachable)
    }
}

fn native(
    project: &str,
    command: SeedCommand,
    status: u16,
    body: Value,
) -> (Result<Value, ClientError>, Vec<Value>) {
    let profile = ClientProfile::validate(profile_input(DeploymentLane::Stable)).unwrap();
    let credential = DeviceCredential::new(
        "device-1".to_owned(),
        "Workstation".to_owned(),
        "linux".to_owned(),
        format!("sha256:{}", "c".repeat(64)),
        "uid-1".to_owned(),
        "user@example.com".to_owned(),
        &DeviceBinding::for_profile(&profile, &"b".repeat(64)).unwrap(),
        "2031-01-01T00:00:00Z".to_owned(),
        DevicePrivateKey::from_secret_bytes([5; 32]),
    )
    .unwrap();
    let access =
        DeviceAccessSession::new("fixture-memory-access".to_owned(), NOW + 300, NOW).unwrap();
    let authorization = credential
        .authorize_api(&access, &profile, "uid-1", NOW)
        .unwrap();
    let mut gateway = NativeGateway {
        response: Some(TransportResponse::new(
            status,
            serde_json::to_vec(&body).unwrap(),
        )),
        ..Default::default()
    };
    let result = authorization.solar_project(&mut gateway, project, &command, NOW + 1);
    (result, gateway.requests)
}
fn seed(source: Option<&str>, cities: &[&str], digest: Option<&str>) -> SeedCommand {
    SeedCommand::Seed {
        source: source.map(str::to_owned),
        cities: cities.iter().map(|s| (*s).to_owned()).collect(),
        overwrite: false,
        digest: digest.map(str::to_owned),
    }
}
fn ok(data: Value) -> Value {
    json!({"success":true,"data":data})
}
fn ds(args: &[&str]) -> (Value, i32) {
    let config = tempfile::tempdir().unwrap();
    let profile = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-cli-auth/tests/fixtures/development-catalog.json");
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("DS_CONFIG_HOME", config.path())
        .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", profile)
        .env_remove("DS_NATIVE_CLIENT_PRODUCT_ROOT")
        .env_remove("DS_NATIVE_CLIENT_PROFILE_SHA256")
        .env(
            "DS_DESKTOP_DESCRIPTOR",
            config.path().join("no-desktop.json"),
        )
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().unwrap_or(-1))
}
const SEED_DIGEST: &str = "d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3";
const OTHER_DIGEST: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";

/// One city row shaped exactly as ds-brain emits it: the city ROOT first,
/// marked `kind: "root"` with an empty subcollection and the city's own id.
fn seed_city(city_id: &str, action: &str, inputs: usize) -> Value {
    let mut documents = vec![json!({
        "subcollection": "",
        "doc_id": city_id,
        "kind": "root",
        "digest": "root-digest",
        "bytes": 96,
    })];
    for index in 0..inputs {
        documents.push(json!({
            "subcollection": "01_city_inputs",
            "doc_id": format!("input-{index}"),
            "digest": format!("input-digest-{index}"),
            "bytes": 128,
        }));
    }
    json!({
        "city_id": city_id,
        "display_name": city_id,
        "action": action,
        "source_digest": format!("source-{city_id}"),
        "root_digest": "root-digest",
        "documents": documents,
        "assets": [],
    })
}

/// A plan whose counts are internally consistent, so a test that breaks one
/// field is breaking exactly that field.
fn seed_plan(cities: Vec<Value>) -> Value {
    let class = |action: &str| {
        cities
            .iter()
            .filter(|city| city["action"] == action)
            .count()
    };
    let document_count: usize = cities
        .iter()
        .filter(|city| city["action"] == "create")
        .map(|city| city["documents"].as_array().map_or(0, Vec::len))
        .sum();
    json!({
        "root": "eds_project/demo/eds_solar",
        "seed_source_root": "eds_solar",
        "ds_project": "demo",
        "seed_digest": SEED_DIGEST,
        "cities": cities,
        "create_count": class("create"),
        "skip_count": class("skip"),
        "changed_count": class("changed"),
        "missing_count": class("missing"),
        "document_count": document_count,
        "asset_count": 1,
        "excluded_asset_count": 1,
        "warnings": ["network_assets_are_not_seeded"],
        "mutated": false,
    })
}

#[test]
fn native_seed_preview_sends_explicit_project_and_only_selected_optional_inputs() {
    let plan = seed_plan(vec![
        seed_city("huye", "create", 2),
        seed_city("gasabo", "changed", 3),
    ]);
    let data = json!({"plan":plan});
    let (result, requests) = native(
        "demo",
        seed(Some("eds_solar"), &["huye", "gasabo"], None),
        200,
        ok(data.clone()),
    );
    assert_eq!(result.unwrap(), data);
    assert_eq!(requests,json!([{"action":"seed_preview","root":"eds_project/demo/eds_solar","seed_source_root":"eds_solar","cities":["huye","gasabo"]}]).as_array().unwrap().clone());
}

#[test]
fn native_seed_preview_omits_unset_source_and_cities() {
    let (result, requests) = native(
        "demo",
        seed(None, &[], None),
        200,
        ok(json!({"plan":seed_plan(vec![seed_city("huye","create",1)])})),
    );
    assert!(result.is_ok());
    assert_eq!(
        requests,
        vec![json!({"action":"seed_preview","root":"eds_project/demo/eds_solar"})]
    );
}

#[test]
fn seed_apply_confirmation_precedes_authentication_and_gateway() {
    let (reply, code) = ds(&[
        "solar",
        "seed",
        "apply",
        "--project",
        "demo",
        "--seed-digest",
        SEED_DIGEST,
        "--output",
        "json",
    ]);
    assert_eq!(reply["error"]["code"], "confirmation_required");
    assert_ne!(code, 0);
}

#[test]
fn native_seed_apply_echoes_the_confirmed_digest_and_returns_exact_plan() {
    let data = json!({"plan":seed_plan(vec![seed_city("huye","create",2)]),"seed_digest":SEED_DIGEST,
        "applied_cities":["huye"],"skipped_cities":[],"applied_count":1,"skipped_count":0,"documents_written":3,"idempotent":false});
    let (result, requests) = native(
        "demo",
        seed(None, &["huye"], Some(SEED_DIGEST)),
        200,
        ok(data.clone()),
    );
    assert_eq!(result.unwrap(), data);
    assert_eq!(
        requests,
        vec![
            json!({"action":"seed_apply","root":"eds_project/demo/eds_solar","cities":["huye"],"seed_digest":SEED_DIGEST})
        ]
    );
}

#[test]
fn native_seed_refuses_unconfirmed_digest_foreign_project_and_mutating_preview() {
    let mut foreign = seed_plan(vec![seed_city("huye", "create", 2)]);
    foreign["root"] = json!("eds_project/other/eds_solar");
    let mut mutating = seed_plan(vec![seed_city("huye", "create", 2)]);
    mutating["mutated"] = json!(true);
    let cases = [
        (
            seed(None, &[], Some(SEED_DIGEST)),
            json!({"plan":seed_plan(vec![seed_city("huye","create",2)]),"seed_digest":OTHER_DIGEST}),
        ),
        (seed(None, &[], None), json!({"plan":foreign})),
        (seed(None, &[], None), json!({"plan":mutating})),
    ];
    for (command, data) in cases {
        let (result, requests) = native("demo", command, 200, ok(data));
        assert_eq!(result.unwrap_err().kind(), ErrorKind::UnreadableResponse);
        assert_eq!(requests.len(), 1);
    }
}

#[test]
fn native_seed_refuses_a_reply_with_another_requested_city_set() {
    let (result, requests) = native(
        "demo",
        seed(None, &["gasabo"], None),
        200,
        ok(json!({"plan":seed_plan(vec![seed_city("huye","create",1)])})),
    );
    assert_eq!(result.unwrap_err().kind(), ErrorKind::UnreadableResponse);
    assert_eq!(requests.len(), 1);
}

#[test]
fn native_seed_preserves_exact_bounded_service_refusals_without_backend_text() {
    for (status, code) in [
        (400, "SOLAR_SEED_PROJECT_ROOT_REQUIRED"),
        (400, "SOLAR_SEED_SOURCE_INVALID"),
        (403, "SOLAR_SEED_COMPONENT_DISABLED"),
        (400, "SOLAR_SEED_DIGEST_REQUIRED"),
        (409, "SOLAR_SEED_DIGEST_MISMATCH"),
        (400, "SOLAR_SEED_BOUNDED"),
    ] {
        let (result, requests) = native(
            "demo",
            seed(None, &[], Some(SEED_DIGEST)),
            status,
            json!({"success":false,"error":{"details":{"code":code},"message":"private backend body must never cross"}}),
        );
        let error = result.unwrap_err();
        let refusal = error
            .service_refusal()
            .expect("exact governed seed code must survive");
        assert_eq!(refusal.code(), Some(code.to_ascii_lowercase().as_str()));
        assert_eq!(refusal.status(), status);
        assert_eq!(refusal.message(), None);
        assert!(!error.to_string().contains("private backend"));
        assert_eq!(requests.len(), 1);
    }
}

#[test]
fn native_seed_drops_unknown_codes_and_codes_under_an_unrelated_status() {
    for (status, code) in [
        (400, "UNDECLARED_PRIVATE_CODE"),
        (403, "SOLAR_SEED_SOURCE_INVALID"),
        (500, "SOLAR_SEED_DIGEST_MISMATCH"),
    ] {
        let (result, requests) = native(
            "demo",
            seed(None, &[], Some(SEED_DIGEST)),
            status,
            json!({"success":false,"error":{"details":{"code":code},"message":"private backend body must never cross"}}),
        );
        let error = result.unwrap_err();
        assert!(error.service_refusal().is_none());
        assert!(!error.to_string().contains("private backend"));
        assert_eq!(requests.len(), 1);
    }
}

#[test]
fn native_seed_rejects_an_invalid_explicit_project_before_gateway_io() {
    let (result, requests) = native(
        "../demo",
        seed(None, &[], None),
        200,
        ok(json!({"plan":seed_plan(vec![seed_city("huye","create",1)])})),
    );
    assert_eq!(result.unwrap_err().kind(), ErrorKind::InvalidInput);
    assert!(requests.is_empty());
}

#[test]
fn seed_commands_require_explicit_project_and_refuse_retired_desktop_input() {
    for mut command in [
        vec!["solar", "seed", "preview"],
        vec![
            "solar",
            "seed",
            "apply",
            "--seed-digest",
            SEED_DIGEST,
            "--yes",
        ],
    ] {
        let mut missing = command.clone();
        missing.extend(["--output", "json"]);
        let (reply, code) = ds(&missing);
        assert_eq!(reply["error"]["code"], "missing_input");
        assert_eq!(code, 2);
        command.extend([
            "--project",
            "demo",
            "--desktop-descriptor",
            "/definitely/not/a/bridge.json",
            "--output",
            "json",
        ]);
        let (reply, code) = ds(&command);
        assert_eq!(reply["error"]["code"], "unknown_flag");
        assert_eq!(code, 2);
    }
}

#[test]
fn seed_validates_selection_and_digest_before_native_auth_or_gateway() {
    let ids: Vec<String> = (0..65).map(|i| format!("city-{i}")).collect();
    let mut too_many = vec!["solar", "seed", "preview"];
    for id in &ids {
        too_many.extend(["--city", id]);
    }
    for (mut args, expected) in [
        (too_many, "solar_seed_bounded"),
        (
            vec!["solar", "seed", "preview", "--city", " huye"],
            "invalid_seed_city",
        ),
        (
            vec!["solar", "seed", "preview", "--source="],
            "invalid_seed_source",
        ),
        (
            vec![
                "solar",
                "seed",
                "apply",
                "--seed-digest",
                "sha256:d3d3d3d3",
                "--yes",
            ],
            "solar_seed_digest_required",
        ),
        (
            vec!["solar", "seed", "apply", "--seed-digest=", "--yes"],
            "solar_seed_digest_required",
        ),
    ] {
        args.extend(["--project", "demo", "--output", "json"]);
        let (reply, code) = ds(&args);
        assert_eq!(reply["error"]["code"], expected, "{args:?}: {reply}");
        assert_eq!(code, 2);
    }
}
