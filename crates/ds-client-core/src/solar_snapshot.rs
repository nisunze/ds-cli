use ds_command_kernel::is_solar_id as canonical_id;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;
use zeroize::Zeroize;

use crate::{ClientError, ErrorKind, TransportResponse};

const SNAPSHOT_MAX_BYTES: usize = 8 * 1024 * 1024;
const SNAPSHOT_MAX_DOCUMENTS: usize = 512;
const SNAPSHOT_MAX_MEDIA: usize = 512;
const MEDIA_MAX_BYTES: i64 = 16 * 1024 * 1024;
const TEXT_MAX_BYTES: usize = 4 * 1024;

/// One exact `desktop_snapshot` success payload.
///
/// The serialized intake bytes include short-lived signed download URLs and
/// are therefore private, zeroized on drop, and intended only for immediate
/// transfer to the fixed ds-solar stdin intake contract. Public accessors
/// expose provenance and the canonical city document, never transport authority.
pub struct SolarSnapshot {
    intake_bytes: Vec<u8>,
    document_json: String,
    ds_project: String,
    template_id: String,
    snapshot_sha256: String,
    input_base_fingerprint: String,
    snapshot_receipt_id: String,
    snapshot_receipt_expires_at: String,
    firestore_read_time: String,
    snapshot_bytes: usize,
    source_document_count: usize,
    media_download_count: usize,
}

impl SolarSnapshot {
    /// Exact validated canonical city document; excludes signed download descriptors.
    pub fn document_json(&self) -> &str {
        &self.document_json
    }
    pub fn ds_project(&self) -> &str {
        &self.ds_project
    }
    pub fn template_id(&self) -> &str {
        &self.template_id
    }
    pub fn snapshot_sha256(&self) -> &str {
        &self.snapshot_sha256
    }
    pub fn input_base_fingerprint(&self) -> &str {
        &self.input_base_fingerprint
    }
    pub fn snapshot_receipt_id(&self) -> &str {
        &self.snapshot_receipt_id
    }
    pub fn snapshot_receipt_expires_at(&self) -> &str {
        &self.snapshot_receipt_expires_at
    }
    pub fn firestore_read_time(&self) -> &str {
        &self.firestore_read_time
    }
    pub const fn snapshot_bytes(&self) -> usize {
        self.snapshot_bytes
    }
    pub const fn source_document_count(&self) -> usize {
        self.source_document_count
    }
    pub const fn media_download_count(&self) -> usize {
        self.media_download_count
    }

    /// Borrow the bounded exact data object for one immediate owner-process
    /// stdin transfer. Callers must never log or persist these bytes.
    pub fn intake_bytes(&self) -> &[u8] {
        &self.intake_bytes
    }
}

/// Transport authority derived only from a validated governed snapshot.
/// Ordinary callers cannot construct a URL download request.
pub struct SolarMediaDownloadCall<'a> {
    url: &'a str,
    response_limit: usize,
}
impl SolarMediaDownloadCall<'_> {
    pub fn url(&self) -> &str {
        self.url
    }
    pub fn response_limit(&self) -> usize {
        self.response_limit
    }
}
impl SolarSnapshot {
    pub(crate) fn download_media<T: crate::AuthorizedTransport>(
        &self,
        transport: &mut T,
        reference: &str,
    ) -> Result<Value, ClientError> {
        use base64::Engine;
        let data: SnapshotWire =
            serde_json::from_slice(&self.intake_bytes).map_err(|_| unreadable())?;
        let download = data
            .media_downloads
            .iter()
            .find(|d| {
                format!(
                    "gs://{}/{}?generation={}",
                    d.r#ref.bucket, d.r#ref.object, d.r#ref.generation
                ) == reference
            })
            .ok_or_else(|| {
                ClientError::new(
                    ErrorKind::ResourceNotFound,
                    "The exact immutable map is not present in the governed city snapshot",
                )
            })?;
        let url = Url::parse(&download.download_url).map_err(|_| unreadable())?;
        let host = url.host_str().ok_or_else(unreadable)?;
        if !matches!(
            host,
            "storage.googleapis.com" | "firebasestorage.googleapis.com"
        ) && !host.ends_with(".storage.googleapis.com")
        {
            return Err(unreadable());
        }
        let response = transport.download_solar_media(SolarMediaDownloadCall {
            url: &download.download_url,
            response_limit: download.r#ref.size_bytes as usize,
        })?;
        let r = &download.r#ref;
        if response.status != 200
            || response.body.len() != r.size_bytes as usize
            || format!("{:x}", Sha256::digest(&response.body)) != r.sha256
            || crate::solar_media::media_type(&response.body)? != r.media_type
        {
            return Err(unreadable());
        }
        Ok(
            serde_json::json!({"reference":reference,"project_id":self.ds_project,"city_id":self.template_id,"media_type":r.media_type,"byte_count":r.size_bytes,"content_digest":format!("sha256:{}",r.sha256),"body_base64":base64::engine::general_purpose::STANDARD.encode(&response.body)}),
        )
    }
}

