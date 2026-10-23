use crate::project_report::ProjectReportOperation;
use crate::tiles::TileOperation;
use crate::{
    ClientProfile, ProjectStatus, SurveyEntriesChangesRequest, SurveyEntriesSelectRequest,
    SurveyQueryRequest, TransportError,
};
use serde::Serialize;
use std::io::Write;
use zeroize::Zeroize;

pub const SIGN_IN_ENDPOINT: &str =
    "https://identitytoolkit.googleapis.com/v1/accounts:signInWithPassword";
pub const REFRESH_ENDPOINT: &str = "https://securetoken.googleapis.com/v1/token";
pub use crate::profile::SRE_EVENTS_ACTIONS;
pub use crate::profile::{
    PROJECT_FORMS_ACTION, PROJECT_FORM_EDITOR_ACTION, SURVEY_ENTRIES_CHANGES_METHOD,
    SURVEY_ENTRIES_CHANGES_PATH, SURVEY_ENTRIES_SELECT_METHOD, SURVEY_ENTRIES_SELECT_PATH,
    SURVEY_ENTRY_CREATE_METHOD, SURVEY_QUERY_METHOD, SURVEY_QUERY_PATH, TRANSFORMER_CONTEXT_FIELDS,
};
pub const AUTH_RESPONSE_LIMIT: usize = 256 * 1024;
pub const PROJECT_RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
pub const TRANSFORMER_CONTEXT_RESPONSE_LIMIT: usize = 64 * 1024 * 1024;
pub const TRANSFORMER_CONTEXT_TIMEOUT_SECONDS: u64 = 120;
pub const PROJECT_FORMS_RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
pub const PROJECT_FORMS_TIMEOUT_SECONDS: u64 = 120;
// The embedded snapshot itself is capped at 8 MiB, but the outer JSON string
// can grow substantially through escaping and is accompanied by up to 512
// signed media handoffs. This matches the governed Solar response lane.
pub const SOLAR_SNAPSHOT_RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
pub const SOLAR_SNAPSHOT_TIMEOUT_SECONDS: u64 = 120;
pub const SURVEY_QUERY_RESPONSE_LIMIT: usize = 1024 * 1024;
pub const SURVEY_QUERY_TIMEOUT_SECONDS: u64 = 120;
pub const SURVEY_ENTRIES_SELECT_RESPONSE_LIMIT: usize = 1024 * 1024;
pub const SURVEY_ENTRIES_SELECT_TIMEOUT_SECONDS: u64 = 120;
pub const SURVEY_ENTRIES_CHANGES_RESPONSE_LIMIT: usize = 1024 * 1024;
pub const SURVEY_ENTRIES_CHANGES_TIMEOUT_SECONDS: u64 = 120;
pub const SURVEY_ENTRY_CREATE_RESPONSE_LIMIT: usize = 1024 * 1024;
pub const SURVEY_ENTRY_CREATE_TIMEOUT_SECONDS: u64 = 120;
pub const TILE_RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
pub const TILE_TIMEOUT_SECONDS: u64 = 120;
/// One administrative polygon, with room to spare: Rwanda's largest province
/// answers in ~0.5 MiB, so anything past this is a query that came back with
/// something other than one boundary.
pub const ADMIN_BOUNDS_RESPONSE_LIMIT: usize = 8 * 1024 * 1024;
/// The gateway's own deadline for the route (`ds-apis-tf/api_rwanda_admin.tf`).
pub const ADMIN_BOUNDS_TIMEOUT_SECONDS: u64 = 60;
/// The overview is a handful of Cloud Monitoring projections; ds-brain composes
/// it inside its own 25-second budget, and the gateway allows 30.
pub const SRE_OVERVIEW_RESPONSE_LIMIT: usize = 8 * 1024 * 1024;
pub const SRE_OVERVIEW_TIMEOUT_SECONDS: u64 = 30;
/// A saturated event window is five thousand rows of NDJSON. The gateway allows
/// the tabular route five minutes; this client asks for three, because an
/// operator waiting on a terminal is not a batch job.
pub const SRE_EVENTS_RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
pub const SRE_EVENTS_TIMEOUT_SECONDS: u64 = 180;
pub const PROJECT_REPORT_RESPONSE_LIMIT: usize = 8 * 1024 * 1024;
/// The compounded deliverable fans out reporter work and streams a ZIP; the
/// gateway allows ten minutes for the `/report` route and so does the core.
pub const PROJECT_REPORT_TIMEOUT_SECONDS: u64 = 600;
pub const PROJECT_REPORT_READ_TIMEOUT_SECONDS: u64 = 120;
pub const PROCESSING_LANE_HEADER: &str = "X-DS-Processing-Lane";
pub const CORRELATION_ID_MAX_BYTES: usize = 128;
pub const CORRELATION_HEADERS: [&str; 2] = ["X-Request-Id", "X-DS-Action-Id"];

/// Response bytes from one closed transport operation.
pub struct TransportResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl TransportResponse {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }
}

impl Drop for TransportResponse {
    fn drop(&mut self) {
        self.body.zeroize();
    }
}

/// A request body whose owned secret copies are scrubbed on drop.
pub struct SecretRequestBody(Vec<u8>);

impl SecretRequestBody {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for SecretRequestBody {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Privileged input for one Sync Center operation. The request has already
/// selected one of four registry entries and validated its selected-project
/// fence; a transport can inspect it but cannot turn it into a generic client.
pub struct SyncGatewayCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) request: &'a crate::NativeSyncRequest,
    pub(crate) authority: CallAuthority<'a>,
}

impl SyncGatewayCall<'_> {
    pub(crate) fn generated_operation(&self) -> ds_gateway_client::Operation {
        match self.request.operation() {
            crate::SyncGatewayOperation::InstallHeartbeat => {
                ds_gateway_client::Operation::EdgeAppInstallHeartbeat
            }
            crate::SyncGatewayOperation::ComputeArtifacts
            | crate::SyncGatewayOperation::ComputeArtifactsOpen
            | crate::SyncGatewayOperation::ComputeArtifactsFinalize => {
                ds_gateway_client::Operation::ComputeArtifactsAction
            }
        }
    }

    fn entry(&self) -> &'static crate::GatewayOperation {
        crate::gateway_operation(self.request.operation().registry_id())
            .expect("the closed Sync Center operation is generated in the registry")
    }
    crate::gateway_compat::gateway_common_accessors! {
        gateway_origin: normal,
        gateway_api_key: normal,
        client_id: const,
        bearer_token: normal,
        canonical_email: normal,
    }
    pub fn path(&self) -> &'static str {
        self.generated_operation().path_template()
    }
    pub fn method(&self) -> &'static str {
        self.generated_operation().method().as_str()
    }
    pub fn response_limit(&self) -> usize {
        self.entry().response_cap_bytes as usize
    }
    pub fn timeout_seconds(&self) -> u64 {
        u64::from(self.entry().timeout_s)
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(self.request.body()).expect("validated sync JSON"))
    }
}

