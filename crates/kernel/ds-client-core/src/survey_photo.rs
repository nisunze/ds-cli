//! Headless survey-photo preparation and retry-safe publication. No browser host.
use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    TransportResponse, UploadBytesCall,
};
use ds_command_kernel::{
    survey_media_address, survey_photo_media as pixels, survey_photo_transport as wire,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "ds.survey-photo-rotation/v1";
/// The gateway route of the `survey_media` operation
/// (`routing/operations.json`): the API Gateway defines `/api/v1/media`
/// and answers `/media` with its own 404 ("not defined by this API"),
/// which the client read as a refusal — so headless rotation never reached
/// ds-brain until 2026-09-20.
pub const PATH: &str = "/api/v1/media";
pub const RESPONSE_LIMIT: usize = 32 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub project_id: String,
    pub account_id: String,
    pub credential_audience_sha256: String,
    pub bucket: String,
    pub object_path: String,
    pub thumbnail_path: String,
    pub expected_generation: String,
    pub thumbnail_expected_generation: Option<String>,
    pub degrees: u16,
    pub media_type: String,
    pub original_size: usize,
    pub thumbnail_size: usize,
    pub original_sha256: String,
    pub thumbnail_sha256: String,
    pub original_width: u32,
    pub original_height: u32,
    pub thumbnail_width: u32,
    pub thumbnail_height: u32,
}
pub struct Prepared {
    pub manifest: Manifest,
    pub original: Vec<u8>,
    pub thumbnail: Vec<u8>,
}
pub enum Command<'a> {
    Prepare {
        object_path: &'a str,
        degrees: u16,
        expected_generation: Option<&'a str>,
    },
    Publish {
        manifest: &'a Manifest,
        original: &'a [u8],
        thumbnail: &'a [u8],
    },
}
pub enum Receipt {
    Prepared(Box<Prepared>),
    Published(Value),
}
fn invalid(_message: impl Into<String>) -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "Invalid photo address, angle, or rotation bundle",
    )
}
fn unreadable(_message: impl Into<String>) -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "Photo service returned an invalid or unverified response",
    )
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn address(project: &str, path: &str) -> Result<Value, ClientError> {
    let a = survey_media_address::parse(path).map_err(invalid)?;
    if a.project != project || a.object_path != path || a.object_path == a.thumbnail_object_path {
        return Err(invalid("Photo must be an original in the explicit project"));
    }
    Ok(survey_media_address::wire(&a))
}
fn response(response: TransportResponse) -> Result<Value, ClientError> {
    if !(200..300).contains(&response.status) {
        let kind = if matches!(response.status, 408 | 425 | 429 | 500..=599) {
            ErrorKind::Transient
        } else {
            ErrorKind::AuthenticationRejected
        };
        return Err(ClientError::new(
            kind,
            "Survey media request refused; keep the prepared bundle for retry",
        ));
    }
    if response.body.len() > RESPONSE_LIMIT {
        return Err(unreadable("Survey media response is oversized"));
    }
    let value: Value = serde_json::from_slice(&response.body)
        .map_err(|_| unreadable("Survey media response is not JSON"))?;
    Ok(value
        .get("data")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or(value))
}
fn call<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    path: &str,
    action: &str,
    extra: Value,
) -> Result<TransportResponse, ClientError> {
    let mut payload =
        survey_media_address::wire(&survey_media_address::parse(path).map_err(invalid)?);
    payload["action"] = json!(action);
    for (key, value) in extra
        .as_object()
        .ok_or_else(|| invalid("Invalid photo request"))?
    {
        payload[key] = value.clone();
    }
    transport
        .survey_photo(crate::SurveyPhotoCall {
            profile,
            payload: &payload,
            authority: authority.call(),
        })
        .map_err(Into::into)
}
fn inspect<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    path: &str,
    missing: bool,
) -> Result<Option<wire::Inspection>, ClientError> {
    let reply = call(
        profile,
        transport,
        authority,
        path,
        "survey_upload_inspect",
        json!({}),
    )?;
    if missing && reply.status == 404 {
        return Ok(None);
    }
    wire::inspect(&response(reply)?, path)
        .map(Some)
        .map_err(unreadable)
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: Command<'_>,
) -> Result<Receipt, ClientError> {
    crate::authorized::validate_tiles(project)?;
    match command {
        Command::Prepare {
            object_path,
            degrees,
            expected_generation,
        } => {
            address(project, object_path)?;
            if !matches!(degrees, 90 | 180 | 270) {
                return Err(invalid("Choose 90, 180, or 270 clockwise degrees"));
            }
            let source = inspect(profile, transport, authority, object_path, false)?
                .ok_or_else(|| unreadable("Source photo is missing"))?;
            if expected_generation.is_some_and(|g| g != source.generation) {
                return Err(invalid(
                    "Photo generation changed before rotation; inspect the new image first",
                ));
            }
            let thumbnail_path =
                ds_command_kernel::survey_photo_outbox::thumbnail_path_for(object_path)
                    .map_err(invalid)?;
            let previous_thumbnail = inspect(profile, transport, authority, &thumbnail_path, true)?;
            if previous_thumbnail
                .as_ref()
                .is_some_and(|v| v.bucket != source.bucket)
            {
                return Err(unreadable("Photo and thumbnail buckets differ"));
            }
            let downloaded = transport.download_survey_photo(DownloadCall {
                url: &source.source_url,
            })?;
            if downloaded.status != 200
                || downloaded.body.is_empty()
                || downloaded.body.len() > pixels::MAX_INPUT_BYTES
            {
                return Err(unreadable(
                    "Photo download failed or exceeded its byte bound",
                ));
            }
            let bundle = rotate_local(&downloaded.body, degrees)?;
            let manifest = Manifest {
                schema: SCHEMA.into(),
                project_id: project.into(),
                account_id: authority.uid.into(),
                credential_audience_sha256: profile.credential_audience_sha256().into(),
                bucket: source.bucket,
                object_path: object_path.into(),
                thumbnail_path,
                expected_generation: source.generation,
                thumbnail_expected_generation: previous_thumbnail.map(|v| v.generation),
                degrees,
                media_type: bundle.media_type.into(),
                original_size: bundle.original_bytes.len(),
                thumbnail_size: bundle.thumbnail_bytes.len(),
                original_sha256: hash(&bundle.original_bytes),
                thumbnail_sha256: hash(&bundle.thumbnail_bytes),
                original_width: bundle.original_width,
                original_height: bundle.original_height,
                thumbnail_width: bundle.thumbnail_width,
                thumbnail_height: bundle.thumbnail_height,
            };
            Ok(Receipt::Prepared(Box::new(Prepared {
                manifest,
                original: bundle.original_bytes,
                thumbnail: bundle.thumbnail_bytes,
            })))
        }
        Command::Publish {
            manifest,
            original,
            thumbnail,
        } => {
            validate_manifest(project, authority.uid, manifest, original, thumbnail)?;
            if manifest.credential_audience_sha256 != profile.credential_audience_sha256() {
                return Err(invalid(
                    "Rotation bundle belongs to another deployment audience",
                ));
            }
            let original_generation = upload(
                profile,
                transport,
                authority,
                &manifest.object_path,
                &manifest.bucket,
                original,
                &manifest.media_type,
                Some(&manifest.expected_generation),
                None,
            )?;
            let a = survey_media_address::parse(&manifest.object_path).map_err(invalid)?;
            let source = json!({"source_file_name":a.file,"source_generation":original_generation});
            let thumbnail_generation = upload(
                profile,
                transport,
                authority,
                &manifest.thumbnail_path,
                &manifest.bucket,
                thumbnail,
                "image/jpeg",
                manifest.thumbnail_expected_generation.as_deref(),
                Some(source),
            )?;
            Ok(Receipt::Published(
                json!({"publication":"published","project_id":project,"object_path":manifest.object_path,"thumbnail_path":manifest.thumbnail_path,
                "generation":original_generation,"thumbnail_generation":thumbnail_generation,"original_sha256":manifest.original_sha256,"thumbnail_sha256":manifest.thumbnail_sha256,"degrees":manifest.degrees}),
            ))
        }
    }
}

