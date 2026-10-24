//! Shared declarations for the existing native transport compatibility surface.
//! No routes, bodies, bounds, retries or authorization policy are stored here.

#[doc(hidden)]
#[macro_export]
macro_rules! __ds_gateway_item {
    (declare; required, $name:ident, $call:ty, $kind:ident, $route:tt) => {
        fn $name(&mut self, call: $call) -> Result<$crate::TransportResponse, $crate::TransportError>;
    };
    (declare; optional, $name:ident, $call:ty, $kind:ident, $route:tt) => {
        fn $name(&mut self, _call: $call) -> Result<$crate::TransportResponse, $crate::TransportError> {
            Err($crate::TransportError::Unreachable)
        }
    };
    (bridge; $required:ident, $name:ident, $call:ty, $kind:ident, $route:tt) => {
        fn $name(&mut self, call: $call) -> Result<$crate::TransportResponse, $crate::TransportError> {
            $crate::Transport::$name(self, call)
        }
    };
    (native, $handler:path; $required:ident, $name:ident, $call:ty, $kind:ident, $route:tt) => {
        fn $name(&mut self, call: $call) -> Result<$crate::TransportResponse, $crate::TransportError> {
            $handler(self, $crate::GatewayCall::capture_gateway(&call))
        }
    };
    (capture; $required:ident, $name:ident, $call:ty, $kind:ident, $route:tt) => {
        impl sealed::Sealed for $call {}
        impl GatewayCall for $call {
            fn capture_gateway(&self) -> CapturedGatewayRequest<'_> {
                capture_kind!(self; $kind; $route)
            }
        }
    };
}

// __ds_gateway_calls is appended from the reviewed current method/type surface.