/// Provider-neutral authority for one fixed typed gateway call.
///
/// This value is never serializable or debuggable. A transport may use the
/// bearer as `Authorization`, retain the canonical identity forwarding used by
/// the existing gateway, and observe device attribution without guessing the
/// bearer provider.
#[derive(Clone, Copy)]
pub(crate) struct CallAuthority<'a> {
    pub(crate) bearer_token: &'a str,
    pub(crate) canonical_uid: &'a str,
    pub(crate) canonical_email: &'a str,
    pub(crate) device_id: Option<&'a str>,
}

/// Privileged adapter input for Firebase email/password sign-in.
///
/// Only a transport implementation should read `password` or the public client
/// keys. It must not log, persist, or return them.
pub struct SignInCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) email: &'a str,
    pub(crate) password: &'a str,
}

impl SignInCall<'_> {
    pub const fn endpoint(&self) -> &'static str {
        SIGN_IN_ENDPOINT
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        AUTH_RESPONSE_LIMIT
    }
    pub fn firebase_api_key(&self) -> &str {
        self.profile.firebase_api_key()
    }
    pub fn email(&self) -> &str {
        self.email
    }
    pub fn body(&self) -> SecretRequestBody {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Body<'a> {
            email: &'a str,
            password: &'a str,
            return_secure_token: bool,
        }
        SecretRequestBody(
            serde_json::to_vec(&Body {
                email: self.email,
                password: self.password,
                return_secure_token: true,
            })
            .expect("a struct containing strings and a boolean always serializes"),
        )
    }
}

/// Privileged adapter input for Firebase refresh-token exchange.
pub struct RefreshCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) refresh_token: &'a str,
}

impl RefreshCall<'_> {
    pub const fn endpoint(&self) -> &'static str {
        REFRESH_ENDPOINT
    }
    pub const fn content_type(&self) -> &'static str {
        "application/x-www-form-urlencoded"
    }
    pub const fn response_limit(&self) -> usize {
        AUTH_RESPONSE_LIMIT
    }
    pub fn firebase_api_key(&self) -> &str {
        self.profile.firebase_api_key()
    }
    pub fn body(&self) -> SecretRequestBody {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "refresh_token")
            .append_pair("refresh_token", self.refresh_token)
            .finish();
        SecretRequestBody(body.into_bytes())
    }
}

/// Privileged adapter input for the one logical project-directory operation.
pub struct ProjectListCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) status: ProjectStatus,
    pub(crate) authority: CallAuthority<'a>,
}

impl ProjectListCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::ListUserProjects;

    crate::gateway_compat::gateway_common_accessors! {
        gateway_origin: normal,
        content_type: const,
        gateway_api_key: normal,
        client_id: const,
        bearer_token: normal,
        canonical_email: normal,
        canonical_uid: normal,
        device_id: normal,
    }
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        PROJECT_RESPONSE_LIMIT
    }
    /// Headers for fresh host-minted correlation values on this request.
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub const fn status(&self) -> ProjectStatus {
        self.status
    }

    pub fn url(&self) -> String {
        format!(
            "{}{}?status={}",
            self.gateway_origin(),
            self.path(),
            self.status.query_value()
        )
    }
}

/// Privileged adapter input for one projected transformer snapshot.
///
/// The call fixes the origin, method, path, action, projection, and one-item
/// array shape. It deliberately has no arbitrary URL, header, or JSON escape.
pub struct TransformerContextCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) ds_project: &'a str,
    pub(crate) transformer: &'a str,
    pub(crate) fields: TransformerContextProjection,
    pub(crate) authority: CallAuthority<'a>,
}

/// Fixed server projections for a transformer read. The raw saved projection
/// is reserved for report content-digest verification; normal callers retain
/// the governed `full` projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransformerContextProjection {
    Full,
    Saved,
    Process,
}

impl TransformerContextProjection {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Full => TRANSFORMER_CONTEXT_FIELDS,
            Self::Saved => crate::profile::TRANSFORMER_CONTEXT_SAVED_FIELDS,
            Self::Process => "process",
        }
    }
}

impl TransformerContextCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DsSystemReport;

    pub const fn projection(&self) -> TransformerContextProjection {
        self.fields
    }
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

    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }

    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        TRANSFORMER_CONTEXT_RESPONSE_LIMIT
    }

    pub const fn timeout_seconds(&self) -> u64 {
        TRANSFORMER_CONTEXT_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(crate::transformers::context_body(
            self.ds_project,
            self.transformer,
            self.fields,
        ))
    }
}

/// Privileged adapter input for one selected project's project-form catalogue.
///
/// The call fixes the origin, method, path, action, and body keys. The host
/// supplies only its audience-fenced selected project; there is no arbitrary
/// URL, header, body, action, or project override.
pub struct ProjectFormsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) ds_project: &'a str,
    pub(crate) authority: CallAuthority<'a>,
}

/// Privileged adapter input for one selected-project form settings editor.
///
/// Origin, method, path, action and body keys are fixed. A host may supply
/// only the audience-fenced selected project and one bounded form slug.
pub struct ProjectFormEditorCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) ds_project: &'a str,
    pub(crate) form_slug: &'a str,
    pub(crate) authority: CallAuthority<'a>,
}

impl ProjectFormEditorCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::ProjectForms;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        PROJECT_FORMS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        PROJECT_FORMS_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        #[derive(Serialize)]
        struct Body<'a> {
            action: &'static str,
            project_id: &'a str,
            form_slug: &'a str,
        }
        SecretRequestBody(
            serde_json::to_vec(&Body {
                action: PROJECT_FORM_EDITOR_ACTION,
                project_id: self.ds_project,
                form_slug: self.form_slug,
            })
            .expect("a struct containing only strings always serializes"),
        )
    }
}

impl ProjectFormsCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::ProjectForms;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        PROJECT_FORMS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        PROJECT_FORMS_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        #[derive(Serialize)]
        struct Body<'a> {
            action: &'static str,
            project_id: &'a str,
        }
        SecretRequestBody(
            serde_json::to_vec(&Body {
                action: PROJECT_FORMS_ACTION,
                project_id: self.ds_project,
            })
            .expect("a struct containing only strings always serializes"),
        )
    }
}

/// Privileged adapter input for one governed selected-project Solar snapshot.
///
/// The host supplies only an audience-fenced project id and one canonical city
/// id. The core derives the project Solar root and fixes the action, method,
/// path, and body keys; there is no global root or generic request escape.
pub struct SolarSnapshotCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) ds_project: &'a str,
    pub(crate) template_id: &'a str,
    pub(crate) authority: CallAuthority<'a>,
}

impl SolarSnapshotCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::SolarAction;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        SOLAR_SNAPSHOT_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        SOLAR_SNAPSHOT_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(crate::solar_snapshot::request_body(
            self.ds_project,
            self.template_id,
        ))
    }
}

