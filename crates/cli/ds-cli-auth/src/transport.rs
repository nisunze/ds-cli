//! Fixed-origin ureq adapter for ds-client-core's closed calls.
//! Explicit debug local Go routing leaves the protected profile unchanged.

use crate::local_go;

#[path = "gateway_transport.rs"]
mod gateway_transport;

use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ds_client_core::{
    RefreshCall, SignInCall, SurveyEntryCreateCall, Transport, TransportError, TransportResponse,
};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
// Governed MV packages are digest-verified after download, but storage reads can
// outlast the short gateway request bound on a constrained server connection.
const GRID_MODEL_BYTES_TIMEOUT: Duration = Duration::from_secs(600);
static CORRELATION_SEQUENCE: AtomicU64 = AtomicU64::new(1);
/// The last correlation id this process sent: a reliability event names it as
/// its `request_id`, which joins the ds-brain rows of the same request.
static LAST_CORRELATION_ID: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub(crate) fn last_correlation_id() -> Option<String> {
    LAST_CORRELATION_ID
        .lock()
        .ok()
        .and_then(|last| last.clone())
}

#[derive(Default)]
pub struct NativeTransport;

impl Transport for NativeTransport {
    fn solar_reference(
        &mut self,
        call: ds_client_core::SolarReferenceCall<'_>,
    ) -> ds_solar_contracts::SolarResult<ds_solar_contracts::BundleBytes> {
        use ds_solar_reference_host::ReferenceBundleProvider;
        if local_go::is_local().map_err(|_| {
            ds_solar_contracts::SolarError::weather_unavailable(
                "native local Go transport selection is invalid",
            )
        })? {
            return Err(ds_solar_contracts::SolarError::weather_unavailable(
                "Solar reference transport does not support local Go; no cloud fallback was attempted",
            ));
        }
        ds_solar_reference_host::HttpReferenceProvider::new(
            call.gateway_origin(),
            Some(call.bearer_token().to_owned()),
        )?
        .with_api_key(Some(call.gateway_api_key().to_owned()))
        .fetch(call.request())
    }

    ds_client_core::__ds_gateway_calls!(native, gateway_transport::send);

