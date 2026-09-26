//! Test-only fixtures shared by this crate's unit tests.
//!
//! One profile, one scripted transport and one in-memory refresh store, so a
//! test that needs a signed-in [`Client`] or a linked [`DeviceSession`] builds
//! it here rather than carrying its own copy of the 130-line profile input.
//! Nothing in this module reaches the network or the operator's state
//! directory.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use ds_client_core::{
    CLIENT_PROFILE_SCHEMA, Client, ClientProfile, ClientProfileInput, DeploymentLane,
    DeviceAccessSession, DeviceBinding, DeviceCredential, DevicePrivateKey, ProjectFormEditorCall,
    ProjectFormsCall, ProjectListCall, ProjectReportCall, RefreshCall, RefreshTokenStore,
    SignInCall, SolarSnapshotCall, StoreError, StoreKey, SurveyEntriesChangesCall,
    SurveyEntriesSelectCall, SurveyEntryCreateCall, SurveyQueryCall, TileCall,
    TransformerContextCall, Transport, TransportError, TransportResponse,
};

use crate::device::DeviceSession;

pub(crate) const NOW: u64 = 1_900_000_000;
pub(crate) const SIGN_IN: &[u8] = include_bytes!(
    "../../../../ds-command-kernel/crates/ds-client-core/tests/fixtures/firebase-sign-in.json"
);
pub(crate) const REFRESH: &[u8] = include_bytes!(
    "../../../../ds-command-kernel/crates/ds-client-core/tests/fixtures/firebase-refresh.json"
);
/// The bearer a linked device presents; nothing decodes it here.
pub(crate) const DEVICE_ACCESS_TOKEN: &str = "device-access-token";

/// What the scripted transport will answer, and what it saw.
#[derive(Default)]
pub(crate) struct Scripted {
    pub sign_in: Option<Vec<u8>>,
    pub refresh: Option<Vec<u8>>,
    /// One `200` body per lifecycle bucket read, in call order.
    pub projects: VecDeque<Vec<u8>>,
    pub device_approve: VecDeque<TransportResponse>,
    /// Scripted answers for `POST /api/v1/projects`; the door records like a
    /// governance door and answers Unreachable when the script is empty.
    pub project_properties: VecDeque<TransportResponse>,
    /// Scripted answers for the design tag route; Unreachable when empty.
    pub design_tags: VecDeque<TransportResponse>,
    /// Every call recorded as `door bearer device-id|user`, or with the
    /// request body for the approval door.
    pub calls: Vec<String>,
}

/// A scripted [`Transport`]. Cloning shares the script and the record, so a
/// test keeps one handle while the [`Client`] owns the other.
#[derive(Clone, Default)]
pub(crate) struct FixtureTransport(Arc<Mutex<Scripted>>);

impl FixtureTransport {
    pub(crate) fn with_sign_in(body: &[u8]) -> Self {
        let transport = Self::default();
        transport.lock().sign_in = Some(body.to_vec());
        transport
    }

    pub(crate) fn with_refresh(body: &[u8]) -> Self {
        let transport = Self::default();
        transport.lock().refresh = Some(body.to_vec());
        transport
    }

    pub(crate) fn push_projects(&self, body: &[u8]) {
        self.lock().projects.push_back(body.to_vec());
    }

    pub(crate) fn push_device_approve(&self, status: u16, body: &[u8]) {
        self.lock()
            .device_approve
            .push_back(TransportResponse::new(status, body.to_vec()));
    }

    pub(crate) fn push_project_properties(&self, status: u16, body: &[u8]) {
        self.lock()
            .project_properties
            .push_back(TransportResponse::new(status, body.to_vec()));
    }

    pub(crate) fn push_design_tags(&self, status: u16, body: &[u8]) {
        self.lock()
            .design_tags
            .push_back(TransportResponse::new(status, body.to_vec()));
    }

    pub(crate) fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }

    fn lock(&self) -> MutexGuard<'_, Scripted> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn record(
        &self,
        door: &str,
        bearer: &str,
        device_id: Option<&str>,
    ) -> Result<TransportResponse, TransportError> {
        self.lock()
            .calls
            .push(format!("{door} {bearer} {}", device_id.unwrap_or("user")));
        Err(TransportError::Unreachable)
    }
}

