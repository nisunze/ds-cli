use crate::project_report::ProjectReportOperation;
use crate::tiles::TileOperation;
use crate::{
    ClientProfile, ProjectStatus, SurveyEntriesChangesRequest, SurveyEntriesSelectRequest,
    SurveyEntryCreateRequest, SurveyQueryRequest, TransportError,
};
use serde::Serialize;
use std::io::Write;
use zeroize::Zeroize;

pub const SIGN_IN_ENDPOINT: &str =
    "https://identitytoolkit.googleapis.com/v1/accounts:signInWithPassword";
pub const REFRESH_ENDPOINT: &str = "https://securetoken.googleapis.com/v1/token";
pub use crate::profile::PROJECT_LIST_PATH;
pub use crate::profile::{DATA_DISTRIBUTION_METHOD, DATA_DISTRIBUTION_PATH};
pub use crate::profile::{
    PROJECT_FORMS_ACTION, PROJECT_FORMS_METHOD, PROJECT_FORMS_PATH, PROJECT_FORM_EDITOR_ACTION,
    SOLAR_SNAPSHOT_ACTION, SOLAR_SNAPSHOT_METHOD, SOLAR_SNAPSHOT_PATH,
    SURVEY_ENTRIES_CHANGES_METHOD, SURVEY_ENTRIES_CHANGES_PATH, SURVEY_ENTRIES_SELECT_METHOD,
    SURVEY_ENTRIES_SELECT_PATH, SURVEY_ENTRY_CREATE_METHOD, SURVEY_ENTRY_CREATE_PATH,
    SURVEY_QUERY_METHOD, SURVEY_QUERY_PATH, TILES_METHOD, TILES_PATH, TRANSFORMER_CONTEXT_ACTION,
    TRANSFORMER_CONTEXT_FIELDS, TRANSFORMER_CONTEXT_METHOD, TRANSFORMER_CONTEXT_PATH,
};
pub use crate::profile::{PROJECT_REPORT_METHOD, PROJECT_REPORT_PATH};
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
    fn entry(&self) -> &'static crate::GatewayOperation {
        crate::gateway_operation(self.request.operation().registry_id())
            .expect("the closed Sync Center operation is generated in the registry")
    }
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> &'static str {
        self.entry().path
    }
    pub fn method(&self) -> &'static str {
        self.entry().method
    }
    pub fn response_limit(&self) -> usize {
        self.entry().response_cap_bytes as usize
    }
    pub fn timeout_seconds(&self) -> u64 {
        u64::from(self.entry().timeout_s)
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
    }
    pub fn bearer_token(&self) -> &str {
        self.authority.bearer_token
    }
    pub fn canonical_email(&self) -> &str {
        self.authority.canonical_email
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        PROJECT_LIST_PATH
    }
    pub const fn method(&self) -> &'static str {
        "GET"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
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
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn url(&self) -> String {
        format!(
            "{}{}?status={}",
            self.gateway_origin(),
            PROJECT_LIST_PATH,
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
    pub(crate) authority: CallAuthority<'a>,
}

impl TransformerContextCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }

    pub const fn path(&self) -> &'static str {
        TRANSFORMER_CONTEXT_PATH
    }

    pub const fn method(&self) -> &'static str {
        TRANSFORMER_CONTEXT_METHOD
    }

    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }

    pub const fn response_limit(&self) -> usize {
        TRANSFORMER_CONTEXT_RESPONSE_LIMIT
    }

    pub const fn timeout_seconds(&self) -> u64 {
        TRANSFORMER_CONTEXT_TIMEOUT_SECONDS
    }

    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }

    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }

    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
        #[derive(Serialize)]
        struct Body<'a> {
            action: &'static str,
            eds_project_id: &'a str,
            transformers: [&'a str; 1],
            fields: &'static str,
        }
        SecretRequestBody(
            serde_json::to_vec(&Body {
                action: TRANSFORMER_CONTEXT_ACTION,
                eds_project_id: self.ds_project,
                transformers: [self.transformer],
                fields: TRANSFORMER_CONTEXT_FIELDS,
            })
            .expect("a struct containing only strings always serializes"),
        )
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        PROJECT_FORMS_PATH
    }
    pub const fn method(&self) -> &'static str {
        PROJECT_FORMS_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        PROJECT_FORMS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        PROJECT_FORMS_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        PROJECT_FORMS_PATH
    }
    pub const fn method(&self) -> &'static str {
        PROJECT_FORMS_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        PROJECT_FORMS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        PROJECT_FORMS_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        SOLAR_SNAPSHOT_PATH
    }
    pub const fn method(&self) -> &'static str {
        SOLAR_SNAPSHOT_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        SOLAR_SNAPSHOT_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        SOLAR_SNAPSHOT_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
        #[derive(Serialize)]
        struct Body<'a> {
            action: &'static str,
            root: String,
            template_id: &'a str,
        }
        SecretRequestBody(
            serde_json::to_vec(&Body {
                action: SOLAR_SNAPSHOT_ACTION,
                root: format!("eds_project/{}/eds_solar", self.ds_project),
                template_id: self.template_id,
            })
            .expect("a struct containing only strings always serializes"),
        )
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        SURVEY_QUERY_PATH
    }
    pub const fn method(&self) -> &'static str {
        SURVEY_QUERY_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        SURVEY_QUERY_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        SURVEY_QUERY_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        SURVEY_ENTRIES_SELECT_PATH
    }
    pub const fn method(&self) -> &'static str {
        SURVEY_ENTRIES_SELECT_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        SURVEY_ENTRIES_SELECT_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        SURVEY_ENTRIES_SELECT_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        SURVEY_ENTRIES_CHANGES_PATH
    }
    pub const fn method(&self) -> &'static str {
        SURVEY_ENTRIES_CHANGES_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        SURVEY_ENTRIES_CHANGES_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        SURVEY_ENTRIES_CHANGES_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
        SecretRequestBody(self.request.body_bytes(self.ds_project))
    }
}

