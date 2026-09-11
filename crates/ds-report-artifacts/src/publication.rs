//! Crash-durable logical commit for one Network Reporter output vector.
//!
//! Artifact directories are prepared independently because each output has a
//! separate opaque locator.  Making three directory renames physically atomic
//! is not possible on the supported filesystems.  Instead, every artifact
//! receipt names one batch and readers accept it only while the corresponding
//! batch receipt is present below `publication-batches/`.  The batch becomes
//! committed through one no-replace directory rename after every artifact is
//! durable.  A process loss before that rename leaves no readable output; a
//! process loss after it leaves all provenance needed to recreate the JS
//! outbox row idempotently.

use super::confined_fs::HeldDirectory;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;

pub const BATCH_DIRECTORY: &str = "publication-batches";
pub const BATCH_RECEIPT_FILE: &str = "publication-receipt.json";
pub const BATCH_SCHEMA: u32 = 3;
/// Durable local export, deliberately ineligible for cloud publication.
pub use ds_command_kernel::report::{DEVELOPMENT_ENGINE, LOCAL_ONLY_SCHEMA};
pub const MAX_BATCH_RECEIPT_BYTES: u64 = 64 * 1024;
const STAGING_PREFIX: &str = ".staging-";
const LOCATOR_PREFIX: &str = "ds-reporter:v1:";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationOutput {
    pub output_id: String,
    pub locator: String,
    pub filename: String,
    pub format: String,
    pub content_type: String,
    pub sha256: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<ds_command_kernel::printing::ArtifactPresentation>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationBatchReceipt {
    pub schema: u32,
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
    #[serde(default)]
    pub engine_build_manifest_sha256: String,
    pub transformer: String,
    pub transformer_revision: i64,
    pub input_base_fingerprint: String,
    pub room_content_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,
    pub snapshot_base: BTreeMap<String, String>,
    pub client_publish_id: String,
    pub outputs: Vec<PublicationOutput>,
}

fn exact_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn exact_edge_id(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn exact_client_publish_id(value: &str) -> bool {
    let Some(uuid) = value.strip_prefix("publish-") else {
        return false;
    };
    uuid.len() == 36
        && uuid.as_bytes().get(8) == Some(&b'-')
        && uuid.as_bytes().get(13) == Some(&b'-')
        && uuid.as_bytes().get(18) == Some(&b'-')
        && uuid.as_bytes().get(23) == Some(&b'-')
        && uuid.as_bytes().get(14) == Some(&b'4')
        && uuid
            .as_bytes()
            .get(19)
            .is_some_and(|value| matches!(value, b'8' | b'9' | b'a' | b'b'))
        && uuid.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23)
                || byte.is_ascii_digit()
                || (b'a'..=b'f').contains(&byte)
        })
}

fn validate_output(output: &PublicationOutput) -> Result<(), String> {
    let locator_id = output
        .locator
        .strip_prefix(LOCATOR_PREFIX)
        .ok_or_else(|| "Network Reporter publication output has an invalid locator".to_string())?;
    let valid_paper = ds_command_kernel::printing::validate_artifact_presentation(
        &output.format,
        output.paper_size.as_deref(),
        output.presentation.as_ref(),
    )
    .is_ok();
    if !exact_edge_id(&output.output_id, 128)
        || !exact_lower_hex(locator_id, 64)
        // Named PDFs carry `pdf__` plus a governed print-layout id. Layout ids
        // are bounded to 80 bytes by ds-command-kernel, so the publication
        // envelope must preserve that identity instead of applying the old
        // short static-format bound.
        || !exact_edge_id(&output.format, 85)
        || output.filename.is_empty()
        || output.filename.len() > 255
        || output.filename == "."
        || output.filename == ".."
        || output.filename.contains(['/', '\\'])
        || output.content_type.is_empty()
        || output.content_type.len() > 160
        || output
            .content_type
            .bytes()
            .any(|byte| byte.is_ascii_control())
        || !exact_lower_hex(&output.sha256, 64)
        || output.size_bytes == 0
        // A streamed transfer never buffers the artifact, so the former 64 MiB
        // ceiling was a preview bound wearing a publication bound's clothes.
        || output.size_bytes > super::MAX_TRANSFERABLE_ARTIFACT_BYTES
        || !valid_paper
    {
        return Err("Network Reporter publication output is invalid".to_string());
    }
    Ok(())
}

