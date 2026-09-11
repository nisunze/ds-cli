//! Shared native report artifact authority for Desktop, CLI and Server.
//! Owns crash-durable publication commits and receipt-bound artifact access.
//! No Tauri dependency, process launcher, browser session or gateway credential.
pub mod confined_fs;
pub mod publication;
use confined_fs::{HeldDirectory, HeldFileLock};
use publication::{
    PendingPublicationCommit, PublicationBatchReceipt, PublicationOutput, BATCH_DIRECTORY,
    BATCH_SCHEMA,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const MAX_RECOVERY_CANDIDATES: usize = 256;

pub const MAX_TRANSFERABLE_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024 * 1024;

pub const HARD_ARTIFACT_LIMIT_BYTES: u64 = 64 * 1024 * 1024;

pub const MAX_RECEIPT_BYTES: u64 = 16 * 1024;

pub const PENDING_UPLOAD_LOCK_FILE: &str = ".publication.lock";

pub const DISCARDING_FILE: &str = ".artifact.discarding";

pub const ARTIFACT_FILE: &str = "artifact.bin";

pub const RECEIPT_FILE: &str = "artifact-receipt.json";

pub const RECEIPT_SCHEMA: u32 = 1;

pub const LOCATOR_PREFIX: &str = "ds-reporter:v1:";

/// Path-free evidence for one crash-durable local publication batch.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveredNetworkReporterPublication {
    pub batch_id: String,
    pub owner_uid: String,
    pub organization_id: Option<String>,
    pub project_id: String,
    pub root: String,
    pub client_run_id: String,
    pub engine: String,
    pub operation: String,
    pub variant: String,
    pub engine_version: String,
    pub engine_build_manifest_sha256: String,
    pub transformer: String,
    pub transformer_revision: i64,
    pub input_base_fingerprint: String,
    pub room_content_sha256: String,
    pub snapshot_base: BTreeMap<String, String>,
    pub client_publish_id: String,
    pub artifacts: Vec<PublishedArtifact>,
}

/// Opaque native discovery handle. The owner is supplied separately and is
/// matched against the sealed receipt before any metadata crosses IPC.
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkReporterRecoveryCandidate {
    pub batch_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkReporterLocalArtifactSet {
    pub project_id: String,
    pub transformer: String,
    pub operation: String,
    pub variant: String,
    pub client_run_id: String,
    pub created_at: u64,
    pub artifacts: Vec<PublishedArtifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedArtifact {
    pub output_id: String,
    pub locator: String,
    pub filename: String,
    pub format: String,
    pub content_type: String,
    pub sha256: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paper_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<ds_command_kernel::printing::ArtifactPresentation>,
}

#[derive(Debug)]
pub struct VerifiedSidecarArtifact {
    pub format: String,
    pub filename: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub paper_size: Option<String>,
    pub presentation: Option<ds_command_kernel::printing::ArtifactPresentation>,
    pub held_file: File,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkReporterArtifactReceipt {
    pub schema: u32,
    pub batch_id: String,
    pub output_id: String,
    pub locator: String,
    pub artifact_file: String,
    pub size_bytes: u64,
    pub sha256: String,
}

pub fn exact_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn valid_compute_id(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn valid_semantic_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value == value.trim()
        && !value.chars().any(char::is_control)
}

pub fn content_type(format: &str) -> Result<&'static str, String> {
    Ok(ds_command_kernel::report_formats::output(format)?.content_type)
}

pub fn copy_verified_artifact(
    artifact: &VerifiedSidecarArtifact,
    destination: &HeldDirectory,
    destination_name: &str,
    authority_deadline: Instant,
) -> Result<(), String> {
    ensure_promotion_authority(authority_deadline)?;
    if artifact.size_bytes == 0
        || artifact.size_bytes > MAX_TRANSFERABLE_ARTIFACT_BYTES
        || !exact_lower_sha256(&artifact.sha256)
    {
        return Err("Network Reporter promotion identity is invalid".to_string());
    }
    let mut source = artifact
        .held_file
        .try_clone()
        .map_err(|error| format!("could not retain Network Reporter staged artifact: {error}"))?;
    source
        .seek(std::io::SeekFrom::Start(0))
        .map_err(|error| format!("could not rewind Network Reporter staged artifact: {error}"))?;
    if source
        .metadata()
        .map_err(|error| format!("could not inspect Network Reporter staged artifact: {error}"))?
        .len()
        != artifact.size_bytes
    {
        return Err("Network Reporter staged artifact size changed before promotion".to_string());
    }
    let mut destination_file = destination.create_new_file(
        std::ffi::OsStr::new(destination_name),
        "Network Reporter pending artifact",
    )?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        ensure_promotion_authority(authority_deadline)?;
        let count = source
            .read(&mut buffer)
            .map_err(|error| format!("could not read Network Reporter staged artifact: {error}"))?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| "Network Reporter promotion size overflowed".to_string())?;
        if total > artifact.size_bytes || total > MAX_TRANSFERABLE_ARTIFACT_BYTES {
            return Err("Network Reporter staged artifact grew during promotion".to_string());
        }
        digest.update(&buffer[..count]);
        destination_file
            .write_all(&buffer[..count])
            .map_err(|error| {
                format!("could not write Network Reporter pending artifact: {error}")
            })?;
    }
    if total != artifact.size_bytes || format!("{:x}", digest.finalize()) != artifact.sha256 {
        return Err("Network Reporter staged artifact changed during promotion".to_string());
    }
    ensure_promotion_authority(authority_deadline)?;
    destination_file
        .sync_all()
        .map_err(|error| format!("could not sync Network Reporter pending artifact: {error}"))?;
    Ok(())
}