/// Privileged adapter input for one governed, create-only Survey entry write.
/// Project identity comes from the host's audience-fenced selection; the core
/// fixes the route, method, operation, request keys, and response bounds.
pub struct SurveyEntryCreateCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) project_id: &'a str,
    pub(crate) request: &'a SurveyEntryCreateRequest,
    pub(crate) authority: CallAuthority<'a>,
}

impl SurveyEntryCreateCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        SURVEY_ENTRY_CREATE_PATH
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
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
        SecretRequestBody(self.request.body_bytes(self.project_id))
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

/// One fixed Standard design-intake request. The core supplies the entire body.
pub struct StatusProcessingCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: &'a serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl StatusProcessingCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        crate::STATUS_PROCESSING_PATH
    }
    pub const fn method(&self) -> &'static str {
        crate::STATUS_PROCESSING_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        crate::STATUS_PROCESSING_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        crate::STATUS_PROCESSING_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
    }
    pub fn bearer_token(&self) -> &str {
        self.authority.bearer_token
    }
    pub fn canonical_email(&self) -> &str {
        self.authority.canonical_email
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        crate::survey_photo::PATH
    }
    pub const fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        crate::survey_photo::RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        120
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
    }
    pub fn bearer_token(&self) -> &str {
        self.authority.bearer_token
    }
    pub fn canonical_email(&self) -> &str {
        self.authority.canonical_email
    }
    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(serde_json::to_vec(self.payload).expect("closed survey photo request"))
    }
}

impl ProjectDataCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        crate::PROJECT_DATA_PATH
    }
    pub const fn method(&self) -> &'static str {
        crate::PROJECT_DATA_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        crate::LAYERS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> String {
        self.route.path()
    }
    pub fn method(&self) -> &'static str {
        self.route.method()
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> String {
        "/api/v1/feedback".into()
    }
    pub fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed feedback request"))
    }
}

pub struct SharedAssetsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl SharedAssetsCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> String {
        "/api/v1/assets".into()
    }
    pub fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed shared asset request"))
    }
}

pub struct DesignVersionsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl DesignVersionsCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> String {
        crate::profile::DESIGN_VERSIONS_PATH.into()
    }
    pub fn method(&self) -> &'static str {
        crate::profile::DESIGN_VERSIONS_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        64 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed design version request"))
    }
}

