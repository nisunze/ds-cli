use std::collections::VecDeque;
use std::sync::{Arc, Barrier, Mutex};

use ds_client_core::{
    probe_stored_refresh, AuthorizedTransport, Client, ClientProfile, ClientProfileInput,
    DeploymentLane, DeviceAccessSession, DeviceBinding, DeviceCredential, DevicePrivateKey,
    ErrorKind, NativeSyncRequest, ProfileError, ProjectFormEditorCall, ProjectFormsCall,
    ProjectListCall, ProjectReportServiceCode, ProjectStatus, RefreshCall, RefreshTokenStore,
    SignInCall, SolarMediaDownloadCall, SolarSnapshotCall, StoreError, StoreKey,
    SurveyEntriesChangesCall, SurveyEntriesChangesRequest, SurveyEntriesChangesServiceCode,
    SurveyEntriesSelectCall, SurveyEntriesSelectRequest, SurveyEntriesSelectServiceCode,
    SurveyEntryCreateCall, SurveyEntryCreateOrigin, SurveyEntryCreateRequest,
    SurveyEntryCreateServiceCode, SurveyFormReadServiceCode, SurveyQueryCall, SurveyQueryFilter,
    SurveyQueryMetric, SurveyQueryOrder, SurveyQueryRequest, SyncGatewayCall, SyncGatewayOperation,
    TileCall, TileOperationStatus, TilePreflightStatus, TileType, TransformerContextCall,
    Transport, TransportError, TransportResponse, WeakNetwork, CLIENT_PROFILE_SCHEMA,
};
use ds_client_core::{
    CompoundedReportRequest, CompoundedReportStatus, ExportReportOutcome, ProjectReportCall,
    ReportFileLevel, RetirementAction, RetirementRefusal, RetirementRequest, TransformerLifecycle,
    TransformerSet,
};
use serde_json::json;
use sha2::Digest;

const NOW: u64 = 1_900_000_000;
const SIGN_IN: &[u8] = include_bytes!("fixtures/firebase-sign-in.json");
const REFRESH: &[u8] = include_bytes!("fixtures/firebase-refresh.json");
const REVOKED: &[u8] = include_bytes!("fixtures/firebase-revoked.json");
const ACTIVE: &[u8] = include_bytes!("fixtures/projects-active.json");
const ARCHIVED: &[u8] = include_bytes!("fixtures/projects-archived.json");
const TESTING: &[u8] = include_bytes!("fixtures/projects-testing.json");
const STYLE_CATALOG_SNAPSHOT: &[u8] = include_bytes!("fixtures/style-catalog-snapshot.json");

fn profile(lane: DeploymentLane) -> ClientProfile {
    ClientProfile::validate(profile_input(lane)).unwrap()
}

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
        tiles_actions: ds_client_core::TILES_ACTIONS
            .iter()
            .map(|action| (*action).to_owned())
            .collect(),
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
        design_attachments_method: "POST".into(),
        design_attachments_path: "/api/v1/design/attachments".into(),
        design_attachments_actions: ds_client_core::DESIGN_ATTACHMENTS_ACTIONS
            .map(str::to_owned)
            .to_vec(),
        design_versions_method: "POST".into(),
        design_versions_path: "/api/v1/design/versions".into(),
        design_versions_actions: ds_client_core::DESIGN_VERSIONS_ACTIONS
            .map(str::to_owned)
            .to_vec(),
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
        sre_overview_method: "GET".to_owned(),
        sre_overview_path: "/api/v1/sre/overview".to_owned(),
        sre_events_method: "POST".to_owned(),
        sre_events_path: "/api/v1/data".to_owned(),
        sre_events_actions: vec!["query_table".to_owned()],
        admin_bounds_method: "GET".to_owned(),
        admin_bounds_path: "/api/v1/admin/rwanda".to_owned(),
        admin_bounds_actions: vec!["children".to_owned(), "geometry".to_owned()],
    }
}

#[test]
fn device_credential_matches_lane_stable_audience_and_uid_not_email_or_release_digest() {
    let stable = profile(DeploymentLane::Stable);
    let approved = DeviceBinding::for_profile(&stable, &"b".repeat(64)).unwrap();
    let credential = DeviceCredential::new(
        "device-1".to_owned(),
        "Workstation".to_owned(),
        "linux".to_owned(),
        format!("sha256:{}", "c".repeat(64)),
        "uid-1".to_owned(),
        "person@example.com".to_owned(),
        &approved,
        "2031-01-01T00:00:00Z".to_owned(),
        DevicePrivateKey::from_secret_bytes([5; 32]),
    )
    .unwrap();
    assert!(credential.matches_authority(&stable, "uid-1"));
    assert!(!credential.matches_authority(&stable, "another-uid"));
    assert!(!credential.matches_authority(&profile(DeploymentLane::Canary), "uid-1"));

    let protected = credential.encode_protected().unwrap();
    assert!(!String::from_utf8_lossy(protected.as_bytes()).contains("access_token"));
    let mut upgraded_input = profile_input(DeploymentLane::Stable);
    upgraded_input.source_revision = "f".repeat(40);
    upgraded_input.descriptor_sha256 = "e".repeat(64);
    upgraded_input.gateway_api_key = "rotated-public-gateway-key".to_owned();
    let upgraded = ClientProfile::validate(upgraded_input).unwrap();
    let restored = DeviceCredential::decode_protected(protected.as_bytes(), &upgraded).unwrap();
    assert!(restored.matches_authority(&upgraded, "uid-1"));
    let current = DeviceBinding::for_profile(&upgraded, &"d".repeat(64)).unwrap();
    assert_ne!(current.profile_digest, approved.profile_digest);
    assert_ne!(current.catalog_digest, approved.catalog_digest);
    let nonce = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        [8_u8; 16],
    );
    let refresh = restored
        .refresh_request(1_900_000_000, &nonce, &current)
        .unwrap();
    assert_eq!(refresh.binding().profile_digest, approved.profile_digest);
    assert_eq!(refresh.binding().catalog_digest, approved.catalog_digest);
}

#[derive(Default)]
struct TransportState {
    asset_bodies: Vec<serde_json::Value>,
    feedback: VecDeque<TransportResponse>,
    feedback_bodies: Vec<serde_json::Value>,
    survey_migration: VecDeque<TransportResponse>,
    survey_migration_bodies: Vec<serde_json::Value>,
    shared_assets: VecDeque<TransportResponse>,
    shared_bytes: VecDeque<TransportResponse>,
    design_tags: VecDeque<TransportResponse>,
    grid_models: VecDeque<TransportResponse>,
    grid_bodies: Vec<serde_json::Value>,
    grid_uploaded: Vec<Vec<u8>>,
    grid_upload_fail: bool,
    grid_bytes: VecDeque<TransportResponse>,
    attachment_bytes: VecDeque<TransportResponse>,
    tag_bodies: Vec<serde_json::Value>,
    configurations: VecDeque<TransportResponse>,
    configuration_bodies: Vec<serde_json::Value>,
    sign_in: VecDeque<TransportResponse>,
    refresh: VecDeque<TransportResponse>,
    projects: VecDeque<TransportResponse>,
    transformer_context: VecDeque<TransportResponse>,
    transformer_analyses: VecDeque<
        Result<
            ds_client_core::TransformerAnalysisResponse,
            ds_client_core::TransformerAnalysisTransportError,
        >,
    >,
    transformer_analysis_bodies: Vec<serde_json::Value>,
    transformer_analysis_authorities: Vec<(String, String, String, Option<String>)>,
    transformer_saves: VecDeque<TransportResponse>,
    transformer_save_bodies: Vec<serde_json::Value>,
    project_forms: VecDeque<TransportResponse>,
    project_form_editor: VecDeque<TransportResponse>,
    solar_snapshot: VecDeque<TransportResponse>,
    solar_media_download: VecDeque<TransportResponse>,
    solar_artifact_download: VecDeque<TransportResponse>,
    solar_project: VecDeque<TransportResponse>,
    survey_query: VecDeque<TransportResponse>,
    survey_entries_select: VecDeque<TransportResponse>,
    survey_entries_changes: VecDeque<TransportResponse>,
    survey_entry_create: VecDeque<TransportResponse>,
    project_data: VecDeque<TransportResponse>,
    status_processing: VecDeque<TransportResponse>,
    survey_photo: VecDeque<TransportResponse>,
    photo_downloads: VecDeque<TransportResponse>,
    photo_bodies: Vec<serde_json::Value>,
    uploaded_bytes: Vec<Vec<u8>>,
    uploads: VecDeque<TransportResponse>,
    survey_control: VecDeque<TransportResponse>,
    styles: VecDeque<TransportResponse>,
    layers: VecDeque<TransportResponse>,
    tiles: VecDeque<TransportResponse>,
    project_report: VecDeque<TransportResponse>,
    design_selections: VecDeque<TransportResponse>,
    design_versions: VecDeque<TransportResponse>,
    design_attachments: VecDeque<TransportResponse>,
    sync_gateway: VecDeque<TransportResponse>,
    governance: VecDeque<TransportResponse>,
    device_approve: VecDeque<TransportResponse>,
    project_properties: VecDeque<TransportResponse>,
    calls: Vec<String>,
    correlated_project_requests: usize,
    correlated_transformer_requests: usize,
    correlated_project_forms_requests: usize,
    correlated_solar_snapshot_requests: usize,
    correlated_survey_query_requests: usize,
    survey_query_projects: Vec<String>,
    correlated_survey_entries_select_requests: usize,
    correlated_survey_entries_changes_requests: usize,
    correlated_survey_entry_create_requests: usize,
    correlated_tile_requests: usize,
    correlated_project_report_requests: usize,
    /// Sign-in and refresh attempts that drop before an answer, as a weak link.
    sign_in_drops: usize,
    refresh_drops: usize,
}

#[derive(Clone, Default)]
struct FixtureTransport(Arc<Mutex<TransportState>>);

impl FixtureTransport {
    fn with_sign_in(body: &[u8]) -> Self {
        let transport = Self::default();
        transport
            .0
            .lock()
            .unwrap()
            .sign_in
            .push_back(TransportResponse::new(200, body));
        transport
    }

    fn with_rejected_sign_in(status: u16, message: &str) -> Self {
        let transport = Self::default();
        let body = serde_json::to_vec(&serde_json::json!({
            "error": { "message": message, "private_detail": "must not escape" }
        }))
        .unwrap();
        transport
            .0
            .lock()
            .unwrap()
            .sign_in
            .push_back(TransportResponse::new(status, body));
        transport
    }

    fn push_refresh(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .refresh
            .push_back(TransportResponse::new(status, body));
    }

    /// Drop the next `sign_in` and `refresh` attempts before any answer.
    fn drop_next(&self, sign_in: usize, refresh: usize) {
        let mut state = self.0.lock().unwrap();
        state.sign_in_drops = sign_in;
        state.refresh_drops = refresh;
    }

    fn count_calls(&self, prefix: &str) -> usize {
        self.calls()
            .iter()
            .filter(|call| call.starts_with(prefix))
            .count()
    }

    fn push_project(&self, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .projects
            .push_back(TransportResponse::new(200, body));
    }

    fn push_transformer_context(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .transformer_context
            .push_back(TransportResponse::new(status, body));
    }

    fn push_project_forms(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .project_forms
            .push_back(TransportResponse::new(status, body));
    }

    fn push_project_form_editor(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .project_form_editor
            .push_back(TransportResponse::new(status, body));
    }

    fn push_solar_snapshot(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .solar_snapshot
            .push_back(TransportResponse::new(status, body));
    }

    fn push_survey_query(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .survey_query
            .push_back(TransportResponse::new(status, body));
    }

    fn push_survey_entries_select(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .survey_entries_select
            .push_back(TransportResponse::new(status, body));
    }

    fn push_survey_entries_changes(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .survey_entries_changes
            .push_back(TransportResponse::new(status, body));
    }

    fn push_survey_entry_create(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .survey_entry_create
            .push_back(TransportResponse::new(status, body));
    }

    fn push_tiles(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .tiles
            .push_back(TransportResponse::new(status, body));
    }

    fn push_project_report(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .project_report
            .push_back(TransportResponse::new(status, body));
    }

    fn push_design_versions(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .design_versions
            .push_back(TransportResponse::new(status, body));
    }
    fn push_design_attachments(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .design_attachments
            .push_back(TransportResponse::new(status, body));
    }

    fn push_design_selections(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .design_selections
            .push_back(TransportResponse::new(status, body));
    }

    fn push_sync_gateway(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .sync_gateway
            .push_back(TransportResponse::new(status, body));
    }

    fn push_device_approve(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .device_approve
            .push_back(TransportResponse::new(status, body));
    }

    /// One queue for the five projectless governance doors: a test pushes
    /// answers in the order it will knock.
    fn push_governance(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .governance
            .push_back(TransportResponse::new(status, body));
    }

    fn push_project_properties(&self, status: u16, body: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .project_properties
            .push_back(TransportResponse::new(status, body));
    }

    fn calls(&self) -> Vec<String> {
        self.0.lock().unwrap().calls.clone()
    }

    fn correlated_project_requests(&self) -> usize {
        self.0.lock().unwrap().correlated_project_requests
    }

    fn correlated_transformer_requests(&self) -> usize {
        self.0.lock().unwrap().correlated_transformer_requests
    }
}

impl Transport for FixtureTransport {
    fn feedback(
        &mut self,
        call: ds_client_core::FeedbackCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/feedback");
        assert_eq!(call.method(), "POST");
        assert!(!call.bearer_token().is_empty());
        let mut state = self.0.lock().unwrap();
        state
            .feedback_bodies
            .push(serde_json::from_slice(call.body().as_bytes()).unwrap());
        Ok(state.feedback.pop_front().unwrap())
    }
    fn survey_migration(
        &mut self,
        call: ds_client_core::SurveyMigrationCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/pipeline");
        assert_eq!(call.method(), "POST");
        assert_eq!(call.timeout_seconds(), 600);
        assert_eq!(call.response_limit(), 1024 * 1024);
        assert!(!call.bearer_token().is_empty());
        let mut state = self.0.lock().unwrap();
        state
            .survey_migration_bodies
            .push(serde_json::from_slice(call.body().as_bytes()).unwrap());
        Ok(state.survey_migration.pop_front().unwrap())
    }
    fn shared_assets(
        &mut self,
        call: ds_client_core::SharedAssetsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/assets");
        assert_eq!(call.method(), "POST");
        assert!(!call.bearer_token().is_empty());
        let mut state = self.0.lock().unwrap();
        state
            .asset_bodies
            .push(serde_json::from_slice(call.body().as_bytes()).unwrap());
        Ok(state.shared_assets.pop_front().unwrap())
    }
    fn shared_asset_bytes(
        &mut self,
        call: ds_client_core::shared_assets::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert!(call.url().starts_with("https://storage.googleapis.com/"));
        assert!(call.response_limit() <= 32 * 1024 * 1024);
        Ok(self.0.lock().unwrap().shared_bytes.pop_front().unwrap())
    }
    fn design_tags(
        &mut self,
        call: ds_client_core::DesignTagsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/design/annotations");
        assert_eq!(call.method(), "POST");
        assert!(!call.bearer_token().is_empty());
        let mut s = self.0.lock().unwrap();
        s.tag_bodies
            .push(serde_json::from_slice(call.body().as_bytes()).unwrap());
        Ok(s.design_tags.pop_front().unwrap())
    }
    fn grid_models(
        &mut self,
        call: ds_client_core::GridModelsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/grid/models");
        assert_eq!(call.method(), "POST");
        assert!(!call.bearer_token().is_empty());
        let mut state = self.0.lock().unwrap();
        state
            .grid_bodies
            .push(serde_json::from_slice(call.body().as_bytes()).unwrap());
        Ok(state
            .grid_models
            .pop_front()
            .expect("no unplanned project request"))
    }
    fn grid_model_upload(
        &mut self,
        call: ds_client_core::grid_models::publication::UploadCall<'_>,
    ) -> Result<(), TransportError> {
        use sha2::{Digest, Sha256};
        assert!(call.uri().starts_with("https://storage.googleapis.com/"));
        assert_eq!(format!("{:x}", Sha256::digest(call.bytes())), call.digest());
        let mut state = self.0.lock().unwrap();
        state.grid_uploaded.push(call.bytes().to_vec());
        if state.grid_upload_fail {
            Err(TransportError::Unreachable)
        } else {
            Ok(())
        }
    }
    fn grid_model_bytes(
        &mut self,
        call: ds_client_core::grid_models::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert!(call.url().starts_with("https://storage.googleapis.com/"));
        assert_eq!(call.response_limit(), 5);
        Ok(self.0.lock().unwrap().grid_bytes.pop_front().unwrap())
    }
    fn design_attachment_bytes(
        &mut self,
        call: ds_client_core::design_attachments::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert!(call.url().starts_with("https://storage.googleapis.com/"));
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!(
            "attachment_bytes url={} limit={}",
            call.url(),
            call.response_limit()
        ));
        Ok(state.attachment_bytes.pop_front().unwrap())
    }

    fn device_approve(
        &mut self,
        call: ds_client_core::DeviceApproveCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/auth/device/approve");
        assert_eq!(call.method(), "POST");
        assert_eq!(call.content_type(), "application/json");
        assert_eq!(call.timeout_seconds(), 30);
        assert_eq!(call.response_limit(), 1024 * 1024);
        assert_eq!(
            call.correlation_headers(),
            ["X-Request-Id", "X-DS-Action-Id"]
        );
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!(
            "device_approve origin={} api_key={} user_id_token={} device={} email={} body={}",
            call.gateway_origin(),
            !call.gateway_api_key().is_empty(),
            call.bearer_token().starts_with("e30."),
            call.device_id().is_some(),
            call.canonical_email(),
            String::from_utf8_lossy(call.body().as_bytes()),
        ));
        Ok(state
            .device_approve
            .pop_front()
            .expect("device approve answer"))
    }
    fn installs(
        &mut self,
        call: ds_client_core::InstallsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/governance");
        assert_eq!(call.method(), "POST");
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!(
            "installs body={}",
            String::from_utf8_lossy(call.body().as_bytes())
        ));
        Ok(state.governance.pop_front().expect("governance answer"))
    }
    fn project_properties(
        &mut self,
        call: ds_client_core::ProjectPropertiesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/projects");
        assert_eq!(call.method(), "POST");
        assert_eq!(call.content_type(), "application/json");
        assert_eq!(call.timeout_seconds(), 60);
        assert_eq!(call.response_limit(), 256 * 1024);
        assert_eq!(
            call.correlation_headers(),
            ["X-Request-Id", "X-DS-Action-Id"]
        );
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!(
            "project_properties origin={} api_key={} user_id_token={} device={} email={} body={}",
            call.gateway_origin(),
            !call.gateway_api_key().is_empty(),
            call.bearer_token().starts_with("e30."),
            call.device_id().is_some(),
            call.canonical_email(),
            String::from_utf8_lossy(call.body().as_bytes()),
        ));
        Ok(state
            .project_properties
            .pop_front()
            .expect("project properties answer"))
    }
    fn sre_overview(
        &mut self,
        call: ds_client_core::SreOverviewCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/sre/overview");
        assert_eq!(call.method(), "GET");
        let mut state = self.0.lock().unwrap();
        state.calls.push("sre_overview".to_owned());
        Ok(state.governance.pop_front().expect("governance answer"))
    }
    fn admin_bounds(
        &mut self,
        call: ds_client_core::AdminBoundsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/admin/rwanda");
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!("admin_bounds url={}", call.url()));
        Ok(state.governance.pop_front().expect("governance answer"))
    }
    fn grid_catalog(
        &mut self,
        call: ds_client_core::GridCatalogCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/grid/catalog");
        assert_eq!(call.method(), "POST");
        assert_eq!(call.response_limit(), 1024 * 1024);
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!(
            "grid_catalog body={}",
            String::from_utf8_lossy(call.body().as_bytes())
        ));
        Ok(state.governance.pop_front().expect("governance answer"))
    }
    fn global_tiles(
        &mut self,
        call: ds_client_core::GlobalTilesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/tiles");
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!(
            "global_tiles body={}",
            String::from_utf8_lossy(call.body().as_bytes())
        ));
        Ok(state.governance.pop_front().expect("governance answer"))
    }

    fn sync_gateway(
        &mut self,
        call: SyncGatewayCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let body = call.body();
        let body: serde_json::Value = serde_json::from_slice(body.as_bytes()).unwrap();
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!(
            "sync_gateway method={} path={} bearer={} device_bearer={} body={body}",
            call.method(),
            call.path(),
            !call.bearer_token().is_empty(),
            call.bearer_token() == "ds-memory-access-jwt",
        ));
        Ok(state
            .sync_gateway
            .pop_front()
            .expect("unexpected sync gateway IO"))
    }

    fn project_configuration(
        &mut self,
        call: ds_client_core::ProjectConfigurationCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        assert!(!call.bearer_token().is_empty());
        assert_eq!(
            call.gateway_origin(),
            profile(DeploymentLane::Stable).gateway_origin()
        );
        state
            .calls
            .push(format!("config {} {}", call.method(), call.path()));
        if call.method() == "POST" {
            state
                .configuration_bodies
                .push(serde_json::from_slice(call.body().as_bytes()).unwrap());
        }
        Ok(state
            .configurations
            .pop_front()
            .expect("unexpected config IO"))
    }
    fn sign_in(&mut self, call: SignInCall<'_>) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let bytes = body.as_bytes();
        let has = |needle: &[u8]| bytes.windows(needle.len()).any(|window| window == needle);
        state.calls.push(format!(
            "sign_in endpoint={} content_type={} limit={} api_key={} email={} password={}",
            call.endpoint(),
            call.content_type(),
            call.response_limit(),
            !call.firebase_api_key().is_empty(),
            has(b"\"email\":") && !call.email().is_empty(),
            has(b"\"password\":") && has(b"\"returnSecureToken\":true")
        ));
        if state.sign_in_drops > 0 {
            state.sign_in_drops -= 1;
            return Err(TransportError::TimedOut);
        }
        Ok(state.sign_in.pop_front().unwrap())
    }

    fn refresh(&mut self, call: RefreshCall<'_>) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let fields = url::form_urlencoded::parse(body.as_bytes()).collect::<Vec<_>>();
        state.calls.push(format!(
            "refresh endpoint={} content_type={} limit={} api_key={} grant_type=refresh_token refresh_token={}",
            call.endpoint(),
            call.content_type(),
            call.response_limit(),
            !call.firebase_api_key().is_empty(),
            fields
                .iter()
                .any(|(key, value)| key == "refresh_token" && !value.is_empty())
                && fields.iter().any(|(key, value)| key == "grant_type" && value == "refresh_token")
        ));
        if state.refresh_drops > 0 {
            state.refresh_drops -= 1;
            return Err(TransportError::Unreachable);
        }
        Ok(state.refresh.pop_front().unwrap())
    }

    fn transformer_analysis(
        &mut self,
        call: ds_client_core::TransformerAnalysisCall<'_>,
    ) -> Result<
        ds_client_core::TransformerAnalysisResponse,
        ds_client_core::TransformerAnalysisTransportError,
    > {
        assert_eq!(call.method(), "POST");
        assert_eq!(call.path(), "/api/v1/data");
        assert_eq!(call.content_type(), "application/json");
        assert_eq!(call.response_limit(), 32 * 1024 * 1024);
        assert_eq!(call.timeout_seconds(), 120);
        assert_eq!(
            call.correlation_headers(),
            ["X-Request-Id", "X-DS-Action-Id"]
        );
        assert_eq!(
            call.gateway_origin(),
            profile(DeploymentLane::Stable).gateway_origin()
        );
        assert_eq!(call.gateway_api_key(), "public-gateway-key");
        assert_eq!(call.client_id(), "ds-native-client");
        let mut state = self.0.lock().unwrap();
        state
            .transformer_analysis_bodies
            .push(serde_json::from_slice(call.body().as_bytes()).unwrap());
        state.transformer_analysis_authorities.push((
            call.bearer_token().to_owned(),
            call.canonical_uid().to_owned(),
            call.canonical_email().to_owned(),
            call.device_id().map(str::to_owned),
        ));
        state
            .transformer_analyses
            .pop_front()
            .expect("one analysis answer per call")
    }

    fn list_projects(
        &mut self,
        call: ds_client_core::ProjectListCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let sequence = state.correlated_project_requests + 1;
        let request_id = format!("test-request-{sequence}");
        let action_id = format!("test-action-{sequence}");
        let correlation_headers = call.correlation_headers();
        let correlation_valid = correlation_headers == ["X-Request-Id", "X-DS-Action-Id"]
            && request_id.len() <= ds_client_core::CORRELATION_ID_MAX_BYTES
            && action_id.len() <= ds_client_core::CORRELATION_ID_MAX_BYTES;
        if correlation_valid {
            state.correlated_project_requests += 1;
        }
        state.calls.push(format!(
            "projects method={} content_type={} url={}{}?status={} limit={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.content_type(),
            call.gateway_origin(),
            call.path(),
            call.status().query_value(),
            call.response_limit(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid
        ));
        Ok(state.projects.pop_front().unwrap())
    }

    fn transformer_save(
        &mut self,
        call: ds_client_core::TransformerSaveCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/data");
        assert_eq!(call.method(), "POST");
        assert!(!call.bearer_token().is_empty());
        let mut state = self.0.lock().unwrap();
        state
            .transformer_save_bodies
            .push(serde_json::from_slice(call.body().as_bytes()).unwrap());
        Ok(state
            .transformer_saves
            .pop_front()
            .expect("unexpected transformer save"))
    }

    fn transformer_context(
        &mut self,
        call: TransformerContextCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let exact_body = serde_json::json!({
            "action": "get_transformers_data",
            "eds_project_id": "active-project",
            "transformers": ["tx_1"],
            "fields": call.projection().token(),
        });
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        if correlation_valid {
            state.correlated_transformer_requests += 1;
        }
        state.calls.push(format!(
            "transformer_context method={} origin={} path={} content_type={} projection={} body_exact={} lane_absent={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            call.projection().token(),
            serde_json::from_slice::<serde_json::Value>(body.as_bytes()).unwrap() == exact_body,
            !body.as_bytes().windows(b"\"lane\"".len()).any(|window| window == b"\"lane\""),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.transformer_context.pop_front().unwrap())
    }

    fn project_forms(
        &mut self,
        call: ProjectFormsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let exact_body = br#"{"action":"activate","project_id":"active-project"}"#;
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        if correlation_valid {
            state.correlated_project_forms_requests += 1;
        }
        state.calls.push(format!(
            "project_forms method={} origin={} path={} content_type={} body_exact={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            serde_json::from_slice::<serde_json::Value>(body.as_bytes()).unwrap() == serde_json::from_slice::<serde_json::Value>(exact_body).unwrap(),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.project_forms.pop_front().unwrap())
    }

    fn project_form_editor(
        &mut self,
        call: ProjectFormEditorCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let exact_body =
            br#"{"action":"settings_editor","project_id":"active-project","form_slug":"a_poles"}"#;
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        state.calls.push(format!(
            "project_form_editor method={} origin={} path={} content_type={} body_exact={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            serde_json::from_slice::<serde_json::Value>(body.as_bytes()).unwrap() == serde_json::from_slice::<serde_json::Value>(exact_body).unwrap(),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.project_form_editor.pop_front().unwrap())
    }

    fn download_solar_artifact(
        &mut self,
        call: ds_client_core::SolarArtifactDownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(
            call.url(),
            "https://storage.googleapis.com/solar-results/result?generation=17&token=private"
        );
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!(
            "solar_artifact_download limit={}",
            call.response_limit()
        ));
        Ok(state
            .solar_artifact_download
            .pop_front()
            .expect("governed artifact response"))
    }

    fn download_solar_media(
        &mut self,
        call: SolarMediaDownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        assert_eq!(
            call.url(),
            "https://storage.googleapis.com/solar-media/signed?token=private"
        );
        state.calls.push(format!(
            "solar_media_download limit={}",
            call.response_limit()
        ));
        Ok(state
            .solar_media_download
            .pop_front()
            .expect("governed media response"))
    }

    fn solar_snapshot(
        &mut self,
        call: SolarSnapshotCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let exact_body = br#"{"action":"desktop_snapshot","root":"eds_project/active-project/eds_solar","template_id":"city_1"}"#;
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        if correlation_valid {
            state.correlated_solar_snapshot_requests += 1;
        }
        state.calls.push(format!(
            "solar_snapshot method={} origin={} path={} content_type={} body_exact={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            serde_json::from_slice::<serde_json::Value>(body.as_bytes()).unwrap() == serde_json::from_slice::<serde_json::Value>(exact_body).unwrap(),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.solar_snapshot.pop_front().unwrap())
    }

    fn survey_query(
        &mut self,
        call: SurveyQueryCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let project = serde_json::from_slice::<serde_json::Value>(body.as_bytes()).unwrap()
            ["ds_project"]
            .as_str()
            .unwrap()
            .to_owned();
        state.survey_query_projects.push(project);
        let exact_body = br#"{"ds_project":"active-project","form":"a_poles","metric":"count_distinct","distinct_field":"created_by","group_by":["created_by"],"filters":[{"field":"created_by","op":"eq","value":"user@example.com"}],"order":"desc","limit":20}"#;
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        if correlation_valid {
            state.correlated_survey_query_requests += 1;
        }
        state.calls.push(format!(
            "survey_query method={} origin={} path={} content_type={} body_exact={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            serde_json::from_slice::<serde_json::Value>(body.as_bytes()).unwrap() == serde_json::from_slice::<serde_json::Value>(exact_body).unwrap(),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.survey_query.pop_front().unwrap())
    }

    fn survey_entries_select(
        &mut self,
        call: SurveyEntriesSelectCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let exact_body = br#"{"ds_project":"project_a","form":"survey_points","bbox":[29.0,-2.0,30.0,-1.0],"limit":2}"#;
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        if correlation_valid {
            state.correlated_survey_entries_select_requests += 1;
        }
        state.calls.push(format!(
            "survey_entries_select method={} origin={} path={} content_type={} body_exact={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            serde_json::from_slice::<serde_json::Value>(body.as_bytes()).unwrap() == serde_json::from_slice::<serde_json::Value>(exact_body).unwrap(),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.survey_entries_select.pop_front().unwrap())
    }

    fn survey_entries_changes(
        &mut self,
        call: SurveyEntriesChangesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let exact_body = br#"{"ds_project":"project_a","form":"survey_points","updated_after":"2026-08-30T00:00:00Z","limit":2}"#;
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        if correlation_valid {
            state.correlated_survey_entries_changes_requests += 1;
        }
        state.calls.push(format!(
            "survey_entries_changes method={} origin={} path={} content_type={} body_exact={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            serde_json::from_slice::<serde_json::Value>(body.as_bytes()).unwrap() == serde_json::from_slice::<serde_json::Value>(exact_body).unwrap(),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.survey_entries_changes.pop_front().unwrap())
    }

    fn survey_entry_create(
        &mut self,
        call: SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let exact_body = br#"{"project_id":"project_a","form_id":"survey_points","doc_id":"entry_1","operation":"create","idempotency_key":"outbox-1","data":{"name":"Pole 1"},"context_key":"parents:parent%3A1","geometry":{"coordinates":[30.0,-1.0],"type":"Point"},"connectivity":{"precedent":"p0"},"detailed_location":{"district":"Huye"},"metadata":{"created_at":"2026-08-30T00:00:00Z","origin":"unknown"}}"#;
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        if correlation_valid {
            state.correlated_survey_entry_create_requests += 1;
        }
        state.calls.push(format!(
            "survey_entry_create method={} origin={} path={} content_type={} body_exact={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            serde_json::from_slice::<serde_json::Value>(body.as_bytes()).unwrap() == serde_json::from_slice::<serde_json::Value>(exact_body).unwrap(),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.survey_entry_create.pop_front().unwrap())
    }

    fn project_data(
        &mut self,
        call: ds_client_core::ProjectDataCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        state.calls.push(format!(
            "project_data method={} origin={} path={} content_type={} body={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            String::from_utf8_lossy(body.as_bytes()),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.project_data.pop_front().unwrap())
    }

    fn survey_photo(
        &mut self,
        call: ds_client_core::SurveyPhotoCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.path(), "/api/v1/media");
        let mut state = self.0.lock().unwrap();
        state
            .photo_bodies
            .push(serde_json::from_slice(call.body().as_bytes()).unwrap());
        Ok(state
            .survey_photo
            .pop_front()
            .expect("unexpected photo call"))
    }
    fn download_survey_photo(
        &mut self,
        call: ds_client_core::survey_photo::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert!(call.url().contains("generation=7"));
        assert!(call.response_limit() <= 50 * 1024 * 1024);
        Ok(self
            .0
            .lock()
            .unwrap()
            .photo_downloads
            .pop_front()
            .expect("unexpected photo download"))
    }
    fn status_processing(
        &mut self,
        call: ds_client_core::StatusProcessingCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        state.calls.push(format!(
            "status_processing method={} path={} body={} limit={} timeout={} correlation={}",
            call.method(),
            call.path(),
            String::from_utf8_lossy(body.as_bytes()),
            call.response_limit(),
            call.timeout_seconds(),
            call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"],
        ));
        Ok(state.status_processing.pop_front().unwrap())
    }

    fn upload_bytes(
        &mut self,
        mut call: ds_client_core::UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let size = call.size();
        let mut bytes = vec![];
        call.reader().read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes.len() as u64, size);
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!("upload bytes={size}"));
        state.uploaded_bytes.push(bytes);
        Ok(state.uploads.pop_front().expect("unexpected byte upload"))
    }
    fn solar_reference(
        &mut self,
        call: ds_client_core::SolarReferenceCall<'_>,
    ) -> ds_solar_contracts::SolarResult<ds_solar_contracts::BundleBytes> {
        assert!(call.gateway_origin().ends_with(".gateway.dev"));
        assert_eq!(call.gateway_api_key(), "public-gateway-key");
        self.0
            .lock()
            .unwrap()
            .calls
            .push(format!("solar-reference:{}", call.request().city));
        Ok(ds_solar_contracts::BundleBytes {
            weather_json: b"weather".to_vec(),
            reference_unit_json: b"unit".to_vec(),
            reference_unit_parquet: b"parquet".to_vec(),
            manifest_json: b"manifest".to_vec(),
        })
    }
    fn solar_project(
        &mut self,
        call: ds_client_core::SolarProjectCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.method(), "POST");
        assert!(matches!(
            call.path().as_str(),
            "/api/v1/solar" | "/api/v1/compute_artifacts"
        ));
        let body: serde_json::Value = serde_json::from_slice(call.body().as_bytes()).unwrap();
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!(
            "solar-project:{}",
            body["action"].as_str().unwrap()
        ));
        state
            .solar_project
            .pop_front()
            .ok_or(TransportError::Unreachable)
    }
    fn survey_control(
        &mut self,
        call: ds_client_core::SurveyControlCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        state.calls.push(format!(
            "survey_control method={} origin={} path={} content_type={} body={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            String::from_utf8_lossy(body.as_bytes()),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.survey_control.pop_front().unwrap())
    }

    fn styles(
        &mut self,
        call: ds_client_core::StylesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        state.calls.push(format!(
            "styles method={} origin={} path={} content_type={} body={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            String::from_utf8_lossy(body.as_bytes()),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.styles.pop_front().unwrap())
    }

    fn printing(
        &mut self,
        call: ds_client_core::PrintingCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        state.calls.push(format!(
            "printing method={} origin={} path={} content_type={} body={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            String::from_utf8_lossy(body.as_bytes()),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.layers.pop_front().unwrap())
    }

    fn design_versions(
        &mut self,
        call: ds_client_core::DesignVersionsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        state.calls.push(format!(
            "design_versions method={} origin={} path={} content_type={} body={} limit={} api_key={} authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            String::from_utf8_lossy(body.as_bytes()),
            call.response_limit(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.design_versions.pop_front().unwrap())
    }
    fn design_attachments(
        &mut self,
        call: ds_client_core::DesignAttachmentsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        state.calls.push(format!(
            "design_attachments method={} origin={} path={} content_type={} body={} limit={} api_key={} authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            String::from_utf8_lossy(body.as_bytes()),
            call.response_limit(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.design_attachments.pop_front().unwrap())
    }
    fn design_attachment_upload(
        &mut self,
        call: ds_client_core::design_attachments::UploadCall<'_>,
    ) -> Result<(), TransportError> {
        self.0.lock().unwrap().calls.push(format!(
            "attachment_upload bytes={} digest={}",
            call.bytes().len(),
            call.digest()
        ));
        Ok(())
    }

    fn design_selections(
        &mut self,
        call: ds_client_core::DesignSelectionsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        state.calls.push(format!(
            "design_selections method={} origin={} path={} action={} content_type={} body={} limit={} api_key={} authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.action(),
            call.content_type(),
            String::from_utf8_lossy(body.as_bytes()),
            call.response_limit(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.design_selections.pop_front().unwrap())
    }

    fn layers(
        &mut self,
        call: ds_client_core::LayersCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        state.calls.push(format!(
            "layers method={} origin={} path={} content_type={} body={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            String::from_utf8_lossy(body.as_bytes()),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.layers.pop_front().unwrap())
    }

    fn tiles(&mut self, call: TileCall<'_>) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        if correlation_valid {
            state.correlated_tile_requests += 1;
        }
        state.calls.push(format!(
            "tiles method={} origin={} path={} content_type={} body={} limit={} timeout={} api_key={} authorization={} forwarded_authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.content_type(),
            String::from_utf8_lossy(body.as_bytes()),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.tiles.pop_front().unwrap())
    }

    fn project_report(
        &mut self,
        call: ProjectReportCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut state = self.0.lock().unwrap();
        let body = call.body();
        let correlation_valid = call.correlation_headers() == ["X-Request-Id", "X-DS-Action-Id"];
        if correlation_valid {
            state.correlated_project_report_requests += 1;
        }
        state.calls.push(format!(
            "project_report method={} origin={} path={} action={} lane={} content_type={} body={} limit={} timeout={} api_key={} authorization={} email={} client_id={} correlation={}",
            call.method(),
            call.gateway_origin(),
            call.path(),
            call.action(),
            call.processing_lane().unwrap_or("none"),
            call.content_type(),
            String::from_utf8_lossy(body.as_bytes()),
            call.response_limit(),
            call.timeout_seconds(),
            !call.gateway_api_key().is_empty(),
            !call.bearer_token().is_empty(),
            !call.canonical_email().is_empty(),
            call.client_id(),
            correlation_valid,
        ));
        Ok(state.project_report.pop_front().unwrap())
    }
}

#[derive(Default)]
struct StoreState {
    value: Option<Vec<u8>>,
    conflict_replacement: Option<Vec<u8>>,
    writes: Vec<Option<Vec<u8>>>,
    keys: Vec<String>,
    leased: bool,
    deletion_error: Option<StoreError>,
    release_error: Option<StoreError>,
}

#[derive(Clone, Default)]
struct MemoryStore(Arc<Mutex<StoreState>>);

impl MemoryStore {
    fn value(&self) -> Option<Vec<u8>> {
        self.0.lock().unwrap().value.clone()
    }

    fn conflict_with(&self, replacement: Vec<u8>) {
        self.0.lock().unwrap().conflict_replacement = Some(replacement);
    }

    fn fail_next_deletion(&self, error: StoreError) {
        self.0.lock().unwrap().deletion_error = Some(error);
    }

    fn fail_next_release(&self, error: StoreError) {
        self.0.lock().unwrap().release_error = Some(error);
    }

    fn last_key(&self) -> String {
        self.0.lock().unwrap().keys.last().unwrap().clone()
    }
}

impl RefreshTokenStore for MemoryStore {
    fn acquire(&mut self, key: &StoreKey) -> Result<(), StoreError> {
        loop {
            let mut state = self.0.lock().unwrap();
            if !state.leased {
                state.leased = true;
                state.keys.push(key.as_str().to_owned());
                return Ok(());
            }
            drop(state);
            std::thread::yield_now();
        }
    }

    fn load(&mut self, key: &StoreKey) -> Result<Option<Vec<u8>>, StoreError> {
        self.0.lock().unwrap().keys.push(key.as_str().to_owned());
        Ok(self.value())
    }

    fn compare_and_swap(
        &mut self,
        key: &StoreKey,
        expected: Option<&[u8]>,
        replacement: Option<&[u8]>,
    ) -> Result<(), StoreError> {
        let mut state = self.0.lock().unwrap();
        state.keys.push(key.as_str().to_owned());
        if replacement.is_none() {
            if let Some(error) = state.deletion_error.take() {
                return Err(error);
            }
        }
        if let Some(newer) = state.conflict_replacement.take() {
            state.value = Some(newer);
            return Err(StoreError::Conflict);
        }
        if state.value.as_deref() != expected {
            return Err(StoreError::Conflict);
        }
        let replacement = replacement.map(<[u8]>::to_vec);
        state.value = replacement.clone();
        state.writes.push(replacement);
        Ok(())
    }

    fn release(&mut self, key: &StoreKey) -> Result<(), StoreError> {
        let mut state = self.0.lock().unwrap();
        state.keys.push(key.as_str().to_owned());
        assert!(state.leased);
        state.leased = false;
        match state.release_error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// The field retry schedule with its pauses removed: the same attempts, no wait.
const IMMEDIATE: WeakNetwork = WeakNetwork::new(&[std::time::Duration::ZERO; 3]);

fn signed_in(
    transport: FixtureTransport,
    store: MemoryStore,
) -> Client<FixtureTransport, MemoryStore> {
    let mut client =
        Client::new(profile(DeploymentLane::Stable), transport, store).with_weak_network(IMMEDIATE);
    client
        .sign_in("user@example.com", "password-from-protected-input", NOW)
        .unwrap();
    client
}

#[test]
fn sign_in_survives_one_dropout() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.drop_next(1, 0);
    let store = MemoryStore::default();
    let mut client = Client::new(
        profile(DeploymentLane::Stable),
        transport.clone(),
        store.clone(),
    )
    .with_weak_network(IMMEDIATE);
    let user = client
        .sign_in("user@example.com", "password-from-protected-input", NOW)
        .unwrap();
    assert_eq!(user.uid(), "uid-1");
    assert_eq!(transport.count_calls("sign_in "), 2);
    assert!(store.value().is_some());
}

#[test]
fn sign_in_that_drops_on_every_retry_is_transient_and_says_it_retried() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.drop_next(4, 0);
    let store = MemoryStore::default();
    let mut client = Client::new(
        profile(DeploymentLane::Stable),
        transport.clone(),
        store.clone(),
    )
    .with_weak_network(IMMEDIATE);
    let error = client
        .sign_in("user@example.com", "password-from-protected-input", NOW)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Transient);
    assert_eq!(error.retries(), 3);
    assert!(error.to_string().ends_with("(retried 3 times)"), "{error}");
    assert_eq!(transport.count_calls("sign_in "), 4);
    assert!(store.value().is_none());
    assert!(client.user().is_none());
}

#[test]
fn a_refused_or_throttled_sign_in_is_answered_once() {
    for (status, message, kind) in [
        (
            400,
            "INVALID_LOGIN_CREDENTIALS",
            ErrorKind::InvalidCredentials,
        ),
        (400, "USER_DISABLED", ErrorKind::AccountDisabled),
        // Firebase's password throttle: another attempt is what it refuses.
        (429, "TOO_MANY_ATTEMPTS_TRY_LATER", ErrorKind::Transient),
    ] {
        let transport = FixtureTransport::with_rejected_sign_in(status, message);
        let mut client = Client::new(
            profile(DeploymentLane::Stable),
            transport.clone(),
            MemoryStore::default(),
        )
        .with_weak_network(IMMEDIATE);
        let error = client
            .sign_in("user@example.com", "protected", NOW)
            .unwrap_err();
        assert_eq!(error.kind(), kind, "{message}");
        assert_eq!(error.retries(), 0, "{message}");
        assert_eq!(transport.count_calls("sign_in "), 1, "{message}");
    }
}

#[test]
fn token_refresh_survives_one_dropout() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let store = MemoryStore::default();
    let mut client = signed_in(transport.clone(), store.clone());
    transport.drop_next(0, 1);
    transport.push_refresh(200, REFRESH);
    client.refresh(NOW + 3_001).unwrap();
    assert_eq!(transport.count_calls("refresh "), 2);
    assert_eq!(client.user().unwrap().uid(), "uid-1");
}

#[test]
fn a_revoked_refresh_is_answered_once() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let store = MemoryStore::default();
    let mut client = signed_in(transport.clone(), store.clone());
    transport.push_refresh(400, REVOKED);
    let error = client.refresh(NOW + 3_001).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::PermanentlyRevoked);
    assert_eq!(error.retries(), 0);
    assert_eq!(transport.count_calls("refresh "), 1);
}

#[test]
fn closed_sync_gateway_executes_record_reads_and_report_publication_through_the_transport() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    for _ in 0..3 {
        transport.push_sync_gateway(200, br#"{"data":{}}"#);
    }
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let record = serde_json::json!({
        "action": "list",
        "project_id": "project-a",
        "limit": 200,
    });
    let open = serde_json::json!({
        "action": "open", "project_id": "project-a",
        "client_run_id": "publish-76ab72a6-6b1d-4095-a54e-139232b5152c",
        "engine": "network_reporter", "operation": "export-tx-a", "variant": "default",
        "engine_version": "ds-network-reporter@0.1.0+development",
        "engine_build_manifest_sha256": "5".repeat(64),
        "input_base_fingerprint": "b".repeat(64),
        "outputs": [{"output_id": "xlsx", "format": "xlsx",
            "content_type": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "sha256": "a".repeat(64), "size_bytes": 9}],
    });
    let finalize = serde_json::json!({
        "action": "finalize", "project_id": "project-a", "work_id": "work_a",
        "client_run_id": "publish-76ab72a6-6b1d-4095-a54e-139232b5152c",
    });
    for request in [
        NativeSyncRequest::for_project(
            SyncGatewayOperation::ComputeArtifacts,
            "project-a",
            &record,
        )
        .unwrap(),
        NativeSyncRequest::report_publication("project-a", &open).unwrap(),
        NativeSyncRequest::report_publication("project-a", &finalize).unwrap(),
    ] {
        assert_eq!(
            client.sync_gateway(&request, NOW).unwrap(),
            serde_json::json!({})
        );
    }
    let calls = transport.calls();
    let sync: Vec<_> = calls
        .into_iter()
        .filter(|call| call.starts_with("sync_gateway "))
        .collect();
    assert_eq!(sync.len(), 3);
    for call in &sync {
        assert!(call.contains("method=POST path=/api/v1/compute_artifacts bearer=true"));
    }
    assert!(sync[0].contains("\"action\":\"list\""));
    assert!(sync[1].contains("\"action\":\"open\""));
    assert!(sync[1].contains("\"engine_version\":\"ds-network-reporter@0.1.0+development\""));
    assert!(!sync[1].contains("grant"));
    assert!(sync[2].contains("\"action\":\"finalize\""));
}

#[test]
fn device_sync_uses_linked_authority_and_refuses_expired_access_before_io() {
    let profile = profile(DeploymentLane::Stable);
    let credential = device_credential(&profile);
    let access = DeviceAccessSession::new("ds-memory-access-jwt".into(), NOW + 300, NOW).unwrap();
    let authorization = credential
        .authorize_api(&access, &profile, "uid-1", NOW)
        .unwrap();
    let mut transport = FixtureTransport::default();
    let request = NativeSyncRequest::for_project(
        SyncGatewayOperation::ComputeArtifacts,
        "project-a",
        &serde_json::json!({"action":"list", "project_id":"project-a", "limit":200}),
    )
    .unwrap();
    transport.push_sync_gateway(200, br#"{"data":{"items":[]}}"#);
    assert_eq!(
        authorization
            .sync_gateway(&mut transport, &request, NOW)
            .unwrap(),
        serde_json::json!({"items":[]})
    );
    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].contains("path=/api/v1/compute_artifacts bearer=true device_bearer=true"));
    assert_eq!(
        authorization
            .sync_gateway(&mut transport, &request, NOW + 300)
            .unwrap_err()
            .kind(),
        ErrorKind::SignedOut
    );
    assert_eq!(transport.calls(), calls);

    let heartbeat = NativeSyncRequest::heartbeat_without_addition(
        ds_client_core::InstallHeartbeatWithoutAddition {
            install_id: "550e8400-e29b-41d4-a716-446655440000",
            device: "linux",
            arch: "x86_64",
            app_version: "0.1.2",
            channel: "stable",
            os_version: "linux native-server",
            webview_version: "native-server",
        },
    )
    .unwrap();
    transport.push_sync_gateway(200, br#"{"data":{"status":"active"}}"#);
    assert_eq!(
        authorization
            .sync_gateway(&mut transport, &heartbeat, NOW)
            .unwrap()["status"],
        "active"
    );
    assert!(transport
        .calls()
        .last()
        .unwrap()
        .contains("device_bearer=true"));

    for (status, kind) in [
        (403, ErrorKind::AuthenticationRejected),
        (409, ErrorKind::InvalidInput),
        (503, ErrorKind::Transient),
    ] {
        transport.push_sync_gateway(status, br#"{"error":{"code":"DENIED"}}"#);
        let error = authorization
            .sync_gateway(&mut transport, &request, NOW)
            .unwrap_err();
        assert_eq!(error.kind(), kind);
        assert_eq!(error.service_refusal().unwrap().status(), status);
    }
}

#[test]
fn password_failure_classification_is_shared_and_actionable_without_response_text() {
    let cases = [
        (
            400,
            "INVALID_LOGIN_CREDENTIALS",
            ErrorKind::InvalidCredentials,
        ),
        (
            400,
            "OPERATION_NOT_ALLOWED",
            ErrorKind::PasswordSignInUnavailable,
        ),
        (400, "USER_DISABLED", ErrorKind::AccountDisabled),
        (429, "TOO_MANY_ATTEMPTS_TRY_LATER", ErrorKind::Transient),
    ];
    for (status, message, expected) in cases {
        let mut client = Client::new(
            profile(DeploymentLane::Stable),
            FixtureTransport::with_rejected_sign_in(status, message),
            MemoryStore::default(),
        );
        let error = client
            .sign_in("user@example.com", "protected", NOW)
            .unwrap_err();
        assert_eq!(error.kind(), expected);
        assert!(!error.to_string().contains("private_detail"));
    }
}

#[test]
fn catalog_profile_is_closed_lane_bound_and_deterministic() {
    let stable = profile(DeploymentLane::Stable);
    assert_eq!(stable.lane(), DeploymentLane::Stable);
    assert_eq!(stable.client_id(), "ds-native-client");
    assert_eq!(stable.profile_sha256().len(), 64);
    assert_eq!(
        stable.profile_sha256(),
        profile(DeploymentLane::Stable).profile_sha256()
    );
    // Stable vectors pin method + path + every closed action vocabulary into
    // the full profile authority.
    assert_eq!(
        stable.profile_sha256(),
        "deba7531d30a2ea9c545c123cb98bd2adb2e4b656c874d411306a73e6b45b196"
    );
    // A profile is a digest over the closed vocabulary, so adding one route is
    // a new identity. Profile v27 added the reviewed closed design-attachment
    // workflow, v28 the national administrative hierarchy, v29 the two
    // platform reliability reads, and v30 the attachment pointer move, the
    // project-wide and reverse attachment reads, and the version summaries
    // and batch. These are the v27, v28 and v29 vectors a build must no
    // longer produce.
    assert_ne!(
        stable.profile_sha256(),
        "2de71a82a7680da77dc7015c15921c8424df945fbfef43e6f1db227889dbbac5"
    );
    assert_ne!(
        stable.profile_sha256(),
        "2a2c7954f468eb5068d958e9c188c70ac0a08c530e400c9ceeef06199396081b"
    );
    assert_ne!(
        stable.profile_sha256(),
        "264cabc267805e4324f3e615db07738d8136dddecbadccf2e68c29a1075eb037"
    );
    assert_ne!(
        stable.profile_sha256(),
        "5773df53261e74ba9b1c8bd3439e75fdbceff31ab6a593d7c06612bd15aded83"
    );
    // A new profile identity is exactly why the schema moved to v20
    // when `/api/v1/design/selections` was declared, and to v21 when
    // `/api/v1/data-distribution` was. These are the v18, v19 and v20
    // vectors: the first predates `list_transformers_status` on `/report`, the
    // second predates saved selections, the third predates the seeding door.
    // A build that still produced any of them would pair a new `ds` with an
    // old catalogue.
    assert_ne!(
        stable.profile_sha256(),
        "38f3da66b83370b71803fb32a005414869d11d46670ffb47eda8d2663aca21e9"
    );
    assert_ne!(
        stable.profile_sha256(),
        "afd2f2082f10a46f73036e41e58183d97816eff8837d43a88d111449cdc3b9c5"
    );
    assert_ne!(
        stable.profile_sha256(),
        "3bf908c7c4683e92f93e9442773b298893b90bb17c4aa7c922ac20a45e56f725"
    );
    assert_eq!(
        stable.credential_audience_sha256(),
        "9c8f8233b2a2395d760d5156edbf4355b23b9b29b0fbfbef5590322906d4b4e8"
    );

    let error = ClientProfile::validate(ClientProfileInput {
        schema_version: CLIENT_PROFILE_SCHEMA.to_owned(),
        lane: DeploymentLane::Stable,
        source_revision: "source".to_owned(),
        descriptor_sha256: "a".repeat(64),
        firebase_project_id: "firebase-project".to_owned(),
        firebase_api_key: "firebase-key".to_owned(),
        gateway_api_key: "gateway-key".to_owned(),
        gateway_origin: "https://eds-gateway-canary-3c0q477h.ue.gateway.dev".to_owned(),
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
        project_list_method: "POST".to_owned(),
        project_list_path: "/anything".to_owned(),
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
        tiles_actions: ds_client_core::TILES_ACTIONS
            .iter()
            .map(|action| (*action).to_owned())
            .collect(),
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
        design_attachments_method: "POST".into(),
        design_attachments_path: "/api/v1/design/attachments".into(),
        design_attachments_actions: ds_client_core::DESIGN_ATTACHMENTS_ACTIONS
            .map(str::to_owned)
            .to_vec(),
        design_versions_method: "POST".into(),
        design_versions_path: "/api/v1/design/versions".into(),
        design_versions_actions: ds_client_core::DESIGN_VERSIONS_ACTIONS
            .map(str::to_owned)
            .to_vec(),
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
        sre_overview_method: "GET".to_owned(),
        sre_overview_path: "/api/v1/sre/overview".to_owned(),
        sre_events_method: "POST".to_owned(),
        sre_events_path: "/api/v1/data".to_owned(),
        sre_events_actions: vec!["query_table".to_owned()],
        admin_bounds_method: "GET".to_owned(),
        admin_bounds_path: "/api/v1/admin/rwanda".to_owned(),
        admin_bounds_actions: vec!["children".to_owned(), "geometry".to_owned()],
    })
    .unwrap_err();
    assert_eq!(error, ProfileError::ProjectRoute);
}

#[test]
fn credential_audience_survives_profile_route_key_and_gateway_rotation() {
    let original = profile(DeploymentLane::Stable);
    let mut rotated = profile_input(DeploymentLane::Stable);
    rotated.source_revision = "fedcba9876543210fedcba9876543210fedcba98".to_owned();
    rotated.descriptor_sha256 = "b".repeat(64);
    rotated.firebase_api_key = "rotated-firebase-public-key".to_owned();
    rotated.gateway_api_key = "rotated-gateway-public-key".to_owned();
    let rotated = ClientProfile::validate(rotated).unwrap();
    assert_ne!(original.profile_sha256(), rotated.profile_sha256());
    assert_eq!(
        original.credential_audience_sha256(),
        rotated.credential_audience_sha256()
    );
    let original_store = MemoryStore::default();
    Client::new(
        original.clone(),
        FixtureTransport::with_sign_in(SIGN_IN),
        original_store.clone(),
    )
    .sign_in("user@example.com", "protected", NOW)
    .unwrap();
    let rotated_store = MemoryStore::default();
    Client::new(
        rotated.clone(),
        FixtureTransport::with_sign_in(SIGN_IN),
        rotated_store.clone(),
    )
    .sign_in("user@example.com", "protected", NOW)
    .unwrap();
    assert_eq!(original_store.last_key(), rotated_store.last_key());

    assert_ne!(
        original.credential_audience_sha256(),
        profile(DeploymentLane::Canary).credential_audience_sha256()
    );
    let mut another_project = profile_input(DeploymentLane::Stable);
    another_project.firebase_project_id = "another-firebase-project".to_owned();
    assert_ne!(
        original.credential_audience_sha256(),
        ClientProfile::validate(another_project)
            .unwrap()
            .credential_audience_sha256()
    );
    let mut another_origin = profile_input(DeploymentLane::Stable);
    another_origin.gateway_origin = "https://eds-gateway-replacement.ue.gateway.dev".to_owned();
    assert_eq!(
        original.credential_audience_sha256(),
        ClientProfile::validate(another_origin)
            .unwrap()
            .credential_audience_sha256()
    );
}

#[test]
fn sign_in_binds_identity_and_persists_no_password_or_id_token() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let store = MemoryStore::default();
    let client = signed_in(transport.clone(), store.clone());

    assert_eq!(client.user().unwrap().uid(), "uid-1");
    let stored = String::from_utf8(store.value().unwrap()).unwrap();
    assert!(stored.contains("refresh-one"));
    assert!(stored.contains("uid-1"));
    let context =
        probe_stored_refresh(stored.as_bytes(), &profile(DeploymentLane::Stable)).unwrap();
    assert_eq!(context.uid(), "uid-1");
    assert_eq!(context.lane(), "stable");
    assert_eq!(
        context.credential_audience_sha256(),
        profile(DeploymentLane::Stable).credential_audience_sha256()
    );
    assert!(!stored.contains("password-from-protected-input"));
    assert!(!stored.contains("idToken"));
    assert!(!stored.contains("e30."));
    assert!(transport.calls()[0].contains(
        "endpoint=https://identitytoolkit.googleapis.com/v1/accounts:signInWithPassword"
    ));
    assert!(!transport
        .calls()
        .join("\n")
        .contains("password-from-protected-input"));
    assert!(!transport.calls().join("\n").contains("refresh-one"));
}

#[test]
fn project_directory_uses_three_exact_queries_in_live_contract_order() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project(ACTIVE);
    transport.push_project(ARCHIVED);
    transport.push_project(TESTING);
    let store = MemoryStore::default();
    let mut client = signed_in(transport.clone(), store);

    let directory = client.list_projects(NOW + 1).unwrap();
    assert_eq!(
        directory
            .projects()
            .iter()
            .map(|project| (project.ds_project(), project.status()))
            .collect::<Vec<_>>(),
        vec![
            ("active-project", ProjectStatus::Active),
            ("archived-project", ProjectStatus::Archived),
            ("testing-project", ProjectStatus::Testing),
        ]
    );
    let calls = transport.calls();
    assert!(calls[1].contains("/api/v1/user/projects?status=active"));
    assert!(calls[2].contains("/api/v1/user/projects?status=archived"));
    assert!(calls[3].contains("/api/v1/user/projects?status=testing"));
    assert!(calls[1].contains("api_key=true"));
    assert!(calls[1].contains("content_type=application/json"));
    assert!(calls[1].contains("authorization=true"));
    assert!(calls[1].contains("forwarded_authorization=true"));
    assert!(calls[1].contains("client_id=ds-native-client"));
    assert!(calls[1].contains("correlation=true"));
    assert_eq!(transport.correlated_project_requests(), 3);
    assert!(!calls.join("\n").contains("public-gateway-key"));
    assert!(!calls.join("\n").contains("e30."));
}

/// `POST /auth/device/approve` needs a Firebase USER principal and nothing
/// else — the restored headless session is one. The receipt goes out only
/// after it was verified against this request and this principal, in the
/// shape the Desktop hands its callers; and a device credential's refusal
/// crosses as its own typed code, because a device may not admit a sibling.
#[test]
fn device_approval_runs_under_the_restored_user_and_types_the_desktop_only_refusal() {
    use ds_client_core::{
        DeviceApprovalDecision, DeviceApprovalRequest, DeviceApprovalServiceCode,
    };
    const FINGERPRINT: &str =
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    fn receipt(status: &str, preview: bool, decision: Option<&str>) -> Vec<u8> {
        let mut data = json!({
            "request": {
                "request_id": "req_01",
                "status": status,
                "device": {"name": "Workstation", "platform": "linux", "fingerprint": FINGERPRINT},
                "user_code": "ABCD-EFGH",
                "scopes": ["ds.api"],
                "binding": {
                    "lane": "stable",
                    "audience": "d".repeat(64),
                    "profile_digest": format!("sha256:{}", "a".repeat(64)),
                    "catalog_digest": format!("sha256:{}", "b".repeat(64)),
                },
                "created_at": "2026-09-19T08:00:00Z",
                "expires_at": "2026-09-19T08:10:00Z",
                "renewable": true
            },
            "preview": preview,
            "principal": {"uid": "uid-1", "email": "user@example.com"}
        });
        if let Some(value) = decision {
            data["decision"] = json!({"value": value, "decided_at": "2026-09-19T08:01:00Z"});
        }
        serde_json::to_vec(&json!({
            "success": true,
            "message": "Device approval preview",
            "data": data,
            "timestamp": "2026-09-19T08:01:00Z",
            "service": "data-solutions-backend",
            "version": "1.0.0"
        }))
        .unwrap()
    }
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_device_approve(200, &receipt("pending", true, None));
    transport.push_device_approve(200, &receipt("approved", false, Some("approve")));
    transport.push_device_approve(
        403,
        &serde_json::to_vec(&json!({
            "success": false,
            "error": {
                "code": "DESKTOP_PRINCIPAL_REQUIRED",
                "message": "An authenticated Desktop principal is required",
                "details": {"remedy": "approve from a signed-in Desktop session; a device credential cannot approve its own authorization"},
                "timestamp": 1
            }
        }))
        .unwrap(),
    );
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    let preview = client
        .device_approve(
            &DeviceApprovalRequest::new(
                "req_01",
                FINGERPRINT,
                DeviceApprovalDecision::Approve,
                false,
            )
            .unwrap(),
            NOW + 1,
        )
        .unwrap();
    assert!(preview.preview());
    let committed = client
        .device_approve(
            &DeviceApprovalRequest::new(
                "req_01",
                FINGERPRINT,
                DeviceApprovalDecision::Approve,
                true,
            )
            .unwrap(),
            NOW + 2,
        )
        .unwrap();
    // The Desktop's camelCase receipt, which the CLI's validator already reads.
    let public = committed.public_value();
    assert_eq!(public["requestId"], "req_01");
    assert_eq!(public["preview"], false);
    assert_eq!(public["status"], "approved");
    assert_eq!(public["renewable"], true);
    assert_eq!(public["scopes"], json!(["ds.api"]));
    assert_eq!(public["device"]["fingerprint"], FINGERPRINT);
    assert_eq!(public["binding"]["lane"], "stable");
    assert_eq!(public["decision"]["value"], "approve");
    assert!(public.get("principal").is_none());

    let refused = client
        .device_approve(
            &DeviceApprovalRequest::new(
                "req_01",
                FINGERPRINT,
                DeviceApprovalDecision::Approve,
                true,
            )
            .unwrap(),
            NOW + 3,
        )
        .unwrap_err();
    assert_eq!(
        refused.device_approval_service_code(),
        Some(DeviceApprovalServiceCode::DesktopPrincipalRequired)
    );
    assert_eq!(refused.kind(), ErrorKind::AuthenticationRejected);

    let calls = transport.calls();
    let approvals: Vec<&String> = calls
        .iter()
        .filter(|call| call.starts_with("device_approve"))
        .collect();
    assert_eq!(approvals.len(), 3);
    // The bearer is the restored user's Firebase id token, never a device
    // credential; the body is exactly `deviceauth.ApprovalRequest`.
    assert!(
        approvals[0].contains("user_id_token=true device=false email=user@example.com"),
        "{}",
        approvals[0]
    );
    assert!(approvals[0].ends_with(
        r#"body={"request_id":"req_01","device_fingerprint":"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","decision":"approve","confirm":false}"#
    ), "{}", approvals[0]);
    assert!(
        approvals[1].ends_with(r#""decision":"approve","confirm":true}"#),
        "{}",
        approvals[1]
    );
    assert!(!calls.join("\n").contains("e30.eyJ"));
}

/// A platform_admin sees every project through ds-brain's elevated path with
/// no membership document behind any row, so every `role` is empty. The
/// server says so — `"elevated": true` beside `projects` — and the directory
/// must carry that verdict, or an operator reads "no role" where the truth is
/// "seen by elevation". The three buckets are one read by one principal, so
/// they agree or the directory is refused.
#[test]
fn project_directory_carries_the_servers_elevation_verdict_and_refuses_a_split_one() {
    fn bucket(id: &str, state: &str, elevated: bool) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "success": true,
            "data": {
                "count": 1,
                "projects": [{"id": id, "eds_project_id": id, "project_name": id, "role": "", "lifecycle_state": state}],
                "elevated": elevated,
                "status": state
            }
        }))
        .unwrap()
    }
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project(&bucket("a", "active", true));
    transport.push_project(&bucket("b", "archived", true));
    transport.push_project(&bucket("c", "testing", true));
    // A second directory read whose buckets disagree.
    transport.push_project(&bucket("a", "active", true));
    transport.push_project(&bucket("b", "archived", false));
    transport.push_project(&bucket("c", "testing", true));
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    let directory = client.list_projects(NOW + 1).unwrap();
    assert!(directory.elevated());
    assert_eq!(directory.projects().len(), 3);
    assert_eq!(directory.exact("a").unwrap().role(), Some(""));

    let error = client.list_projects(NOW + 2).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
    assert_eq!(
        error.to_string(),
        "the lifecycle buckets disagreed about the caller's elevation"
    );

    // The checked-in fixtures predate the verdict: an older server's answer
    // stays readable and reads as not elevated.
    transport.push_project(ACTIVE);
    transport.push_project(ARCHIVED);
    transport.push_project(TESTING);
    assert!(!client.list_projects(NOW + 3).unwrap().elevated());
}

#[test]
fn transformer_context_reads_full_source_with_nested_properties() {
    let response = serde_json::json!({
        "success": true,
        "data": {
            "total": 1,
            "found_count": 1,
            "failed_count": 0,
            "results": [{
                "transformer_name": "tx_1",
                "ok": true,
                "data": {
                    "layers": {
                        "lv_lines": {"type": "FeatureCollection", "features": [{
                            "type": "Feature",
                            "geometry": {"type": "LineString", "coordinates": [[30, -2], [30.1, -2.1]]},
                            "properties": {"engineering": {"phases": [1, 2, 3]}, "tags": ["retained"]}
                        }]}
                    },
                    "metadata": {"version": 7, "content_digest": "abc123"}
                }
            }]
        }
    });
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_transformer_context(200, &serde_json::to_vec(&response).unwrap());
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    let context = client
        .transformer_context("active-project", "tx_1", NOW + 1)
        .unwrap();

    assert_eq!(context.ds_project(), "active-project");
    assert_eq!(context.transformer_name(), "tx_1");
    assert_eq!(
        context.layers().keys().collect::<Vec<_>>(),
        vec!["lv_lines"]
    );
    assert_eq!(
        context.layers()["lv_lines"]["features"][0]["properties"],
        serde_json::json!({"engineering": {"phases": [1, 2, 3]}, "tags": ["retained"]})
    );
    assert_eq!(context.metadata().version(), Some(7));
    assert_eq!(context.metadata().content_digest(), Some("abc123"));
    assert_eq!(transport.correlated_transformer_requests(), 1);
    let call = transport.calls().last().unwrap().clone();
    assert!(call
        .contains("method=POST origin=https://eds-gateway-3c0q477h.ue.gateway.dev path=/report"));
    assert!(call.contains("body_exact=true lane_absent=true"));
    assert!(call.contains("projection=full"));
    assert!(call.contains("limit=67108864 timeout=120"));
    assert!(call.contains("authorization=true forwarded_authorization=true"));
    assert!(call.contains("client_id=ds-native-client correlation=true"));
    assert!(!call.contains("public-gateway-key"));
    assert!(!call.contains("e30."));

    transport.push_transformer_context(200, &serde_json::to_vec(&response).unwrap());
    client
        .transformer_context_saved("active-project", "tx_1", NOW + 2)
        .unwrap();
    let saved_call = transport.calls().last().unwrap().clone();
    assert!(saved_call.contains("projection=saved body_exact=true lane_absent=true"));
}

#[test]
fn transformer_context_profile_and_runtime_inputs_have_no_escape_hatch() {
    for mutate in [
        |input: &mut ClientProfileInput| input.transformer_context_method = "GET".to_owned(),
        |input: &mut ClientProfileInput| input.transformer_context_path = "/other".to_owned(),
        |input: &mut ClientProfileInput| {
            input.transformer_context_action = "query_entries".to_owned()
        },
        |input: &mut ClientProfileInput| input.transformer_context_fields = "context".to_owned(),
        |input: &mut ClientProfileInput| input.transformer_context_fields = "saved".to_owned(),
        |input: &mut ClientProfileInput| input.transformer_context_fields = "full,saved".to_owned(),
    ] {
        let mut input = profile_input(DeploymentLane::Stable);
        mutate(&mut input);
        assert_eq!(
            ClientProfile::validate(input).unwrap_err(),
            ProfileError::TransformerContextRoute
        );
    }

    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    for (project, transformer) in [
        ("", "tx_1"),
        (" active-project", "tx_1"),
        ("active-project", ""),
        ("active-project", "__all__"),
        ("active-project", "tx\n1"),
    ] {
        assert_eq!(
            client
                .transformer_context(project, transformer, NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidInput
        );
    }
    assert_eq!(
        transport.calls().len(),
        1,
        "invalid input must not reach transport"
    );
}

#[test]
fn transformer_context_refuses_mismatched_or_unbounded_responses() {
    let malformed = [
        serde_json::json!({
            "data": {"total": 2, "found_count": 1, "failed_count": 0, "results": []}
        }),
        serde_json::json!({
            "data": {"total": 1, "found_count": 1, "failed_count": 0, "results": [{
                "transformer_name": "another", "ok": true, "data": {"layers": {}}
            }]}
        }),
        serde_json::json!({
            "data": {"total": 1, "found_count": 1, "failed_count": 0, "results": [{
                "transformer_name": "tx_1", "ok": true,
                "data": {"layers": {" bad": {"type": "FeatureCollection", "features": []}}}
            }]}
        }),
    ];
    for body in malformed {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_transformer_context(200, &serde_json::to_vec(&body).unwrap());
        let mut client = signed_in(transport, MemoryStore::default());
        assert_eq!(
            client
                .transformer_context("active-project", "tx_1", NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }

    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_transformer_context(200, &vec![b'x'; 64 * 1024 * 1024 + 1]);
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client
            .transformer_context("active-project", "tx_1", NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
}

#[test]
fn transformer_context_preserves_a_coherent_per_item_refusal() {
    let refused = serde_json::json!({
        "success": true,
        "data": {
            "total": 1,
            "found_count": 0,
            "failed_count": 1,
            "results": [{
                "transformer_name": "tx_1",
                "ok": false,
                "error_code": "TRANSFORMER_NOT_FOUND",
                "error": "Transformer tx_1 was not found"
            }]
        }
    });
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_transformer_context(200, &serde_json::to_vec(&refused).unwrap());
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client
            .transformer_context("active-project", "tx_1", NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::ResourceNotFound
    );
}

#[test]
fn project_forms_is_one_fixed_selected_project_activation_call() {
    let body = serde_json::json!({
        "status": "success",
        "project_id": "active-project",
        "resolved_at": 1_900_000_001_u64,
        "user": { "email": "user@example.com", "roles": ["surveyor"], "is_owner": false, "is_admin": false },
        "forms": [
            {
                "slug": "z_lines", "display_name": "Z Lines", "enabled": false,
                "settings_revision": 3, "drawing_settings": { "geometry_type": "LineString" },
                "metadata": {}, "fields": [{"name":"ignored-by-summary"}]
            },
            {
                "slug": "a_poles", "display_name": "A Poles", "enabled": true,
                "settings_revision": 7, "geometry_type": "Point",
                "metadata": { "unavailable": false }
            }
        ],
        "orphaned_project_forms": [
            { "project_id": "active-project", "form_slug": "old_form", "reason": "master_archived", "enabled": true, "settings_revision": 2 }
        ],
        "field_types": [], "condition_operators": [], "type_aliases": {},
        "settings_registry": [], "settings_editors": {}
    });
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project_forms(200, &serde_json::to_vec(&body).unwrap());
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let snapshot = client.project_forms("active-project", NOW + 1).unwrap();

    assert_eq!(snapshot.ds_project(), "active-project");
    assert_eq!(snapshot.forms()[0].slug(), "a_poles");
    assert_eq!(snapshot.forms()[0].geometry_type(), Some("Point"));
    assert_eq!(snapshot.forms()[1].slug(), "z_lines");
    assert_eq!(snapshot.orphaned_project_forms()[0].form_slug(), "old_form");
    assert_eq!(
        snapshot.orphaned_project_forms()[0].reason(),
        "master_archived"
    );
    assert!(transport.calls().iter().any(|call| call ==
        "project_forms method=POST origin=https://eds-gateway-3c0q477h.ue.gateway.dev path=/api/v1/project-forms content_type=application/json body_exact=true limit=33554432 timeout=120 api_key=true authorization=true forwarded_authorization=true email=true client_id=ds-native-client correlation=true"));
}

#[test]
fn project_forms_decodes_the_activation_payload_ds_brain_actually_sends() {
    // Captured from POST /api/v1/project-forms {"action":"activate"} against
    // zimebnlu_edcl2 on the stable gateway: ds-brain marshals a form with no
    // drawing configuration as `"drawing_settings": null`, nine of thirteen
    // forms on that project included. Serde's `default` covers an absent key,
    // never a present one whose value is null, so the whole snapshot used to
    // fail and `ds survey project-forms list` refused
    // `auth_response_unreadable` on every project the user has.
    let body = serde_json::json!({
        "status": "success",
        "project_id": "active-project",
        "resolved_at": 1_789_739_530_434_u64,
        "user": { "email": "user@example.com", "roles": [], "is_owner": false, "is_admin": false },
        "forms": [
            {
                "slug": "field_notes", "display_name": "Field Notes", "enabled": false,
                "settings_revision": 0, "geometry_type": "point",
                "drawing_settings": serde_json::Value::Null,
                "metadata": {}
            },
            {
                "slug": "lv_lines_survey", "display_name": "LV Lines - Survey", "enabled": true,
                "settings_revision": 0, "geometry_type": serde_json::Value::Null,
                "drawing_settings": { "geometry_type": "LineString" },
                "metadata": serde_json::Value::Null
            }
        ],
        "field_types": [], "condition_operators": [], "type_aliases": {},
        "settings_registry": []
    });
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project_forms(200, &serde_json::to_vec(&body).unwrap());
    let mut client = signed_in(transport, MemoryStore::default());
    let snapshot = client.project_forms("active-project", NOW + 1).unwrap();

    assert_eq!(snapshot.forms().len(), 2);
    assert_eq!(snapshot.forms()[0].slug(), "field_notes");
    assert_eq!(snapshot.forms()[0].geometry_type(), Some("point"));
    // A null metadata map is not a claim that the form is unavailable.
    assert!(snapshot.forms()[1].available());
    assert_eq!(snapshot.forms()[1].geometry_type(), Some("LineString"));
    assert!(snapshot.orphaned_project_forms().is_empty());
}

#[test]
fn project_forms_accepts_matching_orphan_lineage_also_present_in_forms() {
    // The activation response retains archived masters' definitions so their
    // existing entries can still render, and repeats their lineage in the
    // orphan audit. The two views must agree on binding state and reason.
    let body = serde_json::json!({
        "status": "success",
        "project_id": "active-project",
        "resolved_at": 1,
        "user": { "email": "user@example.com", "roles": [], "is_owner": false, "is_admin": false },
        "forms": [{
            "slug": "old_form", "display_name": "Old Form", "enabled": true,
            "settings_revision": 2, "master_availability": "master_archived",
            "metadata": {}
        }],
        "orphaned_project_forms": [{
            "project_id": "active-project", "form_slug": "old_form",
            "reason": "master_archived", "enabled": true, "settings_revision": 2
        }],
        "field_types": [], "condition_operators": [], "type_aliases": {}
    });
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project_forms(200, &serde_json::to_vec(&body).unwrap());
    let mut client = signed_in(transport, MemoryStore::default());
    let snapshot = client.project_forms("active-project", NOW + 1).unwrap();
    assert_eq!(snapshot.forms().len(), 1);
    assert!(!snapshot.forms()[0].available());
    assert_eq!(snapshot.orphaned_project_forms()[0].form_slug(), "old_form");
}

#[test]
fn project_forms_rejects_orphan_lineage_disagreement() {
    for (availability, enabled, revision) in [
        ("master_available", true, 2),
        ("master_archived", false, 2),
        ("master_archived", true, 3),
    ] {
        let body = serde_json::json!({
            "status": "success", "project_id": "active-project", "resolved_at": 1,
            "user": { "email": "user@example.com", "roles": [], "is_owner": false, "is_admin": false },
            "forms": [{
                "slug": "old_form", "display_name": "Old Form", "enabled": enabled,
                "settings_revision": revision, "master_availability": availability
            }],
            "orphaned_project_forms": [{
                "project_id": "active-project", "form_slug": "old_form",
                "reason": "master_archived", "enabled": true, "settings_revision": 2
            }],
            "field_types": [], "condition_operators": [], "type_aliases": {}
        });
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_project_forms(200, &serde_json::to_vec(&body).unwrap());
        let mut client = signed_in(transport, MemoryStore::default());
        assert_eq!(
            client
                .project_forms("active-project", NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }
}

#[test]
fn project_forms_profile_and_response_fail_closed() {
    for mutate in [
        |input: &mut ClientProfileInput| input.project_forms_method = "GET".to_owned(),
        |input: &mut ClientProfileInput| input.project_forms_path = "/other".to_owned(),
        |input: &mut ClientProfileInput| input.project_forms_action = "bulk_save".to_owned(),
        |input: &mut ClientProfileInput| input.project_form_editor_action = "get_editor".to_owned(),
    ] {
        let mut input = profile_input(DeploymentLane::Stable);
        mutate(&mut input);
        assert_eq!(
            ClientProfile::validate(input).unwrap_err(),
            ProfileError::ProjectFormsRoute
        );
    }

    for (project, email) in [
        ("other-project", "user@example.com"),
        ("active-project", "other@example.com"),
    ] {
        let body = serde_json::json!({
            "status": "success", "project_id": project, "resolved_at": 1,
            "user": { "email": email, "roles": [], "is_owner": false, "is_admin": false },
            "forms": [], "field_types": [], "condition_operators": [], "type_aliases": {}
        });
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_project_forms(200, &serde_json::to_vec(&body).unwrap());
        let mut client = signed_in(transport, MemoryStore::default());
        assert_eq!(
            client
                .project_forms("active-project", NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }
}

fn settings_editor_body() -> serde_json::Value {
    serde_json::json!({
        "status": "success",
        "project_id": "active-project",
        "form_slug": "a_poles",
        "editor": {
            "form": {
                "slug": "a_poles",
                "display_name": "A Poles",
                "description": "Pole collection settings",
                "enabled": true
            },
            "current_settings": { "enforce_accuracy": true },
            "effective_settings": { "enforce_accuracy": true },
            "sections": [{
                "key": "location",
                "label": "Location",
                "fields": [{
                    "key": "enforce_accuracy",
                    "label": "Enforce accuracy",
                    "type": "bool",
                    "value": true,
                    "default": false,
                    "hint": "Require a bounded GPS accuracy"
                }]
            }],
            "field_state": {
                "enforce_accuracy": {
                    "visible": true,
                    "applicable": true,
                    "enabled": true,
                    "required": false
                }
            },
            "capabilities": {
                "geometry_type": "point",
                "supports_drawing": false,
                "supports_gps_location": true,
                "supports_location_picking": true,
                "supports_snapping": false,
                "supports_position_editing": true,
                "supports_geometry_drawing_tools": false,
                "supports_geometry_automation": true
            },
            "version": 7
        }
    })
}

#[test]
fn project_form_editor_is_one_fixed_selected_project_call() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project_form_editor(200, &serde_json::to_vec(&settings_editor_body()).unwrap());
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let snapshot = client
        .project_form_editor("active-project", "a_poles", NOW + 1)
        .unwrap();

    assert_eq!(snapshot.ds_project(), "active-project");
    assert_eq!(snapshot.form_slug(), "a_poles");
    assert_eq!(snapshot.editor()["version"], 7);
    assert_eq!(snapshot.editor()["form"]["slug"], "a_poles");
    assert!(transport.calls().iter().any(|call| call ==
        "project_form_editor method=POST origin=https://eds-gateway-3c0q477h.ue.gateway.dev path=/api/v1/project-forms content_type=application/json body_exact=true limit=33554432 timeout=120 api_key=true authorization=true forwarded_authorization=true email=true client_id=ds-native-client correlation=true"));
}

#[test]
fn project_form_editor_refuses_identity_shape_and_field_state_drift() {
    for mutate in [
        |body: &mut serde_json::Value| body["project_id"] = serde_json::json!("other-project"),
        |body: &mut serde_json::Value| body["form_slug"] = serde_json::json!("other_form"),
        |body: &mut serde_json::Value| {
            body["editor"]["form"]["slug"] = serde_json::json!("other_form")
        },
        |body: &mut serde_json::Value| body["editor"]["field_state"] = serde_json::json!({}),
        |body: &mut serde_json::Value| body["editor"]["surprise"] = serde_json::json!(true),
        |body: &mut serde_json::Value| {
            body["editor"]["version"] = serde_json::json!(9_007_199_254_740_992_u64)
        },
    ] {
        let mut body = settings_editor_body();
        mutate(&mut body);
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_project_form_editor(200, &serde_json::to_vec(&body).unwrap());
        let mut client = signed_in(transport, MemoryStore::default());
        assert_eq!(
            client
                .project_form_editor("active-project", "a_poles", NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }
}

#[test]
fn unavailable_project_form_editor_is_explicit_and_read_only() {
    let body = serde_json::json!({
        "status": "success",
        "project_id": "active-project",
        "form_slug": "a_poles",
        "unavailable": true,
        "unavailable_reason": "master_missing_or_inactive",
        "editor": {
            "form": { "slug": "a_poles", "display_name": "a_poles", "enabled": false },
            "current_settings": {},
            "effective_settings": {},
            "sections": [],
            "field_state": {},
            "capabilities": {
                "supports_drawing": false,
                "supports_gps_location": false,
                "supports_location_picking": false,
                "supports_snapping": false,
                "supports_position_editing": false,
                "supports_geometry_drawing_tools": false,
                "supports_geometry_automation": false
            },
            "version": 2,
            "read_only": true,
            "unavailable": true,
            "unavailable_reason": "master_missing_or_inactive",
            "message": "The master form is unavailable."
        }
    });
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project_form_editor(200, &serde_json::to_vec(&body).unwrap());
    let mut client = signed_in(transport, MemoryStore::default());
    let snapshot = client
        .project_form_editor("active-project", "a_poles", NOW + 1)
        .unwrap();
    assert_eq!(snapshot.editor()["read_only"], true);
    assert_eq!(snapshot.editor()["unavailable"], true);
}

#[test]
fn project_forms_refuses_cross_project_and_contradictory_orphans() {
    for (orphan_project, orphan_slug) in [
        ("other-project", "old_form"),
        ("active-project", "active_form"),
    ] {
        let body = serde_json::json!({
            "status": "success",
            "project_id": "active-project",
            "resolved_at": 1,
            "user": { "email": "user@example.com", "roles": [], "is_owner": false, "is_admin": false },
            "forms": [{
                "slug": "active_form", "display_name": "Active Form", "enabled": true,
                "settings_revision": 1, "metadata": {}
            }],
            "orphaned_project_forms": [{
                "project_id": orphan_project, "form_slug": orphan_slug,
                "reason": "master_archived", "enabled": false, "settings_revision": 1
            }],
            "field_types": [], "condition_operators": [], "type_aliases": {},
            "settings_registry": []
        });
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_project_forms(200, &serde_json::to_vec(&body).unwrap());
        let mut client = signed_in(transport, MemoryStore::default());
        assert_eq!(
            client
                .project_forms("active-project", NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }
}

#[test]
fn stable_audience_intentionally_refuses_the_legacy_route_bound_audience() {
    use sha2::{Digest, Sha256};

    let current = profile(DeploymentLane::Stable);
    let mut hasher = Sha256::new();
    for value in [
        "ds.native-client-profiles/v8",
        "stable",
        "ds-native-client",
        "firebase-project",
        "https://eds-gateway-3c0q477h.ue.gateway.dev",
        "GET",
        "/api/v1/user/projects",
        "POST",
        "/report",
        "get_transformers_data",
        "context",
        "POST",
        "/api/v1/project-forms",
        "activate",
        "settings_editor",
        "POST",
        "/api/v1/solar",
        "desktop_snapshot",
        "POST",
        "/api/v1/survey/query",
        "POST",
        "/api/v1/survey/entries/select",
        "POST",
        "/api/v1/survey/entries/changes",
    ] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    let v8_audience = format!("{:x}", hasher.finalize());
    assert_ne!(current.credential_audience_sha256(), v8_audience);

    let store = MemoryStore::default();
    store.0.lock().unwrap().value = Some(
        serde_json::to_vec(&serde_json::json!({
            "schema": "ds-client.refresh/v1",
            "credential_audience": v8_audience,
            "uid": "uid-1",
            "email": "user@example.com",
            "refresh_token": "refresh-one"
        }))
        .unwrap(),
    );
    let mut client = Client::new(current, FixtureTransport::default(), store);
    assert_eq!(
        client.restore(NOW).unwrap_err().kind(),
        ErrorKind::DurableState
    );
}

#[test]
fn login_instance_survives_refresh_and_changes_even_for_same_account_relogin() {
    let profile = profile(DeploymentLane::Stable);
    let store = MemoryStore::default();
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_refresh(200, REFRESH);
    transport.push_project(ACTIVE);
    transport.push_project(ARCHIVED);
    transport.push_project(TESTING);
    let mut client = signed_in(transport, store.clone());
    let first = probe_stored_refresh(&store.value().unwrap(), &profile).unwrap();
    client.list_projects(NOW + 3_001).unwrap();
    let refreshed = probe_stored_refresh(&store.value().unwrap(), &profile).unwrap();
    assert_eq!(
        first.credential_instance_sha256(),
        refreshed.credential_instance_sha256()
    );
    // Simulate logout removing the durable credential. The provider may even
    // return identical wire bytes; a fresh login still has a fresh instance.
    store.0.lock().unwrap().value = None;
    let _replacement = signed_in(FixtureTransport::with_sign_in(SIGN_IN), store.clone());
    let replacement = probe_stored_refresh(&store.value().unwrap(), &profile).unwrap();
    assert_eq!(first.uid(), replacement.uid());
    assert_ne!(
        first.credential_instance_sha256(),
        replacement.credential_instance_sha256()
    );
}

#[test]
fn a_legacy_login_instance_is_preserved_by_its_first_refresh() {
    let profile = profile(DeploymentLane::Stable);
    let store = MemoryStore::default();
    let _client = signed_in(FixtureTransport::with_sign_in(SIGN_IN), store.clone());
    let mut legacy: serde_json::Value = serde_json::from_slice(&store.value().unwrap()).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("credential_instance_sha256");
    store.0.lock().unwrap().value = Some(serde_json::to_vec(&legacy).unwrap());
    let before = probe_stored_refresh(&store.value().unwrap(), &profile).unwrap();
    let transport = FixtureTransport::default();
    transport.push_refresh(200, REFRESH);
    let mut restored = Client::new(profile.clone(), transport, store.clone());
    restored.restore(NOW + 3_001).unwrap();
    let after = probe_stored_refresh(&store.value().unwrap(), &profile).unwrap();
    assert_eq!(
        before.credential_instance_sha256(),
        after.credential_instance_sha256()
    );
    assert!(!after.credential_instance_sha256().is_empty());
}

#[test]
fn rotated_refresh_is_atomically_durable_before_project_reads() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_refresh(200, REFRESH);
    transport.push_project(ACTIVE);
    transport.push_project(ARCHIVED);
    transport.push_project(TESTING);
    let store = MemoryStore::default();
    let mut client = signed_in(transport.clone(), store.clone());

    client.list_projects(NOW + 3_001).unwrap();
    let stored = String::from_utf8(store.value().unwrap()).unwrap();
    assert!(stored.contains("refresh-two"));
    assert!(!stored.contains("refresh-one"));
    assert!(!stored.contains("e30."));
    let calls = transport.calls();
    assert!(calls[1].contains("refresh_token=true"));
    assert!(calls[2].contains("status=active"));
}

#[test]
fn refresh_accepts_id_token_compatibility_and_equal_dual_tokens() {
    let token = valid_jwt();
    for body in [
        refresh_body_with_tokens(None, Some(token.clone()), "refresh-id-only"),
        refresh_body_with_tokens(
            Some(token.clone()),
            Some(token.clone()),
            "refresh-both-equal",
        ),
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_refresh(200, &body);
        let store = MemoryStore::default();
        let mut client = signed_in(transport, store.clone());

        client.refresh(NOW + 3_001).unwrap();
        assert_eq!(client.user().unwrap().uid(), "uid-1");
        assert!(String::from_utf8(store.value().unwrap())
            .unwrap()
            .contains("refresh-"));
    }
}

#[test]
fn refresh_refuses_conflicting_or_missing_token_fields() {
    let token = valid_jwt();
    let conflict = format!("{token}-different");
    for body in [
        refresh_body_with_tokens(Some(token), Some(conflict), "refresh-conflict"),
        refresh_body_with_tokens(None, None, "refresh-missing"),
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_refresh(200, &body);
        let store = MemoryStore::default();
        let mut client = signed_in(transport, store.clone());

        assert_eq!(
            client.refresh(NOW + 3_001).unwrap_err().kind(),
            ErrorKind::IdentityMismatch
        );
        assert!(client.user().is_none());
        assert!(store.value().is_none());
    }
}

#[test]
fn refresh_cas_conflict_preserves_newer_credential_and_is_retriable() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_refresh(200, REFRESH);
    let store = MemoryStore::default();
    let mut client = signed_in(transport, store.clone());
    let newer = String::from_utf8(store.value().unwrap())
        .unwrap()
        .replace("refresh-one", "refresh-newer")
        .into_bytes();
    store.conflict_with(newer.clone());

    let error = client.list_projects(NOW + 3_001).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Transient);
    assert_eq!(store.value(), Some(newer));
}

#[test]
fn permanent_rejection_does_not_erase_or_mask_concurrent_rotation() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_refresh(400, REVOKED);
    let store = MemoryStore::default();
    let mut client = signed_in(transport, store.clone());
    let newer = String::from_utf8(store.value().unwrap())
        .unwrap()
        .replace("refresh-one", "refresh-newer")
        .into_bytes();
    store.conflict_with(newer.clone());

    let error = client.list_projects(NOW + 3_001).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Transient);
    assert_eq!(store.value(), Some(newer));
    assert!(client.user().is_none());
}

#[test]
fn durable_and_auth_error_bodies_are_independently_bounded() {
    let store = MemoryStore::default();
    store.0.lock().unwrap().value = Some(vec![b'x'; 64 * 1024 + 1]);
    let mut client = Client::new(
        profile(DeploymentLane::Stable),
        FixtureTransport::default(),
        store,
    );
    assert_eq!(
        client.restore(NOW).unwrap_err().kind(),
        ErrorKind::DurableState
    );

    let transport = FixtureTransport::default();
    transport
        .0
        .lock()
        .unwrap()
        .sign_in
        .push_back(TransportResponse::new(400, vec![b'x'; 256 * 1024 + 1]));
    let mut client = Client::new(
        profile(DeploymentLane::Stable),
        transport,
        MemoryStore::default(),
    );
    assert_eq!(
        client
            .sign_in("user@example.com", "protected", NOW)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
}

#[test]
fn duplicate_or_mislabelled_project_identity_is_refused() {
    let duplicate = br#"{"data":{"projects":[{"eds_project_id":"active-project","lifecycle_state":"archived"}]}}"#;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project(duplicate);
    transport.push_project(ARCHIVED);
    transport.push_project(TESTING);
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client.list_projects(NOW + 1).unwrap_err().kind(),
        ErrorKind::UnreadableResponse
    );

    let duplicate = br#"{"data":{"projects":[{"eds_project_id":"active-project","lifecycle_state":"archived","is_archived":true}]}}"#;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project(ACTIVE);
    transport.push_project(duplicate);
    transport.push_project(TESTING);
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client.list_projects(NOW + 1).unwrap_err().kind(),
        ErrorKind::UnreadableResponse
    );
}

#[test]
fn permanent_cleanup_failure_is_durable_and_requires_host_cleanup() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_refresh(400, REVOKED);
    let store = MemoryStore::default();
    let mut client = signed_in(transport, store.clone());
    let original = store.value();
    store.fail_next_deletion(StoreError::Unavailable);

    let error = client.list_projects(NOW + 3_001).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::DurableState);
    assert_eq!(store.value(), original);
    assert!(client.user().is_none());
}

#[test]
fn transient_refresh_failure_retains_exact_session_and_durable_state() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    // One answer per attempt: the first and every retry the schedule allows.
    for _ in 0..4 {
        transport.push_refresh(503, br#"{"error":{"message":"UNAVAILABLE"}}"#);
    }
    let store = MemoryStore::default();
    let mut client = signed_in(transport.clone(), store.clone());
    let before = store.value();

    let error = client.list_projects(NOW + 3_001).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Transient);
    assert_eq!(error.retries(), 3);
    assert_eq!(transport.count_calls("refresh "), 4);
    assert_eq!(store.value(), before);
    assert_eq!(client.user().unwrap().uid(), "uid-1");
}

#[test]
fn login_input_may_vary_ascii_case_but_canonical_email_and_issuer_are_exact() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = Client::new(
        profile(DeploymentLane::Stable),
        transport,
        MemoryStore::default(),
    );
    let user = client
        .sign_in("User@Example.com", "protected", NOW)
        .unwrap();
    assert_eq!(user.email(), "user@example.com");

    let wrong_issuer = serde_json::json!({
        "localId": "uid-1",
        "email": "user@example.com",
        "idToken": jwt(serde_json::json!({
            "sub": "uid-1",
            "aud": "firebase-project",
            "email": "user@example.com",
            "iss": "https://attacker.example/firebase-project",
            "exp": 2_000_000_000_u64
        })),
        "refreshToken": "refresh-one",
        "expiresIn": "3600"
    })
    .to_string();
    let transport = FixtureTransport::with_sign_in(wrong_issuer.as_bytes());
    let mut client = Client::new(
        profile(DeploymentLane::Stable),
        transport,
        MemoryStore::default(),
    );
    assert_eq!(
        client
            .sign_in("user@example.com", "protected", NOW)
            .unwrap_err()
            .kind(),
        ErrorKind::IdentityMismatch
    );
}

#[test]
fn project_payload_container_parity_is_explicit_and_controls_are_refused() {
    let top_level = br#"[{"eds_project_id":"array-project","lifecycle_state":"active"}]"#;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project(top_level);
    transport.push_project(br#"{"data":{"projects":[]}}"#);
    transport.push_project(br#"{"data":{"projects":[]}}"#);
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client.list_projects(NOW + 1).unwrap().projects()[0].ds_project(),
        "array-project"
    );

    for invalid in [
        br#"{}"#.as_slice(),
        br#"{"data":{}}"#.as_slice(),
        br#"{"data":{"projects":[]},"projects":[]}"#.as_slice(),
        br#"{"error":"refused","data":{"projects":[]}}"#.as_slice(),
        b"{\"data\":{\"projects\":[{\"eds_project_id\":\"bad\\u0001id\"}]}}".as_slice(),
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_project(invalid);
        transport.push_project(br#"{"data":{"projects":[]}}"#);
        transport.push_project(br#"{"data":{"projects":[]}}"#);
        let mut client = signed_in(transport, MemoryStore::default());
        assert_eq!(
            client.list_projects(NOW + 1).unwrap_err().kind(),
            ErrorKind::UnreadableResponse
        );
    }
}

#[test]
fn exclusive_refresh_lease_serializes_two_clients_and_loser_restores() {
    let store = MemoryStore::default();
    let transport_a = FixtureTransport::with_sign_in(SIGN_IN);
    let transport_b = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client_a = signed_in(transport_a.clone(), store.clone());
    let mut client_b = signed_in(transport_b.clone(), store.clone());
    transport_a.push_refresh(200, &refresh_body("refresh-a"));
    transport_b.push_refresh(200, &refresh_body("refresh-b"));
    let barrier = Arc::new(Barrier::new(2));
    let barrier_a = barrier.clone();
    let first = std::thread::spawn(move || {
        barrier_a.wait();
        let result = client_a.refresh(NOW + 3_001);
        (client_a, result)
    });
    let barrier_b = barrier.clone();
    let second = std::thread::spawn(move || {
        barrier_b.wait();
        let result = client_b.refresh(NOW + 3_001);
        (client_b, result)
    });
    let (client_a, result_a) = first.join().unwrap();
    let (client_b, result_b) = second.join().unwrap();
    assert_ne!(result_a.is_ok(), result_b.is_ok());

    let mut loser = if result_a.is_err() {
        client_a
    } else {
        client_b
    };
    let loser_error = if result_a.is_err() {
        result_a
    } else {
        result_b
    }
    .unwrap_err();
    assert_eq!(loser_error.kind(), ErrorKind::Transient);
    assert!(loser.user().is_none());
    assert_eq!(loser.restore(NOW + 3_002).unwrap().unwrap().uid(), "uid-1");
}

#[test]
fn release_failure_overrides_an_operation_error_as_durable_state() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    for _ in 0..4 {
        transport.push_refresh(503, br#"{"error":{"message":"UNAVAILABLE"}}"#);
    }
    let store = MemoryStore::default();
    let mut client = signed_in(transport, store.clone());
    store.fail_next_release(StoreError::Unavailable);

    assert_eq!(
        client.refresh(NOW + 3_001).unwrap_err().kind(),
        ErrorKind::DurableState
    );
}

#[test]
fn explicit_logout_removes_freshly_loaded_winner_not_stale_session_bytes() {
    let store = MemoryStore::default();
    let transport_a = FixtureTransport::with_sign_in(SIGN_IN);
    let transport_b = FixtureTransport::with_sign_in(SIGN_IN);
    let mut stale = signed_in(transport_a, store.clone());
    let mut winner = signed_in(transport_b.clone(), store.clone());
    transport_b.push_refresh(200, &refresh_body("refresh-winner"));
    winner.refresh(NOW + 3_001).unwrap();
    assert!(String::from_utf8(store.value().unwrap())
        .unwrap()
        .contains("refresh-winner"));

    stale.sign_out().unwrap();
    assert!(store.value().is_none());
}

#[test]
fn explicit_logout_is_immediate_when_durable_cleanup_needs_repair() {
    let store = MemoryStore::default();
    let mut client = signed_in(FixtureTransport::with_sign_in(SIGN_IN), store.clone());
    store.fail_next_deletion(StoreError::Unavailable);

    assert_eq!(
        client.sign_out().unwrap_err().kind(),
        ErrorKind::DurableState
    );
    assert!(client.user().is_none());
    assert!(store.value().is_some());
}

fn solar_snapshot_envelope(root: &str, city: &str) -> serde_json::Value {
    let fingerprint = "b".repeat(64);
    let snapshot = serde_json::json!({
        "_root": { "display_name": "City One" },
        "_config": { "paths": { "_meta": { "root": root, "context": city } } },
        "00_templates": {},
        "01_city_inputs": { "city": { "city": "City One" } },
        "_provenance": {
            "schema": 3,
            "source_kind": "authenticated ds-brain desktop snapshot",
            "root": root,
            "context": city,
            "demand_noise_seed": 42,
            "input_base_fingerprint": fingerprint,
        }
    });
    let snapshot_json = serde_json::to_string(&snapshot).unwrap();
    let snapshot_sha256 = format!("{:x}", sha2::Sha256::digest(snapshot_json.as_bytes()));
    serde_json::json!({
        "success": true,
        "message": "Desktop Solar snapshot prepared",
        "data": {
            "snapshot_json": snapshot_json,
            "snapshot_sha256": snapshot_sha256,
            "input_base_fingerprint": fingerprint,
            "media_downloads": [],
            "snapshot_bytes": snapshot_json.len(),
            "source_document_count": 4,
            "firestore_read_time": "2030-03-17T12:00:00Z",
            "snapshot_receipt_id": "123e4567-e89b-42d3-a456-426614174000",
            "snapshot_receipt_expires_at": "2030-03-24T12:00:00Z"
        },
        "timestamp": "2030-03-17T12:00:00Z",
        "service": "data-solutions-backend",
        "version": "1.0.0",
        "request_id": "request-1"
    })
}

#[test]
fn solar_snapshot_is_one_fixed_project_root_call_and_bounded_data_handoff() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let envelope = solar_snapshot_envelope("eds_project/active-project/eds_solar", "city_1");
    transport.push_solar_snapshot(200, &serde_json::to_vec(&envelope).unwrap());
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    let snapshot = client
        .solar_snapshot("active-project", "city_1", NOW + 1)
        .unwrap();
    assert_eq!(snapshot.ds_project(), "active-project");
    assert_eq!(snapshot.template_id(), "city_1");
    assert_eq!(
        snapshot.snapshot_bytes() as u64,
        envelope["data"]["snapshot_bytes"].as_u64().unwrap()
    );
    assert_eq!(snapshot.source_document_count(), 4);
    assert_eq!(snapshot.media_download_count(), 0);
    assert_eq!(
        snapshot.document_json(),
        envelope["data"]["snapshot_json"].as_str().unwrap()
    );
    assert_eq!(snapshot.snapshot_sha256().len(), 64);
    assert_eq!(snapshot.input_base_fingerprint(), "b".repeat(64));
    assert_eq!(
        snapshot.snapshot_receipt_id(),
        "123e4567-e89b-42d3-a456-426614174000"
    );
    let intake: serde_json::Value = serde_json::from_slice(snapshot.intake_bytes()).unwrap();
    assert_eq!(intake, envelope["data"]);
    assert!(transport.calls().iter().any(|call| call ==
        "solar_snapshot method=POST origin=https://eds-gateway-3c0q477h.ue.gateway.dev path=/api/v1/solar content_type=application/json body_exact=true limit=33554432 timeout=120 api_key=true authorization=true forwarded_authorization=true email=true client_id=ds-native-client correlation=true"));
}

#[test]
fn solar_snapshot_profile_input_identity_and_response_fail_closed() {
    for mutate in [
        |input: &mut ClientProfileInput| input.solar_snapshot_method = "GET".to_owned(),
        |input: &mut ClientProfileInput| input.solar_snapshot_path = "/other".to_owned(),
        |input: &mut ClientProfileInput| input.solar_snapshot_action = "calculate".to_owned(),
    ] {
        let mut input = profile_input(DeploymentLane::Stable);
        mutate(&mut input);
        assert_eq!(
            ClientProfile::validate(input).unwrap_err(),
            ProfileError::SolarSnapshotRoute
        );
    }

    for invalid in [
        ("bad project", "city_1"),
        ("active-project", "../city"),
        ("active-project", ""),
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(transport, MemoryStore::default());
        assert_eq!(
            client
                .solar_snapshot(invalid.0, invalid.1, NOW + 1)
                .err()
                .expect("invalid input must be refused")
                .kind(),
            ErrorKind::InvalidInput
        );
    }

    let mut wrong_root = solar_snapshot_envelope("eds_project/other/eds_solar", "city_1");
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_solar_snapshot(200, &serde_json::to_vec(&wrong_root).unwrap());
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client
            .solar_snapshot("active-project", "city_1", NOW + 1)
            .err()
            .expect("cross-project snapshot must be refused")
            .kind(),
        ErrorKind::UnreadableResponse
    );

    wrong_root["unexpected"] = serde_json::json!(true);
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_solar_snapshot(200, &serde_json::to_vec(&wrong_root).unwrap());
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client
            .solar_snapshot("active-project", "city_1", NOW + 1)
            .err()
            .expect("unknown envelope fields must be refused")
            .kind(),
        ErrorKind::UnreadableResponse
    );

    let mut impossible_budget =
        solar_snapshot_envelope("eds_project/active-project/eds_solar", "city_1");
    let media = serde_json::json!({
        "ref": {
            "schema": 1,
            "kind": "solar_network_media",
            "media_id": "123e4567-e89b-42d3-a456-426614174001",
            "logical_role": "network.map:overview",
            "bucket": "solar-media",
            "object": "eds_project/active-project/eds_solar/city_1/network_plots/_immutable/123e4567-e89b-42d3-a456-426614174001",
            "generation": 1,
            "size_bytes": 1,
            "media_type": "image/png",
            "sha256": "c".repeat(64),
            "file_name": "overview.png"
        },
        "download_url": "https://storage.example/signed"
    });
    impossible_budget["data"]["media_downloads"] = serde_json::json!([media.clone(), media]);
    impossible_budget["data"]["source_document_count"] = serde_json::json!(1);
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_solar_snapshot(200, &serde_json::to_vec(&impossible_budget).unwrap());
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client
            .solar_snapshot("active-project", "city_1", NOW + 1)
            .err()
            .expect("media receipts outside the producer document budget must be refused")
            .kind(),
        ErrorKind::UnreadableResponse
    );
}

#[test]
fn solar_snapshot_accepts_outer_json_expansion_above_twelve_mib_within_governed_lane() {
    let mut envelope = solar_snapshot_envelope("eds_project/active-project/eds_solar", "city_1");
    let mut snapshot: serde_json::Value =
        serde_json::from_str(envelope["data"]["snapshot_json"].as_str().unwrap()).unwrap();
    snapshot["_root"]["escaping_fixture"] = serde_json::json!("\"".repeat(7 * 1024 * 1024 / 2));
    let snapshot_json = serde_json::to_string(&snapshot).unwrap();
    assert!(snapshot_json.len() < 8 * 1024 * 1024);
    envelope["data"]["snapshot_sha256"] = serde_json::json!(format!(
        "{:x}",
        sha2::Sha256::digest(snapshot_json.as_bytes())
    ));
    envelope["data"]["snapshot_bytes"] = serde_json::json!(snapshot_json.len());
    envelope["data"]["snapshot_json"] = serde_json::json!(snapshot_json);
    let encoded = serde_json::to_vec(&envelope).unwrap();
    assert!(encoded.len() > 12 * 1024 * 1024);
    assert!(encoded.len() < 32 * 1024 * 1024);

    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_solar_snapshot(200, &encoded);
    let mut client = signed_in(transport, MemoryStore::default());
    assert!(client
        .solar_snapshot("active-project", "city_1", NOW + 1)
        .is_ok());
}

fn survey_query_request() -> SurveyQueryRequest {
    SurveyQueryRequest::new(
        "a_poles",
        SurveyQueryMetric::CountDistinct,
        Some("created_by".to_owned()),
        vec!["created_by".to_owned()],
        vec![SurveyQueryFilter::eq("created_by", "user@example.com")],
        SurveyQueryOrder::Desc,
        20,
    )
    .unwrap()
}

fn survey_query_envelope() -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "message": "Survey query",
        "data": {
            "form": "a_poles",
            "metric": "count_distinct",
            "group_by": ["created_by"],
            "rows": [{ "groups": ["user@example.com"], "value": 1 }],
            "truncated": false,
            "bytes_processed": 4096
        },
        "timestamp": "2030-03-17T12:00:00Z",
        "service": "data-solutions-backend",
        "version": "1.0.0",
        "request_id": "request-1"
    })
}

#[test]
fn survey_query_is_one_fixed_bounded_call_and_preserves_created_by() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_survey_query(200, &serde_json::to_vec(&survey_query_envelope()).unwrap());
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    let result = client
        .survey_query("active-project", &survey_query_request(), NOW + 1)
        .unwrap();
    assert_eq!(result.form(), "a_poles");
    assert_eq!(result.metric(), SurveyQueryMetric::CountDistinct);
    assert_eq!(result.group_by(), ["created_by"]);
    assert_eq!(result.rows()[0].groups(), ["user@example.com"]);
    assert_eq!(result.rows()[0].value(), 1);
    assert_eq!(result.bytes_processed(), 4096);
    assert!(!result.truncated());
    assert!(transport.calls().iter().any(|call| call ==
        "survey_query method=POST origin=https://eds-gateway-3c0q477h.ue.gateway.dev path=/api/v1/survey/query content_type=application/json body_exact=true limit=1048576 timeout=120 api_key=true authorization=true forwarded_authorization=true email=true client_id=ds-native-client correlation=true"));
}

#[test]
fn survey_query_preserves_each_shared_form_authority_code() {
    for (status, wire, expected) in [
        (
            403,
            "SURVEY_PROJECT_ACCESS_DENIED",
            SurveyFormReadServiceCode::ProjectAccessDenied,
        ),
        (
            403,
            "SURVEY_FORM_ACCESS_DENIED",
            SurveyFormReadServiceCode::FormAccessDenied,
        ),
        (
            404,
            "SURVEY_FORM_BINDING_NOT_FOUND",
            SurveyFormReadServiceCode::FormBindingNotFound,
        ),
        (
            409,
            "SURVEY_FORM_NOT_PARTICIPATING",
            SurveyFormReadServiceCode::FormNotParticipating,
        ),
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_survey_query(status, &survey_entries_select_error_envelope(wire));
        let mut client = signed_in(transport, MemoryStore::default());
        let error = client
            .survey_query("active-project", &survey_query_request(), NOW + 1)
            .unwrap_err();
        assert_eq!(error.survey_form_read_service_code(), Some(expected));
        assert!(!error.to_string().contains("server-secret"));
    }
}

#[test]
fn survey_query_preserves_missing_view_and_other_exact_service_outcomes() {
    use ds_client_core::SurveyQueryServiceCode;
    for (status, wire, expected) in [
        (
            404,
            "SURVEY_FORM_UNKNOWN",
            SurveyQueryServiceCode::FormUnknown,
        ),
        (
            400,
            "SURVEY_FIELD_UNKNOWN",
            SurveyQueryServiceCode::FieldUnknown,
        ),
        (409, "SURVEY_VIEW_STALE", SurveyQueryServiceCode::ViewStale),
        (
            400,
            "SURVEY_QUERY_TOO_EXPENSIVE",
            SurveyQueryServiceCode::TooExpensive,
        ),
        (
            503,
            "SURVEY_QUERY_SYNC_FAILED",
            SurveyQueryServiceCode::SyncFailed,
        ),
        (
            503,
            "SURVEY_QUERY_UNAVAILABLE",
            SurveyQueryServiceCode::Unavailable,
        ),
        (
            404,
            "SURVEY_SCOPE_NOT_FOUND",
            SurveyQueryServiceCode::ScopeNotFound,
        ),
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_survey_query(status, &survey_entries_select_error_envelope(wire));
        let mut client = signed_in(transport, MemoryStore::default());
        let error = client
            .survey_query("active-project", &survey_query_request(), NOW + 1)
            .unwrap_err();
        assert_eq!(error.survey_query_service_code(), Some(expected));
        assert_eq!(error.survey_form_read_service_code(), None);
    }
}

#[test]
fn survey_reads_separate_an_unpublished_route_from_an_invisible_scope() {
    // API Gateway answers its own 404 for a path its spec was never taught,
    // before ds-brain is reached. All three governed Survey read paths were in
    // exactly that state on both lanes, and the core reported every one of them
    // as `the selected project or survey form is not available to this user` —
    // an authority sentence about a route that does not exist.
    let gateway_404 =
        br#"{"message":"The current request is not defined by this API.","code":404}"#;

    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_survey_query(404, gateway_404);
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client
            .survey_query("active-project", &survey_query_request(), NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::RouteUnavailable
    );

    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_survey_entries_select(404, gateway_404);
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client
            .survey_entries_select("active-project", &survey_entries_select_request(), NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::RouteUnavailable
    );

    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_survey_entries_changes(404, gateway_404);
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client
            .survey_entries_changes("active-project", &survey_entries_changes_request(), NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::RouteUnavailable
    );

    // And the converse: ds-brain's own 404 is still a fact about the caller's
    // scope — with a code this core does not type, and with a code that is not
    // even well formed, because a malformed answer from the service is still
    // the service and not a missing route.
    for body in [
        survey_entries_select_error_envelope("NOT_FOUND"),
        serde_json::to_vec(&serde_json::json!({
            "success": false,
            "error": { "code": 42, "message": "Project or form not found" }
        }))
        .unwrap(),
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_survey_query(404, &body);
        let mut client = signed_in(transport, MemoryStore::default());
        assert_eq!(
            client
                .survey_query("active-project", &survey_query_request(), NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::ResourceNotFound
        );
    }
}

#[test]
fn survey_query_concurrent_clients_keep_explicit_project_context_after_refusal() {
    let workers = ["project-a", "project-b"].map(|project| {
        std::thread::spawn(move || {
            let transport = FixtureTransport::with_sign_in(SIGN_IN);
            transport.push_survey_query(
                403,
                &survey_entries_select_error_envelope("SURVEY_PROJECT_ACCESS_DENIED"),
            );
            transport
                .push_survey_query(200, &serde_json::to_vec(&survey_query_envelope()).unwrap());
            let mut client = signed_in(transport.clone(), MemoryStore::default());
            let denied = client
                .survey_query("denied-project", &survey_query_request(), NOW + 1)
                .unwrap_err();
            assert_eq!(
                denied.survey_form_read_service_code(),
                Some(SurveyFormReadServiceCode::ProjectAccessDenied)
            );
            client
                .survey_query(project, &survey_query_request(), NOW + 2)
                .unwrap();
            assert_eq!(
                transport.0.lock().unwrap().survey_query_projects,
                ["denied-project", project]
            );
        })
    });
    for worker in workers {
        worker.join().unwrap();
    }
}

#[test]
fn survey_query_profile_and_typed_grammar_fail_closed() {
    for mutate in [
        |input: &mut ClientProfileInput| input.survey_query_method = "GET".to_owned(),
        |input: &mut ClientProfileInput| input.survey_query_path = "/other".to_owned(),
    ] {
        let mut input = profile_input(DeploymentLane::Stable);
        mutate(&mut input);
        assert_eq!(
            ClientProfile::validate(input).unwrap_err(),
            ProfileError::SurveyQueryRoute
        );
    }

    for request in [
        SurveyQueryRequest::new(
            "A-POLES",
            SurveyQueryMetric::Count,
            None,
            vec![],
            vec![],
            SurveyQueryOrder::Asc,
            20,
        ),
        SurveyQueryRequest::new(
            "a_poles",
            SurveyQueryMetric::Count,
            Some("created_by".to_owned()),
            vec![],
            vec![],
            SurveyQueryOrder::Asc,
            20,
        ),
        SurveyQueryRequest::new(
            "a_poles",
            SurveyQueryMetric::CountDistinct,
            None,
            vec![],
            vec![],
            SurveyQueryOrder::Asc,
            20,
        ),
        SurveyQueryRequest::new(
            "a_poles",
            SurveyQueryMetric::Count,
            None,
            vec!["one".to_owned(), "two".to_owned(), "three".to_owned()],
            vec![],
            SurveyQueryOrder::Asc,
            20,
        ),
        SurveyQueryRequest::new(
            "a_poles",
            SurveyQueryMetric::Count,
            None,
            vec![],
            vec![SurveyQueryFilter::in_values("created_by", vec![])],
            SurveyQueryOrder::Asc,
            20,
        ),
        SurveyQueryRequest::new(
            "a_poles",
            SurveyQueryMetric::Count,
            None,
            vec![],
            vec![],
            SurveyQueryOrder::Asc,
            0,
        ),
    ] {
        assert_eq!(request.unwrap_err().kind(), ErrorKind::InvalidInput);
    }
}

#[test]
fn survey_query_response_identity_shape_and_bounds_fail_closed() {
    for mutate in [
        |body: &mut serde_json::Value| body["data"]["form"] = serde_json::json!("other_form"),
        |body: &mut serde_json::Value| body["data"]["metric"] = serde_json::json!("count"),
        |body: &mut serde_json::Value| body["data"]["group_by"] = serde_json::json!([]),
        |body: &mut serde_json::Value| body["data"]["rows"][0]["groups"] = serde_json::json!([]),
        |body: &mut serde_json::Value| body["data"]["surprise"] = serde_json::json!(true),
        |body: &mut serde_json::Value| body["data"]["bytes_processed"] = serde_json::json!(-1),
        |body: &mut serde_json::Value| body["data"]["rows"][0]["value"] = serde_json::json!(-1),
    ] {
        let mut body = survey_query_envelope();
        mutate(&mut body);
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_survey_query(200, &serde_json::to_vec(&body).unwrap());
        let mut client = signed_in(transport, MemoryStore::default());
        assert_eq!(
            client
                .survey_query("active-project", &survey_query_request(), NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }

    let mut body = survey_query_envelope();
    body["data"]["rows"] = serde_json::json!((0..21)
        .map(|_| serde_json::json!({"groups": ["user@example.com"], "value": 1}))
        .collect::<Vec<_>>());
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_survey_query(200, &serde_json::to_vec(&body).unwrap());
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client
            .survey_query("active-project", &survey_query_request(), NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
}

fn survey_entries_select_request() -> SurveyEntriesSelectRequest {
    SurveyEntriesSelectRequest::new("survey_points", [29.0, -2.0, 30.0, -1.0], Some(2)).unwrap()
}

fn survey_entries_select_envelope() -> Vec<u8> {
    br#"{"success":true,"message":"Survey entry selection","data":{"project":"project_a","form":"survey_points","bbox":[29.0,-2.0,30.0,-1.0],"rows":[{"doc_id":"a","geometry":{"type":"Point","coordinates":[29.1,-1.1]},"created_by":"a@example.test","firestore_updated_at":"2026-08-30T01:00:00Z"},{"doc_id":"b","geometry":{"type":"Point","coordinates":[29.2,-1.2]},"created_by":"b@example.test","firestore_updated_at":"2026-08-30T02:00:00Z"}],"truncated":true,"complete":false,"selection_digest":"9a1f4cf3f07661d30aa892fd2a005e5c4a3506a46e0cc8d06177d8f4d087c5f5","consistency":{"source":"survey_mirror","sync":"ensure_synced_all","snapshot":false,"mutable":true}},"timestamp":"2030-03-17T12:00:00Z","service":"data-solutions-backend","version":"1.0.0","request_id":"request-1"}"#.to_vec()
}

fn mutate_survey_entries_select_envelope(from: &str, to: &str) -> Vec<u8> {
    let source = String::from_utf8(survey_entries_select_envelope()).unwrap();
    let from = from.replace(r#"\""#, r#"""#);
    let to = to.replace(r#"\""#, r#"""#);
    assert_eq!(source.matches(&from).count(), 1, "mutation must be exact");
    source.replacen(&from, &to, 1).into_bytes()
}

#[test]
fn survey_entries_select_is_one_fixed_bounded_call_and_verifies_backend_digest() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_survey_entries_select(200, &survey_entries_select_envelope());
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    let result = client
        .survey_entries_select("project_a", &survey_entries_select_request(), NOW + 1)
        .unwrap();
    assert_eq!(result.project(), "project_a");
    assert_eq!(result.form(), "survey_points");
    assert_eq!(result.bbox(), [29.0, -2.0, 30.0, -1.0]);
    assert_eq!(result.rows()[0].doc_id(), "a");
    assert_eq!(
        result.rows()[0].geometry_json(),
        r#"{"type":"Point","coordinates":[29.1,-1.1]}"#
    );
    assert_eq!(result.rows()[0].created_by(), "a@example.test");
    assert_eq!(
        result.rows()[0].firestore_updated_at(),
        "2026-08-30T01:00:00Z"
    );
    assert!(result.truncated());
    assert!(!result.complete());
    assert_eq!(result.consistency().source(), "survey_mirror");
    assert_eq!(result.consistency().sync(), "ensure_synced_all");
    assert!(!result.consistency().snapshot());
    assert!(result.consistency().mutable());
    assert_eq!(
        result.selection_digest(),
        "9a1f4cf3f07661d30aa892fd2a005e5c4a3506a46e0cc8d06177d8f4d087c5f5"
    );
    assert!(transport.calls().iter().any(|call| call ==
        "survey_entries_select method=POST origin=https://eds-gateway-3c0q477h.ue.gateway.dev path=/api/v1/survey/entries/select content_type=application/json body_exact=true limit=1048576 timeout=120 api_key=true authorization=true forwarded_authorization=true email=true client_id=ds-native-client correlation=true"));
}

#[test]
fn survey_entries_select_profile_and_typed_grammar_fail_closed() {
    for mutate in [
        |input: &mut ClientProfileInput| {
            input.survey_entries_select_method = "GET".to_owned();
        },
        |input: &mut ClientProfileInput| {
            input.survey_entries_select_path = "/other".to_owned();
        },
    ] {
        let mut input = profile_input(DeploymentLane::Stable);
        mutate(&mut input);
        assert_eq!(
            ClientProfile::validate(input).unwrap_err(),
            ProfileError::SurveyEntriesSelectRoute
        );
    }

    for request in [
        SurveyEntriesSelectRequest::new("SURVEY-POINTS", [29.0, -2.0, 30.0, -1.0], None),
        SurveyEntriesSelectRequest::new("survey_points", [f64::NAN, -2.0, 30.0, -1.0], None),
        SurveyEntriesSelectRequest::new("survey_points", [-181.0, -2.0, 30.0, -1.0], None),
        SurveyEntriesSelectRequest::new("survey_points", [30.0, -2.0, 29.0, -1.0], None),
        SurveyEntriesSelectRequest::new("survey_points", [29.0, -1.0, 30.0, -2.0], None),
        SurveyEntriesSelectRequest::new("survey_points", [29.0, -2.0, 30.0, -1.0], Some(0)),
        SurveyEntriesSelectRequest::new("survey_points", [29.0, -2.0, 30.0, -1.0], Some(501)),
    ] {
        assert_eq!(request.unwrap_err().kind(), ErrorKind::InvalidInput);
    }
    let defaulted =
        SurveyEntriesSelectRequest::new("survey_points", [-0.0, -2.0, 30.0, -1.0], None).unwrap();
    assert_eq!(defaulted.limit(), 100);
    assert_eq!(defaulted.bbox()[0].to_bits(), 0.0_f64.to_bits());
}

#[test]
fn survey_entries_select_response_identity_consistency_and_digest_fail_closed() {
    let mutations = [
        (
            r#"\"project\":\"project_a\""#,
            r#"\"project\":\"other-project\""#,
        ),
        (
            r#"\"form\":\"survey_points\""#,
            r#"\"form\":\"other_form\""#,
        ),
        (
            r#"\"bbox\":[29.0,-2.0,30.0,-1.0]"#,
            r#"\"bbox\":[28.0,-2.0,30.0,-1.0]"#,
        ),
        (
            r#"\"source\":\"survey_mirror\""#,
            r#"\"source\":\"firestore\""#,
        ),
        (
            r#"\"sync\":\"ensure_synced_all\""#,
            r#"\"sync\":\"best_effort\""#,
        ),
        (r#"\"snapshot\":false"#, r#"\"snapshot\":true"#),
        (r#"\"mutable\":true"#, r#"\"mutable\":false"#),
        (r#"\"complete\":false"#, r#"\"complete\":true"#),
        (
            "9a1f4cf3f07661d30aa892fd2a005e5c4a3506a46e0cc8d06177d8f4d087c5f5",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ),
        (
            r#"\"created_by\":\"a@example.test\""#,
            r#"\"created_by\":\"changed@example.test\""#,
        ),
        (
            r#"{\"doc_id\":\"a\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[29.1,-1.1]},\"created_by\":\"a@example.test\",\"firestore_updated_at\":\"2026-08-30T01:00:00Z\"},{\"doc_id\":\"b\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[29.2,-1.2]},\"created_by\":\"b@example.test\",\"firestore_updated_at\":\"2026-08-30T02:00:00Z\"}"#,
            r#"{\"doc_id\":\"b\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[29.2,-1.2]},\"created_by\":\"b@example.test\",\"firestore_updated_at\":\"2026-08-30T02:00:00Z\"},{\"doc_id\":\"a\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[29.1,-1.1]},\"created_by\":\"a@example.test\",\"firestore_updated_at\":\"2026-08-30T01:00:00Z\"}"#,
        ),
        (r#"\"doc_id\":\"b\""#, r#"\"doc_id\":\"a\""#),
        ("2026-08-30T01:00:00Z", "2026-08-30T03:00:00+02:00"),
        (
            r#"\"coordinates\":[29.1,-1.1]}"#,
            r#"\"coordinates\":[29.1,-1.1],\"extra\":true}"#,
        ),
        (
            r#"},\"timestamp\":"#,
            r#",\"surprise\":true},\"timestamp\":"#,
        ),
    ];
    for (from, to) in mutations {
        let body = mutate_survey_entries_select_envelope(from, to);
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_survey_entries_select(200, &body);
        let mut client = signed_in(transport, MemoryStore::default());
        assert_eq!(
            client
                .survey_entries_select("project_a", &survey_entries_select_request(), NOW + 1,)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }
}

#[test]
fn survey_entries_select_rejects_oversize_rows_and_inconsistent_completeness() {
    let too_many = mutate_survey_entries_select_envelope(
        r#"],\"truncated\":true"#,
        r#",{\"doc_id\":\"c\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[29.3,-1.3]},\"created_by\":\"c@example.test\",\"firestore_updated_at\":\"2026-08-30T03:00:00Z\"}],\"truncated\":true"#,
    );
    let short_truncated = mutate_survey_entries_select_envelope(
        r#",{\"doc_id\":\"b\",\"geometry\":{\"type\":\"Point\",\"coordinates\":[29.2,-1.2]},\"created_by\":\"b@example.test\",\"firestore_updated_at\":\"2026-08-30T02:00:00Z\"}"#,
        "",
    );
    for body in [too_many, short_truncated] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_survey_entries_select(200, &body);
        let mut client = signed_in(transport, MemoryStore::default());
        assert_eq!(
            client
                .survey_entries_select("project_a", &survey_entries_select_request(), NOW + 1,)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }
}

fn survey_entries_select_error_envelope(code: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "success": false,
        "error": {
            "code": code,
            "message": "password=server-secret",
            "details": {"token": "detail-secret"},
            "timestamp": 1_777_777_777
        },
        "timestamp": "2026-08-30T03:00:00Z",
        "service": "data-solutions-backend",
        "version": "1.0.0",
        "request_id": "request-typed-error"
    }))
    .unwrap()
}

#[test]
fn survey_entries_select_maps_each_exact_service_code_without_exposing_backend_text() {
    for (status, wire, code, kind) in [
        (
            400,
            "SURVEY_ENTRIES_SELECT_TOO_EXPENSIVE",
            SurveyEntriesSelectServiceCode::TooExpensive,
            ErrorKind::InvalidInput,
        ),
        (
            413,
            "SURVEY_ENTRIES_SELECT_TOO_LARGE",
            SurveyEntriesSelectServiceCode::TooLarge,
            ErrorKind::InvalidInput,
        ),
        (
            503,
            "SURVEY_ENTRIES_SELECT_SYNC_FAILED",
            SurveyEntriesSelectServiceCode::SyncFailed,
            ErrorKind::Transient,
        ),
        (
            503,
            "SURVEY_ENTRIES_SELECT_MIRROR_INVALID",
            SurveyEntriesSelectServiceCode::MirrorInvalid,
            ErrorKind::UnreadableResponse,
        ),
        (
            400,
            "SURVEY_ENTRIES_SELECT_INVALID",
            SurveyEntriesSelectServiceCode::Invalid,
            ErrorKind::InvalidInput,
        ),
        (
            503,
            "SURVEY_ENTRIES_SELECT_UNAVAILABLE",
            SurveyEntriesSelectServiceCode::Unavailable,
            ErrorKind::Transient,
        ),
        (
            503,
            "SURVEY_ENTRIES_SELECT_FAILED",
            SurveyEntriesSelectServiceCode::Failed,
            ErrorKind::Transient,
        ),
        (
            404,
            "SURVEY_ENTRIES_SCOPE_NOT_FOUND",
            SurveyEntriesSelectServiceCode::ScopeNotFound,
            ErrorKind::ResourceNotFound,
        ),
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_survey_entries_select(status, &survey_entries_select_error_envelope(wire));
        let mut client = signed_in(transport, MemoryStore::default());
        let error = client
            .survey_entries_select("project_a", &survey_entries_select_request(), NOW + 1)
            .unwrap_err();
        assert_eq!(error.kind(), kind, "wire code {wire}");
        assert_eq!(error.survey_entries_select_service_code(), Some(code));
        assert!(!error.to_string().contains("server-secret"));
        assert!(!format!("{error:?}").contains("detail-secret"));
    }
}

#[test]
fn survey_entries_select_malformed_oversize_and_contradictory_errors_use_status_fallback() {
    let standard: serde_json::Value = serde_json::from_slice(
        &survey_entries_select_error_envelope("SURVEY_ENTRIES_SELECT_SYNC_FAILED"),
    )
    .unwrap();
    let mut unknown_field = standard.clone();
    unknown_field["unexpected"] = serde_json::json!(true);
    let mut success_true = standard.clone();
    success_true["success"] = serde_json::json!(true);
    let mut wrong_service = standard.clone();
    wrong_service["service"] = serde_json::json!("other-service");
    let mut wrong_version = standard.clone();
    wrong_version["version"] = serde_json::json!("2.0.0");
    let mut unbounded_message = standard;
    unbounded_message["error"]["message"] = serde_json::json!("x".repeat(16 * 1024 + 1));
    let malformed_code = serde_json::json!({
        "success": false,
        "error": {
            "code": 42,
            "message": "password=malformed-secret",
            "timestamp": 1_777_777_777
        },
        "timestamp": "2026-08-30T03:00:00Z",
        "service": "data-solutions-backend",
        "version": "1.0.0"
    });
    let cases = vec![
        (503, b"not-json".to_vec(), ErrorKind::Transient),
        (
            503,
            serde_json::to_vec(&unknown_field).unwrap(),
            ErrorKind::Transient,
        ),
        (
            503,
            serde_json::to_vec(&success_true).unwrap(),
            ErrorKind::Transient,
        ),
        (
            503,
            serde_json::to_vec(&wrong_service).unwrap(),
            ErrorKind::Transient,
        ),
        (
            503,
            serde_json::to_vec(&wrong_version).unwrap(),
            ErrorKind::Transient,
        ),
        (
            503,
            serde_json::to_vec(&unbounded_message).unwrap(),
            ErrorKind::Transient,
        ),
        (
            503,
            serde_json::to_vec(&malformed_code).unwrap(),
            ErrorKind::Transient,
        ),
        (
            400,
            survey_entries_select_error_envelope("SURVEY_ENTRIES_SELECT_MIRROR_INVALID"),
            ErrorKind::InvalidInput,
        ),
        (
            503,
            survey_entries_select_error_envelope("SURVEY_ENTRIES_SELECT_TOO_LARGE"),
            ErrorKind::Transient,
        ),
        (413, vec![b'x'; 1024 * 1024 + 1], ErrorKind::InvalidInput),
    ];
    for (status, body, kind) in cases {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_survey_entries_select(status, &body);
        let mut client = signed_in(transport, MemoryStore::default());
        let error = client
            .survey_entries_select("project_a", &survey_entries_select_request(), NOW + 1)
            .unwrap_err();
        assert_eq!(error.kind(), kind);
        assert_eq!(error.survey_entries_select_service_code(), None);
        assert!(!error.to_string().contains("malformed-secret"));
    }
}

fn survey_entries_changes_request() -> SurveyEntriesChangesRequest {
    SurveyEntriesChangesRequest::new("survey_points", "2026-08-30T00:00:00Z", Some(2), None)
        .unwrap()
}

fn survey_entries_changes_envelope(has_more: bool) -> Vec<u8> {
    let mut data = serde_json::json!({
        "project": "project_a",
        "form": "survey_points",
        "updated_after": "2026-08-30T00:00:00Z",
        "upper_fence": "2026-08-30T01:00:00Z",
        "rows": [
            {
                "doc_id": "entry_a",
                "created_by": "user@example.com",
                "is_deleted": true,
                "firestore_updated_at": "2026-08-30T00:15:00Z"
            },
            {
                "doc_id": "entry_b",
                "geometry": {"type":"Point","coordinates":[29.5,-1.5]},
                "created_by": "",
                "is_deleted": false,
                "firestore_updated_at": "2026-08-30T00:15:00.12Z"
            }
        ],
        "has_more": has_more,
        "complete": !has_more,
        "consistency": {
            "source": "survey_mirror",
            "sync": "ensure_synced_all",
            "fence": "bigquery_system_time",
            "firestore_snapshot": false,
            "immutable_page_fence": true
        }
    });
    if has_more {
        data["next_cursor"] = serde_json::json!("opaque.ABC_123");
    }
    serde_json::to_vec(&serde_json::json!({
        "success": true,
        "message": "Survey entries changes",
        "data": data,
        "timestamp": "2026-08-30T01:00:01Z",
        "service": "data-solutions-backend",
        "version": "1.0.0",
        "request_id": "request-1"
    }))
    .unwrap()
}

fn mutate_survey_entries_changes_data(mutator: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut value: serde_json::Value =
        serde_json::from_slice(&survey_entries_changes_envelope(true)).unwrap();
    mutator(&mut value["data"]);
    serde_json::to_vec(&value).unwrap()
}

#[test]
fn survey_entries_changes_is_one_fixed_fenced_call_and_exposes_tombstones() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_survey_entries_changes(200, &survey_entries_changes_envelope(true));
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let result = client
        .survey_entries_changes("project_a", &survey_entries_changes_request(), NOW + 1)
        .unwrap();

    assert_eq!(result.project(), "project_a");
    assert_eq!(result.form(), "survey_points");
    assert_eq!(result.updated_after(), "2026-08-30T00:00:00Z");
    assert_eq!(result.upper_fence(), "2026-08-30T01:00:00Z");
    assert!(result.has_more());
    assert!(!result.complete());
    assert_eq!(result.next_cursor(), Some("opaque.ABC_123"));
    assert_eq!(result.rows().len(), 2);
    assert!(result.rows()[0].is_deleted());
    assert_eq!(result.rows()[0].geometry_json(), None);
    assert!(!result.rows()[1].is_deleted());
    assert_eq!(
        result.rows()[1].geometry_json(),
        Some(r#"{"type":"Point","coordinates":[29.5,-1.5]}"#)
    );
    assert_eq!(result.consistency().source(), "survey_mirror");
    assert_eq!(result.consistency().sync(), "ensure_synced_all");
    assert_eq!(result.consistency().fence(), "bigquery_system_time");
    assert!(!result.consistency().firestore_snapshot());
    assert!(result.consistency().immutable_page_fence());
    assert!(transport.calls().iter().any(|call| call ==
        "survey_entries_changes method=POST origin=https://eds-gateway-3c0q477h.ue.gateway.dev path=/api/v1/survey/entries/changes content_type=application/json body_exact=true limit=1048576 timeout=120 api_key=true authorization=true forwarded_authorization=true email=true client_id=ds-native-client correlation=true"));
}

#[test]
fn survey_entries_changes_request_is_closed_and_normalizes_go_rfc3339_nano() {
    let normalized = SurveyEntriesChangesRequest::new(
        "survey_points",
        "2026-08-30T02:00:00.120000000+02:00",
        None,
        None,
    )
    .unwrap();
    assert_eq!(normalized.updated_after(), "2026-08-30T00:00:00.12Z");
    assert_eq!(normalized.limit(), 100);
    for refused in [
        SurveyEntriesChangesRequest::new("SURVEY-POINTS", "2026-08-30T00:00:00Z", None, None),
        SurveyEntriesChangesRequest::new("survey_points", "not-a-time", None, None),
        SurveyEntriesChangesRequest::new("survey_points", "2026-08-30T00:00:00Z", Some(0), None),
        SurveyEntriesChangesRequest::new("survey_points", "2026-08-30T00:00:00Z", Some(501), None),
        SurveyEntriesChangesRequest::new(
            "survey_points",
            "2026-08-30T00:00:00Z",
            None,
            Some(String::new()),
        ),
        SurveyEntriesChangesRequest::new(
            "survey_points",
            "2026-08-30T00:00:00Z",
            None,
            Some("white space".to_owned()),
        ),
        SurveyEntriesChangesRequest::new(
            "survey_points",
            "2026-08-30T00:00:00Z",
            None,
            Some("x".repeat(4097)),
        ),
    ] {
        assert_eq!(refused.unwrap_err().kind(), ErrorKind::InvalidInput);
    }
}

#[test]
fn survey_entries_changes_profile_identity_and_page_contract_fail_closed() {
    for mutate in [
        |input: &mut ClientProfileInput| input.survey_entries_changes_method = "GET".to_owned(),
        |input: &mut ClientProfileInput| input.survey_entries_changes_path = "/other".to_owned(),
    ] {
        let mut input = profile_input(DeploymentLane::Stable);
        mutate(&mut input);
        assert_eq!(
            ClientProfile::validate(input).unwrap_err(),
            ProfileError::SurveyEntriesChangesRoute
        );
    }

    let cases = vec![
        mutate_survey_entries_changes_data(|data| data["project"] = serde_json::json!("other")),
        mutate_survey_entries_changes_data(|data| data["form"] = serde_json::json!("other")),
        mutate_survey_entries_changes_data(|data| {
            data["updated_after"] = serde_json::json!("2026-08-30T00:00:01Z")
        }),
        mutate_survey_entries_changes_data(|data| {
            data["upper_fence"] = serde_json::json!("2026-08-29T23:59:59Z")
        }),
        mutate_survey_entries_changes_data(|data| {
            data["rows"][0]["firestore_updated_at"] = serde_json::json!("2026-08-29T23:59:59Z")
        }),
        mutate_survey_entries_changes_data(|data| {
            data["rows"][1]["firestore_updated_at"] = serde_json::json!("2026-08-30T01:00:01Z")
        }),
        mutate_survey_entries_changes_data(|data| {
            data["rows"][1]["doc_id"] = serde_json::json!("entry_0");
            data["rows"][1]["firestore_updated_at"] = serde_json::json!("2026-08-30T00:15:00Z");
        }),
        mutate_survey_entries_changes_data(|data| {
            let row = data["rows"][0].clone();
            data["rows"].as_array_mut().unwrap().push(row);
        }),
        mutate_survey_entries_changes_data(|data| data["complete"] = serde_json::json!(true)),
        mutate_survey_entries_changes_data(|data| {
            data["rows"].as_array_mut().unwrap().pop();
        }),
        mutate_survey_entries_changes_data(|data| {
            data.as_object_mut().unwrap().remove("next_cursor");
        }),
        mutate_survey_entries_changes_data(|data| data["next_cursor"] = serde_json::Value::Null),
        mutate_survey_entries_changes_data(|data| {
            data["next_cursor"] = serde_json::json!("white space")
        }),
        mutate_survey_entries_changes_data(|data| {
            data["next_cursor"] = serde_json::json!("x".repeat(4097))
        }),
        mutate_survey_entries_changes_data(|data| {
            data["consistency"]["firestore_snapshot"] = serde_json::json!(true)
        }),
        mutate_survey_entries_changes_data(|data| {
            data["consistency"]["immutable_page_fence"] = serde_json::json!(false)
        }),
        mutate_survey_entries_changes_data(|data| {
            data["rows"][0]["geometry"] = serde_json::Value::Null
        }),
        mutate_survey_entries_changes_data(|data| {
            data["rows"][1]["geometry"] = serde_json::json!({"type":"Feature","geometry":null})
        }),
        mutate_survey_entries_changes_data(|data| {
            data["rows"][1]["firestore_updated_at"] = serde_json::json!("2026-08-30T00:15:00.120Z")
        }),
        mutate_survey_entries_changes_data(|data| {
            data["rows"][0]["doc_id"] = serde_json::json!("x".repeat(513))
        }),
    ];
    for body in cases {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_survey_entries_changes(200, &body);
        let mut client = signed_in(transport, MemoryStore::default());
        assert_eq!(
            client
                .survey_entries_changes("project_a", &survey_entries_changes_request(), NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }

    let complete_with_null_cursor = mutate_survey_entries_changes_data(|data| {
        data["has_more"] = serde_json::json!(false);
        data["complete"] = serde_json::json!(true);
        data["next_cursor"] = serde_json::Value::Null;
    });
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_survey_entries_changes(200, &complete_with_null_cursor);
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client
            .survey_entries_changes("project_a", &survey_entries_changes_request(), NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
}

fn survey_entries_changes_error_envelope(code: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "success": false,
        "error": {
            "code": code,
            "message": "password=backend-secret-must-not-cross",
            "details": {"token": "backend-secret-must-not-cross"},
            "timestamp": 1_777_777_777
        },
        "timestamp": "2026-08-30T03:00:00Z",
        "service": "data-solutions-backend",
        "version": "1.0.0",
        "request_id": "request-1"
    }))
    .unwrap()
}

#[test]
fn survey_entries_changes_maps_only_exact_status_code_pairs_without_backend_text() {
    let cases = [
        (
            400,
            "SURVEY_ENTRIES_CHANGES_INVALID",
            SurveyEntriesChangesServiceCode::Invalid,
            ErrorKind::InvalidInput,
        ),
        (
            400,
            "SURVEY_ENTRIES_CHANGES_CURSOR_INVALID",
            SurveyEntriesChangesServiceCode::CursorInvalid,
            ErrorKind::InvalidInput,
        ),
        (
            410,
            "SURVEY_ENTRIES_CHANGES_FENCE_EXPIRED",
            SurveyEntriesChangesServiceCode::FenceExpired,
            ErrorKind::InvalidInput,
        ),
        (
            400,
            "SURVEY_ENTRIES_CHANGES_TOO_EXPENSIVE",
            SurveyEntriesChangesServiceCode::TooExpensive,
            ErrorKind::InvalidInput,
        ),
        (
            413,
            "SURVEY_ENTRIES_CHANGES_TOO_LARGE",
            SurveyEntriesChangesServiceCode::TooLarge,
            ErrorKind::InvalidInput,
        ),
        (
            503,
            "SURVEY_ENTRIES_CHANGES_MIRROR_INVALID",
            SurveyEntriesChangesServiceCode::MirrorInvalid,
            ErrorKind::UnreadableResponse,
        ),
        (
            503,
            "SURVEY_ENTRIES_CHANGES_SNAPSHOT_UNAVAILABLE",
            SurveyEntriesChangesServiceCode::SnapshotUnavailable,
            ErrorKind::Transient,
        ),
        (
            503,
            "SURVEY_ENTRIES_CHANGES_UNAVAILABLE",
            SurveyEntriesChangesServiceCode::Unavailable,
            ErrorKind::Transient,
        ),
        (
            503,
            "SURVEY_ENTRIES_CHANGES_SYNC_FAILED",
            SurveyEntriesChangesServiceCode::SyncFailed,
            ErrorKind::Transient,
        ),
        (
            503,
            "SURVEY_ENTRIES_CHANGES_FAILED",
            SurveyEntriesChangesServiceCode::Failed,
            ErrorKind::Transient,
        ),
        (
            404,
            "SURVEY_ENTRIES_SCOPE_NOT_FOUND",
            SurveyEntriesChangesServiceCode::ScopeNotFound,
            ErrorKind::ResourceNotFound,
        ),
    ];
    for (status, wire, code, kind) in cases {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_survey_entries_changes(status, &survey_entries_changes_error_envelope(wire));
        let mut client = signed_in(transport, MemoryStore::default());
        let error = client
            .survey_entries_changes("project_a", &survey_entries_changes_request(), NOW + 1)
            .unwrap_err();
        assert_eq!(error.kind(), kind);
        assert_eq!(error.survey_entries_changes_service_code(), Some(code));
        assert_eq!(error.survey_entries_select_service_code(), None);
        assert!(!error.to_string().contains("backend-secret"));
    }
}

#[test]
fn survey_entries_changes_malformed_unknown_contradictory_and_oversize_errors_fallback() {
    let cases = [
        (503, b"not-json".to_vec(), ErrorKind::Transient),
        (
            503,
            survey_entries_changes_error_envelope("SURVEY_ENTRIES_CHANGES_UNKNOWN"),
            ErrorKind::Transient,
        ),
        (
            400,
            survey_entries_changes_error_envelope("SURVEY_ENTRIES_CHANGES_MIRROR_INVALID"),
            ErrorKind::InvalidInput,
        ),
        (
            503,
            survey_entries_changes_error_envelope("SURVEY_ENTRIES_CHANGES_TOO_LARGE"),
            ErrorKind::Transient,
        ),
        (413, vec![b'x'; 1024 * 1024 + 1], ErrorKind::InvalidInput),
    ];
    for (status, body, kind) in cases {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_survey_entries_changes(status, &body);
        let mut client = signed_in(transport, MemoryStore::default());
        let error = client
            .survey_entries_changes("project_a", &survey_entries_changes_request(), NOW + 1)
            .unwrap_err();
        assert_eq!(error.kind(), kind);
        assert_eq!(error.survey_entries_changes_service_code(), None);
        assert!(!error.to_string().contains("backend-secret"));
    }
}

fn survey_entry_create_request() -> SurveyEntryCreateRequest {
    SurveyEntryCreateRequest::new(
        "survey_points",
        "entry_1",
        "outbox-1",
        serde_json::json!({"name": "Pole 1"})
            .as_object()
            .unwrap()
            .clone(),
        "2026-08-30T00:00:00Z",
        SurveyEntryCreateOrigin::Unknown,
    )
    .unwrap()
    .with_context_key("parents:parent%3A1")
    .unwrap()
    .with_geometry(serde_json::json!({"type": "Point", "coordinates": [30.0, -1.0]}))
    .unwrap()
    .with_connectivity(
        serde_json::json!({"precedent": "p0"})
            .as_object()
            .unwrap()
            .clone(),
    )
    .unwrap()
    .with_detailed_location(
        serde_json::json!({"district": "Huye"})
            .as_object()
            .unwrap()
            .clone(),
    )
    .unwrap()
}

fn survey_entry_create_envelope() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "success": true,
        "message": "Entry created successfully",
        "data": {
            "project_id": "project_a",
            "form_id": "survey_points",
            "doc_id": "entry_1",
            "operation": "create",
            "client_version": 0,
            "replication_clock": "metadata.firestore_updated_at",
            "replication_authority": "firestore_server_timestamp"
        },
        "timestamp": "2026-08-30T00:00:00Z",
        "service": "data-solutions-backend",
        "version": "1.0.0",
        "request_id": "request-1"
    }))
    .unwrap()
}

#[test]
fn survey_entry_create_is_one_fixed_selected_project_call_and_verifies_201_receipt() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_survey_entry_create(201, &survey_entry_create_envelope());
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let receipt = client
        .survey_entry_create("project_a", &survey_entry_create_request(), NOW + 1)
        .unwrap();

    assert_eq!(receipt.project_id(), "project_a");
    assert_eq!(receipt.form_id(), "survey_points");
    assert_eq!(receipt.doc_id(), "entry_1");
    assert_eq!(receipt.client_version(), 0);
    assert_eq!(receipt.replication_clock(), "metadata.firestore_updated_at");
    assert_eq!(
        receipt.replication_authority(),
        "firestore_server_timestamp"
    );
    assert!(transport.calls().iter().any(|call| call ==
        "survey_entry_create method=POST origin=https://eds-gateway-3c0q477h.ue.gateway.dev path=/api/v1/entries/mutate content_type=application/json body_exact=true limit=1048576 timeout=120 api_key=true authorization=true forwarded_authorization=true email=true client_id=ds-native-client correlation=true"));
}

#[test]
fn survey_entry_create_request_owns_strict_identity_context_geojson_and_size_bounds() {
    for (form, doc_id, key) in [
        ("Bad Form", "entry_1", "outbox-1"),
        ("survey_points", "../entry", "outbox-1"),
        ("survey_points", "entry_1", " key "),
    ] {
        let error = SurveyEntryCreateRequest::new(
            form,
            doc_id,
            key,
            serde_json::Map::new(),
            "2026-08-30T00:00:00Z",
            SurveyEntryCreateOrigin::Unknown,
        )
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
    }

    for context in ["bad", "parents:parent%2Fchild"] {
        assert_eq!(
            SurveyEntryCreateRequest::new(
                "survey_points",
                "entry_1",
                "outbox-1",
                serde_json::Map::new(),
                "2026-08-30T00:00:00Z",
                SurveyEntryCreateOrigin::Unknown,
            )
            .unwrap()
            .with_context_key(context)
            .unwrap_err()
            .kind(),
            ErrorKind::InvalidInput
        );
    }

    for geometry in [
        serde_json::json!({"type":"Point","coordinates":[30.0]}),
        serde_json::json!({"type":"Feature","geometry":null}),
    ] {
        assert_eq!(
            SurveyEntryCreateRequest::new(
                "survey_points",
                "entry_1",
                "outbox-1",
                serde_json::Map::new(),
                "2026-08-30T00:00:00Z",
                SurveyEntryCreateOrigin::Unknown,
            )
            .unwrap()
            .with_geometry(geometry)
            .unwrap_err()
            .kind(),
            ErrorKind::InvalidInput
        );
    }

    let mut data = serde_json::Map::new();
    data.insert(
        "large".to_owned(),
        serde_json::Value::String(
            "x".repeat(ds_client_core::SURVEY_ENTRY_CREATE_MAX_PAYLOAD_BYTES),
        ),
    );
    assert_eq!(
        SurveyEntryCreateRequest::new(
            "survey_points",
            "entry_1",
            "outbox-1",
            data,
            "2026-08-30T00:00:00Z",
            SurveyEntryCreateOrigin::Unknown,
        )
        .unwrap_err()
        .kind(),
        ErrorKind::InvalidInput
    );

    let normalized = SurveyEntryCreateRequest::new(
        "survey_points",
        "entry_1",
        "outbox-1",
        serde_json::Map::new(),
        "2026-08-30T02:00:00+02:00",
        SurveyEntryCreateOrigin::Unknown,
    )
    .unwrap()
    .with_context_key("parents:parent%3Aone;children:child%3Btwo%25done;siblings:raw:colon")
    .unwrap();
    assert_eq!(normalized.created_at(), "2026-08-30T00:00:00Z");
    assert_eq!(normalized.origin().token(), "unknown");
    assert_eq!(
        normalized.context_key(),
        Some("parents:parent%3Aone;children:child%3Btwo%25done;siblings:raw:colon")
    );
    assert!(serde_json::Number::from_f64(f64::NAN).is_none());

    let private_request = SurveyEntryCreateRequest::new(
        "survey_points",
        "entry_1",
        "private-idempotency-key",
        serde_json::json!({"private_field_name": "private field value"})
            .as_object()
            .unwrap()
            .clone(),
        "2026-08-30T00:00:00Z",
        SurveyEntryCreateOrigin::Unknown,
    )
    .unwrap()
    .with_geometry(serde_json::json!({"type":"Point","coordinates":[31.9876,-2.1234]}))
    .unwrap();
    let debug = format!("{private_request:?}");
    for private in [
        "private-idempotency-key",
        "private_field_name",
        "private field value",
        "31.9876",
        "-2.1234",
    ] {
        assert!(!debug.contains(private), "debug leaked {private}: {debug}");
    }
    assert!(debug.contains("data_field_count: 1"));
    assert!(debug.contains("has_geometry: true"));
}

#[test]
fn survey_entry_create_profile_and_response_identity_fail_closed() {
    for mutate in [
        |input: &mut ClientProfileInput| input.survey_entry_create_method = "GET".to_owned(),
        |input: &mut ClientProfileInput| input.survey_entry_create_path = "/other".to_owned(),
        |input: &mut ClientProfileInput| input.survey_entry_create_operation = "update".to_owned(),
    ] {
        let mut input = profile_input(DeploymentLane::Stable);
        mutate(&mut input);
        assert_eq!(
            ClientProfile::validate(input).unwrap_err(),
            ProfileError::SurveyEntryCreateRoute
        );
    }

    for (from, to) in [
        ("\"project_id\":\"project_a\"", "\"project_id\":\"other\""),
        ("\"form_id\":\"survey_points\"", "\"form_id\":\"other\""),
        ("\"doc_id\":\"entry_1\"", "\"doc_id\":\"other\""),
        ("\"operation\":\"create\"", "\"operation\":\"update\""),
        ("\"client_version\":0", "\"client_version\":1"),
        (
            "\"replication_clock\":\"metadata.firestore_updated_at\"",
            "\"replication_clock\":\"metadata.updated_at\"",
        ),
        (
            "\"replication_authority\":\"firestore_server_timestamp\"",
            "\"replication_authority\":\"client_clock\"",
        ),
    ] {
        let body = String::from_utf8(survey_entry_create_envelope())
            .unwrap()
            .replace(from, to)
            .into_bytes();
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_survey_entry_create(201, &body);
        let mut client = signed_in(transport, MemoryStore::default());
        assert_eq!(
            client
                .survey_entry_create("project_a", &survey_entry_create_request(), NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }

    let mut unknown: serde_json::Value =
        serde_json::from_slice(&survey_entry_create_envelope()).unwrap();
    unknown["data"]["unexpected"] = serde_json::json!(true);
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_survey_entry_create(201, &serde_json::to_vec(&unknown).unwrap());
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client
            .survey_entry_create("project_a", &survey_entry_create_request(), NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );

    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_survey_entry_create(
        201,
        &vec![b'x'; ds_client_core::SURVEY_ENTRY_CREATE_RESPONSE_LIMIT + 1],
    );
    let mut client = signed_in(transport, MemoryStore::default());
    assert_eq!(
        client
            .survey_entry_create("project_a", &survey_entry_create_request(), NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
}

fn survey_entry_create_error_envelope(code: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "success": false,
        "error": {"code": code, "message": "private backend text", "timestamp": 1},
        "timestamp": "2026-08-30T00:00:00Z",
        "service": "data-solutions-backend",
        "version": "1.0.0"
    }))
    .unwrap()
}

#[test]
fn survey_entry_create_maps_only_exact_status_code_pairs_without_backend_text() {
    let cases = [
        (
            400,
            "VALIDATION_FAILED",
            SurveyEntryCreateServiceCode::Invalid,
            ErrorKind::InvalidInput,
        ),
        (
            400,
            "INVALID_FORMAT",
            SurveyEntryCreateServiceCode::Invalid,
            ErrorKind::InvalidInput,
        ),
        (
            401,
            "UNAUTHORIZED",
            SurveyEntryCreateServiceCode::Unauthorized,
            ErrorKind::AuthenticationRejected,
        ),
        (
            403,
            "INSUFFICIENT_PERMISSIONS",
            SurveyEntryCreateServiceCode::PermissionDenied,
            ErrorKind::AuthenticationRejected,
        ),
        (
            404,
            "NOT_FOUND",
            SurveyEntryCreateServiceCode::ScopeNotFound,
            ErrorKind::ResourceNotFound,
        ),
        (
            404,
            "PROJECT_NOT_FOUND",
            SurveyEntryCreateServiceCode::ScopeNotFound,
            ErrorKind::ResourceNotFound,
        ),
        (
            409,
            "FORM_NOT_ENABLED",
            SurveyEntryCreateServiceCode::FormDisabled,
            ErrorKind::InvalidInput,
        ),
        (
            403,
            "PROJECT_NOT_ACTIVE",
            SurveyEntryCreateServiceCode::ProjectReadOnly,
            ErrorKind::InvalidInput,
        ),
        (
            403,
            "PROJECT_ARCHIVED",
            SurveyEntryCreateServiceCode::ProjectReadOnly,
            ErrorKind::InvalidInput,
        ),
        (
            403,
            "PROJECT_EXPIRED",
            SurveyEntryCreateServiceCode::ProjectReadOnly,
            ErrorKind::InvalidInput,
        ),
        (
            409,
            "CONFLICT",
            SurveyEntryCreateServiceCode::IdempotencyConflict,
            ErrorKind::InvalidInput,
        ),
        (
            409,
            "ALREADY_EXISTS",
            SurveyEntryCreateServiceCode::AlreadyExists,
            ErrorKind::InvalidInput,
        ),
        (
            500,
            "INTERNAL_ERROR",
            SurveyEntryCreateServiceCode::Failed,
            ErrorKind::Transient,
        ),
    ];
    for (status, wire, code, kind) in cases {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_survey_entry_create(status, &survey_entry_create_error_envelope(wire));
        let mut client = signed_in(transport, MemoryStore::default());
        let error = client
            .survey_entry_create("project_a", &survey_entry_create_request(), NOW + 1)
            .unwrap_err();
        assert_eq!(error.kind(), kind, "{wire}");
        assert_eq!(
            error.survey_entry_create_service_code(),
            Some(code),
            "{wire}"
        );
        assert!(!error.to_string().contains("private backend text"));
    }

    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_survey_entry_create(
        409,
        &survey_entry_create_error_envelope("PROJECT_NOT_ACTIVE"),
    );
    let mut client = signed_in(transport, MemoryStore::default());
    let error = client
        .survey_entry_create("project_a", &survey_entry_create_request(), NOW + 1)
        .unwrap_err();
    assert_eq!(error.survey_entry_create_service_code(), None);
    assert_eq!(error.kind(), ErrorKind::InvalidInput);

    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_survey_entry_create(
        500,
        &vec![b'x'; ds_client_core::SURVEY_ENTRY_CREATE_RESPONSE_LIMIT + 1],
    );
    let mut client = signed_in(transport, MemoryStore::default());
    let error = client
        .survey_entry_create("project_a", &survey_entry_create_request(), NOW + 1)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Transient);
    assert_eq!(error.survey_entry_create_service_code(), None);
}

fn tile_status_envelope(status: &str, project: &str, tile_type: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "status": status,
        "eds_project_id": project,
        "tile_type": tile_type,
        "cache_available": false,
        "cleanup_pending": false,
        "message": "No tiling has been performed yet"
    }))
    .unwrap()
}

fn tile_preflight_envelope() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "status": "ready",
        "action": "preflight",
        "eds_project_id": "project_a",
        "tile_type": "survey",
        "gcp_project": "",
        "dataset": "",
        "expected_layer_count": 1,
        "will_export_layers": 1,
        "total_rows": 0,
        "total_geometries": 0,
        "layers": [{
            "table": "survey_points",
            "row_count": 0,
            "geometry_count": 0,
            "empty": false,
            "dashed": false
        }],
        "empty_layers": [],
        "dashed_table_names": [],
        "invalid_table_ids": [],
        "errors": [],
        "warnings": [],
        "message": "Ready to tile 1 enabled Firestore survey layers.",
        "repair_required": false
    }))
    .unwrap()
}

#[test]
fn tiles_are_three_fixed_selected_project_calls_with_backend_shaped_results() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_tiles(
        200,
        &tile_status_envelope("not_started", "project_a", "survey"),
    );
    transport.push_tiles(200, &tile_preflight_envelope());
    transport.push_tiles(
        200,
        &serde_json::to_vec(&serde_json::json!({
            "status": "started",
            "phase": "queued",
            "eds_project_id": "project_a",
            "tile_type": "design",
            "started_at": 1_900_000_001_000_u64,
            "triggered_by": "user@example.com",
            "force": true,
            "needs_tiling": true,
            "will_dispatch": true,
            "decision": "dispatch",
            "message": "Tile generation started. Use Check progress to see the latest status."
        }))
        .unwrap(),
    );
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    let status = client
        .tile_status("project_a", TileType::Survey, NOW + 1)
        .unwrap();
    assert_eq!(status.project(), "project_a");
    assert_eq!(status.tile_type(), TileType::Survey);
    assert_eq!(status.status(), TileOperationStatus::NotStarted);
    assert_eq!(status.phase(), None);
    assert_eq!(status.cache_available(), Some(false));
    assert_eq!(status.cleanup_pending(), Some(false));

    let preflight = client
        .tile_preflight("project_a", TileType::Survey, NOW + 1)
        .unwrap();
    assert_eq!(preflight.project(), "project_a");
    assert_eq!(preflight.tile_type(), TileType::Survey);
    assert_eq!(preflight.status(), TilePreflightStatus::Ready);
    assert_eq!(preflight.expected_layer_count(), 1);
    assert_eq!(preflight.will_export_layers(), 1);
    assert_eq!(preflight.layers()[0].table(), "survey_points");
    assert_eq!(preflight.layers()[0].table_type(), None);
    assert_eq!(preflight.projection_state(), None);

    let generated = client
        .tile_generate("project_a", TileType::Design, true, NOW + 1)
        .unwrap();
    assert_eq!(generated.status(), TileOperationStatus::Started);
    assert_eq!(generated.phase(), Some("queued"));
    assert_eq!(generated.started_at(), Some(1_900_000_001_000));
    assert_eq!(generated.force(), Some(true));
    assert_eq!(generated.needs_tiling(), Some(true));
    assert_eq!(generated.will_dispatch(), Some(true));
    assert_eq!(generated.decision(), Some("dispatch"));

    assert!(transport.calls().iter().any(|call| call.contains(
        "tiles method=POST origin=https://eds-gateway-3c0q477h.ue.gateway.dev path=/api/v1/tiles content_type=application/json body={\"action\":\"status\",\"eds_project_id\":\"project_a\",\"tile_type\":\"survey\"} limit=33554432 timeout=120"
    )));
    assert!(transport.calls().iter().any(|call| call.contains(
        "body={\"action\":\"preflight\",\"eds_project_id\":\"project_a\",\"tile_type\":\"survey\"}"
    )));
    assert!(transport.calls().iter().any(|call| call.contains(
        "body={\"action\":\"generate\",\"eds_project_id\":\"project_a\",\"tile_type\":\"design\",\"force\":true}"
    )));
    assert_eq!(transport.0.lock().unwrap().correlated_tile_requests, 3);
}

#[test]
fn tile_profile_input_identity_status_and_bounds_fail_closed() {
    for mutate in [
        |input: &mut ClientProfileInput| input.tiles_method = "GET".to_owned(),
        |input: &mut ClientProfileInput| input.tiles_path = "/anything".to_owned(),
        |input: &mut ClientProfileInput| input.tiles_actions.swap(0, 2),
    ] {
        let mut input = profile_input(DeploymentLane::Stable);
        mutate(&mut input);
        assert_eq!(
            ClientProfile::validate(input).err().unwrap(),
            ProfileError::TilesRoute
        );
    }

    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let before = transport.calls().len();
    let error = client
        .tile_status(" project_a", TileType::Survey, NOW + 1)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert_eq!(transport.calls().len(), before);

    for body in [
        tile_status_envelope("not_started", "other_project", "survey"),
        tile_status_envelope("not_started", "project_a", "design"),
        tile_status_envelope("started", "project_a", "survey"),
        tile_status_envelope("unknown", "project_a", "survey"),
    ] {
        transport.push_tiles(200, &body);
        let error = client
            .tile_status("project_a", TileType::Survey, NOW + 1)
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
    }

    transport.push_tiles(200, &vec![b' '; ds_client_core::TILE_RESPONSE_LIMIT + 1]);
    let error = client
        .tile_status("project_a", TileType::Survey, NOW + 1)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
}

#[test]
fn tile_error_statuses_are_classified_without_exposing_backend_text() {
    for (status, expected) in [
        (400, ErrorKind::InvalidInput),
        (409, ErrorKind::InvalidInput),
        (403, ErrorKind::AuthenticationRejected),
        (503, ErrorKind::Transient),
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_tiles(
            status,
            br#"{"status":"error","error":"private backend detail"}"#,
        );
        let mut client = signed_in(transport, MemoryStore::default());
        let error = client
            .tile_generate("project_a", TileType::Design, false, NOW + 1)
            .unwrap_err();
        assert_eq!(error.kind(), expected);
        assert!(!error.to_string().contains("private backend detail"));
    }
}

fn inventory_envelope(project: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "eds_project_id": project,
        "requested": [],
        "active_count": 2,
        "retired_count": 1,
        "deleted_count": 0,
        "transformers": [
            {"name": "mv_data", "kind": "project_level", "state": "active"},
            {"name": "tx_a", "state": "active"},
            {"name": "tx_b", "state": "retired", "retirement": {
                "state": "retired", "reason": "duplicate of tx_a", "retired_at": 1_900_000_000_000_u64,
                "retired_by": "user@example.com",
                "backup": {
                    "bucket": "ds-transformer-backups", "object": "project_a/tx_b/1900000000000.json",
                    "generation": 1_758_000_000_000_001_i64, "sha256": "ab".repeat(32), "byte_length": 4096
                }
            }}
        ]
    }))
    .unwrap()
}

/// One `list_transformers_status` answer in the shape ds-brain sends: an
/// object with `transformers`, each row a whole `services.TransformerStatus`
/// document. Nothing below the row is modelled, so the fixture keeps the
/// nesting the browser actually reads (`process_metadata`, `report_metadata`,
/// `metadata.version`) to prove it survives the decode untouched.
fn status_envelope() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "transformers": [
            {
                "name": "tx_a",
                "layers": {"consumers": 42, "poles": 17},
                "metadata": {"version": 7, "content_digest": "a".repeat(64)},
                "process_metadata": {"status": "completed", "at": 1_900_000_000_000_u64},
                "report_metadata": {"status": "generated"},
                "retry_capabilities": {"process": {"can_retry": true, "source": "firestore_replay"}}
            },
            {
                "name": "tx_b",
                "layers": {},
                "process_metadata": {"status": "failed", "error": "no source files tracked"}
            }
        ]
    }))
    .unwrap()
}

fn compounded_envelope(individuals: u64) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "status": "partial",
        "transformer": "combined_transformer",
        "prefix": "020926-1500-report-demo",
        "download_files_urls": ["gs://bucket/reports/020926-1500-report-demo/020926-1500-report-demo.zip"],
        "compounded_via": "ds-brain",
        "errors": ["tx_c: report artifacts missing"],
        "individual_artifact_transformer_count": individuals,
        "missing_individual_artifact_count": 1,
        "missing_individual_artifacts": ["tx_c"],
        "missing_individual_artifact_causes": [{
            "transformer": "tx_c",
            "code": "never_processed",
            "detail": "report artifacts missing"
        }],
        "registry_write_failed": true,
        "registry_write_error": "registry unavailable"
    }))
    .unwrap()
}

#[test]
fn project_report_operations_are_six_fixed_selected_project_actions() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project_report(200, &compounded_envelope(2));
    transport.push_project_report(
        200,
        &serde_json::to_vec(&serde_json::json!({
            "compounded_reports": [{
                "artifact_index_state":"ready",
                "archive_members":[{"path":"tx_a.a0.pdf","size_bytes":42}],
                "stem": "020926-1500-report-demo",
                "filename": "020926-1500-report-demo.zip",
                "gcs_path": "gs://bucket/reports/020926-1500-report-demo/020926-1500-report-demo.zip",
                "download_url": "https://signed.example/archive.zip",
                "created_at": "2026-09-02T15:00:00Z",
                "created_by": "user@example.com",
                "status": "partial",
                "transformer_count": 3,
                "transformers": ["tx_a", "tx_b", "tx_c"],
                "district_count": 1,
                "districts": ["Huye"],
                "individual_artifact_transformer_count": 2,
                "missing_individual_artifact_count": 1,
                "errors": ["tx_c: report artifacts missing"],
                "archive_layout": {"file_level": "sector", "combine_per_district": true}
            }]
        }))
        .unwrap(),
    );
    transport.push_project_report(200, &inventory_envelope("project_a"));
    transport.push_project_report(
        200,
        &serde_json::to_vec(&serde_json::json!({
            "eds_project_id": "project_a",
            "action": "retire_transformer",
            "results": [
                {"name": "tx_a", "status": "retired", "at": 1_900_000_001_000_u64, "backup": {
                    "bucket": "ds-transformer-backups", "object": "project_a/tx_a/1900000001000.json",
                    "generation": 7, "sha256": "cd".repeat(32), "byte_length": 2048
                }},
                {"name": "tx_b", "status": "failed", "error_code": "already_retired", "error": "already retired"}
            ],
            "applied_count": 1,
            "failed_count": 1
        }))
        .unwrap(),
    );
    transport.push_project_report(
        200,
        &serde_json::to_vec(&serde_json::json!({
            "eds_project_id": "project_a",
            "action": "restore_transformer",
            "results": [{"name": "tx_b", "status": "restored", "at": 1_900_000_002_000_u64}],
            "applied_count": 1,
            "failed_count": 0
        }))
        .unwrap(),
    );
    transport.push_project_report(200, &status_envelope());
    transport.push_project_report(200, &status_envelope());
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    let request = CompoundedReportRequest::new(
        TransformerSet::new(["tx_a".to_owned(), "tx_b".to_owned(), "tx_c".to_owned()]).unwrap(),
        ReportFileLevel::Sector,
        true,
        false,
    );
    let receipt = client
        .compounded_report("project_a", &request, NOW + 1)
        .unwrap();
    assert_eq!(receipt.project(), "project_a");
    assert_eq!(receipt.status(), CompoundedReportStatus::Partial);
    assert_eq!(receipt.prefix(), "020926-1500-report-demo");
    assert_eq!(receipt.archive_paths().len(), 1);
    assert_eq!(receipt.individual_artifact_transformer_count(), 2);
    assert_eq!(receipt.missing_individual_artifacts(), ["tx_c"]);
    let cause = &receipt.missing_individual_artifact_causes()[0];
    assert_eq!(cause.transformer(), "tx_c");
    assert_eq!(cause.code(), "never_processed");
    assert_eq!(cause.detail(), "report artifacts missing");
    assert!(receipt.registry_write_failed());
    assert_eq!(receipt.registry_write_error(), Some("registry unavailable"));

    let archives = client.compounded_report_list("project_a", NOW + 1).unwrap();
    assert_eq!(archives.len(), 1);
    assert_eq!(archives[0].artifact_index_state(), Some("ready"));
    assert_eq!(archives[0].archive_members()[0].path, "tx_a.a0.pdf");
    assert_eq!(archives[0].stem(), "020926-1500-report-demo");
    assert_eq!(
        archives[0].download_url(),
        Some("https://signed.example/archive.zip")
    );
    assert_eq!(archives[0].transformers().len(), 3);
    assert_eq!(
        archives[0].archive_layout().unwrap().file_level(),
        Some("sector")
    );
    assert!(archives[0].archive_layout().unwrap().combine_per_district());

    let inventory = client
        .transformer_inventory("project_a", &TransformerSet::default(), NOW + 1)
        .unwrap();
    assert_eq!(inventory.active_count(), 2);
    assert_eq!(inventory.retired_count(), 1);
    assert_eq!(inventory.rows().len(), 3);
    assert_eq!(
        inventory.rows()[2].lifecycle(),
        TransformerLifecycle::Retired
    );
    assert_eq!(
        inventory.rows()[2].retirement().unwrap().reason(),
        Some("duplicate of tx_a")
    );
    let backup = inventory.rows()[2].retirement().unwrap().backup().unwrap();
    assert_eq!(backup.object(), "project_a/tx_b/1900000000000.json");
    assert_eq!(backup.generation(), 1_758_000_000_000_001);
    assert_eq!(backup.sha256(), "ab".repeat(32));
    assert_eq!(backup.byte_length(), 4096);
    assert!(inventory.rows()[1].retirement().is_none());

    let retire = RetirementRequest::retire(
        TransformerSet::new(["tx_a".to_owned(), "tx_b".to_owned()]).unwrap(),
        "superseded by the 2026 survey".to_owned(),
    )
    .unwrap();
    let receipt = client
        .transformer_retirement("project_a", RetirementAction::Retire, &retire, NOW + 1)
        .unwrap();
    assert_eq!(receipt.applied_count(), 1);
    assert_eq!(receipt.failed_count(), 1);
    assert!(receipt.results()[0].applied());
    let backup = receipt.results()[0]
        .backup()
        .expect("a retirement names its backup");
    assert_eq!(backup.bucket(), "ds-transformer-backups");
    assert_eq!(backup.generation(), 7);
    assert!(receipt.results()[1].backup().is_none());
    assert_eq!(
        receipt.results()[1].refusal(),
        Some(RetirementRefusal::AlreadyRetired)
    );

    let restore =
        RetirementRequest::restore(TransformerSet::new(["tx_b".to_owned()]).unwrap()).unwrap();
    let receipt = client
        .transformer_retirement("project_a", RetirementAction::Restore, &restore, NOW + 1)
        .unwrap();
    assert_eq!(receipt.action(), RetirementAction::Restore);
    assert_eq!(receipt.applied_count(), 1);

    // The status read is the whole-project default and the exact-names form of
    // the same closed action. Rows arrive as ds-brain sent them: the crate
    // proves the envelope and the reference name, and reshapes nothing.
    let status = client
        .transformer_status("project_a", &TransformerSet::default(), NOW + 1)
        .unwrap();
    assert_eq!(status.len(), 2);
    assert_eq!(status.rows()[0].name(), "tx_a");
    // ds-brain's status document carries no `kind`; the inventory row does.
    assert_eq!(status.rows()[0].kind(), None);
    assert_eq!(status.rows()[0].row()["metadata"]["version"], 7);
    assert_eq!(
        status.rows()[0].row()["process_metadata"]["status"],
        "completed"
    );
    assert_eq!(status.rows()[0].row()["layers"]["consumers"], 42);
    assert_eq!(
        status.rows()[1].row()["process_metadata"]["error"],
        "no source files tracked"
    );
    // No client-only field is synthesised: what is not on the wire is absent.
    assert!(status.rows()[1].row().get("client_state").is_none());
    let named = client
        .transformer_status(
            "project_a",
            &TransformerSet::new(["tx_a".to_owned(), "tx_b".to_owned()]).unwrap(),
            NOW + 1,
        )
        .unwrap();
    assert_eq!(named.into_rows().len(), 2);

    let calls = transport.calls();
    assert!(calls.iter().any(|call| call.contains(
        "project_report method=POST origin=https://eds-gateway-3c0q477h.ue.gateway.dev path=/report action=download_transfo lane=fast content_type=application/json body={\"action\":\"download_transfo\",\"archive_layout\":{\"combine_per_district\":true,\"file_level\":\"sector\"},\"eds_project_id\":\"project_a\",\"report_type\":\"compounded_reports\",\"transfo\":\"combined_transformer\",\"transformers\":[\"tx_a\",\"tx_b\",\"tx_c\"]} limit=8388608 timeout=600"
    )), "{calls:#?}");
    assert!(calls.iter().any(|call| call.contains(
        "action=list_compounded_reports lane=none content_type=application/json body={\"action\":\"list_compounded_reports\",\"eds_project_id\":\"project_a\",\"include_artifacts\":true} limit=8388608 timeout=120"
    )));
    assert!(calls.iter().any(|call| {
        call.contains(
            "body={\"action\":\"transformer_inventory\",\"eds_project_id\":\"project_a\"}",
        )
    }));
    assert!(calls.iter().any(|call| call.contains(
        "body={\"action\":\"retire_transformer\",\"eds_project_id\":\"project_a\",\"reason\":\"superseded by the 2026 survey\",\"transformers\":[\"tx_a\",\"tx_b\"]}"
    )));
    assert!(calls.iter().any(|call| call.contains(
        "body={\"action\":\"restore_transformer\",\"eds_project_id\":\"project_a\",\"transformers\":[\"tx_b\"]}"
    )));
    // Lane-neutral like every read on this route, on the read deadline, and
    // the whole-project form carries no `transformers` key at all.
    assert!(calls.iter().any(|call| call.contains(
        "action=list_transformers_status lane=none content_type=application/json body={\"action\":\"list_transformers_status\",\"eds_project_id\":\"project_a\"} limit=8388608 timeout=120"
    )), "{calls:#?}");
    assert!(calls.iter().any(|call| call.contains(
        "body={\"action\":\"list_transformers_status\",\"eds_project_id\":\"project_a\",\"transformers\":[\"tx_a\",\"tx_b\"]}"
    )));
    assert_eq!(
        transport
            .0
            .lock()
            .unwrap()
            .correlated_project_report_requests,
        7
    );
}

#[test]
fn project_report_profile_inputs_receipts_and_bounds_fail_closed() {
    for mutate in [
        |input: &mut ClientProfileInput| input.project_report_method = "GET".to_owned(),
        |input: &mut ClientProfileInput| input.project_report_path = "/api/v1/report".to_owned(),
        |input: &mut ClientProfileInput| {
            input
                .project_report_actions
                .push("delete_transformer".to_owned())
        },
        |input: &mut ClientProfileInput| input.project_report_actions.swap(0, 3),
    ] {
        let mut input = profile_input(DeploymentLane::Stable);
        mutate(&mut input);
        assert_eq!(
            ClientProfile::validate(input).err().unwrap(),
            ProfileError::ProjectReportRoute
        );
    }

    assert!(TransformerSet::new([" tx_a".to_owned()]).is_err());
    assert_eq!(
        TransformerSet::new([
            "TX-1".to_owned(),
            "tx_1".to_owned(),
            "001 Main".to_owned(),
            "combined-transformers".to_owned(),
        ])
        .unwrap()
        .names(),
        &["tx_1", "_001_main", "combined_transformer"]
    );
    assert!(TransformerSet::new((0..501).map(|index| format!("tx_{index}"))).is_err());
    assert!(RetirementRequest::retire(TransformerSet::default(), "why".to_owned()).is_err());
    assert!(RetirementRequest::retire(
        TransformerSet::new(["tx_a".to_owned()]).unwrap(),
        "  ".to_owned()
    )
    .is_err());
    assert!(RetirementRequest::restore(TransformerSet::default()).is_err());
    assert_eq!(
        ReportFileLevel::parse("district"),
        Some(ReportFileLevel::District)
    );
    assert_eq!(ReportFileLevel::parse("flat"), None);

    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let before = transport.calls().len();
    let error = client
        .compounded_report_list(" project_a", NOW + 1)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert_eq!(transport.calls().len(), before);

    // An archive advertised for zero individual artifacts is an impossible
    // receipt, refused the same way ds-web refuses it.
    transport.push_project_report(200, &compounded_envelope(0));
    let request = CompoundedReportRequest::new(
        TransformerSet::default(),
        ReportFileLevel::Transformer,
        false,
        true,
    );
    let error = client
        .compounded_report("project_a", &request, NOW + 1)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
    assert!(transport.calls().last().unwrap().contains(
        "body={\"action\":\"download_transfo\",\"archive_layout\":{\"combine_per_district\":false,\"file_level\":\"transformer\"},\"eds_project_id\":\"project_a\",\"force\":true,\"report_type\":\"compounded_reports\",\"transfo\":\"combined_transformer\",\"transformers\":[]}"
    ));

    // The producer emits a bounded array of typed causes. The fabricated
    // object shape that previously made this test pass is not wire-compatible.
    let mut wrong_cause_shape: serde_json::Value =
        serde_json::from_slice(&compounded_envelope(2)).unwrap();
    wrong_cause_shape["missing_individual_artifact_causes"] =
        serde_json::json!({"tx_c": "never_processed"});
    transport.push_project_report(200, &serde_json::to_vec(&wrong_cause_shape).unwrap());
    let error = client
        .compounded_report("project_a", &request, NOW + 1)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::UnreadableResponse);

    let mut unbounded_cause: serde_json::Value =
        serde_json::from_slice(&compounded_envelope(2)).unwrap();
    unbounded_cause["missing_individual_artifact_causes"][0]["code"] =
        serde_json::Value::String("x".repeat(129));
    transport.push_project_report(200, &serde_json::to_vec(&unbounded_cause).unwrap());
    let error = client
        .compounded_report("project_a", &request, NOW + 1)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::UnreadableResponse);

    for body in [
        inventory_envelope("other_project"),
        serde_json::to_vec(&serde_json::json!({
            "eds_project_id": "project_a", "requested": [], "transformers": [{"name": "tx", "state": "sleeping"}]
        }))
        .unwrap(),
        vec![b' '; ds_client_core::PROJECT_REPORT_RESPONSE_LIMIT + 1],
    ] {
        transport.push_project_report(200, &body);
        let error = client
            .transformer_inventory("project_a", &TransformerSet::default(), NOW + 1)
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
    }

    // A receipt must answer every requested name in order with a closed code.
    let retire = RetirementRequest::retire(
        TransformerSet::new(["tx_a".to_owned(), "tx_b".to_owned()]).unwrap(),
        "reason".to_owned(),
    )
    .unwrap();
    for results in [
        serde_json::json!([{"name": "tx_a", "status": "retired"}]),
        serde_json::json!([{"name": "tx_b", "status": "retired"}, {"name": "tx_a", "status": "retired"}]),
        serde_json::json!([{"name": "tx_a", "status": "retired"}, {"name": "tx_b", "status": "failed", "error_code": "mystery"}]),
        // A backup receipt is all-or-nothing: a malformed digest is unreadable.
        serde_json::json!([{"name": "tx_a", "status": "retired", "backup": {"bucket": "b", "object": "o", "generation": 1, "sha256": "short", "byte_length": 1}}, {"name": "tx_b", "status": "retired"}]),
    ] {
        transport.push_project_report(
            200,
            &serde_json::to_vec(&serde_json::json!({
                "eds_project_id": "project_a", "action": "retire_transformer", "results": results,
                "applied_count": 2, "failed_count": 0
            }))
            .unwrap(),
        );
        let error = client
            .transformer_retirement("project_a", RetirementAction::Retire, &retire, NOW + 1)
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
    }
}

/// The status decode is a transport proof, not a schema: it refuses anything
/// that is not the closed envelope, and passes everything inside a row through
/// untouched.
#[test]
fn transformer_status_rows_are_bounded_and_refuse_a_broken_envelope() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    let row = |value: serde_json::Value| {
        serde_json::to_vec(&serde_json::json!({ "transformers": [value] })).unwrap()
    };
    for body in [
        // No `transformers` member at all.
        serde_json::to_vec(&serde_json::json!({ "rows": [] })).unwrap(),
        // `transformers` is not an array.
        serde_json::to_vec(&serde_json::json!({ "transformers": { "tx_a": {} } })).unwrap(),
        // A row that is not an object.
        row(serde_json::json!("tx_a")),
        // A row with no `name`.
        row(serde_json::json!({ "process_metadata": { "status": "completed" } })),
        // A row whose `name` is not a string.
        row(serde_json::json!({ "name": 7 })),
        // A row whose `name` is blank, untrimmed, or past the reference bound.
        row(serde_json::json!({ "name": "" })),
        row(serde_json::json!({ "name": " tx_a" })),
        row(serde_json::json!({ "name": "t".repeat(201) })),
        // A row whose `kind`, when present, is not a bounded string.
        row(serde_json::json!({ "name": "tx_a", "kind": 3 })),
        row(serde_json::json!({ "name": "tx_a", "kind": "k".repeat(65) })),
        // More rows than the project's own collection may hold.
        serde_json::to_vec(&serde_json::json!({
            "transformers": (0..2_001)
                .map(|index| serde_json::json!({ "name": format!("tx_{index}") }))
                .collect::<Vec<_>>()
        }))
        .unwrap(),
        // A body past the fixed response cap, refused before any row is read.
        vec![b' '; ds_client_core::PROJECT_REPORT_RESPONSE_LIMIT + 1],
    ] {
        transport.push_project_report(200, &body);
        let error = client
            .transformer_status("project_a", &TransformerSet::default(), NOW + 1)
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
    }

    // A `null` kind is the absence of a kind, not a malformed one.
    transport.push_project_report(
        200,
        &row(serde_json::json!({ "name": "tx_a", "kind": null })),
    );
    let answer = client
        .transformer_status("project_a", &TransformerSet::default(), NOW + 1)
        .unwrap();
    assert_eq!(answer.rows()[0].kind(), None);

    // A `null` `transformers` is the empty answer, not a broken envelope. It
    // is what ds-brain actually sends for a project with no matching
    // transformer document: its additive status projection returns a nil Go
    // slice for an empty input, which encodes as `null`. The browser reads
    // that as zero rows; refusing it here would make every empty project an
    // `auth_response_unreadable` in `ds` alone.
    transport.push_project_report(
        200,
        &serde_json::to_vec(&serde_json::json!({ "transformers": null })).unwrap(),
    );
    let empty = client
        .transformer_status("project_a", &TransformerSet::default(), NOW + 1)
        .unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    assert!(empty.rows().is_empty());

    // An empty array is the same answer, and stays one.
    transport.push_project_report(
        200,
        &serde_json::to_vec(&serde_json::json!({ "transformers": [] })).unwrap(),
    );
    assert!(client
        .transformer_status("project_a", &TransformerSet::default(), NOW + 1)
        .unwrap()
        .is_empty());

    // The selected project is validated locally: a malformed key never leaves.
    let before = transport.calls().len();
    let error = client
        .transformer_status(" project_a", &TransformerSet::default(), NOW + 1)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert_eq!(transport.calls().len(), before);
}

/// The request bound and the answer bound are different numbers on purpose.
#[test]
fn the_status_request_bound_and_the_status_answer_bound_are_deliberately_different() {
    // 500 is how many names one request may carry — a local promise about a
    // request this crate builds.
    assert_eq!(ds_client_core::PROJECT_REPORT_MAX_TRANSFORMERS, 500);
    assert!(TransformerSet::new((0..501).map(|index| format!("tx_{index}"))).is_err());

    // 2,000 is how many rows an answer may contain — a bound on what one
    // project's own transformer collection may hold, which is what an unnamed
    // whole-project read asks for. It is the same bound the inventory decode
    // applies to the same collection, and it is deliberately larger than the
    // request bound: no request could have named these rows.
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    for count in [501_usize, 2_000] {
        transport.push_project_report(
            200,
            &serde_json::to_vec(&serde_json::json!({
                "transformers": (0..count)
                    .map(|index| serde_json::json!({ "name": format!("tx_{index}") }))
                    .collect::<Vec<_>>()
            }))
            .unwrap(),
        );
        let answer = client
            .transformer_status("project_a", &TransformerSet::default(), NOW + 1)
            .unwrap();
        assert_eq!(answer.len(), count);
        assert!(!answer.is_empty());
    }
}

#[test]
fn project_report_error_statuses_are_classified_without_exposing_backend_text() {
    for (status, expected) in [
        (400, ErrorKind::InvalidInput),
        (409, ErrorKind::InvalidInput),
        (423, ErrorKind::InvalidInput),
        (403, ErrorKind::AuthenticationRejected),
        (404, ErrorKind::ResourceNotFound),
        (503, ErrorKind::Transient),
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_project_report(
            status,
            br#"{"status":"error","detail":"private backend detail"}"#,
        );
        let mut client = signed_in(transport, MemoryStore::default());
        let error = client
            .transformer_inventory("project_a", &TransformerSet::default(), NOW + 1)
            .unwrap_err();
        assert_eq!(error.kind(), expected);
        assert!(!error.to_string().contains("private backend detail"));
    }
}

#[test]
fn compounded_no_artifacts_424_is_a_bounded_report_refusal() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project_report(
        424,
        br#"{"status":"failed","code":"no_individual_transformer_artifacts","detail":"private backend detail"}"#,
    );
    let mut client = signed_in(transport, MemoryStore::default());
    let request = CompoundedReportRequest::new(
        TransformerSet::default(),
        ReportFileLevel::Transformer,
        false,
        false,
    );
    let error = client
        .compounded_report("project_a", &request, NOW + 1)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    // The refusal now names the route's own cause instead of the one sentence
    // that four unrelated statuses shared. The bound this test was written to
    // protect is unchanged and asserted below: the code crosses, the backend's
    // own prose does not.
    assert_eq!(
        error.project_report_service_code(),
        Some(ProjectReportServiceCode::NoIndividualArtifacts)
    );
    assert_eq!(
        error.to_string(),
        "no selected transformer had an individual report to package"
    );
    assert!(!error.to_string().contains("private backend detail"));
}

/// The cloud individual-report computation is the seventh `/report`
/// operation: `export_reports_only`, on the Fast lane and the compounded
/// deadline, with the exact transformer list and nothing else. The route's
/// 200 and 207 are both receipts — the 207 is its own "some row errored" —
/// and every row comes back in request order with what the cloud did.
#[test]
fn export_reports_sends_the_fast_lane_action_and_decodes_both_verdicts() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project_report(
        200,
        &serde_json::to_vec(&json!({
            "message": "Success",
            "included": ["akagerero", "tx_b"],
            "skipped": [],
            "failed": [],
            "results": [
                {"name": "akagerero", "status": "success"},
                {"name": "tx_b", "status": "success"}
            ]
        }))
        .unwrap(),
    );
    transport.push_project_report(
        207,
        &serde_json::to_vec(&json!({
            "message": "Partial success",
            "included": ["akagerero"],
            "skipped": ["tx_b"],
            "failed": ["tx_c"],
            "results": [
                {"name": "akagerero", "status": "success"},
                {"name": "tx_b", "status": "skipped", "reason": "report artifacts are fresh; no re-export needed"},
                {"name": "tx_c", "status": "error", "error_message": "reporter unreachable"}
            ]
        }))
        .unwrap(),
    );
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    let two = TransformerSet::new(["Akagerero".to_owned(), "tx_b".to_owned()]).unwrap();
    let receipt = client.export_reports("project_a", &two, NOW + 1).unwrap();
    assert_eq!(receipt.project(), "project_a");
    assert!(!receipt.partial());
    assert_eq!(receipt.computed(), ["akagerero", "tx_b"]);
    assert!(receipt.skipped().is_empty());
    assert!(receipt.failed().is_empty());

    let three = TransformerSet::new(["akagerero".to_owned(), "tx_b".to_owned(), "tx_c".to_owned()])
        .unwrap();
    let receipt = client.export_reports("project_a", &three, NOW + 1).unwrap();
    assert!(receipt.partial());
    assert_eq!(receipt.computed(), ["akagerero"]);
    assert_eq!(receipt.skipped(), ["tx_b"]);
    assert_eq!(receipt.failed(), ["tx_c"]);
    let rows = receipt.results();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].outcome(), ExportReportOutcome::Success);
    assert_eq!(rows[0].reason(), None);
    assert_eq!(rows[1].outcome(), ExportReportOutcome::Skipped);
    assert_eq!(
        rows[1].reason(),
        Some("report artifacts are fresh; no re-export needed")
    );
    assert_eq!(rows[2].outcome(), ExportReportOutcome::Error);
    assert_eq!(rows[2].error(), Some("reporter unreachable"));

    let calls = transport.calls();
    assert!(calls.iter().any(|call| call.contains(
        "project_report method=POST origin=https://eds-gateway-3c0q477h.ue.gateway.dev path=/report action=export_reports_only lane=fast content_type=application/json body={\"action\":\"export_reports_only\",\"eds_project_id\":\"project_a\",\"transformers\":[\"akagerero\",\"tx_b\"]} limit=8388608 timeout=600"
    )), "{calls:#?}");
    assert!(calls.iter().any(|call| call.contains(
        "body={\"action\":\"export_reports_only\",\"eds_project_id\":\"project_a\",\"transformers\":[\"akagerero\",\"tx_b\",\"tx_c\"]}"
    )));
    assert_eq!(
        transport
            .0
            .lock()
            .unwrap()
            .correlated_project_report_requests,
        2
    );
}

/// An empty set is refused before any bytes: on this route an empty list is
/// not "every active" but a fall-through to the retired generic proxy.
#[test]
fn export_reports_refuses_an_empty_set_locally() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let error = client
        .export_reports("project_a", &TransformerSet::default(), NOW + 1)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert!(
        !transport
            .calls()
            .iter()
            .any(|call| call.contains("export_reports_only")),
        "nothing was sent"
    );
}

/// The refusals the route emits for this action, each classified without
/// exposing backend prose: the 423 write-governance lock carries its typed
/// code and the holder's sentence as a bounded service refusal; the rest
/// classify as the report route always has.
#[test]
fn export_reports_refusals_are_classified() {
    let names = TransformerSet::new(["akagerero".to_owned()]).unwrap();
    for (status, body, expected) in [
        (
            423,
            br#"{"error_code":"TRANSFORMER_EDIT_LOCKED","detail":"Transformer is locked by other@example.com."}"#.as_slice(),
            ErrorKind::InvalidInput,
        ),
        (
            423,
            br#"{"error_code":"TRANSFORMER_FINAL_LOCKED","detail":"Transformer is permanently locked."}"#.as_slice(),
            ErrorKind::InvalidInput,
        ),
        (
            403,
            br#"{"detail":"missing design.download capability"}"#.as_slice(),
            ErrorKind::AuthenticationRejected,
        ),
        (
            400,
            br#"{"detail":"transformers must contain at least one transformer"}"#.as_slice(),
            ErrorKind::InvalidInput,
        ),
        (
            404,
            br#"{"detail":"private backend detail"}"#.as_slice(),
            ErrorKind::ResourceNotFound,
        ),
        (
            500,
            br#"{"detail":"Failed to verify transformer governance."}"#.as_slice(),
            ErrorKind::Transient,
        ),
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_project_report(status, body);
        let mut client = signed_in(transport, MemoryStore::default());
        let error = client
            .export_reports("project_a", &names, NOW + 1)
            .unwrap_err();
        assert_eq!(error.kind(), expected, "status {status}");
        assert!(!error.to_string().contains("private backend detail"));
        assert_eq!(error.project_report_service_code(), None);
        if status == 423 {
            let refusal = error.service_refusal().expect("the lock is named");
            assert_eq!(refusal.status(), 423);
            assert!(
                matches!(
                    refusal.code(),
                    Some("transformer_edit_locked") | Some("transformer_final_locked")
                ),
                "{:?}",
                refusal.code()
            );
            assert!(refusal.message().unwrap().contains("locked"));
            assert_eq!(
                error.to_string(),
                "the governed report service refused a locked transformer"
            );
        } else {
            assert_eq!(error.service_refusal(), None);
        }
    }
}

/// The receipt is closed. A member this decoder does not read, a row for a
/// transformer that was not asked for, a partition that disagrees with the
/// rows, a 207 with nothing failed, or a sentence that is not the route's
/// are all "not this route", never a partial reading.
#[test]
fn export_reports_receipt_is_closed() {
    let names = TransformerSet::new(["akagerero".to_owned(), "tx_b".to_owned()]).unwrap();
    let good = json!({
        "message": "Success",
        "included": ["akagerero", "tx_b"],
        "skipped": [],
        "failed": [],
        "results": [
            {"name": "akagerero", "status": "success"},
            {"name": "tx_b", "status": "success"}
        ]
    });
    let mutations: Vec<(u16, serde_json::Value)> = vec![
        (200, {
            let mut v = good.clone();
            v["job_id"] = json!("j-1");
            v
        }),
        (200, {
            let mut v = good.clone();
            v["results"][1]["name"] = json!("tx_z");
            v["included"][1] = json!("tx_z");
            v
        }),
        (200, {
            let mut v = good.clone();
            v["results"][1]["status"] = json!("queued");
            v
        }),
        (200, {
            let mut v = good.clone();
            v["included"] = json!(["akagerero"]);
            v
        }),
        (200, {
            let mut v = good.clone();
            v["message"] = json!("Partial success");
            v
        }),
        (207, good.clone()),
        (200, {
            let mut v = good.clone();
            v["results"][1] = json!({"name": "tx_b", "status": "error", "error_message": "x"});
            v["included"] = json!(["akagerero"]);
            v["failed"] = json!(["tx_b"]);
            v
        }),
        (200, {
            let mut v = good.clone();
            v["results"][1]["extra"] = json!(true);
            v
        }),
    ];
    for (status, body) in mutations {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_project_report(status, &serde_json::to_vec(&body).unwrap());
        let mut client = signed_in(transport, MemoryStore::default());
        let error = client
            .export_reports("project_a", &names, NOW + 1)
            .unwrap_err();
        assert_eq!(
            error.kind(),
            ErrorKind::UnreadableResponse,
            "{status} {body}"
        );
    }
    // And the unmutated body is a receipt.
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project_report(200, &serde_json::to_vec(&good).unwrap());
    let mut client = signed_in(transport, MemoryStore::default());
    assert!(client.export_reports("project_a", &names, NOW + 1).is_ok());
}

fn jwt(claims: serde_json::Value) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    format!(
        "e30.{}.signature",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    )
}

fn refresh_body(refresh_token: &str) -> Vec<u8> {
    refresh_body_with_tokens(Some(valid_jwt()), None, refresh_token)
}

fn valid_jwt() -> String {
    jwt(serde_json::json!({
        "sub": "uid-1",
        "aud": "firebase-project",
        "email": "user@example.com",
        "iss": "https://securetoken.google.com/firebase-project",
        "exp": 2_000_000_000_u64
    }))
}

fn refresh_body_with_tokens(
    access_token: Option<String>,
    id_token: Option<String>,
    refresh_token: &str,
) -> Vec<u8> {
    let mut value = serde_json::json!({
        "user_id": "uid-1",
        "refresh_token": refresh_token,
        "expires_in": "3600",
        "token_type": "Bearer",
        "project_id": 123456789012_u64
    });
    let object = value.as_object_mut().unwrap();
    if let Some(token) = access_token {
        object.insert("access_token".to_owned(), serde_json::Value::String(token));
    }
    if let Some(token) = id_token {
        object.insert("id_token".to_owned(), serde_json::Value::String(token));
    }
    serde_json::to_vec(&value).unwrap()
}

struct DeviceAuthorizedProbe {
    inner: FixtureTransport,
    seen: Vec<&'static str>,
    pm_bodies: Vec<serde_json::Value>,
}

impl DeviceAuthorizedProbe {
    fn new(inner: FixtureTransport) -> Self {
        Self {
            inner,
            seen: Vec::new(),
            pm_bodies: Vec::new(),
        }
    }

    fn verify(
        uid: &str,
        email: &str,
        device_id: Option<&str>,
        bearer: &str,
    ) -> Result<(), TransportError> {
        assert_eq!(uid, "uid-1");
        assert_eq!(email, "user@example.com");
        assert_eq!(device_id, Some("device-1"));
        assert_eq!(bearer, "ds-memory-access-jwt");
        Ok(())
    }
}

macro_rules! device_authorized_call {
    ($self:ident, $call:ident, $name:literal, $method:ident) => {{
        Self::verify(
            $call.canonical_uid(),
            $call.canonical_email(),
            $call.device_id(),
            $call.bearer_token(),
        )?;
        $self.seen.push($name);
        Transport::$method(&mut $self.inner, $call)
    }};
}

impl AuthorizedTransport for DeviceAuthorizedProbe {
    fn project_management(
        &mut self,
        call: ds_client_core::ProjectManagementCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(call.canonical_uid(), "uid-1");
        assert_eq!(call.canonical_email(), "user@example.com");
        assert_eq!(call.device_id(), Some("device-1"));
        assert_eq!(call.bearer_token(), "ds-memory-access-jwt");
        assert_eq!(
            call.gateway_origin(),
            "https://eds-gateway-3c0q477h.ue.gateway.dev"
        );
        assert_eq!(call.path(), "/api/v1/pm");
        self.pm_bodies
            .push(serde_json::from_slice(call.body().as_bytes()).unwrap());
        Ok(TransportResponse::new(
            200,
            br#"{"success":true,"data":{"projects":[],"status":"active","next_cursor":""}}"#
                .to_vec(),
        ))
    }

    fn project_data(
        &mut self,
        call: ds_client_core::ProjectDataCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::project_data(&mut self.inner, call)
    }
    fn upload_bytes(
        &mut self,
        call: ds_client_core::UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::upload_bytes(&mut self.inner, call)
    }

    fn survey_control(
        &mut self,
        call: ds_client_core::SurveyControlCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "survey_control", survey_control)
    }
    fn styles(
        &mut self,
        call: ds_client_core::StylesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::styles(&mut self.inner, call)
    }

    fn layers(
        &mut self,
        call: ds_client_core::LayersCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "layers", layers)
    }

    fn list_projects(
        &mut self,
        call: ProjectListCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "project", list_projects)
    }
    fn transformer_context(
        &mut self,
        call: TransformerContextCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "transformer", transformer_context)
    }
    fn project_forms(
        &mut self,
        call: ProjectFormsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "forms", project_forms)
    }
    fn project_form_editor(
        &mut self,
        call: ProjectFormEditorCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "form-editor", project_form_editor)
    }
    fn solar_snapshot(
        &mut self,
        call: SolarSnapshotCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "solar", solar_snapshot)
    }
    fn survey_query(
        &mut self,
        call: SurveyQueryCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "query", survey_query)
    }
    fn survey_entries_select(
        &mut self,
        call: SurveyEntriesSelectCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "select", survey_entries_select)
    }
    fn survey_entries_changes(
        &mut self,
        call: SurveyEntriesChangesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "changes", survey_entries_changes)
    }
    fn survey_entry_create(
        &mut self,
        call: SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "create", survey_entry_create)
    }
    fn tiles(&mut self, call: TileCall<'_>) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "tiles", tiles)
    }
    fn project_report(
        &mut self,
        call: ProjectReportCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "report", project_report)
    }
    fn installs(
        &mut self,
        call: ds_client_core::InstallsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "installs", installs)
    }
    fn project_properties(
        &mut self,
        call: ds_client_core::ProjectPropertiesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "project_properties", project_properties)
    }
    fn sre_overview(
        &mut self,
        call: ds_client_core::SreOverviewCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "sre", sre_overview)
    }
    fn admin_bounds(
        &mut self,
        call: ds_client_core::AdminBoundsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "admin_bounds", admin_bounds)
    }
    fn grid_catalog(
        &mut self,
        call: ds_client_core::GridCatalogCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "grid_catalog", grid_catalog)
    }
    fn global_tiles(
        &mut self,
        call: ds_client_core::GlobalTilesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        device_authorized_call!(self, call, "global_tiles", global_tiles)
    }
}

fn device_credential(profile: &ClientProfile) -> DeviceCredential {
    DeviceCredential::new(
        "device-1".to_owned(),
        "Workstation".to_owned(),
        "linux".to_owned(),
        format!("sha256:{}", "c".repeat(64)),
        "uid-1".to_owned(),
        "user@example.com".to_owned(),
        &DeviceBinding::for_profile(profile, &"b".repeat(64)).unwrap(),
        "2031-01-01T00:00:00Z".to_owned(),
        DevicePrivateKey::from_secret_bytes([5; 32]),
    )
    .unwrap()
}

#[test]
fn device_authorization_executes_every_existing_fixed_domain_operation() {
    let profile = profile(DeploymentLane::Stable);
    let credential = device_credential(&profile);
    let session = DeviceAccessSession::new("ds-memory-access-jwt".to_owned(), NOW + 300, NOW)
        .expect("bounded memory session");
    let authorization = credential
        .authorize_api(&session, &profile, "uid-1", NOW)
        .expect("exact device authority");
    let context = credential.auth_context();
    assert_eq!(context.device_name(), "Workstation");
    assert_eq!(context.fingerprint(), format!("sha256:{}", "c".repeat(64)));
    assert_eq!(context.lane(), "stable");
    assert_eq!(
        context.credential_audience_sha256(),
        profile.credential_audience_sha256()
    );

    let fixture = FixtureTransport::default();
    fixture.push_project(ACTIVE);
    fixture.push_project(ARCHIVED);
    fixture.push_project(TESTING);
    fixture.push_transformer_context(
        200,
        &serde_json::to_vec(&serde_json::json!({
            "success": true,
            "data": {"total":1,"found_count":1,"failed_count":0,"results":[{
                "transformer_name":"tx_1","ok":true,"data":{"layers":{},"metadata":{}}
            }]}
        }))
        .unwrap(),
    );
    fixture.push_project_forms(
        200,
        &serde_json::to_vec(&serde_json::json!({
            "status":"success","project_id":"active-project","resolved_at":NOW,
            "user":{"email":"user@example.com","roles":[],"is_owner":false,"is_admin":false},
            "forms":[],"field_types":[],"condition_operators":[],"type_aliases":{}
        }))
        .unwrap(),
    );
    fixture.push_project_form_editor(200, &serde_json::to_vec(&settings_editor_body()).unwrap());
    fixture.push_solar_snapshot(
        200,
        &serde_json::to_vec(&solar_snapshot_envelope(
            "eds_project/active-project/eds_solar",
            "city_1",
        ))
        .unwrap(),
    );
    fixture.push_survey_query(200, &serde_json::to_vec(&survey_query_envelope()).unwrap());
    fixture.push_survey_entries_select(200, &survey_entries_select_envelope());
    fixture.push_survey_entries_changes(200, &survey_entries_changes_envelope(true));
    fixture.push_survey_entry_create(201, &survey_entry_create_envelope());
    push_layers(&fixture, headless_style_fixture("project_a"));
    fixture.push_tiles(
        200,
        &tile_status_envelope("completed", "project_a", "design"),
    );
    fixture.push_tiles(200, &tile_preflight_envelope());
    fixture.push_tiles(
        200,
        &serde_json::to_vec(&serde_json::json!({
            "status": "started",
            "eds_project_id": "project_a",
            "tile_type": "design",
            "message": "Tile generation started."
        }))
        .unwrap(),
    );
    fixture.push_project_report(200, &inventory_envelope("project_a"));
    fixture.push_project_report(200, &status_envelope());
    let mut transport = DeviceAuthorizedProbe::new(fixture);

    assert_eq!(
        authorization
            .list_projects(&mut transport, NOW)
            .unwrap()
            .projects()
            .len(),
        3
    );
    assert_eq!(
        authorization
            .transformer_context(&mut transport, "active-project", "tx_1", NOW)
            .unwrap()
            .transformer_name(),
        "tx_1"
    );
    assert_eq!(
        authorization
            .project_forms(&mut transport, "active-project", NOW)
            .unwrap()
            .ds_project(),
        "active-project"
    );
    assert_eq!(
        authorization
            .project_form_editor(&mut transport, "active-project", "a_poles", NOW)
            .unwrap()
            .form_slug(),
        "a_poles"
    );
    assert_eq!(
        authorization
            .solar_snapshot(&mut transport, "active-project", "city_1", NOW)
            .unwrap()
            .template_id(),
        "city_1"
    );
    assert_eq!(
        authorization
            .survey_query(
                &mut transport,
                "active-project",
                &survey_query_request(),
                NOW
            )
            .unwrap()
            .form(),
        "a_poles"
    );
    assert_eq!(
        authorization
            .survey_entries_select(
                &mut transport,
                "project_a",
                &survey_entries_select_request(),
                NOW,
            )
            .unwrap()
            .project(),
        "project_a"
    );
    assert_eq!(
        authorization
            .survey_entries_changes(
                &mut transport,
                "project_a",
                &survey_entries_changes_request(),
                NOW,
            )
            .unwrap()
            .project(),
        "project_a"
    );
    assert_eq!(
        authorization
            .survey_entry_create(
                &mut transport,
                "project_a",
                &survey_entry_create_request(),
                NOW,
            )
            .unwrap()
            .doc_id(),
        "entry_1"
    );
    assert_eq!(
        authorization
            .style_catalog(&mut transport, "project_a", NOW)
            .unwrap()
            .project_id(),
        "project_a"
    );
    assert_eq!(
        authorization
            .tile_status(&mut transport, "project_a", TileType::Design, NOW)
            .unwrap()
            .status(),
        TileOperationStatus::Completed
    );
    assert_eq!(
        authorization
            .tile_preflight(&mut transport, "project_a", TileType::Survey, NOW)
            .unwrap()
            .status(),
        TilePreflightStatus::Ready
    );
    assert_eq!(
        authorization
            .tile_generate(&mut transport, "project_a", TileType::Design, false, NOW,)
            .unwrap()
            .status(),
        TileOperationStatus::Started
    );
    assert_eq!(
        authorization
            .transformer_inventory(&mut transport, "project_a", &TransformerSet::default(), NOW)
            .unwrap()
            .active_count(),
        2
    );
    // The status read reaches the same /report route under the device
    // credential, not only under the Firebase session.
    assert_eq!(
        authorization
            .transformer_status(&mut transport, "project_a", &TransformerSet::default(), NOW)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        transport.seen,
        [
            "project",
            "project",
            "project",
            "transformer",
            "forms",
            "form-editor",
            "solar",
            "query",
            "select",
            "changes",
            "create",
            "layers",
            "tiles",
            "tiles",
            "tiles",
            "report",
            "report",
        ]
    );
}

#[test]
fn device_authorization_refuses_profile_uid_and_expired_session_before_transport() {
    let stable = profile(DeploymentLane::Stable);
    let credential = device_credential(&stable);
    let live = DeviceAccessSession::new("ds-memory-access-jwt".to_owned(), NOW + 300, NOW).unwrap();
    assert_eq!(
        credential
            .authorize_api(&live, &stable, "other-uid", NOW)
            .err()
            .expect("UID mismatch"),
        ds_client_core::DeviceError::AuthorityMismatch
    );
    assert_eq!(
        credential
            .authorize_api(&live, &profile(DeploymentLane::Canary), "uid-1", NOW,)
            .err()
            .expect("lane/audience mismatch"),
        ds_client_core::DeviceError::AuthorityMismatch
    );

    let authorization = credential
        .authorize_api(&live, &stable, "uid-1", NOW)
        .unwrap();
    let mut transport = DeviceAuthorizedProbe::new(FixtureTransport::default());
    let error = authorization
        .list_projects(&mut transport, NOW + 300)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::SignedOut);
    assert!(transport.seen.is_empty());
}

#[test]
fn project_management_device_public_index_keeps_actor_lane_and_project_fences() {
    use ds_client_core::project_management::Command;

    let stable = profile(DeploymentLane::Stable);
    let credential = device_credential(&stable);
    let live = DeviceAccessSession::new("ds-memory-access-jwt".to_owned(), NOW + 300, NOW).unwrap();
    assert_eq!(
        credential
            .authorize_api(&live, &stable, "other-uid", NOW)
            .err()
            .unwrap(),
        ds_client_core::DeviceError::AuthorityMismatch
    );
    assert_eq!(
        credential
            .authorize_api(&live, &profile(DeploymentLane::Canary), "uid-1", NOW)
            .err()
            .unwrap(),
        ds_client_core::DeviceError::AuthorityMismatch
    );
    let authorization = credential
        .authorize_api(&live, &stable, "uid-1", NOW)
        .unwrap();
    let mut transport = DeviceAuthorizedProbe::new(FixtureTransport::default());
    let index = Command::PublicProjects {
        status: "active".into(),
        cursor: String::new(),
        limit: 50,
    };
    assert_eq!(
        authorization
            .project_management(&mut transport, "", &index, NOW)
            .unwrap()["projects"],
        json!([])
    );
    assert_eq!(
        transport.pm_bodies,
        vec![json!({"action":"list_public_projects","status":"active","limit":50})]
    );
    // The exemption names one closed read: a supplied project is still refused.
    assert_eq!(
        authorization
            .project_management(&mut transport, "project-a", &index, NOW)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    let comments = Command::TaskComments {
        task_id: "t".into(),
        cursor: String::new(),
        limit: 20,
    };
    for project in ["", " project-a", "project-a "] {
        assert_eq!(
            authorization
                .project_management(&mut transport, project, &comments, NOW)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidInput
        );
    }
    assert_eq!(
        authorization
            .project_management(&mut transport, &"a".repeat(201), &comments, NOW)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    for project in ["project-a", "project-b", "project-a"] {
        authorization
            .project_management(&mut transport, project, &comments, NOW)
            .unwrap();
    }
    assert_eq!(
        transport
            .pm_bodies
            .iter()
            .skip(1)
            .map(|body| body["ds_project"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["project-a", "project-b", "project-a"]
    );
    assert_eq!(
        authorization
            .project_management(&mut transport, "", &index, NOW + 300)
            .unwrap_err()
            .kind(),
        ErrorKind::SignedOut
    );
    assert_eq!(
        transport.pm_bodies.len(),
        4,
        "refused calls must not reach transport"
    );
}

/// Governance is server-side: the five projectless doors answer a device
/// credential exactly as they answer a restored user, so a box linked by
/// device may read the estate. Before this, `ds install list --lane canary`
/// refused `headless_signed_out` on a host whose `ds auth status` said signed
/// in — the surface simply had no method to call.
#[test]
fn device_authorization_reaches_the_projectless_governance_doors() {
    use ds_client_core::admin_bounds::{Answer, Country, Level};
    let stable = profile(DeploymentLane::Stable);
    let credential = device_credential(&stable);
    let live = DeviceAccessSession::new("ds-memory-access-jwt".to_owned(), NOW + 300, NOW).unwrap();
    let authorization = credential
        .authorize_api(&live, &stable, "uid-1", NOW)
        .unwrap();

    let fixture = FixtureTransport::default();
    fixture.push_governance(
        200,
        &serde_json::to_vec(&json!({
            "success": true,
            "data": {"installs": [{"install_id": "0cd65a76-a5be-440c-b82e-c3442b8c1e40"}], "next_cursor": null}
        }))
        .unwrap(),
    );
    fixture.push_governance(
        200,
        &serde_json::to_vec(
            &json!({"generated_at": "2026-09-19T00:00:00Z", "fleet": {}, "combined_reports": {}}),
        )
        .unwrap(),
    );
    fixture.push_governance(
        200,
        &serde_json::to_vec(&json!({"children": [{"code": "1", "name": "Kigali City"}]})).unwrap(),
    );
    fixture.push_governance(
        200,
        &serde_json::to_vec(&json!({"success": true, "data": {"libraries": []}})).unwrap(),
    );
    fixture.push_governance(
        200,
        &serde_json::to_vec(&json!({"success": true, "data": {"tiles": []}})).unwrap(),
    );
    let mut transport = DeviceAuthorizedProbe::new(fixture);

    let installs = authorization
        .installs(
            &mut transport,
            &ds_client_core::installs::Command::List {
                cursor: None,
                limit: 50,
            },
            NOW,
        )
        .unwrap();
    assert_eq!(installs["installs"].as_array().unwrap().len(), 1);
    let overview = authorization
        .sre(&mut transport, &ds_client_core::sre::Command::Overview, NOW)
        .unwrap();
    assert_eq!(overview["generated_at"], "2026-09-19T00:00:00Z");
    let provinces = authorization
        .admin_bounds(
            &mut transport,
            &ds_client_core::admin_bounds::Command::Children {
                country: Country::Rwanda,
                level: Level::Province,
                parent_code: None,
            },
            NOW,
        )
        .unwrap();
    assert!(matches!(provinces, Answer::Children { ref boundaries, .. } if boundaries.len() == 1));
    let libraries = authorization
        .grid_catalog(
            &mut transport,
            &ds_client_core::grid_catalog::Command::ListLibraries,
            NOW,
        )
        .unwrap();
    assert_eq!(libraries["libraries"], json!([]));
    let tiles = authorization
        .global_tiles(
            &mut transport,
            &ds_client_core::global_tiles::Command::List {
                domain: ds_client_core::global_tiles::Domain::NetworkTemplate,
            },
            NOW,
        )
        .unwrap();
    assert_eq!(tiles["tiles"], json!([]));

    // Every door carried the device authority (uid, email, device id, memory
    // bearer) — the probe asserts each — and knocked once, in order.
    assert_eq!(
        transport.seen,
        [
            "installs",
            "sre",
            "admin_bounds",
            "grid_catalog",
            "global_tiles"
        ]
    );
    let calls = transport.inner.calls();
    assert!(
        calls[0].contains("\"action\":\"list_installs\""),
        "{}",
        calls[0]
    );
    assert_eq!(calls[1], "sre_overview");
    assert!(
        calls[2].ends_with("action=children&level=province"),
        "{}",
        calls[2]
    );
    assert!(calls[3].contains("list_libraries"), "{}", calls[3]);
    assert!(calls[4].contains("\"action\""), "{}", calls[4]);

    // An expired memory session is refused before any I/O, like every other
    // door on this surface.
    let mut idle = DeviceAuthorizedProbe::new(FixtureTransport::default());
    let error = authorization
        .installs(
            &mut idle,
            &ds_client_core::installs::Command::List {
                cursor: None,
                limit: 1,
            },
            NOW + 300,
        )
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::SignedOut);
    assert!(idle.seen.is_empty());
}

/// Creating a project and editing its properties needs no window: the same
/// door answers the restored native user and a linked device, at the one
/// hard-coded route, with the exact body the handler reads. Before this the
/// two integration test projects could not be named from any headless host.
#[test]
fn project_properties_reach_the_projects_route_as_native_user_and_as_device() {
    use ds_client_core::project_properties::{Command, Properties, Receipt};
    let update = Command::Update {
        project_id: "it_rwanda".into(),
        properties: Properties {
            display_name: Some("Integration test — Rwanda".into()),
            country: Some("Rwanda".into()),
            ..Properties::default()
        },
    };
    let create = Command::Create {
        display_name: "Gisagara LV".into(),
        properties: Properties {
            country: Some("Rwanda".into()),
            ..Properties::default()
        },
        network_template: None,
        styling_template: None,
    };
    let updated = serde_json::to_vec(&json!({
        "success": true,
        "message": "Project updated successfully",
        "data": {"project_id": "it_rwanda", "resources_ensured": true, "dataset_exists": true, "fields_updated": 2},
        "timestamp": "2026-09-20T10:00:00Z", "service": "data-solutions-backend", "version": "1.0.0"
    }))
    .unwrap();
    let created = serde_json::to_vec(&json!({
        "success": true,
        "message": "Project created successfully",
        "data": {
            "project_id": "uid1_gisagara_lv", "project_name": "gisagara_lv", "display_name": "Gisagara LV",
            "project_location": "", "network_template": "master",
            "project_params": {"project_type": "new_build", "crs": {"mode": "tm_rwanda"}},
            "project_phases": {}, "project_components": {}, "processing_lanes": {"fast": true},
            "project_type": "new_build", "crs": {"mode": "tm_rwanda"}
        },
        "timestamp": "2026-09-20T10:00:00Z", "service": "data-solutions-backend", "version": "1.0.0"
    }))
    .unwrap();
    let forbidden = serde_json::to_vec(&json!({
        "success": false,
        "error": {"code": "INSUFFICIENT_PERMISSIONS", "message": "Requires project.properties.edit capability (project admin)", "timestamp": 1},
        "timestamp": "2026-09-20T10:00:00Z", "service": "data-solutions-backend", "version": "1.0.0"
    }))
    .unwrap();

    // The restored native user.
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_project_properties(200, &updated);
    transport.push_project_properties(200, &created);
    transport.push_project_properties(403, &forbidden);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let receipt = client.project_properties(&update, NOW + 1).unwrap();
    assert!(
        matches!(receipt, Receipt::Updated(ref r) if r.project_id == "it_rwanda" && r.fields_updated == 2)
    );
    let receipt = client.project_properties(&create, NOW + 1).unwrap();
    assert!(
        matches!(receipt, Receipt::Created(ref r) if r.project_id == "uid1_gisagara_lv" && r.project_name == "gisagara_lv")
    );
    let error = client.project_properties(&update, NOW + 1).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::AuthenticationRejected);
    assert_eq!(
        error.project_properties_service_code(),
        Some(ds_client_core::ProjectPropertiesServiceCode::CapabilityMissing)
    );
    assert_eq!(
        error.service_refusal().and_then(|r| r.message()),
        Some("Requires project.properties.edit capability (project admin)")
    );
    let calls = transport.calls();
    let properties: Vec<&String> = calls
        .iter()
        .filter(|call| call.starts_with("project_properties "))
        .collect();
    assert_eq!(properties.len(), 3);
    assert_eq!(
        properties[0].as_str(),
        r#"project_properties origin=https://eds-gateway-3c0q477h.ue.gateway.dev api_key=true user_id_token=true device=false email=user@example.com body={"action":"update","data":{"country":"Rwanda","display_name":"Integration test — Rwanda","project_id":"it_rwanda"}}"#
    );
    assert!(properties[1].ends_with(
        r#"body={"action":"create","data":{"country":"Rwanda","display_name":"Gisagara LV","project_name":"gisagara_lv"}}"#
    ), "{}", properties[1]);

    // An invalid command never reaches the transport.
    let error = client
        .project_properties(
            &Command::Update {
                project_id: "it_rwanda".into(),
                properties: Properties::default(),
            },
            NOW + 1,
        )
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert_eq!(transport.calls().len(), calls.len());

    // The linked device: same door, device authority on the call.
    let stable = profile(DeploymentLane::Stable);
    let credential = device_credential(&stable);
    let live = DeviceAccessSession::new("ds-memory-access-jwt".to_owned(), NOW + 300, NOW).unwrap();
    let authorization = credential
        .authorize_api(&live, &stable, "uid-1", NOW)
        .unwrap();
    let fixture = FixtureTransport::default();
    fixture.push_project_properties(200, &updated);
    let mut probe = DeviceAuthorizedProbe::new(fixture);
    let receipt = authorization
        .project_properties(&mut probe, &update, NOW)
        .unwrap();
    assert!(matches!(receipt, Receipt::Updated(ref r) if r.project_id == "it_rwanda"));
    assert_eq!(probe.seen, ["project_properties"]);
    let calls = probe.inner.calls();
    assert!(
        calls[0].contains(
            "user_id_token=false device=true email=user@example.com body={\"action\":\"update\""
        ),
        "{}",
        calls[0]
    );

    // An expired memory session is refused before any I/O.
    let mut idle = DeviceAuthorizedProbe::new(FixtureTransport::default());
    let error = authorization
        .project_properties(&mut idle, &update, NOW + 300)
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::SignedOut);
    assert!(idle.seen.is_empty());
}

#[test]
fn grid_catalog_member_sends_exact_pins_and_preserves_server_evidence_without_fetching() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let data = grid_catalog_member_data();
    transport.push_governance(
        200,
        &serde_json::to_vec(&json!({"success": true, "data": data})).unwrap(),
    );
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let before = transport.calls().len();
    let command = grid_catalog_member_pin();
    assert_eq!(command.action(), "resolve_library_member");
    assert!(!command.writes());
    let result = client.grid_catalog(&command, NOW + 1).unwrap();
    assert_eq!(
        result, data,
        "preserve the exact inventory record and signed URL"
    );
    let calls = transport.calls();
    assert_eq!(
        calls.len(),
        before + 1,
        "only one catalog call, no byte fetch"
    );
    let body: serde_json::Value = serde_json::from_str(
        calls
            .last()
            .unwrap()
            .strip_prefix("grid_catalog body=")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        body,
        json!({
            "action": "resolve_library_member",
            "library_id": "library_1",
            "release_id": "release_1",
            "relative_path": "pls-cadd/criteria/Base.CRI",
            "expected_digest": "a".repeat(64),
        })
    );
}

fn grid_catalog_member_pin() -> ds_client_core::grid_catalog::Command {
    ds_client_core::grid_catalog::Command::ResolveLibraryMember {
        library_id: "library_1".into(),
        release_id: "release_1".into(),
        relative_path: "pls-cadd/criteria/Base.CRI".into(),
        expected_digest: "a".repeat(64),
    }
}

fn grid_catalog_member_data() -> serde_json::Value {
    json!({
        "library_id": "library_1",
        "release_id": "release_1",
        "member": {
            "relative_path": "pls-cadd/criteria/Base.CRI",
            "class": "criteria",
            "artifact": {
                "digest": "a".repeat(64),
                "object": format!("ds-grid/objects/sha256/aa/{}", "a".repeat(64)),
                "byte_length": 42,
            },
            "provenance": {
                "kind": "seed", "reference": "reviewed seed", "source_digest": "b".repeat(64),
            },
            "external_definition": {
                "system": "pls_cadd", "definition_id": "base_criteria", "revision": "r1",
            },
        },
        "download_url": "https://storage.googleapis.com/catalog/object?generation=123&X-Goog-Signature=Ab%2Fcd%2Bef&X-Goog-Expires=900",
        "expires_at": "2030-03-17T17:01:40.123456789Z",
    })
}

#[test]
fn grid_catalog_member_invalid_pins_are_rejected_before_any_network_even_when_session_expired() {
    use ds_client_core::grid_catalog::{Command, MAX_MEMBER_PATH_BYTES};
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let before = transport.calls();
    let mut invalid = Vec::new();
    for bad in [
        "",
        " head",
        "release_1 ",
        "a/b",
        "a\\b",
        "a\nb",
        "LIBRARY_1",
        "x",
        "https://host",
    ] {
        for library in [true, false] {
            invalid.push(Command::ResolveLibraryMember {
                library_id: if library {
                    bad.into()
                } else {
                    "library_1".into()
                },
                release_id: if library {
                    "release_1".into()
                } else {
                    bad.into()
                },
                relative_path: "pls-cadd/criteria/Base.CRI".into(),
                expected_digest: "a".repeat(64),
            });
        }
    }
    for bad in [
        "",
        "/Base.CRI",
        "//server/Base.CRI",
        "C:/Base.CRI",
        "C:\\Base.CRI",
        "pls-cadd\\Base.CRI",
        "../Base.CRI",
        "pls-cadd/../Base.CRI",
        "pls-cadd/./Base.CRI",
        "pls-cadd//Base.CRI",
        "pls-cadd/Base.CRI/",
        "pls-cadd/Base\n.CRI",
        "pls-cadd/Base\0.CRI",
        "pls-cadd/Base\u{7f}.CRI",
        "pls-cadd/%2e%2e/Base.CRI",
        " Base.CRI",
        "Base.CRI ",
    ]
    .map(str::to_owned)
    .into_iter()
    .chain([
        "x".repeat(MAX_MEMBER_PATH_BYTES + 1),
        format!("pls-cadd/{}", "x".repeat(129)),
        (0..5)
            .map(|_| "x".repeat(110))
            .collect::<Vec<_>>()
            .join("/"),
    ]) {
        invalid.push(Command::ResolveLibraryMember {
            library_id: "library_1".into(),
            release_id: "release_1".into(),
            relative_path: bad,
            expected_digest: "a".repeat(64),
        });
    }
    for bad in [
        "".into(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
        format!("sha256:{}", "a".repeat(64)),
        format!("{} ", "a".repeat(64)),
        format!("{}\n", "a".repeat(63)),
    ] {
        invalid.push(Command::ResolveLibraryMember {
            library_id: "library_1".into(),
            release_id: "release_1".into(),
            relative_path: "pls-cadd/criteria/Base.CRI".into(),
            expected_digest: bad,
        });
    }
    invalid.push(Command::ResolveLibraryMember {
        library_id: "x".repeat(129),
        release_id: "x".repeat(129),
        relative_path: "Base.CRI".into(),
        expected_digest: "a".repeat(64),
    });
    for command in invalid {
        assert_eq!(
            client
                .grid_catalog(&command, NOW + 7200)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidInput
        );
    }
    assert_eq!(
        transport.calls(),
        before,
        "invalid pins must not refresh or contact the catalog"
    );
    for path in [
        "Base.CRI".into(),
        "pls-cadd/criteria/Base File.CRI".into(),
        [
            "x".repeat(128),
            "x".repeat(128),
            "x".repeat(128),
            "x".repeat(125),
        ]
        .join("/"),
    ] {
        assert!(Command::ResolveLibraryMember {
            library_id: "x".repeat(128),
            release_id: "r1".into(),
            relative_path: path,
            expected_digest: "0123456789abcdef".repeat(4),
        }
        .validate()
        .is_ok());
    }
}

#[test]
fn grid_catalog_member_legacy_inventory_404_and_server_validation_keep_existing_error_mapping() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    for (status, code, kind) in [
        (404, "NOT_FOUND", ErrorKind::ResourceNotFound),
        (400, "VALIDATION_FAILED", ErrorKind::InvalidInput),
    ] {
        transport.push_governance(status, &serde_json::to_vec(&json!({
            "success": false, "error": {"code": code, "message": "legacy release has no inventory"},
        })).unwrap());
        assert_eq!(
            client
                .grid_catalog(&grid_catalog_member_pin(), NOW + 1)
                .unwrap_err()
                .kind(),
            kind
        );
    }
    assert_eq!(
        transport.count_calls("grid_catalog"),
        2,
        "no retry or legacy inventory fallback"
    );
}

#[test]
fn grid_catalog_member_rejects_wrong_pins_malformed_evidence_and_unsafe_urls() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let mut responses = Vec::new();
    for (pointer, value) in [
        ("/library_id", json!("other_library")),
        ("/release_id", json!("other_release")),
        ("/member/relative_path", json!("pls-cadd/criteria/base.cri")),
        ("/member/artifact/digest", json!("b".repeat(64))),
        ("/member", json!([])),
        ("/member/class", json!(null)),
        ("/member/artifact", json!(null)),
        ("/member/artifact/object", json!(7)),
        ("/member/artifact/byte_length", json!("42")),
        ("/member/artifact/byte_length", json!(-1)),
        ("/member/artifact/byte_length", json!(0)),
        ("/member/artifact/byte_length", json!(1.5)),
        ("/member/provenance", json!(null)),
        ("/member/provenance/kind", json!(false)),
        ("/member/provenance/reference", json!(null)),
        ("/member/provenance/source_digest", json!(1)),
        ("/member/external_definition", json!({})),
        ("/member/external_definition", json!(null)),
        ("/member/external_definition/definition_id", json!(8)),
        ("/member/external_definition/revision", json!(false)),
        ("/expires_at", json!(null)),
        ("/expires_at", json!("tomorrow")),
        ("/download_url", json!(null)),
        ("/download_url", json!(5)),
    ] {
        let mut data = grid_catalog_member_data();
        *data.pointer_mut(pointer).unwrap() = value;
        responses.push(json!({"success": true, "data": data}));
    }
    for url in [
        "",
        "http://storage.googleapis.com/object",
        "https:/storage.googleapis.com/object",
        "https://storage.googleapis.com\\object",
        "file:///tmp/Base.CRI",
        "//storage.googleapis.com/object",
        "https://user:secret@storage.googleapis.com/object",
        "https://storage.googleapis.com/object#fragment",
        " https://storage.googleapis.com/object",
        "https://storage.googleapis.com/object\n",
        "https://storage.googleapis.com/ob\tject",
    ]
    .map(str::to_owned)
    .into_iter()
    .chain([format!("https://example.com/{}", "x".repeat(8192))])
    {
        let mut data = grid_catalog_member_data();
        data["download_url"] = json!(url);
        responses.push(json!({"success": true, "data": data}));
    }
    for key in [
        "library_id",
        "release_id",
        "member",
        "download_url",
        "expires_at",
    ] {
        let mut data = grid_catalog_member_data();
        data.as_object_mut().unwrap().remove(key);
        responses.push(json!({"success": true, "data": data}));
    }
    responses.extend([
        json!({"success": false, "data": grid_catalog_member_data()}),
        json!({"success": true}),
        json!({"success": true, "data": null}),
        json!({"success": true, "data": []}),
        json!({"success": true, "data": "not evidence"}),
    ]);
    for response in &responses {
        transport.push_governance(200, &serde_json::to_vec(response).unwrap());
        assert_eq!(
            client
                .grid_catalog(&grid_catalog_member_pin(), NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse,
            "accepted malformed evidence: {response}"
        );
    }
    for body in [b"invalid JSON".to_vec(), vec![b' '; 1024 * 1024 + 1]] {
        transport.push_governance(200, &body);
        assert_eq!(
            client
                .grid_catalog(&grid_catalog_member_pin(), NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }
    assert_eq!(transport.count_calls("grid_catalog"), responses.len() + 2);
}

#[test]
fn grid_catalog_member_preserves_optional_fields_and_does_not_rewrite_signed_url() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let mut data = grid_catalog_member_data();
    data["member"]
        .as_object_mut()
        .unwrap()
        .remove("external_definition");
    data["member"]["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("source_digest");
    data["download_url"] =
        json!("https://download.example/member?signature=Ab%2Fcd%2Bef&generation=123");
    data["server_evidence"] = json!({"generation": "123"});
    transport.push_governance(
        200,
        &serde_json::to_vec(&json!({"success": true, "data": data})).unwrap(),
    );
    assert_eq!(
        client
            .grid_catalog(&grid_catalog_member_pin(), NOW + 1)
            .unwrap(),
        data
    );
    assert_eq!(transport.count_calls("grid_catalog"), 1);
}

#[test]
fn headless_tile_catalog_list_add_remove_are_fixed_and_strip_storage_credentials() {
    use ds_client_core::TileScope;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    for wire in [
        serde_json::json!({"status":"success","eds_project_id":"project_a","count":1,"tiles":[{"id":"project_b_design","name":"Neighbour","scope":"project","tile_type":"design","source_project_id":"project_b","pmtiles_url":"https://secret.invalid/signed","pmtiles_path":"private/path"}]}),
        serde_json::json!({"status":"success","tile_id":"project_b_design","scope":"project","pmtiles_path":"private/path"}),
        serde_json::json!({"status":"success","tile_id":"project_b_design","scope":"project"}),
    ] {
        transport.push_tiles(200, &serde_json::to_vec(&wire).unwrap());
    }
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let list = client.tile_list("project_a", false, NOW + 1).unwrap();
    assert_eq!(list.tiles[0].id, "project_b_design");
    assert!(!serde_json::to_string(&list).unwrap().contains("private"));
    assert!(!serde_json::to_string(&list).unwrap().contains("secret"));
    assert_eq!(
        client
            .tile_add("project_a", TileType::Design, "project_b", NOW + 1)
            .unwrap()
            .tile_id,
        "project_b_design"
    );
    assert_eq!(
        client
            .tile_remove("project_a", "project_b_design", TileScope::Project, NOW + 1)
            .unwrap()
            .scope,
        TileScope::Project
    );
    let calls = transport.calls();
    assert!(calls
        .iter()
        .any(|v| v.contains("\"action\":\"list\"") && v.contains("\"include_global\":false")));
    assert!(calls
        .iter()
        .any(|v| v.contains("\"source_project_id\":\"project_b\"")));
    assert!(calls
        .iter()
        .any(|v| v.contains("\"action\":\"remove\"")
            && v.contains("\"tile_id\":\"project_b_design\"")));
    let count = calls.len();
    assert_eq!(
        client
            .tile_add("project_a", TileType::Design, "../other", NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(count, transport.calls().len());
}

#[test]
fn headless_tile_catalog_rejects_wrong_project_scope_and_false_success() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    for wire in [
        serde_json::json!({"status":"success","eds_project_id":"other","count":0,"tiles":null}),
        serde_json::json!({"status":"success","eds_project_id":"project_a","count":1,"tiles":[{"id":"global","name":"Global","scope":"global"}]}),
        serde_json::json!({"status":"error","error":"secret backend error"}),
        serde_json::json!({"status":"success","eds_project_id":"project_a","count":0}),
    ] {
        transport.push_tiles(200, &serde_json::to_vec(&wire).unwrap());
    }
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    for _ in 0..4 {
        assert_eq!(
            client
                .tile_list("project_a", false, NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }
    transport.push_tiles(
        200,
        br#"{"status":"success","eds_project_id":"project_a","count":0,"tiles":null}"#,
    );
    assert!(client
        .tile_list("project_a", false, NOW + 1)
        .unwrap()
        .tiles
        .is_empty());
}

fn headless_layer_fixture(project: &str) -> serde_json::Value {
    serde_json::json!({"success":true,"data":{"project_id":project,"sources":{"roads":{"type":"vector"}},"layers":[{"id":"render-road","source":"roads","type":"line","metadata":{"config_layer_id":"roads"}}],"styles":{},"style_editors":[]}})
}
fn headless_style_fixture(project: &str) -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "data": {
            "project_id": project,
            "resolved_at": 1_900_000_000_000_i64,
            "styles": {},
            "style_editors": [],
            "style_properties": {},
            "style_presets": {},
            "label_editor": {},
            "fill_pattern": {},
            "platform_config": {},
            "symbol_catalog": {},
            "design_layer_catalog": {},
            "required_images": [],
            "warnings": [],
        }
    })
}
fn push_layers(transport: &FixtureTransport, wire: serde_json::Value) {
    transport
        .0
        .lock()
        .unwrap()
        .layers
        .push_back(TransportResponse::new(
            200,
            serde_json::to_vec(&wire).unwrap(),
        ));
}

#[test]
fn headless_layers_read_and_reorder_use_canonical_ids_and_same_selected_project() {
    use ds_client_core::{LayerOrder, LayerVisibilityDefault};
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    push_layers(&transport, headless_layer_fixture("project_a"));
    push_layers(&transport, headless_layer_fixture("project_a"));
    push_layers(
        &transport,
        serde_json::json!({"success":true,"data":{"project_id":"project_a","reordered":1}}),
    );
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let snapshot = client.layer_config("project_a", false, NOW + 1).unwrap();
    assert!(snapshot.canonical_ids().contains("roads"));
    let receipt = client
        .layer_reorder(
            "project_a",
            &[LayerOrder {
                layer_id: "roads".into(),
                order: 42,
            }],
            NOW + 1,
        )
        .unwrap();
    assert_eq!(receipt.reordered, 1);
    assert!(transport.calls().iter().any(|v| {
        v.contains("\"layer_order\":[{\"layer_id\":\"roads\",\"order\":42}]")
            && v.contains("path=/api/v1/layers")
    }));
    push_layers(&transport, headless_layer_fixture("project_a"));
    push_layers(
        &transport,
        serde_json::json!({"success":true,"data":{"project_id":"project_a","updated":1}}),
    );
    let receipt = client
        .layer_default_visibility(
            "project_a",
            &[LayerVisibilityDefault {
                layer_id: "roads".into(),
                visible: false,
            }],
            NOW + 1,
        )
        .unwrap();
    assert_eq!(receipt.updated, 1);
    assert!(transport.calls().iter().any(|v| {
        v.contains("\"layer_defaults\":[{\"layer_id\":\"roads\",\"visible\":false}]")
            && v.contains("path=/api/v1/layers")
    }));
    push_layers(&transport, headless_layer_fixture("project_a"));
    assert_eq!(
        client
            .layer_reorder(
                "project_a",
                &[LayerOrder {
                    layer_id: "render-road".into(),
                    order: 42
                }],
                NOW + 1
            )
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        transport
            .calls()
            .iter()
            .filter(|v| v.contains("\"action\":\"reorder\""))
            .count(),
        1
    );
    push_layers(&transport, headless_layer_fixture("other"));
    assert!(client.layer_config("project_a", true, NOW + 1).is_err());
}

#[test]
fn headless_style_catalog_is_metadata_only_bounded_and_project_fenced() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut fixture = headless_style_fixture("project_a");
    fixture["data"]["styles"] =
        serde_json::json!({"master/roads":{"type":"line","paint":{"line-width":3}}});
    fixture["data"]["warnings"] = serde_json::json!(["one declaration was omitted"]);
    push_layers(&transport, fixture);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let snapshot = client.style_catalog("project_a", NOW + 1).unwrap();
    assert_eq!(snapshot.project_id(), "project_a");
    assert_eq!(snapshot.warnings(), ["one declaration was omitted"]);
    for forbidden in ["sources", "layers", "sidebar", "viewport", "basemaps"] {
        assert!(snapshot.document().get(forbidden).is_none());
    }
    let calls = transport.calls();
    let layer_calls = calls
        .iter()
        .filter(|call| call.starts_with("layers "))
        .collect::<Vec<_>>();
    assert_eq!(layer_calls.len(), 1);
    assert!(layer_calls[0]
        .contains("body={\"action\":\"get_style_catalog\",\"project_id\":\"project_a\"}"));
    assert!(layer_calls[0].contains("limit=16777216"));
    assert!(!layer_calls[0].contains("get_config"));

    push_layers(&transport, headless_style_fixture("other"));
    assert_eq!(
        client
            .style_catalog("project_a", NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );

    let mut malformed = headless_style_fixture("project_a");
    malformed["data"]["sources"] = serde_json::json!({});
    push_layers(&transport, malformed);
    assert_eq!(
        client
            .style_catalog("project_a", NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );

    let mut excessive = headless_style_fixture("project_a");
    excessive["data"]["styles"] = serde_json::Value::Object(
        (0..=1_000)
            .map(|index| (format!("style/{index}"), serde_json::json!({})))
            .collect(),
    );
    push_layers(&transport, excessive);
    assert_eq!(
        client
            .style_catalog("project_a", NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );

    transport
        .0
        .lock()
        .unwrap()
        .layers
        .push_back(TransportResponse::new(
            200,
            vec![b' '; ds_client_core::STYLE_CATALOG_RESPONSE_LIMIT + 1],
        ));
    assert_eq!(
        client
            .style_catalog("project_a", NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
}

#[test]
fn headless_style_catalog_accepts_the_exact_brain_snapshot_contract() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let data: serde_json::Value = serde_json::from_slice(STYLE_CATALOG_SNAPSHOT).unwrap();
    push_layers(
        &transport,
        serde_json::json!({"success": true, "data": data}),
    );
    let mut client = signed_in(transport, MemoryStore::default());
    let snapshot = client.style_catalog("arjgpydw_fixture", NOW + 1).unwrap();
    assert_eq!(snapshot.document()["styles"].as_object().unwrap().len(), 6);
    assert_eq!(
        snapshot.document()["style_editors"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(snapshot.warnings().len(), 1);
}

#[test]
fn headless_style_plan_publishes_only_on_apply_and_checks_the_returned_target() {
    use ds_client_core::StyleInstruction;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut fixture = headless_style_fixture("project_a");
    fixture["data"]["styles"] =
        serde_json::json!({"master/roads":{"type":"line","paint":{"line-width":3},"minzoom":7}});
    fixture["data"]["style_editors"] = serde_json::json!([{"style_ref":"master/roads","style_target":"design","layer_name":"roads"}]);
    fixture["data"]["warnings"] = serde_json::json!(["catalog omission"]);
    let instruction = StyleInstruction::Appearance {
        icon_overlap: None,
        color: Some("#ff0000".into()),
        icon: None,
        size: None,
    };
    push_layers(&transport, fixture.clone());
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let plan = client
        .style_edit("project_a", "master/roads", &instruction, false, NOW + 1)
        .unwrap();
    assert_eq!(plan.data()["document"]["paint"]["line-color"], "#FF0000");
    assert_eq!(plan.data()["document"]["minzoom"], 7);
    assert_eq!(
        plan.data()["warnings"],
        serde_json::json!(["catalog omission"])
    );
    assert!(!transport.calls().iter().any(|c| c.starts_with("styles ")));
    assert!(!transport.calls().iter().any(|c| c.contains("get_config")));
    push_layers(&transport, fixture.clone());
    transport.0.lock().unwrap().styles.push_back(TransportResponse::new(200,serde_json::to_vec(&serde_json::json!({"success":true,"data":{"updated":true,"style_target":"design","layer_name":"roads","style_data":plan.data()["document"],"warnings":["publish warning"]}})).unwrap()));
    let published = client
        .style_edit("project_a", "master/roads", &instruction, true, NOW + 1)
        .unwrap();
    assert_eq!(published.data()["published"], true);
    assert_eq!(
        published.data()["warnings"],
        serde_json::json!(["catalog omission", "publish warning"])
    );
    assert!(transport.calls().iter().any(|c| c.starts_with("styles ")
        && c.contains("path=/api/v1/styles")
        && c.contains("\"action\":\"update_style\"")));
    push_layers(&transport, fixture);
    transport
        .0
        .lock()
        .unwrap()
        .styles
        .push_back(TransportResponse::new(
        200,
        br#"{"success":true,"data":{"updated":true,"style_target":"user_data","style_data":{}}}"#
            .as_slice(),
    ));
    assert!(client
        .style_edit("project_a", "master/roads", &instruction, true, NOW + 1)
        .is_err());
}

fn push_styles(transport: &FixtureTransport, wire: serde_json::Value) {
    transport
        .0
        .lock()
        .unwrap()
        .styles
        .push_back(TransportResponse::new(
            200,
            serde_json::to_vec(&wire).unwrap(),
        ));
}

/// ds-brain answers expression warnings as `{path, message}` issue objects.
/// The browser has always shown them; the native client refused the whole
/// receipt over them. One kernel now admits and normalises them for both, and
/// still refuses a receipt that is not the style that was authored.
#[test]
fn headless_style_publish_normalises_issue_warnings_and_refuses_a_foreign_receipt() {
    use ds_client_core::StyleInstruction;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut fixture = headless_style_fixture("project_a");
    fixture["data"]["styles"] =
        serde_json::json!({"master/roads":{"type":"line","paint":{"line-width":3}}});
    fixture["data"]["style_editors"] = serde_json::json!([{"style_ref":"master/roads","style_target":"design","layer_name":"roads"}]);
    let instruction = StyleInstruction::Appearance {
        icon_overlap: None,
        color: Some("#00ff00".into()),
        icon: None,
        size: None,
    };
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    push_layers(&transport, fixture.clone());
    push_styles(
        &transport,
        serde_json::json!({"success":true,"data":{"updated":true,"style_target":"design","layer_name":"roads","style_data":{"type":"line","paint":{"line-color":"#00FF00"}},"warnings":[
            {"path":"paint.line-width","message":"clamped to the declared maximum"},
            "a plain warning",
            {"property":"label","issue":{"message":"no text-field"}}
        ]}}),
    );
    let published = client
        .style_edit("project_a", "master/roads", &instruction, true, NOW + 1)
        .unwrap();
    assert_eq!(published.data()["published"], true);
    assert_eq!(
        published.data()["warnings"],
        serde_json::json!([
            "paint.line-width: clamped to the declared maximum",
            "a plain warning",
            "label: no text-field"
        ])
    );
    assert_eq!(
        published.data()["document"]["paint"]["line-color"],
        "#00FF00"
    );

    // A warning that is neither text nor a named issue is a contract slip.
    push_layers(&transport, fixture.clone());
    push_styles(
        &transport,
        serde_json::json!({"success":true,"data":{"updated":true,"style_target":"design","layer_name":"roads","style_data":{"type":"line"},"warnings":[7]}}),
    );
    let malformed = client
        .style_edit("project_a", "master/roads", &instruction, true, NOW + 1)
        .err()
        .expect("a malformed warning is refused");
    assert_eq!(malformed.kind(), ErrorKind::UnreadableResponse);
    assert_eq!(
        malformed
            .service_refusal()
            .and_then(|refusal| refusal.message()),
        Some("style publish warnings are malformed")
    );

    // The receipt must name the identity that was authored, not another layer.
    push_layers(&transport, fixture);
    push_styles(
        &transport,
        serde_json::json!({"success":true,"data":{"updated":true,"style_target":"design","layer_name":"another_layer","style_data":{"type":"line"}}}),
    );
    let foreign = client
        .style_edit("project_a", "master/roads", &instruction, true, NOW + 1)
        .err()
        .expect("a foreign identity is refused");
    assert_eq!(foreign.kind(), ErrorKind::UnreadableResponse);
    assert_eq!(
        foreign
            .service_refusal()
            .and_then(|refusal| refusal.message()),
        Some("style publish receipt does not match the authored target")
    );
}

fn push_refused_style(transport: &FixtureTransport, status: u16, wire: serde_json::Value) {
    transport
        .0
        .lock()
        .unwrap()
        .styles
        .push_back(TransportResponse::new(
            status,
            serde_json::to_vec(&wire).unwrap(),
        ));
}

fn style_refusal_of(error: &ds_client_core::ClientError) -> (u16, Option<String>, Option<String>) {
    let refusal = error
        .service_refusal()
        .expect("a style refusal carries what refused it");
    (
        refusal.status(),
        refusal.code().map(str::to_owned),
        refusal.message().map(str::to_owned),
    )
}

/// `ds style print create --ref gt/rivers` on 2026-09-27: the catalog
/// already listed `gt/rivers_print`, ds-brain refused the create-only write
/// with `409 CONFLICT`, and the shared tile classifier turned that into "the
/// managed tile service refused the bounded operation" — which `ds` rendered
/// with a remedy about the project id. Every style refusal now names what
/// refused it: an existing variant before any request, ds-brain's code,
/// sentence and issues after one, and the planner's own rule.
#[test]
fn headless_style_refusals_name_the_rule_that_refused_them() {
    use ds_client_core::StyleInstruction;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut fixture = headless_style_fixture("project_a");
    fixture["data"]["styles"] = serde_json::json!({
        "gt/rivers": {"type":"line","paint":{"line-color":"#4B4B8F","line-width":3.6}},
        "gt/roads": {"type":"line","paint":{"line-width":2}}
    });
    fixture["data"]["style_editors"] = serde_json::json!([
        {"style_ref":"gt/rivers","style_target":"global_tile","layer_name":"rivers","available_fields":["name"]},
        {"style_ref":"gt/roads","style_target":"global_tile","layer_name":"roads","available_fields":["name"]}
    ]);
    let mut held = fixture.clone();
    held["data"]["styles"]["gt/rivers_print"] =
        serde_json::json!({"type":"line","paint":{"line-color":"#5B8FA3","line-width":1}});
    held["data"]["style_editors"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"style_ref":"gt/rivers_print","style_target":"print","layer_name":"gt/rivers_print"}));
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let posted = |transport: &FixtureTransport| {
        transport
            .calls()
            .iter()
            .filter(|call| call.starts_with("styles "))
            .count()
    };

    // 1. The catalog holds the variant: refused by name, nothing is sent,
    //    for a plan and a create alike.
    for apply in [false, true] {
        push_layers(&transport, held.clone());
        let exists = client
            .style_edit(
                "project_a",
                "gt/rivers",
                &StyleInstruction::PrintVariant,
                apply,
                NOW + 1,
            )
            .err()
            .expect("an existing print variant is never created again");
        assert_eq!(exists.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            style_refusal_of(&exists),
            (
                0,
                Some("style_exists".into()),
                Some("gt/rivers_print already exists; a print variant is created once and never overwritten, so customise gt/rivers_print itself".into())
            )
        );
    }
    assert_eq!(posted(&transport), 0, "a refused create sends no write");

    // 2. Negative control: the variant is absent, so the create is planned
    //    create-only and published.
    push_layers(&transport, fixture.clone());
    let plan = client
        .style_edit(
            "project_a",
            "gt/roads",
            &StyleInstruction::PrintVariant,
            false,
            NOW + 1,
        )
        .unwrap();
    assert_eq!(plan.data()["ref"], "gt/roads_print");
    push_layers(&transport, fixture.clone());
    push_styles(
        &transport,
        serde_json::json!({"success":true,"data":{"updated":true,"style_target":"print","layer_name":"gt/roads_print","style_data":plan.data()["document"]}}),
    );
    let created = client
        .style_edit(
            "project_a",
            "gt/roads",
            &StyleInstruction::PrintVariant,
            true,
            NOW + 1,
        )
        .unwrap();
    assert_eq!(created.data()["published"], true);
    assert!(transport
        .calls()
        .iter()
        .any(|call| call.starts_with("styles ")
            && call.contains("\"style_ref\":\"gt/roads_print\"")
            && call.contains("\"create_only\":true")));

    // 3. ds-brain's own refusals, when the catalog could not have known.
    let cases = [
        (
            409,
            serde_json::json!({"success":false,"error":{"code":"CONFLICT","message":"Print style already exists; edit its _print style instead","timestamp":1}}),
            ErrorKind::InvalidInput,
            "conflict",
            "Print style already exists; edit its _print style instead".to_owned(),
        ),
        (
            400,
            serde_json::json!({"success":false,"error":{"code":"VALIDATION_FAILED","message":"Invalid style document","details":{"issues":[
                {"path":"paint.line-glow","message":"unknown paint property"},
                {"property":"label.text-field","severity":"error","issue":{"path":"","message":"field \"nme\" is not in available_fields"}},
                {"path":"layout.foo","message":"unknown layout property"},
                {"path":"layout.bar","message":"unknown layout property"}
            ]},"timestamp":1}}),
            ErrorKind::InvalidInput,
            "validation_failed",
            "Invalid style document: paint.line-glow: unknown paint property; label.text-field: field \"nme\" is not in available_fields; layout.foo: unknown layout property; +1 more".to_owned(),
        ),
        (
            403,
            serde_json::json!({"success":false,"error":{"code":"INSUFFICIENT_PERMISSIONS","message":"Permission denied","timestamp":1}}),
            ErrorKind::AuthenticationRejected,
            "insufficient_permissions",
            "Permission denied".to_owned(),
        ),
    ];
    for (status, wire, kind, code, sentence) in cases {
        push_layers(&transport, fixture.clone());
        push_refused_style(&transport, status, wire);
        let refused = client
            .style_edit(
                "project_a",
                "gt/roads",
                &StyleInstruction::PrintVariant,
                true,
                NOW + 1,
            )
            .err()
            .expect("a refused write is an error");
        assert_eq!(refused.kind(), kind, "HTTP {status}");
        assert_eq!(
            style_refusal_of(&refused),
            (status, Some(code.into()), Some(sentence)),
            "HTTP {status}"
        );
        assert!(
            !refused
                .to_string()
                .contains("managed tile service refused the bounded operation"),
            "HTTP {status} must not read as a tile refusal"
        );
    }

    // A body that is not the envelope still classifies, with nothing to say.
    push_layers(&transport, fixture.clone());
    push_refused_style(&transport, 409, serde_json::json!("gateway page"));
    let bare = client
        .style_edit(
            "project_a",
            "gt/roads",
            &StyleInstruction::PrintVariant,
            true,
            NOW + 1,
        )
        .err()
        .unwrap();
    assert_eq!(bare.kind(), ErrorKind::InvalidInput);
    assert!(bare.service_refusal().is_none());

    // 4. The planner's own rule crosses in its words, before any request.
    push_layers(&transport, fixture);
    let before = posted(&transport);
    let planned = client
        .style_edit(
            "project_a",
            "gt/roads",
            &StyleInstruction::Appearance {
                icon_overlap: None,
                color: None,
                icon: None,
                size: None,
            },
            true,
            NOW + 1,
        )
        .err()
        .unwrap();
    assert_eq!(planned.kind(), ErrorKind::InvalidInput);
    assert_eq!(
        style_refusal_of(&planned),
        (
            0,
            Some("style_refused".into()),
            Some("choose colour, icon, size or icon overlap".into())
        )
    );
    assert_eq!(posted(&transport), before);
}

fn push_project_data(transport: &FixtureTransport, data: serde_json::Value) {
    transport
        .0
        .lock()
        .unwrap()
        .project_data
        .push_back(TransportResponse::new(
            200,
            serde_json::to_vec(&serde_json::json!({"success":true,"data":data})).unwrap(),
        ));
}
fn upload_start_fixture() -> serde_json::Value {
    serde_json::json!({"session_uri":"https://storage.googleapis.com/upload/storage/v1/b/test-bucket/o?upload_id=private-token","bucket":"test-bucket","blob_path":"projects/project_a/project_data/roads/raw/receipt/roads.geojson","upload_id":"roads"})
}
#[test]
fn project_gis_upload_uses_issued_session_and_registers_only_after_complete_transfer() {
    use ds_client_core::ProjectDataCommand;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    push_project_data(&transport, upload_start_fixture());
    transport
        .0
        .lock()
        .unwrap()
        .uploads
        .push_back(TransportResponse::new(200, b"{}".as_slice()));
    push_project_data(
        &transport,
        serde_json::json!({"upload_id":"roads","tile_status":"success","pmtiles_path":"private/path"}),
    );
    let mut bytes = std::io::Cursor::new(b"abc");
    let done = client
        .project_data(
            "project_a",
            ProjectDataCommand::Upload {
                file_name: "roads.geojson",
                size: 3,
                reader: &mut bytes,
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(done.data()["ready"], true);
    assert!(!done.data().to_string().contains("private"));
    let calls = transport.calls();
    let upload = calls
        .iter()
        .position(|c| c.starts_with("upload bytes"))
        .unwrap();
    assert!(calls[upload - 1].contains("upload_start"));
    assert!(calls[upload + 1].contains("\"action\":\"upload\""));
    // Storage failure must not register the file or attempt a second upload.
    push_project_data(&transport, upload_start_fixture());
    transport
        .0
        .lock()
        .unwrap()
        .uploads
        .push_back(TransportResponse::new(308, b"{}".as_slice()));
    let mut bytes = std::io::Cursor::new(b"abc");
    assert!(client
        .project_data(
            "project_a",
            ProjectDataCommand::Upload {
                file_name: "roads.geojson",
                size: 3,
                reader: &mut bytes
            },
            NOW + 1
        )
        .is_err());
    assert_eq!(
        transport
            .calls()
            .iter()
            .filter(|c| c.contains("\"action\":\"upload\""))
            .count(),
        1
    );
}

fn push_status_processing(transport: &FixtureTransport, data: serde_json::Value) {
    transport
        .0
        .lock()
        .unwrap()
        .status_processing
        .push_back(TransportResponse::new(
            200,
            serde_json::to_vec(&serde_json::json!({"success":true,"data":data})).unwrap(),
        ));
}

#[test]
fn status_intake_upload_is_project_fenced_then_processes_one_file() {
    use ds_client_core::{StatusProcessingCommand, StatusUploadDomain};
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    push_status_processing(
        &transport,
        serde_json::json!({
            "session_uri":"https://storage.googleapis.com/upload/storage/v1/b/test-bucket/o?upload_id=private-token",
            "blob_path":"project_a/lv_process/uploads/receipt/a.zip",
            "upload_domain":"lv_process",
            "upload_receipt":"receipt"
        }),
    );
    transport
        .0
        .lock()
        .unwrap()
        .uploads
        .push_back(TransportResponse::new(201, b"{}".as_slice()));
    let mut bytes = std::io::Cursor::new(b"abc");
    let uploaded = client
        .status_processing(
            "project_a",
            StatusProcessingCommand::Upload {
                domain: StatusUploadDomain::LvProcess,
                file_name: "a.zip",
                size: 3,
                content_type: "application/zip",
                reader: &mut bytes,
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(
        uploaded.data()["blob_path"],
        "project_a/lv_process/uploads/receipt/a.zip"
    );
    push_status_processing(
        &transport,
        serde_json::json!({"results":[{"file_name":"a.zip","submitted":1}]}),
    );
    let path = uploaded.data()["blob_path"].as_str().unwrap();
    let processed = client
        .status_processing(
            "project_a",
            StatusProcessingCommand::Process {
                action: StatusUploadDomain::LvProcess,
                file_name: "a.zip",
                blob_path: path,
                chain: &[],
                extra: serde_json::json!({"settings":{}})
                    .as_object()
                    .expect("settings object"),
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(processed.data()["results"][0]["submitted"], 1);
    let calls = transport.calls();
    assert!(calls.iter().any(|call| call.contains("path=/process")
        && call.contains("upload_start")
        && call.contains("correlation=true")));
    assert!(calls
        .iter()
        .any(|call| call.contains("\"action\":\"lv_process\"")
            && call.contains("\"eds_project_id\":\"project_a\"")));
}

#[test]
fn status_intake_refuses_foreign_storage_target_before_byte_transfer() {
    use ds_client_core::{StatusProcessingCommand, StatusUploadDomain};
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    push_status_processing(
        &transport,
        serde_json::json!({
            "session_uri":"https://storage.googleapis.com/upload/storage/v1/b/test-bucket/o?upload_id=private-token",
            "blob_path":"other/lv_process/uploads/receipt/a.zip",
            "upload_domain":"lv_process"
        }),
    );
    let mut bytes = std::io::Cursor::new(b"abc");
    let error = client
        .status_processing(
            "project_a",
            StatusProcessingCommand::Upload {
                domain: StatusUploadDomain::LvProcess,
                file_name: "a.zip",
                size: 3,
                content_type: "application/zip",
                reader: &mut bytes,
            },
            NOW + 1,
        )
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
    assert!(!transport
        .calls()
        .iter()
        .any(|call| call.starts_with("upload bytes=")));
}
#[test]
fn project_gis_refuses_untrusted_sessions_and_projects_and_preserves_tiling_failure() {
    use ds_client_core::ProjectDataCommand;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    for uri in [
        "https://evil.example/upload",
        "http://storage.googleapis.com/upload/storage/v1/b/test-bucket/o?upload_id=x",
        "https://storage.googleapis.com.evil.test/upload/storage/v1/b/test-bucket/o?upload_id=x",
        "https://user:secret@storage.googleapis.com/upload/storage/v1/b/test-bucket/o?upload_id=x",
    ] {
        let mut start = upload_start_fixture();
        start["session_uri"] = serde_json::json!(uri);
        push_project_data(&transport, start);
        let mut bytes = std::io::Cursor::new(b"abc");
        assert!(client
            .project_data(
                "project_a",
                ProjectDataCommand::Upload {
                    file_name: "roads.geojson",
                    size: 3,
                    reader: &mut bytes
                },
                NOW + 1
            )
            .is_err());
    }
    let mut start = upload_start_fixture();
    start["blob_path"] = serde_json::json!("projects/other/project_data/roads/raw/r/file");
    push_project_data(&transport, start);
    let mut bytes = std::io::Cursor::new(b"abc");
    assert!(client
        .project_data(
            "project_a",
            ProjectDataCommand::Upload {
                file_name: "roads.geojson",
                size: 3,
                reader: &mut bytes
            },
            NOW + 1
        )
        .is_err());
    assert!(!transport
        .calls()
        .iter()
        .any(|c| c.starts_with("upload bytes")));
    push_project_data(&transport, upload_start_fixture());
    transport
        .0
        .lock()
        .unwrap()
        .uploads
        .push_back(TransportResponse::new(201, b"{}".as_slice()));
    push_project_data(
        &transport,
        serde_json::json!({"upload_id":"roads","tile_status":"error","tile_error":"private diagnostic"}),
    );
    let mut bytes = std::io::Cursor::new(b"abc");
    let receipt = client
        .project_data(
            "project_a",
            ProjectDataCommand::Upload {
                file_name: "roads.geojson",
                size: 3,
                reader: &mut bytes,
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(receipt.data()["registered"], true);
    assert_eq!(receipt.data()["ready"], false);
    assert_eq!(receipt.data()["tile_status"], "error");
    assert!(!receipt.data().to_string().contains("private"));
    push_project_data(
        &transport,
        serde_json::json!({"uploads":[{"upload_id":"roads","file_name":"roads.geojson","tile_status":"ready","pmtiles_url":"private-url"}]}),
    );
    let listed = client
        .project_data("project_a", ProjectDataCommand::List, NOW + 1)
        .unwrap();
    assert_eq!(listed.data()["total"], 1);
    assert!(!listed.data().to_string().contains("private"));
    push_project_data(&transport, serde_json::json!({"upload_id":"other"}));
    assert!(client
        .project_data(
            "project_a",
            ProjectDataCommand::Remove { upload_id: "roads" },
            NOW + 1
        )
        .is_err());
}

fn push_survey_control(transport: &FixtureTransport, value: serde_json::Value) {
    transport
        .0
        .lock()
        .unwrap()
        .survey_control
        .push_back(TransportResponse::new(
            200,
            serde_json::to_vec(&value).unwrap(),
        ));
}
#[test]
fn survey_form_purge_is_exact_digest_pinned_and_preserves_partial_and_stale_receipts() {
    use ds_client_core::survey_control::*;
    use ds_command_kernel::survey_control::FORM_PURGE_COUNTS;
    use serde_json::{json, Value};
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let slug = "field_notes_-_areas";
    let digest = format!("sha256:{}", "a".repeat(64));
    let counts = |n: u64| -> Value {
        FORM_PURGE_COUNTS
            .iter()
            .map(|key| (key.to_string(), json!(n)))
            .collect()
    };
    push_survey_control(
        &transport,
        json!({"status":"success","plan":{"slug":slug,"plan_digest":digest,"scope":counts(1),"projects":[],"templates":[]}}),
    );
    let plan = client
        .survey_control(
            None,
            &SurveyControlCommand::FormPurgePlan(FormPurgePlan { slug: slug.into() }),
            NOW + 1,
        )
        .unwrap();
    assert_eq!(plan["plan"]["plan_digest"], digest);
    let plan_call = transport.calls().last().unwrap().clone();
    assert!(plan_call.contains("method=POST") && plan_call.contains("path=/api/v1/form-factory"));
    assert!(plan_call.contains("purge_plan") && !plan_call.contains("confirm_delete"));
    let request = SurveyControlCommand::FormPurge(FormPurge {
        slug: slug.into(),
        plan_digest: digest.clone(),
    });
    push_survey_control(
        &transport,
        json!({"status":"success","receipt":{"slug":slug,"plan_digest":digest,"state":"partial","scope":counts(1),"remaining":counts(1),"projects":[],"errors":["media unavailable"]}}),
    );
    let partial = client.survey_control(None, &request, NOW + 1).unwrap();
    assert_eq!(partial["receipt"]["state"], "partial");
    assert_eq!(partial["receipt"]["remaining"]["media_versions"], 1);
    let apply_call = transport.calls().last().unwrap().clone();
    assert!(apply_call.contains("confirm_delete") && apply_call.contains(&digest));
    for (status, code, kind) in [
        (400, "form_purge_invalid", ErrorKind::InvalidInput),
        (409, "form_purge_scope_changed", ErrorKind::InvalidInput),
        (503, "form_purge_unavailable", ErrorKind::RouteUnavailable),
    ] {
        transport
            .0
            .lock()
            .unwrap()
            .survey_control
            .push_back(TransportResponse::new(
            status,
            serde_json::to_vec(
                &json!({"status":"error","code":code,"error":"purge refused","retryable":false}),
            )
            .unwrap(),
        ));
        let refusal = client.survey_control(None, &request, NOW + 1).unwrap_err();
        assert_eq!(refusal.kind(), kind);
        assert_eq!(refusal.service_refusal().unwrap().code(), Some(code));
    }
    let before = transport.calls().len();
    let invalid = SurveyControlCommand::FormPurge(FormPurge {
        slug: slug.into(),
        plan_digest: "not-pinned".into(),
    });
    assert_eq!(
        client
            .survey_control(None, &invalid, NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(transport.calls().len(), before);
    push_survey_control(
        &transport,
        json!({"status":"success","receipt":{"slug":slug,"plan_digest":digest,"state":"complete","scope":counts(1),"remaining":counts(1),"projects":[],"errors":[]}}),
    );
    assert_eq!(
        client
            .survey_control(None, &request, NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
}
#[test]
fn survey_control_forms_are_native_bounded_and_identity_checked() {
    use ds_client_core::survey_control::*;
    use serde_json::json;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    push_survey_control(
        &transport,
        json!({"status":"success","forms":[{"slug":"b","fields":[]},{"slug":"a","fields":[]}]}),
    );
    let result = client
        .survey_control(
            None,
            &SurveyControlCommand::FormsList(CatalogQuery {
                status: Some("active".into()),
                query: None,
                limit: 1,
                detail: false,
            }),
            NOW + 1,
        )
        .unwrap();
    assert_eq!(result["forms"][0]["slug"], "a");
    assert_eq!(result["more"]["omitted"], 1);
    push_survey_control(
        &transport,
        json!({"status":"success","form":{"slug":"other","version":1,"fields":[]}}),
    );
    assert_eq!(
        client
            .survey_control(
                None,
                &SurveyControlCommand::FormRead(FormRead {
                    slug: "a".into(),
                    field_offset: 0,
                    field_limit: 50
                }),
                NOW + 1
            )
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
    let calls = transport.calls();
    assert!(calls
        .iter()
        .any(|s| s.contains("path=/api/v1/form-factory")));
    let before = calls.len();
    // A project that is not the selected one is refused BY NAME, and never as
    // a malformed request: a caller who named a real project they can see has
    // to be told that selection, not their spelling, is what stands in the
    // way, or the remedy they are given cannot work.
    let mismatch = client
        .survey_control(
            Some("selected"),
            &SurveyControlCommand::ProjectFormEditor(ProjectEditor {
                project: "different".into(),
                form: "a".into(),
            }),
            NOW + 1,
        )
        .unwrap_err();
    assert_eq!(mismatch.kind(), ErrorKind::InvalidInput);
    let sentence = mismatch.to_string();
    assert!(
        sentence.contains("selected project") && sentence.contains("ds auth project use"),
        "the mismatch names the selection and its remedy, got {sentence:?}"
    );
    let malformed = client
        .survey_control(
            Some("selected"),
            &SurveyControlCommand::ProjectFormEditor(ProjectEditor {
                project: String::new(),
                form: "a".into(),
            }),
            NOW + 1,
        )
        .unwrap_err();
    assert_eq!(malformed.kind(), ErrorKind::InvalidInput);
    assert_ne!(
        malformed.to_string(),
        sentence,
        "a malformed request and an unselected project are not the same refusal"
    );
    assert_eq!(transport.calls().len(), before);
    assert!(serde_json::from_value::<SurveyControlCommand>(json!({"operation":"survey.form.read","arguments":{"slug":"a","fieldOffset":0,"fieldLimit":1,"url":"https://example.invalid"}})).is_err());
}
#[test]
fn survey_settings_plan_does_not_publish_and_apply_rechecks_versions() {
    use ds_client_core::survey_control::*;
    use ds_command_kernel::survey::project_forms::ProjectFormChange;
    use serde_json::json;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let snapshot = json!({"status":"success","project_id":"p","forms":[{"slug":"pole"}]});
    let editor = json!({"status":"success","project_id":"p","form_slug":"pole","editor":{"form":{"slug":"pole"},"version":2,"sections":[{"fields":[{"key":"enabled"}]}]}});
    let request = ProjectChanges {
        project: "p".into(),
        changes: vec![ProjectFormChange {
            form_slug: "pole".into(),
            enabled: None,
            settings: Some(serde_json::from_value(json!({"enabled":true})).unwrap()),
            expected_version: Some(2),
        }],
    };
    push_survey_control(&transport, snapshot.clone());
    push_survey_control(&transport, editor.clone());
    let plan = client
        .survey_control(
            Some("p"),
            &SurveyControlCommand::ProjectFormsPlan(request.clone()),
            NOW + 1,
        )
        .unwrap();
    assert_eq!(plan["ready"], true);
    assert!(transport.calls().iter().all(|s| !s.contains("bulk_save")));
    push_survey_control(&transport, snapshot);
    let mut stale = editor;
    stale["editor"]["version"] = json!(3);
    push_survey_control(&transport, stale);
    assert!(client
        .survey_control(
            Some("p"),
            &SurveyControlCommand::ProjectFormsApply(request),
            NOW + 1
        )
        .is_err());
    assert!(transport.calls().iter().all(|s| !s.contains("bulk_save")));
}

#[test]
fn survey_control_retains_all_form_lifecycle_and_template_wire_contracts() {
    use ds_client_core::survey_control::*;
    use serde_json::json;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let form = json!({"slug":"pole","version":2,"fields":[]});
    push_survey_control(
        &transport,
        json!({"status":"success","field_types":[],"condition_operators":[],"type_aliases":{}}),
    );
    assert!(client
        .survey_control(None, &SurveyControlCommand::FormTypes(Empty {}), NOW + 1)
        .unwrap()["fieldTypes"]
        .is_array());
    for (operation, arguments, expected) in [
        (
            "survey.form.create",
            json!({"schema":{"slug":"pole","fields":[]}}),
            "schema_data",
        ),
        (
            "survey.form.update",
            json!({"slug":"pole","expectedVersion":2,"schema":{"slug":"pole","version":2,"fields":[]}}),
            "version",
        ),
        (
            "survey.form.lifecycle",
            json!({"slug":"pole","action":"duplicate","newDisplayName":"Copy"}),
            "confirm_duplicate",
        ),
        (
            "survey.form.lifecycle",
            json!({"slug":"pole","action":"archive","force":true}),
            "confirm_archive",
        ),
        (
            "survey.form.lifecycle",
            json!({"slug":"pole","action":"restore"}),
            "restore",
        ),
        (
            "survey.form.lifecycle",
            json!({"slug":"pole","action":"delete","force":false}),
            "confirm_delete",
        ),
    ] {
        push_survey_control(&transport, json!({"status":"success","form":form}));
        let command =
            serde_json::from_value(json!({"operation":operation,"arguments":arguments})).unwrap();
        client.survey_control(None, &command, NOW + 1).unwrap();
        assert!(transport.calls().last().unwrap().contains(expected));
    }
    for action in ["publish", "unpublish"] {
        push_survey_control(&transport, json!({"status":"success","form":form}));
        push_survey_control(&transport, json!({"status":"success","form":form}));
        let command=serde_json::from_value(json!({"operation":"survey.form.lifecycle","arguments":{"action":action,"slug":"pole","expectedVersion":2}})).unwrap();
        client.survey_control(None, &command, NOW + 1).unwrap();
        assert!(transport.calls().last().unwrap().contains("\"version\":2"));
    }
    for (operation, arguments, project, path) in [
        (
            "survey.template.create",
            json!({"project":"p","name":"Water","visibility":"private"}),
            Some("p"),
            "/api/v1/projects/templates",
        ),
        (
            "survey.template.apply",
            json!({"project":"p","template":"water","mergeStrategy":"preserve"}),
            Some("p"),
            "/api/v1/projects ",
        ),
        (
            "survey.project.create_from_template",
            json!({"template":"water","projectName":"New project"}),
            None,
            "/api/v1/projects/from-template",
        ),
        (
            "survey.template.lifecycle",
            json!({"template":"water","action":"publish"}),
            None,
            "/api/v1/projects/templates",
        ),
        (
            "survey.template.lifecycle",
            json!({"template":"water","action":"privatize"}),
            None,
            "/api/v1/projects/templates",
        ),
        (
            "survey.template.lifecycle",
            json!({"template":"water","action":"delete"}),
            None,
            "/api/v1/projects/templates",
        ),
    ] {
        push_survey_control(&transport, json!({"success":true,"data":{"ok":true}}));
        let command =
            serde_json::from_value(json!({"operation":operation,"arguments":arguments})).unwrap();
        client.survey_control(project, &command, NOW + 1).unwrap();
        assert!(transport.calls().last().unwrap().contains(path));
    }
}

fn project_response(value: serde_json::Value) -> TransportResponse {
    TransportResponse::new(
        200,
        serde_json::to_vec(&serde_json::json!({"success":true,"data":value})).unwrap(),
    )
}
#[test]
fn solar_project_city_commit_fences_response_and_preserves_exact_snapshot() {
    use ds_client_core::solar_project::Command;
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let command = Command::CommitCity {
        city: "bere".into(),
        expected_base: "".into(),
        snapshot_json: "{}".into(),
        fingerprint: format!("{:x}", sha2::Sha256::digest(b"{}")),
    };
    for (city, ok) in [("another", false), ("bere", true)] {
        t.0.lock().unwrap().solar_project.push_back(project_response(serde_json::json!({"root":"eds_project/p-1/eds_solar","city_id":city,"client_snapshot_sha256":format!("{:x}",sha2::Sha256::digest(b"{}")),"input_base_fingerprint":"b".repeat(64)})));
        assert_eq!(client.solar_project("p-1", &command, NOW + 1).is_ok(), ok);
    }
    assert!(client
        .solar_project("../project", &command, NOW + 1)
        .is_err());
}
#[test]
fn solar_project_publish_upload_failure_retries_without_finalizing_and_rejects_stale_inputs() {
    use ds_client_core::solar_project::{Command, Output};
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let snapshot = solar_snapshot_envelope("eds_project/p-1/eds_solar", "bere");
    let fingerprint = snapshot["data"]["input_base_fingerprint"]
        .as_str()
        .unwrap()
        .to_string();
    let command = Command::Publish {
        city: "bere".into(),
        client_run_id: "offline-run".into(),
        engine_version: "ds-solar-engine@0.1.0".into(),
        build_manifest: "b".repeat(64),
        fingerprint,
        calculation: Output {
            id: "report-input".into(),
            format: "json".into(),
            content_type: "application/json".into(),
            bytes: b"{}".to_vec(),
        },
        reports: vec![],
    };
    let open = serde_json::json!({"work_id":"work-1","state":"pending","head_revision":0,"outputs":[{"output_id":"report-input","status":"upload_required","session_uri":"https://storage.googleapis.com/upload/storage/v1/b/test/o?upload_id=session"}]});
    for (status, success) in [(503, false), (200, true)] {
        t.push_solar_snapshot(200, &serde_json::to_vec(&snapshot).unwrap());
        {
            let mut state = t.0.lock().unwrap();
            state
                .solar_project
                .push_back(project_response(open.clone()));
            state.uploads.push_back(TransportResponse::new(status, b""));
            if success {
                state.solar_project.push_back(project_response(
                    serde_json::json!({"work_id":"work-1","state":"published","head_revision":1}),
                ));
            }
        }
        assert_eq!(
            client.solar_project("p-1", &command, NOW + 1).is_ok(),
            success
        );
    }
    let calls = t.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|s| *s == "solar-project:finalize")
            .count(),
        1
    );
    let mut command = command;
    if let Command::Publish {
        ref mut fingerprint,
        ..
    } = command
    {
        *fingerprint = "c".repeat(64);
    }
    t.push_solar_snapshot(200, &serde_json::to_vec(&snapshot).unwrap());
    assert!(client.solar_project("p-1", &command, NOW + 1).is_err());
    assert!(t.0.lock().unwrap().solar_project.is_empty());
}

#[test]
fn printing_global_does_not_require_a_project_and_project_save_keeps_scope_and_revision() {
    use ds_client_core::PrintingRequest;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    push_layers(
        &transport,
        serde_json::json!({"success":true,"data":{"setups":[]}}),
    );
    let layout = ds_command_kernel::printing::default_layout();
    push_layers(
        &transport,
        serde_json::json!({"success":true,"data":{"id":layout.id,"layout":layout,"revision":"a".repeat(64)}}),
    );
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    client
        .printing("", &PrintingRequest::List {}, NOW + 1)
        .unwrap();
    client
        .printing(
            "project_a",
            &PrintingRequest::Save {
                layout,
                expected_revision: String::new(),
            },
            NOW + 1,
        )
        .unwrap();
    let calls = transport.calls();
    let calls: Vec<_> = calls
        .iter()
        .filter(|v| v.starts_with("printing "))
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].contains("path=/api/v1/printing"));
    assert!(calls[0].contains("\"scope\":\"global\""));
    assert!(!calls[0].contains("project_id"));
    assert!(calls[1].contains("\"project_id\":\"project_a\""));
    assert!(calls[1].contains("\"expected_revision\":\"\""));
    let mut profile = profile_input(DeploymentLane::Stable);
    profile.printing_path = "/unreviewed".into();
    assert!(ClientProfile::validate(profile).is_err());
}

#[test]
fn printing_copy_pins_both_scopes_and_delete_pins_the_removed_revision() {
    use ds_client_core::{PrintingDestination, PrintingRequest, PrintingScope, PrintingSource};
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let revision = "a".repeat(64);
    let destination_revision = "b".repeat(64);
    let mut copied = ds_command_kernel::printing::default_layout();
    copied.id = "huye_a0".into();
    copied.name = "A0-LANDSCAPE-Huye-CJIC".into();
    push_layers(
        &transport,
        serde_json::json!({"success":true,"data":{"id":copied.id,"name":copied.name,"layout":copied,"revision":destination_revision}}),
    );
    push_layers(
        &transport,
        serde_json::json!({"success":true,"data":{"deleted":true,"id":"huye_a0","revision":destination_revision}}),
    );
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    client
        .printing(
            "project_a",
            &PrintingRequest::Copy {
                source: PrintingSource {
                    scope: PrintingScope::Global,
                    id: "shared_a0".into(),
                    revision,
                },
                destination: PrintingDestination {
                    scope: PrintingScope::Project,
                    id: "huye_a0".into(),
                    name: Some("A0-LANDSCAPE-Huye-CJIC".into()),
                    expected_revision: String::new(),
                },
            },
            NOW + 1,
        )
        .unwrap();
    client
        .printing(
            "project_a",
            &PrintingRequest::Delete {
                id: "huye_a0".into(),
                expected_revision: destination_revision,
            },
            NOW + 1,
        )
        .unwrap();
    let calls = transport.calls();
    let calls: Vec<_> = calls
        .iter()
        .filter(|value| value.starts_with("printing "))
        .collect();
    assert!(calls[0].contains("\"source\":{\"scope\":\"global\""));
    assert!(!calls[0].contains("\"source\":{\"project_id\""));
    assert!(calls[0].contains("\"expected_revision\":\"\""));
    assert!(calls[0].contains("\"project_id\":\"project_a\""));
    assert!(calls[1].contains("\"action\":\"delete\""));
    assert!(calls[1].contains(&format!("\"expected_revision\":\"{}\"", "b".repeat(64))));
}

#[test]
fn settings_writes_are_validated_then_freshly_verified_without_feeder_catalog() {
    use ds_client_core::ProjectConfigurationChange as Change;
    use serde_json::json;
    let document = json!({"sheets":{"project_settings":[{"parameter":"_Experimental Limit","value":1}],"lv_poles_rules":{"rule_edcl":{"table":[{"priority":1}],"default_material":"wooden"},"other":{"table":[],"default_material":"steel"}}}});
    for (change, sheet, expected) in [
        (
            Change::SetParameter {
                sheet: "project_settings".into(),
                parameter: "_experimental_limit".into(),
                raw: json!("007"),
            },
            "project_settings",
            json!([{"parameter":"_experimental_limit","value":7}]),
        ),
        (
            Change::SaveSheet {
                sheet: "project_settings".into(),
                value: json!([{"parameter":"_Experimental Limit","value":2},{}]),
            },
            "project_settings",
            json!([{"parameter":"_experimental_limit","value":2}]),
        ),
        (
            Change::DuplicateRuleSet {
                sheet: "lv_poles_rules".into(),
                source: "rule_edcl".into(),
                target: "Copy Set".into(),
            },
            "lv_poles_rules",
            json!({"rule_edcl":{"table":[{"priority":1}],"default_material":"wooden"},"other":{"table":[],"default_material":"steel"},"copy_set":{"table":[{"priority":1}],"default_material":"wooden"}}),
        ),
    ] {
        for matches in [true, false] {
            let transport = FixtureTransport::with_sign_in(SIGN_IN);
            let mut readback = document.clone();
            if matches {
                readback["sheets"][sheet] = expected.clone();
            }
            for response in [&document, &json!({"success":true}), &readback] {
                transport
                    .0
                    .lock()
                    .unwrap()
                    .configurations
                    .push_back(TransportResponse::new(
                        200,
                        serde_json::to_vec(response).unwrap(),
                    ));
            }
            let mut client = signed_in(transport.clone(), MemoryStore::default());
            let result = client.feeder_configuration("project_a", Some(&change), NOW);
            if matches {
                assert_eq!(result.unwrap().summary["saved"], true);
            } else {
                assert_eq!(result.unwrap_err().kind(), ErrorKind::UnreadableResponse);
            }
            let state = transport.0.lock().unwrap();
            assert_eq!(
                state.configuration_bodies,
                vec![
                    json!({"action":"save_config","eds_project_id":"project_a","sheet_name":sheet,"data":expected})
                ]
            );
            assert_eq!(
                &state.calls[state.calls.len() - 3..],
                &[
                    "config GET /config/project_a",
                    "config POST /report",
                    "config GET /config/project_a"
                ]
            );
        }
    }
}
#[test]
fn mv_printing_selection_fences_the_exact_base_and_requires_fresh_readback() {
    use ds_client_core::ProjectConfigurationChange;
    let selection: ds_command_kernel::printing::mv::Selection = serde_json::from_value(json!({
        "schema":"ds.mv-print-selection/v1","layout_id":"approved_mv","revision":"a".repeat(64),"external_version":"Version 1","issue_date":"2026-10-01"
    })).unwrap();
    let base = vec![json!({"parameter":"unrelated_lv_choice","value":["xlsx"]})];
    let expected = vec![
        base[0].clone(),
        json!({"parameter":"mv_printing_setup","value":selection}),
    ];
    for mode in ["stale_base", "wrong_readback", "saved"] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        let mut current = json!({"sheets":{"project_settings":base}});
        if mode == "stale_base" {
            current["sheets"]["project_settings"][0]["value"] = json!(["xlsx", "shp"]);
        }
        let readback = if mode == "saved" {
            json!({"sheets":{"project_settings":expected}})
        } else {
            json!({"sheets":{"project_settings":base}})
        };
        for response in [&current, &json!({"success":true}), &readback] {
            transport
                .0
                .lock()
                .unwrap()
                .configurations
                .push_back(TransportResponse::new(
                    200,
                    serde_json::to_vec(response).unwrap(),
                ));
        }
        let mut client = signed_in(transport.clone(), MemoryStore::default());
        let result = client.feeder_configuration(
            "project_a",
            Some(&ProjectConfigurationChange::MvPrintingSelection {
                selection: selection.clone(),
                expected_rows: base.clone(),
            }),
            NOW,
        );
        let state = transport.0.lock().unwrap();
        match mode {
            "stale_base" => {
                assert_eq!(result.unwrap_err().kind(), ErrorKind::InvalidInput);
                assert!(state.configuration_bodies.is_empty());
            }
            "wrong_readback" => {
                assert_eq!(result.unwrap_err().kind(), ErrorKind::UnreadableResponse)
            }
            _ => {
                assert_eq!(result.unwrap().summary["saved"], true);
                assert_eq!(
                    state.configuration_bodies,
                    vec![
                        json!({"action":"save_config","eds_project_id":"project_a","sheet_name":"project_settings","data":expected,"expected_data":base})
                    ]
                );
            }
        }
    }
}

#[test]
fn settings_invalid_mutations_never_reach_a_write() {
    use ds_client_core::ProjectConfigurationChange as Change;
    use serde_json::json;
    let document = json!({"sheets":{"project_settings":[{"parameter":"x","value":1}],"lv_poles_rules":{"Rule A":{"table":[],"default_material":"wooden"}}}});
    for change in [
        Change::SaveSheet {
            sheet: "unknown".into(),
            value: json!([]),
        },
        Change::SetParameter {
            sheet: "project_settings".into(),
            parameter: "missing".into(),
            raw: json!(1),
        },
        Change::DuplicateRuleSet {
            sheet: "lv_poles_rules".into(),
            source: "Rule A".into(),
            target: "rule_a".into(),
        },
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport
            .0
            .lock()
            .unwrap()
            .configurations
            .push_back(TransportResponse::new(
                200,
                serde_json::to_vec(&document).unwrap(),
            ));
        let mut client = signed_in(transport.clone(), MemoryStore::default());
        assert_eq!(
            client
                .feeder_configuration("project_a", Some(&change), NOW)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidInput
        );
        assert!(transport.0.lock().unwrap().configuration_bodies.is_empty());
    }
}

// ---------------------------------------------------------------------------
// Saved Transformer Status selections — the browser's grammar, headlessly
// ---------------------------------------------------------------------------
//
// This is the parity corpus for the `ds design selection …` residency move.
// Every body below is transcribed from the TypeScript that used to compose it
// (`ds-web/src/lib/api/design-collab.ts` request builders, driven by the
// `cli-design.ts` bridge executors this slice deleted), and every field name is
// ds-brain's own (`internal/handlers/design_selections.go` +
// `internal/services/design_selections.go`). ds-brain decodes with
// `DisallowUnknownFields`, so an extra key is a 400: the point of pinning the
// bytes is that the native caller sends the page's request, not a second one.

fn selection_head(state: &str) -> serde_json::Value {
    serde_json::json!({
        "project_id": "project_a",
        "selection_id": "sel-week-32",
        "name": "Week 32 rework",
        "mode": "snapshot",
        "object_kind": "lv_transformer",
        "member_digest": "a".repeat(64),
        "version": 4,
        "state": state,
        "members": [
            { "object": { "kind": "lv_transformer", "id": "tx_a" }, "saved_label": "TX A" },
            { "object": { "kind": "lv_transformer", "id": "tx_b" }, "saved_label": "TX B" },
        ],
        "assignment_count": 2,
    })
}

fn selection_envelope(data: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "success": true, "data": data })).unwrap()
}

#[test]
fn design_selection_requests_are_the_grammar_the_page_sends() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    transport.push_design_selections(
        200,
        &selection_envelope(serde_json::json!({ "selections": [selection_head("active")] })),
    );
    let listed = client
        .design_selections(
            "project_a",
            &ds_client_core::DesignSelectionRequest::List {
                include_archived: true,
            },
            NOW + 1,
        )
        .unwrap();
    match listed {
        ds_client_core::DesignSelectionAnswer::List(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].selection_id, "sel-week-32");
            assert_eq!(rows[0].version, 4);
            // A snapshot listing reports the stored member count; a query
            // selection reports none, because its membership is evaluated on
            // read and zero would read as "empty".
            assert_eq!(rows[0].members, Some(2));
            assert_eq!(rows[0].assignments, 2);
        }
        other => panic!("a listing answers with rows, not {other:?}"),
    }

    transport.push_design_selections(
        200,
        &selection_envelope(serde_json::json!({
            "selection": selection_head("active"),
            "members": [
                { "object": { "kind": "lv_transformer", "id": "tx_a" }, "saved_label": "TX A", "state": "present" },
                // A renamed transformer mints a new document identity, so the
                // member reads as missing — under the label it was saved with.
                { "object": { "kind": "lv_transformer", "id": "tx_b" }, "saved_label": "TX B", "state": "missing" },
                // No saved label: the id is the only spelling that still
                // exists, which is naming, not substitution.
                { "object": { "kind": "lv_transformer", "id": "tx_c" }, "state": "changed" },
            ],
            "present_count": 1,
            "changed_count": 1,
            "missing_count": 1,
            "evaluated_member_digest": "b".repeat(64),
        })),
    );
    let read = client
        .design_selections(
            "project_a",
            &ds_client_core::DesignSelectionRequest::Read {
                selection_id: "sel-week-32".to_owned(),
            },
            NOW + 1,
        )
        .unwrap();
    let read = match read {
        ds_client_core::DesignSelectionAnswer::Read(read) => read,
        other => panic!("a read answers with membership, not {other:?}"),
    };
    assert_eq!(read.member_digest, "b".repeat(64));
    assert_eq!(read.members[1].label, "TX B");
    assert_eq!(read.members[1].state, "missing");
    assert_eq!(read.members[2].label, "tx_c");

    transport.push_design_selections(
        200,
        &selection_envelope(serde_json::json!({ "selection": selection_head("active") })),
    );
    client
        .design_selections(
            "project_a",
            &ds_client_core::DesignSelectionRequest::Save(ds_client_core::DesignSelectionSave {
                selection_id: "sel-week-32".to_owned(),
                name: "Week 32 rework".to_owned(),
                description: None,
                member_ids: vec!["tx_a".to_owned(), "tx_b".to_owned()],
                expected_version: Some(4),
            }),
            NOW + 1,
        )
        .unwrap();

    transport.push_design_selections(
        200,
        &selection_envelope(serde_json::json!({ "selection": selection_head("archived") })),
    );
    client
        .design_selections(
            "project_a",
            &ds_client_core::DesignSelectionRequest::Archive(
                ds_client_core::DesignSelectionArchive {
                    selection_id: "sel-week-32".to_owned(),
                    expected_version: 4,
                    restore: false,
                },
            ),
            NOW + 1,
        )
        .unwrap();

    transport.push_design_selections(
        200,
        &selection_envelope(serde_json::json!({
            "assignment": {
                "assignment_id": "assign-week-32",
                "task_id": "task_71",
                "member_digest": "b".repeat(64),
                "member_ids": ["tx_a", "tx_c"],
                "missing_member_ids": ["tx_b"],
                "committed_revision": 18,
            }
        })),
    );
    let assigned = client
        .design_selections(
            "project_a",
            &ds_client_core::DesignSelectionRequest::Promote(
                ds_client_core::DesignSelectionPromotion {
                    selection_id: "sel-week-32".to_owned(),
                    assignment_id: "assign-week-32".to_owned(),
                    expected_version: 4,
                    expected_member_digest: "b".repeat(64),
                    command_id: "cmd-week-32-a".to_owned(),
                    title: "Rework week 32".to_owned(),
                    purpose: None,
                    responsible_email: Some("owner@example.com".to_owned()),
                    review_required: true,
                },
            ),
            NOW + 1,
        )
        .unwrap();
    match assigned {
        ds_client_core::DesignSelectionAnswer::Assigned(receipt) => {
            assert_eq!(receipt.task_id, "task_71");
            assert_eq!(receipt.missing_member_ids, vec!["tx_b".to_owned()]);
            assert_eq!(receipt.committed_revision, 18);
        }
        other => panic!("a promotion answers with a receipt, not {other:?}"),
    }

    let sent: Vec<String> = transport
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("design_selections "))
        .collect();
    assert_eq!(sent.len(), 5);
    for call in &sent {
        assert!(call.contains("method=POST "));
        assert!(call.contains("path=/api/v1/design/selections "));
        assert!(call.contains("content_type=application/json"));
        assert!(call.contains("api_key=true authorization=true email=true"));
        assert!(call.contains("correlation=true"));
        assert!(call.contains(&format!(
            "limit={}",
            ds_client_core::DESIGN_SELECTIONS_RESPONSE_LIMIT
        )));
    }
    assert!(sent[0]
        .contains(r#"body={"action":"list","project_id":"project_a","include_archived":true}"#));
    assert!(sent[1].contains(
        r#"body={"action":"get","project_id":"project_a","selection_id":"sel-week-32"}"#
    ));
    assert!(sent[2].contains(
        r#"body={"action":"save","project_id":"project_a","selection":{"selection_id":"sel-week-32","name":"Week 32 rework","mode":"snapshot","members":[{"object":{"kind":"lv_transformer","id":"tx_a"}},{"object":{"kind":"lv_transformer","id":"tx_b"}}],"expected_version":4}}"#
    ));
    assert!(sent[3].contains(
        r#"body={"action":"archive","project_id":"project_a","archive":{"selection_id":"sel-week-32","expected_version":4}}"#
    ));
    // `base_revision: 0` is ds-brain's own "no expectation" (projectwork
    // `apply.go`: a non-zero base revision must equal the graph's). The browser
    // asserts the revision it rendered; a headless caller never rendered the
    // work graph, so it asserts only the two fences it did observe — the
    // selection version and the evaluated member digest.
    assert!(sent[4].contains(
        r#"body={"action":"promote_task","project_id":"project_a","promotion":{"selection_id":"sel-week-32","assignment_id":"assign-week-32","expected_version":4,"expected_member_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","title":"Rework week 32","command_id":"cmd-week-32-a","base_revision":0,"responsible_email":"owner@example.com","review_required":true}}"#
    ));
}

#[test]
fn design_selection_requests_fail_closed_before_a_credential_is_spent() {
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let before = transport.calls().len();

    let refused: Vec<ds_client_core::DesignSelectionRequest> = vec![
        // Not ds-brain's id grammar.
        ds_client_core::DesignSelectionRequest::Read {
            selection_id: "Sel Week".to_owned(),
        },
        // A selection with no members is a selection of nothing.
        ds_client_core::DesignSelectionRequest::Save(ds_client_core::DesignSelectionSave {
            selection_id: "sel-week-32".to_owned(),
            name: "Week 32".to_owned(),
            description: None,
            member_ids: vec![],
            expected_version: None,
        }),
        // A replace carries the version it was read at, never 0.
        ds_client_core::DesignSelectionRequest::Archive(ds_client_core::DesignSelectionArchive {
            selection_id: "sel-week-32".to_owned(),
            expected_version: 0,
            restore: false,
        }),
        // A promotion echoes a 64-hex digest it was given, never a summary.
        ds_client_core::DesignSelectionRequest::Promote(ds_client_core::DesignSelectionPromotion {
            selection_id: "sel-week-32".to_owned(),
            assignment_id: "assign-week-32".to_owned(),
            expected_version: 4,
            expected_member_digest: "short".to_owned(),
            command_id: "cmd-week-32-a".to_owned(),
            title: "Rework week 32".to_owned(),
            purpose: None,
            responsible_email: None,
            review_required: true,
        }),
    ];
    for request in &refused {
        let error = client
            .design_selections("project_a", request, NOW + 1)
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
    }
    assert_eq!(transport.calls().len(), before);

    // ds-brain's refusals keep their own meaning rather than collapsing into
    // one unreadable answer: a moved selection is recoverable by reading again.
    for (status, kind) in [
        (403u16, ErrorKind::AuthenticationRejected),
        (404, ErrorKind::ResourceNotFound),
        (409, ErrorKind::InvalidInput),
    ] {
        transport.push_design_selections(
            status,
            &serde_json::to_vec(&serde_json::json!({ "success": false })).unwrap(),
        );
        let error = client
            .design_selections(
                "project_a",
                &ds_client_core::DesignSelectionRequest::Read {
                    selection_id: "sel-week-32".to_owned(),
                },
                NOW + 1,
            )
            .unwrap_err();
        assert_eq!(error.kind(), kind);
    }

    // A read with no evaluated digest cannot be promoted from, so it is not a
    // read at all.
    transport.push_design_selections(
        200,
        &selection_envelope(serde_json::json!({
            "selection": selection_head("active"),
            "members": [],
            "present_count": 0,
            "changed_count": 0,
            "missing_count": 0,
        })),
    );
    let error = client
        .design_selections(
            "project_a",
            &ds_client_core::DesignSelectionRequest::Read {
                selection_id: "sel-week-32".to_owned(),
            },
            NOW + 1,
        )
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
}

#[test]
fn solar_seed_native_rejects_wrong_project_city_mode_and_mutating_preview() {
    use ds_client_core::solar_project::Command;
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let command = Command::Seed {
        source: None,
        cities: vec!["bere".into()],
        overwrite: true,
        digest: None,
    };
    let valid = serde_json::json!({"plan":{"root":"eds_project/p-1/eds_solar","ds_project":"p-1","seed_source_root":"eds_solar","seed_digest":"a".repeat(64),"overwrite":true,"mutated":false,"cities":[{"city_id":"bere"}]}});
    for (field, value) in [
        ("root", serde_json::json!("eds_project/other/eds_solar")),
        ("overwrite", serde_json::json!(false)),
        ("mutated", serde_json::json!(true)),
        ("cities", serde_json::json!([{"city_id":"other"}])),
    ] {
        let mut wrong = valid.clone();
        wrong["plan"][field] = value;
        t.0.lock()
            .unwrap()
            .solar_project
            .push_back(project_response(wrong));
        assert!(client.solar_project("p-1", &command, NOW + 1).is_err());
    }
    t.0.lock()
        .unwrap()
        .solar_project
        .push_back(project_response(valid));
    assert!(client.solar_project("p-1", &command, NOW + 1).is_ok());
    let duplicated = Command::Seed {
        source: None,
        cities: vec!["bere".into(), "bere".into()],
        overwrite: false,
        digest: None,
    };
    assert!(client.solar_project("p-1", &duplicated, NOW + 1).is_err());
}

#[test]
fn solar_city_listing_is_bounded_and_requires_exact_counts_and_ids() {
    use ds_client_core::solar_project::Command;
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    for (data, ok) in [
        (serde_json::json!({"count":0,"templates":null}), true),
        (
            serde_json::json!({"count":1,"templates":[{"template_id":"bere"}]}),
            true,
        ),
        (
            serde_json::json!({"count":2,"templates":[{"template_id":"bere"}]}),
            false,
        ),
        (
            serde_json::json!({"count":1,"templates":[{"template_id":"../other"}]}),
            false,
        ),
    ] {
        t.0.lock()
            .unwrap()
            .solar_project
            .push_back(project_response(data));
        assert_eq!(
            client
                .solar_project("p-1", &Command::ListCities, NOW + 1)
                .is_ok(),
            ok
        );
    }
}

#[test]
fn solar_reference_uses_restored_authority_and_bounds_the_request() {
    use ds_client_core::solar_project::Command;
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let mut request =
        ds_solar_contracts::reference_request::ReferenceRequest::new("Bere", "Tchad", 9.29, 16.16);
    let result = client
        .solar_project(
            "p-1",
            &Command::Reference {
                request: request.clone(),
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(result["project"], "p-1");
    assert_eq!(result["artifacts"].as_array().unwrap().len(), 4);
    assert!(!result.to_string().contains("gateway"));
    request.latitude = 91.0;
    assert!(client
        .solar_project("p-1", &Command::Reference { request }, NOW + 1)
        .is_err());
    assert_eq!(
        t.0.lock()
            .unwrap()
            .calls
            .iter()
            .filter(|c| c.starts_with("solar-reference:"))
            .count(),
        1
    );
}

#[test]
fn headless_tag_batch_preserves_preview_scope_and_refuses_moved_response() {
    use ds_client_core::design_tags::Command;
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let digest = "a".repeat(64);
    let plan = serde_json::json!({"project_id":"p-1","group":"city","operation":"assign","plan_digest":digest,"state":"planned","outcomes":[{"transformer":"a","action":"assign","to":"bere"}],"counts":{"assign":1}});
    t.0.lock()
        .unwrap()
        .design_tags
        .push_back(project_response(serde_json::json!({"plan":plan})));
    let entries = vec![("a".into(), "bere".into())];
    let result = client
        .design_tags(
            "p-1",
            &Command::Batch {
                group: "city".into(),
                entries: entries.clone(),
                digest: None,
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(result["plan_digest"], digest);
    let sent = t.0.lock().unwrap().tag_bodies[0].clone();
    assert_eq!(sent["action"], "preview_tag_group");
    assert_eq!(sent["project_id"], "p-1");
    assert_eq!(sent["group_batch"]["entries"][0]["value"], "bere");
    let mut moved = plan.clone();
    moved["project_id"] = serde_json::json!("other");
    t.0.lock()
        .unwrap()
        .design_tags
        .push_back(project_response(serde_json::json!({"plan":moved})));
    assert!(client
        .design_tags(
            "p-1",
            &Command::Batch {
                group: "city".into(),
                entries: entries.clone(),
                digest: Some(digest)
            },
            NOW + 1
        )
        .is_err());
    assert!(Command::Batch {
        group: "city".into(),
        entries: vec![entries[0].clone(), entries[0].clone()],
        digest: None
    }
    .validate("p-1")
    .is_err());
}

#[test]
fn headless_tag_projection_verifies_exact_document_bytes() {
    use ds_client_core::design_tags::Command;
    use sha2::{Digest, Sha256};
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let doc=serde_json::json!({"project_id":"p-1","schema_version":"ds-report.design-tags/v3","assignments":[]}).to_string();
    let p = serde_json::json!({"project_id":"p-1","definition_ids":["city"],"document":doc,"bytes":doc.len(),"sha256":format!("{:x}",Sha256::digest(doc.as_bytes()))});
    t.0.lock()
        .unwrap()
        .design_tags
        .push_back(project_response(serde_json::json!({"projection":p})));
    let cmd = Command::Projection {
        transformers: vec!["a".into()],
        definitions: vec!["city".into()],
    };
    assert!(client.design_tags("p-1", &cmd, NOW + 1).is_ok());
    let mut wrong = p;
    wrong["document"] = serde_json::json!(format!("{doc} "));
    t.0.lock()
        .unwrap()
        .design_tags
        .push_back(project_response(serde_json::json!({"projection":wrong})));
    assert!(client.design_tags("p-1", &cmd, NOW + 1).is_err());
}

/// ds-brain answers a definition id it does not hold as active with its own
/// 404. That names a missing definition, never a rejected credential; the
/// Gateway's own 404 and every 401/403 keep their classification.
#[test]
fn headless_tag_unknown_definition_is_not_found_not_an_auth_rejection() {
    use ds_client_core::design_tags::Command;
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let brain_404 = serde_json::json!({
        "success": false,
        "error": {
            "code": "NOT_FOUND",
            "message": "selected grouping definition cty is not active in this project",
        },
    });
    let gateway_404 = serde_json::json!({"message": "Not Found", "code": 404});
    let forbidden = serde_json::json!({
        "success": false,
        "error": {"code": "FORBIDDEN", "message": "not a member of this project"},
    });
    let projection = Command::Projection {
        transformers: vec!["a".into()],
        definitions: vec!["cty".into()],
    };
    let batch = Command::Batch {
        group: "cty".into(),
        entries: vec![("a".into(), "bere".into())],
        digest: None,
    };
    let untagged = Command::Projection {
        transformers: vec!["a".into()],
        definitions: vec![],
    };
    for (command, status, body, kind) in [
        (&projection, 404u16, &brain_404, ErrorKind::ResourceNotFound),
        (&batch, 404, &brain_404, ErrorKind::ResourceNotFound),
        // No definition named: a 404 cannot be about one.
        (
            &untagged,
            404,
            &brain_404,
            ErrorKind::AuthenticationRejected,
        ),
        (
            &Command::Definitions,
            404,
            &brain_404,
            ErrorKind::AuthenticationRejected,
        ),
        // API Gateway's own answer for a path it was never taught.
        (
            &projection,
            404,
            &gateway_404,
            ErrorKind::AuthenticationRejected,
        ),
        (
            &projection,
            401,
            &forbidden,
            ErrorKind::AuthenticationRejected,
        ),
        (
            &projection,
            403,
            &forbidden,
            ErrorKind::AuthenticationRejected,
        ),
    ] {
        t.0.lock()
            .unwrap()
            .design_tags
            .push_back(TransportResponse::new(
                status,
                serde_json::to_vec(body).unwrap(),
            ));
        let error = client.design_tags("p-1", command, NOW + 1).unwrap_err();
        assert_eq!(error.kind(), kind, "HTTP {status} {body}");
        let refusal = error
            .service_refusal()
            .expect("the route's refusal crosses");
        assert_eq!(refusal.status(), status);
    }
    // The service's sentence, which names the definition, crosses bounded.
    t.0.lock()
        .unwrap()
        .design_tags
        .push_back(TransportResponse::new(
            404,
            serde_json::to_vec(&brain_404).unwrap(),
        ));
    let error = client.design_tags("p-1", &projection, NOW + 1).unwrap_err();
    let refusal = error.service_refusal().unwrap();
    assert!(refusal.message().unwrap().contains("cty"));
}

#[test]
fn headless_grid_download_checks_identity_and_bytes_before_receipt() {
    use ds_client_core::grid_models::Command;
    use sha2::{Digest, Sha256};
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let hash = format!("{:x}", Sha256::digest(b"model"));
    let object = format!("ds-grid/objects/sha256/{hash}/model.dsgrid");
    let data = serde_json::json!({"revision":{"project_id":"p-1","model_id":"m","revision_id":"r","model":{"digest":hash,"object":object,"byte_length":5}},"download_url":format!("https://storage.googleapis.com/bucket/{object}?signature=opaque")});
    let cmd = Command::Download {
        model: "m".into(),
        revision: "r".into(),
    };
    for (bytes, ok) in [(b"model".as_slice(), true), (b"wrong".as_slice(), false)] {
        t.0.lock()
            .unwrap()
            .grid_models
            .push_back(project_response(data.clone()));
        t.0.lock()
            .unwrap()
            .grid_bytes
            .push_back(TransportResponse::new(200, bytes));
        let r = client.grid_models("p-1", &cmd, NOW + 1);
        assert_eq!(r.is_ok(), ok);
        if let Ok(r) = r {
            assert_eq!(r.bytes.as_deref(), Some(b"model".as_slice()));
            assert!(!r.data.to_string().contains("signature"));
        }
    }
    let mut wrong = data;
    wrong["revision"]["model_id"] = serde_json::json!("other");
    t.0.lock()
        .unwrap()
        .grid_models
        .push_back(project_response(wrong));
    assert!(client.grid_models("p-1", &cmd, NOW + 1).is_err());
}

#[test]
fn shared_assets_missing_is_manual_and_foreign_resolution_is_rejected() {
    use ds_client_core::shared_assets::Command;
    use ds_command_kernel::assets::Link;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let command = Command::Resolve {
        link: Link::Tag {
            definition_id: "city".into(),
            value: "No geography".into(),
        },
        role: "network_information".into(),
    };
    for project in ["p-1", "other"] {
        transport.0.lock().unwrap().shared_assets.push_back(project_response(serde_json::json!({"project_id":project,"status":"missing","manual_entry_allowed":true})));
        let result = client.shared_assets("p-1", &command, NOW + 1);
        if project == "p-1" {
            assert_eq!(result.unwrap()["resolution"]["manual_entry_allowed"], true);
        } else {
            assert!(result.is_err());
        }
    }
    assert!(transport.0.lock().unwrap().shared_bytes.is_empty());
}

#[test]
fn shared_assets_verify_producer_bytes_without_returning_signed_urls() {
    use ds_client_core::shared_assets::Command;
    use sha2::Sha256;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let bytes = b"classified source";
    let digest = format!("sha256:{:x}", Sha256::digest(bytes));
    let command = Command::Read {
        asset_id: "a_000000000001".into(),
        digest: digest.clone(),
    };
    let asset = serde_json::json!({"schema":"ds.assets.asset/v1","project_id":"p-1","asset_id":"a_000000000001","bytes":bytes.len(),"digest":digest});
    for body in [bytes.as_slice(), b"tampered! source".as_slice()] {
        transport.0.lock().unwrap().shared_assets.push_back(project_response(serde_json::json!({"asset":asset,"download_url":"https://storage.googleapis.com/producer/table.xlsx?generation=7&token=private"})));
        transport
            .0
            .lock()
            .unwrap()
            .shared_bytes
            .push_back(TransportResponse::new(200, body));
        let result = client.shared_assets("p-1", &command, NOW + 1);
        if body == bytes {
            let result = result.unwrap();
            assert_eq!(result["verified"], true);
            assert!(!result.to_string().contains("private"));
        } else {
            assert!(result.is_err());
        }
    }
}

fn index_meta() -> serde_json::Value {
    serde_json::json!({
        "schema": "ds.assets.index/v1", "generation": 7,
        "built_at": "2026-09-25T10:00:00Z", "checked_at": "2026-09-25T10:05:00Z",
        "rebuilt": false, "stale_sources": ["solar"],
        "truncated_sources": [{"source": "survey_media", "bound": 20000, "actual": 20431}]
    })
}

fn old_ds_brain() -> TransportResponse {
    TransportResponse::new(
        400,
        br#"{"success":false,"error":{"code":"VALIDATION_FAILED","message":"Invalid assets request"}}"#,
    )
}

#[test]
fn project_assets_index_list_carries_the_meta_through_and_refuses_a_foreign_row() {
    use ds_client_core::project_assets::{Command, IndexList, IndexOrder};
    use serde_json::json;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let command = Command::IndexList(IndexList {
        order: IndexOrder::Recent,
        folder: Some("Transformers/TX-104".into()),
        kind: None,
        status: None,
        sensitivity: None,
        since: None,
        limit: 2,
        cursor: None,
        refresh: true,
    });
    let design = json!({"schema":"ds.assets.asset/v1","asset_id":"sys:transformers:TX-104:design",
        "project_id":"p-1","folder_path":"Transformers/TX-104","modified_at":"2026-09-25T09:00:00Z",
        "versions":{"count":12,"current":"v3","latest_at":"2026-09-25T09:00:00Z"}});
    let upload = json!({"schema":"ds.assets.asset/v1","asset_id":"a_000000000001","project_id":"p-1",
        "folder_path":"Transformers/TX-104","modified_at":null});
    transport
        .0
        .lock()
        .unwrap()
        .shared_assets
        .push_back(project_response(json!({
            "assets": [design, upload], "more": true, "next_cursor": "g7:2", "index": index_meta(),
        })));
    let page = client
        .project_assets("p-1", &command, None, NOW + 1)
        .unwrap();
    assert_eq!(page["index"], index_meta(), "the meta crosses untouched");
    assert_eq!(page["index_status"], "served");
    assert_eq!(page["assets"][0]["versions"]["count"], 12);
    assert_eq!(page["next_cursor"], "g7:2");
    assert_eq!(
        transport.0.lock().unwrap().asset_bodies[0],
        json!({"action":"list","ds_project":"p-1","view":"index","order":"recent",
               "folder":"Transformers/TX-104","limit":2,"refresh":true})
    );

    // A row of another project, a page longer than asked, or an answer
    // without the index meta is not an index page.
    let foreign = json!({"schema":"ds.assets.asset/v1","asset_id":"sys:transformers:TX-9","project_id":"other"});
    let own = json!({"schema":"ds.assets.asset/v1","asset_id":"a_000000000002","project_id":"p-1"});
    for answer in [
        json!({"assets":[foreign],"more":false,"index":index_meta()}),
        json!({"assets":[own.clone(), own.clone(), own.clone()],"more":false,"index":index_meta()}),
        json!({"assets":[own.clone()],"more":false}),
        json!({"assets":[own],"more":false,"index":{"schema":"ds.assets.index/v2","generation":7}}),
    ] {
        transport
            .0
            .lock()
            .unwrap()
            .shared_assets
            .push_back(project_response(answer));
        assert_eq!(
            client
                .project_assets("p-1", &command, None, NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }
}

#[test]
fn project_assets_index_tree_and_versions_are_served_with_their_rows_checked() {
    use ds_client_core::project_assets::Command;
    use serde_json::json;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let row = |id: &str| json!({"schema":"ds.assets.asset/v1","asset_id":id,"project_id":"p-1"});
    let tree = json!({
        "schema": "ds.assets.tree/v1", "total_assets": 2, "total_folders": 3, "truncated": false,
        "folders": [{"kind":"system","path":"Transformers","name":"Transformers",
            "counts":{"assets":0,"folders":1},"assets":[],
            "children":[{"kind":"system","path":"Transformers/TX-104","name":"TX-104",
                "counts":{"assets":2,"folders":0},"children":[],
                "assets":[row("sys:transformers:TX-104"), row("a_000000000001")]}]}],
        "index": index_meta(),
    });
    transport
        .0
        .lock()
        .unwrap()
        .shared_assets
        .push_back(project_response(tree.clone()));
    let served = client
        .project_assets("p-1", &Command::IndexTree { refresh: false }, None, NOW + 1)
        .unwrap();
    assert_eq!(served["index_status"], "served");
    assert_eq!(served["folders"], tree["folders"]);
    assert_eq!(served["index"]["generation"], 7);
    let mut deep_foreign = tree.clone();
    deep_foreign["folders"][0]["children"][0]["assets"][0]["project_id"] = json!("other");
    transport
        .0
        .lock()
        .unwrap()
        .shared_assets
        .push_back(project_response(deep_foreign));
    assert!(client
        .project_assets("p-1", &Command::IndexTree { refresh: true }, None, NOW + 1)
        .is_err());
    assert_eq!(
        transport.0.lock().unwrap().asset_bodies[1],
        json!({"action":"tree","ds_project":"p-1","view":"index","refresh":true})
    );

    let get = Command::IndexGet {
        asset_id: "sys:transformers:TX-104".into(),
    };
    let versions = json!([
        {"asset_id":"sys:transformer_versions:TX-104:v3","label":"v3","created_at":"2026-09-25T09:00:00Z",
         "created_by":"lead@ds.rw","milestone":"IFC","current":true},
        {"asset_id":"sys:transformer_versions:TX-104:v2","label":"v2","created_at":"2026-09-20T09:00:00Z",
         "created_by":"lead@ds.rw","current":false}
    ]);
    transport
        .0
        .lock()
        .unwrap()
        .shared_assets
        .push_back(project_response(
            json!({"asset":row("sys:transformers:TX-104"),"versions":versions}),
        ));
    let answer = client.project_assets("p-1", &get, None, NOW + 1).unwrap();
    assert_eq!(answer["versions"], versions, "newest first, as served");
    assert_eq!(answer["index_status"], "served");
    // The row the server answered must be the row that was asked for.
    transport
        .0
        .lock()
        .unwrap()
        .shared_assets
        .push_back(project_response(
            json!({"asset":row("sys:transformers:TX-9"),"versions":[]}),
        ));
    assert!(client.project_assets("p-1", &get, None, NOW + 1).is_err());
}

#[test]
fn project_assets_index_falls_back_once_and_says_so_on_an_old_ds_brain() {
    use ds_client_core::project_assets::{Command, IndexList, IndexOrder};
    use serde_json::json;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let push = |response: TransportResponse| {
        transport
            .0
            .lock()
            .unwrap()
            .shared_assets
            .push_back(response)
    };
    let bodies = || transport.0.lock().unwrap().asset_bodies.clone();
    let list = |folder: Option<&str>| {
        Command::IndexList(IndexList {
            order: IndexOrder::Name,
            folder: folder.map(str::to_owned),
            kind: Some("doc".into()),
            status: None,
            sensitivity: None,
            since: None,
            limit: 50,
            cursor: None,
            refresh: true,
        })
    };
    let upload =
        json!({"schema":"ds.assets.asset/v1","asset_id":"a_000000000001","project_id":"p-1"});

    // list: the same filters, without view, order, folder or refresh.
    push(old_ds_brain());
    push(project_response(
        json!({"assets":[upload.clone()],"has_more":false,"scanned":1}),
    ));
    let page = client
        .project_assets("p-1", &list(None), None, NOW + 1)
        .unwrap();
    assert_eq!(page["index_status"], "unavailable");
    assert!(
        page.get("index").is_none(),
        "a catalogue page is never the index"
    );
    assert_eq!(page["assets"][0]["asset_id"], "a_000000000001");
    assert_eq!(bodies()[0]["view"], "index");
    assert_eq!(
        bodies()[1],
        json!({"action":"list","ds_project":"p-1","kind":"doc","limit":50})
    );

    // tree: the declared folders.
    push(old_ds_brain());
    push(project_response(json!({"folders":[]})));
    let tree = client
        .project_assets("p-1", &Command::IndexTree { refresh: false }, None, NOW + 1)
        .unwrap();
    assert_eq!(tree, json!({"folders":[],"index_status":"unavailable"}));
    assert_eq!(bodies()[3], json!({"action":"tree","ds_project":"p-1"}));

    // get of a catalogued row: the row, and no claim about its versions.
    push(old_ds_brain());
    push(project_response(json!({"asset": upload.clone()})));
    let got = client
        .project_assets(
            "p-1",
            &Command::IndexGet {
                asset_id: "a_000000000001".into(),
            },
            None,
            NOW + 1,
        )
        .unwrap();
    assert_eq!(got["asset"], upload);
    assert_eq!(got["index_status"], "unavailable");
    assert!(got.get("versions").is_none());
    assert_eq!(
        bodies()[5],
        json!({"action":"get","ds_project":"p-1","asset_id":"a_000000000001"})
    );

    // No catalogue form — a folder path, a projected id — no retry: the
    // marker alone.
    for command in [
        list(Some("Transformers/TX-104")),
        Command::IndexGet {
            asset_id: "sys:transformers:TX-104".into(),
        },
    ] {
        let before = bodies().len();
        push(old_ds_brain());
        assert_eq!(
            client
                .project_assets("p-1", &command, None, NOW + 1)
                .unwrap(),
            json!({"index_status":"unavailable"})
        );
        assert_eq!(bodies().len(), before + 1);
    }

    // Once: a refused retry crosses as it is, and is not retried again.
    let before = bodies().len();
    push(old_ds_brain());
    push(old_ds_brain());
    let error = client
        .project_assets("p-1", &list(None), None, NOW + 1)
        .unwrap_err();
    assert_eq!(
        error.service_refusal().and_then(|r| r.code()),
        Some("validation_failed")
    );
    assert_eq!(bodies().len(), before + 2);

    // Any other refusal is the index's own: no fallback.
    for (status, body) in [
        (
            409,
            &br#"{"success":false,"error":{"code":"ASSETS_INDEX_MOVED","message":"the index moved; restart"}}"#[..],
        ),
        (
            400,
            br#"{"success":false,"error":{"code":"ASSET_REFUSED","message":"no"}}"#,
        ),
    ] {
        let before = bodies().len();
        push(TransportResponse::new(status, body));
        let error = client
            .project_assets("p-1", &list(None), None, NOW + 1)
            .unwrap_err();
        assert_eq!(error.service_refusal().map(|r| r.status()), Some(status));
        assert_eq!(bodies().len(), before + 1);
    }
}

#[test]
fn shared_map_publish_uploads_classifies_links_and_reuses_without_another_upload() {
    use ds_client_core::shared_assets::Command;
    use ds_command_kernel::assets::Link;
    use serde_json::json;
    use sha2::Sha256;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let bytes = b"%PDF-1.7\nmap\n%%EOF";
    let digest = format!("sha256:{:x}", Sha256::digest(bytes));
    let tag = Link::Tag {
        definition_id: "city".into(),
        value: "gagal".into(),
    };
    let command = Command::PublishMap {
        name: "Gagal-A3.pdf".into(),
        bytes: bytes.to_vec(),
        tags: vec![tag.clone()],
        replaces: vec![],
    };
    let mut asset = json!({"schema":"ds.assets.asset/v1","project_id":"p-1","asset_id":"a_000000000001","name":"Gagal-A3.pdf","bytes":bytes.len(),"digest":digest,"format":"pdf","kind":"doc","status":"fresh","version":1,"links":[]});
    {
        let mut state = transport.0.lock().unwrap();
        state.shared_assets.push_back(project_response(
            json!({"assets":[],"has_more":false,"truncated":false}),
        ));
        state.shared_assets.push_back(project_response(json!({"asset_id":"a_000000000001","upload_url":"https://storage.googleapis.com/upload/storage/v1/b/project/o?upload_id=private"})));
        state.uploads.push_back(TransportResponse::new(200, b"{}"));
        state
            .shared_assets
            .push_back(project_response(asset.clone()));
        asset["kind"] = json!("geo");
        asset["status"] = json!("durable");
        asset["version"] = json!(2);
        state
            .shared_assets
            .push_back(project_response(json!({"asset":asset})));
        asset["links"] = json!([tag]);
        state
            .shared_assets
            .push_back(project_response(asset.clone()));
    }
    let published = client.shared_assets("p-1", &command, NOW + 1).unwrap();
    assert_eq!(published["published"], true);
    assert_eq!(published["reused"], false);
    assert_eq!(published["map"]["tags"][0]["value"], "gagal");
    assert!(!published.to_string().contains("private"));
    let bodies = transport.0.lock().unwrap().asset_bodies.clone();
    assert_eq!(
        bodies
            .iter()
            .map(|b| b["action"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "list",
            "ingest_start",
            "ingest_finalize",
            "classify",
            "attach"
        ]
    );
    transport
        .0
        .lock()
        .unwrap()
        .shared_assets
        .push_back(project_response(
            json!({"assets":[asset],"has_more":false,"truncated":false}),
        ));
    let replay = client.shared_assets("p-1", &command, NOW + 2).unwrap();
    assert_eq!(replay["reused"], true);
    assert_eq!(replay["bytes_uploaded"], 0);
}

#[test]
fn shared_map_publish_rejects_foreign_or_incomplete_catalogue_before_upload() {
    use ds_client_core::shared_assets::Command;
    use serde_json::json;
    for page in [
        json!({"assets":[{"project_id":"foreign"}],"has_more":false,"truncated":false}),
        json!({"assets":[],"has_more":true,"truncated":false}),
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(transport.clone(), MemoryStore::default());
        transport
            .0
            .lock()
            .unwrap()
            .shared_assets
            .push_back(project_response(page));
        assert!(client
            .shared_assets(
                "p-1",
                &Command::PublishMap {
                    name: "map.pdf".into(),
                    bytes: b"%PDF-1.7\nmap".to_vec(),
                    tags: vec![],
                    replaces: vec![],
                },
                NOW + 1
            )
            .is_err());
        assert_eq!(transport.0.lock().unwrap().asset_bodies.len(), 1);
    }
}

#[test]
fn shared_map_replacement_archives_only_after_verified_publication_and_can_resume() {
    use ds_client_core::shared_assets::Command;
    use ds_command_kernel::assets::Link;
    use serde_json::json;
    for already_archived in [false, true] {
        let t = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(t.clone(), MemoryStore::default());
        let bytes = b"%PDF-1.7\nreplacement";
        let tag = Link::Tag {
            definition_id: "city".into(),
            value: "kyabe".into(),
        };
        let command = Command::PublishMap {
            name: "Kyabe-A3.pdf".into(),
            bytes: bytes.to_vec(),
            tags: vec![tag.clone()],
            replaces: vec!["a_000000000001".into()],
        };
        let mut old = json!({"schema":"ds.assets.asset/v1","project_id":"p-1","asset_id":"a_000000000001",
            "name":"Old dated A3.pdf","kind":"geo","format":"pdf","status":if already_archived {"archive"} else {"durable"},
            "digest":format!("sha256:{}", "a".repeat(64)),"version":4,"links":[tag]});
        let new = json!({"schema":"ds.assets.asset/v1","project_id":"p-1","asset_id":"a_000000000002",
            "name":"Kyabe-A3.pdf","kind":"geo","format":"pdf","status":"durable","bytes":bytes.len(),
            "digest":format!("sha256:{:x}", sha2::Sha256::digest(bytes)),"version":2,"links":[tag]});
        {
            let mut state = t.0.lock().unwrap();
            state.shared_assets.push_back(project_response(old.clone()));
            state.shared_assets.push_back(project_response(
                json!({"assets":[new],"has_more":false,"truncated":false}),
            ));
            if !already_archived {
                old["status"] = json!("archive");
                old["version"] = json!(5);
                state
                    .shared_assets
                    .push_back(project_response(json!({"asset":old})));
            }
        }
        let reply = client.shared_assets("p-1", &command, NOW + 1).unwrap();
        assert_eq!(reply["superseded_asset_ids"], json!(["a_000000000001"]));
        assert_eq!(reply["bytes_uploaded"], 0);
        let state = t.0.lock().unwrap();
        if !already_archived {
            assert_eq!(state.asset_bodies[2]["action"], "classify");
            assert_eq!(state.asset_bodies[2]["classify"]["expected_version"], 4);
            assert_eq!(state.asset_bodies[2]["classify"]["status"], "archive");
        } else {
            assert_eq!(state.asset_bodies.len(), 2);
        }
    }
}

#[test]
fn shared_map_replacement_refuses_wrong_scope_before_mutating_and_keeps_old_on_failure() {
    use ds_client_core::shared_assets::Command;
    use serde_json::json;
    for fault in ["project", "format", "tags", "publication"] {
        let t = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(t.clone(), MemoryStore::default());
        let command = Command::PublishMap {
            name: "map.pdf".into(),
            bytes: b"%PDF-1.7\nmap".to_vec(),
            tags: vec![],
            replaces: vec!["a_000000000001".into()],
        };
        let mut old = json!({"schema":"ds.assets.asset/v1","project_id":"p-1","asset_id":"a_000000000001",
            "name":"old.pdf","kind":"geo","format":"pdf","status":"durable","version":4,"links":[]});
        match fault {
            "project" => old["project_id"] = json!("other"),
            "format" => old["format"] = json!("png"),
            "tags" => old["links"] = json!([{"kind":"tag","definition_id":"city","value":"other"}]),
            _ => {}
        }
        {
            let mut state = t.0.lock().unwrap();
            state.shared_assets.push_back(project_response(old));
            if fault == "publication" {
                state
                    .shared_assets
                    .push_back(TransportResponse::new(500, b"{}"));
            }
        }
        assert!(client.shared_assets("p-1", &command, NOW + 1).is_err());
        let state = t.0.lock().unwrap();
        assert!(state
            .asset_bodies
            .iter()
            .all(|body| matches!(body["action"].as_str(), Some("get" | "list"))));
    }
}

/// Survey-data migration is stateless: both projects travel on every call, the
/// body is exactly the four keys of the copy-everything policy, and a receipt
/// for the other mode is refused rather than rendered.
#[test]
fn headless_survey_migration_names_both_projects_and_asks_only_to_copy() {
    use ds_client_core::survey_migration::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let service = |dry_run: bool| {
        project_response(json!({
            "dry_run": dry_run, "total_matched": 14, "total_migrated": 12, "total_skipped": 2,
            "per_form": {"customers": 12}, "skip_reasons": {"existing": 2}
        }))
    };
    {
        let mut state = transport.0.lock().unwrap();
        state.survey_migration.push_back(service(true));
        state.survey_migration.push_back(service(false));
        // An apply answered as if it had been a plan.
        state.survey_migration.push_back(service(true));
        state.survey_migration.push_back(TransportResponse::new(
            403,
            br#"{"success":false,"error":"missing pipeline.migrate capability"}"#.to_vec(),
        ));
    }
    let plan = Command {
        source_project: "source_one".into(),
        dry_run: true,
    };
    let apply = Command {
        source_project: "source_one".into(),
        dry_run: false,
    };

    let planned = client
        .survey_migration("target_one", &plan, NOW + 1)
        .unwrap();
    assert_eq!(planned["total_migrated"], 12);
    let applied = client
        .survey_migration("target_one", &apply, NOW + 1)
        .unwrap();
    assert_eq!(applied["dry_run"], false);
    let bodies = transport.0.lock().unwrap().survey_migration_bodies.clone();
    assert_eq!(
        bodies[0],
        json!({"action":"migrate","eds_project_id":"source_one",
            "target_project_id":"target_one","dry_run":true})
    );
    assert_eq!(
        bodies[1],
        json!({"action":"migrate","eds_project_id":"source_one",
            "target_project_id":"target_one","dry_run":false})
    );

    let mismatched = client
        .survey_migration("target_one", &apply, NOW + 1)
        .unwrap_err();
    assert_eq!(mismatched.kind(), ErrorKind::UnreadableResponse);

    let refused = client
        .survey_migration("target_one", &plan, NOW + 1)
        .unwrap_err();
    let refusal = refused.service_refusal().expect("the route's own refusal");
    assert_eq!(refusal.status(), 403);
    assert_eq!(
        refusal.message(),
        Some("missing pipeline.migrate capability")
    );

    // A destination that is its own source, or a path, never leaves the client.
    for destination in ["source_one", "../target_one"] {
        assert_eq!(
            client
                .survey_migration(destination, &plan, NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidInput
        );
    }
    assert_eq!(
        transport.0.lock().unwrap().survey_migration_bodies.len(),
        4,
        "a refused request reached the transport"
    );
}

fn feedback_row(status: &str, version: u64) -> serde_json::Value {
    serde_json::json!({"id":"f1","status":status,"kind":"bug","severity":"major","component":"ds-cli","surface":"cli","title":"Headless feedback","detail":"Must work without a selected project","occurrences":1,"reporters":["operator@example.com"],"reporter_kind":"agent","version":version,"last_seen_at":"2026-09-13T12:00:00Z","updated_at":"2026-09-13T12:00:00Z","updated_by":"operator@example.com"})
}
#[test]
fn headless_feedback_reads_the_difference_and_never_narrows_after_the_answer() {
    use ds_client_core::feedback::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    transport
        .0
        .lock()
        .unwrap()
        .feedback
        .push_back(project_response(serde_json::json!({
            "reports":[feedback_row("open",1)],
            "count":1,"matched":1,"complete":true,"changed":true,"scanned":144,
            "cursor":"fb1.abc.def","cursor_source":"reader",
            "backlog":{"total":144,"open":1,"blocked":0,"settled":143}
        })));
    let request = Command::List {
        view: "not_addressed".into(),
        component: Some("ds-cli".into()),
        query: None,
        limit: 50,
        detail: true,
        cursor: None,
        all: false,
    };
    let result = client.feedback(&request, NOW + 1).unwrap();
    assert_eq!(result["reports"].as_array().unwrap().len(), 1);
    assert_eq!(result["reports"][0]["id"], "f1");
    // The horizon field is gone: the backlog enumerates completely, so there is
    // no "older records may exist" for a caller to cope with.
    assert!(result.get("scan_incomplete").is_none());
    assert_eq!(result["changed"], true);
    assert_eq!(result["complete"], true);
    // This read NARROWS, so it is a lookup and not a chunk of the sweep: its
    // answer is the same reports every time it is asked, and it neither reads
    // nor moves where the sweep left off. The unfiltered read is the drain.
    assert_eq!(result["drains"], false);
    assert_eq!(result["cursor"], "fb1.abc.def");
    assert_eq!(result["backlog"]["settled"], 143);
    // Every filter went to the backlog. A filter applied HERE would have hidden
    // rows the server's watermark had already counted as read.
    assert_eq!(
        transport.0.lock().unwrap().feedback_bodies[0],
        serde_json::json!({"action":"list","limit":50,"filter":{
            "view":"not_addressed","component_contains":"ds-cli","query":"","limit":50}})
    );
    assert!(serde_json::from_value::<Command>(
        serde_json::json!({"operation":"list","view":"all","limit":50,"project":"injected"})
    )
    .is_err());
}

#[test]
fn headless_feedback_says_nothing_changed_without_the_caller_holding_anything() {
    use ds_client_core::feedback::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    transport
        .0
        .lock()
        .unwrap()
        .feedback
        .push_back(project_response(serde_json::json!({
            "reports":[],"count":0,"matched":0,"complete":true,"changed":false,
            "scanned":0,"cursor":"fb1.abc.def","cursor_source":"reader"
        })));
    let request = Command::List {
        view: "not_addressed".into(),
        component: None,
        query: None,
        limit: 20,
        detail: false,
        cursor: None,
        all: false,
    };
    let result = client.feedback(&request, NOW + 1).unwrap();
    // The cheap answer is a FIELD, not an empty list to interpret — and the
    // caller reached it having sent no cursor, no timestamp and no local file.
    assert_eq!(result["changed"], false);
    assert_eq!(result["total"], 0);
    assert_eq!(result["cursor_source"], "reader");
    assert!(result.get("backlog").is_none());
    // No cursor travelled, and no timestamp: the caller opted into the
    // watermark the backlog already holds for this account, and that is the
    // whole of what it had to send.
    let sent = transport.0.lock().unwrap().feedback_bodies[0].clone();
    assert!(sent.get("cursor").is_none());
    assert_eq!(sent["use_watermark"], true);
}

#[test]
fn headless_feedback_open_is_what_waits_on_nothing_and_the_backlog_decides_it() {
    use ds_client_core::feedback::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    for _ in 0..2 {
        transport
            .0
            .lock()
            .unwrap()
            .feedback
            .push_back(project_response(serde_json::json!({
                "reports":[],"count":0,"matched":0,"complete":true,"changed":false,
                "scanned":0,"cursor":"fb1.abc.def","cursor_source":"reader"
            })));
    }
    for view in ["open", "waiting"] {
        let request = Command::List {
            view: view.into(),
            component: None,
            query: None,
            limit: 20,
            detail: false,
            cursor: None,
            all: false,
        };
        let result = client.feedback(&request, NOW + 1).unwrap();
        // The caller keeps the name it asked for; the backlog got its own view
        // plus the blocked filter, so each half drains under its own watermark.
        assert_eq!(result["view"], view);
    }
    let sent = transport.0.lock().unwrap().feedback_bodies.clone();
    assert_eq!(sent.len(), 2);
    for (body, blocked) in sent.iter().zip([false, true]) {
        assert_eq!(body["filter"]["view"], "not_addressed");
        assert_eq!(body["filter"]["blocked"], blocked);
        assert_eq!(body["use_watermark"], true);
    }
}

#[test]
fn headless_feedback_separates_a_drain_from_a_read_of_the_top() {
    use ds_client_core::feedback::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    // Two truncated answers that look identical on the wire. They mean
    // opposite things, and only the request said which: the watermark read
    // hands over the next chunk when it is called again, while `--all` reads
    // the top of the backlog and would return these same rows forever.
    for _ in 0..2 {
        transport
            .0
            .lock()
            .unwrap()
            .feedback
            .push_back(project_response(serde_json::json!({
                "reports":[feedback_row("open",1)],
                "count":1,"matched":45,"complete":false,"changed":true,"scanned":45,
                "cursor":"fb1.abc.def","cursor_source":"none"
            })));
    }
    let drained = client
        .feedback(
            &Command::List {
                view: "not_addressed".into(),
                component: None,
                query: None,
                limit: 1,
                detail: false,
                cursor: None,
                all: false,
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(drained["truncated"], true);
    assert_eq!(drained["drains"], true);

    let top = client
        .feedback(
            &Command::List {
                view: "not_addressed".into(),
                component: None,
                query: None,
                limit: 1,
                detail: false,
                cursor: None,
                all: true,
            },
            NOW + 2,
        )
        .unwrap();
    assert_eq!(top["truncated"], true);
    assert_eq!(top["drains"], false);
    // `--all` neither reads nor advances the account's watermark, so it is a
    // peek at the top and never moves anyone else's starting point.
    let sent = transport.0.lock().unwrap().feedback_bodies[1].clone();
    assert!(sent.get("cursor").is_none());
    assert!(sent.get("use_watermark").is_none());
}

/// A ROW DECIDES; IT DOES NOT EXPLAIN. The excerpt is the measured cost.
///
/// The backlog cost about 1.5 million tokens because every visit read the long
/// acceptance text of every report. The watermark stops the RE-reading, but a
/// row that carries 1200 characters of that text makes the FIRST read a
/// mountain on its own — roughly 37 000 tokens for 124 reports. What decides
/// "do I care about this" is the title, the status, the blocker and the latest
/// note, and those are on the row in full. The acceptance condition decides
/// something else — whether the report can be CLOSED — and that is the handful
/// of ids `--detail` is for. `detail_truncated` says which of the two a row is.
#[test]
fn headless_feedback_rows_carry_a_decision_not_the_acceptance_text() {
    use ds_client_core::feedback::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let mut long = feedback_row("open", 1);
    long["detail"] = serde_json::json!("x".repeat(4000));
    for _ in 0..2 {
        transport
            .0
            .lock()
            .unwrap()
            .feedback
            .push_back(project_response(serde_json::json!({
                "reports":[long.clone()],
                "count":1,"matched":1,"complete":true,"changed":true,"scanned":1,
                "cursor":"fb1.abc.def","cursor_source":"reader"
            })));
    }
    let request = |detail: bool| Command::List {
        view: "not_addressed".into(),
        component: None,
        query: None,
        limit: 20,
        detail,
        cursor: None,
        all: false,
    };
    let row = client.feedback(&request(false), NOW + 1).unwrap();
    let excerpt = row["reports"][0]["detail"].as_str().unwrap();
    assert!(
        excerpt.chars().count() <= 240,
        "a row carried {} characters of acceptance text; that is the mountain, per report",
        excerpt.chars().count()
    );
    // Clipped is never silent: the row says so, and `--detail` is the remedy.
    assert_eq!(row["reports"][0]["detail_truncated"], true);

    let full = client.feedback(&request(true), NOW + 2).unwrap();
    assert_eq!(full["reports"][0]["detail"].as_str().unwrap().len(), 4000);
    assert_eq!(full["reports"][0]["detail_truncated"], false);
}

/// A LOOKUP IS NOT A SWEEP, and answering it with a difference makes the next
/// session blind to the reports it came to find.
///
/// The watermark exists for the mountain: the whole `not_addressed` backlog,
/// read once and then only where it moved. `--component` and `--query` ask a
/// different question — "which reports are about this?" — and its truthful
/// answer is the same reports every time it is asked. The triage procedure
/// searches by component to match reports against the work a session just did,
/// and those reports are old and unchanged by definition; a difference read
/// returns none of them, and the session concludes there is nothing to triage.
/// So a narrowed read never opts in, never advances the sweep's position, and
/// says `drains: false` — its truncation means narrow further, not list again.
#[test]
fn headless_feedback_answers_a_lookup_instead_of_draining_it() {
    use ds_client_core::feedback::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    for _ in 0..2 {
        transport
            .0
            .lock()
            .unwrap()
            .feedback
            .push_back(project_response(serde_json::json!({
                "reports":[feedback_row("open",1)],
                "count":1,"matched":1,"complete":true,"changed":true,"scanned":1,
                "cursor":"fb1.abc.def","cursor_source":"none"
            })));
    }
    let lookup = client
        .feedback(
            &Command::List {
                view: "not_addressed".into(),
                component: Some("ds-web".into()),
                query: None,
                limit: 20,
                detail: true,
                cursor: None,
                all: false,
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(lookup["drains"], false);
    let sent = transport.0.lock().unwrap().feedback_bodies[0].clone();
    assert_eq!(sent["filter"]["component_contains"], "ds-web");
    assert!(
        sent.get("use_watermark").is_none() && sent.get("cursor").is_none(),
        "a lookup that drains answers `nothing changed` the second time it is asked: {sent}"
    );

    // A caller that genuinely wants to drain a narrowed question still can, by
    // echoing the cursor that read handed back. Saying where you are is always
    // stronger than the service's memory of where you were.
    client
        .feedback(
            &Command::List {
                view: "not_addressed".into(),
                component: Some("ds-web".into()),
                query: None,
                limit: 20,
                detail: false,
                cursor: Some("fb1.abc.def".into()),
                all: false,
            },
            NOW + 2,
        )
        .unwrap();
    let sent = transport.0.lock().unwrap().feedback_bodies[1].clone();
    assert_eq!(sent["cursor"], "fb1.abc.def");
}

/// A complete enumeration has exactly one remaining way to be partial, and it
/// must reach the person reading the backlog.
///
/// A stored document the backlog cannot decode is left out of the answer and
/// PASSED OVER by the watermark, so it is missing from every later difference
/// as well. `scan_incomplete` was removed for admitting partiality; swallowing
/// this count here would reintroduce the same partiality with the admission
/// taken out, which is strictly worse. So the count travels, and zero — the
/// normal answer — stays off the wire rather than becoming noise on every read.
#[test]
fn headless_feedback_carries_a_document_the_backlog_could_not_read() {
    use ds_client_core::feedback::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let request = || Command::List {
        view: "not_addressed".into(),
        component: None,
        query: None,
        limit: 20,
        detail: false,
        cursor: None,
        all: false,
    };
    transport
        .0
        .lock()
        .unwrap()
        .feedback
        .push_back(project_response(serde_json::json!({
            "reports":[feedback_row("open",1)],
            "count":1,"matched":1,"complete":true,"changed":true,"scanned":2,
            "cursor":"fb1.abc.def","cursor_source":"reader","unreadable":1
        })));
    let answer = client.feedback(&request(), NOW + 1).unwrap();
    assert_eq!(
        answer["unreadable"], 1,
        "a document the backlog could not read must not disappear at this layer: {answer}"
    );

    transport
        .0
        .lock()
        .unwrap()
        .feedback
        .push_back(project_response(serde_json::json!({
            "reports":[feedback_row("open",1)],
            "count":1,"matched":1,"complete":true,"changed":true,"scanned":1,
            "cursor":"fb1.abc.def","cursor_source":"reader"
        })));
    let clean = client.feedback(&request(), NOW + 2).unwrap();
    assert!(
        clean.get("unreadable").is_none(),
        "the normal answer says nothing about a failure that did not happen: {clean}"
    );
}

#[test]
fn headless_feedback_note_names_a_blocker_without_moving_the_report() {
    use ds_client_core::feedback::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let mut blocked = feedback_row("open", 3);
    blocked["blocked"] = serde_json::json!(true);
    blocked["blocked_on"] = serde_json::json!("deploy:ds-brain-canary");
    blocked["note_count"] = serde_json::json!(1);
    blocked["latest_note"] = serde_json::json!({
        "author":"operator@example.com","at":"2026-09-18T12:00:00Z",
        "text":"Merged on run; waits on the canary deploy.",
        "blocked_on":"deploy:ds-brain-canary"});
    transport
        .0
        .lock()
        .unwrap()
        .feedback
        .push_back(project_response(serde_json::json!({"report":blocked})));
    let request = Command::Note {
        id: "f1".into(),
        text: "Merged on run; waits on the canary deploy.".into(),
        blocked_on: Some("deploy:ds-brain-canary".into()),
        unblock: false,
        expected_version: Some(2),
    };
    let result = client.feedback(&request, NOW + 1).unwrap();
    assert_eq!(result["blocked"], true);
    assert_eq!(result["blocked_on"], "deploy:ds-brain-canary");
    assert_eq!(result["report"]["status"], "open");
    assert_eq!(
        transport.0.lock().unwrap().feedback_bodies[0],
        serde_json::json!({"action":"note","id":"f1","expected_version":2,
            "note":{"text":"Merged on run; waits on the canary deploy.",
                    "blocked_on":"deploy:ds-brain-canary"}})
    );
    // One note cannot both raise a dependency and clear one.
    assert!(Command::Note {
        id: "f1".into(),
        text: "both".into(),
        blocked_on: Some("x".into()),
        unblock: true,
        expected_version: None,
    }
    .validate()
    .is_err());
}

#[test]
fn headless_feedback_refuses_a_settled_report_by_its_own_name() {
    use ds_client_core::feedback::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    transport
        .0
        .lock()
        .unwrap()
        .feedback
        .push_back(ds_client_core::TransportResponse {
            status: 409,
            body: br#"{"success":false,"error":{"code":"FEEDBACK_SETTLED","message":"settled"}}"#
                .to_vec(),
        });
    let request = Command::Note {
        id: "f1".into(),
        text: "still broken".into(),
        blocked_on: None,
        unblock: false,
        expected_version: None,
    };
    // A settled report and a version conflict both arrive as 409 and want
    // opposite things: one wants a NEW report, the other wants a re-read.
    assert_eq!(
        client.feedback(&request, NOW + 1).unwrap_err().to_string(),
        "feedback_settled"
    );
}

#[test]
fn headless_feedback_close_preserves_the_callers_version_fence() {
    use ds_client_core::feedback::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    {
        let mut state = transport.0.lock().unwrap();
        state.feedback.push_back(project_response(
            serde_json::json!({"report":feedback_row("open",2)}),
        ));
        state.feedback.push_back(TransportResponse {
            status: 409,
            body: Vec::new(),
        });
    }
    let request = Command::Close {
        id: "f1".into(),
        status: "resolved".into(),
        resolution: "Verified in the installed CLI".into(),
        expected_version: Some(1),
    };
    assert_eq!(
        client.feedback(&request, NOW + 1).unwrap_err().to_string(),
        "feedback_conflict"
    );
    let calls = &transport.0.lock().unwrap().feedback_bodies;
    assert_eq!(calls[1]["expected_version"], 1);
    assert!(calls.iter().all(|r| r.get("ds_project").is_none()));
}

#[test]
fn headless_feedback_submit_uses_authenticated_agent_contract_without_project() {
    use ds_client_core::feedback::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    transport
        .0
        .lock()
        .unwrap()
        .feedback
        .push_back(project_response(
            serde_json::json!({"report":feedback_row("open",1),"deduplicated":false}),
        ));
    let request = Command::Submit {
        title: "Headless feedback".into(),
        detail: "Must work without a selected project".into(),
        component: "ds-cli".into(),
        kind: "bug".into(),
        severity: "major".into(),
        agent: "codex".into(),
        model: None,
        client: Some("CLI".into()),
        evidence: vec!["No Desktop is paired".into()],
        context: Default::default(),
    };
    let result = client.feedback(&request, NOW + 1).unwrap();
    assert_eq!(result["report"]["id"], "f1");
    assert_eq!(result["deduplicated"], false);
    let state = transport.0.lock().unwrap();
    let call = &state.feedback_bodies[0];
    assert_eq!(call["action"], "submit");
    assert_eq!(call["report"]["reporter_kind"], "agent");
    assert_eq!(call["report"]["agent"]["name"], "codex");
    assert_eq!(call["report"]["evidence"][0], "No Desktop is paired");
    assert!(call.get("ds_project").is_none());
    assert!(call["report"].get("reporter_email").is_none());
}

#[test]
fn headless_version_comparison_uses_pinned_snapshots_and_has_no_geometry_payload() {
    use ds_client_core::design_versions::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let row = |id: &str| serde_json::json!({"project_id":"project_a","object":{"kind":"lv_transformer","id":"tr"},"version_id":id,"version":id[1..].parse::<u64>().unwrap(),"playback_available":true});
    let snapshot = |id: &str, n: i64| serde_json::json!({"version":row(id),"snapshot":{"schema":"ds.transformer-version-snapshot/v1","layers":{"lv_poles":{"type":"FeatureCollection","features":[{"type":"Feature","id":"P1","geometry":{"type":"Point","coordinates":[30,-2]},"properties":{"height":n}}]}}}});
    transport.push_design_versions(
        200,
        &selection_envelope(serde_json::json!({"versions":[row("v2"),row("v1")]})),
    );
    transport.push_design_versions(200, &selection_envelope(snapshot("v1", 9)));
    transport.push_design_versions(200, &selection_envelope(snapshot("v2", 11)));
    let result = client
        .design_versions(
            "project_a",
            &Command::Compare {
                transformer: "tr".into(),
                from: "v1".into(),
                to: "v2".into(),
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(result["comparison"]["totals"]["attributeOnlyChanged"], 1);
    assert_eq!(result["mutated"], false);
    assert!(result["comparison"]["layers"][0]
        .get("reviewCollections")
        .is_none());
    assert!(result["comparison"]["layers"][0].get("details").is_none());
    assert_ne!(result["from_snapshot_digest"], result["to_snapshot_digest"]);
    transport.push_design_versions(200, &selection_envelope(serde_json::json!({"versions":[]})));
    let missing = client
        .design_versions(
            "project_a",
            &Command::Compare {
                transformer: "tr".into(),
                from: "v1".into(),
                to: "head".into(),
            },
            NOW + 1,
        )
        .unwrap_err();
    assert_eq!(missing.kind(), ds_client_core::ErrorKind::ResourceNotFound);
    let refusal = missing.service_refusal().expect("typed version refusal");
    assert_eq!(refusal.code(), Some("version_not_found"));
    assert!(refusal.message().is_some_and(|message| {
        message.contains("transformer tr")
            && message.contains("published v1")
            && message.contains("latest is v0")
    }));
    let created = serde_json::json!({
        "project_id":"project_a",
        "object":{"kind":"lv_transformer","id":"tr"},
        "version_id":"v1",
        "version":1,
        "playback_available":true,
    });
    transport.push_design_versions(
        200,
        &selection_envelope(serde_json::json!({"version":created})),
    );
    let begun = client
        .design_versions(
            "project_a",
            &Command::Begin {
                transformer: "tr".into(),
                create: ds_command_kernel::design_versions_headless::Create {
                    reason: "before field review".into(),
                    idempotency_key: "version-request-1".into(),
                    ..Default::default()
                },
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(begun["version"]["version_id"], "v1");
    assert_eq!(begun["mutated"], true);
    transport.push_design_versions(
        200,
        &selection_envelope(serde_json::json!({"head":{
            "project_id":"project_a",
            "object":{"kind":"lv_transformer","id":"tr"},
            "source_revision":"2026-09-15T12:00:00.123456Z",
            "source_version":12
        }})),
    );
    transport.push_design_versions(
        200,
        &selection_envelope(serde_json::json!({"version":created})),
    );
    let restored = client
        .design_versions(
            "project_a",
            &Command::Restore {
                transformer: "tr".into(),
                version: "v1".into(),
                reason: "Return to approved survey".into(),
                idempotency_key: "restore-tr-v1-1".into(),
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(restored["version"]["version_id"], "v1");
    assert_eq!(
        restored["prepared_source_revision"],
        "2026-09-15T12:00:00.123456Z"
    );
    assert_eq!(restored["mutated"], true);
    let calls = transport.0.lock().unwrap().calls.clone();
    let restore = calls
        .iter()
        .rev()
        .find(|call| call.starts_with("design_versions "))
        .expect("restore call");
    assert!(restore.contains(r#""action":"restore_version""#));
    assert!(restore.contains(r#""version_id":"v1""#));
    assert!(restore.contains(r#""expected_source_revision":"2026-09-15T12:00:00.123456Z""#));
    transport.push_design_versions(
        200,
        &selection_envelope(serde_json::json!({"versions":[row("v1")]})),
    );
    assert!(client
        .design_versions(
            "another_project",
            &Command::List {
                transformer: "tr".into()
            },
            NOW + 1
        )
        .is_err());
}

#[test]
fn grid_publication_requires_storage_proof_commit_and_exact_readback() {
    use ds_client_core::grid_models::Command;
    use ds_command_kernel::grid_publication::Intent;
    use serde_json::json;
    for scenario in [
        "upload",
        "already_stored",
        "transfer_failure",
        "denied",
        "conflict",
        "foreign_readback",
    ] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(transport.clone(), MemoryStore::default());
        let intent = Intent {
            project: "p-1".into(),
            model_id: "model_one".into(),
            model_kind: "mv_line".into(),
            expected_head_revision_id: "rev-old".into(),
            source_revision_id: "rev:new".into(),
            manifest_model_id: "native-model".into(),
            manifest_model_revision: 1,
            model_schema_version: "1".into(),
            engine_version: "ds-grid-engine/0.1.4".into(),
            display_name: None,
            reason: Some("reviewed".into()),
            ..Default::default()
        };
        let plan = intent.plan("p-1", b"model").unwrap();
        let grant = json!({"artifact":plan.artifact,"already_exists":false,"session_uri":"https://storage.googleapis.com/upload/storage/v1/b/bucket/o?upload_id=private"});
        let stored = json!({"artifact":plan.artifact,"already_exists":true});
        let mut version = plan.version_request()["version"].clone();
        version["project_id"] = json!("p-1");
        version["version"] = json!(2);
        version["parent_revision_id"] = json!("rev-old");
        {
            let mut state = transport.0.lock().unwrap();
            if scenario == "denied" {
                state
                    .grid_models
                    .push_back(TransportResponse::new(403, b"{}"));
            } else {
                state.grid_upload_fail = scenario == "transfer_failure";
                state
                    .grid_models
                    .push_back(project_response(if scenario == "already_stored" {
                        stored.clone()
                    } else {
                        grant
                    }));
                if scenario != "transfer_failure" {
                    if scenario != "already_stored" {
                        state.grid_models.push_back(project_response(stored));
                    }
                    if scenario == "conflict" {
                        state
                            .grid_models
                            .push_back(TransportResponse::new(409, b"{}"));
                    } else {
                        state
                            .grid_models
                            .push_back(project_response(json!({"version":version})));
                        if scenario == "foreign_readback" {
                            version["project_id"] = json!("other-project");
                        }
                        state
                            .grid_models
                            .push_back(project_response(json!({"revision":version})));
                    }
                }
            }
        }
        let result = client.grid_models(
            "p-1",
            &Command::Publish {
                intent: Box::new(intent),
                bytes: b"model".to_vec(),
            },
            NOW + 1,
        );
        assert_eq!(
            result.is_ok(),
            matches!(scenario, "upload" | "already_stored"),
            "{scenario}"
        );
        if scenario == "conflict" {
            let refusal = result
                .as_ref()
                .err()
                .expect("publication refused")
                .service_refusal()
                .unwrap();
            assert_eq!(refusal.status(), 409);
            assert_eq!(refusal.code(), Some("grid_publication_conflict"));
        }
        if let Ok(receipt) = result {
            assert_eq!(receipt.data["verified"], true);
            assert!(!receipt.data.to_string().contains("upload_id"));
            assert_eq!(receipt.data["expected_head"], "rev-old");
        }
        let state = transport.0.lock().unwrap();
        assert!(
            state.grid_models.is_empty(),
            "unexpected unconsumed response: {scenario}"
        );
        assert!(state
            .grid_bodies
            .iter()
            .all(|body| body["project_id"] == "p-1"));
        let commits = state
            .grid_bodies
            .iter()
            .filter(|body| body["action"] == "create_version")
            .count();
        assert_eq!(
            commits,
            usize::from(!matches!(scenario, "transfer_failure" | "denied")),
            "{scenario}"
        );
        assert_eq!(
            state.grid_uploaded.len(),
            usize::from(!matches!(scenario, "denied" | "already_stored"))
        );
    }
}

fn push_photo(transport: &FixtureTransport, status: u16, data: serde_json::Value) {
    transport
        .0
        .lock()
        .unwrap()
        .survey_photo
        .push_back(TransportResponse::new(
            status,
            serde_json::to_vec(&serde_json::json!({"success":status == 200,"data":data})).unwrap(),
        ));
}

#[test]
fn photo_rotation_is_headless_pinned_and_retry_does_not_rotate_twice() {
    use ds_client_core::survey_photo::{Command, Receipt};
    use serde_json::json;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let path = "project_a/forms/poles/entry/photo.png";
    let thumb = "project_a/forms/poles/entry/photo_thunder.jpeg";
    push_photo(
        &transport,
        200,
        json!({"bucket":"test-bucket","blob_path":path,"generation":"7","source_url":format!("https://storage.googleapis.com/test-bucket/{path}?generation=7")}),
    );
    push_photo(&transport, 404, json!({}));
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").unwrap();
    transport
        .0
        .lock()
        .unwrap()
        .photo_downloads
        .push_back(TransportResponse::new(200, png));
    let Receipt::Prepared(bundle) = client
        .survey_photo(
            "project_a",
            Command::Prepare {
                object_path: path,
                degrees: 90,
                expected_generation: Some("7"),
            },
            NOW + 1,
        )
        .unwrap()
    else {
        panic!("expected prepared")
    };
    assert_eq!(bundle.manifest.thumbnail_path, thumb);
    assert!(!serde_json::to_string(&bundle.manifest)
        .unwrap()
        .contains("source_url"));
    // Neither another project nor edited local bytes can consume an upload grant.
    for project in ["project_b", "project_a"] {
        let original = if project == "project_a" {
            &b"changed"[..]
        } else {
            bundle.original.as_slice()
        };
        assert!(client
            .survey_photo(
                project,
                Command::Publish {
                    manifest: &bundle.manifest,
                    original,
                    thumbnail: &bundle.thumbnail
                },
                NOW + 1
            )
            .is_err());
    }
    let mut other_audience = bundle.manifest.clone();
    other_audience.credential_audience_sha256 = "0".repeat(64);
    assert!(client
        .survey_photo(
            "project_a",
            Command::Publish {
                manifest: &other_audience,
                original: &bundle.original,
                thumbnail: &bundle.thumbnail
            },
            NOW + 1
        )
        .is_err());
    assert_eq!(transport.0.lock().unwrap().photo_bodies.len(), 2);
    push_photo(
        &transport,
        200,
        json!({"bucket":"test-bucket","blob_path":path,"session_uri":"https://storage.googleapis.com/upload/storage/v1/b/test-bucket/o?uploadType=resumable&upload_id=a"}),
    );
    transport
        .0
        .lock()
        .unwrap()
        .uploads
        .push_back(TransportResponse::new(200, b"{}".as_slice()));
    push_photo(
        &transport,
        200,
        json!({"bucket":"test-bucket","blob_path":path,"generation":"8","already_present":true}),
    );
    // Fail after the original has published, before the thumbnail gets a grant.
    push_photo(&transport, 503, json!({}));
    assert!(client
        .survey_photo(
            "project_a",
            Command::Publish {
                manifest: &bundle.manifest,
                original: &bundle.original,
                thumbnail: &bundle.thumbnail
            },
            NOW + 1
        )
        .is_err());
    push_photo(
        &transport,
        200,
        json!({"bucket":"test-bucket","blob_path":path,"generation":"8","already_present":true}),
    );
    push_photo(
        &transport,
        200,
        json!({"bucket":"test-bucket","blob_path":thumb,"session_uri":"https://storage.googleapis.com/upload/storage/v1/b/test-bucket/o?uploadType=resumable&upload_id=b"}),
    );
    transport
        .0
        .lock()
        .unwrap()
        .uploads
        .push_back(TransportResponse::new(200, b"{}".as_slice()));
    push_photo(
        &transport,
        200,
        json!({"bucket":"test-bucket","blob_path":thumb,"generation":"9","already_present":true}),
    );
    let Receipt::Published(done) = client
        .survey_photo(
            "project_a",
            Command::Publish {
                manifest: &bundle.manifest,
                original: &bundle.original,
                thumbnail: &bundle.thumbnail,
            },
            NOW + 1,
        )
        .unwrap()
    else {
        panic!("expected published")
    };
    assert_eq!(done["publication"], "published");
    let state = transport.0.lock().unwrap();
    assert_eq!(state.uploaded_bytes, [bundle.original, bundle.thumbnail]);
    for body in state
        .photo_bodies
        .iter()
        .filter(|v| v["file_name"] == "photo_thunder.jpeg" && v["action"] == "survey_upload_start")
    {
        assert_eq!(body["source_generation"], "8");
        assert_eq!(body["source_file_name"], "photo.png");
        assert!(body.get("expected_generation").is_none());
    }
    assert!(state
        .photo_bodies
        .iter()
        .all(|v| v["project_id"] == "project_a"));
}

#[test]
fn solar_portfolio_catalog_rejects_changed_membership_and_wrong_project_before_transport() {
    use ds_client_core::{solar_portfolio, solar_project::Command};
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let membership: serde_json::Value = serde_json::from_str(
        &ds_command_kernel::solar_portfolio_membership_identity(
            br#"{"portfolio_id":"pf_1","cities":["a","b"]}"#,
        )
        .unwrap(),
    )
    .unwrap();
    let row = serde_json::json!({"id":"pf_1","name":"Two cities","cities":["a","b"],"membership_revision":membership["revision"]});
    for (data, ok) in [
        (serde_json::Value::Null, true),
        (serde_json::json!([row.clone()]), true),
        (
            {
                let mut changed = row.clone();
                changed["cities"] = serde_json::json!(["b", "a"]);
                serde_json::json!([changed])
            },
            false,
        ),
    ] {
        t.0.lock()
            .unwrap()
            .solar_project
            .push_back(project_response(data));
        assert_eq!(
            client
                .solar_project(
                    "p-1",
                    &Command::Portfolio(solar_portfolio::Command::List),
                    NOW + 1
                )
                .is_ok(),
            ok
        );
    }
    let before = t.0.lock().unwrap().calls.len();
    assert!(client
        .solar_project(
            "../other",
            &Command::Portfolio(solar_portfolio::Command::List),
            NOW + 1
        )
        .is_err());
    assert!(client.solar_project("p-1",&Command::Portfolio(solar_portfolio::Command::Mutate {intent:serde_json::json!({"operation":"update","portfolio_id":"pf_1","cities":["a","b"]})}),NOW+1).is_err());
    assert_eq!(t.0.lock().unwrap().calls.len(), before);
}

#[test]
fn solar_portfolio_analysis_rechecks_membership_and_preserves_absent_analysis() {
    use ds_client_core::{solar_portfolio, solar_project::Command};
    let membership: serde_json::Value = serde_json::from_str(
        &ds_command_kernel::solar_portfolio_membership_identity(
            br#"{"portfolio_id":"pf_1","cities":["a","b"]}"#,
        )
        .unwrap(),
    )
    .unwrap();
    let row = serde_json::json!({"id":"pf_1","name":"Two cities","cities":["a","b"],"membership_revision":membership["revision"]});
    let changed: serde_json::Value = serde_json::from_str(
        &ds_command_kernel::solar_portfolio_membership_identity(
            br#"{"portfolio_id":"pf_1","cities":["b","a"]}"#,
        )
        .unwrap(),
    )
    .unwrap();
    for changed_membership in [false, true] {
        let t = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(t.clone(), MemoryStore::default());
        let mut after = row.clone();
        if changed_membership {
            after["cities"] = serde_json::json!(["b", "a"]);
            after["membership_revision"] = changed["revision"].clone();
        }
        t.0.lock().unwrap().solar_project.extend([
            project_response(row.clone()),
            TransportResponse::new(404, b""),
            project_response(after),
        ]);
        let result = client.solar_project(
            "p-1",
            &Command::Portfolio(solar_portfolio::Command::Analysis {
                portfolio: "pf_1".into(),
            }),
            NOW + 1,
        );
        if changed_membership {
            assert!(result.is_err());
        } else {
            let result = result.unwrap();
            assert_eq!(result["saved_analysis"], "none");
            assert_eq!(result["city_ids"], serde_json::json!(["a", "b"]));
            assert_eq!(result["project_id"], "p-1");
        }
    }
}

#[test]
fn solar_portfolio_mutation_requires_exact_authoritative_readback() {
    use ds_client_core::{solar_portfolio, solar_project::Command};
    let membership = |cities: serde_json::Value| -> serde_json::Value {
        serde_json::from_str(
            &ds_command_kernel::solar_portfolio_membership_identity(
                &serde_json::to_vec(&serde_json::json!({"portfolio_id":"pf_1","cities":cities}))
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let revision = membership(serde_json::json!(["a", "b"]))["revision"].clone();
    let row = serde_json::json!({"id":"pf_1","name":"Two cities","display_name":"Two cities","cities":["a","b"],"membership_revision":revision});
    for mismatch in [false, true] {
        let t = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(t.clone(), MemoryStore::default());
        let mut readback = row.clone();
        if mismatch {
            readback["cities"] = serde_json::json!(["b", "a"]);
            readback["membership_revision"] =
                membership(readback["cities"].clone())["revision"].clone();
        }
        t.0.lock().unwrap().solar_project.extend([
            project_response(row.clone()),
            project_response(serde_json::json!([readback])),
        ]);
        let output = client.solar_project("p-1",&Command::Portfolio(solar_portfolio::Command::Mutate {
            intent:serde_json::json!({"operation":"update","portfolio_id":"pf_1","expected_membership_revision":revision,"cities":["a","b"]})
        }),NOW+1);
        assert_eq!(output.is_ok(), !mismatch);
        assert!(t.0.lock().unwrap().solar_project.is_empty());
        if let Ok(receipt) = output {
            assert_eq!(receipt["project_id"], "p-1");
            assert_eq!(
                receipt["portfolios"][0]["cities"],
                serde_json::json!(["a", "b"])
            );
        }
    }
}

#[test]
fn governed_solar_media_download_verifies_exact_generation_bytes_and_context() {
    use ds_client_core::solar_project::Command;
    let png = b"\x89PNG\r\n\x1a\nverified-image";
    let reference = "gs://solar-media/eds_project/active-project/eds_solar/city_1/network_plots/_immutable/123e4567-e89b-42d3-a456-426614174001?generation=1";
    for corrupt in [false, true] {
        let t = FixtureTransport::with_sign_in(SIGN_IN);
        let mut envelope =
            solar_snapshot_envelope("eds_project/active-project/eds_solar", "city_1");
        envelope["data"]["media_downloads"] = serde_json::json!([{
            "ref": {"schema":1,"kind":"solar_network_media","media_id":"123e4567-e89b-42d3-a456-426614174001", "logical_role":"network.map:overview", "bucket":"solar-media", "object":"eds_project/active-project/eds_solar/city_1/network_plots/_immutable/123e4567-e89b-42d3-a456-426614174001", "generation":1,"size_bytes":png.len(),"media_type":"image/png","sha256":format!("{:x}",sha2::Sha256::digest(png)),"file_name":"overview.png"},
            "download_url":"https://storage.googleapis.com/solar-media/signed?token=private"
        }]);
        t.push_solar_snapshot(200, &serde_json::to_vec(&envelope).unwrap());
        let body: &[u8] = if corrupt { b"changed-image" } else { png };
        t.0.lock()
            .unwrap()
            .solar_media_download
            .push_back(TransportResponse::new(200, body));
        let mut client = signed_in(t.clone(), MemoryStore::default());
        let result = client.solar_project(
            "active-project",
            &Command::ReadMedia {
                city: "city_1".into(),
                reference: reference.into(),
            },
            NOW + 1,
        );
        if corrupt {
            assert_eq!(result.unwrap_err().kind(), ErrorKind::UnreadableResponse);
        } else {
            let value = result.unwrap();
            assert_eq!(value["reference"], reference);
            assert_eq!(value["byte_count"], png.len());
            assert!(!value.to_string().contains("signed?token"));
            assert!(!value.to_string().contains("download_url"));
        }
        assert!(t
            .calls()
            .iter()
            .any(|s| s.starts_with("solar_media_download")));
    }
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    assert!(client
        .solar_project(
            "another-project",
            &Command::ReadMedia {
                city: "city_1".into(),
                reference: reference.into()
            },
            NOW + 1
        )
        .is_err());
    assert!(!t.calls().iter().any(|s| s.starts_with("solar_")));
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    t.push_solar_snapshot(
        200,
        &serde_json::to_vec(&solar_snapshot_envelope(
            "eds_project/active-project/eds_solar",
            "city_1",
        ))
        .unwrap(),
    );
    let mut client = signed_in(t.clone(), MemoryStore::default());
    assert_eq!(
        client
            .solar_project(
                "active-project",
                &Command::ReadMedia {
                    city: "city_1".into(),
                    reference: reference.into()
                },
                NOW + 1
            )
            .unwrap_err()
            .kind(),
        ErrorKind::ResourceNotFound
    );
    assert!(!t
        .calls()
        .iter()
        .any(|s| s.starts_with("solar_media_download")));
}

#[test]
fn published_portfolio_reads_fence_exact_bytes_head_and_membership_before_domain_admission() {
    use ds_client_core::{solar_portfolio, solar_project::Command};
    let revision: serde_json::Value = serde_json::from_str(
        &ds_command_kernel::solar_portfolio_membership_identity(
            br#"{"portfolio_id":"pf_1","cities":["a","b"]}"#,
        )
        .unwrap(),
    )
    .unwrap();
    let row = serde_json::json!({"id":"pf_1","name":"Two cities","cities":["a","b"],"membership_revision":revision["revision"]});
    // Deliberately not a portfolio: successful transport must proceed to the
    // separate Rust domain refusal; no transport defect may borrow that admission.
    let bytes = b"{}";
    for read_command in ["analysis", "published_read"] {
        for defect in [
            "none",
            "same_length_corrupt",
            "moved_head",
            "stale_membership",
            "wrong_host",
            "unpinned",
            "expired",
        ] {
            let t = FixtureTransport::with_sign_in(SIGN_IN);
            let mut client = signed_in(t.clone(), MemoryStore::default());
            let output = serde_json::json!({"output_id":"portfolio-result","format":"json","content_type":"application/json","size_bytes":bytes.len(),"sha256":format!("{:x}",sha2::Sha256::digest(bytes))});
            let head = serde_json::json!({"engine":"solar","operation":"portfolio-pf_1","variant":"aggregate-v3","input_base_fingerprint":revision["revision"].as_str().unwrap().trim_start_matches("sha256:"),"head_revision":1,"work_id":"work-1","outputs":[output.clone()]});
            let mut download = serde_json::json!({"head":head,"output":output,"download_url":"https://storage.googleapis.com/solar-results/result?generation=17&token=private","expires_at":"2099-01-01T00:00:00Z"});
            match defect {
                "stale_membership" => {
                    download["head"]["input_base_fingerprint"] = serde_json::json!("e".repeat(64))
                }
                "wrong_host" => {
                    download["download_url"] =
                        serde_json::json!("https://caller.example/result?generation=17")
                }
                "unpinned" => {
                    download["download_url"] = serde_json::json!(
                        "https://storage.googleapis.com/solar-results/result?token=private"
                    )
                }
                "expired" => download["expires_at"] = serde_json::json!("2000-01-01T00:00:00Z"),
                _ => (),
            }
            let mut after = head.clone();
            if defect == "moved_head" {
                after["head_revision"] = serde_json::json!(2);
            }
            {
                let mut state = t.0.lock().unwrap();
                state.solar_project.extend([
                    project_response(row.clone()),
                    project_response(download),
                    project_response(after),
                ]);
                state
                    .solar_artifact_download
                    .push_back(TransportResponse::new(
                        200,
                        if defect == "same_length_corrupt" {
                            b"[]"
                        } else {
                            bytes
                        },
                    ));
            }
            let error = client
                .solar_project(
                    "p-1",
                    &Command::Portfolio(if read_command == "analysis" {
                        solar_portfolio::Command::Analysis {
                            portfolio: "pf_1".into(),
                        }
                    } else {
                        solar_portfolio::Command::PublishedRead {
                            portfolio: "pf_1".into(),
                            run_id: "selected-run".into(),
                        }
                    }),
                    NOW + 1,
                )
                .unwrap_err();
            assert_eq!(
                error.kind(),
                if defect == "none" {
                    ErrorKind::InvalidInput
                } else {
                    ErrorKind::UnreadableResponse
                },
                "{defect}"
            );
            assert!(!error.to_string().contains("token=private"));
            if matches!(
                defect,
                "wrong_host" | "unpinned" | "expired" | "stale_membership"
            ) {
                assert!(
                    !t.calls()
                        .iter()
                        .any(|s| s.starts_with("solar_artifact_download")),
                    "{defect}"
                );
            }
        }
    }
}

#[test]
fn published_portfolio_reads_require_selected_run_and_unchanged_ordered_membership() {
    use ds_client_core::{solar_portfolio, solar_project::Command};
    use serde_json::json;
    let digest = format!("sha256:{}", "a".repeat(64));
    let revision: serde_json::Value = serde_json::from_str(
        &ds_command_kernel::solar_portfolio_membership_identity(
            br#"{"portfolio_id":"pf_1","cities":["a","b"]}"#,
        )
        .unwrap(),
    )
    .unwrap();
    let cities: Vec<_> = ["a", "b"].into_iter().map(|city| json!({
        "city_id":city,"display_name":city,"city_run_id":format!("city-{city}"),
        "input_digest":digest,"result_digest":digest,"contribution_digest":digest,
        "calculation_projection_digest":digest,"selected_system":"solar_battery",
        "inverter_count":1,"pv_capacity_mw":1.,"battery_capacity_mwh":1.,
        "generator_count":0.,"diesel_capacity_mw":0.,"total_customers":10,"annual_energy_kwh":1000.
    })).collect();
    let mut sections = serde_json::Map::new();
    for section in [
        "assumptions",
        "site",
        "maps",
        "network",
        "demand",
        "scenarios",
        "performance",
        "investment",
        "financial_kpis",
        "environmental",
        "risk",
        "system_rationale",
    ] {
        sections.insert(section.into(), json!({"available":true}));
    }
    sections.insert(
        "representative_graphs".into(),
        json!({"_strategy":"first",
        "_representative_cities":["a"],"graph_count":1,
        "graphs":[{"graph_key":"load","city_id":"a","url":{"city_id":"a"}}],"_unavailable":[]}),
    );
    let mut document = json!({"schema_version":"ds-solar.portfolio-result/v3",
        "engine":{"name":"ds-solar-engine","version":"0.1.0","build_manifest_sha256":"a".repeat(64)},
        "project_id":"p-1","root":"eds_project/p-1/eds_solar","portfolio_id":"pf_1",
        "portfolio_name":"Two cities","membership_revision":revision["revision"],
        "run_id":"selected-run","input_digest":digest,"representative_city_id":"a",
        "assumptions":{"currency":"XAF","project_years":25,"discount_rate":0.08},
        "cities":cities,"city_count":2,"total_customers":20,"annual_energy_kwh":2000.,
        "systems":{"solar_battery":{"cities":2,"inverter_count":2,"pv_capacity_mw":2.,
            "battery_capacity_mwh":2.,"generator_count":0.,"diesel_capacity_mw":0.}},
        "sections":sections,"portfolio_digest":digest});
    let typed: ds_solar_contracts::SolarPortfolioResult =
        serde_json::from_value(document.clone()).unwrap();
    document["portfolio_digest"] = json!(typed.compute_digest().unwrap());
    let bytes = serde_json::to_vec(&document).unwrap();
    let row = json!({"id":"pf_1","name":"Two cities","cities":["a","b"],"membership_revision":revision["revision"]});
    let output = json!({"output_id":"portfolio-result","format":"json","content_type":"application/json",
        "size_bytes":bytes.len(),"sha256":format!("{:x}",sha2::Sha256::digest(&bytes))});
    let head = json!({"engine":"solar","engine_version":"ds-solar-engine@0.1.0",
        "engine_build_manifest_sha256":"a".repeat(64),"operation":"portfolio-pf_1","variant":"aggregate-v3",
        "input_base_fingerprint":revision["revision"].as_str().unwrap().trim_start_matches("sha256:"),
        "head_revision":1,"work_id":"work-1","outputs":[output.clone()]});
    let download = json!({"head":head,"output":output,
        "download_url":"https://storage.googleapis.com/solar-results/result?generation=17&token=private",
        "expires_at":"2099-01-01T00:00:00Z"});
    for defect in ["none", "different_run", "changed_membership"] {
        let t = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(t.clone(), MemoryStore::default());
        let mut after = row.clone();
        if defect == "changed_membership" {
            after["cities"] = json!(["b", "a"]);
        }
        {
            let mut state = t.0.lock().unwrap();
            state.solar_project.extend([
                project_response(row.clone()),
                project_response(download.clone()),
                project_response(head.clone()),
                project_response(after),
            ]);
            state
                .solar_artifact_download
                .push_back(TransportResponse::new(200, bytes.as_slice()));
        }
        let answer = client.solar_project(
            "p-1",
            &Command::Portfolio(solar_portfolio::Command::PublishedRead {
                portfolio: "pf_1".into(),
                run_id: if defect == "different_run" {
                    "other-run"
                } else {
                    "selected-run"
                }
                .into(),
            }),
            NOW + 1,
        );
        if defect == "none" {
            let receipt = answer.unwrap();
            assert_eq!(receipt["document"], document);
            assert_eq!(receipt["trace"]["portfolio_run_id"], "selected-run");
            assert_eq!(receipt["publication"]["head_revision"], 1);
            assert!(!receipt.to_string().contains("token=private"));
        } else {
            assert_eq!(
                answer.unwrap_err().kind(),
                ErrorKind::InvalidInput,
                "{defect}"
            );
        }
    }
}

#[test]
fn generic_mv_history_uses_explicit_interleaved_projects_and_rejects_lv_restore() {
    use ds_client_core::design_versions::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let request = || Command::Object {
        kind: "mv_model".into(),
        command: Box::new(Command::List {
            transformer: "line".into(),
        }),
    };
    for project in ["project_a", "project_b"] {
        transport.push_design_versions(200,&selection_envelope(json!({"versions":[{"project_id":project,"object":{"kind":"mv_model","id":"line"},"version_id":"v2","version":2,"source_revision":"rev_native","pinned_model_digest":"abc"}]})));
        let result = client
            .design_versions(project, &request(), NOW + 1)
            .unwrap();
        assert_eq!(result["project"], project);
        assert_eq!(result["versions"][0]["source_revision"], "rev_native");
    }
    transport.push_design_versions(200,&selection_envelope(json!({"versions":[{"project_id":"project_a","object":{"kind":"mv_model","id":"line"},"version_id":"v2","version":2}]})));
    assert_eq!(
        client
            .design_versions("project_b", &request(), NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
    let restore = Command::Object {
        kind: "mv_model".into(),
        command: Box::new(Command::Restore {
            transformer: "line".into(),
            version: "v1".into(),
            reason: "review".into(),
            idempotency_key: "restore".into(),
        }),
    };
    assert_eq!(
        client
            .design_versions("project_a", &restore, NOW + 1)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    let calls = &transport.0.lock().unwrap().calls;
    assert!(calls
        .iter()
        .any(|call| call.contains("\"project_id\":\"project_a\"")
            && call.contains("\"kind\":\"mv_model\"")));
    assert!(calls
        .iter()
        .any(|call| call.contains("\"project_id\":\"project_b\"")));
    assert!(!calls.iter().any(|call| call.contains("restore_version")));
}

#[test]
fn native_attachment_publication_preserves_bytes_and_one_original_pointer_fence() {
    use ds_client_core::design_attachments::{Command, Object};
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let object = Object {
        kind: "mv_model".into(),
        id: "line".into(),
        version: Some("rev_native".into()),
        version_digest: None,
    };
    // The MV pin names a content revision; its model digest is read from the
    // catalogue before anything is uploaded, and never fetched as bytes.
    let model_digest = "c".repeat(64);
    push_grid_revision(&transport, "line", "rev_native", &model_digest);
    let head = json!({"project_id":"project_a","attachment_id":"file_one","object":{"kind":"mv_model","id":"line"},"version":4,"state":"ready"});
    transport.push_design_attachments(200, &selection_envelope(json!({"attachment":head})));
    let bytes = b"opaque native backup".to_vec();
    let digest = format!("{:x}", sha2::Sha256::digest(&bytes));
    let key = serde_json::to_vec(&(
        "project_a",
        json!({"kind":"mv_model","id":"line","version_id":"rev_native"}),
        "model.bak",
        &digest,
    ))
    .unwrap();
    let revision = format!("rev_{:x}", sha2::Sha256::digest(key));
    transport.push_design_attachments(200,&selection_envelope(json!({"attachment_id":"file_one","revision_id":revision,"session_uri":"https://storage.googleapis.com/upload/storage/v1/b/bucket/o?upload_id=issued","blob_path":format!("design-attachments/v1/projects/project_a/mv_model/line/file_one/{revision}/model.bak")})));
    transport.push_design_attachments(409,&serde_json::to_vec(&json!({"success":false,"error":{"code":"design_collab_conflict","message":"pointer changed"}})).unwrap());
    let command = Command::Publish {
        object,
        attachment: Some("file_one".into()),
        file: "model.bak".into(),
        label: None,
        purpose: None,
        media_type: None,
        source_kind: None,
        source_reference: None,
        make_latest: true,
        bytes,
    };
    let refused = client
        .design_attachments("project_a", &command, NOW + 1)
        .unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::InvalidInput);
    let state = transport.0.lock().unwrap();
    assert_eq!(
        state.grid_bodies,
        vec![
            json!({"action":"get_version","project_id":"project_a","model_id":"line","revision_id":"rev_native"})
        ]
    );
    let calls = &state.calls;
    assert!(calls
        .iter()
        .filter(|call| call.contains("start_upload") || call.contains("finalize_upload"))
        .all(|call| call.contains(&format!("\"version_digest\":\"{model_digest}\""))));
    // The pointer choice is finalize's alone and the default moves it, so the
    // wire is what it was before the choice existed.
    assert!(!calls.iter().any(|call| call.contains("make_latest")));
    assert!(calls
        .iter()
        .any(|call| call.contains("attachment_upload bytes=20")));
    let finalize: Vec<_> = calls
        .iter()
        .filter(|call| call.contains("finalize_upload"))
        .collect();
    assert_eq!(finalize.len(), 1);
    assert!(finalize[0].contains("\"expected_version\":4"));
    assert!(finalize[0].contains("\"version_id\":\"rev_native\""));
    assert!(finalize[0].contains(&digest));
}

/// A catalog revision row as `list_versions` / `get_version` return it.
fn grid_revision(id: &str, version: u64, parent: &str, digest: &str) -> serde_json::Value {
    serde_json::json!({"project_id":"p-1","model_id":"m","revision_id":id,"version":version,
        "parent_revision_id":parent,"model":{"digest":digest,
        "object":format!("ds-grid/objects/sha256/{}/{}", &digest[..2], digest),"byte_length":5},
        "reason":"save","milestone":"","approval":{"status":"draft"},"created_by":"a@b.c"})
}

fn grid_named_head(revision: &str) -> TransportResponse {
    project_response(
        serde_json::json!({"head": {"project_id":"p-1","object":{"kind":"mv_model","id":"m"},
        "source_revision":revision,"pinned_model_digest":"a".repeat(64),"published_version":0}}),
    )
}

#[test]
fn grid_show_reports_the_current_version_its_revision_count_and_the_head_ordinal() {
    use ds_client_core::grid_models::Command;
    use serde_json::json;
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let digest = "a".repeat(64);
    let head = json!({"project_id":"p-1","model_id":"m","display_name":"Kamonyi MV","head_revision_id":"rev-c",
        "head_version":2,"head_model_digest":digest,"approval_status":"submitted","state":"active",
        "updated_at":"2026-09-25T10:00:00Z","backup":{"bucket":"private","object":"private"}});
    {
        let mut state = t.0.lock().unwrap();
        state.design_versions.push_back(grid_named_head("rev-c"));
        state
            .grid_models
            .push_back(project_response(json!({"model":head})));
        // Page 1 holds two v2 saves in hash order; page 2 finishes v2 and
        // reaches v1, so the walk is complete after two pages.
        state.grid_models.push_back(project_response(json!({"versions":[
            grid_revision("rev-c", 2, "rev-b", &digest), grid_revision("rev-a", 2, "rev-z", &digest)],
            "has_more":true,"next_cursor":"page-2"})));
        state
            .grid_models
            .push_back(project_response(json!({"versions":[
            grid_revision("rev-b", 2, "rev-a", &digest), grid_revision("rev-z", 1, "", &digest)],
            "has_more":false,"next_cursor":""})));
    }
    let shown = client
        .grid_models("p-1", &Command::Show { model: "m".into() }, NOW + 1)
        .unwrap();
    assert_eq!(shown.data["management"]["named_version"]["version"], 0);
    assert_eq!(
        shown.data["management"]["working_head"]["revision_id"],
        "rev-c"
    );
    assert_eq!(shown.data["management"]["mutated"], false);
    assert_eq!(shown.data["model"]["head_version"], 2);
    assert_eq!(shown.data["model"]["approval_status"], "submitted");
    assert!(shown.data["model"].get("backup").is_none());
    assert_eq!(shown.data["current_version"]["version"], 2);
    assert_eq!(shown.data["current_version"]["version_revision_count"], 3);
    assert_eq!(
        shown.data["current_version"]["revision_ordinal_within_version"],
        3
    );
    assert_eq!(shown.data["current_version"]["count_exact"], true);
    let state = t.0.lock().unwrap();
    assert_eq!(state.grid_bodies[0]["action"], "get_model");
    assert_eq!(state.grid_bodies[2]["cursor"], "page-2");
    assert!(state.grid_models.is_empty());
}

#[test]
fn grid_show_refuses_a_moved_or_foreign_named_head_without_writing() {
    use ds_client_core::grid_models::Command;
    for foreign in [false, true] {
        let t = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(t.clone(), MemoryStore::default());
        let head = serde_json::json!({"project_id":"p-1","model_id":"m","head_revision_id":"rev-c",
            "head_version":3,"head_version_revision_count":1,"head_model_digest":"a".repeat(64)});
        let named = serde_json::json!({"head":{"project_id":if foreign {"other"} else {"p-1"},
            "object":{"kind":"mv_model","id":"m"},"source_revision":if foreign {"rev-c"} else {"moved"},
            "pinned_model_digest":"a".repeat(64),"published_version":1}});
        {
            let mut state = t.0.lock().unwrap();
            state
                .grid_models
                .push_back(project_response(serde_json::json!({"model":head})));
            state.design_versions.push_back(project_response(named));
        }
        assert_eq!(
            client
                .grid_models("p-1", &Command::Show { model: "m".into() }, NOW + 1)
                .err()
                .expect("a moved or foreign head must be refused")
                .kind(),
            ErrorKind::UnreadableResponse
        );
        let state = t.0.lock().unwrap();
        assert_eq!(state.grid_bodies.len(), 1);
        assert!(state
            .calls
            .iter()
            .filter(|c| c.starts_with("design_versions "))
            .all(|c| c.contains("get_head") && !c.contains("create_version")));
    }
}

#[test]
fn grid_show_version_reads_metadata_without_fetching_bytes_or_leaking_the_locator() {
    use ds_client_core::grid_models::Command;
    use serde_json::json;
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let mut revision = grid_revision("rev-c", 2, "rev-b", &"a".repeat(64));
    revision["migration_source"] = json!({"kind":"pls_cadd_bak","reference":"k.bak"});
    t.0.lock().unwrap().grid_models.push_back(project_response(json!({"revision":revision,
        "download_url":"https://storage.googleapis.com/b/o?X-Goog-Signature=secret","expires_at":"x"})));
    let shown = client
        .grid_models(
            "p-1",
            &Command::ShowVersion {
                model: "m".into(),
                revision: "rev-c".into(),
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(shown.data["revision"]["parent_revision_id"], "rev-b");
    assert_eq!(
        shown.data["revision"]["migration_source"]["kind"],
        "pls_cadd_bak"
    );
    assert_eq!(shown.data["bytes_fetched"], false);
    assert!(shown.bytes.is_none());
    assert!(!shown.data.to_string().contains("Signature"));
    // A revision of another model is refused, not shown.
    let mut foreign = grid_revision("rev-c", 2, "rev-b", &"a".repeat(64));
    foreign["model_id"] = json!("other");
    t.0.lock()
        .unwrap()
        .grid_models
        .push_back(project_response(json!({"revision":foreign})));
    assert!(client
        .grid_models(
            "p-1",
            &Command::ShowVersion {
                model: "m".into(),
                revision: "rev-c".into()
            },
            NOW + 1
        )
        .is_err());
}

#[test]
fn grid_versions_group_revisions_by_version_in_save_order() {
    use ds_client_core::grid_models::Command;
    use serde_json::json;
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let digest = "b".repeat(64);
    t.0.lock()
        .unwrap()
        .grid_models
        .push_back(project_response(json!({"versions":[
        grid_revision("rev-e", 2, "rev-d", &digest), grid_revision("rev-d", 2, "rev-c", &digest),
        grid_revision("rev-b", 1, "rev-a", &digest), grid_revision("rev-c", 1, "rev-b", &digest),
        grid_revision("rev-a", 1, "", &digest)],"has_more":false,"next_cursor":""})));
    let listed = client
        .grid_models(
            "p-1",
            &Command::ListVersions {
                model: "m".into(),
                limit: 50,
                cursor: None,
            },
            NOW + 1,
        )
        .unwrap();
    let versions = listed.data["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0]["version"], 2);
    assert_eq!(versions[1]["revisions"][0]["revision_id"], "rev-a");
    assert_eq!(versions[1]["revisions"][2]["revision_id"], "rev-c");
    assert_eq!(
        versions[1]["revisions"][2]["revision_ordinal_within_version"],
        3
    );
    assert_eq!(listed.data["revisions_listed"], 5);
}

#[test]
fn grid_publication_bumps_only_when_asked_and_records_governance() {
    use ds_client_core::grid_models::Command;
    use ds_command_kernel::grid_publication::{Approval, Intent};
    use serde_json::json;
    for (bump, old_server) in [(false, false), (true, false), (true, true)] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(transport.clone(), MemoryStore::default());
        let intent = Intent {
            project: "p-1".into(),
            model_id: "model_one".into(),
            model_kind: "mv_line".into(),
            expected_head_revision_id: "rev-old".into(),
            source_revision_id: "rev:new".into(),
            manifest_model_id: "native-model".into(),
            manifest_model_revision: 2,
            model_schema_version: "1".into(),
            engine_version: "e".into(),
            bump_version: bump,
            milestone: Some("Submission 2".into()),
            approval: Some(Approval {
                status: "submitted".into(),
                level_id: None,
                decision_reason: None,
            }),
            ..Default::default()
        };
        let plan = intent.plan("p-1", b"model").unwrap();
        let stored = json!({"artifact":plan.artifact,"already_exists":true});
        let mut version = plan.version_request()["version"].clone();
        version["project_id"] = json!("p-1");
        version["version"] = json!(if bump { 3 } else { 2 });
        version["parent_revision_id"] = json!("rev-old");
        {
            let mut state = transport.0.lock().unwrap();
            state.grid_models.push_back(project_response(stored));
            if old_server {
                let refusal = serde_json::to_vec(&json!({"success":false,
                    "error":{"code":"VALIDATION_FAILED","message":"Invalid DS Grid model request"}}))
                .unwrap();
                state
                    .grid_models
                    .push_back(TransportResponse::new(400, refusal));
            } else {
                state
                    .grid_models
                    .push_back(project_response(json!({"version":version})));
                state
                    .grid_models
                    .push_back(project_response(json!({"revision":version})));
            }
        }
        let result = client.grid_models(
            "p-1",
            &Command::Publish {
                intent: Box::new(intent),
                bytes: b"model".to_vec(),
            },
            NOW + 1,
        );
        let state = transport.0.lock().unwrap();
        let commit = state
            .grid_bodies
            .iter()
            .find(|b| b["action"] == "create_version")
            .unwrap();
        assert_eq!(commit["version"]["milestone"], "Submission 2");
        assert_eq!(commit["version"]["approval"]["status"], "submitted");
        assert_eq!(commit["version"].get("bump_version").is_some(), bump);
        if old_server {
            let refusal = result.err().unwrap();
            assert_eq!(
                refusal.service_refusal().unwrap().code(),
                Some("server_action_unsupported")
            );
        } else {
            let receipt = result.unwrap();
            assert_eq!(receipt.data["bump_version"], bump);
            assert_eq!(receipt.data["milestone"], "Submission 2");
        }
    }
}

#[test]
fn grid_exports_publish_pins_the_revision_digest_and_download_verifies_bytes() {
    use ds_client_core::grid_models::Command;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let model_digest = "c".repeat(64);
    let bak = b"bak!!".to_vec();
    let digest = format!("{:x}", Sha256::digest(&bak));
    let object = format!("ds-grid/objects/sha256/{}/{}", &digest[..2], digest);
    let artifact = json!({"digest":digest,"object":object,"byte_length":5});
    let item = json!({"output_id":"pls-delivered-workspace","scope":"model","format":"pls_cadd_bak","artifact":artifact});
    let export = json!({"project_id":"p-1","model_id":"m","revision_id":"rev-c","model_digest":model_digest,
        "export_id":"submission-2","items":[item],"created_at":"t","created_by":"a@b.c"});
    {
        let mut state = t.0.lock().unwrap();
        state.grid_models.push_back(project_response(
            json!({"revision":grid_revision("rev-c", 2, "rev-b", &model_digest)}),
        ));
        state.grid_models.push_back(project_response(json!({"artifact":artifact,"already_exists":false,
            "session_uri":"https://storage.googleapis.com/upload/storage/v1/b/bucket/o?upload_id=private"})));
        state.grid_models.push_back(project_response(
            json!({"artifact":artifact,"already_exists":true}),
        ));
        state
            .grid_models
            .push_back(project_response(json!({"export":export})));
    }
    let published = client
        .grid_models(
            "p-1",
            &Command::PublishExport {
                model: "m".into(),
                revision: "rev-c".into(),
                export: "submission-2".into(),
                output: "pls-delivered-workspace".into(),
                format: "pls_cadd_bak".into(),
                bytes: bak.clone(),
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(published.data["model_digest"], model_digest);
    assert_eq!(published.data["head_moved"], false);
    assert!(!published.data.to_string().contains("upload_id"));
    {
        let state = t.0.lock().unwrap();
        assert_eq!(state.grid_bodies[1]["upload"]["purpose"], "export");
        assert_eq!(state.grid_bodies[3]["action"], "publish_export");
        assert_eq!(state.grid_bodies[3]["export"]["model_digest"], model_digest);
        assert_eq!(state.grid_uploaded, vec![bak.clone()]);
    }
    // Download: list, sign, fetch, verify. Wrong bytes are refused.
    for (bytes, ok) in [(b"bak!!".as_slice(), true), (b"evil!".as_slice(), false)] {
        {
            let mut state = t.0.lock().unwrap();
            state
                .grid_models
                .push_back(project_response(json!({"exports":[export]})));
            state.grid_models.push_back(project_response(json!({"download_url":
                format!("https://storage.googleapis.com/bucket/{object}?X-Goog-Signature=s"),"artifact":artifact})));
            state
                .grid_bytes
                .push_back(TransportResponse::new(200, bytes));
        }
        let read = client.grid_models(
            "p-1",
            &Command::DownloadExport {
                model: "m".into(),
                revision: "rev-c".into(),
                export: "submission-2".into(),
                output: "pls-delivered-workspace".into(),
            },
            NOW + 1,
        );
        assert_eq!(read.is_ok(), ok);
        if let Ok(read) = read {
            assert_eq!(read.bytes.as_deref(), Some(bak.as_slice()));
            assert_eq!(read.data["format"], "pls_cadd_bak");
            assert!(!read.data.to_string().contains("Signature"));
        }
    }
    // A Server that predates the signed read says so by name.
    {
        let mut state = t.0.lock().unwrap();
        state
            .grid_models
            .push_back(project_response(json!({"exports":[export]})));
        let refusal = serde_json::to_vec(&json!({"success":false,
            "error":{"code":"VALIDATION_FAILED","message":"Invalid DS Grid model request"}}))
        .unwrap();
        state
            .grid_models
            .push_back(TransportResponse::new(400, refusal));
    }
    let old = client
        .grid_models(
            "p-1",
            &Command::DownloadExport {
                model: "m".into(),
                revision: "rev-c".into(),
                export: "submission-2".into(),
                output: "pls-delivered-workspace".into(),
            },
            NOW + 1,
        )
        .err()
        .expect("an old Server refuses");
    assert_eq!(
        old.service_refusal().unwrap().code(),
        Some("server_action_unsupported")
    );
    // An output the record does not hold is refused before any signing.
    t.0.lock()
        .unwrap()
        .grid_models
        .push_back(project_response(json!({"exports":[export]})));
    let missing = client
        .grid_models(
            "p-1",
            &Command::DownloadExport {
                model: "m".into(),
                revision: "rev-c".into(),
                export: "submission-2".into(),
                output: "gis-layer".into(),
            },
            NOW + 1,
        )
        .err()
        .expect("a missing output is refused");
    assert_eq!(
        missing.service_refusal().unwrap().code(),
        Some("grid_model_not_found")
    );
    assert!(t.0.lock().unwrap().grid_models.is_empty());
}

#[test]
fn mv_design_versions_send_milestone_fence_batch_and_summaries() {
    use ds_client_core::design_versions::Command;
    use ds_command_kernel::design_versions_headless::{BatchItem, Create, ObjectRef, Shared};
    use serde_json::json;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let record = |id: &str, n: u64| {
        json!({"project_id":"project_a","object":{"kind":"mv_model","id":"model-a"},"version_id":id,
            "version":n,"milestone":"Submission 2","source_revision":"rev-abc","content_pinned":true})
    };
    transport.push_design_versions(200, &selection_envelope(json!({"version":record("v2",2)})));
    let begun = client
        .design_versions(
            "project_a",
            &Command::Object {
                kind: "mv_model".into(),
                command: Box::new(Command::Begin {
                    transformer: "model-a".into(),
                    create: Create {
                        reason: "Submitted to REG".into(),
                        milestone: Some("Submission 2".into()),
                        expected_source: Some("rev-abc".into()),
                        idempotency_key: "submission-2".into(),
                    },
                }),
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(begun["content_revision"], "rev-abc");
    transport.push_design_versions(
        200,
        &selection_envelope(json!({"results":[
            {"object":{"kind":"mv_model","id":"model-a"},"ok":true,"version":record("v3",3)}]})),
    );
    let batch = client
        .design_versions(
            "project_a",
            &Command::BeginBatch {
                items: vec![BatchItem {
                    object: ObjectRef {
                        kind: "mv_model".into(),
                        id: "model-a".into(),
                    },
                    reason: None,
                    milestone: None,
                    expected_source_revision: Some("-".into()),
                    idempotency_key: "k-1".into(),
                }],
                shared: Shared {
                    reason: Some("Submission 3".into()),
                    milestone: None,
                },
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(batch["ok_count"], 1);
    transport.push_design_versions(
        200,
        &selection_envelope(json!({"summaries":[
            {"object":{"kind":"mv_model","id":"model-a"},"count":3,"truncated":false,"latest_version_id":"v3"}]})),
    );
    let summaries = client
        .design_versions(
            "project_a",
            &Command::Summaries {
                objects: vec![ObjectRef {
                    kind: "mv_model".into(),
                    id: "model-a".into(),
                }],
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(summaries["summaries"][0]["count"], 3);
    let calls = transport.calls();
    let bodies: Vec<&String> = calls
        .iter()
        .filter(|c| c.starts_with("design_versions "))
        .collect();
    assert!(bodies[0].contains(r#""milestone":"Submission 2""#));
    assert!(bodies[0].contains(r#""expected_source_revision":"rev-abc""#));
    assert!(bodies[1].contains(r#""action":"create_versions""#));
    assert!(bodies[1].contains(r#""version":{"reason":"Submission 3"}"#));
    assert!(bodies[2].contains(r#""action":"list_version_summaries""#));
    assert!(bodies[2].contains(r#""objects":[{"kind":"mv_model","id":"model-a"}]"#));
}

#[test]
fn grid_show_uses_the_servers_stored_count_without_walking() {
    use ds_client_core::grid_models::Command;
    use serde_json::json;
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    t.0.lock()
        .unwrap()
        .grid_models
        .push_back(project_response(json!({"model":{
        "project_id":"p-1","model_id":"m","head_revision_id":"rev-c","head_version":3,
        "head_version_revision_count":42,"head_model_digest":"a".repeat(64)}})));
    t.0.lock()
        .unwrap()
        .design_versions
        .push_back(grid_named_head("rev-c"));
    let shown = client
        .grid_models("p-1", &Command::Show { model: "m".into() }, NOW + 1)
        .unwrap();
    assert_eq!(shown.data["current_version"]["version_revision_count"], 42);
    assert_eq!(
        shown.data["current_version"]["revision_ordinal_within_version"],
        42
    );
    assert_eq!(shown.data["current_version"]["pages_read"], 0);
    assert_eq!(t.0.lock().unwrap().grid_bodies.len(), 1, "no catalog walk");
}

#[test]
fn grid_governance_bumps_renames_decides_and_reads_the_backup() {
    use ds_client_core::grid_models::Command;
    use ds_command_kernel::grid_publication::Approval;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    let t = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(t.clone(), MemoryStore::default());
    let digest = format!("{:x}", Sha256::digest(b"model"));
    let bump = Command::BumpVersion {
        model: "m".into(),
        expected_head: "rev-c".into(),
        reason: "Submitted to REG".into(),
        milestone: Some("Submission 2".into()),
    };
    let mut bumped = grid_revision("bump-0123", 3, "rev-c", &digest);
    bumped["revision_ordinal_within_version"] = json!(1);
    bumped["milestone"] = json!("Submission 2");
    t.0.lock()
        .unwrap()
        .grid_models
        .push_back(project_response(json!({"version":bumped})));
    let started = client.grid_models("p-1", &bump, NOW + 1).unwrap();
    assert_eq!(started.data["version"], 3);
    assert_eq!(started.data["content_changed"], false);
    {
        let state = t.0.lock().unwrap();
        let body = &state.grid_bodies[0];
        assert_eq!(body["action"], "bump_version");
        assert_eq!(body["expected_head_revision_id"], "rev-c");
        assert_eq!(body["milestone"], "Submission 2");
        assert!(body.get("version").is_none(), "flat fields only");
    }
    // A version cut from another head is not this bump.
    let mut foreign = bumped.clone();
    foreign["parent_revision_id"] = json!("rev-other");
    t.0.lock()
        .unwrap()
        .grid_models
        .push_back(project_response(json!({"version":foreign})));
    assert!(client.grid_models("p-1", &bump, NOW + 1).is_err());

    let rename = Command::UpdateModel {
        model: "m".into(),
        expected_head: "rev-c".into(),
        display_name: Some("Kamonyi MV (as built)".into()),
        description: None,
        reason: Some("Customer name".into()),
    };
    t.0.lock()
        .unwrap()
        .grid_models
        .push_back(project_response(json!({"model":{
        "project_id":"p-1","model_id":"m","head_revision_id":"rev-c",
        "display_name":"Kamonyi MV (as built)"},"changed":true,"changes":["display_name"]})));
    let renamed = client.grid_models("p-1", &rename, NOW + 1).unwrap();
    assert_eq!(renamed.data["changes"][0], "display_name");
    assert_eq!(renamed.data["revisions_changed"], false);

    let decide = |status: &str, level: Option<&str>| Command::SetApproval {
        model: "m".into(),
        revision: "rev-b".into(),
        expected_head: "rev-c".into(),
        approval: Approval {
            status: status.into(),
            level_id: level.map(str::to_owned),
            decision_reason: Some("Reviewed on site".into()),
        },
    };
    assert!(decide("approved", None).validate("p-1").is_err());
    t.0.lock()
        .unwrap()
        .grid_models
        .push_back(project_response(json!({
        "project_id":"p-1","model_id":"m","revision_id":"rev-b","version":2,
        "effective_approval":{"status":"approved","level_id":"board"},"changed":true})));
    let decided = client
        .grid_models("p-1", &decide("approved", Some("board")), NOW + 1)
        .unwrap();
    assert_eq!(decided.data["effective_approval"]["level_id"], "board");

    let object = format!("project-grid-models/v1/p-1/m/rev-c/{digest}.dsgrid");
    let backup = |generation: i64| {
        json!({"project_id":"p-1","model_id":"m","state":"deleted","head_revision_id":"rev-c",
            "head_model_digest":digest,"digest":digest,"byte_length":5,
            "backup":{"bucket":"ds-backups","object":object,"generation":7,"digest":digest,"byte_length":5},
            "download_url":format!("https://storage.googleapis.com/ds-backups/{object}?generation={generation}&X-Goog-Signature=s")})
    };
    {
        let mut state = t.0.lock().unwrap();
        state.grid_models.push_back(project_response(backup(7)));
        state
            .grid_bytes
            .push_back(TransportResponse::new(200, b"model"));
    }
    let read = client
        .grid_models(
            "p-1",
            &Command::DownloadBackup { model: "m".into() },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(read.bytes.as_deref(), Some(b"model".as_slice()));
    assert!(!read.data.to_string().contains("Signature"));
    // A URL for another generation is refused before any byte is fetched.
    t.0.lock()
        .unwrap()
        .grid_models
        .push_back(project_response(backup(8)));
    assert!(client
        .grid_models(
            "p-1",
            &Command::DownloadBackup { model: "m".into() },
            NOW + 1
        )
        .is_err());

    // A Server that predates the action is named, not misread.
    let refusal = serde_json::to_vec(&json!({"success":false,
        "error":{"code":"VALIDATION_FAILED","message":"Unknown DS Grid model action"}}))
    .unwrap();
    t.0.lock()
        .unwrap()
        .grid_models
        .push_back(TransportResponse::new(400, refusal));
    let old = client
        .grid_models("p-1", &bump, NOW + 1)
        .err()
        .expect("an old Server refuses");
    assert_eq!(
        old.service_refusal().unwrap().code(),
        Some("server_action_unsupported")
    );
    let state = t.0.lock().unwrap();
    assert!(state.grid_models.is_empty() && state.grid_bytes.is_empty());
}

/// One `get_version` answer for the catalogue read an MV pin resolves through.
fn push_grid_revision(transport: &FixtureTransport, model: &str, revision: &str, digest: &str) {
    transport
        .0
        .lock()
        .unwrap()
        .grid_models
        .push_back(TransportResponse::new(
            200,
            serde_json::to_vec(&json!({"success":true,"data":{
                "revision":{"project_id":"project_a","model_id":model,"revision_id":revision,
                    "model":{"digest":digest,"object":format!("ds-grid/objects/sha256/{digest}"),"byte_length":5}},
                "download_url":"https://storage.googleapis.com/bucket/ds-grid/objects/sha256/x",
                "expires_at":"2026-09-25T10:15:00Z"}}))
            .unwrap(),
        ));
}

fn attachment_head(version: u64, latest: &str) -> serde_json::Value {
    json!({"project_id":"project_a","attachment_id":"file_one","object":{"kind":"mv_model","id":"line"},
           "version":version,"state":"active","latest_revision_id":latest,"latest_revision":1,"revision_count":2})
}

fn attachment_revision(revision: &str, version_id: &str) -> serde_json::Value {
    json!({"project_id":"project_a","attachment_id":"file_one","revision_id":revision,"revision":1,
           "object":{"kind":"mv_model","id":"line","version_id":version_id},"file_name":"model.bak",
           "media_type":"application/octet-stream","byte_size":5,"byte_digest":"e".repeat(64),
           "storage_generation":9,"readiness":"ready"})
}

fn attachment_bodies(transport: &FixtureTransport) -> Vec<serde_json::Value> {
    transport
        .0
        .lock()
        .unwrap()
        .calls
        .iter()
        .filter_map(|call| {
            let body = call.strip_prefix("design_attachments ")?;
            let start = body.find("body=")? + 5;
            let end = body.find(" limit=")?;
            serde_json::from_str(&body[start..end]).ok()
        })
        .collect()
}

#[test]
fn attachment_reads_name_one_file_the_project_page_and_the_versions_that_carry_it() {
    use ds_client_core::design_attachments::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    // show: the head and its revisions, archived included on request.
    transport.push_design_attachments(
        200,
        &selection_envelope(json!({"attachment":attachment_head(3,"rev_2"),
            "revisions":[attachment_revision("rev_2","rev-b"),attachment_revision("rev_1","rev-a")]})),
    );
    let shown = client
        .design_attachments(
            "project_a",
            &Command::Show {
                attachment: "file_one".into(),
                archived: true,
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(shown["attachment"]["latest_revision_id"], "rev_2");
    assert_eq!(shown["revisions"].as_array().unwrap().len(), 2);
    assert_eq!(shown["truncated"], false);
    assert_eq!(
        attachment_bodies(&transport).last().unwrap(),
        &json!({"action":"get","project_id":"project_a","attachment_id":"file_one","include_archived":true})
    );
    // A revision that belongs to another file is not this file's history.
    let mut foreign = attachment_revision("rev_9", "rev-a");
    foreign["attachment_id"] = json!("file_two");
    transport.push_design_attachments(
        200,
        &selection_envelope(json!({"attachment":attachment_head(3,"rev_2"),"revisions":[foreign]})),
    );
    assert_eq!(
        client
            .design_attachments(
                "project_a",
                &Command::Show {
                    attachment: "file_one".into(),
                    archived: false,
                },
                NOW + 1,
            )
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );

    // list-project: one bounded page, its continuation reported, never silent.
    let mut other = attachment_head(1, "rev_x");
    other["attachment_id"] = json!("file_two");
    transport.push_design_attachments(
        200,
        &selection_envelope(
            json!({"attachments":[{"attachment":attachment_head(3,"rev_2")},{"attachment":other}],
            "has_more":true,"limit":2,"next_cursor":"cursor-2"}),
        ),
    );
    let page = client
        .design_attachments(
            "project_a",
            &Command::ListProject {
                archived: false,
                limit: 2,
                cursor: Some("cursor-1".into()),
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(page["total"], 2);
    assert_eq!(page["more"], true);
    assert_eq!(page["next_cursor"], "cursor-2");
    assert_eq!(page["attachments"][1]["attachment_id"], "file_two");
    assert_eq!(
        attachment_bodies(&transport).last().unwrap(),
        &json!({"action":"list_project","project_id":"project_a","page_size":2,"cursor":"cursor-1","include_archived":false})
    );
    // More rows than the page, or "more" with nowhere to continue, is refused.
    for bad_page in [
        json!({"attachments":[{"attachment":attachment_head(3,"rev_2")},{"attachment":attachment_head(3,"rev_2")},{"attachment":attachment_head(3,"rev_2")}],"has_more":false,"limit":2}),
        json!({"attachments":[],"has_more":true,"limit":2}),
    ] {
        transport.push_design_attachments(200, &selection_envelope(bad_page));
        assert_eq!(
            client
                .design_attachments(
                    "project_a",
                    &Command::ListProject {
                        archived: false,
                        limit: 2,
                        cursor: None,
                    },
                    NOW + 1,
                )
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }

    // versions: which model versions carry this file, one row per binding.
    transport.push_design_attachments(
        200,
        &selection_envelope(json!({"object_versions":[
            {"object":{"kind":"mv_model","id":"line","version_id":"rev-b"},"revision_ids":["rev_2"],"latest_upload":"2026-09-25T00:00:00Z","revision_count":1},
            {"object":{"kind":"mv_model","id":"line","version_id":"rev-a"},"revision_ids":["rev_1","rev_0"],"latest_upload":"2026-09-20T00:00:00Z","revision_count":2}]})),
    );
    let carried = client
        .design_attachments(
            "project_a",
            &Command::Versions {
                attachment: "file_one".into(),
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(carried["total"], 2);
    assert_eq!(
        carried["object_versions"][1]["object"]["version_id"],
        "rev-a"
    );
    assert_eq!(
        attachment_bodies(&transport).last().unwrap(),
        &json!({"action":"list_object_versions","project_id":"project_a","attachment_id":"file_one"})
    );
    // One file stays anchored to one object; a row naming another is refused.
    transport.push_design_attachments(
        200,
        &selection_envelope(json!({"object_versions":[
            {"object":{"kind":"mv_model","id":"line","version_id":"rev-b"},"revision_ids":["rev_2"],"revision_count":1},
            {"object":{"kind":"mv_model","id":"other","version_id":"rev-a"},"revision_ids":["rev_1"],"revision_count":1}]})),
    );
    assert_eq!(
        client
            .design_attachments(
                "project_a",
                &Command::Versions {
                    attachment: "file_one".into(),
                },
                NOW + 1,
            )
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
}

#[test]
fn set_latest_moves_the_pointer_under_the_fence_it_read_or_was_given() {
    use ds_client_core::design_attachments::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());

    // No fence given: the head is read and its version is the fence.
    transport.push_design_attachments(
        200,
        &selection_envelope(json!({"attachment":attachment_head(4,"rev_2")})),
    );
    transport.push_design_attachments(
        200,
        &selection_envelope(json!({"attachment":attachment_head(5,"rev_1")})),
    );
    let moved = client
        .design_attachments(
            "project_a",
            &Command::SetLatest {
                attachment: "file_one".into(),
                revision: "rev_1".into(),
                expected_version: None,
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(moved["latest"], "rev_1");
    assert_eq!(moved["previous_version"], 4);
    assert_eq!(moved["version"], 5);
    let bodies = attachment_bodies(&transport);
    assert_eq!(bodies[0]["action"], "get");
    assert_eq!(
        bodies[1],
        json!({"action":"set_latest","project_id":"project_a","latest":{"project_id":"project_a","attachment_id":"file_one","revision_id":"rev_1","expected_version":4}})
    );

    // The caller's fence is sent as given, with no read in between.
    transport.push_design_attachments(
        200,
        &selection_envelope(json!({"attachment":attachment_head(8,"rev_1")})),
    );
    client
        .design_attachments(
            "project_a",
            &Command::SetLatest {
                attachment: "file_one".into(),
                revision: "rev_1".into(),
                expected_version: Some(7),
            },
            NOW + 1,
        )
        .unwrap();
    let bodies = attachment_bodies(&transport);
    assert_eq!(bodies.len(), 3);
    assert_eq!(bodies[2]["latest"]["expected_version"], 7);

    // A pointer the answer did not move is not a receipt.
    transport.push_design_attachments(
        200,
        &selection_envelope(json!({"attachment":attachment_head(8,"rev_2")})),
    );
    assert_eq!(
        client
            .design_attachments(
                "project_a",
                &Command::SetLatest {
                    attachment: "file_one".into(),
                    revision: "rev_1".into(),
                    expected_version: Some(7),
                },
                NOW + 1,
            )
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
    // A moved fence is ds-brain's conflict, surfaced once and never retried.
    transport.push_design_attachments(
        409,
        &serde_json::to_vec(&json!({"success":false,"error":{"code":"design_collab_conflict","message":"attachment is at version 9, not 7"}})).unwrap(),
    );
    let conflict = client
        .design_attachments(
            "project_a",
            &Command::SetLatest {
                attachment: "file_one".into(),
                revision: "rev_1".into(),
                expected_version: Some(7),
            },
            NOW + 1,
        )
        .unwrap_err();
    assert_eq!(conflict.kind(), ErrorKind::InvalidInput);
    assert_eq!(conflict.service_refusal().unwrap().status(), 409);
    assert_eq!(attachment_bodies(&transport).len(), 5);
}

#[test]
fn publish_declares_media_type_provenance_and_a_backfill_that_leaves_latest_alone() {
    use ds_client_core::design_attachments::{Command, Object};
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let object = Object {
        kind: "lv_transformer".into(),
        id: "kigali_a".into(),
        version: Some("v2".into()),
        version_digest: None,
    };
    let bytes = b"lv deliverable".to_vec();
    let digest = format!("{:x}", sha2::Sha256::digest(&bytes));
    let key = serde_json::to_vec(&(
        "project_a",
        json!({"kind":"lv_transformer","id":"kigali_a","version_id":"v2"}),
        "sheet.pdf",
        &digest,
    ))
    .unwrap();
    let attachment = format!("att_{:x}", sha2::Sha256::digest(&key));
    let revision = format!("rev_{:x}", sha2::Sha256::digest(&key));
    transport.push_design_attachments(200, &selection_envelope(json!({"attachment_id":attachment,"revision_id":revision,
        "session_uri":"https://storage.googleapis.com/upload/storage/v1/b/bucket/o?upload_id=issued",
        "blob_path":format!("design-attachments/v1/projects/project_a/lv_transformer/kigali_a/{attachment}/{revision}/sheet.pdf")})));
    // A new file back-filled without moving latest has no latest at all.
    let stored = json!({"project_id":"project_a","attachment_id":attachment,"revision_id":revision,"revision":1,
        "object":{"kind":"lv_transformer","id":"kigali_a","version_id":"v2"},"file_name":"sheet.pdf",
        "media_type":"application/pdf","byte_size":bytes.len(),"byte_digest":digest,"storage_generation":3,"readiness":"ready"});
    transport.push_design_attachments(200, &selection_envelope(json!({"attachment":{"project_id":"project_a","attachment_id":attachment,
        "object":{"kind":"lv_transformer","id":"kigali_a"},"version":1,"state":"active","latest_revision":0,"revision_count":1},
        "revisions":[stored]})));
    let published = client
        .design_attachments(
            "project_a",
            &Command::Publish {
                object,
                attachment: None,
                file: "sheet.pdf".into(),
                label: None,
                purpose: Some("client_deliverable".into()),
                media_type: Some("Application/PDF".into()),
                source_kind: Some("report".into()),
                source_reference: Some(" printout batch 7 ".into()),
                make_latest: false,
                bytes,
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(published["latest"], false);
    assert_eq!(published["media_type"], "application/pdf");
    assert_eq!(published["verified"], true);
    // A transformer version carries no digest, so nothing was resolved.
    assert!(transport.0.lock().unwrap().grid_bodies.is_empty());
    let bodies = attachment_bodies(&transport);
    let upload = &bodies[0]["upload"];
    assert_eq!(upload["media_type"], "application/pdf");
    assert_eq!(
        upload["source"],
        json!({"kind":"report","reference":"printout batch 7"})
    );
    assert!(
        upload.get("make_latest").is_none(),
        "the grant declaration has no pointer choice"
    );
    assert!(upload["object"].get("version_digest").is_none());
    let finalize = &bodies[1]["finalize"];
    assert_eq!(finalize["make_latest"], false);
    assert_eq!(finalize["source"], upload["source"]);
    assert!(
        finalize.get("expected_version").is_none(),
        "a new file carries no fence"
    );
}

#[test]
fn an_mv_publish_sends_the_callers_digest_pin_and_refuses_a_foreign_catalogue_answer() {
    use ds_client_core::design_attachments::{Command, Object};
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let pinned = "d".repeat(64);
    let publish = |object: Object| Command::Publish {
        object,
        attachment: None,
        file: "delivered.bak".into(),
        label: None,
        purpose: Some("native_workspace".into()),
        media_type: None,
        source_kind: Some("pls_cadd".into()),
        source_reference: None,
        make_latest: true,
        bytes: b"delivered".to_vec(),
    };
    let object = Object {
        kind: "mv_model".into(),
        id: "line".into(),
        version: Some("rev-b".into()),
        version_digest: Some(pinned.clone()),
    };
    let digest = format!("{:x}", sha2::Sha256::digest(b"delivered"));
    let key = serde_json::to_vec(&(
        "project_a",
        json!({"kind":"mv_model","id":"line","version_id":"rev-b"}),
        "delivered.bak",
        &digest,
    ))
    .unwrap();
    let attachment = format!("att_{:x}", sha2::Sha256::digest(&key));
    let revision = format!("rev_{:x}", sha2::Sha256::digest(&key));
    transport.push_design_attachments(200, &selection_envelope(json!({"attachment_id":attachment,"revision_id":revision,
        "session_uri":"https://storage.googleapis.com/upload/storage/v1/b/bucket/o?upload_id=issued",
        "blob_path":format!("design-attachments/v1/projects/project_a/mv_model/line/{attachment}/{revision}/delivered.bak")})));
    let stored = json!({"project_id":"project_a","attachment_id":attachment,"revision_id":revision,"revision":1,
        "object":{"kind":"mv_model","id":"line","version_id":"rev-b","version_digest":pinned},"file_name":"delivered.bak",
        "media_type":"application/octet-stream","byte_size":9,"byte_digest":digest,"storage_generation":3,"readiness":"ready"});
    transport.push_design_attachments(200, &selection_envelope(json!({"attachment":{"project_id":"project_a","attachment_id":attachment,
        "object":{"kind":"mv_model","id":"line"},"version":1,"state":"active","latest_revision_id":revision,"latest_revision":1,"revision_count":1},
        "revisions":[stored]})));
    let published = client
        .design_attachments("project_a", &publish(object.clone()), NOW + 1)
        .unwrap();
    assert_eq!(published["object_version"], "rev-b");
    assert_eq!(published["object_version_digest"], pinned);
    assert_eq!(published["latest"], true);
    // The caller's pin is the fence ds-brain compares; nothing re-reads it.
    assert!(transport.0.lock().unwrap().grid_bodies.is_empty());
    assert_eq!(
        attachment_bodies(&transport)[0]["upload"]["object"]["version_digest"],
        pinned
    );

    // A catalogue answer about another revision resolves nothing, and no
    // byte is uploaded against it.
    push_grid_revision(&transport, "line", "rev-z", &"c".repeat(64));
    let refused = client
        .design_attachments(
            "project_a",
            &publish(Object {
                version_digest: None,
                ..object
            }),
            NOW + 1,
        )
        .unwrap_err();
    assert_eq!(refused.kind(), ErrorKind::UnreadableResponse);
    assert_eq!(attachment_bodies(&transport).len(), 2);
    assert_eq!(
        transport
            .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|call| call.starts_with("attachment_upload"))
            .count(),
        1
    );
}

#[test]
fn an_attachment_fetch_verifies_size_and_digest_and_spends_its_locator() {
    use ds_client_core::design_attachments::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    let body = b"hello".to_vec();
    let digest = format!("{:x}", sha2::Sha256::digest(&body));
    let authorized = |url: &str| {
        let mut revision = attachment_revision("rev_1", "rev-a");
        revision["byte_digest"] = json!(digest);
        selection_envelope(
            json!({"attachment":attachment_head(3,"rev_1"),"revision":revision,
            "download_url":url,"expires_at":"2026-09-25T10:15:00Z"}),
        )
    };
    let pinned = "https://storage.googleapis.com/bucket/design-attachments/v1/x?generation=9&X-Goog-Signature=abc";
    transport.push_design_attachments(200, &authorized(pinned));
    transport
        .0
        .lock()
        .unwrap()
        .attachment_bytes
        .push_back(TransportResponse::new(200, body.clone()));
    let (receipt, bytes) = client
        .design_attachment_bytes("project_a", "file_one", Some("rev_1"), NOW + 1)
        .unwrap();
    assert_eq!(bytes, body);
    assert_eq!(receipt["verified"], true);
    assert_eq!(receipt["digest"], digest);
    assert!(
        receipt.get("url").is_none(),
        "the signed locator never leaves the owner"
    );
    assert!(transport
        .0
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|call| call == &format!("attachment_bytes url={pinned} limit=5")));

    // Changed bytes and short bytes are both refused.
    for served in [b"hellO".to_vec(), b"hell".to_vec()] {
        transport.push_design_attachments(200, &authorized(pinned));
        transport
            .0
            .lock()
            .unwrap()
            .attachment_bytes
            .push_back(TransportResponse::new(200, served));
        assert_eq!(
            client
                .design_attachment_bytes("project_a", "file_one", Some("rev_1"), NOW + 1)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }
    // A locator not pinned to the verified generation is refused before a
    // byte is fetched.
    transport.push_design_attachments(
        200,
        &authorized("https://storage.googleapis.com/bucket/design-attachments/v1/x?generation=8"),
    );
    assert!(client
        .design_attachment_bytes("project_a", "file_one", Some("rev_1"), NOW + 1)
        .is_err());
    assert_eq!(
        transport
            .0
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|call| call.starts_with("attachment_bytes"))
            .count(),
        3
    );
    // An unreadable id is refused before any request.
    assert_eq!(
        client
            .design_attachments(
                "project_a",
                &Command::Download {
                    attachment: "a/b".into(),
                    revision: None,
                },
                NOW + 1,
            )
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
}

#[test]
fn retire_carries_its_reason_to_the_audit() {
    use ds_client_core::design_attachments::Command;
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    transport.push_design_attachments(
        200,
        &selection_envelope(json!({"attachment":attachment_head(3,"rev_1")})),
    );
    let mut retired = attachment_head(4, "rev_1");
    retired["state"] = json!("archived");
    transport.push_design_attachments(200, &selection_envelope(json!({"attachment":retired})));
    let receipt = client
        .design_attachments(
            "project_a",
            &Command::Retire {
                attachment: "file_one".into(),
                revision: None,
                restore: false,
                reason: Some(" superseded by the v2 submission ".into()),
            },
            NOW + 1,
        )
        .unwrap();
    assert_eq!(receipt["state"], "archived");
    let bodies = attachment_bodies(&transport);
    assert_eq!(
        bodies[1]["archive"],
        json!({"project_id":"project_a","attachment_id":"file_one","expected_version":3,"restore":false,"reason":"superseded by the v2 submission"})
    );
}

#[test]
fn native_lv_save_atomically_sends_exact_analysis_and_refuses_replaced_readback() {
    use ds_client_core::{TransformerSaveBatch, TransformerSaveItem};
    use ds_command_kernel::report_export::jcs::layers_content_digest;
    use ds_network::network::native_fast_lv::{
        decode_native_fast_lv_request, encode_native_fast_lv_result, process_native_fast_lv,
        project_native_fast_lv_result,
    };
    let request = include_bytes!("fixtures/native-lv-save-request.json");
    let processed =
        process_native_fast_lv(decode_native_fast_lv_request(request).unwrap()).unwrap();
    let projection = project_native_fast_lv_result(
        request,
        &encode_native_fast_lv_result(&processed).unwrap(),
        "T1",
    )
    .unwrap();
    let item = TransformerSaveItem {
        transformer_name: "T1".into(),
        base_version: 7,
        source_content_digest: layers_content_digest(&projection.source_layers).unwrap(),
        source_layers: projection.source_layers,
        gdfs: projection.gdfs,
        config_dfs: projection.config_dfs,
        process_metadata: projection.process_metadata,
        process_input: projection.process_input,
        voltage_drop: projection.voltage_drop,
        operation_id: "save-T1-7".into(),
    };
    let result_digest = layers_content_digest(&item.gdfs).unwrap();
    let analysis = serde_json::to_value(&item.voltage_drop).unwrap();
    let sha = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&analysis).unwrap())
    );
    let a = &item.voltage_drop;
    let meta = json!({"state":"ready","schema":a.schema,"method":a.method,"status":a.status,
        "content_digest":result_digest,"analysis_sha256":sha,"inputs_digest":a.inputs_digest,
        "analysis_inputs_digest":a.analysis_inputs_digest,"processed_digest":a.processed_digest,"summary":a.summary});
    let context = |layers: &std::collections::BTreeMap<String, serde_json::Value>,
                   version,
                   receipt: serde_json::Value| {
        serde_json::to_vec(&json!({"total":1,"found_count":1,"failed_count":0,"results":[{"transformer_name":"T1","ok":true,
            "data":{"layers":layers,"metadata":{"version":version,"content_digest":layers_content_digest(layers).unwrap()},"voltage_drop_metadata":receipt}}]})).unwrap()
    };
    for replace_readback in [false, true] {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(transport.clone(), MemoryStore::default());
        let mut readback_meta = meta.clone();
        if replace_readback {
            readback_meta["analysis_sha256"] = json!("0".repeat(64));
        }
        transport.push_transformer_context(
            200,
            &context(&item.source_layers, 7, serde_json::Value::Null),
        );
        transport.push_transformer_context(200, &context(&item.gdfs, 8, readback_meta));
        {
            let mut state = transport.0.lock().unwrap();
            state.configurations.push_back(TransportResponse {
                status: 200,
                body: serde_json::to_vec(&json!({"sheets":item.config_dfs})).unwrap(),
            });
            state.transformer_saves.push_back(TransportResponse { status:200, body:serde_json::to_vec(&json!({"total":1,"saved_count":1,"failed_count":0,"results":[{
                "transformer_name":"T1","operation_id":item.operation_id,"ok":true,"result":{"transformer_name":"T1","version":8,"content_digest":result_digest,"unchanged":false,"voltage_drop_metadata":meta}
            }]})).unwrap() });
        }
        let receipt = client
            .save_transformers(
                "explicit-project",
                &TransformerSaveBatch {
                    project_id: "explicit-project".into(),
                    items: vec![item.clone()],
                },
                NOW + 1,
            )
            .unwrap();
        assert_eq!(receipt.project_id, "explicit-project");
        assert_eq!(receipt.results[0].verified, !replace_readback);
        if replace_readback {
            assert_eq!(
                receipt.results[0].error_code.as_deref(),
                Some("TRANSFORMER_SAVE_READBACK_UNVERIFIED")
            );
        } else {
            assert_eq!(
                receipt.results[0].analysis_sha256.as_deref(),
                Some(sha.as_str())
            );
        }
        let state = transport.0.lock().unwrap();
        assert_eq!(state.transformer_save_bodies.len(), 1);
        let wire = &state.transformer_save_bodies[0];
        assert_eq!(wire["eds_project_id"], "explicit-project");
        assert_eq!(wire["action"], "save_transformers_data");
        let row = &wire["transformer_saves"][0];
        assert_eq!(row.as_object().unwrap().len(), 7);
        assert_eq!(row["voltage_drop"], analysis);
        assert_eq!(row["gdfs"], serde_json::to_value(&item.gdfs).unwrap());
        assert!(row["process_metadata"].get("voltage_drop").is_none());
        assert!(row.get("process_input").is_none());
    }
}

fn analysis_sha(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn analysis_response(bytes: &[u8]) -> ds_client_core::TransformerAnalysisResponse {
    ds_client_core::TransformerAnalysisResponse::from_reader(
        200,
        ds_client_core::TransformerAnalysisHeaders {
            content_type: Some("application/json".into()),
            analysis_sha256: Some(analysis_sha(bytes)),
            cache_control: Some("no-store".into()),
        },
        bytes,
    )
    .unwrap()
}

fn analysis_client() -> (Client<FixtureTransport, MemoryStore>, FixtureTransport) {
    let fixture = FixtureTransport::with_sign_in(SIGN_IN);
    let mut client = Client::new(
        profile(DeploymentLane::Stable),
        fixture.clone(),
        MemoryStore::default(),
    );
    client
        .sign_in("user@example.com", "protected", NOW)
        .unwrap();
    (client, fixture)
}

#[test]
fn transformer_analysis_exact_bytes_explicit_project_and_body_without_wrapper_assumption() {
    let (mut client, fixture) = analysis_client();
    // RawValue accepts the large exponent without f64 conversion. The scalar
    // spellings, whitespace and envelope-like field names all stay untouched.
    let bytes =
        b" \n{\"n\":1.00,\"exponent\":1e999,\"zero\":-0.0,\"success\":false,\"data\":{}}\t\n";
    let sha = analysis_sha(bytes);
    fixture
        .0
        .lock()
        .unwrap()
        .transformer_analyses
        .push_back(Ok(analysis_response(bytes)));
    assert_eq!(
        client
            .transformer_analysis("project-a", "T1", 8, &"c".repeat(64), &sha, NOW)
            .unwrap(),
        bytes
    );
    let state = fixture.0.lock().unwrap();
    assert_eq!(
        state.transformer_analysis_bodies,
        vec![json!({
            "action":"get_transformer_analysis","eds_project_id":"project-a",
            "transformer_name":"T1","analysis_version":8,
            "content_digest":"c".repeat(64),"analysis_sha256":sha
        })]
    );
    let authority = &state.transformer_analysis_authorities[0];
    let signed_in: serde_json::Value = serde_json::from_slice(SIGN_IN).unwrap();
    assert_eq!(authority.0, signed_in["idToken"].as_str().unwrap());
    assert_eq!(
        (&authority.1, &authority.2, &authority.3),
        (&"uid-1".to_owned(), &"user@example.com".to_owned(), &None)
    );
}

#[test]
fn transformer_analysis_device_auth_project_interleaving_and_expiry() {
    let profile = profile(DeploymentLane::Stable);
    let credential = device_credential(&profile);
    let session = DeviceAccessSession::new("ds-memory-access-jwt".into(), NOW + 300, NOW).unwrap();
    let authorization = credential
        .authorize_api(&session, &profile, "uid-1", NOW)
        .unwrap();
    let mut fixture = FixtureTransport::default();
    let bytes = b"{\"analysis\":1.20}";
    for _ in 0..3 {
        fixture
            .0
            .lock()
            .unwrap()
            .transformer_analyses
            .push_back(Ok(analysis_response(bytes)));
    }
    for project in ["project-a", "project-b", "project-a"] {
        assert_eq!(
            authorization
                .transformer_analysis(
                    &mut fixture,
                    project,
                    "T1",
                    8,
                    &"c".repeat(64),
                    &analysis_sha(bytes),
                    NOW
                )
                .unwrap(),
            bytes
        );
    }
    let error = authorization
        .transformer_analysis(
            &mut fixture,
            "project-a",
            "T1",
            8,
            &"c".repeat(64),
            &analysis_sha(bytes),
            NOW + 301,
        )
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::SignedOut);
    let state = fixture.0.lock().unwrap();
    assert_eq!(
        state
            .transformer_analysis_bodies
            .iter()
            .map(|v| v["eds_project_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["project-a", "project-b", "project-a"]
    );
    for authority in &state.transformer_analysis_authorities {
        assert_eq!(authority.0, "ds-memory-access-jwt");
        assert_eq!(authority.1, "uid-1");
        assert_eq!(authority.2, "user@example.com");
        assert!(authority.3.is_some());
    }
}

#[test]
fn transformer_analysis_missing_moved_fence_and_invalid_refusals_remain_distinct() {
    let (mut client, fixture) = analysis_client();
    // This boundary carries the server's exact code, so it does not need a
    // duplicate backend vocabulary or conflate the two 409 causes.
    for (status, code) in [
        (409, "analysis_missing"),
        (409, "analysis_stale"),
        (400, "analysis_invalid"),
    ] {
        let bytes = serde_json::to_vec(&json!({"success":false,"error":{
            "code":code,"message":"The saved head moved", "details":{"head_version":9}
        }}))
        .unwrap();
        fixture.0.lock().unwrap().transformer_analyses.push_back(Ok(
            ds_client_core::TransformerAnalysisResponse::from_reader(
                status,
                Default::default(),
                bytes.as_slice(),
            )
            .unwrap(),
        ));
        let error = client
            .transformer_analysis("project-a", "T1", 8, &"c".repeat(64), &"a".repeat(64), NOW)
            .unwrap_err();
        let refusal = error.service_refusal().unwrap();
        assert_eq!(refusal.status(), status);
        assert_eq!(refusal.code(), Some(code));
        assert_eq!(refusal.detail_integer("head_version"), Some(9));
        assert_ne!(error.kind(), ErrorKind::Transient);
    }
    let state = fixture.0.lock().unwrap();
    assert_eq!(state.transformer_analysis_bodies.len(), 3); // no retry/fallback
    assert!(state
        .transformer_analysis_bodies
        .iter()
        .all(|v| v["analysis_version"] == 8 && v["content_digest"] == "c".repeat(64)));
}

#[test]
fn transformer_analysis_missing_malformed_wrong_sha_and_bad_contract_headers_are_refused() {
    let (mut client, fixture) = analysis_client();
    let bytes = b"{\"value\":1.00}";
    let sha = analysis_sha(bytes);
    for bad in 0..9 {
        let mut response = analysis_response(bytes);
        match bad {
            0 => response.headers.analysis_sha256 = None,
            1 => response.headers.analysis_sha256 = Some("no-sha".into()),
            2 => response.headers.analysis_sha256 = Some("0".repeat(64)),
            3 => response.headers.analysis_sha256 = Some(sha.to_uppercase()),
            4 => response.response.body.push(b' '), // unchanged JSON, wrong exact-byte SHA
            5 => response.headers.content_type = None,
            6 => response.headers.content_type = Some("text/plain".into()),
            7 => response.headers.cache_control = None,
            8 => response.headers.cache_control = Some("public".into()),
            _ => unreachable!(),
        }
        fixture
            .0
            .lock()
            .unwrap()
            .transformer_analyses
            .push_back(Ok(response));
        let error = client
            .transformer_analysis("project-a", "T1", 8, &"c".repeat(64), &sha, NOW)
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnreadableResponse, "case {bad}");
        assert!(error.service_refusal().is_none());
    }
}

#[test]
fn transformer_analysis_matching_sha_does_not_admit_malformed_json_or_refusal() {
    let (mut client, fixture) = analysis_client();
    for bytes in [
        b"{\"broken\":".as_slice(),
        b"{}{}",
        b"\xff",
        b"",
        b"{\"n\":01}",
    ] {
        fixture
            .0
            .lock()
            .unwrap()
            .transformer_analyses
            .push_back(Ok(analysis_response(bytes)));
        let error = client
            .transformer_analysis(
                "project-a",
                "T1",
                8,
                &"c".repeat(64),
                &analysis_sha(bytes),
                NOW,
            )
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnreadableResponse);
    }
    fixture.0.lock().unwrap().transformer_analyses.push_back(Ok(
        ds_client_core::TransformerAnalysisResponse::from_reader(
            409,
            Default::default(),
            b"{}".as_slice(),
        )
        .unwrap(),
    ));
    assert!(client
        .transformer_analysis("project-a", "T1", 8, &"c".repeat(64), &"a".repeat(64), NOW)
        .unwrap_err()
        .service_refusal()
        .is_none());
}

#[test]
fn transformer_analysis_stream_is_bounded_and_oversized_mock_cannot_bypass_cap() {
    use ds_client_core::{
        TransformerAnalysisHeaders, TransformerAnalysisResponse,
        TRANSFORMER_ANALYSIS_RESPONSE_LIMIT as LIMIT,
    };
    use std::io::Read;
    let mut stream = std::io::repeat(b' ').take(LIMIT as u64 + 100);
    let overflow = TransformerAnalysisResponse::from_reader(
        200,
        TransformerAnalysisHeaders::default(),
        &mut stream,
    );
    assert!(matches!(
        overflow,
        Err(ds_client_core::TransformerAnalysisTransportError::ResponseTooLarge)
    ));
    assert_eq!(stream.limit(), 99); // consumed only LIMIT + one sentinel
    let (mut client, fixture) = analysis_client();
    let response = TransformerAnalysisResponse {
        response: TransportResponse::new(200, vec![b' '; LIMIT + 1]),
        headers: TransformerAnalysisHeaders::default(),
    };
    fixture
        .0
        .lock()
        .unwrap()
        .transformer_analyses
        .push_back(Ok(response));
    assert_eq!(
        client
            .transformer_analysis("project-a", "T1", 8, &"c".repeat(64), &"a".repeat(64), NOW)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
    fixture
        .0
        .lock()
        .unwrap()
        .transformer_analyses
        .push_back(Err(
            ds_client_core::TransformerAnalysisTransportError::ResponseTooLarge,
        ));
    assert_eq!(
        client
            .transformer_analysis("project-a", "T1", 8, &"c".repeat(64), &"a".repeat(64), NOW)
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
}

#[test]
fn transformer_analysis_exact_cap_is_accepted() {
    let (mut client, fixture) = analysis_client();
    let mut bytes = vec![b' '; ds_client_core::TRANSFORMER_ANALYSIS_RESPONSE_LIMIT];
    bytes[0] = b'{';
    bytes[1] = b'}';
    let sha = analysis_sha(&bytes);
    fixture
        .0
        .lock()
        .unwrap()
        .transformer_analyses
        .push_back(Ok(analysis_response(&bytes)));
    assert_eq!(
        client
            .transformer_analysis("project-a", "T1", 8, &"c".repeat(64), &sha, NOW)
            .unwrap(),
        bytes
    );
}

#[test]
fn transformer_analysis_invalid_inputs_and_signed_out_are_refused_before_transport() {
    let (mut client, fixture) = analysis_client();
    for (project, transformer, content, sha) in [
        ("", "T1", "c".repeat(64), "a".repeat(64)),
        ("project-a", "", "c".repeat(64), "a".repeat(64)),
        ("project-a", "T1", "C".repeat(64), "a".repeat(64)),
        ("project-a", "T1", "c".repeat(64), "abc".into()),
        (
            "project-a",
            "T1",
            "c".repeat(64),
            format!("sha256:{}", "a".repeat(64)),
        ),
    ] {
        assert_eq!(
            client
                .transformer_analysis(project, transformer, 8, &content, &sha, NOW)
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidInput
        );
    }
    assert!(fixture
        .0
        .lock()
        .unwrap()
        .transformer_analysis_bodies
        .is_empty());
    let mut signed_out = Client::new(
        profile(DeploymentLane::Stable),
        fixture.clone(),
        MemoryStore::default(),
    );
    assert_eq!(
        signed_out
            .transformer_analysis("project-a", "T1", 8, &"c".repeat(64), &"a".repeat(64), NOW)
            .unwrap_err()
            .kind(),
        ErrorKind::SignedOut
    );
    assert!(fixture
        .0
        .lock()
        .unwrap()
        .transformer_analysis_bodies
        .is_empty());
}