pub fn validate_receipt(receipt: &PublicationBatchReceipt) -> Result<(), String> {
    let valid_admission_shape = match receipt.schema {
        // Schema 1 is retained so an upgrade can recover artifacts created by
        // the old pre-compute admission model.
        1 => {
            receipt.grant_id.as_deref().is_some_and(|grant| {
                grant.starts_with("grant_")
                    && exact_lower_hex(grant.strip_prefix("grant_").unwrap_or_default(), 40)
            }) && receipt.snapshot_base.len() == 1
                && receipt.snapshot_base.get(&receipt.operation)
                    == Some(&receipt.input_base_fingerprint)
        }
        // Schema 2 is the pre-build-manifest local computation receipt.
        2 => {
            receipt.grant_id.is_none()
                && receipt.snapshot_base.is_empty()
                && receipt.engine_build_manifest_sha256.is_empty()
        }
        // Schema 3 is a local computation receipt. Cloud authority is attached
        // to the IndexedDB outbox only when its sync drain opens fresh work.
        BATCH_SCHEMA | LOCAL_ONLY_SCHEMA => {
            receipt.grant_id.is_none()
                && receipt.snapshot_base.is_empty()
                && exact_lower_hex(&receipt.engine_build_manifest_sha256, 64)
        }
        _ => false,
    };
    if !valid_admission_shape
        || !exact_lower_hex(&receipt.batch_id, 64)
        || !exact_edge_id(&receipt.owner_uid, 128)
        || receipt
            .organization_id
            .as_deref()
            .is_some_and(|value| !exact_edge_id(value, 128))
        || !exact_edge_id(&receipt.project_id, 128)
        || receipt.root != format!("eds_project/{}/eds_design", receipt.project_id)
        || !receipt.client_run_id.starts_with("report-")
        || !exact_edge_id(&receipt.client_run_id, 128)
        || receipt.engine != "network_reporter"
        || !receipt.operation.starts_with("export-")
        || !exact_edge_id(&receipt.operation, 128)
        || receipt.variant != "default"
        || !(if receipt.schema == LOCAL_ONLY_SCHEMA {
            receipt.engine_version == DEVELOPMENT_ENGINE
        } else {
            receipt
                .engine_version
                .strip_prefix("ds-network-reporter@0.1.0+")
                .is_some_and(|source| exact_lower_hex(source, 40))
        })
        || !exact_edge_id(&receipt.transformer, 128)
        || receipt.operation != format!("export-{}", receipt.transformer)
        || receipt.transformer_revision < 0
        || !exact_lower_hex(&receipt.input_base_fingerprint, 64)
        || !exact_lower_hex(&receipt.room_content_sha256, 64)
        || !exact_client_publish_id(&receipt.client_publish_id)
        || receipt.outputs.is_empty()
        || receipt.outputs.len() > 5
    {
        return Err("Network Reporter publication receipt is invalid".to_string());
    }
    let mut output_ids = HashSet::with_capacity(receipt.outputs.len());
    let mut locators = HashSet::with_capacity(receipt.outputs.len());
    for output in &receipt.outputs {
        validate_output(output)?;
        if !output_ids.insert(output.output_id.as_str())
            || !locators.insert(output.locator.as_str())
        {
            return Err(
                "Network Reporter publication receipt contains duplicate outputs".to_string(),
            );
        }
    }
    Ok(())
}

