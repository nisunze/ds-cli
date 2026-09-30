//! Exact persisted analysis reads under an explicit project and saved-head fence.
//!
//! The server owns membership, head/content validation and missing/stale/invalid
//! decisions. This client verifies the raw JSON bytes, never reconstructs them,
//! and retains each standard error envelope's status/code in `service_refusal`.
use crate::{
    authorized::Authority, transport::CallAuthority, AuthorizedTransport, ClientError,
    ClientProfile, ErrorKind, SecretRequestBody, ServiceRefusal, TransportError, TransportResponse,
    CORRELATION_HEADERS,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;

// Fixed typed call, like transformer_save; no new profile override or schema.
pub const TRANSFORMER_ANALYSIS_METHOD: &str = "POST";
pub const TRANSFORMER_ANALYSIS_PATH: &str = "/api/v1/data";
pub const TRANSFORMER_ANALYSIS_ACTION: &str = "get_transformer_analysis";
pub const TRANSFORMER_ANALYSIS_SHA256_HEADER: &str = "X-DS-Analysis-SHA256";
pub const TRANSFORMER_ANALYSIS_RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
pub const TRANSFORMER_ANALYSIS_TIMEOUT_SECONDS: u64 = 120;

/// Closed privileged transport input. The project and all three head pins are
/// captured for this one call; there is no selected-project lookup in flight.
pub struct TransformerAnalysisCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) authority: CallAuthority<'a>,
    pub(crate) project: &'a str,
    pub(crate) transformer: &'a str,
    pub(crate) analysis_version: u64,
    pub(crate) content_digest: &'a str,
    pub(crate) analysis_sha256: &'a str,
}

impl TransformerAnalysisCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub const fn method(&self) -> &'static str {
        TRANSFORMER_ANALYSIS_METHOD
    }
    pub const fn path(&self) -> &'static str {
        TRANSFORMER_ANALYSIS_PATH
    }
    pub const fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub const fn response_limit(&self) -> usize {
        TRANSFORMER_ANALYSIS_RESPONSE_LIMIT
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TRANSFORMER_ANALYSIS_TIMEOUT_SECONDS
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
    pub fn canonical_uid(&self) -> &str {
        self.authority.canonical_uid
    }
    pub fn canonical_email(&self) -> &str {
        self.authority.canonical_email
    }
    pub fn device_id(&self) -> Option<&str> {
        self.authority.device_id
    }
    pub fn body(&self) -> SecretRequestBody {
        #[derive(Serialize)]
        struct Body<'a> {
            action: &'static str,
            eds_project_id: &'a str,
            transformer_name: &'a str,
            analysis_version: u64,
            content_digest: &'a str,
            analysis_sha256: &'a str,
        }
        SecretRequestBody::new(
            serde_json::to_vec(&Body {
                action: TRANSFORMER_ANALYSIS_ACTION,
                eds_project_id: self.project,
                transformer_name: self.transformer,
                analysis_version: self.analysis_version,
                content_digest: self.content_digest,
                analysis_sha256: self.analysis_sha256,
            })
            .expect("fixed strings and an integer serialize"),
        )
    }
}

/// Only this raw-body operation needs a bounded-reader refusal. Existing
/// transport error vocabularies remain unchanged for other typed calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformerAnalysisTransportError {
    Transport(TransportError),
    ResponseTooLarge,
}

impl From<TransportError> for TransformerAnalysisTransportError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}

impl From<TransformerAnalysisTransportError> for ClientError {
    fn from(error: TransformerAnalysisTransportError) -> Self {
        match error {
            TransformerAnalysisTransportError::Transport(error) => error.into(),
            TransformerAnalysisTransportError::ResponseTooLarge => {
                unreadable("analysis response exceeds 32 MiB")
            }
        }
    }
}

/// Response headers captured by the host using case-insensitive HTTP lookup.
/// Duplicate contract headers must be rejected, not silently selected.
#[derive(Default)]
pub struct TransformerAnalysisHeaders {
    pub content_type: Option<String>,
    pub analysis_sha256: Option<String>,
    pub cache_control: Option<String>,
}

/// Unlike ordinary wrapped JSON calls, this operation needs response headers
/// and exact bytes. Body storage keeps `TransportResponse`'s zeroizing drop.
pub struct TransformerAnalysisResponse {
    pub response: TransportResponse,
    pub headers: TransformerAnalysisHeaders,
}