impl Drop for SolarSnapshot {
    fn drop(&mut self) {
        self.intake_bytes.zeroize();
        self.document_json.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuccessEnvelope {
    success: bool,
    message: String,
    data: SnapshotWire,
    timestamp: String,
    service: String,
    version: String,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotWire {
    snapshot_json: String,
    snapshot_sha256: String,
    input_base_fingerprint: String,
    media_downloads: Vec<MediaDownloadWire>,
    snapshot_bytes: usize,
    source_document_count: usize,
    firestore_read_time: String,
    snapshot_receipt_id: String,
    snapshot_receipt_expires_at: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MediaDownloadWire {
    r#ref: MediaRefWire,
    download_url: String,
}

impl Drop for MediaDownloadWire {
    fn drop(&mut self) {
        self.download_url.zeroize();
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MediaRefWire {
    schema: i64,
    kind: String,
    media_id: String,
    logical_role: String,
    bucket: String,
    object: String,
    generation: i64,
    size_bytes: i64,
    media_type: String,
    sha256: String,
    file_name: String,
}

/// The one closed request body of a governed snapshot read: the fixed action,
/// the project root derived from the selected project, and one city.
pub(crate) fn request_body(ds_project: &str, template_id: &str) -> Vec<u8> {
    #[derive(Serialize)]
    struct Body<'a> {
        action: &'static str,
        root: String,
        template_id: &'a str,
    }
    serde_json::to_vec(&Body {
        action: crate::profile::SOLAR_SNAPSHOT_ACTION,
        root: format!("eds_project/{ds_project}/eds_solar"),
        template_id,
    })
    .expect("a struct containing only strings always serializes")
}

/// One governed snapshot read that a host performs under a session credential
/// it already holds: the Desktop shell's webview-supplied bearer, which the
/// shell attaches to its pinned gateway origin exactly as its sync door does.
///
/// The core fixes the method, path, action, root and body, and decodes the
/// exact answer with the same strict decoder as a native session's read. The
/// host chooses no route or field; it only moves bytes and returns the status
/// and body it received.
pub struct HeldSessionSnapshotRead {
    ds_project: String,
    template_id: String,
}

impl HeldSessionSnapshotRead {
    pub fn new(ds_project: &str, template_id: &str) -> Result<Self, ClientError> {
        validate_input(ds_project, template_id)?;
        Ok(Self {
            ds_project: ds_project.to_owned(),
            template_id: template_id.to_owned(),
        })
    }
    pub const fn method(&self) -> &'static str {
        crate::profile::SOLAR_SNAPSHOT_METHOD
    }
    pub const fn path(&self) -> &'static str {
        crate::profile::SOLAR_SNAPSHOT_PATH
    }
    pub const fn timeout_seconds(&self) -> u64 {
        crate::SOLAR_SNAPSHOT_TIMEOUT_SECONDS
    }
    pub const fn response_limit(&self) -> usize {
        crate::SOLAR_SNAPSHOT_RESPONSE_LIMIT
    }
    pub fn body(&self) -> crate::SecretRequestBody {
        crate::SecretRequestBody::new(request_body(&self.ds_project, &self.template_id))
    }
    /// The answer to this exact read, or the same typed refusal a native
    /// session receives for the same status.
    pub fn decode(&self, response: TransportResponse) -> Result<SolarSnapshot, ClientError> {
        if response.status != 200 {
            return Err(crate::authorized::classify_solar_snapshot_error(
                response.status,
            ));
        }
        decode_snapshot(
            response,
            self.response_limit(),
            &self.ds_project,
            &self.template_id,
        )
    }
}

pub(crate) fn validate_input(ds_project: &str, template_id: &str) -> Result<(), ClientError> {
    if !canonical_id(ds_project) || !canonical_id(template_id) {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "the selected project or Solar city is outside the canonical 128-character id contract",
        ));
    }
    Ok(())
}

pub(crate) fn decode_snapshot(
    response: TransportResponse,
    response_limit: usize,
    ds_project: &str,
    template_id: &str,
) -> Result<SolarSnapshot, ClientError> {
    if response.body.len() > response_limit {
        return Err(unreadable());
    }
    let envelope: SuccessEnvelope =
        serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if !envelope.success
        || envelope.message != "Desktop Solar snapshot prepared"
        || envelope.service != "data-solutions-backend"
        || envelope.version != "1.0.0"
        || !bounded_text(&envelope.timestamp, 128)
        || envelope
            .request_id
            .as_deref()
            .is_some_and(|value| !bounded_text(value, 128))
    {
        return Err(unreadable());
    }

    let data = envelope.data;
    if data.snapshot_json.is_empty()
        || data.snapshot_bytes != data.snapshot_json.len()
        || data.snapshot_bytes > SNAPSHOT_MAX_BYTES
        || data.source_document_count == 0
        || data.source_document_count > SNAPSHOT_MAX_DOCUMENTS
        || data.media_downloads.len() > SNAPSHOT_MAX_MEDIA
        || data.media_downloads.len() > data.source_document_count
        || !lower_sha256(&data.snapshot_sha256)
        || !lower_sha256(&data.input_base_fingerprint)
        || format!("{:x}", Sha256::digest(data.snapshot_json.as_bytes())) != data.snapshot_sha256
        || !bounded_text(&data.firestore_read_time, 128)
        || !canonical_uuid(&data.snapshot_receipt_id)
        || !bounded_text(&data.snapshot_receipt_expires_at, 128)
    {
        return Err(unreadable());
    }

    let expected_root = format!("eds_project/{ds_project}/eds_solar");
    validate_snapshot_json(
        &data.snapshot_json,
        &expected_root,
        template_id,
        &data.input_base_fingerprint,
    )?;
    for download in &data.media_downloads {
        validate_media_download(download, &expected_root, template_id)?;
    }

    let intake_bytes = serde_json::to_vec(&data).map_err(|_| unreadable())?;
    if intake_bytes.len() > response_limit {
        return Err(unreadable());
    }
    Ok(SolarSnapshot {
        intake_bytes,
        document_json: data.snapshot_json,
        ds_project: ds_project.to_owned(),
        template_id: template_id.to_owned(),
        snapshot_sha256: data.snapshot_sha256,
        input_base_fingerprint: data.input_base_fingerprint,
        snapshot_receipt_id: data.snapshot_receipt_id,
        snapshot_receipt_expires_at: data.snapshot_receipt_expires_at,
        firestore_read_time: data.firestore_read_time,
        snapshot_bytes: data.snapshot_bytes,
        source_document_count: data.source_document_count,
        media_download_count: data.media_downloads.len(),
    })
}

fn validate_snapshot_json(
    encoded: &str,
    expected_root: &str,
    expected_city: &str,
    expected_fingerprint: &str,
) -> Result<(), ClientError> {
    let value: Value = serde_json::from_str(encoded).map_err(|_| unreadable())?;
    let object = value.as_object().ok_or_else(unreadable)?;
    let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
    keys.sort_unstable();
    if keys
        != [
            "00_templates",
            "01_city_inputs",
            "_config",
            "_provenance",
            "_root",
        ]
    {
        return Err(unreadable());
    }
    let provenance = object
        .get("_provenance")
        .and_then(Value::as_object)
        .ok_or_else(unreadable)?;
    let mut provenance_keys = provenance.keys().map(String::as_str).collect::<Vec<_>>();
    provenance_keys.sort_unstable();
    if provenance_keys
        != [
            "context",
            "demand_noise_seed",
            "input_base_fingerprint",
            "root",
            "schema",
            "source_kind",
        ]
        || provenance.get("schema").and_then(Value::as_u64) != Some(3)
        || provenance.get("source_kind").and_then(Value::as_str)
            != Some("authenticated ds-brain desktop snapshot")
        || provenance.get("root").and_then(Value::as_str) != Some(expected_root)
        || provenance.get("context").and_then(Value::as_str) != Some(expected_city)
        || provenance
            .get("input_base_fingerprint")
            .and_then(Value::as_str)
            != Some(expected_fingerprint)
        || provenance
            .get("demand_noise_seed")
            .and_then(Value::as_u64)
            .is_none()
    {
        return Err(unreadable());
    }
    Ok(())
}

fn validate_media_download(
    download: &MediaDownloadWire,
    expected_root: &str,
    expected_city: &str,
) -> Result<(), ClientError> {
    let reference = &download.r#ref;
    let expected_prefix = format!("{expected_root}/{expected_city}/");
    if reference.schema != 1
        || reference.kind != "solar_network_media"
        || !canonical_uuid(&reference.media_id)
        || !canonical_media_role(&reference.logical_role)
        || !bounded_text(&reference.bucket, 512)
        || !bounded_text(&reference.object, TEXT_MAX_BYTES)
        || !reference.object.starts_with(&expected_prefix)
        || reference.generation <= 0
        || !(1..=MEDIA_MAX_BYTES).contains(&reference.size_bytes)
        || !matches!(
            reference.media_type.as_str(),
            "image/jpeg" | "image/png" | "image/webp"
        )
        || !lower_sha256(&reference.sha256)
        || !canonical_media_file_name(&reference.file_name)
    {
        return Err(unreadable());
    }
    let url = Url::parse(&download.download_url).map_err(|_| unreadable())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || download.download_url.len() > 16 * 1024
    {
        return Err(unreadable());
    }
    Ok(())
}

pub(crate) fn canonical_media_role(value: &str) -> bool {
    let Some((prefix, role)) = value.split_once(':') else {
        return false;
    };
    let mut chars = role.chars();
    matches!(prefix, "network.map" | "network.transformer")
        && role.len() <= 128
        && chars
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
        && chars.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
        })
}

pub(crate) fn canonical_media_file_name(value: &str) -> bool {
    bounded_text(value, 255) && value != "." && value != ".." && !value.contains(['/', '\\'])
}

pub(crate) fn canonical_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            14 => matches!(*byte, b'1'..=b'5'),
            19 => matches!(*byte, b'8' | b'9' | b'a' | b'b'),
            _ => byte.is_ascii_digit() || matches!(*byte, b'a'..=b'f'),
        })
}

fn lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn bounded_text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.len() <= max
        && !value.chars().any(char::is_control)
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the Solar snapshot response is outside its exact bounded success contract",
    )
}
