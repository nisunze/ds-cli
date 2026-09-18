use std::fmt;

use sha2::{Digest, Sha256};
use url::Url;

pub const CLIENT_PROFILE_SCHEMA: &str = "ds.native-client-profiles/v27";
pub const DEVICE_CREDENTIAL_AUDIENCE_SCHEMA: &str = "ds.native-client-credential-audience/v1";
pub const NATIVE_CLIENT_ID: &str = "ds-native-client";
pub const AUTH_LINK_BEGIN_METHOD: &str = "POST";
pub const AUTH_LINK_BEGIN_PATH: &str = "/api/v1/auth/device/begin";
pub const AUTH_LINK_STATUS_METHOD: &str = "POST";
pub const AUTH_LINK_STATUS_PATH: &str = "/api/v1/auth/device/status";
pub const AUTH_LINK_COMPLETE_METHOD: &str = "POST";
pub const AUTH_LINK_COMPLETE_PATH: &str = "/api/v1/auth/device/complete";
pub const AUTH_DEVICE_REFRESH_METHOD: &str = "POST";
pub const AUTH_DEVICE_REFRESH_PATH: &str = "/api/v1/auth/device/refresh";
pub const AUTH_DEVICE_LIST_METHOD: &str = "GET";
pub const AUTH_DEVICE_LIST_PATH: &str = "/api/v1/auth/devices";
pub const AUTH_DEVICE_READ_METHOD: &str = "GET";
pub const AUTH_DEVICE_READ_PATH_TEMPLATE: &str = "/api/v1/auth/devices/{device_id}";
pub const AUTH_DEVICE_REVOKE_METHOD: &str = "DELETE";
pub const AUTH_DEVICE_REVOKE_PATH_TEMPLATE: &str = "/api/v1/auth/devices/{device_id}";
pub const PROJECT_LIST_METHOD: &str = "GET";
pub const PROJECT_LIST_PATH: &str = "/api/v1/user/projects";
pub const TRANSFORMER_CONTEXT_METHOD: &str = "POST";
pub const TRANSFORMER_CONTEXT_PATH: &str = "/report";
pub const TRANSFORMER_CONTEXT_ACTION: &str = "get_transformers_data";
pub const TRANSFORMER_CONTEXT_FIELDS: &str = "full";
pub const PROJECT_FORMS_METHOD: &str = "POST";
pub const PROJECT_FORMS_PATH: &str = "/api/v1/project-forms";
pub const PROJECT_FORMS_ACTION: &str = "activate";
pub const PROJECT_FORM_EDITOR_ACTION: &str = "settings_editor";
pub const SOLAR_SNAPSHOT_METHOD: &str = "POST";
pub const SOLAR_SNAPSHOT_PATH: &str = "/api/v1/solar";
pub const SOLAR_SNAPSHOT_ACTION: &str = "desktop_snapshot";
pub const SURVEY_QUERY_METHOD: &str = "POST";
pub const SURVEY_QUERY_PATH: &str = "/api/v1/survey/query";
pub const SURVEY_ENTRIES_SELECT_METHOD: &str = "POST";
pub const SURVEY_ENTRIES_SELECT_PATH: &str = "/api/v1/survey/entries/select";
pub const SURVEY_ENTRIES_CHANGES_METHOD: &str = "POST";
pub const SURVEY_ENTRIES_CHANGES_PATH: &str = "/api/v1/survey/entries/changes";
pub const SURVEY_ENTRY_CREATE_METHOD: &str = "POST";
pub const SURVEY_ENTRY_CREATE_PATH: &str = "/api/v1/entries/mutate";
pub const SURVEY_ENTRY_CREATE_OPERATION: &str = "create";
pub const PROJECT_DATA_METHOD: &str = "POST";
pub const PROJECT_DATA_PATH: &str = "/api/v1/project_data";
pub const PROJECT_DATA_ACTIONS: [&str; 4] = ["list", "upload_start", "upload", "delete"];
pub const SURVEY_CONTROL_ROUTES: [&str; 7] = [
    "POST /api/v1/form-factory: list,get,get_field_types,create,update,duplicate,publish,unpublish,archive,restore,delete",
    "POST /api/v1/project-forms: activate,settings_editor,bulk_save",
    "POST /api/v1/projects/templates: list,create,set_public,delete",
    "GET /api/v1/projects/templates/{slug}",
    "POST /api/v1/projects: apply_template",
    "POST /api/v1/projects/from-template", "POST /media: survey_upload_inspect,survey_upload_start,survey_upload_complete",
];
pub const STYLES_METHOD: &str = "POST";
pub const STYLES_PATH: &str = "/api/v1/styles";
pub const STYLES_ACTION: &str = "update_style";
pub const LAYERS_METHOD: &str = "POST";
pub const LAYERS_PATH: &str = "/api/v1/layers";
pub const LAYERS_ACTIONS: [&str; 5] = [
    "get_config",
    "get_style_catalog",
    "refresh",
    "reorder",
    "set_default_visibility",
];
pub const TILES_METHOD: &str = "POST";
pub const TILES_PATH: &str = "/api/v1/tiles";
pub const TILES_ACTIONS: [&str; 6] = ["status", "preflight", "generate", "list", "add", "remove"];
pub const PROJECT_REPORT_METHOD: &str = "POST";
pub const PROJECT_REPORT_PATH: &str = "/report";
/// The closed `/report` action vocabulary the core may send: the compounded
/// deliverable, its archive registry, the transformer inventory, reversible
/// retirement, and the transformer status rows every headless Design read is
/// built from. Order is part of the profile identity.
pub const PROJECT_REPORT_ACTIONS: [&str; 6] = [
    "download_transfo",
    "list_compounded_reports",
    "transformer_inventory",
    "retire_transformer",
    "restore_transformer",
    "list_transformers_status",
];
pub const DESIGN_ATTACHMENTS_METHOD: &str = "POST";
pub const DESIGN_ATTACHMENTS_PATH: &str = "/api/v1/design/attachments";
pub const DESIGN_ATTACHMENTS_ACTIONS: [&str; 7] = [
    "list",
    "get",
    "start_upload",
    "finalize_upload",
    "download",
    "archive",
    "archive_revision",
];
pub const DESIGN_VERSIONS_METHOD: &str = "POST";
pub const DESIGN_VERSIONS_PATH: &str = "/api/v1/design/versions";
pub const DESIGN_VERSIONS_ACTIONS: [&str; 5] = [
    "list_versions",
    "get_version",
    "get_head",
    "create_version",
    "restore_version",
];
pub const DESIGN_SELECTIONS_METHOD: &str = "POST";
pub const DESIGN_SELECTIONS_PATH: &str = "/api/v1/design/selections";
/// The closed `/api/v1/design/selections` action vocabulary the core may send.
///
/// Saved Transformer Status selections are the one Design decision that is
/// correctly ds-brain's — membership is evaluated there, never here — so this
/// route exists to give a headless caller the SAME answer the page gets, not a
/// second one. `get` is the load-bearing read: it returns the evaluated member
/// digest that `promote_task` must echo back. Order is part of the profile
/// identity.
pub const DESIGN_SELECTIONS_ACTIONS: [&str; 5] = ["list", "get", "save", "archive", "promote_task"];
pub const DATA_DISTRIBUTION_METHOD: &str = "POST";
pub const DATA_DISTRIBUTION_PATH: &str = "/api/v1/data-distribution";
/// The closed `/api/v1/data-distribution` action vocabulary the core may send:
/// the catalogue a headless seeder plans from, and the derived buildings or
/// contours acquisition ds-brain relays to the vector tiler. The billed
/// `query_dataset` page read and the publication actions are deliberately
/// absent. Order is part of the profile identity; declaring this route moved
/// the schema to v21.
pub const DATA_DISTRIBUTION_ACTIONS: [&str; 2] = ["list_datasets", "query_print_context"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentLane {
    Stable,
    Canary,
}

impl DeploymentLane {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Canary => "canary",
        }
    }
}