pub fn ensure_promotion_authority(deadline: Instant) -> Result<(), String> {
    if deadline <= Instant::now() {
        return Err(
            "Network Reporter native authority expired during artifact promotion".to_string(),
        );
    }
    Ok(())
}

#[derive(Debug)]
pub struct PendingPromotion {
    pub root: HeldDirectory,
    pub temporary: Option<HeldDirectory>,
    pub final_directory: Option<HeldDirectory>,
    pub final_path: PathBuf,
    pub published: PublishedArtifact,
    pub finalized: bool,
    pub armed: bool,
}

impl PendingPromotion {
    pub fn stage(
        root: &HeldDirectory,
        artifact: &VerifiedSidecarArtifact,
        batch_id: &str,
        authority_deadline: Instant,
    ) -> Result<Self, String> {
        ensure_promotion_authority(authority_deadline)?;
        let content_type = content_type(&artifact.format)?.to_string();
        for _ in 0..16 {
            let id = random_hex_256();
            let locator = format!("{LOCATOR_PREFIX}{id}");
            let final_path = root.path().join(&id);
            if root
                .names()?
                .iter()
                .any(|name| name == std::ffi::OsStr::new(&id))
            {
                continue;
            }
            let temporary_name = format!(".promoting-{id}");
            let temporary = match root.create_child(std::ffi::OsStr::new(&temporary_name)) {
                Ok(directory) => directory,
                Err(error) if error.contains("already exists") => continue,
                Err(error) => return Err(error),
            };
            let staged = (|| {
                copy_verified_artifact(artifact, &temporary, ARTIFACT_FILE, authority_deadline)?;
                let receipt = NetworkReporterArtifactReceipt {
                    schema: RECEIPT_SCHEMA,
                    batch_id: batch_id.to_string(),
                    output_id: artifact.format.clone(),
                    locator: locator.clone(),
                    artifact_file: ARTIFACT_FILE.to_string(),
                    size_bytes: artifact.size_bytes,
                    sha256: artifact.sha256.clone(),
                };
                let mut receipt_bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| {
                    format!("could not encode Network Reporter artifact receipt: {error}")
                })?;
                receipt_bytes.push(b'\n');
                temporary.write_new_bounded(
                    std::ffi::OsStr::new(RECEIPT_FILE),
                    &receipt_bytes,
                    MAX_RECEIPT_BYTES,
                    "Network Reporter artifact receipt",
                )?;
                temporary.sync()
            })();
            if let Err(error) = staged {
                let _ = root.remove_child_tree(temporary);
                return Err(error);
            }
            return Ok(Self {
                root: root.clone(),
                temporary: Some(temporary),
                final_directory: None,
                final_path,
                published: PublishedArtifact {
                    output_id: artifact.format.clone(),
                    locator,
                    filename: artifact.filename.clone(),
                    format: artifact.format.clone(),
                    content_type: content_type.clone(),
                    sha256: artifact.sha256.clone(),
                    size_bytes: artifact.size_bytes,
                    paper_size: artifact.paper_size.clone(),
                    presentation: artifact.presentation.clone(),
                },
                finalized: false,
                armed: true,
            });
        }
        Err("could not allocate a Network Reporter artifact locator".to_string())
    }

    pub fn finalize(&mut self, authority_deadline: Instant) -> Result<(), String> {
        ensure_promotion_authority(authority_deadline)?;
        let temporary = self
            .temporary
            .take()
            .ok_or_else(|| "Network Reporter promotion lost its temporary directory".to_string())?;
        let final_name = self
            .final_path
            .file_name()
            .ok_or_else(|| "Network Reporter promotion has no final name".to_string())?;
        self.final_directory = Some(self.root.rename_child(temporary, final_name)?);
        self.finalized = true;
        Ok(())
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }
}

#[derive(Debug)]
pub struct PendingPromotionBatch {
    pub promotions: Vec<PendingPromotion>,
    // Retain the cross-lane lock until the batch is either committed or its
    // armed Drop rollback has removed every temporary/final locator.
    pub _lock: HeldFileLock,
}

impl PendingPromotionBatch {
    /// Disarm rollback only after the caller has revalidated the live native
    /// authority once more after all blocking copy/rename/fsync work.
    pub fn commit(mut self) -> Vec<PublishedArtifact> {
        let published = self
            .promotions
            .iter()
            .map(|promotion| promotion.published.clone())
            .collect();
        for promotion in &mut self.promotions {
            promotion.disarm();
        }
        published
    }
}

impl Drop for PendingPromotion {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let directory = if self.finalized {
            self.final_directory.take()
        } else {
            self.temporary.take()
        };
        if let Some(directory) = directory {
            if let Err(error) = self.root.remove_child_tree(directory) {
                eprintln!("failed to roll back Network Reporter artifact promotion: {error}");
            }
        }
    }
}

#[derive(Debug)]
pub struct PendingAuthorizedPromotionBatch {
    // Declaration order is drop order. Remove the logical commit marker first
    // on rollback, then remove locator directories while `promotions` still
    // retains the pending-upload lock through its final field.
    pub publication: PendingPublicationCommit,
    pub promotions: PendingPromotionBatch,
}

#[derive(Debug)]
pub struct CommittedAuthorizedPublication {
    pub receipt: PublicationBatchReceipt,
    pub artifacts: Vec<PublishedArtifact>,
}

