//! Closed project snapshot, differential acquisition and local-view contracts.
//!
//! These types carry source authority, not credentials or arbitrary URLs. A
//! structurally complete receipt permits local planning; only the authenticated
//! publication owner can attest it and commit a centrally distributed archive.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "ds.project-snapshot/v1";
pub const MAX_LAYERS: usize = 10_000;
pub const MAX_CHUNKS: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    Stable,
    Canary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Owner {
    Project {
        project_id: String,
    },
    Personal {
        project_id: String,
        owner_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub principal: String,
    pub lane: Lane,
    pub audience: String,
    pub owner: Owner,
    /// Opaque server-issued visibility identity, including restricted scopes.
    pub visibility: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Survey,
    SavedDesign,
    GeographicUpload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadSource {
    BigquerySurveyMirror,
    SavedFirestoreAndDsgrid,
    OriginalAsset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SelectionScope {
    ProjectComplete,
    PrincipalComplete,
    Filtered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub epoch: String,
    pub vector: BTreeMap<String, String>,
    pub coverage: String,
    pub change_cursor: String,
    pub schema_sha256: String,
    pub vocabulary_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WatermarkKind {
    Authoritative,
    BigqueryMirror,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionReceipt {
    pub attestation_id: String,
    pub revision: Revision,
    pub expected_layers: BTreeSet<String>,
    pub observed_layers: BTreeSet<String>,
    pub expected_pages: u64,
    pub completed_pages: u64,
    pub terminal: bool,
    pub whole_authorized_scope: bool,
    pub watermark_kind: WatermarkKind,
    pub watermark: String,
    /// Explicitly distinguishes a complete mirror revision from current source.
    pub mirror_behind: bool,
    pub deletion_history_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Chunk {
    pub sha256: String,
    pub bytes: u64,
    pub rows: u64,
    /// Opaque local catalogue identity; never a path or download authority.
    pub artifact: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GeometryKind {
    Point,
    MultiPoint,
    LineString,
    MultiLineString,
    Polygon,
    MultiPolygon,
    Mixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub name: String,
    pub geometry: GeometryKind,
    pub crs: String,
    pub schema_sha256: String,
    pub stable_id_column: String,
    pub revision_column: String,
    pub row_count: u64,
    pub byte_count: u64,
    pub chunks: Vec<Chunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedDesign {
    pub saved_models: BTreeMap<String, String>,
    pub transformer_revisions: BTreeMap<String, String>,
    pub tag_revision: String,
    pub unsaved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OriginalAsset {
    pub asset_id: String,
    pub generation: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub context: Context,
    pub source_kind: SourceKind,
    pub read_source: ReadSource,
    pub selection: SelectionScope,
    /// A captured acquisition filter, not the mutable local work-area mask.
    pub acquisition_filter: Option<String>,
    pub revision: Revision,
    pub layers: Vec<Layer>,
    pub acquisition: AcquisitionReceipt,
    pub saved_design: Option<SavedDesign>,
    pub original_asset: Option<OriginalAsset>,
    /// Digest over logical live rows, independent of physical chunk layout.
    pub logical_data_sha256: String,
    pub manifest_sha256: String,
    /// Observation time is not a source revision or freshness proof.
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCode {
    SnapshotInvalid,
    SnapshotSourcePolicy,
    SnapshotContextMismatch,
    SnapshotIncomplete,
    SnapshotMovingHead,
    SnapshotCorrupt,
    SnapshotResnapshotRequired,
    SnapshotStaleDelta,
    SnapshotBudgetExceeded,
    SnapshotAccessRevoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    pub code: RefusalCode,
    pub reason: String,
}
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.reason)
    }
}
impl std::error::Error for Refusal {}
fn refuse(code: RefusalCode, reason: impl Into<String>) -> Refusal {
    Refusal {
        code,
        reason: reason.into(),
    }
}
pub fn identifier(s: &str) -> bool {
    !s.is_empty() && s.len() <= 1024 && s.trim() == s && !s.chars().any(char::is_control)
}
pub fn sha256(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Context {
    pub fn validate(&self) -> Result<(), Refusal> {
        let (project, personal) = match &self.owner {
            Owner::Project { project_id } => (project_id, None),
            Owner::Personal {
                project_id,
                owner_id,
            } => (project_id, Some(owner_id)),
        };
        if [&self.principal, &self.audience, &self.visibility, project]
            .into_iter()
            .any(|s| !identifier(s))
            || personal.is_some_and(|s| !identifier(s) || s != &self.principal)
        {
            return Err(refuse(
                RefusalCode::SnapshotContextMismatch,
                "invalid captured principal/audience/project/owner/visibility",
            ));
        }
        Ok(())
    }
}
impl Revision {
    pub fn validate(&self) -> Result<(), Refusal> {
        if !identifier(&self.epoch)
            || !identifier(&self.coverage)
            || !identifier(&self.change_cursor)
            || !identifier(&self.vocabulary_revision)
            || !sha256(&self.schema_sha256)
            || self.vector.is_empty()
            || self.vector.len() > MAX_LAYERS
            || self
                .vector
                .iter()
                .any(|(key, value)| !identifier(key) || !identifier(value))
        {
            return Err(refuse(
                RefusalCode::SnapshotInvalid,
                "invalid source revision vector",
            ));
        }
        Ok(())
    }
}
impl Manifest {
    /// Canonical identity of all captured fields except the identity itself.
    /// This verifies a claim's consistency, not its server attestation.
    pub fn computed_sha256(&self) -> Result<String, Refusal> {
        use sha2::{Digest, Sha256};
        let mut value = serde_json::to_value(self)
            .map_err(|_| refuse(RefusalCode::SnapshotInvalid, "cannot encode snapshot"))?;
        value["manifest_sha256"] = serde_json::Value::String(String::new());
        value.sort_all_objects();
        let bytes = serde_json::to_vec(&value)
            .map_err(|_| refuse(RefusalCode::SnapshotInvalid, "cannot encode snapshot"))?;
        let mut hash = Sha256::new();
        hash.update(b"ds.project-snapshot-manifest/v1\0");
        hash.update(bytes);
        Ok(format!("{:x}", hash.finalize()))
    }

    pub fn verify_sha256(&self) -> Result<(), Refusal> {
        if self.computed_sha256()? != self.manifest_sha256 {
            return Err(refuse(
                RefusalCode::SnapshotCorrupt,
                "snapshot manifest digest differs",
            ));
        }
        Ok(())
    }
    /// Structural admission only. Authenticity and byte verification belong to
    /// the source transport, local store and independent central verifier.
    pub fn validate(&self, expected: &Context) -> Result<(), Refusal> {
        self.context.validate()?;
        expected.validate()?;
        if &self.context != expected {
            return Err(refuse(
                RefusalCode::SnapshotContextMismatch,
                "captured snapshot context differs",
            ));
        }
        self.revision.validate()?;
        if self.schema != SCHEMA
            || !sha256(&self.logical_data_sha256)
            || !sha256(&self.manifest_sha256)
        {
            return Err(refuse(
                RefusalCode::SnapshotInvalid,
                "invalid snapshot schema/digest",
            ));
        }
        let admitted = matches!(
            (self.source_kind, self.read_source),
            (SourceKind::Survey, ReadSource::BigquerySurveyMirror)
                | (SourceKind::SavedDesign, ReadSource::SavedFirestoreAndDsgrid)
                | (SourceKind::GeographicUpload, ReadSource::OriginalAsset)
        );
        if !admitted {
            return Err(refuse(RefusalCode::SnapshotSourcePolicy, "survey uses governed BigQuery; saved design uses Firestore/DS Grid; uploads use original assets"));
        }
        let receipt = &self.acquisition;
        if self.revision != receipt.revision {
            return Err(refuse(
                RefusalCode::SnapshotMovingHead,
                "acquisition mixes source revisions",
            ));
        }
        if !identifier(&receipt.attestation_id) || !identifier(&receipt.watermark) {
            return Err(refuse(
                RefusalCode::SnapshotInvalid,
                "missing source attestation/watermark",
            ));
        }
        if (self.source_kind == SourceKind::Survey)
            != (receipt.watermark_kind == WatermarkKind::BigqueryMirror)
            || (self.source_kind != SourceKind::Survey && receipt.mirror_behind)
        {
            return Err(refuse(
                RefusalCode::SnapshotSourcePolicy,
                "incorrect source watermark authority",
            ));
        }
        if !receipt.terminal
            || receipt.expected_pages != receipt.completed_pages
            || receipt.expected_layers != receipt.observed_layers
            || !receipt.deletion_history_complete
        {
            return Err(refuse(
                RefusalCode::SnapshotIncomplete,
                "incomplete page/layer/deletion coverage",
            ));
        }
        if self.layers.len() > MAX_LAYERS {
            return Err(refuse(
                RefusalCode::SnapshotBudgetExceeded,
                "too many snapshot layers",
            ));
        }
        let mut names = BTreeSet::new();
        let mut chunk_count = 0usize;
        for layer in &self.layers {
            if !identifier(&layer.name)
                || !names.insert(layer.name.clone())
                || !sha256(&layer.schema_sha256)
                || !identifier(&layer.stable_id_column)
                || !identifier(&layer.revision_column)
                || !matches!(layer.crs.as_str(), "EPSG:4326" | "OGC:CRS84")
            {
                return Err(refuse(
                    RefusalCode::SnapshotInvalid,
                    "invalid or duplicate layer schema/identity/CRS",
                ));
            }
            let mut chunks = BTreeSet::new();
            let mut rows = 0u64;
            let mut bytes = 0u64;
            for chunk in &layer.chunks {
                chunk_count = chunk_count.checked_add(1).ok_or_else(|| {
                    refuse(RefusalCode::SnapshotBudgetExceeded, "chunk count overflow")
                })?;
                if chunk_count > MAX_CHUNKS {
                    return Err(refuse(
                        RefusalCode::SnapshotBudgetExceeded,
                        "too many snapshot chunks",
                    ));
                }
                if !sha256(&chunk.sha256)
                    || !identifier(&chunk.artifact)
                    || !chunks.insert(&chunk.sha256)
                    || chunk.bytes == 0
                    || chunk.rows == 0
                {
                    return Err(refuse(
                        RefusalCode::SnapshotInvalid,
                        "invalid/duplicate chunk identity or counts",
                    ));
                }
                rows = rows
                    .checked_add(chunk.rows)
                    .ok_or_else(|| refuse(RefusalCode::SnapshotInvalid, "row count overflow"))?;
                bytes = bytes
                    .checked_add(chunk.bytes)
                    .ok_or_else(|| refuse(RefusalCode::SnapshotInvalid, "byte count overflow"))?;
            }
            if rows != layer.row_count || bytes != layer.byte_count {
                return Err(refuse(
                    RefusalCode::SnapshotCorrupt,
                    "layer counts differ from immutable chunks",
                ));
            }
        }
        if names != receipt.observed_layers {
            return Err(refuse(
                RefusalCode::SnapshotIncomplete,
                "manifest layer set differs from receipt",
            ));
        }
        match (self.selection, &self.context.owner) {
            (SelectionScope::ProjectComplete, Owner::Project { .. })
            | (SelectionScope::PrincipalComplete, Owner::Personal { .. }) => {
                if self.acquisition_filter.is_some() || !receipt.whole_authorized_scope {
                    return Err(refuse(
                        RefusalCode::SnapshotIncomplete,
                        "complete selection requires unfiltered server-attested whole scope",
                    ));
                }
            }
            (SelectionScope::Filtered, _) => {
                if !self.acquisition_filter.as_deref().is_some_and(identifier) {
                    return Err(refuse(
                        RefusalCode::SnapshotInvalid,
                        "filtered acquisition must name its exact predicate",
                    ));
                }
            }
            _ => {
                return Err(refuse(
                    RefusalCode::SnapshotContextMismatch,
                    "selection and owner scopes differ",
                ))
            }
        }
        if self.source_kind == SourceKind::SavedDesign {
            let saved = self.saved_design.as_ref().ok_or_else(|| {
                refuse(
                    RefusalCode::SnapshotIncomplete,
                    "saved design revision manifest is absent",
                )
            })?;
            if saved.unsaved
                || !identifier(&saved.tag_revision)
                || saved
                    .saved_models
                    .iter()
                    .any(|(id, digest)| !identifier(id) || !sha256(digest))
                || saved
                    .transformer_revisions
                    .iter()
                    .any(|(id, revision)| !identifier(id) || !identifier(revision))
            {
                return Err(refuse(
                    RefusalCode::SnapshotSourcePolicy,
                    "unsaved or invalid design revisions cannot form a published snapshot",
                ));
            }
        } else if self.saved_design.is_some() {
            return Err(refuse(
                RefusalCode::SnapshotSourcePolicy,
                "unexpected saved design identity",
            ));
        }
        if self.source_kind == SourceKind::GeographicUpload {
            let asset = self.original_asset.as_ref().ok_or_else(|| {
                refuse(
                    RefusalCode::SnapshotIncomplete,
                    "original upload identity is absent",
                )
            })?;
            if !identifier(&asset.asset_id)
                || !identifier(&asset.generation)
                || !sha256(&asset.sha256)
                || asset.bytes == 0
            {
                return Err(refuse(
                    RefusalCode::SnapshotInvalid,
                    "invalid original asset identity",
                ));
            }
        } else if self.original_asset.is_some() {
            return Err(refuse(
                RefusalCode::SnapshotSourcePolicy,
                "unexpected original asset identity",
            ));
        }
        Ok(())
    }
    pub fn complete_project_candidate(&self, expected: &Context) -> Result<bool, Refusal> {
        self.validate(expected)?;
        Ok(self.selection == SelectionScope::ProjectComplete && !self.acquisition.mirror_behind)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessState {
    LocallyCached,
    Checking,
    CurrentAtRevision,
    Changed,
    MirrorBehind,
    OfflineUnverified,
    AccessRevoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryReason {
    CursorExpired,
    SourceEpochChanged,
    IncompatibleSchema,
    DeletionHistoryMissing,
    AccessScopeChanged,
    MissingBase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Recovery {
    pub reason: RecoveryReason,
    pub estimated_bytes: u64,
    pub reusable_chunks: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionMetrics {
    pub source_requests: u64,
    pub transferred_bytes: u64,
    pub source_rows: u64,
    pub full_source_scans: u64,
    pub bigquery_scanned_bytes: Option<u64>,
    pub bigquery_billed_bytes: Option<u64>,
    pub firestore_document_reads: Option<u64>,
    pub cloud_compute_invocations: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tombstone {
    pub layer: String,
    pub object_id: String,
    pub object_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeltaPage {
    pub context: Context,
    pub batch_id: String,
    pub base_manifest_sha256: String,
    pub from: Revision,
    pub target: Revision,
    pub page: u64,
    pub next_page: Option<u64>,
    pub changed_chunks: BTreeMap<String, Vec<Chunk>>,
    pub retired_chunks: BTreeMap<String, BTreeSet<String>>,
    pub tombstones: Vec<Tombstone>,
    pub terminal_manifest: Option<Manifest>,
}
impl DeltaPage {
    pub fn validate(&self, base: &Manifest, expected_page: u64) -> Result<(), Refusal> {
        base.validate(&base.context)?;
        if self.context != base.context {
            return Err(refuse(
                RefusalCode::SnapshotContextMismatch,
                "delta changed captured authority",
            ));
        }
        if self.base_manifest_sha256 != base.manifest_sha256
            || self.from != base.revision
            || self.page != expected_page
            || !identifier(&self.batch_id)
        {
            return Err(refuse(
                RefusalCode::SnapshotStaleDelta,
                "delta does not continue the pinned base/page",
            ));
        }
        self.target.validate()?;
        if self.target == self.from {
            return Err(refuse(
                RefusalCode::SnapshotStaleDelta,
                "unchanged revision is a freshness no-op, not a delta",
            ));
        }
        if self.target.epoch != self.from.epoch
            || self.target.schema_sha256 != self.from.schema_sha256
        {
            return Err(refuse(
                RefusalCode::SnapshotResnapshotRequired,
                "source epoch or schema changed",
            ));
        }
        if self
            .next_page
            .is_some_and(|p| self.page.checked_add(1) != Some(p))
            || self.next_page.is_some() == self.terminal_manifest.is_some()
        {
            return Err(refuse(
                RefusalCode::SnapshotIncomplete,
                "delta must continue exactly one page or carry a terminal manifest",
            ));
        }
        if let Some(manifest) = &self.terminal_manifest {
            manifest.validate(&self.context)?;
            if manifest.source_kind != base.source_kind
                || manifest.read_source != base.read_source
                || manifest.selection != base.selection
                || manifest.acquisition_filter != base.acquisition_filter
            {
                return Err(refuse(
                    RefusalCode::SnapshotResnapshotRequired,
                    "delta changed source or acquisition scope",
                ));
            }
            if manifest.revision != self.target {
                return Err(refuse(
                    RefusalCode::SnapshotMovingHead,
                    "terminal receipt differs from pinned delta target",
                ));
            }
        }
        if self.changed_chunks.len() > MAX_LAYERS
            || self.retired_chunks.len() > MAX_LAYERS
            || self.tombstones.len() > MAX_CHUNKS
        {
            return Err(refuse(
                RefusalCode::SnapshotBudgetExceeded,
                "delta exceeds layer/change bounds",
            ));
        }
        let mut count = 0usize;
        for (layer, chunks) in &self.changed_chunks {
            if !identifier(layer) {
                return Err(refuse(
                    RefusalCode::SnapshotInvalid,
                    "invalid changed layer",
                ));
            }
            let mut seen = BTreeSet::new();
            for chunk in chunks {
                count = count.saturating_add(1);
                if count > MAX_CHUNKS {
                    return Err(refuse(
                        RefusalCode::SnapshotBudgetExceeded,
                        "delta exceeds chunk bound",
                    ));
                }
                if !sha256(&chunk.sha256)
                    || !identifier(&chunk.artifact)
                    || chunk.bytes == 0
                    || chunk.rows == 0
                    || !seen.insert(&chunk.sha256)
                {
                    return Err(refuse(
                        RefusalCode::SnapshotInvalid,
                        "invalid or repeated changed chunk",
                    ));
                }
            }
        }
        for (layer, chunks) in &self.retired_chunks {
            count = count.saturating_add(chunks.len());
            if count > MAX_CHUNKS {
                return Err(refuse(
                    RefusalCode::SnapshotBudgetExceeded,
                    "delta exceeds total changed/retired chunk bound",
                ));
            }
            if !identifier(layer) || chunks.iter().any(|digest| !sha256(digest)) {
                return Err(refuse(
                    RefusalCode::SnapshotInvalid,
                    "invalid retired chunk identity",
                ));
            }
        }
        let mut deleted = BTreeSet::new();
        for tombstone in &self.tombstones {
            if !identifier(&tombstone.layer)
                || !identifier(&tombstone.object_id)
                || !identifier(&tombstone.object_revision)
                || !deleted.insert((&tombstone.layer, &tombstone.object_id))
            {
                return Err(refuse(
                    RefusalCode::SnapshotInvalid,
                    "invalid or repeated tombstone identity",
                ));
            }
        }
        Ok(())
    }
}

/// A complete ordered delta, validated before its native store changes any
/// head. The store separately verifies row-level deletions and actual bytes.
pub struct DeltaPlan<'a> {
    pub manifest: &'a Manifest,
    pub tombstones: Vec<Tombstone>,
}

pub fn assemble_delta<'a>(
    base: &Manifest,
    pages: &'a [DeltaPage],
) -> Result<DeltaPlan<'a>, Refusal> {
    base.validate(&base.context)?;
    base.verify_sha256()?;
    if pages.is_empty() || pages.len() > MAX_CHUNKS {
        return Err(refuse(
            RefusalCode::SnapshotIncomplete,
            "delta requires bounded ordered pages",
        ));
    }
    let first = &pages[0];
    let mut chunks: BTreeMap<&str, BTreeMap<&str, &Chunk>> = base
        .layers
        .iter()
        .map(|layer| {
            (
                layer.name.as_str(),
                layer
                    .chunks
                    .iter()
                    .map(|chunk| (chunk.sha256.as_str(), chunk))
                    .collect(),
            )
        })
        .collect();
    let mut changed = BTreeSet::new();
    let mut retired = BTreeSet::new();
    let mut deleted = BTreeSet::new();
    let mut tombstones = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        page.validate(base, index as u64)?;
        if page.batch_id != first.batch_id || page.target != first.target {
            return Err(refuse(
                RefusalCode::SnapshotMovingHead,
                "delta pages changed batch or target",
            ));
        }
        if page.terminal_manifest.is_some() != (index + 1 == pages.len()) {
            return Err(refuse(
                RefusalCode::SnapshotIncomplete,
                "delta terminal page must be last",
            ));
        }
        for (layer, hashes) in &page.retired_chunks {
            let held = chunks.get_mut(layer.as_str()).ok_or_else(|| {
                refuse(
                    RefusalCode::SnapshotResnapshotRequired,
                    "delta changed layer inventory",
                )
            })?;
            for hash in hashes {
                if !retired.insert((layer.as_str(), hash.as_str()))
                    || held.remove(hash.as_str()).is_none()
                {
                    return Err(refuse(
                        RefusalCode::SnapshotStaleDelta,
                        "retired chunk is repeated or absent",
                    ));
                }
            }
        }
        for (layer, additions) in &page.changed_chunks {
            let held = chunks.get_mut(layer.as_str()).ok_or_else(|| {
                refuse(
                    RefusalCode::SnapshotResnapshotRequired,
                    "delta changed layer inventory",
                )
            })?;
            for chunk in additions {
                if !changed.insert((layer.as_str(), chunk.sha256.as_str()))
                    || held.insert(&chunk.sha256, chunk).is_some()
                {
                    return Err(refuse(
                        RefusalCode::SnapshotInvalid,
                        "changed chunk is repeated or already held",
                    ));
                }
            }
        }
        for tombstone in &page.tombstones {
            if !chunks.contains_key(tombstone.layer.as_str())
                || !deleted.insert((tombstone.layer.as_str(), tombstone.object_id.as_str()))
            {
                return Err(refuse(
                    RefusalCode::SnapshotInvalid,
                    "tombstone layer is absent or ID repeats across pages",
                ));
            }
            tombstones.push(tombstone.clone());
        }
        if changed.len() > MAX_CHUNKS || retired.len() > MAX_CHUNKS || deleted.len() > MAX_CHUNKS {
            return Err(refuse(
                RefusalCode::SnapshotBudgetExceeded,
                "assembled delta exceeds change bounds",
            ));
        }
    }
    let manifest = pages
        .last()
        .and_then(|page| page.terminal_manifest.as_ref())
        .ok_or_else(|| {
            refuse(
                RefusalCode::SnapshotIncomplete,
                "delta has no terminal receipt",
            )
        })?;
    manifest.verify_sha256()?;
    if manifest.layers.len() != base.layers.len() {
        return Err(refuse(
            RefusalCode::SnapshotResnapshotRequired,
            "delta changed layer inventory",
        ));
    }
    for layer in &manifest.layers {
        let old = base
            .layers
            .iter()
            .find(|old| old.name == layer.name)
            .ok_or_else(|| {
                refuse(
                    RefusalCode::SnapshotResnapshotRequired,
                    "delta changed layer inventory",
                )
            })?;
        if layer.geometry != old.geometry
            || layer.crs != old.crs
            || layer.schema_sha256 != old.schema_sha256
            || layer.stable_id_column != old.stable_id_column
            || layer.revision_column != old.revision_column
        {
            return Err(refuse(
                RefusalCode::SnapshotResnapshotRequired,
                "delta changed layer schema",
            ));
        }
        let actual: BTreeMap<&str, &Chunk> = layer
            .chunks
            .iter()
            .map(|chunk| (chunk.sha256.as_str(), chunk))
            .collect();
        if chunks.get(layer.name.as_str()) != Some(&actual) {
            return Err(refuse(
                RefusalCode::SnapshotIncomplete,
                "terminal manifest differs from accumulated chunk changes",
            ));
        }
    }
    Ok(DeltaPlan {
        manifest,
        tombstones,
    })
}