    fn sign_in(&mut self, call: SignInCall<'_>) -> Result<TransportResponse, TransportError> {
        let body = call.body();
        let response = ureq::post(call.endpoint())
            .query("key", call.firebase_api_key())
            .content_type(call.content_type())
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(CALL_TIMEOUT))
            .build()
            .send(body.as_bytes())
            .map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn refresh(&mut self, call: RefreshCall<'_>) -> Result<TransportResponse, TransportError> {
        let body = call.body();
        let response = ureq::post(call.endpoint())
            .query("key", call.firebase_api_key())
            .content_type(call.content_type())
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(CALL_TIMEOUT))
            .build()
            .send(body.as_bytes())
            .map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn transformer_analysis(
        &mut self,
        call: ds_client_core::TransformerAnalysisCall<'_>,
    ) -> Result<
        ds_client_core::TransformerAnalysisResponse,
        ds_client_core::TransformerAnalysisTransportError,
    > {
        let (request_id, action_id) = correlation_headers();
        let bearer = Zeroizing::new(format!("Bearer {}", call.bearer_token()));
        let body = call.body();
        let response = local_go::api_agent()?
            .post(format!(
                "{}{}",
                local_go::api_origin(call.gateway_origin())?,
                call.path()
            ))
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &*bearer)
            .header("X-Forwarded-Authorization", &*bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes())
            .map_err(classify)?;
        raw_transformer_analysis(response)
    }

    fn survey_entry_create(
        &mut self,
        call: SurveyEntryCreateCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        debug_assert_eq!(call.method(), "POST");
        debug_assert_eq!(call.origin(), "https://firestore.googleapis.com");
        debug_assert_eq!(call.timeout_seconds(), 120);
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let url = format!("{}{}", call.origin(), call.path());
        let result = ureq::Agent::new_with_defaults()
            .post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        let response = result.map_err(classify)?;
        bounded(response, call.response_limit())
    }

    /// Seal the closed client's forward-only stream and drive the shared native
    /// transport. Completion includes the digest re-proof; refusal mapping stays
    /// in this host because the client call returns only an HTTP-shaped outcome.
    fn upload_bytes(
        &mut self,
        mut call: ds_client_core::UploadBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let session_uri = Zeroizing::new(call.uri().to_owned());
        let size = call.size();
        let result = ds_sync_runtime::native_transfer::drive_upload_stream(
            &session_uri,
            size,
            call.reader(),
        )
        .map_err(|_| TransportError::Unreachable)?;
        upload_response(result)
    }

    fn download_solar_artifact(
        &mut self,
        call: ds_client_core::SolarArtifactDownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let response = ureq::get(call.url())
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .call()
            .map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn download_solar_media(
        &mut self,
        call: ds_client_core::SolarMediaDownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let response = ureq::get(call.url())
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .call()
            .map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn download_survey_photo(
        &mut self,
        call: ds_client_core::survey_photo::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let response = ureq::get(call.url())
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .call()
            .map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn transformer_save(
        &mut self,
        call: ds_client_core::TransformerSaveCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let (request_id, action_id) = correlation_headers();
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let body = call.body();
        let result = local_go::api_agent()?
            .post(format!(
                "{}{}",
                local_go::api_origin(call.gateway_origin())?,
                call.path()
            ))
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("X-User-Email", call.canonical_email())
            .header("x-api-key", call.gateway_api_key())
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", &bearer)
            .header("X-DS-Processing-Lane", "fast")
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .send(body.as_bytes());
        bearer.zeroize();
        bounded(result.map_err(classify)?, call.response_limit())
    }

    fn sre_client_event(
        &mut self,
        call: ds_client_core::reporter::SreClientEventCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let (request_id, action_id) = correlation_headers();
        let url = format!(
            "{}{}",
            local_go::api_origin(call.gateway_origin())?,
            call.path()
        );
        let mut bearer = call.bearer_token().map(|token| format!("Bearer {token}"));
        let mut request = local_go::api_agent()?
            .post(url)
            .header("Accept", call.content_type())
            .header("Content-Type", call.content_type())
            .header("X-App-Id", call.client_id())
            .header("X-Request-Id", &request_id)
            .header("X-DS-Action-Id", &action_id)
            .header("x-api-key", call.gateway_api_key());
        if let Some(value) = bearer.as_deref() {
            request = request
                .header("Authorization", value)
                .header("X-Forwarded-Authorization", value);
        }
        let result = request
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(call.timeout().min(CONNECT_TIMEOUT)))
            .timeout_global(Some(call.timeout()))
            .build()
            .send(call.body());
        if let Some(value) = bearer.as_mut() {
            value.zeroize();
        }
        bounded(result.map_err(classify)?, call.response_limit())
    }

    /// `POST /api/v1/projects` — create a project or edit its properties.
    /// Without this the trait's default answered `Unreachable`, which reads as
    /// `auth_transient`, so `ds auth project create|update` could never
    /// succeed from a native session and never said why.

    /// `POST /api/v1/projects` — one owner-only project delete action. The
    /// same door as `project_properties`, with the larger bounds an export
    /// page and an apply need.

    fn survey_media_bytes(
        &mut self,
        call: ds_client_core::SurveyMediaBytesCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        // The link is the whole authority: no bearer, key or identity header
        // is sent, so following the resolver's one redirect to its storage
        // signature hands nothing to a second origin. An https link never
        // follows that redirect down to plain http: the signature it lands on
        // is a bearer for the photo.
        let response = ureq::get(call.url())
            .config()
            .max_redirects(call.max_redirects())
            .https_only(call.url().starts_with("https://"))
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .call()
            .map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn asset_preview_bytes(
        &mut self,
        call: ds_client_core::asset_preview_source::DownloadCall<'_>,
        writer: &mut dyn Write,
        control: &dyn ds_client_core::asset_preview_source::TransferControl,
    ) -> Result<
        ds_client_core::asset_preview_source::DownloadReceipt,
        ds_client_core::asset_preview_source::TransferError,
    > {
        use ds_client_core::asset_preview_source::{DownloadReceipt, TransferError};
        control.check_current()?;
        let mut response = ureq::get(call.url())
            .header("Accept-Encoding", "identity")
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .call()
            .map_err(|e| TransferError::from(classify(e)))?;
        control.check_current()?;
        let status = response.status().as_u16();
        let generation = response
            .headers()
            .get("x-goog-generation")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if status != 200
            || generation.as_deref() != Some(call.generation())
            || response
                .headers()
                .get("content-encoding")
                .is_some_and(|v| v.as_bytes() != b"identity")
            || response
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .is_some_and(|size| size != call.response_limit())
        {
            return Err(TransferError::SourceChanged);
        }
        let size_bytes = stream_preview_body(
            &mut response.body_mut().as_reader(),
            writer,
            call.response_limit(),
            control,
        )?;
        Ok(DownloadReceipt {
            status,
            generation,
            size_bytes,
        })
    }
    fn shared_asset_bytes(
        &mut self,
        call: ds_client_core::shared_assets::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let response = ureq::get(call.url())
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .call()
            .map_err(classify)?;
        bounded(response, call.response_limit())
    }
    fn grid_model_upload(
        &mut self,
        call: ds_client_core::grid_models::publication::UploadCall<'_>,
    ) -> Result<(), TransportError> {
        use ds_sync_runtime::native_transfer::{NativeTransferOutcome, drive_verified_output};
        let result = drive_verified_output(
            call.uri(),
            call.bytes().len() as u64,
            call.digest(),
            std::io::Cursor::new(call.bytes()),
            None,
            None,
            &|| false,
            &mut |_| {},
        )
        .map_err(|_| TransportError::Unreachable)?;
        if result.outcome == NativeTransferOutcome::Done {
            Ok(())
        } else {
            Err(TransportError::Unreachable)
        }
    }
    fn design_attachment_upload(
        &mut self,
        call: ds_client_core::design_attachments::UploadCall<'_>,
    ) -> Result<(), TransportError> {
        use ds_sync_runtime::native_transfer::{NativeTransferOutcome, drive_verified_output};
        let result = drive_verified_output(
            call.uri(),
            call.bytes().len() as u64,
            call.digest(),
            std::io::Cursor::new(call.bytes()),
            None,
            None,
            &|| false,
            &mut |_| {},
        )
        .map_err(|_| TransportError::Unreachable)?;
        if result.outcome == NativeTransferOutcome::Done {
            Ok(())
        } else {
            Err(TransportError::Unreachable)
        }
    }
    fn grid_model_bytes(
        &mut self,
        call: ds_client_core::grid_models::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let response = ureq::get(call.url())
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(GRID_MODEL_BYTES_TIMEOUT))
            .build()
            .call()
            .map_err(classify)?;
        bounded(response, call.response_limit())
    }

    fn design_attachment_bytes(
        &mut self,
        call: ds_client_core::design_attachments::DownloadCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        // A revision may be 512 MiB, twice a project model; the read is
        // bounded by the declared size, and its time scales with it.
        let seconds = 120 + (call.response_limit() as u64 / (1024 * 1024));
        let response = ureq::get(call.url())
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(seconds)))
            .build()
            .call()
            .map_err(classify)?;
        bounded(response, call.response_limit())
    }

    /// One public reference-bundle object, streamed into the caller's sink.
    ///
    /// This call carries NO DS credential: no bearer, api key, user email or
    /// app id, and no correlation ids either — the object is public and the
    /// origin is not ours, so nothing of ours travels with the request. The
    /// core pinned the host before this adapter saw the URL, and the bytes are
    /// verified by the core (`ds_client_core::download_bundle`), not here.
    fn download_bundle(
        &mut self,
        call: ds_client_core::BundleDownloadCall<'_>,
        sink: &mut dyn Write,
    ) -> Result<(), TransportError> {
        debug_assert_eq!(call.method(), "GET");
        fetch_bundle(
            call.url(),
            BundleOrigin::Storage,
            call.response_limit(),
            call.timeout_seconds(),
            sink,
        )
    }
}