impl PendingAuthorizedPromotionBatch {
    pub fn commit(
        mut self,
        authority_deadline: Instant,
    ) -> Result<CommittedAuthorizedPublication, String> {
        ensure_promotion_authority(authority_deadline)?;
        self.publication.activate()?;
        // The one marker rename is the logical visibility boundary. Check the
        // conservative monotonic deadline again after its durability sync; on
        // failure both marker and locator vector remain armed for rollback.
        ensure_promotion_authority(authority_deadline)?;
        let receipt = self.publication.commit()?;
        let artifacts = self.promotions.commit();
        Ok(CommittedAuthorizedPublication { receipt, artifacts })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn local_publication_receipt(
    owner_uid: &str,
    project_id: &str,
    engine_version: &str,
    engine_build_manifest_sha256: &str,
    transformer: &str,
    transformer_revision: i64,
    input_base_fingerprint: &str,
    room_content_sha256: &str,
    client_run_id: &str,
    batch_id: String,
    promotions: &PendingPromotionBatch,
    expected_formats: &[&str],
) -> Result<PublicationBatchReceipt, String> {
    if promotions.promotions.len() != expected_formats.len() {
        return Err(
            "Network Reporter promotion does not match its local output inventory".to_string(),
        );
    }
    let mut outputs = Vec::with_capacity(promotions.promotions.len());
    for (promotion, expected_format) in promotions
        .promotions
        .iter()
        .zip(expected_formats.iter().copied())
    {
        let published = &promotion.published;
        if published.output_id != expected_format
            || published.format != expected_format
            || published.content_type != content_type(expected_format)?
        {
            return Err(
                "Network Reporter promotion changed its local output inventory".to_string(),
            );
        }
        outputs.push(PublicationOutput {
            output_id: published.output_id.clone(),
            locator: published.locator.clone(),
            filename: published.filename.clone(),
            format: published.format.clone(),
            content_type: published.content_type.clone(),
            sha256: published.sha256.clone(),
            size_bytes: published.size_bytes,
            paper_size: published.paper_size.clone(),
            presentation: published.presentation.clone(),
        });
    }
    let operation = format!("export-{transformer}");
    Ok(PublicationBatchReceipt {
        schema: if ds_command_kernel::report::publication_state(engine_version, true, false)?
            == ds_command_kernel::report::PublicationState::LocalOnly
        {
            publication::LOCAL_ONLY_SCHEMA
        } else {
            BATCH_SCHEMA
        },
        batch_id,
        owner_uid: owner_uid.to_string(),
        organization_id: None,
        project_id: project_id.to_string(),
        root: format!("eds_project/{project_id}/eds_design"),
        client_run_id: client_run_id.to_string(),
        engine: "network_reporter".to_string(),
        operation,
        variant: "default".to_string(),
        engine_version: engine_version.to_string(),
        engine_build_manifest_sha256: engine_build_manifest_sha256.to_string(),
        transformer: transformer.to_string(),
        transformer_revision,
        input_base_fingerprint: input_base_fingerprint.to_string(),
        room_content_sha256: room_content_sha256.to_string(),
        grant_id: None,
        snapshot_base: BTreeMap::new(),
        client_publish_id: format!("publish-{}", uuid::Uuid::new_v4()),
        outputs,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn promote_local_artifacts(
    pending_root: &Path,
    artifacts: &[VerifiedSidecarArtifact],
    owner_uid: &str,
    project_id: &str,
    engine_version: &str,
    engine_build_manifest_sha256: &str,
    transformer: &str,
    transformer_revision: i64,
    input_base_fingerprint: &str,
    room_content_sha256: &str,
    client_run_id: &str,
    expected_formats: &[&str],
    operation_deadline: Instant,
) -> Result<PendingAuthorizedPromotionBatch, String> {
    ensure_promotion_authority(operation_deadline)?;
    let pending_root = HeldDirectory::ensure_absolute(pending_root)?;
    let lock = pending_root.promotion_lock_until(
        std::ffi::OsStr::new(PENDING_UPLOAD_LOCK_FILE),
        "Network Reporter pending-upload lock",
        operation_deadline,
    )?;
    ensure_promotion_authority(operation_deadline)?;
    sweep_abandoned_pending_upload_locked(&pending_root, &lock)?;
    let batch_id = random_hex_256();
    let mut promotions = Vec::with_capacity(artifacts.len());
    for artifact in artifacts {
        promotions.push(PendingPromotion::stage(
            &pending_root,
            artifact,
            &batch_id,
            operation_deadline,
        )?);
    }
    let mut promotions = PendingPromotionBatch {
        promotions,
        _lock: lock,
    };
    let receipt = local_publication_receipt(
        owner_uid,
        project_id,
        engine_version,
        engine_build_manifest_sha256,
        transformer,
        transformer_revision,
        input_base_fingerprint,
        room_content_sha256,
        client_run_id,
        batch_id,
        &promotions,
        expected_formats,
    )?;
    let publication = PendingPublicationCommit::stage(&pending_root, &receipt)?;
    for promotion in &mut promotions.promotions {
        promotion.finalize(operation_deadline)?;
    }
    pending_root.sync()?;
    ensure_promotion_authority(operation_deadline)?;
    Ok(PendingAuthorizedPromotionBatch {
        publication,
        promotions,
    })
}

pub fn locator_id(locator: &str) -> Result<&str, String> {
    let id = locator.strip_prefix(LOCATOR_PREFIX).ok_or_else(|| {
        "Network Reporter artifact locator has an unsupported contract".to_string()
    })?;
    if !exact_lower_sha256(id) {
        return Err("Network Reporter artifact locator is invalid".to_string());
    }
    Ok(id)
}

#[derive(Debug)]
pub struct HeldArtifactEntry {
    pub root: HeldDirectory,
    pub entry: HeldDirectory,
}

pub fn checked_entry_in_root(
    root: &HeldDirectory,
    locator: &str,
) -> Result<Option<HeldArtifactEntry>, String> {
    let id = locator_id(locator)?;
    let Some(entry) = root.open_child(std::ffi::OsStr::new(id))? else {
        return Ok(None);
    };
    Ok(Some(HeldArtifactEntry {
        root: root.clone(),
        entry,
    }))
}

pub fn read_receipt(
    entry: &HeldDirectory,
    locator: &str,
) -> Result<NetworkReporterArtifactReceipt, String> {
    let bytes = entry.read_bounded(
        std::ffi::OsStr::new(RECEIPT_FILE),
        MAX_RECEIPT_BYTES,
        "Network Reporter artifact receipt",
    )?;
    let receipt: NetworkReporterArtifactReceipt = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Network Reporter artifact receipt is malformed: {error}"))?;
    if receipt.schema != RECEIPT_SCHEMA
        || !exact_lower_sha256(&receipt.batch_id)
        || !valid_compute_id(&receipt.output_id, 128)
        || receipt.locator != locator
        || receipt.artifact_file != ARTIFACT_FILE
        || receipt.size_bytes == 0
        || receipt.size_bytes > MAX_TRANSFERABLE_ARTIFACT_BYTES
        || !exact_lower_sha256(&receipt.sha256)
    {
        return Err("Network Reporter artifact receipt does not match its locator".to_string());
    }
    Ok(receipt)
}

pub fn verify_open_artifact(
    file: &mut File,
    expected_size: u64,
    expected_sha256: &str,
    collect: bool,
) -> Result<Vec<u8>, String> {
    let actual_size = file
        .metadata()
        .map_err(|error| format!("could not inspect opened Network Reporter artifact: {error}"))?
        .len();
    if actual_size != expected_size {
        return Err("Network Reporter artifact size does not match its receipt".to_string());
    }
    let capacity = if collect {
        usize::try_from(expected_size)
            .map_err(|_| "Network Reporter artifact is too large for this platform".to_string())?
    } else {
        0
    };
    let mut bytes = Vec::with_capacity(capacity);
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("could not read Network Reporter artifact: {error}"))?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| "Network Reporter artifact size overflowed".to_string())?;
        if total > expected_size
            || total > MAX_TRANSFERABLE_ARTIFACT_BYTES
            || (collect && total > HARD_ARTIFACT_LIMIT_BYTES)
        {
            return Err("Network Reporter artifact changed while it was read".to_string());
        }
        digest.update(&buffer[..count]);
        if collect {
            bytes.extend_from_slice(&buffer[..count]);
        }
    }
    if total != expected_size {
        return Err("Network Reporter artifact size does not match its receipt".to_string());
    }
    if format!("{:x}", digest.finalize()) != expected_sha256 {
        return Err("Network Reporter artifact digest does not match its receipt".to_string());
    }
    Ok(bytes)
}

/// One receipt-verified, authority-gated open of a pending artifact.
///
/// Every consumer of a pending artifact's bytes passes through here: the opaque
/// locator, the entry inventory, the artifact receipt, the committed batch
/// receipt and the caller's own gate are all enforced before a handle exists.
/// Consumers differ only in what they then do with the handle — collect it into
/// memory (preview/copy) or stream bounded ranges out of it (transfer).
pub struct AuthorizedArtifact {
    pub file: File,
    pub receipt: NetworkReporterArtifactReceipt,
    pub committed: PublicationBatchReceipt,
}

pub fn open_artifact_from_root_with_gate<F>(
    root: &Path,
    locator: &str,
    mut authorize: F,
) -> Result<AuthorizedArtifact, String>
where
    F: FnMut(&PublicationBatchReceipt) -> Result<Instant, String>,
{
    let pending_root = HeldDirectory::open_absolute(root)?
        .ok_or_else(|| "Network Reporter artifact locator is unavailable".to_string())?;
    // Committed locator directories and their receipt are immutable. Reading
    // one through held directory/file handles therefore does not need the
    // writer lock. Taking that global lock here made a harmless background
    // publication prevent PDF preview and downloads with "lock is busy".
    // The digest and authority checks below still fail closed if a concurrent
    // confirmed publication removes the visible receipt before it is opened.
    let held = checked_entry_in_root(&pending_root, locator)?
        .ok_or_else(|| "Network Reporter artifact locator is unavailable".to_string())?;
    let names = exact_entry_names(&held.entry)?;
    if names.len() != 2
        || !names.iter().any(|name| name == RECEIPT_FILE)
        || !names.iter().any(|name| name == ARTIFACT_FILE)
    {
        return Err("Network Reporter artifact entry is not in its readable state".to_string());
    }
    let receipt = read_receipt(&held.entry, locator)?;
    let committed = publication::require_committed_output(
        &held.root,
        &receipt.batch_id,
        locator,
        &receipt.output_id,
        &receipt.sha256,
        receipt.size_bytes,
    )?;
    authorize(&committed)?;
    let artifact = held.entry.open_regular_file(
        std::ffi::OsStr::new(ARTIFACT_FILE),
        "Network Reporter artifact",
    )?;
    let actual = artifact
        .metadata()
        .map_err(|error| format!("could not inspect opened Network Reporter artifact: {error}"))?
        .len();
    if actual != receipt.size_bytes {
        return Err("Network Reporter artifact size does not match its receipt".to_string());
    }
    Ok(AuthorizedArtifact {
        file: artifact,
        receipt,
        committed,
    })
}

pub fn read_artifact_from_root_with_gate<F>(
    root: &Path,
    locator: &str,
    max_bytes: u64,
    mut authorize: F,
) -> Result<Vec<u8>, String>
where
    F: FnMut(&PublicationBatchReceipt) -> Result<Instant, String>,
{
    let limit = max_bytes.min(HARD_ARTIFACT_LIMIT_BYTES);
    if limit == 0 {
        return Err("Network Reporter artifact read limit must be greater than zero".to_string());
    }
    let mut opened = open_artifact_from_root_with_gate(root, locator, &mut authorize)?;
    if opened.receipt.size_bytes > limit {
        return Err(format!(
            "Network Reporter artifact exceeds the requested {limit}-byte limit"
        ));
    }
    let bytes = verify_open_artifact(
        &mut opened.file,
        opened.receipt.size_bytes,
        &opened.receipt.sha256,
        true,
    )?;
    authorize(&opened.committed)?;
    Ok(bytes)
}

/// A closed, receipt-verified handle for **streaming** transfer.
///
/// The same authority as [`read_authorized_artifact_bytes`]; it differs only in
/// that the bytes are never collected. The caller streams bounded ranges from
/// the single held handle and verifies `sha256` itself before reporting
/// success, so artifact size stays independent of resident memory.
pub struct SealedArtifactHandle {
    pub file: File,
    pub size_bytes: u64,
    pub sha256: String,
}

pub fn list_recovery_candidates_from_root(
    root: &Path,
    owner_uid: &str,
) -> Result<Vec<NetworkReporterRecoveryCandidate>, String> {
    if !valid_semantic_text(owner_uid, 128) {
        return Err("Network Reporter recovery owner is invalid".to_string());
    }
    let Some(pending_root) = HeldDirectory::open_absolute(root)? else {
        return Ok(Vec::new());
    };
    let lock = pending_root.try_lock_file(
        std::ffi::OsStr::new(PENDING_UPLOAD_LOCK_FILE),
        "Network Reporter pending-upload lock",
    )?;
    let (_, receipts) = sweep_abandoned_pending_upload_locked(&pending_root, &lock)?;
    let matching = receipts
        .into_iter()
        .filter(|receipt| {
            receipt.owner_uid == owner_uid && receipt.schema != publication::LOCAL_ONLY_SCHEMA
        })
        .collect::<Vec<_>>();
    if matching.len() > MAX_RECOVERY_CANDIDATES {
        return Err(format!(
            "Network Reporter recovery has more than {MAX_RECOVERY_CANDIDATES} pending batches"
        ));
    }
    let mut candidates = Vec::with_capacity(matching.len());
    for receipt in matching {
        candidates.push(NetworkReporterRecoveryCandidate {
            batch_id: receipt.batch_id,
        });
    }
    Ok(candidates)
}

pub fn list_project_local_artifacts_from_root(
    root: &Path,
    owner_uid: &str,
    project_id: &str,
    transformer: Option<&str>,
) -> Result<Vec<NetworkReporterLocalArtifactSet>, String> {
    if !valid_semantic_text(owner_uid, 128)
        || !valid_semantic_text(project_id, 128)
        || transformer.is_some_and(|name| !valid_semantic_text(name, 128))
    {
        return Err("Network Reporter local artifact scope is invalid".to_string());
    }
    let Some(pending_root) = HeldDirectory::open_absolute(root)? else {
        return Ok(Vec::new());
    };
    // Listing is a projection of immutable committed receipts. Cleanup owns
    // the exclusive lock; a read-only inventory must remain available while
    // background publication is moving another batch.
    let receipts = publication::list_committed(&pending_root)?;
    let mut sets = receipts
        .into_iter()
        .filter(|receipt| {
            receipt.owner_uid == owner_uid
                && receipt.project_id == project_id
                && transformer.is_none_or(|name| receipt.transformer == name)
        })
        .map(|receipt| {
            let created_at = std::fs::metadata(
                root.join(publication::BATCH_DIRECTORY)
                    .join(&receipt.batch_id),
            )
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |elapsed| {
                elapsed.as_millis().min(u64::MAX as u128) as u64
            });
            NetworkReporterLocalArtifactSet {
                project_id: receipt.project_id,
                transformer: receipt.transformer,
                operation: receipt.operation,
                variant: receipt.variant,
                client_run_id: receipt.client_run_id,
                created_at,
                artifacts: receipt
                    .outputs
                    .into_iter()
                    .map(|output| PublishedArtifact {
                        output_id: output.output_id,
                        locator: output.locator,
                        filename: output.filename,
                        format: output.format,
                        content_type: output.content_type,
                        sha256: output.sha256,
                        size_bytes: output.size_bytes,
                        paper_size: output.paper_size,
                        presentation: output.presentation,
                    })
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    sets.sort_by_key(|row| std::cmp::Reverse(row.created_at));
    // Preserve each unselected output in its original committed batch. New
    // iterations replace canonical outputs, never an entire report inventory.
    let indices = ds_command_kernel::report::current_output_indices(
        &sets
            .iter()
            .map(|set| {
                set.artifacts
                    .iter()
                    .map(|artifact| {
                        format!(
                            "{}\0{}\0{}",
                            set.transformer, set.variant, artifact.output_id
                        )
                    })
                    .collect()
            })
            .collect::<Vec<_>>(),
    );
    for (set, kept) in sets.iter_mut().zip(indices) {
        set.artifacts = std::mem::take(&mut set.artifacts)
            .into_iter()
            .enumerate()
            .filter_map(|(index, artifact)| kept.contains(&index).then_some(artifact))
            .collect();
    }
    sets.retain(|set| !set.artifacts.is_empty());
    Ok(sets)
}

/// One committed local publication batch, as the sync gate reads it.
///
/// Project-keyed, never account-keyed: a sync row is project data, and the
/// grant that carries it is bound to an install and a project
/// (`ds-command-kernel/docs/contracts/ds-sync-engine.md` §3.2). `owner_uid`
/// travels only because the artifact read gate is receipt-bound, and it comes
/// from the same receipt this row was projected from rather than from a
/// caller's claim.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalPublicationBatch {
    pub batch_id: String,
    pub owner_uid: String,
    pub project_id: String,
    pub client_publish_id: String,
    /// The compute-artifact engine id (`network_reporter`), which is what a
    /// head is keyed by. The edge lane spells the same engine
    /// `ds-network-reporter`; they are two vocabularies, not a choice.
    pub engine: String,
    pub operation: String,
    pub variant: String,
    /// `ds-network-reporter@<semver>+<40 hex>` — the sidecar release that
    /// produced these bytes, as the receipt recorded it.
    pub engine_release: String,
    pub engine_build_manifest_sha256: String,
    pub transformer: String,
    /// The v2 OCC/head identity this result was computed against. It is the
    /// same value `snapshot_base` pins at publish, so it is what the sync gate
    /// reads as this row's base revision.
    pub input_base_fingerprint: String,
    pub produced_at_ms: u64,
    pub outputs: Vec<PublishedArtifact>,
}

pub fn list_project_publication_batches_from_root(
    root: &Path,
    project_id: &str,
) -> Result<Vec<LocalPublicationBatch>, String> {
    if !valid_semantic_text(project_id, 128) {
        return Err("Network Reporter publication scope is invalid".to_string());
    }
    let Some(pending_root) = HeldDirectory::open_absolute(root)? else {
        return Ok(Vec::new());
    };
    let mut batches: Vec<LocalPublicationBatch> = publication::list_committed(&pending_root)?
        .into_iter()
        .filter(|receipt| {
            receipt.project_id == project_id && receipt.schema == publication::BATCH_SCHEMA
        })
        .map(|receipt| {
            let produced_at_ms = std::fs::metadata(
                root.join(publication::BATCH_DIRECTORY)
                    .join(&receipt.batch_id),
            )
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |elapsed| {
                elapsed.as_millis().min(u64::MAX as u128) as u64
            });
            LocalPublicationBatch {
                batch_id: receipt.batch_id,
                owner_uid: receipt.owner_uid,
                project_id: receipt.project_id,
                client_publish_id: receipt.client_publish_id,
                engine: receipt.engine,
                operation: receipt.operation,
                variant: receipt.variant,
                engine_release: receipt.engine_version,
                engine_build_manifest_sha256: receipt.engine_build_manifest_sha256,
                transformer: receipt.transformer,
                input_base_fingerprint: receipt.input_base_fingerprint,
                produced_at_ms,
                outputs: receipt
                    .outputs
                    .into_iter()
                    .map(|output| PublishedArtifact {
                        output_id: output.output_id,
                        locator: output.locator,
                        filename: output.filename,
                        format: output.format,
                        content_type: output.content_type,
                        sha256: output.sha256,
                        size_bytes: output.size_bytes,
                        paper_size: output.paper_size,
                        presentation: output.presentation,
                    })
                    .collect(),
            }
        })
        .collect();
    batches.sort_by(|left, right| {
        right
            .produced_at_ms
            .cmp(&left.produced_at_ms)
            .then_with(|| left.batch_id.cmp(&right.batch_id))
    });
    Ok(batches)
}

/// Reopen and verify one exact committed batch without materializing artifact
/// bytes. Enumeration and adoption are intentionally separate: the caller
/// must present the authenticated local owner. Recovery rehashes the committed
/// receipt and inventory; it does not claim a server-minted entitlement.
pub fn recover_committed_publication_from_root(
    root: &Path,
    owner_uid: &str,
    batch_id: &str,
) -> Result<Option<RecoveredNetworkReporterPublication>, String> {
    let Some(pending_root) = HeldDirectory::open_absolute(root)? else {
        return Ok(None);
    };
    let _lock = pending_root.try_lock_file(
        std::ffi::OsStr::new(PENDING_UPLOAD_LOCK_FILE),
        "Network Reporter pending-upload lock",
    )?;
    sweep_abandoned_pending_upload_locked(&pending_root, &_lock)?;
    let Some(receipt) = publication::committed_receipt(&pending_root, batch_id)? else {
        return Ok(None);
    };
    if receipt.owner_uid != owner_uid {
        return Err("Network Reporter recovery owner does not match the local receipt".to_string());
    }
    if receipt.schema == publication::LOCAL_ONLY_SCHEMA {
        return Err(
            "Development report artifacts are local-only and cannot enter cloud publication"
                .to_string(),
        );
    }
    let mut artifacts = Vec::with_capacity(receipt.outputs.len());
    for output in &receipt.outputs {
        let locator_name = locator_id(&output.locator)?;
        let entry = pending_root
            .open_child(std::ffi::OsStr::new(locator_name))?
            .ok_or_else(|| {
                "Network Reporter committed publication has a missing artifact".to_string()
            })?;
        let names = exact_entry_names(&entry)?;
        if names.len() != 2
            || !names.iter().any(|name| name == RECEIPT_FILE)
            || !names.iter().any(|name| name == ARTIFACT_FILE)
        {
            return Err(
                "Network Reporter committed publication has an unreadable artifact".to_string(),
            );
        }
        let artifact_receipt = read_receipt(&entry, &output.locator)?;
        if artifact_receipt.batch_id != receipt.batch_id
            || artifact_receipt.output_id != output.output_id
            || artifact_receipt.sha256 != output.sha256
            || artifact_receipt.size_bytes != output.size_bytes
        {
            return Err(
                "Network Reporter artifact changed from its committed publication".to_string(),
            );
        }
        let committed = publication::require_committed_output(
            &pending_root,
            &receipt.batch_id,
            &output.locator,
            &output.output_id,
            &output.sha256,
            output.size_bytes,
        )?;
        if committed != receipt {
            return Err("Network Reporter publication changed while it was recovered".to_string());
        }
        let mut artifact = entry.open_regular_file(
            std::ffi::OsStr::new(ARTIFACT_FILE),
            "Network Reporter recovery artifact",
        )?;
        verify_open_artifact(
            &mut artifact,
            artifact_receipt.size_bytes,
            &artifact_receipt.sha256,
            false,
        )?;
        artifacts.push(PublishedArtifact {
            output_id: output.output_id.clone(),
            locator: output.locator.clone(),
            filename: output.filename.clone(),
            format: output.format.clone(),
            content_type: output.content_type.clone(),
            sha256: output.sha256.clone(),
            size_bytes: output.size_bytes,
            paper_size: output.paper_size.clone(),
            presentation: output.presentation.clone(),
        });
    }
    Ok(Some(RecoveredNetworkReporterPublication {
        batch_id: receipt.batch_id,
        owner_uid: receipt.owner_uid,
        organization_id: receipt.organization_id,
        project_id: receipt.project_id,
        root: receipt.root,
        client_run_id: receipt.client_run_id,
        engine: receipt.engine,
        operation: receipt.operation,
        variant: receipt.variant,
        engine_version: receipt.engine_version,
        engine_build_manifest_sha256: receipt.engine_build_manifest_sha256,
        transformer: receipt.transformer,
        transformer_revision: receipt.transformer_revision,
        input_base_fingerprint: receipt.input_base_fingerprint,
        room_content_sha256: receipt.room_content_sha256,
        snapshot_base: receipt.snapshot_base,
        client_publish_id: receipt.client_publish_id,
        artifacts,
    }))
}

pub fn exact_entry_names(entry: &HeldDirectory) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for name in entry.names()? {
        let name = name
            .into_string()
            .map_err(|_| "Network Reporter artifact entry has a non-Unicode member".to_string())?;
        if name != RECEIPT_FILE && name != ARTIFACT_FILE && name != DISCARDING_FILE {
            return Err("Network Reporter artifact entry contains an unowned member".to_string());
        }
        names.push(name);
    }
    Ok(names)
}

/// Reap only reporter-owned pre-commit state while the caller holds the
/// process-crash-safe pending-upload lock. A committed batch marker is the
/// sole retention authority; age and process identifiers are never used.
pub fn sweep_abandoned_pending_upload_locked(
    pending_root: &HeldDirectory,
    _lock: &HeldFileLock,
) -> Result<(usize, Vec<PublicationBatchReceipt>), String> {
    let committed = publication::sweep_staged_and_list_committed(pending_root)?;
    let committed_by_id = committed
        .iter()
        .map(|receipt| (receipt.batch_id.as_str(), receipt))
        .collect::<HashMap<_, _>>();
    let mut removed = 0_usize;
    for name in pending_root.names()? {
        let name = name.into_string().map_err(|_| {
            "Network Reporter pending-upload root contains a non-Unicode member".to_string()
        })?;
        if name == PENDING_UPLOAD_LOCK_FILE || name == BATCH_DIRECTORY {
            continue;
        }
        if let Some(id) = name.strip_prefix(".promoting-") {
            if !exact_lower_sha256(id) {
                return Err(
                    "Network Reporter pending-upload root contains an invalid promotion"
                        .to_string(),
                );
            }
            let staging = pending_root
                .open_child(std::ffi::OsStr::new(&name))?
                .ok_or_else(|| {
                    "Network Reporter abandoned promotion changed during recovery".to_string()
                })?;
            pending_root.remove_child_tree(staging)?;
            removed += 1;
            continue;
        }
        if !exact_lower_sha256(&name) {
            return Err(
                "Network Reporter pending-upload root contains an unowned member".to_string(),
            );
        }
        let locator = format!("{LOCATOR_PREFIX}{name}");
        let entry = pending_root
            .open_child(std::ffi::OsStr::new(&name))?
            .ok_or_else(|| {
                "Network Reporter pending artifact changed during recovery".to_string()
            })?;
        exact_entry_names(&entry)?;
        let artifact_receipt = read_receipt(&entry, &locator)?;
        let Some(batch) = committed_by_id.get(artifact_receipt.batch_id.as_str()) else {
            pending_root.remove_child_tree(entry)?;
            removed += 1;
            continue;
        };
        let exact = publication::require_committed_output(
            pending_root,
            &artifact_receipt.batch_id,
            &artifact_receipt.locator,
            &artifact_receipt.output_id,
            &artifact_receipt.sha256,
            artifact_receipt.size_bytes,
        )?;
        if &exact != *batch {
            return Err(
                "Network Reporter committed publication changed during recovery".to_string(),
            );
        }
    }
    if removed > 0 {
        pending_root.sync()?;
    }
    Ok((removed, committed))
}

pub fn restore_discarding_artifact(entry: &HeldDirectory) -> Result<(), String> {
    if entry
        .names()?
        .iter()
        .any(|name| name == std::ffi::OsStr::new(ARTIFACT_FILE))
    {
        return Err(
            "Network Reporter artifact changed while a failed discard was restored".to_string(),
        );
    }
    entry.rename_file(
        std::ffi::OsStr::new(DISCARDING_FILE),
        std::ffi::OsStr::new(ARTIFACT_FILE),
        "rejected Network Reporter artifact",
    )
}

pub fn committed_publication_for_locator(
    pending_root: &HeldDirectory,
    locator: &str,
) -> Result<Option<PublicationBatchReceipt>, String> {
    let matches = publication::list_committed(pending_root)?
        .into_iter()
        .filter(|receipt| {
            receipt
                .outputs
                .iter()
                .any(|output| output.locator == locator)
        })
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err("Network Reporter locator belongs to multiple publication batches".to_string());
    }
    Ok(matches.into_iter().next())
}

