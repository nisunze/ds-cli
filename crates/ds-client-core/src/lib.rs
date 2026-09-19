//! The closed native user-client boundary owned by ds-command-kernel.
//!
//! This leaf crate deliberately has no Tauri, Svelte, CLI, filesystem, keyring,
//! or generic HTTP dependency. A host supplies the privileged [`Transport`] and
//! atomic [`RefreshTokenStore`] adapters. Ordinary callers receive identity and
//! project values, never Firebase ID/refresh tokens or an arbitrary URL/path API.

mod authorized;
mod project_feeders;
pub use project_feeders::{FeederConfiguration, ProjectConfigurationChange};
pub use transport::ProjectConfigurationCall;
mod device;
mod error;
pub mod gateway;
pub use gateway::{
    operation as gateway_operation, GatewayOperation, GATEWAY_REGISTRY_DESCRIPTOR_SHA256,
    GATEWAY_REGISTRY_OPERATIONS, GATEWAY_REGISTRY_SCHEMA_VERSION,
};
mod layers;
mod profile;
mod project_data;
mod project_form_editor;
mod project_forms;
mod project_report;
mod projects;
mod session;
mod solar_artifact;
pub use solar_artifact::SolarArtifactDownloadCall;
mod solar_snapshot;
pub use solar_snapshot::SolarMediaDownloadCall;
mod status_processing;
mod styles;
mod survey_entries_changes;
mod survey_entries_select;
mod survey_entry_create;
mod survey_form_read;
pub mod survey_photo;
mod survey_query;
pub use transport::SurveyPhotoCall;
mod sync_gateway;
mod tiles;
mod transformers;
mod transport;