fn encode_receipt(receipt: &PublicationBatchReceipt) -> Result<Vec<u8>, String> {
    validate_receipt(receipt)?;
    let mut bytes = serde_json::to_vec_pretty(receipt)
        .map_err(|error| format!("could not encode Network Reporter publication: {error}"))?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_BATCH_RECEIPT_BYTES {
        return Err("Network Reporter publication receipt is too large".to_string());
    }
    Ok(bytes)
}

fn batch_root(pending_root: &HeldDirectory) -> Result<HeldDirectory, String> {
    pending_root.ensure_child(OsStr::new(BATCH_DIRECTORY))
}

#[derive(Debug)]
pub struct PendingPublicationCommit {
    root: HeldDirectory,
    staging: Option<HeldDirectory>,
    committed: Option<HeldDirectory>,
    receipt: PublicationBatchReceipt,
    batch_id: String,
    armed: bool,
}

impl PendingPublicationCommit {
    pub fn stage(
        pending_root: &HeldDirectory,
        receipt: &PublicationBatchReceipt,
    ) -> Result<Self, String> {
        let bytes = encode_receipt(receipt)?;
        let root = batch_root(pending_root)?;
        if root.open_child(OsStr::new(&receipt.batch_id))?.is_some() {
            return Err("Network Reporter publication batch already exists".to_string());
        }
        let staging_name = format!("{STAGING_PREFIX}{}", receipt.batch_id);
        let staging = root.create_child(OsStr::new(&staging_name))?;
        let result = staging
            .write_new_bounded(
                OsStr::new(BATCH_RECEIPT_FILE),
                &bytes,
                MAX_BATCH_RECEIPT_BYTES,
                "Network Reporter publication receipt",
            )
            .and_then(|_| staging.sync())
            .and_then(|_| root.sync());
        if let Err(error) = result {
            let _ = root.remove_child_tree(staging);
            return Err(error);
        }
        Ok(Self {
            root,
            staging: Some(staging),
            committed: None,
            receipt: receipt.clone(),
            batch_id: receipt.batch_id.clone(),
            armed: true,
        })
    }

    /// One no-replace rename is the logical commit point for the full output
    /// vector. Artifact readers must reject receipts whose batch is not yet
    /// visible here.
    pub fn activate(&mut self) -> Result<&PublicationBatchReceipt, String> {
        if self.committed.is_some() {
            return Ok(&self.receipt);
        }
        let staging = self.staging.take().ok_or_else(|| {
            "Network Reporter publication lost its staged commit record".to_string()
        })?;
        let committed = match self.root.rename_child(staging, OsStr::new(&self.batch_id)) {
            Ok(committed) => committed,
            Err(error) => {
                // `rename_child` verifies the new handle after the kernel
                // rename. If that post-rename verification fails, retain any
                // exact committed child we can still open so Drop rolls it
                // back instead of stranding an authority marker.
                if let Ok(Some(committed)) = self.root.open_child(OsStr::new(&self.batch_id)) {
                    self.committed = Some(committed);
                } else {
                    let staging_name = format!("{STAGING_PREFIX}{}", self.batch_id);
                    if let Ok(Some(staging)) = self.root.open_child(OsStr::new(&staging_name)) {
                        self.staging = Some(staging);
                    }
                }
                return Err(error);
            }
        };
        if let Err(error) = committed.sync().and_then(|_| self.root.sync()) {
            self.committed = Some(committed);
            return Err(error);
        }
        let receipt = match read_receipt(&committed, &self.batch_id) {
            Ok(receipt) => receipt,
            Err(error) => {
                self.committed = Some(committed);
                return Err(error);
            }
        };
        if receipt != self.receipt {
            self.committed = Some(committed);
            return Err("Network Reporter publication changed during activation".to_string());
        }
        self.committed = Some(committed);
        Ok(&self.receipt)
    }

    pub fn commit(mut self) -> Result<PublicationBatchReceipt, String> {
        self.activate()?;
        self.armed = false;
        Ok(self.receipt.clone())
    }
}

