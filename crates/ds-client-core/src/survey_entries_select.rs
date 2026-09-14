use std::collections::BTreeMap;

use serde::de::IgnoredAny;
use serde::Deserialize;

use crate::{ClientError, ErrorKind, SurveyEntriesSelectServiceCode, TransportResponse};
use ds_command_kernel::survey_entries_select as kernel;

const MAX_ERROR_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_ERROR_DETAIL_FIELDS: usize = 128;
const MAX_REQUEST_ID_BYTES: usize = 128;

pub const SURVEY_ENTRIES_SELECT_DEFAULT_LIMIT: u16 = 100;
pub const SURVEY_ENTRIES_SELECT_MAX_LIMIT: u16 = 500;

#[derive(Debug, Clone, PartialEq)]
pub struct SurveyEntriesSelectRequest {
    form: String,
    bbox: [f64; 4],
    limit: u16,
}
impl SurveyEntriesSelectRequest {
    pub fn new(
        form: impl Into<String>,
        bbox: [f64; 4],
        limit: Option<u16>,
    ) -> Result<Self, ClientError> {
        let mut request = Self {
            form: form.into(),
            bbox,
            limit: limit.unwrap_or(SURVEY_ENTRIES_SELECT_DEFAULT_LIMIT),
        };
        request.validate()?;
        for coordinate in &mut request.bbox {
            if *coordinate == 0.0 {
                *coordinate = 0.0;
            }
        }
        Ok(request)
    }

    pub fn form(&self) -> &str {
        &self.form
    }

    pub const fn bbox(&self) -> [f64; 4] {
        self.bbox
    }

    pub const fn limit(&self) -> u16 {
        self.limit
    }

    fn validate(&self) -> Result<(), ClientError> {
        kernel::SurveyEntriesSelectRequest::new(self.form.clone(), self.bbox, Some(self.limit))
            .map(|_| ())
            .map_err(|_| invalid())
    }