// No parse/serialize step: the raw entity and contract headers belong to core.
fn raw_transformer_analysis(
    mut response: ureq::http::Response<ureq::Body>,
) -> Result<
    ds_client_core::TransformerAnalysisResponse,
    ds_client_core::TransformerAnalysisTransportError,
> {
    let header = |name: &str| -> Result<Option<String>, TransportError> {
        let mut values = response.headers().get_all(name).iter();
        let value = values
            .next()
            .map(|value| value.to_str().map(str::to_owned))
            .transpose()
            .map_err(|_| TransportError::Unreachable)?;
        if values.next().is_some() {
            return Err(TransportError::Unreachable);
        }
        Ok(value)
    };
    let headers = ds_client_core::TransformerAnalysisHeaders {
        content_type: header("content-type")?,
        analysis_sha256: header(ds_client_core::TRANSFORMER_ANALYSIS_SHA256_HEADER)?,
        cache_control: header("cache-control")?,
    };
    ds_client_core::TransformerAnalysisResponse::from_reader(
        response.status().as_u16(),
        headers,
        response.body_mut().as_reader(),
    )
}

#[cfg(test)]
fn survey_entry_create_url(firebase_project: &str) -> String {
    format!(
        "https://firestore.googleapis.com{}",
        ds_client_core::SURVEY_ENTRY_CREATE_PATH.replace("{firebase_project}", firebase_project)
    )
}

/// Where a bundle `GET` may go. Production names only the TLS storage host
/// the core pinned; tests name a scripted loopback listener, which is the one
/// thing this adapter can do that production cannot.
#[derive(Clone, Copy)]
enum BundleOrigin {
    Storage,
    #[cfg(test)]
    Loopback,
}

/// The slice handed to the sink per read. Bounded memory: a bundle is never
/// held whole.
const BUNDLE_READ_BYTES: usize = 64 * 1024;

/// Stream one `GET` body into `sink`, at most `limit` bytes. Redirects are
/// off — a redirect could move the fetch to an origin nobody reviewed — and a
/// status other than 200 is a refusal with nothing written.
fn fetch_bundle(
    url: &str,
    origin: BundleOrigin,
    limit: u64,
    timeout_seconds: u64,
    sink: &mut dyn Write,
) -> Result<(), TransportError> {
    let agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .https_only(matches!(origin, BundleOrigin::Storage))
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(Duration::from_secs(timeout_seconds)))
        .build()
        .new_agent();
    let mut response = agent
        .get(url)
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(classify)?;
    if response.status().as_u16() != 200 {
        return Err(TransportError::Unreachable);
    }
    let mut reader = response.body_mut().with_config().limit(limit).reader();
    let mut buffer = [0u8; BUNDLE_READ_BYTES];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| TransportError::Unreachable)?;
        if read == 0 {
            break;
        }
        sink.write_all(&buffer[..read])
            .map_err(|_| TransportError::Unreachable)?;
    }
    sink.flush().map_err(|_| TransportError::Unreachable)
}