impl TransformerAnalysisResponse {
    /// Read a response stream with at most 32 MiB plus one overflow sentinel.
    /// Hosts must supply the original JSON entity bytes, without parsing or
    /// reserializing, and must not follow redirects with DS authority.
    pub fn from_reader(
        status: u16,
        headers: TransformerAnalysisHeaders,
        reader: impl Read,
    ) -> Result<Self, TransformerAnalysisTransportError> {
        let mut response = TransportResponse::new(status, Vec::new());
        reader
            .take(TRANSFORMER_ANALYSIS_RESPONSE_LIMIT as u64 + 1)
            .read_to_end(&mut response.body)
            .map_err(|_| TransportError::Unreachable)?;
        if response.body.len() > TRANSFORMER_ANALYSIS_RESPONSE_LIMIT {
            return Err(TransformerAnalysisTransportError::ResponseTooLarge);
        }
        Ok(Self { response, headers })
    }
}

fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(crate) fn validate(
    project: &str,
    transformer: &str,
    _analysis_version: u64,
    content_digest: &str,
    analysis_sha256: &str,
) -> Result<(), ClientError> {
    crate::authorized::validate_transformer_context(project, transformer)?;
    if !sha256(content_digest) || !sha256(analysis_sha256) {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "analysis read requires exact lowercase SHA-256 content and analysis pins",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn read<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    transformer: &str,
    analysis_version: u64,
    content_digest: &str,
    analysis_sha256: &str,
) -> Result<Vec<u8>, ClientError> {
    let response = transport.transformer_analysis(TransformerAnalysisCall {
        profile,
        authority: authority.call(),
        project,
        transformer,
        analysis_version,
        content_digest,
        analysis_sha256,
    })?;
    accept(response, analysis_sha256)
}

fn unreadable(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, message)
}

fn accept(
    mut response: TransformerAnalysisResponse,
    requested_sha: &str,
) -> Result<Vec<u8>, ClientError> {
    if response.response.body.len() > TRANSFORMER_ANALYSIS_RESPONSE_LIMIT {
        return Err(TransformerAnalysisTransportError::ResponseTooLarge.into());
    }
    if response.response.status != 200 {
        return Err(refusal(&response.response));
    }
    if response.headers.content_type.as_deref() != Some("application/json")
        || response.headers.cache_control.as_deref() != Some("no-store")
    {
        return Err(unreadable(
            "analysis response headers do not match the raw JSON contract",
        ));
    }
    let header = response
        .headers
        .analysis_sha256
        .as_deref()
        .ok_or_else(|| unreadable("analysis response is missing its SHA-256 header"))?;
    if !sha256(header) || header != requested_sha {
        return Err(unreadable(
            "analysis response SHA-256 header does not match the requested pin",
        ));
    }
    if format!("{:x}", Sha256::digest(&response.response.body)) != requested_sha {
        return Err(unreadable(
            "analysis response bytes do not match the requested SHA-256",
        ));
    }
    // RawValue validates JSON without converting numbers or looking for an
    // envelope. The returned buffer retains every whitespace/number byte.
    serde_json::from_slice::<&serde_json::value::RawValue>(&response.response.body)
        .map_err(|_| unreadable("analysis response body is not valid JSON"))?;
    Ok(std::mem::take(&mut response.response.body))
}

fn refusal(response: &TransportResponse) -> ClientError {
    #[derive(Deserialize)]
    struct Envelope {
        error: Error,
    }
    #[derive(Deserialize)]
    struct Error {
        code: String,
        message: Option<String>,
        details: Option<serde_json::Map<String, serde_json::Value>>,
    }
    let envelope: Envelope = match serde_json::from_slice(&response.body) {
        Ok(envelope) => envelope,
        Err(_) => return unreadable("analysis refusal is not a standard error envelope"),
    };
    let refusal = ServiceRefusal::with_details(
        response.status,
        Some(&envelope.error.code),
        envelope.error.message.as_deref(),
        envelope.error.details.as_ref(),
    );
    if refusal.code() != Some(envelope.error.code.as_str()) {
        return unreadable("analysis refusal has no valid service code");
    }
    // Keep missing and moved-fence/stale 409 codes intact. A 409 is never an
    // automatic retry or an invitation to calculate a replacement locally.
    let kind = match response.status {
        400 => ErrorKind::InvalidInput,
        401 => ErrorKind::AuthenticationRejected,
        403 => ErrorKind::IdentityMismatch,
        404 => ErrorKind::ResourceNotFound,
        429 | 500..=599 => ErrorKind::Transient,
        _ => ErrorKind::UnreadableResponse,
    };
    ClientError::with_service_refusal(kind, "the server refused the fenced analysis read", refusal)
}