impl Drop for PendingPublicationCommit {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let directory = self.committed.take().or_else(|| self.staging.take());
        if let Some(directory) = directory {
            if let Err(error) = self.root.remove_child_tree(directory) {
                eprintln!("failed to roll back Network Reporter publication intent: {error}");
            }
        }
    }
}

fn read_receipt(
    batch_directory: &HeldDirectory,
    expected_batch_id: &str,
) -> Result<PublicationBatchReceipt, String> {
    let bytes = batch_directory.read_bounded(
        OsStr::new(BATCH_RECEIPT_FILE),
        MAX_BATCH_RECEIPT_BYTES,
        "Network Reporter publication receipt",
    )?;
    let receipt: PublicationBatchReceipt = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Network Reporter publication receipt is malformed: {error}"))?;
    validate_receipt(&receipt)?;
    if receipt.batch_id != expected_batch_id {
        return Err("Network Reporter publication receipt changed batch identity".to_string());
    }
    let names = batch_directory.names()?;
    if names.len() != 1 || names[0] != OsStr::new(BATCH_RECEIPT_FILE) {
        return Err("Network Reporter publication directory has an invalid inventory".to_string());
    }
    Ok(receipt)
}

pub fn committed_receipt(
    pending_root: &HeldDirectory,
    batch_id: &str,
) -> Result<Option<PublicationBatchReceipt>, String> {
    if !exact_lower_hex(batch_id, 64) {
        return Err("Network Reporter publication batch identity is invalid".to_string());
    }
    let Some(root) = pending_root.open_child(OsStr::new(BATCH_DIRECTORY))? else {
        return Ok(None);
    };
    let Some(batch) = root.open_child(OsStr::new(batch_id))? else {
        return Ok(None);
    };
    read_receipt(&batch, batch_id).map(Some)
}

pub fn require_committed_output(
    pending_root: &HeldDirectory,
    batch_id: &str,
    locator: &str,
    output_id: &str,
    sha256: &str,
    size_bytes: u64,
) -> Result<PublicationBatchReceipt, String> {
    let receipt = committed_receipt(pending_root, batch_id)?.ok_or_else(|| {
        "Network Reporter artifact belongs to an uncommitted publication".to_string()
    })?;
    let matching = receipt.outputs.iter().filter(|output| {
        output.locator == locator
            && output.output_id == output_id
            && output.sha256 == sha256
            && output.size_bytes == size_bytes
    });
    if matching.count() != 1 {
        return Err(
            "Network Reporter artifact does not match its committed publication".to_string(),
        );
    }
    Ok(receipt)
}

pub fn list_committed(
    pending_root: &HeldDirectory,
) -> Result<Vec<PublicationBatchReceipt>, String> {
    list_committed_from_root(pending_root, false)
}

/// Remove only exact, never-activated batch-intent directories while the
/// caller holds the cross-lane pending-upload lock, then return every durable
/// commit marker. Marker age is deliberately irrelevant.
pub fn sweep_staged_and_list_committed(
    pending_root: &HeldDirectory,
) -> Result<Vec<PublicationBatchReceipt>, String> {
    list_committed_from_root(pending_root, true)
}

fn list_committed_from_root(
    pending_root: &HeldDirectory,
    remove_staged: bool,
) -> Result<Vec<PublicationBatchReceipt>, String> {
    let Some(root) = pending_root.open_child(OsStr::new(BATCH_DIRECTORY))? else {
        return Ok(Vec::new());
    };
    let mut receipts = Vec::new();
    let mut removed = false;
    for name in root.names()? {
        let name = name.into_string().map_err(|_| {
            "Network Reporter publication root contains a non-Unicode member".to_string()
        })?;
        if name.starts_with(STAGING_PREFIX) {
            let id = name.strip_prefix(STAGING_PREFIX).unwrap_or_default();
            if !exact_lower_hex(id, 64) {
                return Err(
                    "Network Reporter publication root contains an invalid staged member"
                        .to_string(),
                );
            }
            if remove_staged {
                let staging = root.open_child(OsStr::new(&name))?.ok_or_else(|| {
                    "Network Reporter staged publication changed during recovery".to_string()
                })?;
                root.remove_child_tree(staging)?;
                removed = true;
            }
            continue;
        }
        if !exact_lower_hex(&name, 64) {
            return Err("Network Reporter publication root contains an unowned member".to_string());
        }
        let batch = root.open_child(OsStr::new(&name))?.ok_or_else(|| {
            "Network Reporter publication changed while it was enumerated".to_string()
        })?;
        receipts.push(read_receipt(&batch, &name)?);
    }
    if removed {
        root.sync()?;
        pending_root.sync()?;
    }
    receipts.sort_by(|left, right| left.client_publish_id.cmp(&right.client_publish_id));
    Ok(receipts)
}

