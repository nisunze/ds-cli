//! The closed native user-client boundary owned by ds-command-kernel.
//!
//! This leaf crate deliberately has no Tauri, Svelte, CLI, filesystem, keyring,
//! or generic HTTP dependency. A host supplies the privileged [`Transport`] and
//! atomic [`RefreshTokenStore`] adapters. Ordinary callers receive identity and
//! project values, never Firebase ID/refresh tokens or an arbitrary URL/path API.

mod project_id;
pub use project_id::{validate_project_id, MAX_PROJECT_ID_CHARS};
mod authorized;
mod project_feeders;
pub use project_feeders::{FeederConfiguration, ProjectConfigurationChange};
pub use transport::KnownColumnsCall;
pub use transport::ProjectConfigurationCall;
mod device;
mod device_approval;
pub use device_approval::{
    DeviceApprovalDecision, DeviceApprovalDevice, DeviceApprovalReceipt,
    DeviceApprovalRecordedDecision, DeviceApprovalRequest,
    MAX_REQUEST_ID_BYTES as DEVICE_APPROVAL_MAX_REQUEST_ID_BYTES,
};
pub use transport::{DeviceApproveCall, DEVICE_APPROVE_RESPONSE_LIMIT};
mod error;
pub mod reporter;
pub mod weak_network;
pub use weak_network::{WeakNetwork, WEAK_NETWORK_DELAYS};
pub mod gateway;
pub use gateway::{
    operation as gateway_operation, GatewayOperation, GATEWAY_REGISTRY_DESCRIPTOR_SHA256,
    GATEWAY_REGISTRY_OPERATIONS, GATEWAY_REGISTRY_SCHEMA_VERSION,
};
mod layers;
pub mod member_form_grants;
mod profile;
mod project_data;
mod project_form_editor;
mod project_forms;
pub use transport::MemberFormGrantCall;
mod project_report;
mod projects;
mod session;
mod solar_artifact;
pub use solar_artifact::SolarArtifactDownloadCall;
mod solar_snapshot;
pub use solar_snapshot::{HeldSessionSnapshotRead, SolarMediaDownloadCall};
mod status_processing;
mod styles;
mod survey_delete;
mod survey_entries_changes;
pub mod survey_entries_read;
mod survey_entries_select;
mod survey_entry_create;
pub use survey_delete::{SurveyDeleteCall, SurveyDeleteRequest};
mod survey_form_read;
pub use survey_entries_read::{
    SurveyEntriesRead, SurveyEntriesReadRequest, SurveyEntry, SurveyEntryMedia,
};
pub use transport::SurveyEntriesReadCall;
pub mod survey_media_fetch;
pub use survey_media_fetch::{MediaGrant, MediaGrants, SurveyPhoto, SurveyThumbnail};
pub use transport::{SurveyMediaBytesCall, SurveyMediaGrantCall};
pub mod survey_photo;
mod survey_query;
pub use transport::SurveyPhotoCall;
mod sync_gateway;
mod tiles;
pub mod transformer_analysis;
mod transformers;
pub use transformer_analysis::{
    saved_lv_analysis_pins, validate_saved_lv_analysis_pins, SavedLvAnalysisPins,
    TransformerAnalysisCall, TransformerAnalysisHeaders, TransformerAnalysisResponse,
    TransformerAnalysisTransportError, SAVED_LV_ANALYSIS_METHOD, SAVED_LV_ANALYSIS_SCHEMA,
    TRANSFORMER_ANALYSIS_ACTION, TRANSFORMER_ANALYSIS_METHOD, TRANSFORMER_ANALYSIS_PATH,
    TRANSFORMER_ANALYSIS_RESPONSE_LIMIT, TRANSFORMER_ANALYSIS_SHA256_HEADER,
    TRANSFORMER_ANALYSIS_TIMEOUT_SECONDS,
};
mod transport;