/// Privileged adapter input for one bounded aggregate over a selected
/// project's governed survey form. The request grammar is typed by the core;
/// origin, method, path and project identity have no caller-controlled escape.
pub struct SurveyQueryCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) ds_project: &'a str,
    pub(crate) query: &'a SurveyQueryRequest,
    pub(crate) authority: CallAuthority<'a>,
}

impl SurveyQueryCall<'_> {
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
    pub const fn path(&self) -> &'static str {
        SURVEY_QUERY_PATH
    }
    pub const fn method(&self) -> &'static str {
        SURVEY_QUERY_METHOD
    }

    pub const fn response_limit(&self) -> usize {
        SURVEY_QUERY_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        SURVEY_QUERY_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(self.query.body_bytes(self.ds_project))
    }
}

/// Privileged adapter input for one bounded spatial entry selection. Project
/// identity is selected by the host and the core owns the only request shape;
/// there is no generic action, cursor, projection, deletion, or authority seam.
pub struct SurveyEntriesSelectCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) ds_project: &'a str,
    pub(crate) request: &'a SurveyEntriesSelectRequest,
    pub(crate) authority: CallAuthority<'a>,
}

impl SurveyEntriesSelectCall<'_> {
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
    pub const fn path(&self) -> &'static str {
        SURVEY_ENTRIES_SELECT_PATH
    }
    pub const fn method(&self) -> &'static str {
        SURVEY_ENTRIES_SELECT_METHOD
    }

    pub const fn response_limit(&self) -> usize {
        SURVEY_ENTRIES_SELECT_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        SURVEY_ENTRIES_SELECT_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(self.request.body_bytes(self.ds_project))
    }
}

/// Privileged adapter input for one immutable-fence page of coalesced Survey
/// mirror changes. The selected project and fixed route cannot be overridden;
/// the continuation remains opaque to both host and core.
pub struct SurveyEntriesChangesCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) ds_project: &'a str,
    pub(crate) request: &'a SurveyEntriesChangesRequest,
    pub(crate) authority: CallAuthority<'a>,
}

impl SurveyEntriesChangesCall<'_> {
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
    pub const fn path(&self) -> &'static str {
        SURVEY_ENTRIES_CHANGES_PATH
    }
    pub const fn method(&self) -> &'static str {
        SURVEY_ENTRIES_CHANGES_METHOD
    }

    pub const fn response_limit(&self) -> usize {
        SURVEY_ENTRIES_CHANGES_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        SURVEY_ENTRIES_CHANGES_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(self.request.body_bytes(self.ds_project))
    }
}

/// Privileged adapter input for one governed, create-only Survey entry write.
/// Project identity comes from the host's audience-fenced selection; the core
/// fixes the route, method, operation, request keys, and response bounds.
pub struct SurveyEntryCreateCall<'a> {
    pub(crate) direct: &'a ds_command_kernel::survey_entry_create::direct::DirectPlan,
    pub(crate) authority: CallAuthority<'a>,
}

impl SurveyEntryCreateCall<'_> {
    pub fn origin(&self) -> &str {
        self.direct.origin
    }
    pub fn path(&self) -> &str {
        &self.direct.path
    }
    pub const fn method(&self) -> &'static str {
        SURVEY_ENTRY_CREATE_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        SURVEY_ENTRY_CREATE_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        SURVEY_ENTRY_CREATE_TIMEOUT_SECONDS
    }
    pub fn bearer_token(&self) -> &str {
        self.authority.bearer_token
    }
    pub fn canonical_email(&self) -> &str {
        self.authority.canonical_email
    }
    pub fn canonical_uid(&self) -> &str {
        self.authority.canonical_uid
    }
    pub fn device_id(&self) -> Option<&str> {
        self.authority.device_id
    }
    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            serde_json::to_vec(&self.direct.body).expect("admitted direct create body"),
        )
    }
}

/// Privileged adapter input for one fixed selected-project tile operation.
///
/// The core owns the route and the complete request grammar. Callers can only
/// choose survey or design tiles and, for generation, the explicit force bit;
/// there is no generic tile action or body escape hatch.
pub struct ProjectDataCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: &'a serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

/// One fixed design-intake request. The core supplies the entire body.
pub struct StatusProcessingCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: &'a serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl StatusProcessingCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DsSystemProcess;

    crate::gateway_compat::gateway_common_accessors! {
        gateway_origin: normal,
        content_type: const,
        correlation_headers: const,
        gateway_api_key: normal,
        client_id: const,
        bearer_token: normal,
        canonical_email: normal,
    }
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        crate::STATUS_PROCESSING_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        crate::STATUS_PROCESSING_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            serde_json::to_vec(self.payload).expect("closed status processing request"),
        )
    }
}

pub struct SurveyPhotoCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: &'a serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl SurveyPhotoCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::MediaAction;

    crate::gateway_compat::gateway_common_accessors! {
        gateway_origin: normal,
        content_type: const,
        correlation_headers: const,
        gateway_api_key: normal,
        client_id: const,
        bearer_token: normal,
        canonical_email: normal,
    }
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        crate::survey_photo::RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        120
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(self.payload).expect("closed survey photo request"))
    }
}

impl ProjectDataCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::ProjectDataAction;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        crate::LAYERS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(self.payload).expect("closed GIS request"))
    }
}

pub struct SurveyControlCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) route: crate::survey_control::SurveyRoute,
    pub(crate) authority: CallAuthority<'a>,
}

impl SurveyControlCall<'_> {
    pub(crate) fn generated_operation(&self) -> Option<ds_gateway_client::Operation> {
        self.route.generated_operation()
    }

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
    pub fn path(&self) -> String {
        self.route.path()
    }
    pub fn method(&self) -> &'static str {
        self.route.method()
    }

    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed Survey request"))
    }
}

/// One authenticated request to the fixed Solar reference producer.
/// Only the host transport sees this value; no caller supplies an origin.
pub struct SolarReferenceCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) authority: CallAuthority<'a>,
    pub(crate) request: &'a ds_solar_contracts::reference_request::ReferenceRequest,
}
impl SolarReferenceCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub fn bearer_token(&self) -> &str {
        self.authority.bearer_token
    }
    pub fn request(&self) -> &ds_solar_contracts::reference_request::ReferenceRequest {
        self.request
    }
}

pub struct FeedbackCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl FeedbackCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::FeedbackAction;

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
    pub fn path(&self) -> String {
        Self::GATEWAY_OPERATION.path_template().into()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed feedback request"))
    }
}