pub struct DesignAttachmentsCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl DesignAttachmentsCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> String {
        crate::profile::DESIGN_ATTACHMENTS_PATH.into()
    }
    pub fn method(&self) -> &'static str {
        crate::profile::DESIGN_ATTACHMENTS_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> String {
        "/api/v1/design/annotations".into()
    }
    pub fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed design tag request"))
    }
}

pub struct ReportArtifactCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl ReportArtifactCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> String {
        "/api/v1/report_artifacts".into()
    }
    pub fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> String {
        "/api/v1/grid/models".into()
    }
    pub fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed grid model request"))
    }
}

pub struct GridCatalogCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl GridCatalogCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> String {
        "/api/v1/grid/catalog".into()
    }
    pub fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    /// The reviewed descriptor's cap for this operation.
    pub const fn response_limit(&self) -> usize {
        1024 * 1024
    }
    /// The reviewed descriptor's timeout for this operation.
    pub const fn timeout_seconds(&self) -> u64 {
        30
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> String {
        if self.artifacts {
            "/api/v1/compute_artifacts".into()
        } else {
            "/api/v1/solar".into()
        }
    }
    pub fn method(&self) -> &'static str {
        "POST"
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
        SecretRequestBody(serde_json::to_vec(&self.payload).expect("closed Solar project request"))
    }
}

pub struct StylesCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) payload: &'a serde_json::Value,
    pub(crate) authority: CallAuthority<'a>,
}

impl StylesCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        crate::STYLES_PATH
    }
    pub const fn method(&self) -> &'static str {
        crate::STYLES_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        crate::LAYERS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    /// The kernel builds the body it validated: one owner for what a publish
    /// may carry. `authorized::style_edit` admits the payload before the call,
    /// so a refusal here would be a contract the client broke itself.
    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            serde_json::to_vec(
                &ds_command_kernel::style_document::publish_body(self.payload)
                    .expect("admitted style payload"),
            )
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        crate::LAYERS_PATH
    }
    pub const fn method(&self) -> &'static str {
        crate::LAYERS_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        self.operation.response_limit()
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        crate::PRINTING_PATH
    }
    pub const fn method(&self) -> &'static str {
        crate::PRINTING_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        crate::PRINTING_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        crate::DESIGN_SELECTIONS_PATH
    }
    pub const fn method(&self) -> &'static str {
        crate::DESIGN_SELECTIONS_METHOD
    }
    pub const fn action(&self) -> &'static str {
        self.operation.action()
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        crate::DESIGN_SELECTIONS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        DATA_DISTRIBUTION_PATH
    }
    pub const fn method(&self) -> &'static str {
        DATA_DISTRIBUTION_METHOD
    }
    pub const fn action(&self) -> &'static str {
        self.operation.action()
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        crate::DATA_DISTRIBUTION_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        crate::DATA_DISTRIBUTION_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        TILES_PATH
    }
    pub const fn method(&self) -> &'static str {
        TILES_METHOD
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        TILE_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TILE_TIMEOUT_SECONDS
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn path(&self) -> &'static str {
        PROJECT_REPORT_PATH
    }
    pub const fn method(&self) -> &'static str {
        PROJECT_REPORT_METHOD
    }
    pub const fn action(&self) -> &'static str {
        self.operation.action()
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        PROJECT_REPORT_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        self.operation.timeout_seconds()
    }
    /// `Some("fast")` for the compounded deliverable, sent as the fixed
    /// `X-DS-Processing-Lane` header; `None` for lane-neutral actions.
    pub const fn processing_lane(&self) -> Option<&'static str> {
        self.operation.processing_lane()
    }
    pub const fn processing_lane_header(&self) -> &'static str {
        PROCESSING_LANE_HEADER
    }
    pub const fn correlation_headers(&self) -> [&'static str; 2] {
        CORRELATION_HEADERS
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub const fn client_id(&self) -> &'static str {
        self.profile.client_id()
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
    pub(crate) project: &'a str,
    pub(crate) settings: Option<&'a serde_json::Value>,
    pub(crate) sheet: &'a str,
    pub(crate) authority: CallAuthority<'a>,
}