pub use device::{
    parse_device_begin, parse_device_complete, parse_device_list, parse_device_read,
    parse_device_refresh, parse_device_revoke, parse_device_status,
    secret_json as device_secret_json, DeviceAccessSession, DeviceApiAuthorization,
    DeviceAuthContext, DeviceAuthorizationStatus, DeviceBeginPublic, DeviceBeginRequest,
    DeviceBeginResult, DeviceBinding, DeviceCallFailure, DeviceCompleteRequest, DeviceCredential,
    DeviceCredentialStore, DeviceError, DeviceListResult, DevicePendingAuthorization,
    DevicePrincipal, DevicePrivateKey, DeviceProtectedCall, DeviceProtectedOperation,
    DeviceReadResult, DeviceRefreshRequest, DeviceRevokeResult, DeviceStatusRequest,
    DeviceStatusResult, DeviceSummary, DeviceTransport, AUTH_CONTEXT_MISMATCH,
    DEVICE_ACCESS_RESPONSE_LIMIT, DEVICE_ACCESS_TTL_MAX_SECONDS, DEVICE_AUTH_SCHEMA, DEVICE_SCOPE,
};
pub use error::{
    ClientError, DeviceApprovalServiceCode, ErrorKind, ProjectPropertiesServiceCode,
    ProjectReportServiceCode, ServiceRefusal, ServiceRefusalDetail, StoreError,
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
    TRANSFORMER_CONTEXT_SAVED_FIELDS,
};
pub use profile::{DESIGN_ATTACHMENTS_ACTIONS, DESIGN_ATTACHMENTS_METHOD, DESIGN_ATTACHMENTS_PATH};
pub use profile::{DESIGN_SELECTIONS_ACTIONS, DESIGN_SELECTIONS_METHOD, DESIGN_SELECTIONS_PATH};
pub use profile::{DESIGN_VERSIONS_ACTIONS, DESIGN_VERSIONS_METHOD, DESIGN_VERSIONS_PATH};
pub use profile::{LAYERS_ACTIONS, LAYERS_METHOD, LAYERS_PATH};
pub use project_form_editor::ProjectFormSettingsEditor;
pub use project_forms::{ProjectFormOrphan, ProjectFormSummary, ProjectFormsSnapshot};
pub use project_report::artifact_identity;
pub use project_report::{
    ArchiveGroupingKind, ArchiveGroupingPlan, ArchiveGroups, ArtifactDownloadRequest, ArtifactFile,
    ArtifactIdentity, ArtifactOutcome, CompoundedArchive, CompoundedArchiveCatalog,
    CompoundedArchiveLayout, CompoundedReportReceipt, CompoundedReportRequest,
    CompoundedReportStatus, DeletedBackupRead, DeletedBackupRequest, DeletedTransformerRestoration,
    ExportReportOutcome, ExportReportResult, ExportReportsReceipt, MissingIndividualArtifactCause,
    ReportFileLevel, RetirementAction, RetirementBackup, RetirementReceipt, RetirementRecord,
    RetirementRefusal, RetirementRequest, RetirementResult, TransformerInventory,
    TransformerInventoryRow, TransformerKind, TransformerLifecycle, TransformerSet,
    TransformerStatusList, TransformerStatusRow, MAX_ARTIFACT_DOWNLOADS,
    PROJECT_REPORT_MAX_REASON_CHARS, PROJECT_REPORT_MAX_TRANSFORMERS,
    PROJECT_REPORT_MAX_TRANSFORMER_CHARS,
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
    InstallHeartbeat, InstallHeartbeatWithoutAddition, NativeSyncRequest, SyncGatewayOperation,
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
    TileCall, TransformerContextCall, TransformerContextProjection, Transport, TransportResponse,
    CORRELATION_HEADERS, CORRELATION_ID_MAX_BYTES, PROCESSING_LANE_HEADER,
    PROJECT_FORMS_RESPONSE_LIMIT, PROJECT_FORMS_TIMEOUT_SECONDS,
    PROJECT_REPORT_READ_TIMEOUT_SECONDS, PROJECT_REPORT_RESPONSE_LIMIT,
    PROJECT_REPORT_TIMEOUT_SECONDS, SOLAR_SNAPSHOT_RESPONSE_LIMIT, SOLAR_SNAPSHOT_TIMEOUT_SECONDS,
    SURVEY_ENTRIES_CHANGES_RESPONSE_LIMIT, SURVEY_ENTRIES_CHANGES_TIMEOUT_SECONDS,
    SURVEY_ENTRIES_SELECT_RESPONSE_LIMIT, SURVEY_ENTRIES_SELECT_TIMEOUT_SECONDS,
    SURVEY_ENTRY_CREATE_RESPONSE_LIMIT, SURVEY_ENTRY_CREATE_TIMEOUT_SECONDS,
    SURVEY_QUERY_RESPONSE_LIMIT, SURVEY_QUERY_TIMEOUT_SECONDS, TILE_RESPONSE_LIMIT,
    TILE_TIMEOUT_SECONDS, TRANSFORMER_CONTEXT_RESPONSE_LIMIT, TRANSFORMER_CONTEXT_TIMEOUT_SECONDS,
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

pub mod held_printing;
pub use held_printing::HeldSessionPrintingRead;
pub mod printing_capture;
pub mod printing_standard;
pub use printing_standard::{
    PrintingStandardCall, PrintingStandardKind, PrintingStandardRequest,
    PRINTING_STANDARD_CATALOG_SCHEMA, PRINTING_STANDARD_PATH, PRINTING_STANDARD_RESPONSE_LIMIT,
};

pub mod printing_style_keys;
pub use printing_style_keys::{
    PrintStyleKeyChoices, PrintStyleKeysCall, PrintStyleKeysRequest,
    PRINT_STYLE_KEYS_CENSUS_SCHEMA, PRINT_STYLE_KEYS_PATH, PRINT_STYLE_KEYS_PLAN_SCHEMA,
    PRINT_STYLE_KEYS_RECEIPT_SCHEMA, PRINT_STYLE_KEYS_RESPONSE_LIMIT,
};

pub mod network_documents;
pub use network_documents::{
    NetworkDocumentsCall, NetworkDocumentsRequest, NETWORK_DOCUMENTS_ACTION,
    NETWORK_DOCUMENTS_PLAN_SCHEMA, NETWORK_DOCUMENTS_RECEIPT_SCHEMA,
    NETWORK_DOCUMENTS_RESPONSE_LIMIT, NETWORK_DOCUMENT_PARTS,
};

pub mod data_distribution;
pub use data_distribution::{
    download_bundle, normalize_upi, BundleDownloadReceipt, BundleSink, ContourParameters,
    DataDistributionRequest, PrintContextKind, SpatialResult, BUNDLE_DOWNLOAD_HOST,
    BUNDLE_DOWNLOAD_MAX_BYTES, BUNDLE_DOWNLOAD_MAX_URL_BYTES, CUSTOMERS_ROW_CAP,
    DATA_DISTRIBUTION_MAX_AREA_BYTES, DATA_DISTRIBUTION_MAX_CATALOG_ROWS,
    DATA_DISTRIBUTION_RESPONSE_LIMIT, DATA_DISTRIBUTION_TIMEOUT_SECONDS, FOUNDATION_ACTIONS,
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
/// Correspondence — parties, records, threads and task blockers — on the
/// same `POST /api/v1/pm` door, reachable without a window from the start.
pub mod project_correspondence;
pub mod project_management;
/// Creating a project and editing its properties: server-side, admin-gated,
/// and reachable without a window for the first time.
pub mod project_properties;
pub use transport::GridModelsCall;
pub use transport::ProjectPropertiesCall;
pub use transport::ReportArtifactCall;
pub use transport::{SreEventsCall, SreOverviewCall};

pub mod design_attachments;
pub mod design_versions;
pub use transport::{DesignAttachmentsCall, DesignVersionsCall};

pub mod design_migration;
pub mod design_tags;
pub use transport::DesignMigrationCall;
pub use transport::DesignTagsCall;

/// Project-to-project survey-data migration, both projects explicit —
/// reachable without a window.
pub mod survey_migration;
pub use transport::SurveyMigrationCall;

/// Tags, groups, consumer groupings, enrichment and comment threads on the
/// design annotations route — reachable without a window.
pub mod design_annotations;
/// The project's known-column visibility on the config route.
pub mod known_columns;
/// The project's document catalogue — folders, rows, bytes, classification,
/// links and ingest — reachable without a window.
pub mod project_assets;
pub mod shared_assets;
mod shared_map_publish;
pub use transport::ProjectManagementCall;
pub use transport::SharedAssetsCall;

mod transformer_save;
pub mod transformer_repair;
pub use transformer_repair::TransformerRepairPublication;
pub use transformer_save::{
    TransformerSaveBatch, TransformerSaveCall, TransformerSaveItem, TransformerSaveReceipt,
    TransformerSaved,
};

pub mod feedback;
pub use transport::FeedbackCall;

pub mod account_notifications;

pub mod messaging;