    pub(crate) fn body_bytes(&self, ds_project: &str) -> Vec<u8> {
        kernel::SurveyEntriesSelectRequest::new(self.form.clone(), self.bbox, Some(self.limit))
            .expect("the closed Survey entry selection request remains validated")
            .body_bytes(ds_project)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesSelection {
    project: String,
    form: String,
    bbox: [u64; 4],
    rows: Vec<SurveyEntriesSelectRow>,
    truncated: bool,
    complete: bool,
    selection_digest: String,
    consistency: SurveyEntriesSelectConsistency,
}

impl SurveyEntriesSelection {
    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn form(&self) -> &str {
        &self.form
    }

    pub fn bbox(&self) -> [f64; 4] {
        self.bbox.map(f64::from_bits)
    }

    pub fn rows(&self) -> &[SurveyEntriesSelectRow] {
        &self.rows
    }

    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    pub const fn complete(&self) -> bool {
        self.complete
    }

    pub fn selection_digest(&self) -> &str {
        &self.selection_digest
    }

    pub const fn consistency(&self) -> &SurveyEntriesSelectConsistency {
        &self.consistency
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesSelectRow {
    doc_id: String,
    geometry: String,
    created_by: String,
    firestore_updated_at: String,
}

impl SurveyEntriesSelectRow {
    pub fn doc_id(&self) -> &str {
        &self.doc_id
    }

    pub fn geometry_json(&self) -> &str {
        &self.geometry
    }

    pub fn created_by(&self) -> &str {
        &self.created_by
    }

    pub fn firestore_updated_at(&self) -> &str {
        &self.firestore_updated_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurveyEntriesSelectConsistency {
    source: String,
    sync: String,
    snapshot: bool,
    mutable: bool,
}

impl SurveyEntriesSelectConsistency {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn sync(&self) -> &str {
        &self.sync
    }

    pub const fn snapshot(&self) -> bool {
        self.snapshot
    }

    pub const fn mutable(&self) -> bool {
        self.mutable
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
    request: &SurveyEntriesSelectRequest,
) -> Result<SurveyEntriesSelection, ClientError> {
    let kernel_request = kernel::SurveyEntriesSelectRequest::new(
        request.form.clone(),
        request.bbox,
        Some(request.limit),
    )
    .map_err(|_| invalid())?;
    let selected = kernel::decode(&response.body, response_limit, ds_project, &kernel_request)
        .map_err(|_| unreadable())?;
    Ok(SurveyEntriesSelection {
        project: selected.project().into(),
        form: selected.form().into(),
        bbox: selected.bbox().map(f64::to_bits),
        rows: selected
            .rows()
            .iter()
            .map(|row| SurveyEntriesSelectRow {
                doc_id: row.doc_id().into(),
                geometry: row.geometry_json().into(),
                created_by: row.created_by().into(),
                firestore_updated_at: row.firestore_updated_at().into(),
            })
            .collect(),
        truncated: selected.truncated(),
        complete: selected.complete(),
        selection_digest: selected.selection_digest().into(),
        consistency: SurveyEntriesSelectConsistency {
            source: selected.consistency().source().into(),
            sync: selected.consistency().sync().into(),
            snapshot: selected.consistency().snapshot(),
            mutable: selected.consistency().mutable(),
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
        || envelope.timestamp.is_empty()
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
        SurveyEntriesSelectServiceCode::TooExpensive => (
            ErrorKind::InvalidInput,
            "the survey entry selection exceeded its query budget; narrow the bbox",
        ),
        SurveyEntriesSelectServiceCode::TooLarge => (
            ErrorKind::InvalidInput,
            "the survey entry selection exceeded its response bound; narrow the bbox or limit",
        ),
        SurveyEntriesSelectServiceCode::SyncFailed => (
            ErrorKind::Transient,
            "survey data could not be synchronized before entry selection",
        ),
        SurveyEntriesSelectServiceCode::MirrorInvalid => (
            ErrorKind::UnreadableResponse,
            "the survey mirror could not represent the entry selection safely",
        ),
        SurveyEntriesSelectServiceCode::Invalid => (
            ErrorKind::InvalidInput,
            "the governed survey entry selection service refused the bounded selection",
        ),
        SurveyEntriesSelectServiceCode::Unavailable => (
            ErrorKind::Transient,
            "the governed survey entry selection service is unavailable on this deployment",
        ),
        SurveyEntriesSelectServiceCode::Failed => (
            ErrorKind::Transient,
            "the governed survey entry selection service failed temporarily",
        ),
        SurveyEntriesSelectServiceCode::ScopeNotFound => (
            ErrorKind::ResourceNotFound,
            "the selected project or survey form is not available to this user",
        ),
    };
    Some(ClientError::with_survey_entries_select_service_code(
        kind, message, code,
    ))
}

fn exact_service_code(status: u16, code: &str) -> Option<SurveyEntriesSelectServiceCode> {
    match (status, code) {
        (400, "SURVEY_ENTRIES_SELECT_TOO_EXPENSIVE") => {
            Some(SurveyEntriesSelectServiceCode::TooExpensive)
        }
        (413, "SURVEY_ENTRIES_SELECT_TOO_LARGE") => Some(SurveyEntriesSelectServiceCode::TooLarge),
        (503, "SURVEY_ENTRIES_SELECT_SYNC_FAILED") => {
            Some(SurveyEntriesSelectServiceCode::SyncFailed)
        }
        (503, "SURVEY_ENTRIES_SELECT_MIRROR_INVALID") => {
            Some(SurveyEntriesSelectServiceCode::MirrorInvalid)
        }
        (400, "SURVEY_ENTRIES_SELECT_INVALID") => Some(SurveyEntriesSelectServiceCode::Invalid),
        (503, "SURVEY_ENTRIES_SELECT_UNAVAILABLE") => {
            Some(SurveyEntriesSelectServiceCode::Unavailable)
        }
        (503, "SURVEY_ENTRIES_SELECT_FAILED") => Some(SurveyEntriesSelectServiceCode::Failed),
        (404, "SURVEY_ENTRIES_SCOPE_NOT_FOUND") => {
            Some(SurveyEntriesSelectServiceCode::ScopeNotFound)
        }
        _ => None,
    }
}

fn canonical_utc_rfc3339_nano(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes.last() != Some(&b'Z')
    {
        return false;
    }
    for index in [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18] {
        if !bytes[index].is_ascii_digit() {
            return false;
        }
    }
    let year = digits(bytes, 0, 4);
    let month = digits(bytes, 5, 7);
    let day = digits(bytes, 8, 10);
    let hour = digits(bytes, 11, 13);
    let minute = digits(bytes, 14, 16);
    let second = digits(bytes, 17, 19);
    if year == 0
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return false;
    }
    if bytes.len() == 20 {
        return true;
    }
    if bytes[19] != b'.' || !(22..=30).contains(&bytes.len()) {
        return false;
    }
    let fraction = &bytes[20..bytes.len() - 1];
    !fraction.is_empty()
        && fraction.len() <= 9
        && fraction.iter().all(u8::is_ascii_digit)
        && fraction.last() != Some(&b'0')
}

fn digits(bytes: &[u8], start: usize, end: usize) -> u32 {
    bytes[start..end]
        .iter()
        .fold(0, |value, digit| value * 10 + u32::from(digit - b'0'))
}

const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "the survey entry selection request is outside its closed grammar or bounds",
    )
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the survey entry selection response is outside its closed bounded contract",
    )
}
