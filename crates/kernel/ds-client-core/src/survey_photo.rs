//! Headless survey-photo rotation: entry metadata plus a regenerated
//! thumbnail. The camera original is never rewritten (owner, 2026-10-21).
//!
//! `Prepare` inspects the original at its storage generation, reads the entry
//! the photo belongs to, adds the turn to the entry's `ds_media_rotation`
//! record (`survey_media_rotation`), downloads the generation-pinned original
//! and makes its thumbnail showing the net turn. `Publish` commits the record
//! through ds-brain's mutate under the entry version that was read, then
//! uploads the thumbnail companion under its storage-generation precondition
//! with the original's generation and turns as its source. A publish whose
//! entry already committed resumes with the thumbnail alone.
use crate::{
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind, TransportResponse, UploadBytesCall,
    authorized::Authority,
};
use ds_command_kernel::{
    survey_media_address, survey_media_rotation, survey_photo_media as pixels,
    survey_photo_transport as wire,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "ds.survey-photo-rotation/v2";
/// The gateway route of the `survey_media` operation
/// (`routing/operations.json`): the API Gateway defines `/api/v1/media`
/// and answers `/media` with its own 404 ("not defined by this API"),
/// which the client read as a refusal — so headless rotation never reached
/// ds-brain until 2026-09-20.
pub const PATH: &str = "/api/v1/media";
pub const RESPONSE_LIMIT: usize = 32 * 1024;
/// The largest form read to find the entry a photo belongs to.
const ENTRY_READ_LIMIT: usize = crate::survey_entries_read::SURVEY_ENTRIES_READ_MAX_LIMIT;

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
    pub form_id: String,
    pub entry_id: String,
    /// The original's storage generation the turn is pinned to.
    pub generation: String,
    pub media_type: String,
    /// The original's bytes as read: unchanged by any rotation.
    pub original_size: usize,
    pub original_sha256: String,
    pub previous_quarter_turns: u8,
    /// Net clockwise quarter-turns the entry records after publishing.
    pub quarter_turns: u8,
    /// The entry version the turn was chosen against (mutate's fence).
    pub expected_version: u64,
    /// The exact `updated_data` patch: `{"ds_media_rotation": {...}}`.
    pub updated_data: Value,
    /// Set once the entry committed, so a retry sends only the thumbnail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_committed_version: Option<u64>,
    pub thumbnail_expected_generation: Option<String>,
    pub thumbnail_size: usize,
    pub thumbnail_sha256: String,
    /// The original as displayed after the turn.
    pub width: u32,
    pub height: u32,
    pub thumbnail_width: u32,
    pub thumbnail_height: u32,
}
pub struct Prepared {
    pub manifest: Manifest,
    /// The camera original exactly as stored.
    pub original: Vec<u8>,
    pub thumbnail: Vec<u8>,
}
pub enum Command<'a> {
    /// Add `degrees` (±90, ±180, ±270) to what the entry records — or, when
    /// `pending` names a turn still waiting on this machine, to that one.
    Prepare {
        object_path: &'a str,
        degrees: i64,
        expected_generation: Option<&'a str>,
        pending: Option<&'a Manifest>,
    },
    Publish {
        manifest: &'a Manifest,
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
fn address(
    project: &str,
    path: &str,
) -> Result<survey_media_address::SurveyMediaAddress, ClientError> {
    let a = survey_media_address::parse(path).map_err(invalid)?;
    if a.project != project || a.object_path != path || a.object_path == a.thumbnail_object_path {
        return Err(invalid("Photo must be an original in the explicit project"));
    }
    Ok(a)
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

/// The entry a photo belongs to, as the mirror holds it now.
fn entry_of<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    form: &str,
    entry: &str,
) -> Result<Value, ClientError> {
    let request = crate::survey_entries_read::SurveyEntriesReadRequest::new(
        form,
        None,
        None,
        false,
        Some(ENTRY_READ_LIMIT),
    )?;
    let read = crate::survey_entries_read::run(profile, transport, authority, project, &request)?;
    read.entries
        .into_iter()
        .map(|held| held.feature)
        .find(|feature| {
            let props = &feature["properties"];
            [&feature["id"], &props["id"], &props["doc_id"]]
                .iter()
                .any(|id| id.as_str() == Some(entry))
        })
        .ok_or_else(|| {
            ClientError::new(
                ErrorKind::ResourceNotFound,
                "The survey entry this photo belongs to is not readable",
            )
        })
}

fn client_version(feature: &Value) -> Result<u64, ClientError> {
    match feature["properties"]["metadata"].get("client_version") {
        None | Some(Value::Null) => Ok(0),
        Some(value) => value
            .as_u64()
            .ok_or_else(|| unreadable("Entry version is not a count")),
    }
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
            pending,
        } => {
            let a = address(project, object_path)?;
            let source = inspect(profile, transport, authority, object_path, false)?
                .ok_or_else(|| unreadable("Source photo is missing"))?;
            if expected_generation.is_some_and(|g| g != source.generation)
                || pending.is_some_and(|p| p.generation != source.generation)
            {
                return Err(invalid(
                    "Photo generation changed before rotation; inspect the new image first",
                ));
            }
            // The base the turn adds to: a turn still waiting here, or the
            // entry as it is now.
            let (data, expected_version) = match pending {
                Some(waiting) => {
                    if waiting.object_path != object_path || waiting.project_id != project {
                        return Err(invalid("The waiting turn belongs to another photo"));
                    }
                    (
                        waiting.updated_data.clone(),
                        waiting
                            .entry_committed_version
                            .unwrap_or(waiting.expected_version),
                    )
                }
                None => {
                    let feature =
                        entry_of(profile, transport, authority, project, &a.form, &a.entry)?;
                    let data = feature["properties"]["data"].clone();
                    (data, client_version(&feature)?)
                }
            };
            let data = data
                .as_object()
                .cloned()
                .ok_or_else(|| unreadable("Entry data is not an object"))?;
            let turned =
                survey_media_rotation::turn(&data, object_path, &source.generation, degrees)
                    .map_err(invalid)?;
            let previous_thumbnail = inspect(
                profile,
                transport,
                authority,
                &turned.thumbnail_object_path,
                true,
            )?;
            if previous_thumbnail
                .as_ref()
                .is_some_and(|v| v.bucket != source.bucket)
            {
                return Err(unreadable("Photo and thumbnail buckets differ"));
            }
            let mut downloaded = transport.download_survey_photo(DownloadCall {
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
            let original = std::mem::take(&mut downloaded.body);
            let rotation =
                pixels::Rotation::from_quarter_turns(turned.quarter_turns).map_err(invalid)?;
            let thumbnail = pixels::thumbnail(&original, rotation).map_err(invalid)?;
            let (width, height) =
                pixels::display_dimensions(&original, rotation).map_err(invalid)?;
            let manifest = Manifest {
                schema: SCHEMA.into(),
                project_id: project.into(),
                account_id: authority.uid.into(),
                credential_audience_sha256: profile.credential_audience_sha256().into(),
                bucket: source.bucket,
                object_path: object_path.into(),
                thumbnail_path: turned.thumbnail_object_path,
                form_id: a.form,
                entry_id: a.entry,
                generation: source.generation,
                media_type: pixels::source_media_type(&original)
                    .map_err(invalid)?
                    .into(),
                original_size: original.len(),
                original_sha256: hash(&original),
                // What the entry records before this publish: a waiting turn that
                // never committed leaves the entry where it was.
                previous_quarter_turns: match pending {
                    Some(p) if p.entry_committed_version.is_none() => p.previous_quarter_turns,
                    _ => turned.previous_quarter_turns,
                },
                quarter_turns: turned.quarter_turns,
                expected_version,
                updated_data: turned.updated_data,
                entry_committed_version: None,
                thumbnail_expected_generation: previous_thumbnail.map(|v| v.generation),
                thumbnail_size: thumbnail.bytes.len(),
                thumbnail_sha256: hash(&thumbnail.bytes),
                width,
                height,
                thumbnail_width: thumbnail.width,
                thumbnail_height: thumbnail.height,
            };
            Ok(Receipt::Prepared(Box::new(Prepared {
                manifest,
                original,
                thumbnail: thumbnail.bytes,
            })))
        }
        Command::Publish {
            manifest,
            thumbnail,
        } => {
            validate_manifest(project, authority.uid, manifest, thumbnail)?;
            if manifest.credential_audience_sha256 != profile.credential_audience_sha256() {
                return Err(invalid(
                    "Rotation bundle belongs to another deployment audience",
                ));
            }
            // Publication is explicit. Verify the source and entry once before
            // any mutation; a retained receipt is a hint, never cloud authority.
            let source = inspect(profile, transport, authority, &manifest.object_path, false)?
                .ok_or_else(|| unreadable("Source photo is missing"))?;
            if source.generation != manifest.generation || source.bucket != manifest.bucket {
                return Err(invalid(
                    "The original generation changed before publication",
                ));
            }
            let feature = entry_of(
                profile,
                transport,
                authority,
                project,
                &manifest.form_id,
                &manifest.entry_id,
            )?;
            let current_version = client_version(&feature)?;
            let same_rotation = feature["properties"]["data"][survey_media_rotation::KEY]
                == manifest.updated_data[survey_media_rotation::KEY];
            // 1. The entry metadata, under the version the turn was read at.
            let committed = match manifest.entry_committed_version {
                Some(version) if version == current_version && same_rotation => {
                    json!({"state": "committed", "client_version": version, "resumed": true})
                }
                Some(_) => {
                    return Err(invalid(
                        "The committed rotation receipt no longer matches the entry",
                    ));
                }
                None if manifest.expected_version.checked_add(1) == Some(current_version)
                    && same_rotation =>
                {
                    json!({"state": "committed", "client_version": current_version, "resumed": true})
                }
                None if manifest.expected_version != current_version => {
                    return Err(invalid("The entry changed since the turn was prepared"));
                }
                None => {
                    let now = time::OffsetDateTime::now_utc()
                        .format(&time::format_description::well_known::Rfc3339)
                        .map_err(|_| invalid("clock"))?;
                    let request = crate::survey_update::SurveyUpdateRequest::media_rotation(
                        project,
                        &manifest.form_id,
                        &manifest.entry_id,
                        &publication_key(manifest)?,
                        &now,
                        manifest.expected_version,
                        &manifest.updated_data,
                    )?;
                    let receipt = crate::survey_update::run(
                        profile, transport, authority, project, &request,
                    )?;
                    if receipt["complete"] != true {
                        let code = receipt["failure"]["code"]
                            .as_str()
                            .unwrap_or("service_refused");
                        return Err(ClientError::new(
                            if code == "conflict" {
                                ErrorKind::InvalidInput
                            } else {
                                ErrorKind::Transient
                            },
                            if code == "conflict" {
                                "The entry changed since the turn was prepared; prepare it again"
                            } else {
                                "The entry rotation was not committed; publish again"
                            },
                        ));
                    }
                    receipt["receipt"].clone()
                }
            };
            // 2. The regenerated thumbnail, sourced to the original it shows.
            let a = survey_media_address::parse(&manifest.object_path).map_err(invalid)?;
            let source = json!({"source_file_name": a.file, "source_generation": manifest.generation,
                "source_quarter_turns": manifest.quarter_turns});
            let thumbnail_result = upload(
                profile,
                transport,
                authority,
                &manifest.thumbnail_path,
                &manifest.bucket,
                thumbnail,
                "image/jpeg",
                manifest.thumbnail_expected_generation.as_deref(),
                Some(source),
            );
            let mut receipt = json!({"project_id":project,"object_path":manifest.object_path,
                "thumbnail_path":manifest.thumbnail_path,"generation":manifest.generation,
                "original_sha256":manifest.original_sha256,"original_rewritten":false,
                "previous_quarter_turns":manifest.previous_quarter_turns,
                "quarter_turns":manifest.quarter_turns,"degrees":u16::from(manifest.quarter_turns) * 90,
                "entry":committed,"thumbnail_sha256":manifest.thumbnail_sha256});
            match thumbnail_result {
                Ok(generation) => {
                    receipt["publication"] = json!("published");
                    receipt["thumbnail_generation"] = json!(generation);
                }
                Err(error) => {
                    receipt["publication"] = json!("entry_committed");
                    receipt["thumbnail_failure"] = json!(error.to_string());
                }
            }
            Ok(Receipt::Published(receipt))
        }
    }
}

fn publication_key(manifest: &Manifest) -> Result<String, ClientError> {
    let input = serde_json::to_vec(&json!({
        "path": manifest.object_path, "generation": manifest.generation,
        "expected_version": manifest.expected_version, "updated_data": manifest.updated_data,
    }))
    .map_err(|_| invalid("Rotation identity cannot be encoded"))?;
    Ok(format!("rotation-{}", hash(&input)))
}

pub fn validate_manifest(
    project: &str,
    account: &str,
    m: &Manifest,
    thumbnail: &[u8],
) -> Result<(), ClientError> {
    let a = address(project, &m.object_path)?;
    let rotation_only = m.updated_data.as_object().is_some_and(|patch| {
        patch.len() == 1
            && patch
                .get(survey_media_rotation::KEY)
                .is_some_and(Value::is_object)
    });
    let expected_turns = m.updated_data[survey_media_rotation::KEY]
        .get(&m.object_path)
        .map_or(Some(0), |record| {
            (record["generation"] == json!(m.generation))
                .then(|| record["quarter_turns"].as_u64())
                .flatten()
        });
    if m.schema != SCHEMA
        || m.project_id != project
        || m.account_id != account
        || m.form_id != a.form
        || m.entry_id != a.entry
        || m.quarter_turns > 3
        || m.previous_quarter_turns > 3
        || !rotation_only
        || !matches!(m.media_type.as_str(), "image/jpeg" | "image/png")
        || expected_turns != Some(u64::from(m.quarter_turns))
        || m.thumbnail_path != a.thumbnail_object_path
        || !survey_media_rotation::valid_generation(&m.generation)
        || m.thumbnail_sha256.len() != 64
        || thumbnail.is_empty()
        || thumbnail.len() > pixels::MAX_INPUT_BYTES
        || thumbnail.len() != m.thumbnail_size
        || hash(thumbnail) != m.thumbnail_sha256
    {
        return Err(invalid(
            "Rotation bundle bytes, identity, or schema changed",
        ));
    }
    if let Some(generation) = &m.thumbnail_expected_generation {
        wire::generation(&json!(generation)).map_err(invalid)?;
    }
    Ok(())
}

/// Add a turn to retained metadata using the same unchanged camera original.
/// This path performs no transport or clock read, including after app restart.
pub fn turn_pending(
    project: &str,
    account: &str,
    audience: &str,
    prior: &Prepared,
    degrees: i64,
    expected_generation: Option<&str>,
) -> Result<Prepared, ClientError> {
    let m = &prior.manifest;
    validate_manifest(project, account, m, &prior.thumbnail)?;
    if m.credential_audience_sha256 != audience
        || expected_generation.is_some_and(|generation| generation != m.generation)
        || prior.original.is_empty()
        || prior.original.len() > pixels::MAX_INPUT_BYTES
        || prior.original.len() != m.original_size
        || hash(&prior.original) != m.original_sha256
    {
        return Err(invalid(
            "The retained original, generation, or owner changed",
        ));
    }
    let data = m
        .updated_data
        .as_object()
        .ok_or_else(|| invalid("Rotation metadata is not an object"))?;
    let turned = survey_media_rotation::turn(data, &m.object_path, &m.generation, degrees)
        .map_err(invalid)?;
    let rotation = pixels::Rotation::from_quarter_turns(turned.quarter_turns).map_err(invalid)?;
    let thumbnail = pixels::thumbnail(&prior.original, rotation).map_err(invalid)?;
    let (width, height) = pixels::display_dimensions(&prior.original, rotation).map_err(invalid)?;
    let mut manifest = m.clone();
    manifest.previous_quarter_turns = if m.entry_committed_version.is_some() {
        m.quarter_turns
    } else {
        m.previous_quarter_turns
    };
    manifest.quarter_turns = turned.quarter_turns;
    manifest.updated_data = turned.updated_data;
    manifest.expected_version = m.entry_committed_version.unwrap_or(m.expected_version);
    manifest.entry_committed_version = None;
    manifest.width = width;
    manifest.height = height;
    manifest.thumbnail_size = thumbnail.bytes.len();
    manifest.thumbnail_sha256 = hash(&thumbnail.bytes);
    manifest.thumbnail_width = thumbnail.width;
    manifest.thumbnail_height = thumbnail.height;
    Ok(Prepared {
        manifest,
        original: prior.original.clone(),
        thumbnail: thumbnail.bytes,
    })
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

/// A local rendered copy turned by `degrees`, plus its thumbnail: for local
/// files only (no authority, no upload). The input file is never changed.
pub fn rotate_local(bytes: &[u8], degrees: u16) -> Result<LocalRotation, ClientError> {
    if !matches!(degrees, 90 | 180 | 270) {
        return Err(invalid("Choose 90, 180, or 270 clockwise degrees"));
    }
    let rotation = pixels::Rotation::try_from(degrees).map_err(invalid)?;
    Ok(LocalRotation {
        image: pixels::render(bytes, rotation, pixels::MAX_RENDER_EDGE).map_err(invalid)?,
        thumbnail: pixels::thumbnail(bytes, rotation).map_err(invalid)?,
    })
}

/// A local rendered view and its thumbnail.
pub struct LocalRotation {
    pub image: pixels::Encoded,
    pub thumbnail: pixels::Encoded,
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

#[cfg(test)]
#[path = "survey_photo_tests.rs"]
mod tests;