pub fn retire_if_all_outputs_absent(
    pending_root: &HeldDirectory,
    batch_id: &str,
) -> Result<bool, String> {
    let Some(receipt) = committed_receipt(pending_root, batch_id)? else {
        return Ok(true);
    };
    for output in &receipt.outputs {
        let locator_id = output.locator.strip_prefix(LOCATOR_PREFIX).ok_or_else(|| {
            "Network Reporter publication output has an invalid locator".to_string()
        })?;
        if pending_root.open_child(OsStr::new(locator_id))?.is_some() {
            return Ok(false);
        }
    }
    let root = pending_root
        .open_child(OsStr::new(BATCH_DIRECTORY))?
        .ok_or_else(|| "Network Reporter publication root disappeared".to_string())?;
    let batch = root
        .open_child(OsStr::new(batch_id))?
        .ok_or_else(|| "Network Reporter publication changed before retirement".to_string())?;
    if read_receipt(&batch, batch_id)? != receipt {
        return Err("Network Reporter publication changed before retirement".to_string());
    }
    root.remove_child_tree(batch)?;
    Ok(true)
}

pub fn retire_for_absent_locator(
    pending_root: &HeldDirectory,
    locator: &str,
) -> Result<(), String> {
    let locator_id = locator.strip_prefix(LOCATOR_PREFIX).ok_or_else(|| {
        "Network Reporter publication locator has an invalid contract".to_string()
    })?;
    if !exact_lower_hex(locator_id, 64) {
        return Err("Network Reporter publication locator is invalid".to_string());
    }
    let matches = list_committed(pending_root)?
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
    if let Some(receipt) = matches.first() {
        retire_if_all_outputs_absent(pending_root, &receipt.batch_id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "network-reporter-publication-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn receipt() -> PublicationBatchReceipt {
        let operation = "export-tx_1".to_string();
        PublicationBatchReceipt {
            schema: BATCH_SCHEMA,
            batch_id: "a".repeat(64),
            owner_uid: "owner-1".to_string(),
            organization_id: Some("org-1".to_string()),
            project_id: "project-1".to_string(),
            root: "eds_project/project-1/eds_design".to_string(),
            client_run_id: "report-0123456789abcdef0123456789abcdef".to_string(),
            engine: "network_reporter".to_string(),
            operation: operation.clone(),
            variant: "default".to_string(),
            engine_version: format!("ds-network-reporter@0.1.0+{}", "b".repeat(40)),
            engine_build_manifest_sha256: "d".repeat(64),
            transformer: "tx_1".to_string(),
            transformer_revision: 7,
            input_base_fingerprint: "c".repeat(64),
            room_content_sha256: "1".repeat(64),
            grant_id: None,
            snapshot_base: BTreeMap::new(),
            client_publish_id: "publish-123e4567-e89b-42d3-a456-426614174000".to_string(),
            outputs: vec![PublicationOutput {
                output_id: "shp".to_string(),
                locator: format!("{LOCATOR_PREFIX}{}", "e".repeat(64)),
                filename: "tx_1.shp.zip".to_string(),
                format: "shp".to_string(),
                content_type: "application/zip".to_string(),
                sha256: "f".repeat(64),
                size_bytes: 42,
                paper_size: None,
                presentation: None,
            }],
        }
    }

    #[test]
    fn development_exports_require_a_local_only_receipt() {
        let mut local = receipt();
        local.engine_version = DEVELOPMENT_ENGINE.to_string();
        assert!(validate_receipt(&local).is_err());
        local.schema = LOCAL_ONLY_SCHEMA;
        assert!(validate_receipt(&local).is_ok());
        local.engine_build_manifest_sha256.clear();
        assert!(validate_receipt(&local).is_err());
        local.engine_build_manifest_sha256 = "d".repeat(64);
        local.grant_id = Some(format!("grant_{}", "a".repeat(40)));
        assert!(validate_receipt(&local).is_err());
    }

    #[test]
    fn print_output_accepts_only_matching_sealed_paper_metadata() {
        let mut receipt = receipt();
        let output = &mut receipt.outputs[0];
        output.output_id = "pdf__a3-project".to_string();
        output.format = "pdf__a3-project".to_string();
        output.filename = "tx_1.a3-project.pdf".to_string();
        output.content_type = "application/pdf".to_string();
        output.paper_size = Some("A3".to_string());
        assert!(validate_receipt(&receipt).is_ok());

        receipt.outputs[0].paper_size = Some("Letter".to_string());
        assert!(validate_receipt(&receipt).is_ok());
        receipt.outputs[0].presentation = Some(ds_command_kernel::printing::ArtifactPresentation {
            layout_id: "a3-project".into(),
            layout_name: "Pole details".into(),
            paper_name: "Letter".into(),
            orientation: ds_command_kernel::printing::Orientation::Portrait,
            page_mm: [215.9, 279.4],
        });
        assert!(validate_receipt(&receipt).is_ok());
        let decoded: PublicationBatchReceipt =
            serde_json::from_slice(&serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert_eq!(decoded, receipt);
        receipt.outputs[0]
            .presentation
            .as_mut()
            .unwrap()
            .page_mm
            .reverse();
        assert!(validate_receipt(&receipt).is_err());
        receipt.outputs[0].presentation = None;
        receipt.outputs[0].paper_size = Some("bad\nname".to_string());
        assert!(validate_receipt(&receipt).is_err());
    }

    #[test]
    fn staged_vector_is_not_visible_until_single_commit_rename() {
        let temp = TempRoot::new();
        let pending = HeldDirectory::ensure_absolute(&temp.0).unwrap();
        let expected = receipt();
        let staged = PendingPublicationCommit::stage(&pending, &expected).unwrap();
        assert!(list_committed(&pending).unwrap().is_empty());
        assert!(committed_receipt(&pending, &expected.batch_id)
            .unwrap()
            .is_none());

        assert_eq!(staged.commit().unwrap(), expected);
        assert_eq!(list_committed(&pending).unwrap(), vec![expected.clone()]);
        assert_eq!(
            committed_receipt(&pending, &expected.batch_id).unwrap(),
            Some(expected)
        );
    }

    #[test]
    fn dropped_intent_removes_only_its_owned_staging_directory() {
        let temp = TempRoot::new();
        let pending = HeldDirectory::ensure_absolute(&temp.0).unwrap();
        let staged = PendingPublicationCommit::stage(&pending, &receipt()).unwrap();
        let staging_path = staged.staging.as_ref().unwrap().path().to_path_buf();
        drop(staged);
        assert!(!staging_path.exists());
        assert!(list_committed(&pending).unwrap().is_empty());
    }

    #[test]
    fn activated_but_unaccepted_commit_rolls_back_its_marker() {
        let temp = TempRoot::new();
        let pending = HeldDirectory::ensure_absolute(&temp.0).unwrap();
        let expected = receipt();
        let mut staged = PendingPublicationCommit::stage(&pending, &expected).unwrap();
        staged.activate().unwrap();
        assert_eq!(list_committed(&pending).unwrap(), vec![expected]);
        drop(staged);
        assert!(list_committed(&pending).unwrap().is_empty());
    }

    #[test]
    fn locked_sweep_removes_only_never_activated_batch_intents() {
        let temp = TempRoot::new();
        let pending = HeldDirectory::ensure_absolute(&temp.0).unwrap();
        let expected = receipt();
        PendingPublicationCommit::stage(&pending, &expected)
            .unwrap()
            .commit()
            .unwrap();
        let abandoned_id = "9".repeat(64);
        let abandoned = temp
            .0
            .join(BATCH_DIRECTORY)
            .join(format!("{STAGING_PREFIX}{abandoned_id}"));
        std::fs::create_dir(&abandoned).unwrap();
        std::fs::write(abandoned.join("partial.json"), b"partial").unwrap();

        assert_eq!(
            sweep_staged_and_list_committed(&pending).unwrap(),
            vec![expected]
        );
        assert!(!abandoned.exists());
    }

    #[test]
    fn committed_receipt_tampering_fails_closed() {
        let temp = TempRoot::new();
        let pending = HeldDirectory::ensure_absolute(&temp.0).unwrap();
        let expected = receipt();
        PendingPublicationCommit::stage(&pending, &expected)
            .unwrap()
            .commit()
            .unwrap();
        let path = temp
            .0
            .join(BATCH_DIRECTORY)
            .join(&expected.batch_id)
            .join(BATCH_RECEIPT_FILE);
        std::fs::write(path, b"{}\n").unwrap();
        assert!(committed_receipt(&pending, &expected.batch_id).is_err());
    }

    #[test]
    fn duplicate_output_identity_is_rejected() {
        let mut duplicate = receipt();
        duplicate.outputs.push(duplicate.outputs[0].clone());
        assert!(validate_receipt(&duplicate).is_err());
    }

    #[test]
    fn named_print_output_preserves_the_full_governed_layout_id() {
        let mut named = receipt();
        let format = "pdf__a0-landscape-huye-cjic-review";
        named.outputs[0].output_id = format.to_string();
        named.outputs[0].format = format.to_string();
        named.outputs[0].filename = "tx_1.a0-landscape-huye-cjic-review.pdf".to_string();
        named.outputs[0].content_type = "application/pdf".to_string();
        assert!(validate_receipt(&named).is_ok());
    }

    #[test]
    fn committed_output_binds_locator_output_digest_and_size() {
        let temp = TempRoot::new();
        let pending = HeldDirectory::ensure_absolute(&temp.0).unwrap();
        let expected = receipt();
        PendingPublicationCommit::stage(&pending, &expected)
            .unwrap()
            .commit()
            .unwrap();
        let output = &expected.outputs[0];
        assert_eq!(
            require_committed_output(
                &pending,
                &expected.batch_id,
                &output.locator,
                &output.output_id,
                &output.sha256,
                output.size_bytes,
            )
            .unwrap(),
            expected
        );
        assert!(require_committed_output(
            &pending,
            &"a".repeat(64),
            &output.locator,
            "xlsx",
            &output.sha256,
            output.size_bytes,
        )
        .is_err());
    }

    #[test]
    fn batch_receipt_retires_only_after_its_full_vector_is_absent() {
        let temp = TempRoot::new();
        let pending = HeldDirectory::ensure_absolute(&temp.0).unwrap();
        let expected = receipt();
        PendingPublicationCommit::stage(&pending, &expected)
            .unwrap()
            .commit()
            .unwrap();
        let output_id = expected.outputs[0]
            .locator
            .strip_prefix(LOCATOR_PREFIX)
            .unwrap();
        let output = pending.create_child(OsStr::new(output_id)).unwrap();
        assert!(!retire_if_all_outputs_absent(&pending, &expected.batch_id).unwrap());
        pending.remove_child_tree(output).unwrap();
        retire_for_absent_locator(&pending, &expected.outputs[0].locator).unwrap();
        assert!(committed_receipt(&pending, &expected.batch_id)
            .unwrap()
            .is_none());
    }
}