pub struct SharedAssetsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl SharedAssetsCall<'_> {
    pub(crate) fn generated_operation(&self) -> ds_gateway_client::Operation {
        if self.semantic_commit_route().is_some() {
            ds_gateway_client::Operation::DataInterop
        } else {
            ds_gateway_client::Operation::ProjectAssetsAction
        }
    }

    fn semantic_commit_route(&self) -> Option<&'static crate::GatewayOperation> {
        (self.payload["action"] == "finalize_asset_semantic").then(|| {
            crate::gateway_operation("domains.assets.semantic_finalize")
                .expect("generated closed semantic derivative Bulk route")
        })
    }
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
    pub fn path(&self) -> String {
        self.generated_operation().path_template().into()
    }
    pub fn method(&self) -> &'static str {
        self.generated_operation().method().as_str()
    }

    pub fn response_limit(&self) -> usize {
        self.semantic_commit_route()
            .map_or(8 * 1024 * 1024, |route| route.response_cap_bytes as usize)
    }
    pub fn timeout_seconds(&self) -> u64 {
        self.semantic_commit_route()
            .map_or(TILE_TIMEOUT_SECONDS, |route| u64::from(route.timeout_s))
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed shared asset request"))
    }
}

/// `POST /api/v1/pm` — the project-management action endpoint.
///
/// The path is hard-coded here, like `/api/v1/assets` and `/api/v1/governance`
/// beside it: the profile supplies the gateway ORIGIN and the credentials, not
/// the route. Declaring a path in the profile would bump
/// `CLIENT_PROFILE_SCHEMA` and strand every installed `ds` until a package is
/// cut, and PM needs nothing of the sort — ds-brain's PM handler already
/// authenticates from the bearer, with no pairing and no device.
pub struct ProjectManagementCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl ProjectManagementCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::PmAction;

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
    pub fn path(&self) -> String {
        Self::GATEWAY_OPERATION.path_template().into()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    /// A whole plan with its tasks, records and residuals. Bounded like the
    /// shared-assets read rather than like a tile.
    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub fn timeout_seconds(&self) -> u64 {
        if self.payload["action"] == "download_attachment_url" {
            crate::asset_preview_source::TIMEOUT_SECONDS
        } else {
            30
        }
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            serde_json::to_vec(&self.payload).expect("closed project management request"),
        )
    }
}

pub struct MemberFormGrantCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl MemberFormGrantCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::ProjectsAccessAction;

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
    pub fn path(&self) -> String {
        Self::GATEWAY_OPERATION.path_template().into()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    /// One finite grant snapshot and exact receipt, capped at 128 KiB.
    pub const fn response_limit(&self) -> usize {
        crate::member_form_grants::MAX_BYTES
    }
    pub const fn timeout_seconds(&self) -> u64 {
        30
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            serde_json::to_vec(&self.payload).expect("closed member form grant request"),
        )
    }
}

/// `POST /api/v1/projects` — create one project or edit one project's
/// properties.
///
/// Hard-coded like `/api/v1/pm` and `/api/v1/governance` beside it, and for
/// the same reason: the profile supplies the origin and the credentials, not
/// the route, and a path in the profile would bump `CLIENT_PROFILE_SCHEMA`.
/// ds-brain's `Authentication` middleware admits a restored user AND a device
/// credential here, and `project.create` / `project.properties.edit` on the
/// principal decides the answer either way.
pub struct ProjectPropertiesCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl ProjectPropertiesCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::ProjectsAction;

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
    pub fn path(&self) -> String {
        Self::GATEWAY_OPERATION.path_template().into()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    /// One project document's receipt: far smaller than a tile or a plan.
    pub const fn response_limit(&self) -> usize {
        crate::project_properties::MAX_RESPONSE_BYTES
    }
    /// Creation provisions a dataset and copies template forms before it
    /// answers, so this waits longer than a read.
    pub const fn timeout_seconds(&self) -> u64 {
        60
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            serde_json::to_vec(&self.payload).expect("closed project properties request"),
        )
    }
}

/// `POST /api/v1/projects` — one governed whole-project delete action
/// (`delete_plan` … `delete_apply`), owner-only at ds-brain. Hard-coded like
/// [`ProjectPropertiesCall`]; it differs only in its bounds: an export page
/// and an apply are larger and slower than a properties receipt.
pub struct ProjectDeletionCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl ProjectDeletionCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::ProjectsAction;

    crate::gateway_compat::gateway_common_accessors! {
        gateway_origin: normal,
        content_type: const,
        gateway_api_key: normal,
        client_id: const,
        bearer_token: normal,
        canonical_email: normal,
        canonical_uid: normal,
        device_id: normal,
    }
    pub fn path(&self) -> String {
        Self::GATEWAY_OPERATION.path_template().into()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        crate::project_deletion::MAX_RESPONSE_BYTES
    }
    pub const fn timeout_seconds(&self) -> u64 {
        crate::project_deletion::TIMEOUT_SECONDS
    }

    /// The action this call sends, for host diagnostics.
    pub fn action(&self) -> &str {
        self.payload["action"].as_str().unwrap_or("")
    }
    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed project delete request"))
    }
}

/// `POST /api/v1/auth/device/approve` — decide one headless device
/// authorization under a Firebase USER principal.
///
/// Hard-coded like `/api/v1/pm` and `/api/v1/governance` beside it, and for
/// the same reason: the profile supplies the origin and the credentials, not
/// the route, and a path in the profile would bump `CLIENT_PROFILE_SCHEMA`.
/// ds-brain's `FirebaseAuthentication` middleware admits only a Firebase user
/// id token here and refuses a device credential with
/// `DESKTOP_PRINCIPAL_REQUIRED`; the bearer this call carries is the restored
/// session's id token, the same principal a Desktop window holds. The deadline
/// and cap are the registry row's for `domains.auth.device.approve`.
pub struct DeviceApproveCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl DeviceApproveCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DecideDeviceAuthorization;

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
    pub fn path(&self) -> String {
        Self::GATEWAY_OPERATION.path_template().into()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        DEVICE_APPROVE_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        30
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            serde_json::to_vec(&self.payload).expect("closed device approval request"),
        )
    }
}

/// The registry's cap for `domains.auth.device.approve`; a receipt is a few
/// hundred bytes.
pub const DEVICE_APPROVE_RESPONSE_LIMIT: usize = 1024 * 1024;

pub struct DesignVersionsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl DesignVersionsCall<'_> {
    pub(crate) fn generated_operation(&self) -> ds_gateway_client::Operation {
        if self.import_route().is_some() {
            ds_gateway_client::Operation::DataInterop
        } else {
            ds_gateway_client::Operation::DesignVersionsAction
        }
    }

    // Only these closed kernel commands use fixed Bulk operations. The remaining
    // version calls retain the admitted metadata-only Run route.
    fn import_route(&self) -> Option<&'static crate::GatewayOperation> {
        let operation = match self.payload["action"].as_str() {
            Some("import_transformer_version") => "domains.design_versions.import",
            Some("publish_transformer_version_revision") => {
                "domains.design_versions.publish_revision"
            }
            Some("get_transformer_version_revision") => "domains.design_versions.read_revision",
            Some("promote_transformer_version_revision") => {
                "domains.design_versions.promote_revision"
            }
            _ => return None,
        };
        Some(crate::gateway_operation(operation).expect("generated closed history Bulk route"))
    }
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
    pub fn path(&self) -> String {
        self.generated_operation().path_template().into()
    }
    pub fn method(&self) -> &'static str {
        self.generated_operation().method().as_str()
    }

    pub fn response_limit(&self) -> usize {
        self.import_route()
            .map_or(64 * 1024 * 1024, |route| route.response_cap_bytes as usize)
    }
    pub fn timeout_seconds(&self) -> u64 {
        self.import_route()
            .map_or(TILE_TIMEOUT_SECONDS, |route| u64::from(route.timeout_s))
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed design version request"))
    }
}

