//! Explicit consumers of a native preview read one exact disk SVG or PDF. This owner
//! has no transport or credential refresh; hosts admit their captured context.
use crate::{HostFailure, PreviewPage, CODE_INPUTS_INVALID, CODE_RESULT_INVALID};
use ds_project_data::preview_room::Room;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const SCHEMA: &str = "ds.print-preview-artifact/v1";
pub const RETENTION_MS: u64 = 24 * 60 * 60 * 1000;
pub const LONG_RETENTION_MS: u64 = 7 * RETENTION_MS;
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

pub use ds_command_kernel::asset_preview::Retention;
/// Closed native preview media; arbitrary MIME types never admit a file.
#[derive(Clone, Copy)]
enum Media {
    Svg,
    Pdf,
    Docx,
    Xlsx,
    Markdown,
}
impl Media {
    fn admit(raw: &str) -> Result<Self, HostFailure> {
        match raw {
            "image/svg+xml" => Ok(Self::Svg),
            "application/pdf" => Ok(Self::Pdf),
            ds_command_kernel::assets::semantic::DOCX_TYPE => Ok(Self::Docx),
            ds_command_kernel::assets::semantic::XLSX_TYPE => Ok(Self::Xlsx),
            ds_command_kernel::assets::semantic::MARKDOWN_TYPE => Ok(Self::Markdown),
            _ => Err(failed(
                "preview media is not an admitted SVG, PDF, DOCX, XLSX or Markdown document",
            )),
        }
    }
    fn filename(self) -> &'static str {
        match self {
            Self::Svg => "page.svg",
            Self::Pdf => "page.pdf",
            Self::Docx => "page.docx",
            Self::Xlsx => "page.xlsx",
            Self::Markdown => "page.md",
        }
    }
    fn mime(self) -> &'static str {
        match self {
            Self::Svg => "image/svg+xml",
            Self::Pdf => "application/pdf",
            Self::Docx => ds_command_kernel::assets::semantic::DOCX_TYPE,
            Self::Xlsx => ds_command_kernel::assets::semantic::XLSX_TYPE,
            Self::Markdown => ds_command_kernel::assets::semantic::MARKDOWN_TYPE,
        }
    }
    fn limit(self) -> u64 {
        match self {
            Self::Svg => crate::MAX_PREVIEW_BYTES,
            Self::Pdf => 128 * 1024 * 1024,
            Self::Docx | Self::Xlsx => ds_command_kernel::assets::MAX_DOCUMENT_BYTES,
            Self::Markdown => ds_command_kernel::assets::semantic::MAX_OUTPUT_BYTES,
        }
    }
    fn signature(self, bytes: &[u8]) -> bool {
        match self {
            Self::Svg => std::str::from_utf8(bytes).is_ok_and(|s| {
                s.trim_start().starts_with("<svg") || s.trim_start().starts_with("<?xml")
            }),
            Self::Pdf => bytes.starts_with(b"%PDF-"),
            // Content format is verified by the existing bounded OOXML decoder before retention.
            Self::Docx | Self::Xlsx => bytes.starts_with(b"PK\x03\x04"),
            Self::Markdown => !bytes.is_empty(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub schema: String,
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub content_type: String,
    pub authority_sha256: String,
    pub result_sha256: String,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub retention_ms: u64,
}

/// A shared disk input index. Result bytes exist only in the common managed
/// file room; this index joins native producers before they render.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestManifest {
    schema: String,
    authority_sha256: String,
    input_sha256: String,
    inputs: Value,
    artifact: Artifact,
    metadata_sha256: String,
    metadata: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RequestReceipt {
    pub input_sha256: String,
    pub reused: bool,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub retention_ms: u64,
}
pub struct RequestLease {
    _lock: fs::File,
    pub(crate) path: PathBuf,
    room: Room,
    inputs: Value,
    key: String,
    retention: Retention,
    retained: Option<RequestManifest>,
}
pub fn begin_request(
    room: &Room,
    inputs: Value,
    retention: Retention,
) -> Result<RequestLease, HostFailure> {
    begin_request_at(room, inputs, retention, now()?)
}
pub(crate) fn begin_request_at(
    room: &Room,
    inputs: Value,
    retention: Retention,
    clock: u64,
) -> Result<RequestLease, HostFailure> {
    begin_request_with_lock(room, inputs, retention, Some(clock), |path| {
        ds_layer_store::exclusive_lock(path).map_err(failed)
    })
}

/// A cancelled waiting consumer leaves the shared producer and its exact file
/// untouched. Hosts supply their captured account/project/revocation check.
pub fn begin_request_cancellable(
    room: &Room,
    inputs: Value,
    retention: Retention,
    check_current: impl Fn() -> Result<(), HostFailure>,
) -> Result<RequestLease, HostFailure> {
    begin_request_with_lock(room, inputs, retention, None, |path| {
        if let Some(parent) = path.parent() {
            ds_layer_store::private::create_dir_all(parent).map_err(|e| failed(e.to_string()))?;
        }
        let lock = ds_layer_store::private::open_options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| failed(e.to_string()))?;
        loop {
            check_current()?;
            match lock.try_lock() {
                Ok(()) => return Ok(lock),
                Err(fs::TryLockError::WouldBlock) => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                Err(fs::TryLockError::Error(error)) => return Err(failed(error.to_string())),
            }
        }
    })
}
fn begin_request_with_lock(
    room: &Room,
    inputs: Value,
    retention: Retention,
    clock: Option<u64>,
    lock: impl FnOnce(&Path) -> Result<fs::File, HostFailure>,
) -> Result<RequestLease, HostFailure> {
    if !inputs.is_object() {
        return Err(failed("preview input index requires native semantic pins"));
    }
    let owner = authority(room)?;
    let key = digest(
        &serde_json::to_vec(&json!({"inputs":inputs,"retention_ms":retention.millis()}))
            .map_err(|e| failed(e.to_string()))?,
    );
    let base = root(room, &owner).join("requests");
    for ancestor in base.ancestors().take_while(|p| p.starts_with(&room.root)) {
        match fs::symlink_metadata(ancestor) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            Ok(_) => return Err(failed("preview input index root is crossed")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(failed(e.to_string())),
        }
    }
    let lock = lock(&base.join(format!("{key}.lock")))?;
    let clock = clock.map(Ok).unwrap_or_else(now)?;
    let path = base.join(format!("{key}.json"));
    let mut lease = RequestLease {
        _lock: lock,
        path,
        room: room.clone(),
        inputs,
        key,
        retention,
        retained: None,
    };
    if let Some(bytes) = crate::read_bounded(&lease.path, MAX_MANIFEST_BYTES)? {
        let old: RequestManifest =
            serde_json::from_slice(&bytes).map_err(|e| failed(e.to_string()))?;
        if old.schema != SCHEMA
            || old.authority_sha256 != owner
            || old.input_sha256 != lease.key
            || old.inputs != lease.inputs
            || old.artifact.retention_ms != retention.millis()
            || digest(&serde_json::to_vec(&old.metadata).map_err(|e| failed(e.to_string()))?)
                != old.metadata_sha256
        {
            return Err(failed(
                "preview input index differs from its exact native inputs or result",
            ));
        }
        if clock < old.artifact.expires_at_unix_ms {
            let manifest = read_manifest(&root(room, &owner).join(&old.artifact.result_sha256))?;
            admit_manifest_identity(room, &manifest)?;
            if manifest.artifact != old.artifact {
                return Err(failed(
                    "preview request differs from its exact native file manifest",
                ));
            }
            // An observed denial is never a retained answer. A native producer
            // must acquire and verify complete new bytes before re-admission.
            if !manifest.revoked {
                admit_receipt_at(room, &old.artifact, clock)?;
                lease.retained = Some(old);
            }
        }
    }
    Ok(lease)
}
impl RequestLease {
    pub fn retained(&self) -> Option<&Value> {
        self.retained.as_ref().map(|record| &record.metadata)
    }
    pub fn receipt(&self, reused: bool) -> Result<RequestReceipt, HostFailure> {
        let record = self
            .retained
            .as_ref()
            .ok_or_else(|| failed("preview has no committed file"))?;
        Ok(RequestReceipt {
            input_sha256: self.key.clone(),
            reused,
            created_at_unix_ms: record.artifact.created_at_unix_ms,
            expires_at_unix_ms: record.artifact.expires_at_unix_ms,
            retention_ms: record.artifact.retention_ms,
        })
    }
    /// Only an admitted common file can complete the input index. Metadata
    /// never contains the rendered PDF/SVG bytes and never grants a new path.
    pub fn commit(&mut self, metadata: Value, artifact: &Artifact) -> Result<(), HostFailure> {
        self.commit_at(metadata, artifact, now()?)
    }
    pub(crate) fn commit_at(
        &mut self,
        metadata: Value,
        artifact: &Artifact,
        clock: u64,
    ) -> Result<(), HostFailure> {
        if self.retained.is_some()
            || !metadata.is_object()
            || artifact.retention_ms != self.retention.millis()
        {
            return Err(failed(
                "preview input index requires one newly admitted file result",
            ));
        }
        admit_receipt_at(&self.room, artifact, clock)?;
        let record = RequestManifest {
            schema: SCHEMA.into(),
            authority_sha256: authority(&self.room)?,
            input_sha256: self.key.clone(),
            inputs: self.inputs.clone(),
            artifact: artifact.clone(),
            metadata_sha256: digest(
                &serde_json::to_vec(&metadata).map_err(|e| failed(e.to_string()))?,
            ),
            metadata,
        };
        let bytes = serde_json::to_vec(&record).map_err(|e| failed(e.to_string()))?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(failed("preview index metadata exceeds its bound"));
        }
        ds_layer_store::private::write_atomic(&self.path, bytes)
            .map_err(|e| failed(e.to_string()))?;
        self.retained = Some(record);
        Ok(())
    }
}

