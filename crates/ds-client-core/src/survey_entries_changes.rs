use std::collections::BTreeMap;

use serde::de::IgnoredAny;
use serde::Deserialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::{ClientError, ErrorKind, SurveyEntriesChangesServiceCode, TransportResponse};
use ds_command_kernel::survey_entries_changes as kernel;

const MAX_ERROR_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_ERROR_DETAIL_FIELDS: usize = 128;
const MAX_REQUEST_ID_BYTES: usize = 128;

pub const SURVEY_ENTRIES_CHANGES_DEFAULT_LIMIT: u16 = 100;
pub const SURVEY_ENTRIES_CHANGES_MAX_LIMIT: u16 = 500;
pub const SURVEY_ENTRIES_CHANGES_MAX_CURSOR_BYTES: usize = 4096;

/// One closed request for a fenced page of coalesced Survey mirror changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesChangesRequest {
    form: String,
    updated_after: String,
    limit: u16,
    cursor: Option<String>,
}
impl SurveyEntriesChangesRequest {
    pub fn new(
        form: impl Into<String>,
        updated_after: impl Into<String>,
        limit: Option<u16>,
        cursor: Option<String>,
    ) -> Result<Self, ClientError> {
        let inner = kernel::SurveyEntriesChangesRequest::new(form, updated_after, limit, cursor)
            .map_err(|_| invalid())?;
        Ok(Self {
            form: inner.form().into(),
            updated_after: inner.updated_after().into(),
            limit: inner.limit(),
            cursor: inner.cursor().map(str::to_owned),
        })
    }

    pub fn form(&self) -> &str {
        &self.form
    }

    /// Canonical UTC RFC3339Nano inclusive lower mirror clock.
    pub fn updated_after(&self) -> &str {
        &self.updated_after
    }

    pub const fn limit(&self) -> u16 {
        self.limit
    }