pub struct DesignAttachmentsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl DesignAttachmentsCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DesignAttachmentsAction;

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
    pub fn path(&self) -> String {
        Self::GATEWAY_OPERATION.path_template().into()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            serde_json::to_vec(&self.payload).expect("closed design attachment request"),
        )
    }
}

pub struct DesignTagsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl DesignTagsCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DesignAnnotationsAction;

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
    pub fn path(&self) -> String {
        Self::GATEWAY_OPERATION.path_template().into()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed design tag request"))
    }
}

pub struct DesignMigrationCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl DesignMigrationCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DataInterop;

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
    /// The design data plane, the same door the transformer copy path this
    /// endpoint extends already answers on.
    pub fn path(&self) -> String {
        Self::GATEWAY_OPERATION.path_template().into()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            serde_json::to_vec(&self.payload).expect("closed design migration request"),
        )
    }
}

/// `POST /api/v1/pipeline` `action=migrate` — survey data from a source
/// project into an explicit destination.
///
/// Hard-coded like `/api/v1/pm`: the profile supplies the gateway origin and
/// the credentials, not the route, and a path in the profile would bump
/// `CLIENT_PROFILE_SCHEMA`. The only body is the one
/// `crate::survey_migration::Command` writes.
pub struct SurveyMigrationCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl SurveyMigrationCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DataPipeline;

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
    pub fn path(&self) -> String {
        Self::GATEWAY_OPERATION.path_template().into()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        crate::survey_migration::MAX_RESPONSE_BYTES
    }
    pub const fn timeout_seconds(&self) -> u64 {
        crate::survey_migration::TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            serde_json::to_vec(&self.payload).expect("closed survey migration request"),
        )
    }
}

pub struct ReportArtifactCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl ReportArtifactCall<'_> {
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
    pub fn path(&self) -> String {
        "/api/v1/report_artifacts".into()
    }
    pub fn method(&self) -> &'static str {
        "POST"
    }

    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            serde_json::to_vec(&self.payload).expect("closed MV print attachment request"),
        )
    }
}

pub struct GridModelsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl GridModelsCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::ProjectGridModelsAction;

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
    pub fn path(&self) -> String {
        Self::GATEWAY_OPERATION.path_template().into()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed grid model request"))
    }
}

pub struct GlobalTilesCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl GlobalTilesCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::TilesAction;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        TILE_RESPONSE_LIMIT
    }
    /// A publication job is accepted, not awaited, so this is the catalog's
    /// read timeout rather than a build's.
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed global tile request"))
    }
}

/// One exact read of a national administrative hierarchy.
///
/// The country is in the path and the read is in the query, and both are built
/// by [`crate::admin_bounds::Command`] from closed tokens and ASCII digits — so
/// this call carries no body, no caller URL and nothing to escape.
pub struct AdminBoundsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) query: String,
    pub(crate) authority: CallAuthority<'a>,
}

impl AdminBoundsCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::GetRwandaAdmin;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }
    pub fn url(&self) -> String {
        format!("{}{}?{}", self.gateway_origin(), self.path(), self.query)
    }

    pub const fn response_limit(&self) -> usize {
        ADMIN_BOUNDS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        ADMIN_BOUNDS_TIMEOUT_SECONDS
    }
}

/// The platform reliability top line.
///
/// No body, no parameters and no project: the whole request is a path, and the
/// restored user's `platform.admin` capability is what decides the answer.
pub struct SreOverviewCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) authority: CallAuthority<'a>,
}

impl SreOverviewCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::SreOverview;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }
    pub fn url(&self) -> String {
        format!("{}{}", self.gateway_origin(), self.path())
    }

    pub const fn response_limit(&self) -> usize {
        SRE_OVERVIEW_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        SRE_OVERVIEW_TIMEOUT_SECONDS
    }
}

/// One bounded newest-first window of platform request events.
///
/// The answer is NDJSON rather than one object, so a host reads it to the
/// response limit and hands the bytes back whole; the line grammar and its
/// closing summary belong to [`crate::sre`], not to a transport.
pub struct SreEventsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl SreEventsCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DataInterop;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }
    pub const fn action(&self) -> &'static str {
        SRE_EVENTS_ACTIONS[0]
    }

    /// What the caller must accept. The route streams one JSON object per line
    /// and closes with a summary, so a host that asked for `application/json`
    /// would be asking for a document this route never produces.
    pub const fn accept(&self) -> &'static str {
        "application/x-ndjson"
    }
    pub const fn response_limit(&self) -> usize {
        SRE_EVENTS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        SRE_EVENTS_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed sre event request"))
    }
}

/// One bounded read of a survey form's entries, as the map reads them.
///
/// The route streams one GeoJSON feature per line and closes with a summary;
/// the transport keeps the whole body under the response limit and hands it
/// back, and [`crate::survey_entries_read`] owns the line grammar.
pub struct SurveyEntriesReadCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl SurveyEntriesReadCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DataInterop;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn accept(&self) -> &'static str {
        "application/x-ndjson"
    }
    pub const fn response_limit(&self) -> usize {
        crate::survey_entries_read::SURVEY_ENTRIES_READ_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        crate::survey_entries_read::SURVEY_ENTRIES_READ_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed survey entries request"))
    }
}

/// `POST /report` `media_scope`: one grant per project whose survey photos a
/// headless reader fetches. Hard-coded like the other `/report` calls; the
/// profile supplies origin and credentials, not the route.
pub struct SurveyMediaGrantCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl SurveyMediaGrantCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DsSystemReport;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        crate::survey_media_fetch::GRANT_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        crate::survey_media_fetch::GRANT_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed media grant request"))
    }
}

/// `GET` one survey photo through a media grant's resolver link. The link is
/// the only authority: no user credential, API key or identity header may
/// accompany it. The resolver answers with one redirect to a short-lived
/// storage signature, which an implementation follows (and only that one).
pub struct SurveyMediaBytesCall<'a> {
    pub(crate) url: &'a str,
}

impl SurveyMediaBytesCall<'_> {
    pub fn url(&self) -> &str {
        self.url
    }
    pub const fn max_redirects(&self) -> u32 {
        1
    }
    pub const fn response_limit(&self) -> usize {
        crate::survey_media_fetch::MAX_PHOTO_BYTES
    }
    pub const fn timeout_seconds(&self) -> u64 {
        crate::survey_media_fetch::PHOTO_TIMEOUT_SECONDS
    }
}