/// A real source denial invalidates only this asset's retained previews and semantic derivatives.
/// Files from other consumers and unknown contents are never deleted. Online
/// authority failure without a typed denial must not call this operation.
pub(crate) fn forget_asset_requests(room: &Room, asset_id: &str) -> Result<usize, HostFailure> {
    let owner = authority(room)?;
    let base = root(room, &owner).join("requests");
    for ancestor in base.ancestors().take_while(|p| p.starts_with(&room.root)) {
        match fs::symlink_metadata(ancestor) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            Ok(_) => return Err(failed("source denial preview root is crossed")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(failed(e.to_string())),
        }
    }
    let entries = match fs::read_dir(&base) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(failed(e.to_string())),
    };
    let mut removed = 0;
    let mut cleanup_error = None;
    let mut inspected = 0;
    for entry in entries {
        let entry = entry.map_err(|e| failed(e.to_string()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(key) = name.strip_suffix(".json").filter(|v| digest_token(v)) else {
            continue;
        };
        inspected += 1;
        if inspected > 4096 {
            return Err(failed(
                "source denial preview cleanup exceeds its bounded request index",
            ));
        }
        let path = entry.path();
        let Some(bytes) = crate::read_bounded(&path, MAX_MANIFEST_BYTES)? else {
            continue;
        };
        let candidate: RequestManifest =
            serde_json::from_slice(&bytes).map_err(|e| failed(e.to_string()))?;
        if !matches!(
            candidate.inputs["schema"].as_str(),
            Some("ds.asset-preview.acquire/v1" | ds_command_kernel::assets::semantic::SCHEMA)
        ) || candidate.inputs["asset_id"] != asset_id
        {
            continue;
        }
        let _lock =
            ds_layer_store::exclusive_lock(&base.join(format!("{key}.lock"))).map_err(failed)?;
        let Some(bytes) = crate::read_bounded(&path, MAX_MANIFEST_BYTES)? else {
            continue;
        };
        let record: RequestManifest =
            serde_json::from_slice(&bytes).map_err(|e| failed(e.to_string()))?;
        if record.schema != SCHEMA
            || record.authority_sha256 != owner
            || record.input_sha256 != key
            || !matches!(
                record.inputs["schema"].as_str(),
                Some("ds.asset-preview.acquire/v1" | ds_command_kernel::assets::semantic::SCHEMA)
            )
            || record.inputs["asset_id"] != asset_id
            || digest(
                &serde_json::to_vec(
                    &json!({"inputs":record.inputs,"retention_ms":record.artifact.retention_ms}),
                )
                .map_err(|e| failed(e.to_string()))?,
            ) != key
            || digest(&serde_json::to_vec(&record.metadata).map_err(|e| failed(e.to_string()))?)
                != record.metadata_sha256
        {
            return Err(failed(
                "source denial index differs from its exact native authority",
            ));
        }
        let result = root(room, &owner).join(&record.artifact.result_sha256);
        let _result_lock = ds_layer_store::exclusive_lock(
            &root(room, &owner).join(format!("{}.lock", record.artifact.result_sha256)),
        )
        .map_err(failed)?;
        match fs::symlink_metadata(&result) {
            Ok(_) => {
                let mut manifest = read_manifest(&result)?;
                let page = admit_manifest_identity(room, &manifest)?;
                if manifest.artifact != record.artifact {
                    return Err(failed("source denial artifact differs from its request"));
                }
                // Refuse every old locator FIRST. Foreign contents may block
                // deletion, but cannot turn a denied source into a readable PDF.
                manifest.revoked = true;
                ds_layer_store::private::write_atomic(
                    &result.join("manifest.json"),
                    serde_json::to_vec(&manifest).map_err(|e| failed(e.to_string()))?,
                )
                .map_err(|e| failed(e.to_string()))?;
                if let Err(error) = remove_owned(&result, &manifest, &page) {
                    // Continue revoking exact source-bound locators when cleanup
                    // is blocked; keep this revoked request for later cleanup.
                    cleanup_error.get_or_insert(error);
                    continue;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(failed(e.to_string())),
        }
        if let Err(error) = fs::remove_file(&path) {
            cleanup_error.get_or_insert_with(|| failed(error.to_string()));
            continue;
        }
        removed += 1;
    }
    match cleanup_error {
        Some(error) => Err(error),
        None => Ok(removed),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    artifact: Artifact,
    metadata: Value,
    #[serde(default, skip_serializing_if = "not_revoked")]
    revoked: bool,
}
fn not_revoked(value: &bool) -> bool {
    !*value
}

fn failed(message: impl Into<String>) -> HostFailure {
    HostFailure::new("print_preview_artifact_unavailable", message)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn digest_token(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn now() -> Result<u64, HostFailure> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| failed(e.to_string()))?
        .as_millis()
        .try_into()
        .map_err(|_| failed("preview clock exceeds its bound"))
}
pub(crate) fn authority(room: &Room) -> Result<String, HostFailure> {
    if !room.root.is_absolute()
        || room.scope.principal.trim().is_empty()
        || room.scope.project.trim().is_empty()
        || room.lane.trim().is_empty()
        || !digest_token(&room.credential_audience_sha256)
    {
        return Err(HostFailure::new(
            CODE_INPUTS_INVALID,
            "preview artifact requires exact native account/project/lane/audience",
        ));
    }
    Ok(digest(
        &serde_json::to_vec(&json!([
            SCHEMA,
            room.scope.principal,
            room.scope.project,
            room.lane,
            room.credential_audience_sha256
        ]))
        .map_err(|e| failed(e.to_string()))?,
    ))
}
fn root(room: &Room, authority: &str) -> PathBuf {
    ds_project_data::room_hold::room_dir(&room.root, &room.scope)
        .join("preview-files")
        .join(authority)
}
fn metadata(page: &PreviewPage, retention: Retention) -> Result<Value, HostFailure> {
    // Serialize metadata directly; never serialize the SVG to discard it.
    Ok(
        json!({"transformer":page.transformer,"server_version":page.server_version,
        "sha256":page.sha256,"size_bytes":page.size_bytes,"content_type":page.content_type,
        "output_id":page.output_id,"layout_id":page.layout_id,"layout_sha256":page.layout_sha256,
        "format":page.format,"warnings":page.warnings,"print_diagnostics":page.print_diagnostics,
        "elapsed_ms":page.elapsed_ms,"engine":page.engine,"context_layers":page.context_layers,
        "context_omitted":page.context_omitted,"survey_forms_omitted":page.survey_forms_omitted,
        "context_sha256":page.context_sha256,"filename":page.filename,"receipt":page.receipt,
        "retention_ms":retention.millis()}),
    )
}
fn admitted_page(page: &PreviewPage) -> Result<(), HostFailure> {
    if page.size_bytes == 0
        || page.size_bytes > crate::MAX_PREVIEW_BYTES
        || page.size_bytes != page.page.len() as u64
        || !digest_token(&page.sha256)
        || digest(page.page.as_bytes()) != page.sha256
        || page.content_type != "image/svg+xml"
        || !page.page.trim_start().starts_with("<svg")
    {
        return Err(HostFailure::new(
            CODE_RESULT_INVALID,
            "preview SVG differs from its native size, digest or content type",
        ));
    }
    Ok(())
}
fn read_manifest(directory: &Path) -> Result<Manifest, HostFailure> {
    let metadata = fs::symlink_metadata(directory).map_err(|e| failed(e.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(failed("preview result directory must be a real directory"));
    }
    let path = directory.join("manifest.json");
    let bytes = crate::read_bounded(&path, MAX_MANIFEST_BYTES)?
        .ok_or_else(|| failed("preview artifact manifest is missing"))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| failed(format!("preview artifact manifest is unreadable: {e}")))
}
fn admit_manifest(room: &Room, manifest: &Manifest) -> Result<PathBuf, HostFailure> {
    if manifest.revoked {
        return Err(failed("preview was revoked by its source authority"));
    }
    admit_manifest_identity(room, manifest)
}
fn admit_manifest_identity(room: &Room, manifest: &Manifest) -> Result<PathBuf, HostFailure> {
    let artifact = &manifest.artifact;
    let media = Media::admit(&artifact.content_type)?;
    let owner = authority(room)?;
    if artifact.schema != SCHEMA
        || artifact.authority_sha256 != owner
        || !digest_token(&artifact.result_sha256)
        || !digest_token(&artifact.sha256)
        || artifact.size_bytes == 0
        || artifact.size_bytes > media.limit()
        || ![RETENTION_MS, LONG_RETENTION_MS].contains(&artifact.retention_ms)
        || artifact
            .created_at_unix_ms
            .checked_add(artifact.retention_ms)
            != Some(artifact.expires_at_unix_ms)
        || digest(&serde_json::to_vec(&manifest.metadata).map_err(|e| failed(e.to_string()))?)
            != artifact.result_sha256
        || manifest.metadata["sha256"] != artifact.sha256
        || manifest.metadata["retention_ms"] != artifact.retention_ms
        || manifest.metadata["size_bytes"] != artifact.size_bytes
        || manifest.metadata["content_type"] != artifact.content_type
    {
        return Err(failed(
            "preview artifact differs from its captured native authority or result",
        ));
    }
    let path = root(room, &owner)
        .join(&artifact.result_sha256)
        .join(media.filename());
    if path.to_string_lossy() != artifact.path {
        return Err(failed(
            "preview artifact path differs from its managed result",
        ));
    }
    Ok(path)
}

/// Materialize a proven result once. Reuse never extends its original expiry.
pub fn materialize(room: &Room, page: &PreviewPage) -> Result<Artifact, HostFailure> {
    materialize_retained(room, page, Retention::default())
}
pub fn materialize_retained(
    room: &Room,
    page: &PreviewPage,
    retention: Retention,
) -> Result<Artifact, HostFailure> {
    materialize_with_retention_at(room, page, retention, now()?)
}
#[cfg(test)]
fn materialize_at(room: &Room, page: &PreviewPage, clock: u64) -> Result<Artifact, HostFailure> {
    materialize_with_retention_at(room, page, Retention::default(), clock)
}
fn materialize_with_retention_at(
    room: &Room,
    page: &PreviewPage,
    retention: Retention,
    clock: u64,
) -> Result<Artifact, HostFailure> {
    admitted_page(page)?;
    materialize_svg_at(
        room,
        &page.page,
        metadata(page, retention)?,
        retention,
        clock,
    )
}

/// Persist another native SVG subject through the same artifact authority.
/// Provenance is authored by the admitted native renderer host, never a path
/// or artifact receipt supplied by a consumer.
pub fn materialize_svg_retained(
    room: &Room,
    svg: &str,
    provenance: Value,
    retention: Retention,
) -> Result<Artifact, HostFailure> {
    let metadata = json!({"provenance":provenance,"sha256":digest(svg.as_bytes()),
        "size_bytes":svg.len(),"content_type":"image/svg+xml","retention_ms":retention.millis()});
    materialize_svg_at(room, svg, metadata, retention, now()?)
}

fn materialize_svg_at(
    room: &Room,
    svg: &str,
    metadata: Value,
    retention: Retention,
    clock: u64,
) -> Result<Artifact, HostFailure> {
    let size_bytes = svg.len() as u64;
    if size_bytes == 0
        || size_bytes > crate::MAX_PREVIEW_BYTES
        || !(svg.trim_start().starts_with("<svg") || svg.trim_start().starts_with("<?xml"))
    {
        return Err(HostFailure::new(
            CODE_RESULT_INVALID,
            "native preview is empty, oversized or not an SVG document",
        ));
    }
    let sha256 = digest(svg.as_bytes());
    materialize_file_at(
        room,
        size_bytes,
        sha256,
        metadata,
        Media::Svg,
        retention,
        clock,
        |path| crate::write_new_private(path, svg.as_bytes()),
    )
}

/// A renderer host supplies its already confined, verified PDF handle. The
/// common preview owner streams those same bytes to the same managed room;
/// this never invokes a renderer or accepts a consumer-supplied source path.
pub fn materialize_pdf_retained(
    room: &Room,
    source: &mut fs::File,
    size_bytes: u64,
    sha256: &str,
    provenance: Value,
    retention: Retention,
) -> Result<Artifact, HostFailure> {
    materialize_document_retained(
        room,
        source,
        size_bytes,
        sha256,
        "application/pdf",
        provenance,
        retention,
    )
}
/// Retain the exact source bytes after the existing document decoder verified its format.
/// This does not render, extract Markdown, or infer a MIME from a filename.
pub fn materialize_document_retained(
    room: &Room,
    source: &mut fs::File,
    size_bytes: u64,
    sha256: &str,
    content_type: &str,
    provenance: Value,
    retention: Retention,
) -> Result<Artifact, HostFailure> {
    if ds_command_kernel::assets::semantic::source_format(content_type).is_none() {
        return Err(failed("native source document media is not admitted"));
    }
    let media = Media::admit(content_type)?;
    if size_bytes == 0
        || size_bytes > media.limit()
        || !digest_token(sha256)
        || source.metadata().map_err(|e| failed(e.to_string()))?.len() != size_bytes
    {
        return Err(failed(
            "Document preview differs from its verified native byte pins",
        ));
    }
    source.rewind().map_err(|e| failed(e.to_string()))?;
    let mut prefix = [0_u8; 5];
    source
        .read_exact(&mut prefix)
        .map_err(|e| failed(e.to_string()))?;
    if !media.signature(&prefix) {
        return Err(failed(
            "native preview bytes do not match the admitted document media",
        ));
    }
    source.rewind().map_err(|e| failed(e.to_string()))?;
    let metadata = json!({"provenance":provenance,"sha256":sha256,"size_bytes":size_bytes,
        "content_type":media.mime(),"retention_ms":retention.millis()});
    materialize_file_at(
        room,
        size_bytes,
        sha256.into(),
        metadata,
        media,
        retention,
        now()?,
        |path| {
            let mut target = ds_layer_store::private::open_options()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|e| failed(e.to_string()))?;
            std::io::copy(&mut source.take(size_bytes + 1), &mut target)
                .map_err(|e| failed(e.to_string()))?;
            target.flush().map_err(|e| failed(e.to_string()))?;
            target.sync_all().map_err(|e| failed(e.to_string()))
        },
    )
}
/// Retain externally produced Markdown in the same native room. This is byte
/// validation/materialization only; it never runs an extractor or reads a URL.
pub fn materialize_markdown_retained(
    room: &Room,
    bytes: &[u8],
    provenance: Value,
    retention: Retention,
) -> Result<Artifact, HostFailure> {
    if bytes.len() as u64 > Media::Markdown.limit()
        || !std::str::from_utf8(bytes).is_ok_and(|text| !text.trim().is_empty())
    {
        return Err(failed(
            "semantic output needs bounded nonempty UTF-8 Markdown",
        ));
    }
    let sha256 = digest(bytes);
    let metadata = json!({"provenance":provenance,"sha256":sha256,
        "size_bytes":bytes.len(),"content_type":Media::Markdown.mime(),"retention_ms":retention.millis()});
    materialize_file_at(
        room,
        bytes.len() as u64,
        sha256,
        metadata,
        Media::Markdown,
        retention,
        now()?,
        |path| crate::write_new_private(path, bytes),
    )
}
fn materialize_file_at(
    room: &Room,
    size_bytes: u64,
    sha256: String,
    metadata: Value,
    media: Media,
    retention: Retention,
    clock: u64,
    write: impl FnOnce(&Path) -> Result<(), HostFailure>,
) -> Result<Artifact, HostFailure> {
    let owner = authority(room)?;
    let result = digest(&serde_json::to_vec(&metadata).map_err(|e| failed(e.to_string()))?);
    let base = root(room, &owner);
    ds_project_data::private::create_dir_all(&base).map_err(|e| failed(e.to_string()))?;
    prune_at(room, clock, 16)?;
    let _lock =
        ds_layer_store::exclusive_lock(&base.join(format!("{result}.lock"))).map_err(failed)?;
    let target = base.join(&result);
    if fs::symlink_metadata(&target).is_ok() {
        let manifest = read_manifest(&target)?;
        let path = admit_manifest_identity(room, &manifest)?;
        if manifest.metadata != metadata {
            return Err(failed(
                "preview result identity differs from its retained manifest",
            ));
        }
        if manifest.revoked {
            // This materializer received a completed native producer's bytes,
            // not a cached read. Give the verified new result its own identity;
            // the denied result and foreign files remain untouched/unreadable.
            let mut metadata = metadata;
            metadata["native_re_admission"] = json!({"operation":crate::random_run_id()});
            drop(_lock);
            return materialize_file_at(
                room, size_bytes, sha256, metadata, media, retention, clock, write,
            );
        }
        crate::verify_file(&path, size_bytes, &sha256)?;
        if clock < manifest.artifact.expires_at_unix_ms {
            return Ok(manifest.artifact);
        }
        // Only this admitted key's two owned files expire. Unknown files and
        // other result directories are never traversed or removed.
        remove_owned(&target, &manifest, &path)?;
    }
    let artifact = Artifact {
        schema: SCHEMA.into(),
        path: target.join(media.filename()).to_string_lossy().into_owned(),
        sha256: sha256.clone(),
        size_bytes,
        content_type: media.mime().into(),
        authority_sha256: owner,
        result_sha256: result,
        created_at_unix_ms: clock,
        expires_at_unix_ms: clock
            .checked_add(retention.millis())
            .ok_or_else(|| failed("preview expiry exceeds its bound"))?,
        retention_ms: retention.millis(),
    };
    let staged = crate::Staging {
        path: base.join(format!(".stage-{}", crate::random_run_id())),
    };
    crate::create_private_dir(&staged.path)?;
    write(&staged.path.join(media.filename()))?;
    crate::verify_file(&staged.path.join(media.filename()), size_bytes, &sha256)?;
    let manifest = serde_json::to_vec(&Manifest {
        artifact: artifact.clone(),
        metadata,
        revoked: false,
    })
    .map_err(|e| failed(e.to_string()))?;
    if manifest.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(failed(
            "preview manifest exceeds its bounded transport metadata",
        ));
    }
    crate::write_new_private(&staged.path.join("manifest.json"), &manifest)?;
    crate::promote_report(&staged.path, &target)?;
    Ok(artifact)
}
#[cfg(test)]
pub(crate) fn materialize_pdf_at(
    room: &Room,
    bytes: &[u8],
    provenance: Value,
    retention: Retention,
    clock: u64,
) -> Result<Artifact, HostFailure> {
    if !Media::Pdf.signature(bytes) || bytes.len() as u64 > Media::Pdf.limit() {
        return Err(failed("test native PDF is invalid"));
    }
    let sha = digest(bytes);
    materialize_file_at(
        room,
        bytes.len() as u64,
        sha.clone(),
        json!({"provenance":provenance,"sha256":sha,"size_bytes":bytes.len(),"content_type":Media::Pdf.mime(),"retention_ms":retention.millis()}),
        Media::Pdf,
        retention,
        clock,
        |path| crate::write_new_private(path, bytes),
    )
}

fn remove_owned(directory: &Path, manifest: &Manifest, page: &Path) -> Result<(), HostFailure> {
    let names = fs::read_dir(directory)
        .map_err(|e| failed(e.to_string()))?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .map_err(|e| failed(e.to_string()))?;
    let expected = [
        "manifest.json",
        Media::admit(&manifest.artifact.content_type)?.filename(),
    ]
    .map(std::ffi::OsString::from)
    .into_iter()
    .collect();
    if names != expected {
        return Err(failed(
            "preview result contains foreign files; expiration left every file intact",
        ));
    }
    crate::verify_file(
        page,
        manifest.artifact.size_bytes,
        &manifest.artifact.sha256,
    )?;
    fs::remove_file(page).map_err(|e| failed(e.to_string()))?;
    fs::remove_file(directory.join("manifest.json")).map_err(|e| failed(e.to_string()))?;
    fs::remove_dir(directory).map_err(|e| failed(e.to_string()))
}
/// Bounded cleanup only at an explicit consuming write or explicit native
/// cleanup. It cannot acquire data, inspect remote freshness or traverse jobs.
pub fn prune_expired(room: &Room, limit: usize) -> Result<usize, HostFailure> {
    prune_at(room, now()?, limit)
}
fn prune_at(room: &Room, clock: u64, limit: usize) -> Result<usize, HostFailure> {
    if limit == 0 || limit > 128 {
        return Err(HostFailure::new(
            CODE_INPUTS_INVALID,
            "preview cleanup limit must be 1..128",
        ));
    }
    let base = root(room, &authority(room)?);
    let entries = match fs::read_dir(&base) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(failed(e.to_string())),
    };
    let mut candidates = 0;
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|e| failed(e.to_string()))?;
        let key = entry.file_name();
        let Some(key) = key.to_str() else { continue };
        if !digest_token(key) {
            continue;
        }
        candidates += 1;
        if candidates > limit {
            break;
        }
        let _lock =
            ds_layer_store::exclusive_lock(&base.join(format!("{key}.lock"))).map_err(failed)?;
        let Ok(manifest) = read_manifest(&entry.path()) else {
            continue;
        };
        let Ok(page) = admit_manifest(room, &manifest) else {
            continue;
        };
        if page.parent() != Some(entry.path().as_path())
            || clock < manifest.artifact.expires_at_unix_ms
        {
            continue;
        }
        if remove_owned(&entry.path(), &manifest, &page).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Read only the exact admitted managed file; no render, acquisition or refresh.
pub fn read(room: &Room, artifact: &Artifact) -> Result<Vec<u8>, HostFailure> {
    read_at(room, artifact, now()?)
}
/// Validate a retained locator without reading an unselected page's bytes.
/// The actual file consumer still calls `read` to verify its exact content.
pub fn admit_receipt(room: &Room, artifact: &Artifact) -> Result<(), HostFailure> {
    admit_receipt_at(room, artifact, now()?)
}
fn admit_receipt_at(room: &Room, artifact: &Artifact, clock: u64) -> Result<(), HostFailure> {
    if !digest_token(&artifact.result_sha256) {
        return Err(failed("preview result locator is invalid"));
    }
    let manifest = read_manifest(&root(room, &authority(room)?).join(&artifact.result_sha256))?;
    admit_manifest(room, &manifest)?;
    if &manifest.artifact != artifact {
        return Err(failed(
            "preview receipt differs from its exact native manifest",
        ));
    }
    if clock >= artifact.expires_at_unix_ms {
        return Err(HostFailure::new(
            "print_preview_artifact_expired",
            "Preview receipt expired; request its exact native preview again.",
        ));
    }
    Ok(())
}
/// Prove an input-pinned producer result without loading its file into RAM.
pub fn verify(room: &Room, artifact: &Artifact, provenance: &Value) -> Result<(), HostFailure> {
    admit_receipt(room, artifact)?;
    let manifest = read_manifest(&root(room, &authority(room)?).join(&artifact.result_sha256))?;
    if manifest.metadata["provenance"] != *provenance {
        return Err(failed("preview file differs from its native semantic pins"));
    }
    let path = admit_manifest(room, &manifest)?;
    crate::verify_file(&path, artifact.size_bytes, &artifact.sha256)?;
    let mut file = fs::File::open(path).map_err(|e| failed(e.to_string()))?;
    let mut prefix = [0_u8; 512];
    let count = file.read(&mut prefix).map_err(|e| failed(e.to_string()))?;
    if !Media::admit(&artifact.content_type)?.signature(&prefix[..count]) {
        return Err(failed("preview file differs from its admitted media"));
    }
    Ok(())
}
fn read_at(room: &Room, artifact: &Artifact, clock: u64) -> Result<Vec<u8>, HostFailure> {
    if !digest_token(&artifact.result_sha256) {
        return Err(failed("preview result locator is invalid"));
    }
    let manifest = read_manifest(&root(room, &authority(room)?).join(&artifact.result_sha256))?;
    let path = admit_manifest(room, &manifest)?;
    if &manifest.artifact != artifact {
        return Err(failed("preview receipt differs from its managed manifest"));
    }
    if clock >= artifact.expires_at_unix_ms {
        return Err(HostFailure::new(
            "print_preview_artifact_expired",
            "Preview expired; request its exact native preview again.",
        ));
    }
    crate::verify_file(&path, artifact.size_bytes, &artifact.sha256)?;
    let bytes = crate::read_bounded(&path, artifact.size_bytes)?
        .ok_or_else(|| failed("preview file is missing"))?;
    if bytes.len() as u64 != artifact.size_bytes || digest(&bytes) != artifact.sha256 {
        return Err(failed("preview file changed while reading"));
    }
    if !Media::admit(&artifact.content_type)?.signature(&bytes) {
        return Err(failed("preview bytes differ from their admitted media"));
    }
    if matches!(Media::admit(&artifact.content_type)?, Media::Markdown)
        && !std::str::from_utf8(&bytes).is_ok_and(|text| !text.trim().is_empty())
    {
        return Err(failed("retained Markdown is not nonempty UTF-8 text"));
    }
    Ok(bytes)
}

/// Validate the explicit output before rendering. The managed result remains
/// authoritative; a copy never replaces or adopts a source or another output.
pub fn admit_output(raw: &str) -> Result<PathBuf, HostFailure> {
    let path = admit_file_output(raw)?;
    if path.extension().and_then(|s| s.to_str()) != Some("svg") {
        return Err(HostFailure::new(
            CODE_INPUTS_INVALID,
            "SVG preview output must be a new absolute .svg path",
        ));
    }
    Ok(path)
}
fn admit_file_output(raw: &str) -> Result<PathBuf, HostFailure> {
    let path = PathBuf::from(raw);
    if raw.is_empty()
        || raw.len() > 4096
        || raw.chars().any(char::is_control)
        || !path.is_absolute()
        || !matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("svg" | "pdf" | "docx" | "xlsx" | "md")
        )
    {
        return Err(HostFailure::new(
            CODE_INPUTS_INVALID,
            "preview output must be a new absolute .svg, .pdf, .docx, .xlsx or .md path",
        ));
    }
    if fs::symlink_metadata(&path).is_ok() {
        return Err(HostFailure::new(
            crate::CODE_OUTPUT_EXISTS,
            "preview output already exists",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| failed("preview output has no parent"))?;
    let metadata =
        fs::symlink_metadata(parent).map_err(|_| failed("preview output parent is unavailable"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(failed("preview output parent must be a real directory"));
    }
    Ok(path)
}
pub fn copy(room: &Room, artifact: &Artifact, raw: &str) -> Result<PathBuf, HostFailure> {
    let destination = admit_file_output(raw)?;
    if destination.extension().and_then(|s| s.to_str())
        != Some(match Media::admit(&artifact.content_type)? {
            Media::Svg => "svg",
            Media::Pdf => "pdf",
            Media::Docx => "docx",
            Media::Xlsx => "xlsx",
            Media::Markdown => "md",
        })
    {
        return Err(failed(
            "preview copy extension differs from its admitted media",
        ));
    }
    let bytes = read(room, artifact)?;
    let temporary = destination
        .parent()
        .unwrap()
        .join(format!(".preview-copy-{}", crate::random_run_id()));
    let staged = crate::Staging {
        path: temporary.clone(),
    };
    crate::create_private_dir(&temporary)?;
    let file = temporary.join(Media::admit(&artifact.content_type)?.filename());
    crate::write_new_private(&file, &bytes)?;
    crate::verify_file(&file, artifact.size_bytes, &artifact.sha256)?;
    // A same-filesystem hard link publishes complete bytes with atomic
    // no-replace semantics on every supported native host.
    fs::hard_link(&file, &destination)
        .map_err(|e| failed(format!("could not publish new preview copy: {e}")))?;
    crate::sync_directory(destination.parent().unwrap())?;
    drop(staged);
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn room(root: PathBuf) -> Room {
        Room {
            root,
            scope: ds_command_kernel::project_dataset_cache::Scope {
                principal: "owner".into(),
                project: "project".into(),
            },
            lane: "local".into(),
            credential_audience_sha256: "a".repeat(64),
        }
    }
    fn page(size: usize) -> PreviewPage {
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\">{}</svg>",
            " ".repeat(size)
        );
        serde_json::from_value(json!({"transformer":"tx_a","server_version":7,"page":svg,
            "sha256":digest(svg.as_bytes()),"size_bytes":svg.len(),"content_type":"image/svg+xml",
            "output_id":"svg__a0","layout_id":"a0","layout_sha256":"b".repeat(64),"format":"svg",
            "warnings":[],"print_diagnostics":[],"elapsed_ms":1,"engine":{"engine_version":"dev","build_manifest_sha256":"c".repeat(64),
            "package_version":"1","source_sha":"d".repeat(40),"profile":"debug"},"context_layers":[],"context_omitted":[],
            "survey_forms_omitted":[],"context_sha256":null,"filename":"tx_a.svg","receipt":{"input":"native-pinned"}})).unwrap()
    }
    #[test]
    fn pdf_and_svg_share_authority_file_reader_and_verified_copy_with_closed_media() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let bytes = b"%PDF-1.7 exact saved analysis";
        let source = dir.path().join("verified-source.pdf");
        fs::write(&source, bytes).unwrap();
        let mut handle = fs::File::open(&source).unwrap();
        let provenance = json!({"saved_analysis_sha256":"b".repeat(64)});
        let pdf = materialize_pdf_retained(
            &room,
            &mut handle,
            bytes.len() as u64,
            &digest(bytes),
            provenance.clone(),
            Retention::Hours24,
        )
        .unwrap();
        assert_eq!(pdf.content_type, "application/pdf");
        assert!(pdf.path.ends_with("/page.pdf"));
        assert!(pdf.path.contains("/preview-files/"));
        assert!(serde_json::to_vec(&pdf).unwrap().len() < 2048);
        assert_eq!(read(&room, &pdf).unwrap(), bytes);
        verify(&room, &pdf, &provenance).unwrap();
        let svg = materialize(&room, &page(10)).unwrap();
        assert_eq!(pdf.authority_sha256, svg.authority_sha256);
        let out = dir.path().join("copy.pdf");
        copy(&room, &pdf, out.to_str().unwrap()).unwrap();
        assert_eq!(fs::read(out).unwrap(), bytes);
        assert!(copy(&room, &pdf, dir.path().join("wrong.svg").to_str().unwrap()).is_err());
        let mut crossed = room.clone();
        crossed.lane = "stable".into();
        assert!(read(&crossed, &pdf).is_err());
        let mut wrong = pdf.clone();
        wrong.content_type = "text/html".into();
        assert!(read(&room, &wrong).is_err());
        assert!(verify(
            &room,
            &pdf,
            &json!({"saved_analysis_sha256":"c".repeat(64)})
        )
        .is_err());
        fs::write(&pdf.path, b"%PDF-1.7 changed saved result").unwrap();
        assert!(read(&room, &pdf).is_err());
    }
    #[test]
    fn pdf_materialization_requires_verified_bytes_and_expiry_never_renders_or_refreshes() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let pdf = materialize_pdf_at(
            &room,
            b"%PDF-1.7 held",
            json!({"analysis":"pinned"}),
            Retention::Days7,
            100,
        )
        .unwrap();
        assert!(read_at(&room, &pdf, 100 + LONG_RETENTION_MS - 1).is_ok());
        assert_eq!(
            read_at(&room, &pdf, 100 + LONG_RETENTION_MS)
                .unwrap_err()
                .code,
            "print_preview_artifact_expired"
        );
        let file = dir.path().join("bad.pdf");
        fs::write(&file, b"<svg>wrong media</svg>").unwrap();
        let mut source = fs::File::open(&file).unwrap();
        assert!(materialize_pdf_retained(
            &room,
            &mut source,
            fs::metadata(&file).unwrap().len(),
            &digest(b"<svg>wrong media</svg>"),
            json!({}),
            Retention::Hours24
        )
        .is_err());
        fs::write(&file, b"%PDF-1.7 changed pins").unwrap();
        let mut source = fs::File::open(&file).unwrap();
        assert!(materialize_pdf_retained(
            &room,
            &mut source,
            fs::metadata(&file).unwrap().len(),
            &"f".repeat(64),
            json!({}),
            Retention::Hours24
        )
        .is_err());
    }
    #[test]
    fn large_svg_is_disk_bytes_with_small_receipt_and_reuse_keeps_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let page = page(9 * 1024 * 1024);
        let artifact = materialize_at(&room, &page, 100).unwrap();
        assert!(artifact.size_bytes > 8 * 1024 * 1024);
        assert!(serde_json::to_vec(&artifact).unwrap().len() < 2048);
        assert_eq!(materialize_at(&room, &page, 500).unwrap(), artifact);
        assert_eq!(
            read_at(&room, &artifact, 500).unwrap(),
            page.page.as_bytes()
        );
        assert_eq!(artifact.expires_at_unix_ms, 100 + RETENTION_MS);
    }
    #[test]
    fn exact_scope_result_digest_path_and_bytes_are_admitted_on_every_read() {
        let dir = tempfile::tempdir().unwrap();
        let base = room(dir.path().into());
        let page = page(10);
        let artifact = materialize_at(&base, &page, 100).unwrap();
        for changed in [
            Room {
                scope: ds_command_kernel::project_dataset_cache::Scope {
                    principal: "other".into(),
                    ..base.scope.clone()
                },
                ..base.clone()
            },
            Room {
                scope: ds_command_kernel::project_dataset_cache::Scope {
                    project: "other".into(),
                    ..base.scope.clone()
                },
                ..base.clone()
            },
            Room {
                lane: "canary".into(),
                ..base.clone()
            },
            Room {
                credential_audience_sha256: "e".repeat(64),
                ..base.clone()
            },
        ] {
            assert!(read_at(&changed, &artifact, 101).is_err());
        }
        let mut changed = artifact.clone();
        changed.path = dir.path().join("foreign.svg").display().to_string();
        assert!(read_at(&base, &changed, 101).is_err());
        let mut changed = artifact.clone();
        changed.sha256 = "f".repeat(64);
        assert!(read_at(&base, &changed, 101).is_err());
        fs::write(&artifact.path, b"foreign").unwrap();
        assert!(read_at(&base, &artifact, 101).is_err());
    }
    #[test]
    fn expiry_republishes_only_exact_owned_result_and_leaves_other_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let page = page(10);
        let artifact = materialize_at(&room, &page, 100).unwrap();
        let mut other = page.clone();
        other.server_version += 1;
        let other = materialize_at(&room, &other, 100).unwrap();
        let foreign = dir.path().join("foreign-job");
        fs::write(&foreign, b"keep").unwrap();
        assert_eq!(
            read_at(&room, &artifact, 100 + RETENTION_MS)
                .unwrap_err()
                .code,
            "print_preview_artifact_expired"
        );
        let replacement = materialize_at(&room, &page, 100 + RETENTION_MS).unwrap();
        assert_eq!(replacement.path, artifact.path);
        assert_ne!(replacement.expires_at_unix_ms, artifact.expires_at_unix_ms);
        assert!(!Path::new(&other.path).exists());
        assert_eq!(fs::read(foreign).unwrap(), b"keep");
    }
    #[test]
    fn invalid_native_result_and_output_paths_cannot_write_or_replace() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let page = page(10);
        let mut bad = page.clone();
        bad.server_version += 1;
        bad.sha256 = "f".repeat(64);
        assert!(materialize_at(&room, &bad, 100).is_err());
        assert!(!root(&room, &authority(&room).unwrap()).exists());
        for path in [
            "relative.svg",
            "/missing-parent/preview.svg",
            dir.path().join("preview.pdf").to_str().unwrap(),
        ] {
            assert!(admit_output(path).is_err());
        }
        let artifact = materialize(&room, &page).unwrap();
        let output = dir.path().join("preview.svg");
        copy(&room, &artifact, output.to_str().unwrap()).unwrap();
        assert!(copy(&room, &artifact, output.to_str().unwrap()).is_err());
        assert_eq!(fs::read(output).unwrap(), page.page.as_bytes());
    }
    #[test]
    fn concurrent_consumers_publish_one_exact_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let page = page(10);
        let answers = std::thread::scope(|threads| {
            let jobs: Vec<_> = (0..4)
                .map(|_| threads.spawn(|| materialize_at(&room, &page, 100).unwrap()))
                .collect();
            jobs.into_iter()
                .map(|job| job.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(answers.iter().all(|value| value == &answers[0]));
        assert_eq!(
            fs::read_dir(Path::new(&answers[0].path).parent().unwrap())
                .unwrap()
                .count(),
            2
        );
    }
    #[test]
    fn seven_day_retention_and_foreign_files_are_preserved_by_bounded_prune() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let page = page(10);
        let short = materialize_at(&room, &page, 100).unwrap();
        let long = materialize_with_retention_at(&room, &page, Retention::Days7, 100).unwrap();
        assert_ne!(short.path, long.path);
        assert_eq!(long.expires_at_unix_ms, 100 + LONG_RETENTION_MS);
        assert_eq!(
            materialize_with_retention_at(&room, &page, Retention::Days7, 500).unwrap(),
            long
        );
        let foreign = Path::new(&short.path).parent().unwrap().join("other-job");
        fs::write(&foreign, b"keep").unwrap();
        assert_eq!(prune_at(&room, 100 + RETENTION_MS, 16).unwrap(), 0);
        assert!(Path::new(&short.path).is_file());
        assert!(foreign.is_file());
        assert_eq!(
            read_at(&room, &long, 100 + RETENTION_MS).unwrap(),
            page.page.as_bytes()
        );
        fs::remove_file(foreign).unwrap();
        assert_eq!(prune_at(&room, 100 + RETENTION_MS, 16).unwrap(), 1);
        assert!(!Path::new(&short.path).exists());
        assert!(Path::new(&long.path).is_file());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_destination_parent_and_managed_result_refuse() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let page = page(10);
        let artifact = materialize_at(&room, &page, 100).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(dir.path(), &link).unwrap();
        assert!(admit_output(link.join("preview.svg").to_str().unwrap()).is_err());
        let target = Path::new(&artifact.path).parent().unwrap();
        let moved = dir.path().join("moved");
        fs::rename(target, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, target).unwrap();
        assert!(read_at(&room, &artifact, 101).is_err());
        assert!(materialize_at(&room, &page, 101).is_err());
    }
}