impl Transport for FixtureTransport {
    fn survey_control(
        &mut self,
        _call: ds_client_core::SurveyControlCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn sign_in(&mut self, _call: SignInCall<'_>) -> Result<TransportResponse, TransportError> {
        self.lock()
            .sign_in
            .take()
            .map(|body| TransportResponse::new(200, body))
            .ok_or(TransportError::Unreachable)
    }

    fn refresh(&mut self, _call: RefreshCall<'_>) -> Result<TransportResponse, TransportError> {
        self.lock()
            .refresh
            .take()
            .map(|body| TransportResponse::new(200, body))
            .ok_or(TransportError::Unreachable)
    }

    fn list_projects(
        &mut self,
        _call: ProjectListCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        self.lock()
            .projects
            .pop_front()
            .map(|body| TransportResponse::new(200, body))
            .ok_or(TransportError::Unreachable)
    }

    fn device_approve(
        &mut self,
        call: ds_client_core::DeviceApproveCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let mut scripted = self.lock();
        scripted.calls.push(format!(
            "device_approve {} {} {}",
            call.path(),
            call.bearer_token(),
            String::from_utf8_lossy(call.body().as_bytes())
        ));
        scripted
            .device_approve
            .pop_front()
            .ok_or(TransportError::Unreachable)
    }

    // The five projectless governance doors. Each records the door, the
    // bearer and the device id it was handed and answers nothing: the tests
    // that reach them ask which credential arrived, not what came back.
    fn installs(
        &mut self,
        call: ds_client_core::InstallsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        self.record("installs", call.bearer_token(), call.device_id())
    }

    fn sre_overview(
        &mut self,
        call: ds_client_core::SreOverviewCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        self.record("sre_overview", call.bearer_token(), call.device_id())
    }

    fn project_properties(
        &mut self,
        call: ds_client_core::ProjectPropertiesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let recorded = self.record("project_properties", call.bearer_token(), call.device_id());
        match self.lock().project_properties.pop_front() {
            Some(response) => Ok(response),
            None => recorded,
        }
    }

    fn design_tags(
        &mut self,
        _call: ds_client_core::DesignTagsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        self.lock()
            .design_tags
            .pop_front()
            .ok_or(TransportError::Unreachable)
    }

    fn admin_bounds(
        &mut self,
        call: ds_client_core::AdminBoundsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        self.record("admin_bounds", call.bearer_token(), call.device_id())
    }

    fn grid_catalog(
        &mut self,
        call: ds_client_core::GridCatalogCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        self.record("grid_catalog", call.bearer_token(), call.device_id())
    }

    fn global_tiles(
        &mut self,
        call: ds_client_core::GlobalTilesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        self.record("global_tiles", call.bearer_token(), call.device_id())
    }

    fn transformer_context(
        &mut self,
        _call: TransformerContextCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn project_forms(
        &mut self,
        _call: ProjectFormsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn project_form_editor(
        &mut self,
        _call: ProjectFormEditorCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn solar_snapshot(
        &mut self,
        _call: SolarSnapshotCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn survey_query(
        &mut self,
        _call: SurveyQueryCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn survey_entries_select(
        &mut self,
        _call: SurveyEntriesSelectCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn survey_entries_changes(
        &mut self,
        _call: SurveyEntriesChangesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn survey_entry_create(
        &mut self,
        _call: SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn project_data(
        &mut self,
        _call: ds_client_core::ProjectDataCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        panic!("project_data not expected")
    }
    fn upload_bytes(
        &mut self,
        _call: ds_client_core::UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        panic!("unexpected upload")
    }
    fn styles(
        &mut self,
        _call: ds_client_core::StylesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        panic!("styles not expected")
    }
    fn layers(
        &mut self,
        _call: ds_client_core::LayersCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        panic!("layers not expected")
    }
    fn tiles(&mut self, _call: TileCall<'_>) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn project_report(
        &mut self,
        _call: ProjectReportCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
}

/// One refresh credential held in memory, never on disk.
#[derive(Clone, Default)]
pub(crate) struct MemoryStore(Arc<Mutex<Option<Vec<u8>>>>);

impl RefreshTokenStore for MemoryStore {
    fn acquire(&mut self, _key: &StoreKey) -> Result<(), StoreError> {
        Ok(())
    }
    fn load(&mut self, _key: &StoreKey) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn compare_and_swap(
        &mut self,
        _key: &StoreKey,
        expected: Option<&[u8]>,
        replacement: Option<&[u8]>,
    ) -> Result<(), StoreError> {
        let mut value = self.0.lock().unwrap();
        if value.as_deref() != expected {
            return Err(StoreError::Conflict);
        }
        *value = replacement.map(<[u8]>::to_vec);
        Ok(())
    }
    fn release(&mut self, _key: &StoreKey) -> Result<(), StoreError> {
        Ok(())
    }
}

/// A native user signed in at [`NOW`] through the scripted transport.
pub(crate) fn signed_in(transport: FixtureTransport) -> Client<FixtureTransport, MemoryStore> {
    let mut client = Client::new(profile(), transport, MemoryStore::default());
    client
        .sign_in("user@example.com", "protected-password", NOW)
        .expect("the sign-in fixture restores a session");
    client
}

/// A linked device on the fixture profile, with a live memory-only access
/// session, driving the scripted transport. `now` is the wall clock the
/// session must outlive, because the device session reads it itself.
pub(crate) fn linked_device(
    transport: FixtureTransport,
    now: u64,
) -> DeviceSession<FixtureTransport> {
    let profile = profile();
    let binding = DeviceBinding::for_profile(&profile, &"b".repeat(64)).expect("binding");
    let credential = DeviceCredential::new(
        "device-1".to_owned(),
        "Workstation".to_owned(),
        "linux".to_owned(),
        format!("sha256:{}", "c".repeat(64)),
        "uid-1".to_owned(),
        "user@example.com".to_owned(),
        &binding,
        "2027-01-01T00:00:00Z".to_owned(),
        DevicePrivateKey::from_secret_bytes([7u8; 32]),
    )
    .expect("credential");
    let access = DeviceAccessSession::new(DEVICE_ACCESS_TOKEN.to_owned(), now + 600, now)
        .expect("access session");
    DeviceSession::for_test(profile, credential, access, transport)
}

pub(crate) fn profile() -> ClientProfile {
    ClientProfile::validate(ClientProfileInput {
        schema_version: CLIENT_PROFILE_SCHEMA.to_owned(),
        lane: DeploymentLane::Stable,
        source_revision: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        descriptor_sha256: "a".repeat(64),
        firebase_project_id: "firebase-project".to_owned(),
        firebase_api_key: "public-firebase-key".to_owned(),
        gateway_api_key: "public-gateway-key".to_owned(),
        gateway_origin: "https://eds-gateway-3c0q477h.ue.gateway.dev".to_owned(),
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
        layers_actions: vec![
            "get_config".to_owned(),
            "get_style_catalog".to_owned(),
            "refresh".to_owned(),
            "reorder".to_owned(),
            "set_default_visibility".to_owned(),
        ],
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
        sre_overview_method: ds_client_core::SRE_OVERVIEW_METHOD.to_owned(),
        sre_overview_path: ds_client_core::SRE_OVERVIEW_PATH.to_owned(),
        sre_events_method: ds_client_core::SRE_EVENTS_METHOD.to_owned(),
        sre_events_path: ds_client_core::SRE_EVENTS_PATH.to_owned(),
        sre_events_actions: ds_client_core::SRE_EVENTS_ACTIONS
            .iter()
            .map(|action| (*action).to_owned())
            .collect(),
        admin_bounds_method: ds_client_core::ADMIN_BOUNDS_METHOD.to_owned(),
        admin_bounds_path: ds_client_core::ADMIN_BOUNDS_PATH.to_owned(),
        admin_bounds_actions: ds_client_core::ADMIN_BOUNDS_ACTIONS
            .iter()
            .map(|action| (*action).to_owned())
            .collect(),
    })
    .unwrap()
}