/// The installation inventory and its two governed transitions.
///
/// One route, one closed action vocabulary, and no project: an installation
/// belongs to the product. `platform.admin` or `app.admin` on the restored
/// user is what decides whether there is an answer.
pub struct InstallsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl InstallsCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::Governance;

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
    pub fn path(&self) -> String {
        Self::GATEWAY_OPERATION.path_template().into()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        30
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            serde_json::to_vec(&self.payload).expect("closed installation inventory request"),
        )
    }
}

pub struct GridCatalogCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl GridCatalogCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::GridCatalogAction;

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
    pub fn path(&self) -> String {
        Self::GATEWAY_OPERATION.path_template().into()
    }
    pub fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    /// The reviewed descriptor's cap for this operation.
    pub const fn response_limit(&self) -> usize {
        1024 * 1024
    }
    /// The reviewed descriptor's timeout for this operation.
    pub const fn timeout_seconds(&self) -> u64 {
        30
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed grid catalog request"))
    }
}

pub struct SolarProjectCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) artifacts: bool,
    pub(crate) authority: CallAuthority<'a>,
}

impl SolarProjectCall<'_> {
    pub(crate) fn generated_operation(&self) -> ds_gateway_client::Operation {
        if self.artifacts {
            ds_gateway_client::Operation::ComputeArtifactsAction
        } else {
            ds_gateway_client::Operation::SolarAction
        }
    }

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
    pub fn path(&self) -> String {
        self.generated_operation().path_template().into()
    }
    pub fn method(&self) -> &'static str {
        self.generated_operation().method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed Solar project request"))
    }
}

pub struct StylesCall<'a> {
    pub(crate) history: Option<&'a ds_command_kernel::style_history::Request>,
    pub(crate) governance: Option<&'a ds_command_kernel::style_governance::Request>,
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: &'a serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl StylesCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::StylesAction;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        crate::LAYERS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    /// The kernel builds the body it validated: one owner for what a publish
    /// may carry. `authorized::style_edit` admits the payload before the call,
    /// so a refusal here would be a contract the client broke itself.
    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            serde_json::to_vec(&match (self.history, self.governance) {
                (Some(request), None) => ds_command_kernel::style_history::request_body(request)
                    .expect("admitted history request"),
                (None, Some(request)) => ds_command_kernel::style_governance::request_body(request)
                    .expect("admitted governance request"),
                (None, None) => ds_command_kernel::style_document::publish_body(self.payload)
                    .expect("admitted style payload"),
                (Some(_), Some(_)) => unreachable!("one style operation per captured call"),
            })
            .expect("closed style payload"),
        )
    }
}

pub struct LayersCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) ds_project: &'a str,
    pub(crate) operation: crate::layers::LayerOperation<'a>,
    pub(crate) authority: CallAuthority<'a>,
}

impl LayersCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::LayersAction;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        self.operation.response_limit()
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(self.operation.body(self.ds_project))
    }
}

pub struct PrintingCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) ds_project: &'a str,
    pub(crate) operation: &'a crate::PrintingRequest,
    pub(crate) authority: CallAuthority<'a>,
}

impl PrintingCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::PrintingAction;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        crate::PRINTING_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(self.operation.body(self.ds_project))
    }
}

/// Saved Transformer Status selections. The core owns the route, the closed
/// action vocabulary, the request grammar and the response bound; ds-brain owns
/// membership, versioning and every capability check.
pub struct DesignSelectionsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) ds_project: &'a str,
    pub(crate) operation: &'a crate::DesignSelectionRequest,
    pub(crate) authority: CallAuthority<'a>,
}

impl DesignSelectionsCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DesignSelectionsAction;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }
    pub const fn action(&self) -> &'static str {
        self.operation.action()
    }

    pub const fn response_limit(&self) -> usize {
        crate::DESIGN_SELECTIONS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(self.operation.body(self.ds_project))
    }
}

/// Privileged adapter input for one fixed selected-project data-distribution
/// action: the reference catalogue, or one bounded derived print-context
/// acquisition. The core owns the route, the closed two-action vocabulary, the
/// request grammar, the response bound and the gateway's 180-second deadline;
/// ds-brain rechecks project membership on every call.
pub struct DataDistributionCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) ds_project: &'a str,
    pub(crate) operation: &'a crate::DataDistributionRequest,
    pub(crate) authority: CallAuthority<'a>,
}

impl DataDistributionCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DataDistributionAction;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }
    pub const fn action(&self) -> &'static str {
        self.operation.action()
    }

    pub const fn response_limit(&self) -> usize {
        crate::DATA_DISTRIBUTION_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        crate::DATA_DISTRIBUTION_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(self.operation.body(self.ds_project))
    }
}

/// One national reference-bundle fetch: an unauthenticated `GET` of a public
/// object on the one bundle host, admitted by [`crate::download_bundle`] only
/// when the streamed bytes carry the catalogue row's exact size and SHA-256.
///
/// This is deliberately not a gateway operation and carries no DS credential
/// (no bearer, no api key, no user email, no app id): the object is public,
/// and attaching a credential to a third-party origin would widen what that
/// credential reaches. The URL is validated here, once, before any socket.
/// It is `Copy` so a retried download sends the same admitted call again.
#[derive(Clone, Copy)]
pub struct BundleDownloadCall<'a> {
    url: &'a str,
    expected_sha256: &'a str,
    expected_bytes: u64,
}

impl<'a> BundleDownloadCall<'a> {
    /// Admit one catalogue row's bundle locator. Refuses anything but an
    /// unsigned `https://storage.googleapis.com/<object>` URL, a lowercase
    /// 64-hex digest, and a positive size within [`crate::BUNDLE_DOWNLOAD_MAX_BYTES`].
    pub fn new(
        url: &'a str,
        expected_sha256: &'a str,
        expected_bytes: u64,
    ) -> Result<Self, crate::ClientError> {
        let invalid = |message: &'static str| {
            crate::ClientError::new(crate::ErrorKind::InvalidInput, message)
        };
        if url.is_empty() || url.len() > crate::BUNDLE_DOWNLOAD_MAX_URL_BYTES {
            return Err(invalid(
                "Reference bundle URL is empty or exceeds its bound",
            ));
        }
        let parsed =
            url::Url::parse(url).map_err(|_| invalid("Reference bundle URL is not a URL"))?;
        if parsed.scheme() != "https"
            || parsed.host_str() != Some(crate::BUNDLE_DOWNLOAD_HOST)
            || parsed.port().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path().len() < 2
            || parsed
                .path_segments()
                .is_some_and(|mut segments| segments.any(|s| s.is_empty() || s == "." || s == ".."))
            || parsed.as_str() != url
        {
            return Err(invalid(
                "Reference bundle URL must be one unsigned object on storage.googleapis.com",
            ));
        }
        if expected_sha256.len() != 64
            || !expected_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid(
                "Reference bundle digest is not a lowercase SHA-256",
            ));
        }
        if expected_bytes == 0 || expected_bytes > crate::BUNDLE_DOWNLOAD_MAX_BYTES {
            return Err(invalid(
                "Reference bundle size is zero or exceeds its bound",
            ));
        }
        Ok(Self {
            url,
            expected_sha256,
            expected_bytes,
        })
    }
    pub fn url(&self) -> &str {
        self.url
    }
    pub const fn method(&self) -> &'static str {
        "GET"
    }
    pub fn expected_sha256(&self) -> &str {
        self.expected_sha256
    }
    pub const fn expected_bytes(&self) -> u64 {
        self.expected_bytes
    }
    /// The most bytes a transport may hand the sink: one past the catalogue
    /// size, so an oversized object is observed and refused, never buffered.
    pub const fn response_limit(&self) -> u64 {
        self.expected_bytes.saturating_add(1)
    }
    /// A progress-derived ceiling, not a stall detector: a bundle of this size
    /// must complete at 256 KiB/s with two minutes of grace.
    pub const fn timeout_seconds(&self) -> u64 {
        120 + self.expected_bytes / (256 * 1024)
    }
}