pub struct ClientProfileInput {
    pub schema_version: String,
    pub lane: DeploymentLane,
    pub source_revision: String,
    pub descriptor_sha256: String,
    pub firebase_project_id: String,
    pub firebase_api_key: String,
    pub gateway_api_key: String,
    pub gateway_origin: String,
    pub auth_link_begin_method: String,
    pub auth_link_begin_path: String,
    pub auth_link_status_method: String,
    pub auth_link_status_path: String,
    pub auth_link_complete_method: String,
    pub auth_link_complete_path: String,
    pub auth_device_refresh_method: String,
    pub auth_device_refresh_path: String,
    pub auth_device_list_method: String,
    pub auth_device_list_path: String,
    pub auth_device_read_method: String,
    pub auth_device_read_path_template: String,
    pub auth_device_revoke_method: String,
    pub auth_device_revoke_path_template: String,
    pub project_list_method: String,
    pub project_list_path: String,
    pub transformer_context_method: String,
    pub transformer_context_path: String,
    pub transformer_context_action: String,
    pub transformer_context_fields: String,
    pub project_forms_method: String,
    pub project_forms_path: String,
    pub project_forms_action: String,
    pub project_form_editor_action: String,
    pub solar_snapshot_method: String,
    pub solar_snapshot_path: String,
    pub solar_snapshot_action: String,
    pub survey_query_method: String,
    pub survey_query_path: String,
    pub survey_entries_select_method: String,
    pub survey_entries_select_path: String,
    pub survey_entries_changes_method: String,
    pub survey_entries_changes_path: String,
    pub survey_entry_create_method: String,
    pub survey_entry_create_path: String,
    pub survey_entry_create_operation: String,
    pub project_data_method: String,
    pub project_data_path: String,
    pub project_data_actions: Vec<String>,
    pub survey_control_routes: Vec<String>,
    pub styles_method: String,
    pub styles_path: String,
    pub styles_action: String,
    pub printing_method: String,
    pub printing_path: String,
    pub printing_actions: Vec<String>,
    pub layers_method: String,
    pub layers_path: String,
    pub layers_actions: Vec<String>,
    pub tiles_method: String,
    pub tiles_path: String,
    pub tiles_actions: Vec<String>,
    pub project_report_method: String,
    pub project_report_path: String,
    pub project_report_actions: Vec<String>,
    pub design_attachments_method: String,
    pub design_attachments_path: String,
    pub design_attachments_actions: Vec<String>,
    pub design_versions_method: String,
    pub design_versions_path: String,
    pub design_versions_actions: Vec<String>,
    pub design_selections_method: String,
    pub design_selections_path: String,
    pub design_selections_actions: Vec<String>,
    pub data_distribution_method: String,
    pub data_distribution_path: String,
    pub data_distribution_actions: Vec<String>,
}