pub fn discard_artifact_from_root_with_gate<F>(
    root: &Path,
    locator: &str,
    expected_sha256: &str,
    mut authorize: F,
) -> Result<(), String>
where
    F: FnMut(&PublicationBatchReceipt) -> Result<Instant, String>,
{
    // Validate caller authority even when the target is already absent. A
    // malformed digest must never turn into a successful destructive call.
    locator_id(locator)?;
    if !exact_lower_sha256(expected_sha256) {
        return Err("Network Reporter discard digest must be one lowercase SHA-256".to_string());
    }
    let Some(pending_root) = HeldDirectory::open_absolute(root)? else {
        return Ok(());
    };
    let _lock = pending_root.try_lock_file(
        std::ffi::OsStr::new(PENDING_UPLOAD_LOCK_FILE),
        "Network Reporter pending-upload lock",
    )?;
    sweep_abandoned_pending_upload_locked(&pending_root, &_lock)?;
    let Some(held) = checked_entry_in_root(&pending_root, locator)? else {
        if let Some(receipt) = committed_publication_for_locator(&pending_root, locator)? {
            let output = receipt
                .outputs
                .iter()
                .find(|output| output.locator == locator)
                .expect("matched publication contains the locator");
            authorize(&receipt)?;
            if output.sha256 != expected_sha256 {
                return Err(
                    "Network Reporter discard digest does not match the committed publication"
                        .to_string(),
                );
            }
        }
        publication::retire_for_absent_locator(&pending_root, locator)?;
        return Ok(());
    };
    let names = exact_entry_names(&held.entry)?;
    let has_receipt = names.iter().any(|name| name == RECEIPT_FILE);
    let has_artifact = names.iter().any(|name| name == ARTIFACT_FILE);
    let has_discarding = names.iter().any(|name| name == DISCARDING_FILE);

    if !has_receipt {
        return Err(
            "Network Reporter artifact entry has no receipt-derived read authority".to_string(),
        );
    }
    if has_artifact && has_discarding {
        return Err("Network Reporter artifact entry has conflicting discard state".to_string());
    }

    let receipt = read_receipt(&held.entry, locator)?;
    let committed = publication::require_committed_output(
        &held.root,
        &receipt.batch_id,
        locator,
        &receipt.output_id,
        &receipt.sha256,
        receipt.size_bytes,
    )?;
    authorize(&committed)?;
    if receipt.sha256 != expected_sha256 {
        return Err("Network Reporter discard digest does not match the receipt".to_string());
    }

    if has_artifact {
        authorize(&committed)?;
        held.entry.rename_file(
            std::ffi::OsStr::new(ARTIFACT_FILE),
            std::ffi::OsStr::new(DISCARDING_FILE),
            "Network Reporter artifact",
        )?;
    }

    if has_artifact || has_discarding {
        let verification = (|| {
            let mut discarding = held.entry.open_regular_file(
                std::ffi::OsStr::new(DISCARDING_FILE),
                "Network Reporter discarding artifact",
            )?;
            verify_open_artifact(&mut discarding, receipt.size_bytes, &receipt.sha256, false)
        })();
        if let Err(error) = verification {
            // A failed verification must leave the durable artifact readable
            // under its receipt-bound name. If this is a retry after a crash,
            // restoring the fixed name also repairs that partial state.
            restore_discarding_artifact(&held.entry)?;
            return Err(error);
        }
        if let Err(error) = authorize(&committed) {
            restore_discarding_artifact(&held.entry)?;
            return Err(error);
        }
        held.entry.remove_file_idempotent(
            std::ffi::OsStr::new(DISCARDING_FILE),
            "Network Reporter artifact",
        )?;
    }

    held.entry.remove_file_idempotent(
        std::ffi::OsStr::new(RECEIPT_FILE),
        "Network Reporter artifact receipt",
    )?;
    let pending_root = held.root.clone();
    let batch_id = receipt.batch_id;
    held.root.remove_child_tree(held.entry)?;
    publication::retire_if_all_outputs_absent(&pending_root, &batch_id)?;
    Ok(())
}