pub struct TileCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) ds_project: &'a str,
    pub(crate) operation: TileOperation<'a>,
    pub(crate) authority: CallAuthority<'a>,
}

impl TileCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::TilesAction;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }

    pub const fn response_limit(&self) -> usize {
        TILE_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(self.operation.body(self.ds_project))
    }
}

/// Privileged adapter input for one fixed selected-project `/report` action.
///
/// The core owns the route, the closed action vocabulary, the request
/// grammar, the response bound, and the deadline. The compounded deliverable
/// additionally carries the fixed Fast processing-lane header; adapters send
/// it only when [`Self::processing_lane`] names one.
pub struct ProjectReportCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) ds_project: &'a str,
    pub(crate) operation: ProjectReportOperation<'a>,
    pub(crate) authority: CallAuthority<'a>,
}

impl ProjectReportCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DsSystemReport;

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
    pub const fn path(&self) -> &'static str {
        Self::GATEWAY_OPERATION.path_template()
    }
    pub const fn method(&self) -> &'static str {
        Self::GATEWAY_OPERATION.method().as_str()
    }
    pub const fn action(&self) -> &'static str {
        self.operation.action()
    }

    pub const fn response_limit(&self) -> usize {
        PROJECT_REPORT_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        self.operation.timeout_seconds()
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(self.operation.body(self.ds_project))
    }
}

/// The only network operations this crate can request.
///
/// This is a privileged boundary: implementations necessarily observe login
/// secrets and bearer proofs while writing the fixed requests. Ordinary client
/// callers never receive these call values. Implementations must disable HTTP
/// redirects so an exact validated origin can never hand credentials to a
/// second origin.
pub struct ProjectConfigurationCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) request: ds_command_kernel::gateway_exchange::Request,
    pub(crate) authority: CallAuthority<'a>,
}

impl ProjectConfigurationCall<'_> {
    pub(crate) fn generated_operation(&self) -> Option<ds_gateway_client::Operation> {
        match self.request.method {
            ds_command_kernel::gateway_exchange::Method::Post => {
                Some(ds_gateway_client::Operation::DsSystemReport)
            }
            _ => None,
        }
    }

    crate::gateway_compat::gateway_common_accessors! {
        gateway_origin: normal,
        content_type: normal,
        client_id: normal,
        gateway_api_key: normal,
        bearer_token: normal,
        canonical_email: normal,
    }
    pub fn path(&self) -> String {
        self.generated_operation().map_or_else(
            || self.request.path.clone(),
            |operation| operation.path_template().to_owned(),
        )
    }
    pub fn method(&self) -> &'static str {
        self.generated_operation().map_or_else(
            || self.request.method.as_str(),
            |operation| operation.method().as_str(),
        )
    }

    pub fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub fn timeout_seconds(&self) -> u64 {
        120
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            self.request
                .body
                .as_ref()
                .map(|body| serde_json::to_vec(body).expect("owning configuration request"))
                .unwrap_or_default(),
        )
    }
}

/// `GET|PATCH /config/{project}/known-columns` — the project's known-column
/// visibility (`ds design known-columns list|set`).
///
/// Hard-coded like `/config/{project}` beside it: the profile supplies the
/// origin and the credentials, not the route. A `PATCH` carries one
/// `{layer, field, visible, expected_revision}` patch, byte for byte as the
/// browser's `setProjectKnownColumn` sends it; a `GET` carries nothing.
pub struct KnownColumnsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) project: &'a str,
    pub(crate) patch: Option<&'a serde_json::Value>,
    pub(crate) authority: CallAuthority<'a>,
}

impl KnownColumnsCall<'_> {
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
    pub fn path(&self) -> String {
        format!(
            "/config/{}/known-columns",
            url::form_urlencoded::byte_serialize(self.project.as_bytes()).collect::<String>()
        )
    }
    pub fn method(&self) -> &'static str {
        if self.patch.is_some() {
            "PATCH"
        } else {
            "GET"
        }
    }

    pub const fn response_limit(&self) -> usize {
        4 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        60
    }

    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            self.patch
                .map(|patch| serde_json::to_vec(patch).expect("closed known-columns patch"))
                .unwrap_or_default(),
        )
    }
}

pub trait Transport {
    crate::__ds_gateway_calls!(declare);

    /// Read exact persisted analysis bytes; adapters must retain the response
    /// headers and obey the call's bounded response limit.
    fn transformer_analysis(
        &mut self,
        _call: crate::TransformerAnalysisCall<'_>,
    ) -> Result<crate::TransformerAnalysisResponse, crate::TransformerAnalysisTransportError> {
        Err(TransportError::Unreachable.into())
    }