/// One immutable, build-bound native client profile.
#[derive(Clone)]
pub struct ClientProfile {
    lane: DeploymentLane,
    source_revision: String,
    descriptor_sha256: String,
    firebase_project_id: String,
    firebase_api_key: String,
    gateway_api_key: String,
    gateway_origin: String,
    profile_sha256: String,
    credential_audience_sha256: String,
}

impl ClientProfile {
    pub fn validate(input: ClientProfileInput) -> Result<Self, ProfileError> {
        if input.schema_version != CLIENT_PROFILE_SCHEMA {
            return Err(ProfileError::Schema);
        }
        if input.source_revision.is_empty()
            || input.source_revision.trim() != input.source_revision
            || input.source_revision.len() > 128
            || input
                .source_revision
                .bytes()
                .any(|byte| byte.is_ascii_control())
        {
            return Err(ProfileError::SourceRevision);
        }
        if input.descriptor_sha256.len() != 64
            || !input
                .descriptor_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ProfileError::DescriptorDigest);
        }
        if !valid_project_id(&input.firebase_project_id) {
            return Err(ProfileError::FirebaseProject);
        }
        if !valid_public_key(&input.firebase_api_key) {
            return Err(ProfileError::FirebaseKey);
        }
        if !valid_public_key(&input.gateway_api_key) {
            return Err(ProfileError::GatewayKey);
        }
        if input.auth_link_begin_method != AUTH_LINK_BEGIN_METHOD
            || input.auth_link_begin_path != AUTH_LINK_BEGIN_PATH
            || input.auth_link_status_method != AUTH_LINK_STATUS_METHOD
            || input.auth_link_status_path != AUTH_LINK_STATUS_PATH
            || input.auth_link_complete_method != AUTH_LINK_COMPLETE_METHOD
            || input.auth_link_complete_path != AUTH_LINK_COMPLETE_PATH
            || input.auth_device_refresh_method != AUTH_DEVICE_REFRESH_METHOD
            || input.auth_device_refresh_path != AUTH_DEVICE_REFRESH_PATH
            || input.auth_device_list_method != AUTH_DEVICE_LIST_METHOD
            || input.auth_device_list_path != AUTH_DEVICE_LIST_PATH
            || input.auth_device_read_method != AUTH_DEVICE_READ_METHOD
            || input.auth_device_read_path_template != AUTH_DEVICE_READ_PATH_TEMPLATE
            || input.auth_device_revoke_method != AUTH_DEVICE_REVOKE_METHOD
            || input.auth_device_revoke_path_template != AUTH_DEVICE_REVOKE_PATH_TEMPLATE
        {
            return Err(ProfileError::DeviceAuthorizationRoutes);
        }
        if input.project_list_method != PROJECT_LIST_METHOD
            || input.project_list_path != PROJECT_LIST_PATH
        {
            return Err(ProfileError::ProjectRoute);
        }
        if input.transformer_context_method != TRANSFORMER_CONTEXT_METHOD
            || input.transformer_context_path != TRANSFORMER_CONTEXT_PATH
            || input.transformer_context_action != TRANSFORMER_CONTEXT_ACTION
            || input.transformer_context_fields != TRANSFORMER_CONTEXT_FIELDS
        {
            return Err(ProfileError::TransformerContextRoute);
        }
        if input.project_forms_method != PROJECT_FORMS_METHOD
            || input.project_forms_path != PROJECT_FORMS_PATH
            || input.project_forms_action != PROJECT_FORMS_ACTION
            || input.project_form_editor_action != PROJECT_FORM_EDITOR_ACTION
        {
            return Err(ProfileError::ProjectFormsRoute);
        }
        if input.solar_snapshot_method != SOLAR_SNAPSHOT_METHOD
            || input.solar_snapshot_path != SOLAR_SNAPSHOT_PATH
            || input.solar_snapshot_action != SOLAR_SNAPSHOT_ACTION
        {
            return Err(ProfileError::SolarSnapshotRoute);
        }
        if input.survey_query_method != SURVEY_QUERY_METHOD
            || input.survey_query_path != SURVEY_QUERY_PATH
        {
            return Err(ProfileError::SurveyQueryRoute);
        }
        if input.survey_entries_select_method != SURVEY_ENTRIES_SELECT_METHOD
            || input.survey_entries_select_path != SURVEY_ENTRIES_SELECT_PATH
        {
            return Err(ProfileError::SurveyEntriesSelectRoute);
        }
        if input.survey_entries_changes_method != SURVEY_ENTRIES_CHANGES_METHOD
            || input.survey_entries_changes_path != SURVEY_ENTRIES_CHANGES_PATH
        {
            return Err(ProfileError::SurveyEntriesChangesRoute);
        }
        if input.survey_entry_create_method != SURVEY_ENTRY_CREATE_METHOD
            || input.survey_entry_create_path != SURVEY_ENTRY_CREATE_PATH
            || input.survey_entry_create_operation != SURVEY_ENTRY_CREATE_OPERATION
        {
            return Err(ProfileError::SurveyEntryCreateRoute);
        }
        if input.project_data_method != PROJECT_DATA_METHOD
            || input.project_data_path != PROJECT_DATA_PATH
            || input.project_data_actions != PROJECT_DATA_ACTIONS
        {
            return Err(ProfileError::ProjectDataRoute);
        }
        if input.survey_control_routes != SURVEY_CONTROL_ROUTES {
            return Err(ProfileError::SurveyControlRoute);
        }
        if input.styles_method != STYLES_METHOD
            || input.styles_path != STYLES_PATH
            || input.styles_action != STYLES_ACTION
        {
            return Err(ProfileError::StylesRoute);
        }
        if input.printing_method != crate::PRINTING_METHOD
            || input.printing_path != crate::PRINTING_PATH
            || input.printing_actions != crate::PRINTING_ACTIONS.map(str::to_owned).to_vec()
        {
            return Err(ProfileError::PrintingRoute);
        }
        if input.layers_method != LAYERS_METHOD
            || input.layers_path != LAYERS_PATH
            || input.layers_actions != LAYERS_ACTIONS.map(str::to_owned).to_vec()
        {
            return Err(ProfileError::LayersRoute);
        }
        if input.tiles_method != TILES_METHOD
            || input.tiles_path != TILES_PATH
            || input.tiles_actions != TILES_ACTIONS.map(str::to_owned).to_vec()
        {
            return Err(ProfileError::TilesRoute);
        }
        if input.project_report_method != PROJECT_REPORT_METHOD
            || input.project_report_path != PROJECT_REPORT_PATH
            || input.project_report_actions != PROJECT_REPORT_ACTIONS.map(str::to_owned).to_vec()
        {
            return Err(ProfileError::ProjectReportRoute);
        }
        if input.design_attachments_method != DESIGN_ATTACHMENTS_METHOD
            || input.design_attachments_path != DESIGN_ATTACHMENTS_PATH
            || input.design_attachments_actions
                != DESIGN_ATTACHMENTS_ACTIONS.map(str::to_owned).to_vec()
        {
            return Err(ProfileError::DesignAttachmentsRoute);
        }
        if input.design_versions_method != DESIGN_VERSIONS_METHOD
            || input.design_versions_path != DESIGN_VERSIONS_PATH
            || input.design_versions_actions != DESIGN_VERSIONS_ACTIONS.map(str::to_owned).to_vec()
        {
            return Err(ProfileError::DesignVersionsRoute);
        }
        if input.design_selections_method != DESIGN_SELECTIONS_METHOD
            || input.design_selections_path != DESIGN_SELECTIONS_PATH
            || input.design_selections_actions
                != DESIGN_SELECTIONS_ACTIONS.map(str::to_owned).to_vec()
        {
            return Err(ProfileError::DesignSelectionsRoute);
        }
        if input.data_distribution_method != DATA_DISTRIBUTION_METHOD
            || input.data_distribution_path != DATA_DISTRIBUTION_PATH
            || input.data_distribution_actions
                != DATA_DISTRIBUTION_ACTIONS.map(str::to_owned).to_vec()
        {
            return Err(ProfileError::DataDistributionRoute);
        }
        let origin = Url::parse(&input.gateway_origin).map_err(|_| ProfileError::GatewayOrigin)?;
        if origin.scheme() != "https"
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || origin.path() != "/"
            || origin.port().is_some()
        {
            return Err(ProfileError::GatewayOrigin);
        }
        let host = origin
            .host_str()
            .filter(|host| host.ends_with(".ue.gateway.dev"))
            .ok_or(ProfileError::GatewayOrigin)?;
        let host_is_canary = host
            .split(['-', '.'])
            .any(|part| part.eq_ignore_ascii_case("canary"));
        if host_is_canary != matches!(input.lane, DeploymentLane::Canary) {
            return Err(ProfileError::LaneMismatch);
        }
        let gateway_origin = origin.as_str().trim_end_matches('/').to_owned();
        let mut hasher = Sha256::new();
        for value in [
            CLIENT_PROFILE_SCHEMA,
            input.lane.token(),
            NATIVE_CLIENT_ID,
            &input.source_revision,
            &input.descriptor_sha256,
            &input.firebase_project_id,
            &input.firebase_api_key,
            &input.gateway_api_key,
            &gateway_origin,
            AUTH_LINK_BEGIN_METHOD,
            AUTH_LINK_BEGIN_PATH,
            AUTH_LINK_STATUS_METHOD,
            AUTH_LINK_STATUS_PATH,
            AUTH_LINK_COMPLETE_METHOD,
            AUTH_LINK_COMPLETE_PATH,
            AUTH_DEVICE_REFRESH_METHOD,
            AUTH_DEVICE_REFRESH_PATH,
            AUTH_DEVICE_LIST_METHOD,
            AUTH_DEVICE_LIST_PATH,
            AUTH_DEVICE_READ_METHOD,
            AUTH_DEVICE_READ_PATH_TEMPLATE,
            AUTH_DEVICE_REVOKE_METHOD,
            AUTH_DEVICE_REVOKE_PATH_TEMPLATE,
            PROJECT_LIST_METHOD,
            PROJECT_LIST_PATH,
            TRANSFORMER_CONTEXT_METHOD,
            TRANSFORMER_CONTEXT_PATH,
            TRANSFORMER_CONTEXT_ACTION,
            TRANSFORMER_CONTEXT_FIELDS,
            PROJECT_FORMS_METHOD,
            PROJECT_FORMS_PATH,
            PROJECT_FORMS_ACTION,
            PROJECT_FORM_EDITOR_ACTION,
            SOLAR_SNAPSHOT_METHOD,
            SOLAR_SNAPSHOT_PATH,
            SOLAR_SNAPSHOT_ACTION,
            SURVEY_QUERY_METHOD,
            SURVEY_QUERY_PATH,
            SURVEY_ENTRIES_SELECT_METHOD,
            SURVEY_ENTRIES_SELECT_PATH,
            SURVEY_ENTRIES_CHANGES_METHOD,
            SURVEY_ENTRIES_CHANGES_PATH,
            SURVEY_ENTRY_CREATE_METHOD,
            SURVEY_ENTRY_CREATE_PATH,
            SURVEY_ENTRY_CREATE_OPERATION,
            PROJECT_DATA_METHOD,
            PROJECT_DATA_PATH,
            &PROJECT_DATA_ACTIONS.join(","),
            STYLES_METHOD,
            STYLES_PATH,
            STYLES_ACTION,
            crate::PRINTING_METHOD,
            crate::PRINTING_PATH,
            &crate::PRINTING_ACTIONS.join(","),
            LAYERS_METHOD,
            LAYERS_PATH,
            LAYERS_ACTIONS[0],
            LAYERS_ACTIONS[1],
            LAYERS_ACTIONS[2],
            LAYERS_ACTIONS[3],
            LAYERS_ACTIONS[4],
            TILES_METHOD,
            TILES_PATH,
            TILES_ACTIONS[0],
            TILES_ACTIONS[1],
            TILES_ACTIONS[2],
            TILES_ACTIONS[3],
            TILES_ACTIONS[4],
            TILES_ACTIONS[5],
            PROJECT_REPORT_METHOD,
            PROJECT_REPORT_PATH,
            PROJECT_REPORT_ACTIONS[0],
            PROJECT_REPORT_ACTIONS[1],
            PROJECT_REPORT_ACTIONS[2],
            PROJECT_REPORT_ACTIONS[3],
            PROJECT_REPORT_ACTIONS[4],
            PROJECT_REPORT_ACTIONS[5],
            DESIGN_ATTACHMENTS_METHOD,
            DESIGN_ATTACHMENTS_PATH,
            DESIGN_ATTACHMENTS_ACTIONS[0],
            DESIGN_ATTACHMENTS_ACTIONS[1],
            DESIGN_ATTACHMENTS_ACTIONS[2],
            DESIGN_ATTACHMENTS_ACTIONS[3],
            DESIGN_ATTACHMENTS_ACTIONS[4],
            DESIGN_ATTACHMENTS_ACTIONS[5],
            DESIGN_ATTACHMENTS_ACTIONS[6],
            DESIGN_VERSIONS_METHOD,
            DESIGN_VERSIONS_PATH,
            DESIGN_VERSIONS_ACTIONS[0],
            DESIGN_VERSIONS_ACTIONS[1],
            DESIGN_VERSIONS_ACTIONS[2],
            DESIGN_VERSIONS_ACTIONS[3],
            DESIGN_VERSIONS_ACTIONS[4],
            DESIGN_SELECTIONS_METHOD,
            DESIGN_SELECTIONS_PATH,
            DESIGN_SELECTIONS_ACTIONS[0],
            DESIGN_SELECTIONS_ACTIONS[1],
            DESIGN_SELECTIONS_ACTIONS[2],
            DESIGN_SELECTIONS_ACTIONS[3],
            DESIGN_SELECTIONS_ACTIONS[4],
            DATA_DISTRIBUTION_METHOD,
            DATA_DISTRIBUTION_PATH,
            DATA_DISTRIBUTION_ACTIONS[0],
            DATA_DISTRIBUTION_ACTIONS[1],
        ] {
            hasher.update((value.len() as u64).to_be_bytes());
            hasher.update(value.as_bytes());
        }
        let profile_sha256 = format!("{:x}", hasher.finalize());
        let mut audience_hasher = Sha256::new();
        for value in [
            DEVICE_CREDENTIAL_AUDIENCE_SCHEMA,
            input.lane.token(),
            NATIVE_CLIENT_ID,
            &input.firebase_project_id,
        ] {
            audience_hasher.update((value.len() as u64).to_be_bytes());
            audience_hasher.update(value.as_bytes());
        }
        let credential_audience_sha256 = format!("{:x}", audience_hasher.finalize());
        Ok(Self {
            lane: input.lane,
            source_revision: input.source_revision,
            descriptor_sha256: input.descriptor_sha256,
            firebase_project_id: input.firebase_project_id,
            firebase_api_key: input.firebase_api_key,
            gateway_api_key: input.gateway_api_key,
            gateway_origin,
            profile_sha256,
            credential_audience_sha256,
        })
    }

    pub const fn lane(&self) -> DeploymentLane {
        self.lane
    }

    pub const fn client_id(&self) -> &'static str {
        NATIVE_CLIENT_ID
    }

    pub fn descriptor_sha256(&self) -> &str {
        &self.descriptor_sha256
    }

    pub fn source_revision(&self) -> &str {
        &self.source_revision
    }

    pub fn profile_sha256(&self) -> &str {
        &self.profile_sha256
    }

    /// Stable durable-session audience. Profile schema, routes, transport
    /// origin, release provenance and rotatable public keys do not enter it.
    pub fn credential_audience_sha256(&self) -> &str {
        &self.credential_audience_sha256
    }

    pub fn firebase_project_id(&self) -> &str {
        &self.firebase_project_id
    }

    pub fn gateway_origin(&self) -> &str {
        &self.gateway_origin
    }

    pub(crate) fn firebase_api_key(&self) -> &str {
        &self.firebase_api_key
    }

    pub(crate) fn gateway_api_key(&self) -> &str {
        &self.gateway_api_key
    }

    pub(crate) fn binding(&self) -> &str {
        &self.credential_audience_sha256
    }
}