impl ProjectConfigurationCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> String {
        if self.settings.is_some() {
            "/report".into()
        } else {
            format!(
                "/config/{}",
                url::form_urlencoded::byte_serialize(self.project.as_bytes()).collect::<String>()
            )
        }
    }
    pub fn method(&self) -> &'static str {
        if self.settings.is_some() {
            "POST"
        } else {
            "GET"
        }
    }
    pub fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub fn response_limit(&self) -> usize {
        8 * 1024 * 1024
    }
    pub fn timeout_seconds(&self) -> u64 {
        120
    }
    pub fn client_id(&self) -> &'static str {
        self.profile.client_id()
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub fn bearer_token(&self) -> &str {
        self.authority.bearer_token
    }
    pub fn canonical_email(&self) -> &str {
        self.authority.canonical_email
    }
    pub fn body(&self) -> SecretRequestBody {
        SecretRequestBody(
            self.settings
                .map(|rows| save_config_body(self.project, self.sheet, rows))
                .unwrap_or_default(),
        )
    }
}

/// The `/report` `save_config` request, byte for byte as the browser sends it
/// (`ds-web/src/lib/api/design.ts:2119-2125` through `wireProject`, which is
/// `eds_project_id`). One builder, so a native settings save and a browser
/// settings save cannot be two different requests; `expected_data` is the
/// browser's optional dirty-check and is not part of the shape itself.
pub(crate) fn save_config_body(
    project: &str,
    sheet: &str,
    rows: &impl serde::Serialize,
) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "action":"save_config","eds_project_id":project,"sheet_name":sheet,"data":rows
    }))
    .expect("JSON settings serialize")
}