// Common getter bodies are maintained once. The invocation retains every
// existing method's const qualifier; a body builder or bound is never inferred.
macro_rules! gateway_accessor {
    (gateway_origin; $($qualifier:tt)*) => { pub $($qualifier)* fn gateway_origin(&self) -> &str { self.profile.gateway_origin() } };
    (gateway_api_key; $($qualifier:tt)*) => { pub $($qualifier)* fn gateway_api_key(&self) -> &str { self.profile.gateway_api_key() } };
    (client_id; $($qualifier:tt)*) => { pub $($qualifier)* fn client_id(&self) -> &'static str { self.profile.client_id() } };
    (bearer_token; $($qualifier:tt)*) => { pub $($qualifier)* fn bearer_token(&self) -> &str { self.authority.bearer_token } };
    (canonical_email; $($qualifier:tt)*) => { pub $($qualifier)* fn canonical_email(&self) -> &str { self.authority.canonical_email } };
    (canonical_uid; $($qualifier:tt)*) => { pub $($qualifier)* fn canonical_uid(&self) -> &str { self.authority.canonical_uid } };
    (device_id; $($qualifier:tt)*) => { pub $($qualifier)* fn device_id(&self) -> Option<&str> { self.authority.device_id } };
    (content_type; $($qualifier:tt)*) => { pub $($qualifier)* fn content_type(&self) -> &'static str { "application/json" } };
    (correlation_headers; $($qualifier:tt)*) => { pub $($qualifier)* fn correlation_headers(&self) -> [&'static str; 2] { crate::transport::CORRELATION_HEADERS } };
}

macro_rules! gateway_accessor_mode {
    ($name:ident, normal) => { crate::gateway_compat::gateway_accessor!($name;); };
    ($name:ident, const) => { crate::gateway_compat::gateway_accessor!($name; const); };
}

macro_rules! gateway_common_accessors {
    (standard) => {
        crate::gateway_compat::gateway_common_accessors! {
            gateway_origin: normal,
            content_type: const,
            correlation_headers: const,
            gateway_api_key: normal,
            client_id: const,
            bearer_token: normal,
            canonical_email: normal,
            canonical_uid: normal,
            device_id: normal,
        }
    };
    ($($name:ident : $mode:ident),* $(,)?) => {
        $(crate::gateway_compat::gateway_accessor_mode!($name, $mode);)*
    };
}

macro_rules! gateway_fixed_route {
    ($(#[$path_meta:meta])* borrowed) => {
        $(#[$path_meta])*
        pub const fn path(&self) -> &'static str {
            Self::GATEWAY_OPERATION.path_template()
        }
        pub const fn method(&self) -> &'static str {
            Self::GATEWAY_OPERATION.method().as_str()
        }
    };
    ($(#[$path_meta:meta])* owned) => {
        $(#[$path_meta])*
        pub fn path(&self) -> String {
            Self::GATEWAY_OPERATION.path_template().into()
        }
        pub fn method(&self) -> &'static str {
            Self::GATEWAY_OPERATION.method().as_str()
        }
    };
}

pub(crate) use gateway_accessor;
pub(crate) use gateway_accessor_mode;
pub(crate) use gateway_common_accessors;
pub(crate) use gateway_fixed_route;

/// One reviewed list of existing ordinary native gateway methods.
/// Kinds preserve byte-send, GET, Accept, processing-lane and fixture mechanics.
#[doc(hidden)]
#[macro_export]
macro_rules! __ds_gateway_calls {
    ($mode:ident $(, $handler:path)?) => {
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, sync_gateway, $crate::SyncGatewayCall<'_>, sync, (selected));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, project_configuration, $crate::ProjectConfigurationCall<'_>, forced, (optional));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, known_columns, $crate::KnownColumnsCall<'_>, forced, (absent));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, list_projects, $crate::ProjectListCall<'_>, directory, (fixed $crate::ProjectListCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, transformer_context, $crate::TransformerContextCall<'_>, bytes, (fixed $crate::TransformerContextCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, project_forms, $crate::ProjectFormsCall<'_>, bytes, (fixed $crate::ProjectFormsCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, project_form_editor, $crate::ProjectFormEditorCall<'_>, bytes, (fixed $crate::ProjectFormEditorCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, solar_snapshot, $crate::SolarSnapshotCall<'_>, bytes, (fixed $crate::SolarSnapshotCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, survey_query, $crate::SurveyQueryCall<'_>, bytes, (absent));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, survey_entries_select, $crate::SurveyEntriesSelectCall<'_>, bytes, (absent));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, survey_entries_changes, $crate::SurveyEntriesChangesCall<'_>, bytes, (absent));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, survey_delete, $crate::SurveyDeleteCall<'_>, json_bytes, (fixed $crate::SurveyDeleteCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, survey_update, $crate::SurveyUpdateCall<'_>, json_bytes, (fixed $crate::SurveyUpdateCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, project_data, $crate::ProjectDataCall<'_>, bytes, (fixed $crate::ProjectDataCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, status_processing, $crate::StatusProcessingCall<'_>, processing, (fixed $crate::StatusProcessingCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, survey_photo, $crate::SurveyPhotoCall<'_>, bytes, (fixed $crate::SurveyPhotoCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, feedback, $crate::FeedbackCall<'_>, forced, (fixed $crate::FeedbackCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, shared_assets, $crate::SharedAssetsCall<'_>, forced, (selected));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, design_versions, $crate::DesignVersionsCall<'_>, forced, (selected));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, design_attachments, $crate::DesignAttachmentsCall<'_>, forced, (fixed $crate::DesignAttachmentsCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, design_tags, $crate::DesignTagsCall<'_>, forced, (fixed $crate::DesignTagsCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, project_properties, $crate::ProjectPropertiesCall<'_>, bytes, (fixed $crate::ProjectPropertiesCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, project_deletion, $crate::ProjectDeletionCall<'_>, bytes, (fixed $crate::ProjectDeletionCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, design_migration, $crate::DesignMigrationCall<'_>, forced, (fixed $crate::DesignMigrationCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, survey_migration, $crate::SurveyMigrationCall<'_>, bytes, (fixed $crate::SurveyMigrationCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, report_artifact, $crate::ReportArtifactCall<'_>, forced, (absent));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, grid_models, $crate::GridModelsCall<'_>, forced, (fixed $crate::GridModelsCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, grid_catalog, $crate::GridCatalogCall<'_>, forced, (fixed $crate::GridCatalogCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, installs, $crate::InstallsCall<'_>, bytes, (fixed $crate::InstallsCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, device_approve, $crate::DeviceApproveCall<'_>, bytes, (fixed $crate::DeviceApproveCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, messaging, $crate::messaging::MessagingCall<'_>, messaging, (selected));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, personal_notes, $crate::personal_notes::PersonalNotesCall<'_>, bytes, (fixed $crate::personal_notes::PersonalNotesCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, project_management, $crate::ProjectManagementCall<'_>, bytes, (fixed $crate::ProjectManagementCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, member_form_grant, $crate::MemberFormGrantCall<'_>, bytes, (fixed $crate::MemberFormGrantCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, admin_bounds, $crate::AdminBoundsCall<'_>, get, (fixed $crate::AdminBoundsCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, sre_overview, $crate::SreOverviewCall<'_>, get, (fixed $crate::SreOverviewCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, sre_events, $crate::SreEventsCall<'_>, ndjson, (fixed $crate::SreEventsCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, survey_entries_read, $crate::SurveyEntriesReadCall<'_>, ndjson, (fixed $crate::SurveyEntriesReadCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, survey_media_grant, $crate::SurveyMediaGrantCall<'_>, bytes, (fixed $crate::SurveyMediaGrantCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, global_tiles, $crate::GlobalTilesCall<'_>, forced, (fixed $crate::GlobalTilesCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, solar_project, $crate::SolarProjectCall<'_>, forced, (selected));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, survey_control, $crate::SurveyControlCall<'_>, forced, (optional));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, styles, $crate::StylesCall<'_>, bytes, (fixed $crate::StylesCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, printing, $crate::PrintingCall<'_>, bytes, (fixed $crate::PrintingCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, printing_standard, $crate::PrintingStandardCall<'_>, bytes, (absent));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, print_style_keys, $crate::PrintStyleKeysCall<'_>, bytes, (absent));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, network_documents, $crate::NetworkDocumentsCall<'_>, bytes, (fixed $crate::NetworkDocumentsCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, data_distribution, $crate::DataDistributionCall<'_>, bytes, (fixed $crate::DataDistributionCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, design_selections, $crate::DesignSelectionsCall<'_>, bytes, (fixed $crate::DesignSelectionsCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, layers, $crate::LayersCall<'_>, bytes, (fixed $crate::LayersCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, tiles, $crate::TileCall<'_>, bytes, (fixed $crate::TileCall<'_>));
        $crate::__ds_gateway_item!($mode $(, $handler)?; required, project_report, $crate::ProjectReportCall<'_>, bytes, (fixed $crate::ProjectReportCall<'_>));
    };
}