impl fmt::Debug for ClientProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientProfile")
            .field("lane", &self.lane)
            .field("source_revision", &self.source_revision)
            .field("descriptor_sha256", &self.descriptor_sha256)
            .field("firebase_project_id", &self.firebase_project_id)
            .field("firebase_api_key", &"[REDACTED]")
            .field("gateway_api_key", &"[REDACTED]")
            .field("gateway_origin", &self.gateway_origin)
            .field(
                "credential_audience_sha256",
                &self.credential_audience_sha256,
            )
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileError {
    Schema,
    SourceRevision,
    DescriptorDigest,
    FirebaseProject,
    FirebaseKey,
    GatewayKey,
    GatewayOrigin,
    LaneMismatch,
    DeviceAuthorizationRoutes,
    ProjectRoute,
    TransformerContextRoute,
    ProjectFormsRoute,
    SolarSnapshotRoute,
    SurveyQueryRoute,
    SurveyEntriesSelectRoute,
    SurveyEntriesChangesRoute,
    SurveyEntryCreateRoute,
    ProjectDataRoute,
    SurveyControlRoute,
    StylesRoute,
    PrintingRoute,
    LayersRoute,
    TilesRoute,
    ProjectReportRoute,
    DesignVersionsRoute,
    DesignAttachmentsRoute,
    DesignSelectionsRoute,
    DataDistributionRoute,
}

fn valid_project_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    (6..=30).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn valid_public_key(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.len() <= 512
        && !value.bytes().any(|byte| byte.is_ascii_control())
}
