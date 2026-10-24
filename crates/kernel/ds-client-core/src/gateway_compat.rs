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
        $crate::__ds_gateway_item!($mode $(, $handler)?; optional, account_deletion, $crate::account_deletion::AccountDeletionCall<'_>, bytes, (fixed $crate::account_deletion::AccountDeletionCall<'_>));
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

// Existing authenticated command wrappers share declarations, while each
// provider keeps its own credential mechanism and validates before acquiring it.
macro_rules! gateway_domain_command {
    ($provider:ident; $(#[$meta:meta])* feedback) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* feedback(command: &crate::feedback::Command) -> serde_json::Value; (now, now); command.validate()?; crate::feedback::run);
    };
    ($provider:ident; $(#[$meta:meta])* shared_assets) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* shared_assets(project: &str, command: &crate::shared_assets::Command) -> serde_json::Value; (now, now); command.validate(project)?; crate::shared_assets::run);
    };
    ($provider:ident; $(#[$meta:meta])* project_assets) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* project_assets(project: &str, command: &crate::project_assets::Command, reader: Option<&mut dyn std::io::Read>) -> serde_json::Value; (now, now); command.validate(project)?; crate::project_assets::run);
    };
    ($provider:ident; $(#[$meta:meta])* design_annotations) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* design_annotations(project: &str, command: &crate::design_annotations::Command) -> serde_json::Value; (now, now); command.validate(project)?; crate::design_annotations::run);
    };
    ($provider:ident; $(#[$meta:meta])* known_columns) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* known_columns(project: &str, command: &crate::known_columns::Command) -> serde_json::Value; (now, now); command.validate(project)?; crate::known_columns::run);
    };
    ($provider:ident; $(#[$meta:meta])* design_attachments) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* design_attachments(project: &str, command: &crate::design_attachments::Command) -> serde_json::Value; (now, now); command.validate(project)?; crate::design_attachments::run);
    };
    ($provider:ident; $(#[$meta:meta])* design_tags) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* design_tags(project: &str, command: &crate::design_tags::Command) -> serde_json::Value; (now, now); command.validate(project)?; crate::design_tags::run);
    };
    ($provider:ident; $(#[$meta:meta])* design_migration) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* design_migration(project: &str, command: &crate::design_migration::Command) -> serde_json::Value; (now, now); command.validate(project)?; crate::design_migration::run);
    };
    ($provider:ident; $(#[$meta:meta])* survey_migration) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* survey_migration(project: &str, command: &crate::survey_migration::Command) -> serde_json::Value; (now, now); command.validate(project)?; crate::survey_migration::run);
    };
    ($provider:ident; $(#[$meta:meta])* report_artifact) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* report_artifact(project: &str, command: &crate::report_artifact::Command) -> serde_json::Value; (now, now); command.validate(project)?; crate::report_artifact::run);
    };
    ($provider:ident; $(#[$meta:meta])* remove_report_artifact) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* remove_report_artifact(project: &str, command: &crate::report_artifact::RemoveCommand) -> serde_json::Value; (now, now); command.validate(project)?; crate::report_artifact::remove);
    };
    ($provider:ident; $(#[$meta:meta])* global_tiles) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* global_tiles(command: &crate::global_tiles::Command) -> serde_json::Value; (now, now); command.validate()?; crate::global_tiles::run);
    };
    ($provider:ident; $(#[$meta:meta])* admin_bounds) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* admin_bounds(command: &crate::admin_bounds::Command) -> crate::admin_bounds::Answer; (now, now); command.validate()?; crate::admin_bounds::run);
    };
    ($provider:ident; $(#[$meta:meta])* sre) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* sre(command: &crate::sre::Command) -> serde_json::Value; (now, now); command.validate()?; crate::sre::run);
    };
    ($provider:ident; $(#[$meta:meta])* installs) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* installs(command: &crate::installs::Command) -> serde_json::Value; (now, now); command.validate()?; crate::installs::run);
    };
    ($provider:ident; $(#[$meta:meta])* project_properties) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* project_properties(command: &crate::project_properties::Command) -> crate::project_properties::Receipt; (now, now); command.validate()?; crate::project_properties::run);
    };
    ($provider:ident; $(#[$meta:meta])* project_deletion) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* project_deletion(command: &crate::project_deletion::Command) -> serde_json::Value; (now, now); command.validate()?; crate::project_deletion::run);
    };
    ($provider:ident; $(#[$meta:meta])* messaging) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* messaging(command: &crate::messaging::Command) -> serde_json::Value; (now, now); command.validate()?; crate::messaging::run);
    };
    ($provider:ident; $(#[$meta:meta])* grid_catalog) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* grid_catalog(command: &crate::grid_catalog::Command) -> serde_json::Value; (now, now); command.validate()?; crate::grid_catalog::run);
    };
    ($provider:ident; $(#[$meta:meta])* grid_models) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* grid_models(project: &str, command: &crate::grid_models::Command) -> crate::grid_models::Receipt; (now, now); command.validate(project)?; crate::grid_models::run);
    };
    ($provider:ident; $(#[$meta:meta])* solar_project) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* solar_project(project: &str, command: &crate::solar_project::Command) -> serde_json::Value; (now, now); command.validate(project)?; crate::solar_project::run);
    };
    ($provider:ident; $(#[$meta:meta])* survey_control) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* survey_control(project: Option<&str>, command: &crate::SurveyControlCommand) -> serde_json::Value; (now, now); command.validate()?; crate::survey_control_run::run);
    };
    ($provider:ident; $(#[$meta:meta])* feeder_configuration) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* feeder_configuration(project: &str, bounds: Option<&crate::ProjectConfigurationChange>) -> crate::FeederConfiguration; (now, now); crate::authorized::validate_project_report(project)?; crate::authorized::feeder_configuration);
    };
    ($provider:ident; $(#[$meta:meta])* transformer_analysis) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* transformer_analysis(project: &str, transformer: &str, analysis_version: u64, content_digest: &str, analysis_sha256: &str) -> Vec<u8>; (now, now); crate::transformer_analysis::validate(
            project,
            transformer,
            analysis_version,
            content_digest,
            analysis_sha256,
        )?; crate::transformer_analysis::read);
    };
    ($provider:ident; $(#[$meta:meta])* project_forms) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* project_forms(ds_project: &str) -> ProjectFormsSnapshot; (now_epoch_seconds, now); crate::authorized::validate_project_forms(ds_project)?; crate::authorized::project_forms);
    };
    ($provider:ident; $(#[$meta:meta])* project_form_editor) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* project_form_editor(ds_project: &str, form_slug: &str) -> ProjectFormSettingsEditor; (now_epoch_seconds, now); crate::authorized::validate_project_form_editor(ds_project, form_slug)?; crate::authorized::project_form_editor);
    };
    ($provider:ident; $(#[$meta:meta])* solar_snapshot) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* solar_snapshot(ds_project: &str, template_id: &str) -> SolarSnapshot; (now_epoch_seconds, now); crate::authorized::validate_solar_snapshot(ds_project, template_id)?; crate::authorized::solar_snapshot);
    };
    ($provider:ident; $(#[$meta:meta])* survey_query) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* survey_query(ds_project: &str, query: &SurveyQueryRequest) -> SurveyQueryResult; (now_epoch_seconds, now); crate::authorized::validate_survey_query(ds_project)?; crate::authorized::survey_query);
    };
    ($provider:ident; $(#[$meta:meta])* survey_entries_select) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* survey_entries_select(ds_project: &str, request: &SurveyEntriesSelectRequest) -> SurveyEntriesSelection; (now_epoch_seconds, now); crate::authorized::validate_survey_entries_select(ds_project)?; crate::authorized::survey_entries_select);
    };
    ($provider:ident; $(#[$meta:meta])* survey_entries_changes) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* survey_entries_changes(ds_project: &str, request: &SurveyEntriesChangesRequest) -> SurveyEntriesChanges; (now_epoch_seconds, now); crate::authorized::validate_survey_entries_changes(ds_project)?; crate::authorized::survey_entries_changes);
    };
    ($provider:ident; $(#[$meta:meta])* survey_delete) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* survey_delete(project: &str, request: &crate::SurveyDeleteRequest) -> serde_json::Value; (now, now); { if request.project() != project {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "Survey delete project mismatch",
            ));
        }
        crate::authorized::validate_survey_entry_create(project)?; }; crate::survey_delete::run);
    };
    ($provider:ident; $(#[$meta:meta])* survey_update) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* survey_update(project: &str, request: &crate::SurveyUpdateRequest) -> serde_json::Value; (now, now); { if request.project() != project {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "Survey update project mismatch",
            ));
        }
        crate::authorized::validate_survey_entry_create(project)?; }; crate::survey_update::run);
    };
    ($provider:ident; $(#[$meta:meta])* survey_entry_create) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* survey_entry_create(project_id: &str, request: &SurveyEntryCreateRequest) -> SurveyEntryCreateReceipt; (now_epoch_seconds, now); crate::authorized::validate_survey_entry_create(project_id)?; crate::authorized::survey_entry_create);
    };
    ($provider:ident; $(#[$meta:meta])* tile_list) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* tile_list(ds_project: &str, include_global: bool) -> crate::TileCatalog; (now_epoch_seconds, now); crate::authorized::validate_tiles(ds_project)?; crate::authorized::tile_list);
    };
    ($provider:ident; $(#[$meta:meta])* tile_add) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* tile_add(ds_project: &str, tile_type: TileType, source_project: &str) -> crate::TileMutation; (now_epoch_seconds, now); { crate::authorized::validate_tiles(ds_project)?;
        crate::tiles::validate_catalog_id(source_project)?; }; crate::authorized::tile_add);
    };
    ($provider:ident; $(#[$meta:meta])* tile_remove) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* tile_remove(ds_project: &str, tile_id: &str, scope: crate::TileScope) -> crate::TileMutation; (now_epoch_seconds, now); { crate::authorized::validate_tiles(ds_project)?;
        crate::tiles::validate_catalog_id(tile_id)?; }; crate::authorized::tile_remove);
    };
    ($provider:ident; $(#[$meta:meta])* design_selections) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* design_selections(ds_project: &str, request: &crate::DesignSelectionRequest) -> crate::DesignSelectionAnswer; (now_epoch_seconds, now); { crate::authorized::validate_tiles(ds_project)?;
        request.validate()?; }; crate::authorized::design_selections);
    };
    ($provider:ident; $(#[$meta:meta])* data_distribution) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* data_distribution(ds_project: &str, request: &crate::DataDistributionRequest) -> serde_json::Value; (now_epoch_seconds, now); { crate::authorized::validate_tiles(ds_project)?;
        request.validate()?; }; crate::authorized::data_distribution);
    };
    ($provider:ident; $(#[$meta:meta])* printing) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* printing(ds_project: &str, request: &crate::PrintingRequest) -> serde_json::Value; (now_epoch_seconds, now); { crate::authorized::validate_tiles(ds_project)?;
        request.validate_for(ds_project)?; }; crate::authorized::printing);
    };
    ($provider:ident; $(#[$meta:meta])* printing_standard) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* printing_standard(ds_project: &str, request: &crate::PrintingStandardRequest) -> serde_json::Value; (now_epoch_seconds, now); { crate::authorized::validate_tiles(ds_project)?;
        request.validate()?; }; crate::printing_standard::read);
    };
    ($provider:ident; $(#[$meta:meta])* network_documents) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* network_documents(ds_project: &str, request: &crate::NetworkDocumentsRequest) -> serde_json::Value; (now_epoch_seconds, now); request.validate_for(ds_project)?; crate::network_documents::call);
    };
    ($provider:ident; $(#[$meta:meta])* print_style_keys) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* print_style_keys(ds_project: &str, request: &crate::PrintStyleKeysRequest) -> serde_json::Value; (now_epoch_seconds, now); request.validate_for(ds_project)?; crate::printing_style_keys::call);
    };
    ($provider:ident; $(#[$meta:meta])* layer_config) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* layer_config(ds_project: &str, refresh: bool) -> crate::LayerSnapshot; (now_epoch_seconds, now); crate::authorized::validate_tiles(ds_project)?; crate::authorized::layer_config);
    };
    ($provider:ident; $(#[$meta:meta])* style_catalog) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* style_catalog(ds_project: &str) -> crate::StyleSnapshot; (now_epoch_seconds, now); crate::authorized::validate_tiles(ds_project)?; crate::authorized::style_catalog);
    };
    ($provider:ident; $(#[$meta:meta])* layer_reorder) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* layer_reorder(ds_project: &str, orders: &[crate::LayerOrder]) -> crate::LayerOrderReceipt; (now_epoch_seconds, now); { crate::authorized::validate_tiles(ds_project)?;
        crate::layers::validate_orders(orders)?; }; crate::authorized::layer_reorder);
    };
    ($provider:ident; $(#[$meta:meta])* layer_default_visibility) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* layer_default_visibility(ds_project: &str, defaults: &[crate::LayerVisibilityDefault]) -> crate::LayerVisibilityDefaultReceipt; (now_epoch_seconds, now); { crate::authorized::validate_tiles(ds_project)?;
        crate::layers::validate_visibility_defaults(defaults)?; }; crate::authorized::layer_default_visibility);
    };
    ($provider:ident; $(#[$meta:meta])* project_data) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* project_data(project: &str, command: crate::ProjectDataCommand<'_>) -> crate::ProjectDataReceipt; (now, now); { command.validate()?;
        crate::authorized::validate_tiles(project)?; }; crate::authorized::project_data);
    };
    ($provider:ident; $(#[$meta:meta])* status_processing) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* status_processing(project: &str, command: crate::StatusProcessingCommand<'_>) -> crate::StatusProcessingReceipt; (now, now); crate::authorized::validate_tiles(project)?; crate::authorized::status_processing);
    };
    ($provider:ident; $(#[$meta:meta])* survey_photo) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* survey_photo(project: &str, command: crate::survey_photo::Command<'_>) -> crate::survey_photo::Receipt; (now, now); crate::authorized::validate_tiles(project)?; crate::survey_photo::run);
    };
    ($provider:ident; $(#[$meta:meta])* repair_transformer) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* repair_transformer(project: &str, input: &crate::TransformerRepairPublication) -> serde_json::Value; (now, now); input.validate(project)?; crate::transformer_repair::run);
    };
    ($provider:ident; $(#[$meta:meta])* reseal_transformer_digest) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* reseal_transformer_digest(project: &str, input: &crate::TransformerDigestReseal) -> serde_json::Value; (now, now); input.validate(project)?; crate::transformer_reseal::run);
    };
    ($provider:ident; $(#[$meta:meta])* save_collisions) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* save_collisions(project: &str, publication: &crate::CollisionsPublication) -> crate::CollisionsPublicationReceipt; (now, now); publication.validate()?; crate::collisions_publish::run);
    };
    ($provider:ident; $(#[$meta:meta])* material_propagation) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* material_propagation(project: &str, request: &serde_json::Value) -> serde_json::Value; (now, now); crate::authorized::validate_project_report(project)?; crate::authorized::material_propagation);
    };
    ($provider:ident; $(#[$meta:meta])* authorize_asset_preview) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* authorize_asset_preview(project: &str, request: &ds_command_kernel::asset_preview::Request) -> crate::asset_preview_source::Grant; (now, now); crate::asset_preview_source::validate_online_request(project, request)?; crate::asset_preview_source::authorize);
    };
    ($provider:ident; $(#[$meta:meta])* transformer_history_import) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* transformer_history_import(project: &str, command: &crate::transformer_history_import::Import) -> serde_json::Value; (now, now); command.validate(project).map_err(|_| {
            ClientError::new(
                crate::ErrorKind::InvalidInput,
                "invalid pinned historical import request; use stable version/revision identities and reviewed fences",
            )
        })?; crate::transformer_history_import::run);
    };
    ($provider:ident; $(#[$meta:meta])* design_version_catalog) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* design_version_catalog(project: &str, command: &crate::design_version_catalog::Command) -> serde_json::Value; (now, now); command.validate(project).map_err(|_| {
            ClientError::new(
                crate::ErrorKind::InvalidInput,
                "invalid catalog request; use stable version/revision identities and reviewed fences",
            )
        })?; crate::design_version_catalog::run);
    };
    ($provider:ident; $(#[$meta:meta])* design_versions) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* design_versions(project: &str, command: &crate::design_versions::Command) -> serde_json::Value; (now, now); command.validate(project).map_err(|_| {
            ClientError::new(
                crate::ErrorKind::InvalidInput,
                "invalid published version request; use distinct vN IDs or --to head",
            )
        })?; crate::design_versions::run);
    };
    ($provider:ident; $(#[$meta:meta])* design_attachment_bytes) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* design_attachment_bytes(project: &str, attachment: &str, revision: Option<&str>) -> (serde_json::Value, Vec<u8>); (now, now); crate::design_attachments::Command::Download {
            attachment: attachment.to_owned(),
            revision: revision.map(str::to_owned),
        }
        .validate(project)?; crate::design_attachments::fetch);
    };
    ($provider:ident; $(#[$meta:meta])* personal_notes) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* personal_notes(action: &crate::personal_notes::Action) -> serde_json::Value; (now, now); action.validate()?; crate::personal_notes::run);
    };
    ($provider:ident; $(#[$meta:meta])* style_edit) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* style_edit(ds_project: &str, reference: &str, instruction: &crate::StyleInstruction, apply: bool) -> crate::StyleEditReceipt; (now_epoch_seconds, now); crate::authorized::validate_tiles(ds_project)?; crate::authorized::style_edit);
    };
    ($provider:ident; $(#[$meta:meta])* style_history) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* style_history(ds_project: &str, command: &ds_command_kernel::style_history::Command) -> serde_json::Value; (now_epoch_seconds, now); crate::authorized::validate_tiles(ds_project)?; crate::styles::history);
    };
    ($provider:ident; $(#[$meta:meta])* style_governance) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* style_governance(ds_project: &str, command: &ds_command_kernel::style_governance::Command) -> serde_json::Value; (now_epoch_seconds, now); crate::authorized::validate_tiles(ds_project)?; crate::styles::governance);
    };
    ($provider:ident; $(#[$meta:meta])* tile_status) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* tile_status(ds_project: &str, tile_type: TileType) -> TileOperationResult; (now_epoch_seconds, now); crate::authorized::validate_tiles(ds_project)?; crate::authorized::tile_status);
    };
    ($provider:ident; $(#[$meta:meta])* tile_preflight) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* tile_preflight(ds_project: &str, tile_type: TileType) -> TilePreflight; (now_epoch_seconds, now); crate::authorized::validate_tiles(ds_project)?; crate::authorized::tile_preflight);
    };
    ($provider:ident; $(#[$meta:meta])* tile_generate) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* tile_generate(ds_project: &str, tile_type: TileType, force: bool) -> TileOperationResult; (now_epoch_seconds, now); crate::authorized::validate_tiles(ds_project)?; crate::authorized::tile_generate);
    };
    ($provider:ident; $(#[$meta:meta])* compounded_report_list) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* compounded_report_list(ds_project: &str) -> CompoundedArchiveCatalog; (now_epoch_seconds, now); crate::authorized::validate_project_report(ds_project)?; crate::authorized::compounded_report_list);
    };
    ($provider:ident; $(#[$meta:meta])* transformer_inventory) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* transformer_inventory(ds_project: &str, requested: &TransformerSet) -> TransformerInventory; (now_epoch_seconds, now); crate::authorized::validate_project_report(ds_project)?; crate::authorized::transformer_inventory);
    };
    ($provider:ident; $(#[$meta:meta])* transformer_status) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* transformer_status(ds_project: &str, requested: &TransformerSet) -> TransformerStatusList; (now_epoch_seconds, now); crate::authorized::validate_project_report(ds_project)?; crate::authorized::transformer_status);
    };
    ($provider:ident; $(#[$meta:meta])* transformer_retirement) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* transformer_retirement(ds_project: &str, action: RetirementAction, request: &RetirementRequest) -> RetirementReceipt; (now_epoch_seconds, now); crate::authorized::validate_project_report(ds_project)?; crate::authorized::transformer_retirement);
    };
    ($provider:ident; $(#[$meta:meta])* deleted_transformer_backup) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* deleted_transformer_backup(ds_project: &str, request: &crate::DeletedBackupRequest, fetch: bool) -> crate::DeletedBackupRead; (now_epoch_seconds, now); crate::authorized::validate_project_report(ds_project)?; crate::project_report::deleted_backup_read);
    };
    ($provider:ident; $(#[$meta:meta])* download_artifacts) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* download_artifacts(ds_project: &str, request: &crate::ArtifactDownloadRequest, sink: &mut dyn FnMut(crate::ArtifactFile) -> Result<(), ClientError>) -> Vec<(crate::ArtifactIdentity, crate::ArtifactOutcome)>; (now_epoch_seconds, now); crate::authorized::validate_project_report(ds_project)?; crate::project_report::download_artifacts);
    };
    ($provider:ident; $(#[$meta:meta])* restore_deleted_transformer) => {
        crate::gateway_compat::gateway_domain_command!(@$provider; $(#[$meta])* restore_deleted_transformer(ds_project: &str, request: &crate::DeletedBackupRequest) -> crate::DeletedTransformerRestoration; (now_epoch_seconds, now); crate::authorized::validate_project_report(ds_project)?; crate::project_report::deleted_backup_restore);
    };
    (@session; $(#[$meta:meta])* $name:ident($($arg:ident: $ty:ty),*) -> $output:ty; ($now:ident, $device_now:ident); $validate:expr; $run:path) => {
        $(#[$meta])*
        pub fn $name(&mut self, $($arg: $ty,)* $now: u64) -> Result<$output, $crate::ClientError> {
            $validate;
            self.with_authority($now, |profile, transport, authority| {
                $run(profile, transport, authority, $($arg),*)
            })
        }
    };
    (@device; $(#[$meta:meta])* $name:ident($($arg:ident: $ty:ty),*) -> $output:ty; ($session_now:ident, $now:ident); $validate:expr; $run:path) => {
        $(#[$meta])*
        pub fn $name<T: $crate::AuthorizedTransport>(&self, transport: &mut T, $($arg: $ty,)* $now: u64) -> Result<$output, $crate::ClientError> {
            $validate;
            self.ensure_active($now)?;
            $run(self.profile, transport, self.authority(), $($arg),*)
        }
    };
}
pub(crate) use gateway_domain_command;