pub(crate) fn bounded(
    mut response: ureq::http::Response<ureq::Body>,
    limit: usize,
) -> Result<TransportResponse, TransportError> {
    let status = response.status().as_u16();
    crate::network_cause::answered(
        status,
        response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
    );
    let mut body = Zeroizing::new(Vec::with_capacity(limit.min(64 * 1024)));
    response
        .body_mut()
        .with_config()
        .limit(limit.saturating_add(1) as u64)
        .reader()
        .read_to_end(&mut body)
        .map_err(|error| {
            crate::network_cause::body_failed(&error);
            TransportError::Unreachable
        })?;
    Ok(TransportResponse::new(status, std::mem::take(&mut *body)))
}

/// The one native classification, which also records the cause a refusal names.
fn classify(error: ureq::Error) -> TransportError {
    crate::network_cause::classify(error)
}

fn correlation_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = CORRELATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut hasher = Sha256::new();
    hasher.update(b"ds-native-correlation/v1");
    hasher.update(std::process::id().to_be_bytes());
    hasher.update(nanos.to_be_bytes());
    hasher.update(sequence.to_be_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

fn correlation_headers() -> (String, String) {
    // Every governed request starts here: nothing an earlier one saw
    // describes it.
    crate::network_cause::request_started();
    let request_id = correlation_id();
    if let Ok(mut last) = LAST_CORRELATION_ID.lock() {
        *last = Some(request_id.clone());
    }
    (request_id.clone(), request_id)
}

/// Keep the native client's stable transfer classifications. The shared
/// driver owns completion, retry decisions and causes; this only shapes them.
fn upload_response(
    result: ds_sync_runtime::native_transfer::NativeTransferResult,
) -> Result<TransportResponse, TransportError> {
    use ds_command_kernel::transfer::Cause;
    use ds_sync_runtime::native_transfer::NativeTransferOutcome;
    if result.outcome == NativeTransferOutcome::Done {
        return Ok(TransportResponse::new(200, Vec::new()));
    }
    match result.cause {
        Some(Cause::HttpStatus { status }) => Ok(TransportResponse::new(status, Vec::new())),
        Some(Cause::SessionExpired) => Ok(TransportResponse::new(
            result.last_http_status.unwrap_or(410),
            Vec::new(),
        )),
        Some(Cause::Timeout { .. }) => Err(TransportError::TimedOut),
        _ => Err(TransportError::Unreachable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    #[test]
    fn raw_analysis_host_preserves_exact_bytes_headers_status_and_bound() {
        for (status, body) in [
            (200, b"{ \"n\": 1.000e+02 }\n".to_vec()),
            (400, br#"{"error":{"code":"analysis_invalid"}}"#.to_vec()),
            (409, br#"{"error":{"code":"analysis_stale"}}"#.to_vec()),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let expected = body.clone();
            let thread = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                }
                write!(stream, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nX-DS-Analysis-SHA256: {}\r\nConnection: close\r\n\r\n", body.len(), "a".repeat(64)).unwrap();
                stream.write_all(&body).unwrap();
            });
            let response = ureq::get(format!("http://{address}"))
                .config()
                .http_status_as_error(false)
                .build()
                .call()
                .unwrap();
            let raw = raw_transformer_analysis(response).unwrap();
            assert_eq!(raw.response.status, status);
            assert_eq!(raw.response.body, expected);
            assert_eq!(
                raw.headers.content_type.as_deref(),
                Some("application/json")
            );
            assert_eq!(raw.headers.cache_control.as_deref(), Some("no-store"));
            assert_eq!(raw.headers.analysis_sha256, Some("a".repeat(64)));
            thread.join().unwrap();
        }
        let overflow = ds_client_core::TransformerAnalysisResponse::from_reader(
            200,
            Default::default(),
            std::io::repeat(b' ')
                .take(ds_client_core::TRANSFORMER_ANALYSIS_RESPONSE_LIMIT as u64 + 1),
        );
        assert!(matches!(
            overflow,
            Err(ds_client_core::TransformerAnalysisTransportError::ResponseTooLarge)
        ));
    }

    #[test]
    fn raw_analysis_host_rejects_duplicate_contract_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
            }
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: application/json\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}").unwrap();
        });
        let response = ureq::get(format!("http://{address}")).call().unwrap();
        assert!(raw_transformer_analysis(response).is_err());
        thread.join().unwrap();
    }

    #[test]
    fn raw_analysis_client_and_device_are_explicit_and_never_retry_moved_heads() {
        use crate::test_support::{FixtureTransport, NOW, SIGN_IN, linked_device, signed_in};
        let bytes = b"{ \"n\": 1.000e+02 }\n";
        let sha = format!("{:x}", Sha256::digest(bytes));
        let content = "c".repeat(64);
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(transport.clone());
        let mut device = linked_device(transport.clone(), crate::now());
        for project in ["project-a", "project-b"] {
            transport.lock().transformer_analysis.push_back(
                ds_client_core::TransformerAnalysisResponse::from_reader(
                    200,
                    ds_client_core::TransformerAnalysisHeaders {
                        content_type: Some("application/json".into()),
                        cache_control: Some("no-store".into()),
                        analysis_sha256: Some(sha.clone()),
                    },
                    bytes.as_slice(),
                )
                .unwrap(),
            );
            let got = if project == "project-a" {
                client
                    .transformer_analysis(project, "T1", 8, &content, &sha, NOW)
                    .unwrap()
            } else {
                device
                    .transformer_analysis(project, "T1", 8, &content, &sha)
                    .unwrap()
            };
            assert_eq!(got, bytes);
        }
        for status in [400, 409] {
            transport.lock().transformer_analysis.push_back(
                ds_client_core::TransformerAnalysisResponse::from_reader(
                    status,
                    Default::default(),
                    br#"{"error":{"code":"analysis_stale","message":"captured fence moved"}}"#
                        .as_slice(),
                )
                .unwrap(),
            );
            let error = client
                .transformer_analysis("project-a", "T1", 8, &content, &sha, NOW)
                .unwrap_err();
            let refusal = error.service_refusal().unwrap();
            assert_eq!(refusal.status(), status);
            assert_eq!(refusal.code(), Some("analysis_stale"));
        }
        let script = transport.lock();
        assert_eq!(script.analysis_bodies.len(), 4);
        for (body, project) in
            script
                .analysis_bodies
                .iter()
                .zip(["project-a", "project-b", "project-a", "project-a"])
        {
            assert_eq!(
                *body,
                serde_json::json!({"action":"get_transformer_analysis","eds_project_id":project,"transformer_name":"T1","analysis_version":8,"content_digest":content,"analysis_sha256":sha})
            );
        }
    }

    /// One scripted loopback `GET` answer. Returns the port and the thread that
    /// yields the request line and the lower-cased header names it saw.
    #[test]
    fn shared_upload_results_keep_client_refusal_classifications() {
        use ds_command_kernel::transfer::{Cause, Phase, Stage, TransferState};
        use ds_sync_runtime::native_transfer::{NativeTransferOutcome, NativeTransferResult};
        let result = |outcome, cause, status| NativeTransferResult {
            outcome,
            cause,
            last_http_status: status,
            retryable: false,
            committed_bytes: 0,
            total_bytes: 1,
            state: TransferState {
                schema: ds_command_kernel::transfer::TRANSFER_SCHEMA.into(),
                total_bytes: 1,
                chunk_bytes: ds_command_kernel::transfer::DEFAULT_CHUNK_BYTES,
                committed: 0,
                sent: 0,
                stage: Stage::Probing,
                stalled_attempts: 0,
                inconclusive_probes: 0,
                last_cause: None,
                retry_at: 0,
            },
        };
        assert_eq!(
            upload_response(result(NativeTransferOutcome::Done, None, Some(201)))
                .unwrap()
                .status,
            200
        );
        for status in [404, 410] {
            assert_eq!(
                upload_response(result(
                    NativeTransferOutcome::Reopen,
                    Some(Cause::SessionExpired),
                    Some(status)
                ))
                .unwrap()
                .status,
                status
            );
        }
        for status in [401, 403, 302, 500] {
            assert_eq!(
                upload_response(result(
                    NativeTransferOutcome::Failed,
                    Some(Cause::HttpStatus { status }),
                    Some(status)
                ))
                .unwrap()
                .status,
                status
            );
        }
        assert!(matches!(
            upload_response(result(
                NativeTransferOutcome::Failed,
                Some(Cause::Timeout {
                    phase: Phase::Response
                }),
                None
            )),
            Err(TransportError::TimedOut)
        ));
        for cause in [
            Cause::DigestMismatch,
            Cause::Cancelled,
            Cause::ConnectionClosed,
        ] {
            assert!(matches!(
                upload_response(result(
                    NativeTransferOutcome::Failed,
                    Some(cause),
                    Some(200)
                )),
                Err(TransportError::Unreachable)
            ));
        }
    }

    fn serve_once(
        status: u16,
        body: Vec<u8>,
    ) -> (u16, std::thread::JoinHandle<(String, Vec<String>)>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("one request");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut request_line = String::new();
            reader.read_line(&mut request_line).expect("request line");
            let mut names = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("header line");
                if line.trim().is_empty() {
                    break;
                }
                names.push(
                    line.split(':')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_ascii_lowercase(),
                );
            }
            let head = format!(
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).expect("head");
            stream.write_all(&body).expect("body");
            stream.flush().expect("flush");
            (request_line, names)
        });
        (port, worker)
    }

    /// Storage transfers and downloads carry no DS control-plane credential:
    /// the bundle host is not ours, so nothing of ours travels with the GET.
    #[test]
    fn a_bundle_download_carries_no_ds_credential() {
        let body = b"a national reference bundle".to_vec();
        let (port, worker) = serve_once(200, body.clone());
        let mut sink = Vec::new();
        fetch_bundle(
            &format!("http://127.0.0.1:{port}/edcl/reference/roads.geojsonl.gz"),
            BundleOrigin::Loopback,
            body.len() as u64 + 1,
            30,
            &mut sink,
        )
        .expect("the scripted object streams");
        let (request_line, names) = worker.join().expect("server thread");
        assert_eq!(sink, body);
        assert!(
            request_line.starts_with("GET /edcl/reference/roads.geojsonl.gz HTTP/1.1"),
            "{request_line}"
        );
        for forbidden in [
            "authorization",
            "x-forwarded-authorization",
            "x-api-key",
            "x-user-email",
            "x-app-id",
            "x-ds-processing-lane",
            "x-request-id",
            "x-ds-action-id",
            "cookie",
        ] {
            assert!(
                !names.iter().any(|name| name == forbidden),
                "{forbidden} must never reach the bundle host"
            );
        }
    }

    #[test]
    fn a_bundle_download_refuses_a_non_200_answer_and_writes_nothing() {
        let (port, worker) = serve_once(404, b"<xml>NoSuchKey</xml>".to_vec());
        let mut sink = Vec::new();
        let refused = fetch_bundle(
            &format!("http://127.0.0.1:{port}/edcl/reference/missing.geojsonl.gz"),
            BundleOrigin::Loopback,
            1024,
            30,
            &mut sink,
        );
        let _ = worker.join();
        assert_eq!(refused, Err(TransportError::Unreachable));
        assert!(sink.is_empty());
    }

    /// Production names only the TLS storage origin: a plaintext URL is
    /// refused before any socket opens, so the host pin cannot be downgraded.
    #[test]
    fn a_bundle_download_never_leaves_tls_in_production() {
        let mut sink = Vec::new();
        assert_eq!(
            fetch_bundle(
                "http://127.0.0.1:9/edcl/reference/roads.geojsonl.gz",
                BundleOrigin::Storage,
                16,
                5,
                &mut sink,
            ),
            Err(TransportError::Unreachable)
        );
        assert!(sink.is_empty());
    }

    #[test]
    fn verified_grid_model_storage_read_has_a_bounded_longer_window() {
        // A 6 MB published revision was refused at 120 seconds even though the
        // authenticated catalog answered. Keep this storage-only allowance long
        // enough for the byte read while retaining a finite upper bound.
        assert!(GRID_MODEL_BYTES_TIMEOUT > Duration::from_secs(120));
        assert!(GRID_MODEL_BYTES_TIMEOUT <= Duration::from_secs(600));
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(10));
    }

    #[test]
    fn data_distribution_wire_target_and_limits_are_fixed() {
        assert_eq!(
            format!(
                "https://fixture.ue.gateway.dev{}",
                ds_client_core::DATA_DISTRIBUTION_PATH
            ),
            "https://fixture.ue.gateway.dev/api/v1/data-distribution"
        );
        assert_eq!(ds_client_core::DATA_DISTRIBUTION_METHOD, "POST");
        assert_eq!(ds_client_core::DATA_DISTRIBUTION_TIMEOUT_SECONDS, 180);
        assert_eq!(
            ds_client_core::DATA_DISTRIBUTION_RESPONSE_LIMIT,
            32 * 1024 * 1024
        );
        assert_eq!(
            ds_client_core::DATA_DISTRIBUTION_ACTIONS,
            ["list_datasets", "query_print_context"]
        );
        assert_eq!(
            ds_client_core::BUNDLE_DOWNLOAD_HOST,
            "storage.googleapis.com"
        );
    }

    #[test]
    fn correlation_values_are_fresh_bounded_and_nonsecret() {
        let (one, action) = correlation_headers();
        let (two, _) = correlation_headers();
        assert_ne!(one, two);
        assert_eq!(one, action);
        assert!(one.len() <= ds_client_core::CORRELATION_ID_MAX_BYTES);
        assert_eq!(one.len(), 36);
        assert_eq!(one.as_bytes()[14], b'4');
        assert_eq!(one.matches('-').count(), 4);
    }

    #[test]
    fn transformer_wire_target_and_limits_are_fixed() {
        assert_eq!(
            format!(
                "https://fixture.ue.gateway.dev{}",
                ds_client_core::TRANSFORMER_CONTEXT_PATH
            ),
            "https://fixture.ue.gateway.dev/report"
        );
        assert_eq!(ds_client_core::TRANSFORMER_CONTEXT_METHOD, "POST");
        assert_eq!(ds_client_core::TRANSFORMER_CONTEXT_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::TRANSFORMER_CONTEXT_RESPONSE_LIMIT,
            64 * 1024 * 1024
        );
    }

    #[test]
    fn project_forms_wire_target_and_limits_are_fixed() {
        assert_eq!(
            format!(
                "https://fixture.ue.gateway.dev{}",
                ds_client_core::PROJECT_FORMS_PATH
            ),
            "https://fixture.ue.gateway.dev/api/v1/project-forms"
        );
        assert_eq!(ds_client_core::PROJECT_FORMS_METHOD, "POST");
        assert_eq!(ds_client_core::PROJECT_FORMS_ACTION, "activate");
        assert_eq!(
            ds_client_core::PROJECT_FORM_EDITOR_ACTION,
            "settings_editor"
        );
        assert_eq!(ds_client_core::PROJECT_FORMS_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::PROJECT_FORMS_RESPONSE_LIMIT,
            32 * 1024 * 1024
        );
    }

    #[test]
    fn solar_snapshot_wire_target_and_limits_are_fixed() {
        assert_eq!(
            format!(
                "https://fixture.ue.gateway.dev{}",
                ds_client_core::SOLAR_SNAPSHOT_PATH
            ),
            "https://fixture.ue.gateway.dev/api/v1/solar"
        );
        assert_eq!(ds_client_core::SOLAR_SNAPSHOT_METHOD, "POST");
        assert_eq!(ds_client_core::SOLAR_SNAPSHOT_ACTION, "desktop_snapshot");
        assert_eq!(ds_client_core::SOLAR_SNAPSHOT_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::SOLAR_SNAPSHOT_RESPONSE_LIMIT,
            32 * 1024 * 1024
        );
    }

    #[test]
    fn survey_query_wire_target_and_limits_are_fixed() {
        assert_eq!(
            format!(
                "https://fixture.ue.gateway.dev{}",
                ds_client_core::SURVEY_QUERY_PATH
            ),
            "https://fixture.ue.gateway.dev/api/v1/survey/query"
        );
        assert_eq!(ds_client_core::SURVEY_QUERY_METHOD, "POST");
        assert_eq!(ds_client_core::SURVEY_QUERY_TIMEOUT_SECONDS, 120);
        assert_eq!(ds_client_core::SURVEY_QUERY_RESPONSE_LIMIT, 1024 * 1024);
    }

    #[test]
    fn survey_entries_select_wire_target_and_limits_are_fixed() {
        assert_eq!(
            format!(
                "https://fixture.ue.gateway.dev{}",
                ds_client_core::SURVEY_ENTRIES_SELECT_PATH
            ),
            "https://fixture.ue.gateway.dev/api/v1/survey/entries/select"
        );
        assert_eq!(ds_client_core::SURVEY_ENTRIES_SELECT_METHOD, "POST");
        assert_eq!(ds_client_core::SURVEY_ENTRIES_SELECT_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::SURVEY_ENTRIES_SELECT_RESPONSE_LIMIT,
            1024 * 1024
        );
    }

    #[test]
    fn survey_entries_changes_wire_target_and_limits_are_fixed() {
        assert_eq!(
            format!(
                "https://fixture.ue.gateway.dev{}",
                ds_client_core::SURVEY_ENTRIES_CHANGES_PATH
            ),
            "https://fixture.ue.gateway.dev/api/v1/survey/entries/changes"
        );
        assert_eq!(ds_client_core::SURVEY_ENTRIES_CHANGES_METHOD, "POST");
        assert_eq!(ds_client_core::SURVEY_ENTRIES_CHANGES_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::SURVEY_ENTRIES_CHANGES_RESPONSE_LIMIT,
            1024 * 1024
        );
    }

    #[test]
    fn survey_entry_create_wire_target_and_limits_are_fixed() {
        assert_eq!(
            survey_entry_create_url("test-project"),
            "https://firestore.googleapis.com/v1/projects/test-project/databases/(default)/documents:commit"
        );
        assert_eq!(ds_client_core::SURVEY_ENTRY_CREATE_METHOD, "POST");
        assert_eq!(ds_client_core::SURVEY_ENTRY_CREATE_OPERATION, "create");
        assert_eq!(ds_client_core::SURVEY_ENTRY_CREATE_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::SURVEY_ENTRY_CREATE_RESPONSE_LIMIT,
            1024 * 1024
        );
    }

    #[test]
    fn project_report_wire_target_and_limits_are_fixed() {
        assert_eq!(
            format!(
                "https://fixture.ue.gateway.dev{}",
                ds_client_core::PROJECT_REPORT_PATH
            ),
            "https://fixture.ue.gateway.dev/report"
        );
        assert_eq!(ds_client_core::PROJECT_REPORT_METHOD, "POST");
        assert_eq!(ds_client_core::PROJECT_REPORT_TIMEOUT_SECONDS, 600);
        assert_eq!(ds_client_core::PROJECT_REPORT_READ_TIMEOUT_SECONDS, 120);
        assert_eq!(
            ds_client_core::PROJECT_REPORT_RESPONSE_LIMIT,
            8 * 1024 * 1024
        );
        assert_eq!(
            ds_client_core::PROCESSING_LANE_HEADER,
            "X-DS-Processing-Lane"
        );
    }

    #[test]
    fn tiles_wire_target_and_limits_are_fixed() {
        assert_eq!(
            format!(
                "https://fixture.ue.gateway.dev{}",
                ds_client_core::TILES_PATH
            ),
            "https://fixture.ue.gateway.dev/api/v1/tiles"
        );
        assert_eq!(ds_client_core::TILES_METHOD, "POST");
        assert_eq!(ds_client_core::TILE_TIMEOUT_SECONDS, 120);
        assert_eq!(ds_client_core::TILE_RESPONSE_LIMIT, 32 * 1024 * 1024);
    }
}