pub use device::{
    parse_device_begin, parse_device_complete, parse_device_list, parse_device_read,
    parse_device_refresh, parse_device_revoke, parse_device_status,
    secret_json as device_secret_json, DeviceAccessSession, DeviceApiAuthorization,
    DeviceAuthContext, DeviceAuthorizationStatus, DeviceBeginPublic, DeviceBeginRequest,
    DeviceBeginResult, DeviceBinding, DeviceCompleteRequest, DeviceCredential,
    DeviceCredentialStore, DeviceError, DeviceListResult, DevicePendingAuthorization,
    DevicePrincipal, DevicePrivateKey, DeviceProtectedCall, DeviceProtectedOperation,
    DeviceReadResult, DeviceRefreshRequest, DeviceRevokeResult, DeviceStatusRequest,
    DeviceStatusResult, DeviceSummary, DeviceTransport, AUTH_CONTEXT_MISMATCH,
    DEVICE_ACCESS_RESPONSE_LIMIT, DEVICE_ACCESS_TTL_MAX_SECONDS, DEVICE_AUTH_SCHEMA, DEVICE_SCOPE,
};
pub use error::{
    ClientError, ErrorKind, ProjectReportServiceCode, ServiceRefusal, StoreError,
    SurveyEntriesChangesServiceCode, SurveyEntriesSelectServiceCode, SurveyEntryCreateServiceCode,
    SurveyFormReadServiceCode, SurveyQueryServiceCode, TransportError,
};
pub use layers::{
    LayerOrder, LayerOrderReceipt, LayerSnapshot, LayerVisibilityDefault,
    LayerVisibilityDefaultReceipt, StyleSnapshot, LAYERS_RESPONSE_LIMIT,
    STYLE_CATALOG_RESPONSE_LIMIT,
};
pub use profile::{
    ClientProfile, ClientProfileInput, DeploymentLane, ProfileError, CLIENT_PROFILE_SCHEMA,
    DEVICE_CREDENTIAL_AUDIENCE_SCHEMA, NATIVE_CLIENT_ID, PROJECT_FORMS_ACTION,
    PROJECT_FORMS_METHOD, PROJECT_FORMS_PATH, PROJECT_FORM_EDITOR_ACTION, PROJECT_REPORT_ACTIONS,
    PROJECT_REPORT_METHOD, PROJECT_REPORT_PATH, SOLAR_SNAPSHOT_ACTION, SOLAR_SNAPSHOT_METHOD,
    SOLAR_SNAPSHOT_PATH, SURVEY_ENTRIES_CHANGES_METHOD, SURVEY_ENTRIES_CHANGES_PATH,
    SURVEY_ENTRIES_SELECT_METHOD, SURVEY_ENTRIES_SELECT_PATH, SURVEY_ENTRY_CREATE_METHOD,
    SURVEY_ENTRY_CREATE_OPERATION, SURVEY_ENTRY_CREATE_PATH, SURVEY_QUERY_METHOD,
    SURVEY_QUERY_PATH, TILES_ACTIONS, TILES_METHOD, TILES_PATH, TRANSFORMER_CONTEXT_ACTION,
    TRANSFORMER_CONTEXT_FIELDS, TRANSFORMER_CONTEXT_METHOD, TRANSFORMER_CONTEXT_PATH,
};
pub use profile::{DESIGN_ATTACHMENTS_ACTIONS, DESIGN_ATTACHMENTS_METHOD, DESIGN_ATTACHMENTS_PATH};
pub use profile::{DESIGN_SELECTIONS_ACTIONS, DESIGN_SELECTIONS_METHOD, DESIGN_SELECTIONS_PATH};
pub use profile::{DESIGN_VERSIONS_ACTIONS, DESIGN_VERSIONS_METHOD, DESIGN_VERSIONS_PATH};
pub use profile::{LAYERS_ACTIONS, LAYERS_METHOD, LAYERS_PATH};
pub use project_form_editor::ProjectFormSettingsEditor;
pub use project_forms::{ProjectFormOrphan, ProjectFormSummary, ProjectFormsSnapshot};
pub use project_report::{
    CompoundedArchive, CompoundedArchiveLayout, CompoundedReportReceipt, CompoundedReportRequest,
    CompoundedReportStatus, MissingIndividualArtifactCause, ReportFileLevel, RetirementAction,
    RetirementReceipt, RetirementRecord, RetirementRefusal, RetirementRequest, RetirementResult,
    TransformerInventory, TransformerInventoryRow, TransformerKind, TransformerLifecycle,
    TransformerSet, TransformerStatusList, TransformerStatusRow, PROJECT_REPORT_MAX_REASON_CHARS,
    PROJECT_REPORT_MAX_TRANSFORMERS, PROJECT_REPORT_MAX_TRANSFORMER_CHARS,
};
pub use projects::{Project, ProjectDirectory, ProjectStatus, PROJECT_STATUS_ORDER};
pub use session::{
    probe_stored_refresh, AuthenticatedUser, Client, RefreshTokenStore, StoreKey, StoredAuthContext,
};
pub use solar_snapshot::SolarSnapshot;
pub use survey_entries_changes::{
    SurveyEntriesChangeRow, SurveyEntriesChanges, SurveyEntriesChangesConsistency,
    SurveyEntriesChangesRequest, SURVEY_ENTRIES_CHANGES_DEFAULT_LIMIT,
    SURVEY_ENTRIES_CHANGES_MAX_CURSOR_BYTES, SURVEY_ENTRIES_CHANGES_MAX_LIMIT,
};
pub use survey_entries_select::{
    SurveyEntriesSelectConsistency, SurveyEntriesSelectRequest, SurveyEntriesSelectRow,
    SurveyEntriesSelection, SURVEY_ENTRIES_SELECT_DEFAULT_LIMIT, SURVEY_ENTRIES_SELECT_MAX_LIMIT,
};
pub use survey_entry_create::{
    SurveyEntryCreateOrigin, SurveyEntryCreateReceipt, SurveyEntryCreateRequest,
    SURVEY_ENTRY_CREATE_MAX_DOCUMENT_ID_BYTES, SURVEY_ENTRY_CREATE_MAX_FORM_SLUG_BYTES,
    SURVEY_ENTRY_CREATE_MAX_IDEMPOTENCY_KEY_BYTES, SURVEY_ENTRY_CREATE_MAX_PAYLOAD_BYTES,
};
pub use survey_query::{
    SurveyQueryFilter, SurveyQueryFilterOp, SurveyQueryMetric, SurveyQueryOrder,
    SurveyQueryRequest, SurveyQueryResult, SurveyQueryRow,
};
pub use sync_gateway::{
    InstallHeartbeat, InstallHeartbeatWithoutAddition, NativeSyncRequest,
    SolarCalculationArtifactFinalize, SolarCalculationArtifactOpen, SyncGatewayOperation,
};
pub use tiles::{
    TileCatalog, TileCatalogEntry, TileMutation, TileOperationResult, TileOperationStatus,
    TilePreflight, TilePreflightLayer, TilePreflightStatus, TileScope, TileType,
};
pub use transformers::{TransformerContext, TransformerContextMetadata};
pub use transport::LayersCall;
pub use transport::SyncGatewayCall;
pub use transport::{
    AuthorizedTransport, ProjectFormEditorCall, ProjectFormsCall, ProjectListCall,
    ProjectReportCall, RefreshCall, SecretRequestBody, SignInCall, SolarSnapshotCall,
    SurveyEntriesChangesCall, SurveyEntriesSelectCall, SurveyEntryCreateCall, SurveyQueryCall,
    TileCall, TransformerContextCall, Transport, TransportResponse, CORRELATION_HEADERS,
    CORRELATION_ID_MAX_BYTES, PROCESSING_LANE_HEADER, PROJECT_FORMS_RESPONSE_LIMIT,
    PROJECT_FORMS_TIMEOUT_SECONDS, PROJECT_REPORT_READ_TIMEOUT_SECONDS,
    PROJECT_REPORT_RESPONSE_LIMIT, PROJECT_REPORT_TIMEOUT_SECONDS, SOLAR_SNAPSHOT_RESPONSE_LIMIT,
    SOLAR_SNAPSHOT_TIMEOUT_SECONDS, SURVEY_ENTRIES_CHANGES_RESPONSE_LIMIT,
    SURVEY_ENTRIES_CHANGES_TIMEOUT_SECONDS, SURVEY_ENTRIES_SELECT_RESPONSE_LIMIT,
    SURVEY_ENTRIES_SELECT_TIMEOUT_SECONDS, SURVEY_ENTRY_CREATE_RESPONSE_LIMIT,
    SURVEY_ENTRY_CREATE_TIMEOUT_SECONDS, SURVEY_QUERY_RESPONSE_LIMIT, SURVEY_QUERY_TIMEOUT_SECONDS,
    TILE_RESPONSE_LIMIT, TILE_TIMEOUT_SECONDS, TRANSFORMER_CONTEXT_RESPONSE_LIMIT,
    TRANSFORMER_CONTEXT_TIMEOUT_SECONDS,
};