    fn transformer_save(
        &mut self,
        _call: crate::TransformerSaveCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    /// One `ds` reliability event ([`crate::reporter`]); its bearer is optional.
    fn sre_client_event(
        &mut self,
        _call: crate::reporter::SreClientEventCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn design_attachment_upload(
        &mut self,
        _call: crate::design_attachments::UploadCall<'_>,
    ) -> Result<(), TransportError> {
        Err(TransportError::Unreachable)
    }

    fn survey_media_bytes(
        &mut self,
        _call: SurveyMediaBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn asset_preview_bytes(
        &mut self,
        _call: crate::asset_preview_source::DownloadCall<'_>,
        _writer: &mut dyn std::io::Write,
        _control: &dyn crate::asset_preview_source::TransferControl,
    ) -> Result<
        crate::asset_preview_source::DownloadReceipt,
        crate::asset_preview_source::TransferError,
    > {
        Err(crate::asset_preview_source::TransferError::Unreachable)
    }
    fn shared_asset_bytes(
        &mut self,
        _call: crate::shared_assets::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn grid_model_upload(
        &mut self,
        _call: crate::grid_models::publication::UploadCall<'_>,
    ) -> Result<(), TransportError> {
        Err(TransportError::Unreachable)
    }
    fn grid_model_bytes(
        &mut self,
        _call: crate::grid_models::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    /// One design attachment revision's bytes through its server-signed,
    /// generation-pinned read. No credential is sent to Storage.
    fn design_attachment_bytes(
        &mut self,
        _call: crate::design_attachments::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn solar_reference(
        &mut self,
        _call: SolarReferenceCall<'_>,
    ) -> ds_solar_contracts::SolarResult<ds_solar_contracts::BundleBytes> {
        Err(ds_solar_contracts::SolarError::weather_unavailable(
            "reference transport unavailable",
        ))
    }

    fn sign_in(&mut self, call: SignInCall<'_>) -> Result<TransportResponse, TransportError>;
    fn refresh(&mut self, call: RefreshCall<'_>) -> Result<TransportResponse, TransportError>;

    fn survey_entry_create(
        &mut self,
        call: SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, TransportError>;

    fn download_solar_artifact(
        &mut self,
        _call: crate::SolarArtifactDownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn download_solar_media(
        &mut self,
        _call: crate::SolarMediaDownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn download_survey_photo(
        &mut self,
        _call: crate::survey_photo::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn upload_bytes(
        &mut self,
        call: crate::UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError>;

    /// Stream one public bundle object into `sink`, writing at most
    /// [`BundleDownloadCall::response_limit`] bytes, with redirects disabled
    /// and no DS credential attached. Verification is the core's
    /// ([`crate::download_bundle`]), not the adapter's.
    fn download_bundle(
        &mut self,
        _call: BundleDownloadCall<'_>,
        _sink: &mut dyn Write,
    ) -> Result<(), TransportError> {
        Err(TransportError::Unreachable)
    }
}

/// Provider-neutral transport for the fixed authenticated domain operations.
///
/// Implementations receive closed call values only. They cannot choose a URL,
/// path, method, body shape, or arbitrary header, and must send the supplied
/// bearer as ordinary `Authorization: Bearer ...` without inferring that it is
/// a Firebase token.
pub trait AuthorizedTransport {
    crate::__ds_gateway_calls!(declare);

    /// Read exact persisted analysis bytes; adapters must retain the response
    /// headers and obey the call's bounded response limit.
    fn transformer_analysis(
        &mut self,
        _call: crate::TransformerAnalysisCall<'_>,
    ) -> Result<crate::TransformerAnalysisResponse, crate::TransformerAnalysisTransportError> {
        Err(TransportError::Unreachable.into())
    }

    fn transformer_save(
        &mut self,
        _call: crate::TransformerSaveCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn design_attachment_upload(
        &mut self,
        _call: crate::design_attachments::UploadCall<'_>,
    ) -> Result<(), TransportError> {
        Err(TransportError::Unreachable)
    }

    fn asset_preview_bytes(
        &mut self,
        _call: crate::asset_preview_source::DownloadCall<'_>,
        _writer: &mut dyn std::io::Write,
        _control: &dyn crate::asset_preview_source::TransferControl,
    ) -> Result<
        crate::asset_preview_source::DownloadReceipt,
        crate::asset_preview_source::TransferError,
    > {
        Err(crate::asset_preview_source::TransferError::Unreachable)
    }
    fn shared_asset_bytes(
        &mut self,
        _call: crate::shared_assets::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn grid_model_upload(
        &mut self,
        _call: crate::grid_models::publication::UploadCall<'_>,
    ) -> Result<(), TransportError> {
        Err(TransportError::Unreachable)
    }
    fn grid_model_bytes(
        &mut self,
        _call: crate::grid_models::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    /// One design attachment revision's bytes through its server-signed,
    /// generation-pinned read. No credential is sent to Storage.
    fn design_attachment_bytes(
        &mut self,
        _call: crate::design_attachments::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn solar_reference(
        &mut self,
        _call: SolarReferenceCall<'_>,
    ) -> ds_solar_contracts::SolarResult<ds_solar_contracts::BundleBytes> {
        Err(ds_solar_contracts::SolarError::weather_unavailable(
            "reference transport unavailable",
        ))
    }

    fn survey_entry_create(
        &mut self,
        call: SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, TransportError>;

    fn download_solar_artifact(
        &mut self,
        _call: crate::SolarArtifactDownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn download_solar_media(
        &mut self,
        _call: crate::SolarMediaDownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn download_survey_photo(
        &mut self,
        _call: crate::survey_photo::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn upload_bytes(
        &mut self,
        call: crate::UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
}

impl<T: Transport> AuthorizedTransport for T {
    crate::__ds_gateway_calls!(bridge);

    fn transformer_analysis(
        &mut self,
        call: crate::TransformerAnalysisCall<'_>,
    ) -> Result<crate::TransformerAnalysisResponse, crate::TransformerAnalysisTransportError> {
        Transport::transformer_analysis(self, call)
    }

    fn transformer_save(
        &mut self,
        call: crate::TransformerSaveCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::transformer_save(self, call)
    }

    fn design_attachment_upload(
        &mut self,
        call: crate::design_attachments::UploadCall<'_>,
    ) -> Result<(), TransportError> {
        Transport::design_attachment_upload(self, call)
    }

    fn asset_preview_bytes(
        &mut self,
        call: crate::asset_preview_source::DownloadCall<'_>,
        writer: &mut dyn std::io::Write,
        control: &dyn crate::asset_preview_source::TransferControl,
    ) -> Result<
        crate::asset_preview_source::DownloadReceipt,
        crate::asset_preview_source::TransferError,
    > {
        Transport::asset_preview_bytes(self, call, writer, control)
    }
    fn shared_asset_bytes(
        &mut self,
        call: crate::shared_assets::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::shared_asset_bytes(self, call)
    }
    fn grid_model_upload(
        &mut self,
        call: crate::grid_models::publication::UploadCall<'_>,
    ) -> Result<(), TransportError> {
        Transport::grid_model_upload(self, call)
    }
    fn grid_model_bytes(
        &mut self,
        call: crate::grid_models::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::grid_model_bytes(self, call)
    }
    fn design_attachment_bytes(
        &mut self,
        call: crate::design_attachments::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::design_attachment_bytes(self, call)
    }

    fn solar_reference(
        &mut self,
        call: SolarReferenceCall<'_>,
    ) -> ds_solar_contracts::SolarResult<ds_solar_contracts::BundleBytes> {
        Transport::solar_reference(self, call)
    }

    fn survey_entry_create(
        &mut self,
        call: SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::survey_entry_create(self, call)
    }

    fn download_solar_artifact(
        &mut self,
        call: crate::SolarArtifactDownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::download_solar_artifact(self, call)
    }
    fn download_solar_media(
        &mut self,
        call: crate::SolarMediaDownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::download_solar_media(self, call)
    }
    fn download_survey_photo(
        &mut self,
        call: crate::survey_photo::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::download_survey_photo(self, call)
    }

    fn upload_bytes(
        &mut self,
        call: crate::UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::upload_bytes(self, call)
    }
}
