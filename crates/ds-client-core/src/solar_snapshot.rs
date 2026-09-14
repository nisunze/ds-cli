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
/// expose provenance, never transport authority.
pub struct SolarSnapshot {
    intake_bytes: Vec<u8>,
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

impl Drop for SolarSnapshot {
    fn drop(&mut self) {
        self.intake_bytes.zeroize();
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

fn canonical_id(value: &str) -> bool {
    let mut chars = value.chars();
    value.len() <= 128
        && chars
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
        && chars
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
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