pub use profile::{STYLES_ACTION, STYLES_METHOD, STYLES_PATH};
pub use styles::{StyleEditReceipt, StyleInstruction};
pub use transport::StylesCall;

pub use profile::{PROJECT_DATA_ACTIONS, PROJECT_DATA_METHOD, PROJECT_DATA_PATH};
pub use project_data::{ProjectDataCommand, ProjectDataReceipt, UploadBytesCall, MAX_UPLOAD_BYTES};
pub use status_processing::{
    StatusProcessingCommand, StatusProcessingReceipt, StatusUploadDomain, STATUS_PROCESSING_METHOD,
    STATUS_PROCESSING_PATH, STATUS_PROCESSING_RESPONSE_LIMIT, STATUS_PROCESSING_TIMEOUT_SECONDS,
};
pub use transport::ProjectDataCall;
pub use transport::StatusProcessingCall;

pub mod survey_control;
mod survey_control_run;
pub use survey_control::SurveyControlCommand;
pub use transport::SurveyControlCall;

pub use profile::SURVEY_CONTROL_ROUTES;

mod solar_media;
pub mod solar_portfolio;
pub mod solar_project;
pub use transport::{SolarProjectCall, SolarReferenceCall};

pub mod design_selections;
pub use design_selections::{
    DesignSelectionAnswer, DesignSelectionArchive, DesignSelectionAssignmentReceipt,
    DesignSelectionMemberRow, DesignSelectionPromotion, DesignSelectionRead,
    DesignSelectionRequest, DesignSelectionSave, DesignSelectionSummary,
    DESIGN_SELECTIONS_RESPONSE_LIMIT, MAX_SELECTIONS, MAX_SELECTION_MEMBERS,
};
pub use transport::DesignSelectionsCall;

pub mod printing;
pub use printing::{
    PrintingDestination, PrintingRequest, PrintingScope, PrintingSource, PRINTING_ACTIONS,
    PRINTING_METHOD, PRINTING_PATH, PRINTING_RESPONSE_LIMIT,
};
pub use transport::PrintingCall;

pub mod data_distribution;
pub use data_distribution::{
    download_bundle, BundleDownloadReceipt, ContourParameters, DataDistributionRequest,
    PrintContextKind, BUNDLE_DOWNLOAD_HOST, BUNDLE_DOWNLOAD_MAX_BYTES,
    BUNDLE_DOWNLOAD_MAX_URL_BYTES, DATA_DISTRIBUTION_MAX_AREA_BYTES,
    DATA_DISTRIBUTION_MAX_CATALOG_ROWS, DATA_DISTRIBUTION_RESPONSE_LIMIT,
    DATA_DISTRIBUTION_TIMEOUT_SECONDS,
};
pub use profile::{DATA_DISTRIBUTION_ACTIONS, DATA_DISTRIBUTION_METHOD, DATA_DISTRIBUTION_PATH};
pub use transport::{BundleDownloadCall, DataDistributionCall};

pub mod admin_bounds;
pub mod global_tiles;
pub mod grid_catalog;
pub mod grid_models;
pub mod report_artifact;
pub mod sre;
pub use profile::{ADMIN_BOUNDS_ACTIONS, ADMIN_BOUNDS_METHOD, ADMIN_BOUNDS_PATH};
pub use profile::{
    SRE_EVENTS_ACTIONS, SRE_EVENTS_METHOD, SRE_EVENTS_PATH, SRE_OVERVIEW_METHOD, SRE_OVERVIEW_PATH,
};
pub use transport::AdminBoundsCall;
pub use transport::GlobalTilesCall;
pub use transport::GridCatalogCall;
pub use transport::InstallsCall;

/// The installation inventory: global, admin-gated, and reachable without a
/// browser for the first time.
pub mod installs;
pub mod project_management;
pub use transport::GridModelsCall;
pub use transport::ReportArtifactCall;
pub use transport::{SreEventsCall, SreOverviewCall};

pub mod design_attachments;
pub mod design_versions;
pub use transport::{DesignAttachmentsCall, DesignVersionsCall};

pub mod design_tags;
pub use transport::DesignTagsCall;

pub mod shared_assets;
mod shared_map_publish;
pub use transport::ProjectManagementCall;
pub use transport::SharedAssetsCall;

mod transformer_save;
pub use transformer_save::{
    TransformerSaveBatch, TransformerSaveCall, TransformerSaveItem, TransformerSaveReceipt,
    TransformerSaved,
};

pub mod feedback;
pub use transport::FeedbackCall;