/// Read/write only a source-owned body, bounded by its descriptor. The reader
/// remains the existing fixed native HTTP adapter; cancellation is checked
/// between chunks and before publication, not simulated by a dropped promise.
fn stream_preview_body(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    total: u64,
    control: &dyn ds_client_core::asset_preview_source::TransferControl,
) -> Result<u64, ds_client_core::asset_preview_source::TransferError> {
    use ds_client_core::asset_preview_source::TransferError;
    let mut buffer = [0u8; 64 * 1024];
    let mut received = 0u64;
    loop {
        control.check_current()?;
        let count = reader
            .read(&mut buffer)
            .map_err(|_| TransferError::Unreachable)?;
        control.check_current()?;
        if count == 0 {
            break;
        }
        received = received
            .checked_add(count as u64)
            .ok_or(TransferError::SourceChanged)?;
        if received > total {
            return Err(TransferError::SourceChanged);
        }
        writer
            .write_all(&buffer[..count])
            .map_err(|_| TransferError::WriteFailed)?;
        control.progress(received, total);
    }
    if received != total {
        return Err(TransferError::SourceChanged);
    }
    control.check_current()?;
    Ok(received)
}

#[cfg(test)]
mod asset_preview_stream_tests {
    use super::*;
    use ds_client_core::asset_preview_source::{TransferControl, TransferError};
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    struct Guard {
        cancelled: AtomicBool,
        progress: AtomicUsize,
        cancel_on_progress: bool,
    }
    impl TransferControl for Guard {
        fn check_current(&self) -> Result<(), TransferError> {
            if self.cancelled.load(Ordering::SeqCst) {
                Err(TransferError::Cancelled)
            } else {
                Ok(())
            }
        }
        fn progress(&self, _: u64, _: u64) {
            self.progress.fetch_add(1, Ordering::SeqCst);
            if self.cancel_on_progress {
                self.cancelled.store(true, Ordering::SeqCst);
            }
        }
    }
    fn guard(cancel_on_progress: bool) -> Guard {
        Guard {
            cancelled: AtomicBool::new(false),
            progress: AtomicUsize::new(0),
            cancel_on_progress,
        }
    }
    #[test]
    fn verified_source_body_streams_exact_bytes_and_actual_progress() {
        let bytes = vec![7; 128 * 1024 + 1];
        let mut output = Vec::new();
        let control = guard(false);
        assert_eq!(
            stream_preview_body(
                &mut std::io::Cursor::new(&bytes),
                &mut output,
                bytes.len() as u64,
                &control
            )
            .unwrap(),
            bytes.len() as u64
        );
        assert_eq!(output, bytes);
        assert_eq!(control.progress.load(Ordering::SeqCst), 3);
    }
    #[test]
    fn short_or_excess_source_never_reports_complete() {
        for declared in [2, 4] {
            let mut output = Vec::new();
            assert_eq!(
                stream_preview_body(
                    &mut std::io::Cursor::new(b"abc"),
                    &mut output,
                    declared,
                    &guard(false)
                ),
                Err(TransferError::SourceChanged)
            );
        }
    }
    #[test]
    fn native_cancellation_stops_before_a_second_source_chunk() {
        let bytes = vec![7; 128 * 1024];
        let mut output = Vec::new();
        let control = guard(true);
        assert_eq!(
            stream_preview_body(
                &mut std::io::Cursor::new(&bytes),
                &mut output,
                bytes.len() as u64,
                &control
            ),
            Err(TransferError::Cancelled)
        );
        assert_eq!(output.len(), 64 * 1024);
        assert_eq!(control.progress.load(Ordering::SeqCst), 1);
    }
}