pub fn validate_manifest(
    project: &str,
    account: &str,
    m: &Manifest,
    original: &[u8],
    thumbnail: &[u8],
) -> Result<(), ClientError> {
    address(project, &m.object_path)?;
    if m.schema != SCHEMA
        || m.project_id != project
        || m.account_id != account
        || !matches!(m.degrees, 90 | 180 | 270)
        || m.thumbnail_path
            != ds_command_kernel::survey_photo_outbox::thumbnail_path_for(&m.object_path)
                .map_err(invalid)?
        || !matches!(m.media_type.as_str(), "image/jpeg" | "image/png")
        || original.is_empty()
        || original.len() > pixels::MAX_INPUT_BYTES
        || thumbnail.is_empty()
        || thumbnail.len() > pixels::MAX_INPUT_BYTES
        || original.len() != m.original_size
        || thumbnail.len() != m.thumbnail_size
        || hash(original) != m.original_sha256
        || hash(thumbnail) != m.thumbnail_sha256
    {
        return Err(invalid(
            "Rotation bundle bytes, identity, or schema changed",
        ));
    }
    wire::generation(&json!(m.expected_generation)).map_err(invalid)?;
    if let Some(generation) = &m.thumbnail_expected_generation {
        wire::generation(&json!(generation)).map_err(invalid)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn upload<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    path: &str,
    bucket: &str,
    bytes: &[u8],
    media_type: &str,
    expected: Option<&str>,
    source: Option<Value>,
) -> Result<String, ClientError> {
    let mut payload = json!({"size":bytes.len(),"content_type":media_type,"sha256":hash(bytes)});
    if let Some(expected) = expected {
        payload["expected_generation"] = json!(expected);
    }
    if let Some(source) = source {
        for (key, value) in source.as_object().expect("source fields") {
            payload[key] = value.clone();
        }
    }
    let started = response(call(
        profile,
        transport,
        authority,
        path,
        "survey_upload_start",
        payload.clone(),
    )?)?;
    if wire::target(&started, path).map_err(unreadable)? != bucket {
        return Err(unreadable("Photo upload bucket changed"));
    }
    if started["already_present"] == true {
        return wire::generation(&started["generation"]).map_err(unreadable);
    }
    let uri = wire::session(&started["session_uri"], bucket).map_err(unreadable)?;
    let uploaded = transport.upload_bytes(UploadBytesCall {
        uri: &uri,
        size: bytes.len() as u64,
        reader: &mut std::io::Cursor::new(bytes),
    })?;
    crate::project_data::uploaded(uploaded)?;
    let complete = response(call(
        profile,
        transport,
        authority,
        path,
        "survey_upload_complete",
        payload,
    )?)?;
    if wire::target(&complete, path).map_err(unreadable)? != bucket
        || complete["already_present"] != true
    {
        return Err(unreadable("Photo publication was not verified"));
    }
    wire::generation(&complete["generation"]).map_err(unreadable)
}

/// Local image edits share the native/WASM pixel owner; no authority or upload.
pub fn rotate_local(bytes: &[u8], degrees: u16) -> Result<pixels::PhotoBundle, ClientError> {
    if !matches!(degrees, 90 | 180 | 270) {
        return Err(invalid("Choose 90, 180, or 270 clockwise degrees"));
    }
    pixels::process(bytes, pixels::Rotation::try_from(degrees).map_err(invalid)?).map_err(invalid)
}

/// A core-validated generation-pinned source URL. No user credentials accompany it.
pub struct DownloadCall<'a> {
    pub(crate) url: &'a str,
}
impl DownloadCall<'_> {
    pub fn url(&self) -> &str {
        self.url
    }
    pub const fn response_limit(&self) -> usize {
        pixels::MAX_INPUT_BYTES
    }
}
