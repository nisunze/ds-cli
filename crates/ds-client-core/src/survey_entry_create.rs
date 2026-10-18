use ds_command_kernel::survey_entry_create as kernel;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::{ClientError, ErrorKind, SurveyEntryCreateServiceCode, TransportResponse};

pub const SURVEY_ENTRY_CREATE_MAX_DOCUMENT_ID_BYTES: usize = kernel::MAX_DOCUMENT_ID_BYTES;
pub const SURVEY_ENTRY_CREATE_MAX_FORM_SLUG_BYTES: usize = kernel::MAX_FORM_SLUG_BYTES;
pub const SURVEY_ENTRY_CREATE_MAX_IDEMPOTENCY_KEY_BYTES: usize = kernel::MAX_IDEMPOTENCY_KEY_BYTES;
pub const SURVEY_ENTRY_CREATE_MAX_PAYLOAD_BYTES: usize = kernel::MAX_PAYLOAD_BYTES;
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

    pub(crate) fn direct_plan(&self, project: &str, authority: &kernel::direct::Authority) -> Result<kernel::direct::DirectPlan, ClientError> {
        kernel::direct::plan(&self.request, project, authority).map_err(map_refusal)
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
struct ErrorEnvelope { error: ErrorDetailsWire }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorDetailsWire {
    code:u16, status:String, message:String,
    #[serde(default)] details:Vec<serde::de::IgnoredAny>,
}
pub(crate) fn validate_project(project_id:&str)->Result<(),ClientError> { kernel::validate_project(project_id).map_err(map_refusal) }
pub(crate) fn decode_service_error(response:&TransportResponse,limit:usize)->Option<ClientError> {
    if response.body.len()>limit {return None;}
    let envelope:ErrorEnvelope=serde_json::from_slice(&response.body).ok()?;
    let e=envelope.error;
    if e.code!=response.status || e.message.len()>16384 || e.details.len()>128 {return None;}
    use SurveyEntryCreateServiceCode as C;
    let (code,kind)=match (response.status,e.status.as_str()) {
        (400,"INVALID_ARGUMENT"|"FAILED_PRECONDITION")=>(C::Invalid,ErrorKind::InvalidInput),
        (401,"UNAUTHENTICATED")=>(C::Unauthorized,ErrorKind::AuthenticationRejected),
        (403,"PERMISSION_DENIED")=>(C::PermissionDenied,ErrorKind::AuthenticationRejected),
        (404,"NOT_FOUND")=>(C::ScopeNotFound,ErrorKind::ResourceNotFound),
        (409,"ALREADY_EXISTS")=>(C::AlreadyExists,ErrorKind::InvalidInput),
        (429,"RESOURCE_EXHAUSTED")|(500,"INTERNAL")|(503,"UNAVAILABLE")=>(C::Failed,ErrorKind::Transient),
        _=>return None,
    };
    Some(ClientError::with_survey_entry_create_service_code(kind,"Direct Survey create was not acknowledged; retain the exact row",code))
}

fn map_refusal(refusal: kernel::Refusal) -> ClientError {
    let kind = match refusal {
        kernel::Refusal::InvalidInput => ErrorKind::InvalidInput,
        kernel::Refusal::UnreadableResponse => ErrorKind::UnreadableResponse,
    };
    ClientError::new(kind, refusal.message())
}