/// Native receipt authority is independent of browser publication storage.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactReadAuthority {
    pub owner_uid: String,
    pub project_id: String,
    pub operation: String,
    pub variant: String,
    pub output_id: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub content_type: String,
}
impl ArtifactReadAuthority {
    pub fn authorize(
        &self,
        receipt: &PublicationBatchReceipt,
        locator: &str,
    ) -> Result<Instant, String> {
        if self.owner_uid != receipt.owner_uid
            || self.project_id != receipt.project_id
            || self.operation != receipt.operation
            || self.variant != receipt.variant
            || !receipt.outputs.iter().any(|output| {
                output.locator == locator
                    && output.output_id == self.output_id
                    && output.sha256 == self.sha256
                    && output.size_bytes == self.size_bytes
                    && output.content_type == self.content_type
            })
        {
            return Err("Network Reporter artifact receipt does not match its requested owner/project/output".into());
        }
        Ok(Instant::now() + Duration::from_secs(60))
    }
}

pub fn random_hex_256() -> String {
    format!(
        "{:016x}{:016x}{:016x}{:016x}",
        rand::random::<u64>(),
        rand::random::<u64>(),
        rand::random::<u64>(),
        rand::random::<u64>()
    )
}

/// Reopen a receipt-declared output through the one held-file authority.
pub fn open_batch_output(
    root: &Path,
    batch: &LocalPublicationBatch,
    output_id: &str,
) -> Result<SealedArtifactHandle, String> {
    let output = batch
        .outputs
        .iter()
        .find(|output| output.output_id == output_id)
        .ok_or("that output is not declared by this publication")?;
    let authority = ArtifactReadAuthority {
        owner_uid: batch.owner_uid.clone(),
        project_id: batch.project_id.clone(),
        operation: batch.operation.clone(),
        variant: batch.variant.clone(),
        output_id: output.output_id.clone(),
        sha256: output.sha256.clone(),
        size_bytes: output.size_bytes,
        content_type: output.content_type.clone(),
    };
    let opened = open_artifact_from_root_with_gate(root, &output.locator, |receipt| {
        authority.authorize(receipt, &output.locator)
    })?;
    Ok(SealedArtifactHandle {
        file: opened.file,
        size_bytes: opened.receipt.size_bytes,
        sha256: opened.receipt.sha256,
    })
}

#[cfg(test)]
mod tests;