pub trait Transport {
    fn transformer_save(
        &mut self,
        _call: crate::TransformerSaveCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn feedback(&mut self, _call: FeedbackCall<'_>) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn shared_assets(
        &mut self,
        _call: SharedAssetsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn design_attachments(
        &mut self,
        _call: DesignAttachmentsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn design_attachment_upload(
        &mut self,
        _call: crate::design_attachments::UploadCall<'_>,
    ) -> Result<(), TransportError> {
        Err(TransportError::Unreachable)
    }
    fn design_versions(
        &mut self,
        _call: DesignVersionsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn design_tags(
        &mut self,
        _call: DesignTagsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn report_artifact(
        &mut self,
        _call: ReportArtifactCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn grid_models(
        &mut self,
        _call: GridModelsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn grid_catalog(
        &mut self,
        _call: GridCatalogCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
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

    fn solar_reference(
        &mut self,
        _call: SolarReferenceCall<'_>,
    ) -> ds_solar_contracts::SolarResult<ds_solar_contracts::BundleBytes> {
        Err(ds_solar_contracts::SolarError::weather_unavailable(
            "reference transport unavailable",
        ))
    }

    fn sync_gateway(
        &mut self,
        _call: SyncGatewayCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn project_configuration(
        &mut self,
        _call: ProjectConfigurationCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn sign_in(&mut self, call: SignInCall<'_>) -> Result<TransportResponse, TransportError>;
    fn refresh(&mut self, call: RefreshCall<'_>) -> Result<TransportResponse, TransportError>;
    fn list_projects(
        &mut self,
        call: ProjectListCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn transformer_context(
        &mut self,
        call: TransformerContextCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn project_forms(
        &mut self,
        call: ProjectFormsCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn project_form_editor(
        &mut self,
        call: ProjectFormEditorCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn solar_snapshot(
        &mut self,
        call: SolarSnapshotCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn survey_query(
        &mut self,
        call: SurveyQueryCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn survey_entries_select(
        &mut self,
        call: SurveyEntriesSelectCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn survey_entries_changes(
        &mut self,
        call: SurveyEntriesChangesCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn survey_entry_create(
        &mut self,
        call: SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn project_data(
        &mut self,
        call: ProjectDataCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn survey_photo(
        &mut self,
        _call: SurveyPhotoCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
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
    fn status_processing(
        &mut self,
        _call: StatusProcessingCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn upload_bytes(
        &mut self,
        call: crate::UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn solar_project(
        &mut self,
        _call: SolarProjectCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn survey_control(
        &mut self,
        call: SurveyControlCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn styles(&mut self, call: StylesCall<'_>) -> Result<TransportResponse, TransportError>;
    fn layers(&mut self, call: LayersCall<'_>) -> Result<TransportResponse, TransportError>;
    fn printing(&mut self, _call: PrintingCall<'_>) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn design_selections(
        &mut self,
        _call: DesignSelectionsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn data_distribution(
        &mut self,
        _call: DataDistributionCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
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
    fn tiles(&mut self, call: TileCall<'_>) -> Result<TransportResponse, TransportError>;
    fn project_report(
        &mut self,
        call: ProjectReportCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
}

/// Provider-neutral transport for the fixed authenticated domain operations.
///
/// Implementations receive closed call values only. They cannot choose a URL,
/// path, method, body shape, or arbitrary header, and must send the supplied
/// bearer as ordinary `Authorization: Bearer ...` without inferring that it is
/// a Firebase token.
pub trait AuthorizedTransport {
    fn transformer_save(
        &mut self,
        _call: crate::TransformerSaveCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn feedback(&mut self, _call: FeedbackCall<'_>) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn shared_assets(
        &mut self,
        _call: SharedAssetsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn design_attachments(
        &mut self,
        _call: DesignAttachmentsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn design_attachment_upload(
        &mut self,
        _call: crate::design_attachments::UploadCall<'_>,
    ) -> Result<(), TransportError> {
        Err(TransportError::Unreachable)
    }
    fn design_versions(
        &mut self,
        _call: DesignVersionsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn design_tags(
        &mut self,
        _call: DesignTagsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }

    fn report_artifact(
        &mut self,
        _call: ReportArtifactCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn grid_models(
        &mut self,
        _call: GridModelsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn grid_catalog(
        &mut self,
        _call: GridCatalogCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
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

    fn solar_reference(
        &mut self,
        _call: SolarReferenceCall<'_>,
    ) -> ds_solar_contracts::SolarResult<ds_solar_contracts::BundleBytes> {
        Err(ds_solar_contracts::SolarError::weather_unavailable(
            "reference transport unavailable",
        ))
    }

    fn sync_gateway(
        &mut self,
        _call: SyncGatewayCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn project_configuration(
        &mut self,
        _call: ProjectConfigurationCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn list_projects(
        &mut self,
        call: ProjectListCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn transformer_context(
        &mut self,
        call: TransformerContextCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn project_forms(
        &mut self,
        call: ProjectFormsCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn project_form_editor(
        &mut self,
        call: ProjectFormEditorCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn solar_snapshot(
        &mut self,
        call: SolarSnapshotCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn survey_query(
        &mut self,
        call: SurveyQueryCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn survey_entries_select(
        &mut self,
        call: SurveyEntriesSelectCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn survey_entries_changes(
        &mut self,
        call: SurveyEntriesChangesCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn survey_entry_create(
        &mut self,
        call: SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn project_data(
        &mut self,
        call: ProjectDataCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn survey_photo(
        &mut self,
        _call: SurveyPhotoCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
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
    fn status_processing(
        &mut self,
        _call: StatusProcessingCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn upload_bytes(
        &mut self,
        call: crate::UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn solar_project(
        &mut self,
        _call: SolarProjectCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn survey_control(
        &mut self,
        call: SurveyControlCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
    fn styles(&mut self, call: StylesCall<'_>) -> Result<TransportResponse, TransportError>;
    fn layers(&mut self, call: LayersCall<'_>) -> Result<TransportResponse, TransportError>;
    fn printing(&mut self, _call: PrintingCall<'_>) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn design_selections(
        &mut self,
        _call: DesignSelectionsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn data_distribution(
        &mut self,
        _call: DataDistributionCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::Unreachable)
    }
    fn tiles(&mut self, call: TileCall<'_>) -> Result<TransportResponse, TransportError>;
    fn project_report(
        &mut self,
        call: ProjectReportCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
}

impl<T: Transport> AuthorizedTransport for T {
    fn transformer_save(
        &mut self,
        call: crate::TransformerSaveCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::transformer_save(self, call)
    }
    fn feedback(&mut self, call: FeedbackCall<'_>) -> Result<TransportResponse, TransportError> {
        Transport::feedback(self, call)
    }
    fn shared_assets(
        &mut self,
        call: SharedAssetsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::shared_assets(self, call)
    }
    fn design_attachments(
        &mut self,
        call: DesignAttachmentsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::design_attachments(self, call)
    }
    fn design_attachment_upload(
        &mut self,
        call: crate::design_attachments::UploadCall<'_>,
    ) -> Result<(), TransportError> {
        Transport::design_attachment_upload(self, call)
    }
    fn design_versions(
        &mut self,
        call: DesignVersionsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::design_versions(self, call)
    }
    fn design_tags(
        &mut self,
        call: DesignTagsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::design_tags(self, call)
    }

    fn report_artifact(
        &mut self,
        call: ReportArtifactCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::report_artifact(self, call)
    }
    fn grid_models(
        &mut self,
        call: GridModelsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::grid_models(self, call)
    }
    fn grid_catalog(
        &mut self,
        call: GridCatalogCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::grid_catalog(self, call)
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

    fn solar_reference(
        &mut self,
        call: SolarReferenceCall<'_>,
    ) -> ds_solar_contracts::SolarResult<ds_solar_contracts::BundleBytes> {
        Transport::solar_reference(self, call)
    }

    fn sync_gateway(
        &mut self,
        call: SyncGatewayCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::sync_gateway(self, call)
    }
    fn project_configuration(
        &mut self,
        call: ProjectConfigurationCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::project_configuration(self, call)
    }
    fn solar_project(
        &mut self,
        call: SolarProjectCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::solar_project(self, call)
    }
    fn list_projects(
        &mut self,
        call: ProjectListCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::list_projects(self, call)
    }
    fn transformer_context(
        &mut self,
        call: TransformerContextCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::transformer_context(self, call)
    }
    fn project_forms(
        &mut self,
        call: ProjectFormsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::project_forms(self, call)
    }
    fn project_form_editor(
        &mut self,
        call: ProjectFormEditorCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::project_form_editor(self, call)
    }
    fn solar_snapshot(
        &mut self,
        call: SolarSnapshotCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::solar_snapshot(self, call)
    }
    fn survey_query(
        &mut self,
        call: SurveyQueryCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::survey_query(self, call)
    }
    fn survey_entries_select(
        &mut self,
        call: SurveyEntriesSelectCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::survey_entries_select(self, call)
    }
    fn survey_entries_changes(
        &mut self,
        call: SurveyEntriesChangesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::survey_entries_changes(self, call)
    }
    fn survey_entry_create(
        &mut self,
        call: SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::survey_entry_create(self, call)
    }
    fn project_data(
        &mut self,
        call: ProjectDataCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::project_data(self, call)
    }
    fn survey_photo(
        &mut self,
        call: SurveyPhotoCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::survey_photo(self, call)
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
    fn status_processing(
        &mut self,
        call: StatusProcessingCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::status_processing(self, call)
    }
    fn upload_bytes(
        &mut self,
        call: crate::UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::upload_bytes(self, call)
    }
    fn survey_control(
        &mut self,
        call: SurveyControlCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::survey_control(self, call)
    }
    fn styles(&mut self, call: StylesCall<'_>) -> Result<TransportResponse, TransportError> {
        Transport::styles(self, call)
    }
    fn printing(&mut self, call: PrintingCall<'_>) -> Result<TransportResponse, TransportError> {
        Transport::printing(self, call)
    }
    fn layers(&mut self, call: LayersCall<'_>) -> Result<TransportResponse, TransportError> {
        Transport::layers(self, call)
    }
    fn design_selections(
        &mut self,
        call: DesignSelectionsCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::design_selections(self, call)
    }
    fn data_distribution(
        &mut self,
        call: DataDistributionCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::data_distribution(self, call)
    }
    fn tiles(&mut self, call: TileCall<'_>) -> Result<TransportResponse, TransportError> {
        Transport::tiles(self, call)
    }
    fn project_report(
        &mut self,
        call: ProjectReportCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        Transport::project_report(self, call)
    }
}