    /// Opaque backend-authenticated continuation; the core never decodes it.
    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }

    pub(crate) fn body_bytes(&self, ds_project: &str) -> Vec<u8> {
        kernel::SurveyEntriesChangesRequest::new(
            self.form.clone(),
            self.updated_after.clone(),
            Some(self.limit),
            self.cursor.clone(),
        )
        .expect("the closed Survey entry changes request remains validated")
        .body_bytes(ds_project)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesChanges {
    project: String,
    form: String,
    updated_after: String,
    upper_fence: String,
    rows: Vec<SurveyEntriesChangeRow>,
    next_cursor: Option<String>,
    has_more: bool,
    complete: bool,
    consistency: SurveyEntriesChangesConsistency,
}

impl SurveyEntriesChanges {
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn form(&self) -> &str {
        &self.form
    }
    pub fn updated_after(&self) -> &str {
        &self.updated_after
    }
    pub fn upper_fence(&self) -> &str {
        &self.upper_fence
    }
    pub fn rows(&self) -> &[SurveyEntriesChangeRow] {
        &self.rows
    }
    pub fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
    pub const fn has_more(&self) -> bool {
        self.has_more
    }
    pub const fn complete(&self) -> bool {
        self.complete
    }
    pub const fn consistency(&self) -> &SurveyEntriesChangesConsistency {
        &self.consistency
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesChangeRow {
    doc_id: String,
    geometry: Option<String>,
    created_by: String,
    is_deleted: bool,
    firestore_updated_at: String,
}

impl SurveyEntriesChangeRow {
    pub fn doc_id(&self) -> &str {
        &self.doc_id
    }
    pub fn geometry_json(&self) -> Option<&str> {
        self.geometry.as_deref()
    }
    pub fn created_by(&self) -> &str {
        &self.created_by
    }
    pub const fn is_deleted(&self) -> bool {
        self.is_deleted
    }
    pub fn firestore_updated_at(&self) -> &str {
        &self.firestore_updated_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesChangesConsistency {
    source: String,
    sync: String,
    fence: String,
    firestore_snapshot: bool,
    immutable_page_fence: bool,
}

impl SurveyEntriesChangesConsistency {
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn sync(&self) -> &str {
        &self.sync
    }
    pub fn fence(&self) -> &str {
        &self.fence
    }
    pub const fn firestore_snapshot(&self) -> bool {
        self.firestore_snapshot
    }
    pub const fn immutable_page_fence(&self) -> bool {
        self.immutable_page_fence
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

pub(crate) fn validate_project(ds_project: &str) -> Result<(), ClientError> {
    kernel::validate_project(ds_project).map_err(|_| invalid())
}

pub(crate) fn decode(
    response: TransportResponse,
    response_limit: usize,
    ds_project: &str,
    request: &SurveyEntriesChangesRequest,
) -> Result<SurveyEntriesChanges, ClientError> {
    let kernel_request = kernel::SurveyEntriesChangesRequest::new(
        request.form.clone(),
        request.updated_after.clone(),
        Some(request.limit),
        request.cursor.clone(),
    )
    .map_err(|_| invalid())?;
    let changes = kernel::decode(&response.body, response_limit, ds_project, &kernel_request)
        .map_err(|_| unreadable())?;
    Ok(SurveyEntriesChanges {
        project: changes.project().into(),
        form: changes.form().into(),
        updated_after: changes.updated_after().into(),
        upper_fence: changes.upper_fence().into(),
        rows: changes
            .rows()
            .iter()
            .map(|row| SurveyEntriesChangeRow {
                doc_id: row.doc_id().into(),
                geometry: row.geometry_json().map(str::to_owned),
                created_by: row.created_by().into(),
                is_deleted: row.is_deleted(),
                firestore_updated_at: row.firestore_updated_at().into(),
            })
            .collect(),
        next_cursor: changes.next_cursor().map(str::to_owned),
        has_more: changes.has_more(),
        complete: changes.complete(),
        consistency: SurveyEntriesChangesConsistency {
            source: changes.consistency().source().into(),
            sync: changes.consistency().sync().into(),
            fence: changes.consistency().fence().into(),
            firestore_snapshot: changes.consistency().firestore_snapshot(),
            immutable_page_fence: changes.consistency().immutable_page_fence(),
        },
    })
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
        || !canonical_utc_rfc3339_nano(&envelope.timestamp)
        || envelope.request_id.len() > MAX_REQUEST_ID_BYTES
        || (!envelope.request_id.is_empty()
            && (envelope.request_id.trim() != envelope.request_id
                || envelope.request_id.chars().any(char::is_control)))
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
        SurveyEntriesChangesServiceCode::Invalid => (
            ErrorKind::InvalidInput,
            "the survey entry changes request is outside its closed grammar or bounds",
        ),
        SurveyEntriesChangesServiceCode::CursorInvalid => (
            ErrorKind::InvalidInput,
            "the survey entry changes cursor is invalid for this request or authority",
        ),
        SurveyEntriesChangesServiceCode::FenceExpired => (
            ErrorKind::InvalidInput,
            "the BigQuery mirror page fence expired; restart from the last completed upper fence",
        ),
        SurveyEntriesChangesServiceCode::TooExpensive => (
            ErrorKind::InvalidInput,
            "the survey entry changes query exceeded its bounded query budget",
        ),
        SurveyEntriesChangesServiceCode::TooLarge => (
            ErrorKind::InvalidInput,
            "the survey entry changes response exceeded its bound; reduce the limit",
        ),
        SurveyEntriesChangesServiceCode::MirrorInvalid => (
            ErrorKind::UnreadableResponse,
            "the survey mirror contains incomplete or invalid change evidence",
        ),
        SurveyEntriesChangesServiceCode::SnapshotUnavailable => (
            ErrorKind::Transient,
            "the fenced BigQuery mirror version is temporarily unavailable",
        ),
        SurveyEntriesChangesServiceCode::Unavailable => (
            ErrorKind::Transient,
            "the governed survey entry changes service is unavailable on this deployment",
        ),
        SurveyEntriesChangesServiceCode::SyncFailed => (
            ErrorKind::Transient,
            "survey data could not be synchronized before reading changes",
        ),
        SurveyEntriesChangesServiceCode::Failed => (
            ErrorKind::Transient,
            "the governed survey entry changes service failed temporarily",
        ),
        SurveyEntriesChangesServiceCode::ScopeNotFound => (
            ErrorKind::ResourceNotFound,
            "the selected project or survey form is not available to this user",
        ),
    };
    Some(ClientError::with_survey_entries_changes_service_code(
        kind, message, code,
    ))
}

fn exact_service_code(status: u16, code: &str) -> Option<SurveyEntriesChangesServiceCode> {
    match (status, code) {
        (400, "SURVEY_ENTRIES_CHANGES_INVALID") => Some(SurveyEntriesChangesServiceCode::Invalid),
        (400, "SURVEY_ENTRIES_CHANGES_CURSOR_INVALID") => {
            Some(SurveyEntriesChangesServiceCode::CursorInvalid)
        }
        (410, "SURVEY_ENTRIES_CHANGES_FENCE_EXPIRED") => {
            Some(SurveyEntriesChangesServiceCode::FenceExpired)
        }
        (400, "SURVEY_ENTRIES_CHANGES_TOO_EXPENSIVE") => {
            Some(SurveyEntriesChangesServiceCode::TooExpensive)
        }
        (413, "SURVEY_ENTRIES_CHANGES_TOO_LARGE") => {
            Some(SurveyEntriesChangesServiceCode::TooLarge)
        }
        (503, "SURVEY_ENTRIES_CHANGES_MIRROR_INVALID") => {
            Some(SurveyEntriesChangesServiceCode::MirrorInvalid)
        }
        (503, "SURVEY_ENTRIES_CHANGES_SNAPSHOT_UNAVAILABLE") => {
            Some(SurveyEntriesChangesServiceCode::SnapshotUnavailable)
        }
        (503, "SURVEY_ENTRIES_CHANGES_UNAVAILABLE") => {
            Some(SurveyEntriesChangesServiceCode::Unavailable)
        }
        (503, "SURVEY_ENTRIES_CHANGES_SYNC_FAILED") => {
            Some(SurveyEntriesChangesServiceCode::SyncFailed)
        }
        (503, "SURVEY_ENTRIES_CHANGES_FAILED") => Some(SurveyEntriesChangesServiceCode::Failed),
        (404, "SURVEY_ENTRIES_SCOPE_NOT_FOUND") => {
            Some(SurveyEntriesChangesServiceCode::ScopeNotFound)
        }
        _ => None,
    }
}

fn canonicalize_rfc3339(value: &str) -> Option<String> {
    let parsed = OffsetDateTime::parse(value.trim(), &Rfc3339).ok()?;
    parsed.to_offset(time::UtcOffset::UTC).format(&Rfc3339).ok()
}

fn canonical_utc_rfc3339_nano(value: &str) -> bool {
    !value.is_empty() && canonicalize_rfc3339(value).as_deref() == Some(value)
}

fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "the survey entry changes request is outside its closed grammar or bounds",
    )
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the survey entry changes response is outside its closed contract",
    )
}
