use std::collections::BTreeMap;

use ds_command_kernel::survey_entry_create as kernel;
use serde::de::IgnoredAny;
use serde::Deserialize;
use serde_json::{Map, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::{ClientError, ErrorKind, SurveyEntryCreateServiceCode, TransportResponse};

pub const SURVEY_ENTRY_CREATE_MAX_DOCUMENT_ID_BYTES: usize = kernel::MAX_DOCUMENT_ID_BYTES;
pub const SURVEY_ENTRY_CREATE_MAX_FORM_SLUG_BYTES: usize = kernel::MAX_FORM_SLUG_BYTES;
pub const SURVEY_ENTRY_CREATE_MAX_IDEMPOTENCY_KEY_BYTES: usize = kernel::MAX_IDEMPOTENCY_KEY_BYTES;
pub const SURVEY_ENTRY_CREATE_MAX_PAYLOAD_BYTES: usize = kernel::MAX_PAYLOAD_BYTES;
const MAX_ERROR_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_ERROR_DETAIL_FIELDS: usize = 128;
const MAX_REQUEST_ID_BYTES: usize = 128;
pub use kernel::{Origin as SurveyEntryCreateOrigin, Receipt as SurveyEntryCreateReceipt};

#[derive(Clone, PartialEq)]
pub struct SurveyEntryCreateRequest {
    request: kernel::Request,
}

impl SurveyEntryCreateRequest {
    pub fn new(
        form: impl Into<String>,
        doc_id: impl Into<String>,
        idempotency_key: impl Into<String>,
        data: Map<String, Value>,
        created_at: impl Into<String>,
        origin: SurveyEntryCreateOrigin,
    ) -> Result<Self, ClientError> {
        kernel::Request::new(form, doc_id, idempotency_key, data, created_at, origin)
            .map(|request| Self { request })
            .map_err(map_refusal)
    }

    pub fn with_context_key(self, context_key: impl Into<String>) -> Result<Self, ClientError> {
        self.request
            .with_context_key(context_key)
            .map(|request| Self { request })
            .map_err(map_refusal)
    }

    pub fn with_geometry(self, geometry: Value) -> Result<Self, ClientError> {
        self.request
            .with_geometry(geometry)
            .map(|request| Self { request })
            .map_err(map_refusal)
    }

    pub fn with_connectivity(self, connectivity: Map<String, Value>) -> Result<Self, ClientError> {
        self.request
            .with_connectivity(connectivity)
            .map(|request| Self { request })
            .map_err(map_refusal)
    }

    pub fn with_detailed_location(
        self,
        detailed_location: Map<String, Value>,
    ) -> Result<Self, ClientError> {
        self.request
            .with_detailed_location(detailed_location)
            .map(|request| Self { request })
            .map_err(map_refusal)
    }

    pub fn form(&self) -> &str {
        self.request.form()
    }

    pub fn doc_id(&self) -> &str {
        self.request.doc_id()
    }

    pub fn idempotency_key(&self) -> &str {
        self.request.idempotency_key()
    }

    pub fn data(&self) -> &Map<String, Value> {
        self.request.data()
    }

    pub fn created_at(&self) -> &str {
        self.request.created_at()
    }

    pub const fn origin(&self) -> SurveyEntryCreateOrigin {
        self.request.origin()
    }

    pub fn context_key(&self) -> Option<&str> {
        self.request.context_key()
    }

    pub fn geometry(&self) -> Option<&Value> {
        self.request.geometry()
    }

    pub fn connectivity(&self) -> Option<&Map<String, Value>> {
        self.request.connectivity()
    }

    pub fn detailed_location(&self) -> Option<&Map<String, Value>> {
        self.request.detailed_location()
    }

    pub(crate) fn body_bytes(&self, project_id: &str) -> Vec<u8> {
        kernel::plan(&self.request, project_id)
            .expect("the closed Survey create request always plans")
            .body_bytes()
    }
}

impl std::fmt::Debug for SurveyEntryCreateRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SurveyEntryCreateRequest")
            .field("form", &self.form())
            .field("doc_id", &self.doc_id())
            .field("data_field_count", &self.data().len())
            .field("origin", &self.origin())
            .field("has_context", &self.context_key().is_some())
            .field("has_geometry", &self.geometry().is_some())
            .field(
                "connectivity_field_count",
                &self.connectivity().map_or(0, Map::len),
            )
            .field(
                "detailed_location_field_count",
                &self.detailed_location().map_or(0, Map::len),
            )
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorEnvelope {
    success: bool,
    error: ErrorDetailsWire,
    timestamp: String,
    service: String,
    version: String,
    #[serde(default)]
    request_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorDetailsWire {
    code: String,
    message: String,
    #[serde(default)]
    details: Option<BTreeMap<String, IgnoredAny>>,
    timestamp: i64,
}

pub(crate) fn validate_project(project_id: &str) -> Result<(), ClientError> {
    kernel::validate_project(project_id).map_err(map_refusal)
}

pub(crate) fn decode(
    response: TransportResponse,
    response_limit: usize,
    project_id: &str,
    request: &SurveyEntryCreateRequest,
) -> Result<SurveyEntryCreateReceipt, ClientError> {
    if response.body.len() > response_limit {
        return Err(unreadable());
    }
    let response: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    let plan = kernel::plan(&request.request, project_id).map_err(map_refusal)?;
    kernel::validate_receipt(&plan, 201, &response).map_err(map_refusal)
}

pub(crate) fn decode_service_error(
    response: &TransportResponse,
    response_limit: usize,
) -> Option<ClientError> {
    if response.body.len() > response_limit {
        return None;
    }
    let envelope: ErrorEnvelope = serde_json::from_slice(&response.body).ok()?;
    if envelope.success
        || envelope.service != "data-solutions-backend"
        || envelope.version != "1.0.0"
        || !canonical_utc_rfc3339(&envelope.timestamp)
        || !bounded_request_id(&envelope.request_id)
        || envelope.error.message.is_empty()
        || envelope.error.message.len() > MAX_ERROR_MESSAGE_BYTES
        || envelope.error.message.chars().any(char::is_control)
        || envelope.error.timestamp <= 0
        || envelope
            .error
            .details
            .as_ref()
            .is_some_and(|details| details.len() > MAX_ERROR_DETAIL_FIELDS)
    {
        return None;
    }
    let code = exact_service_code(response.status, &envelope.error.code)?;
    let (kind, message) = match code {
        SurveyEntryCreateServiceCode::Invalid => (
            ErrorKind::InvalidInput,
            "the Survey entry create request is outside its closed grammar or bounds",
        ),
        SurveyEntryCreateServiceCode::Unauthorized => (
            ErrorKind::AuthenticationRejected,
            "the governed Survey entry create service rejected the current session",
        ),
        SurveyEntryCreateServiceCode::PermissionDenied => (
            ErrorKind::AuthenticationRejected,
            "the verified user is not allowed to create entries in this Survey form",
        ),
        SurveyEntryCreateServiceCode::ScopeNotFound => (
            ErrorKind::ResourceNotFound,
            "the selected project, Survey form, or context ancestor is not available",
        ),
        SurveyEntryCreateServiceCode::FormDisabled => (
            ErrorKind::InvalidInput,
            "the Survey form is not enabled for entry creation in the selected project",
        ),
        SurveyEntryCreateServiceCode::ProjectReadOnly => (
            ErrorKind::InvalidInput,
            "the selected project is read-only and cannot accept Survey entries",
        ),
        SurveyEntryCreateServiceCode::IdempotencyConflict => (
            ErrorKind::InvalidInput,
            "the Survey create idempotency key is bound to a different mutation",
        ),
        SurveyEntryCreateServiceCode::AlreadyExists => (
            ErrorKind::InvalidInput,
            "the Survey entry already exists and is not an exact create replay",
        ),
        SurveyEntryCreateServiceCode::Failed => (
            ErrorKind::Transient,
            "the governed Survey entry create service failed temporarily",
        ),
    };
    Some(ClientError::with_survey_entry_create_service_code(
        kind, message, code,
    ))
}

fn exact_service_code(status: u16, code: &str) -> Option<SurveyEntryCreateServiceCode> {
    match (status, code) {
        (400, "VALIDATION_FAILED" | "INVALID_FORMAT") => {
            Some(SurveyEntryCreateServiceCode::Invalid)
        }
        (401, "UNAUTHORIZED") => Some(SurveyEntryCreateServiceCode::Unauthorized),
        (403, "INSUFFICIENT_PERMISSIONS") => Some(SurveyEntryCreateServiceCode::PermissionDenied),
        (404, "NOT_FOUND" | "PROJECT_NOT_FOUND") => {
            Some(SurveyEntryCreateServiceCode::ScopeNotFound)
        }
        (409, "FORM_NOT_ENABLED") => Some(SurveyEntryCreateServiceCode::FormDisabled),
        (403, "PROJECT_NOT_ACTIVE" | "PROJECT_ARCHIVED" | "PROJECT_EXPIRED") => {
            Some(SurveyEntryCreateServiceCode::ProjectReadOnly)
        }
        (409, "CONFLICT") => Some(SurveyEntryCreateServiceCode::IdempotencyConflict),
        (409, "ALREADY_EXISTS") => Some(SurveyEntryCreateServiceCode::AlreadyExists),
        (500, "INTERNAL_ERROR") => Some(SurveyEntryCreateServiceCode::Failed),
        _ => None,
    }
}

fn canonicalize_rfc3339(value: &str) -> Option<String> {
    if value.is_empty() || value.trim() != value {
        return None;
    }
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()?
        .to_offset(time::UtcOffset::UTC)
        .format(&Rfc3339)
        .ok()
}

fn canonical_utc_rfc3339(value: &str) -> bool {
    canonicalize_rfc3339(value).as_deref() == Some(value)
}

fn bounded_request_id(value: &str) -> bool {
    value.len() <= MAX_REQUEST_ID_BYTES
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn map_refusal(refusal: kernel::Refusal) -> ClientError {
    let kind = match refusal {
        kernel::Refusal::InvalidInput => ErrorKind::InvalidInput,
        kernel::Refusal::UnreadableResponse => ErrorKind::UnreadableResponse,
    };
    ClientError::new(kind, refusal.message())
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the Survey entry create response is outside the closed native contract",
    )
}
