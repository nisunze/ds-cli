//! Fixed selected-project operations behind ds-brain's `/report` action gate.
//!
//! These are the map-independent project reads and governed writes: the
//! registry of compounded archives, the project transformer inventory, the
//! transformer status rows every Design read is built from, and reversible
//! transformer retirement. ds-brain computes no report here: individual,
//! combined and compounded reports are composed on an edge and published
//! through the compute-artifact door. None of them consumes a Desktop room, a
//! browser store, or a visible map project; the selected
//! project is injected by the host from its audience-fenced context and the
//! core owns every request key, action spelling, bound, and deadline.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::{ClientError, ErrorKind, TransportResponse};
use ds_command_kernel::report::archive_groups::{self, ArchiveGroupingFields};
pub use ds_command_kernel::report::archive_groups::{
    ArchiveGroupingKind, ArchiveGroupingPlan, ArchiveGroups,
};

/// The most transformers one request may name. ds-brain's own compounded and
/// retirement paths accept no more, so a larger list is refused locally.
pub const PROJECT_REPORT_MAX_TRANSFORMERS: usize =
    ds_command_kernel::report::transformers::MAX_TRANSFORMERS;
/// One transformer reference name never exceeds this many characters.
pub const PROJECT_REPORT_MAX_TRANSFORMER_CHARS: usize =
    ds_command_kernel::report::transformers::MAX_NAME_CHARS;
/// A retirement reason is bounded operator prose, never a document.
pub const PROJECT_REPORT_MAX_REASON_CHARS: usize = 512;
const MAX_TEXT_CHARS: usize = 8_192;
const MAX_LIST: usize = 2_000;

pub(crate) const ACTION_COMPOUNDED_LIST: &str = "list_compounded_reports";
pub(crate) const ACTION_INVENTORY: &str = "transformer_inventory";
pub(crate) const ACTION_LIST_STATUS: &str = "list_transformers_status";
pub(crate) const ACTION_RETIRE: &str = "retire_transformer";
pub(crate) const ACTION_RESTORE: &str = "restore_transformer";
/// Deleted-transformer recovery: read one verified delete backup, or create the
/// deleted working document again from it. Both are governed storage actions
/// on the fixed `/report` route.
pub(crate) const ACTION_READ_DELETED_BACKUP: &str = "read_transformer_backup";
pub(crate) const ACTION_RESTORE_DELETED: &str = "restore_deleted_transformer";
/// The authorized signer for stored objects a status read or listing named.
pub(crate) const ACTION_SIGN_URL: &str = "sign_url";
/// The preview-pinned pole-material catalogue repair (`ds design materials
/// preview|apply`): the kernel builds the fenced request
/// (`design_config::material_propagation_request`) and judges the receipt;
/// this route carries it under `material_propagation`, as the browser's
/// `propagateConfigMaterials` did. Gated on `network.template.propagate`.
pub(crate) const ACTION_PROPAGATE_MATERIALS: &str = "propagate_config_materials";

/// One bounded, validated set of transformer reference names.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TransformerSet(Vec<String>);

impl TransformerSet {
    /// Validate and canonicalize one explicit set using the same reference
    /// shape as ds-brain. Canonical aliases are de-duplicated, then sorted in
    /// byte-wise identity order so the request and its exact receipt cannot
    /// disagree after the service normalizes a name.
    pub fn new(names: impl IntoIterator<Item = String>) -> Result<Self, ClientError> {
        let out = ds_command_kernel::report::transformers::transformer_set(names)
            .map_err(|message| ClientError::new(ErrorKind::InvalidInput, message))?;
        Ok(Self(out))
    }

    pub fn names(&self) -> &[String] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// One retirement write: the exact names and, for retirement, the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetirementRequest {
    transformers: TransformerSet,
    reason: Option<String>,
}

impl RetirementRequest {
    /// A retirement names at least one transformer and carries a bounded
    /// non-empty reason; the reason is recorded on the document and audited.
    pub fn retire(transformers: TransformerSet, reason: String) -> Result<Self, ClientError> {
        if transformers.is_empty() {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "a retirement names at least one transformer",
            ));
        }
        if !bounded(&reason, PROJECT_REPORT_MAX_REASON_CHARS, false) {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "a retirement reason is empty, untrimmed, or outside its bound",
            ));
        }
        Ok(Self {
            transformers,
            reason: Some(reason),
        })
    }

    /// A restoration names at least one retired transformer.
    pub fn restore(transformers: TransformerSet) -> Result<Self, ClientError> {
        if transformers.is_empty() {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "a restoration names at least one transformer",
            ));
        }
        Ok(Self {
            transformers,
            reason: None,
        })
    }

    pub fn transformers(&self) -> &TransformerSet {
        &self.transformers
    }
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

/// The closed operation vocabulary behind the fixed `/report` route.
pub(crate) enum ProjectReportOperation<'a> {
    CompoundedList,
    Inventory(&'a TransformerSet),
    ListStatus(&'a TransformerSet),
    Retire(&'a RetirementRequest),
    Restore(&'a RetirementRequest),
    ReadDeletedBackup(&'a DeletedBackupRequest),
    RestoreDeleted(&'a DeletedBackupRequest),
    SignUrls(&'a ArtifactDownloadRequest),
    /// The kernel-built material propagation request, sent whole.
    MaterialPropagation(&'a Value),
}

impl ProjectReportOperation<'_> {
    pub(crate) const fn action(&self) -> &'static str {
        match self {
            Self::CompoundedList => ACTION_COMPOUNDED_LIST,
            Self::Inventory(_) => ACTION_INVENTORY,
            Self::ListStatus(_) => ACTION_LIST_STATUS,
            Self::Retire(_) => ACTION_RETIRE,
            Self::Restore(_) => ACTION_RESTORE,
            Self::ReadDeletedBackup(_) => ACTION_READ_DELETED_BACKUP,
            Self::RestoreDeleted(_) => ACTION_RESTORE_DELETED,
            Self::SignUrls(_) => ACTION_SIGN_URL,
            Self::MaterialPropagation(_) => ACTION_PROPAGATE_MATERIALS,
        }
    }

    pub(crate) const fn timeout_seconds(&self) -> u64 {
        match self {
            Self::ListStatus(_) => {
                ds_command_kernel::transformer_status_store::READ_TIMEOUT_SECONDS
            }
            _ => crate::transport::PROJECT_REPORT_READ_TIMEOUT_SECONDS,
        }
    }

    pub(crate) fn body(&self, project: &str) -> Vec<u8> {
        if let Self::ListStatus(requested) = self {
            return serde_json::to_vec(&ds_command_kernel::transformer_status_store::request_body(
                project,
                requested.names(),
            ))
            .expect("closed status request serializes");
        }
        let mut body = Map::new();
        body.insert("action".into(), Value::String(self.action().into()));
        body.insert("eds_project_id".into(), Value::String(project.into()));
        match self {
            Self::CompoundedList => {
                body.insert("include_artifacts".into(), Value::Bool(true));
            }
            Self::Inventory(set) | Self::ListStatus(set) => {
                if !set.is_empty() {
                    body.insert("transformers".into(), names_value(set));
                }
            }
            Self::Retire(request) => {
                body.insert("transformers".into(), names_value(&request.transformers));
                if let Some(reason) = request.reason.as_deref() {
                    body.insert("reason".into(), Value::String(reason.into()));
                }
            }
            Self::Restore(request) => {
                body.insert("transformers".into(), names_value(&request.transformers));
            }
            Self::ReadDeletedBackup(request) | Self::RestoreDeleted(request) => {
                body.insert("transformers".into(), names_value(&request.transformers));
                body.insert("backup".into(), request.backup_value());
            }
            Self::SignUrls(request) => {
                body.insert(
                    "paths".into(),
                    Value::Array(
                        request
                            .identities
                            .iter()
                            .map(|identity| Value::String(identity.locator()))
                            .collect(),
                    ),
                );
            }
            Self::MaterialPropagation(request) => {
                body.insert("material_propagation".into(), (*request).clone());
            }
        }
        // Keep fixed request bytes independent of Cargo feature unification: the
        // shared kernel also links serde_json with insertion-order maps.
        let mut document = Value::Object(body);
        document.sort_all_objects();
        serde_json::to_vec(&document).expect("a closed report request always serializes")
    }
}

fn names_value(set: &TransformerSet) -> Value {
    Value::Array(
        set.names()
            .iter()
            .map(|name| Value::String(name.clone()))
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Compounded archive registry
// ---------------------------------------------------------------------------

/// The layout recorded on one archive at creation. Absent on legacy archives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundedArchiveLayout {
    file_level: Option<String>,
    transformer_grouping: Option<String>,
    combine_per_district: bool,
    group_depth: Option<u32>,
    transformer_folders: Option<bool>,
    combine_per_group: Option<bool>,
}

/// The deepest grouping level a recorded layout may name. A plan has at most
/// a handful of ordered definitions; anything beyond this is not a layout.
const MAX_ARCHIVE_GROUP_DEPTH: u64 = 64;

impl CompoundedArchiveLayout {
    pub fn file_level(&self) -> Option<&str> {
        self.file_level.as_deref()
    }
    /// The legacy spelling of the same choice. Archives written before
    /// `file_level` carry only this, so a reader that ignores it reports a
    /// foldered archive as having no layout at all — which is how the command
    /// line and the application came to describe one archive differently. The
    /// fold between the two spellings belongs to the shared kernel, not here.
    pub fn transformer_grouping(&self) -> Option<&str> {
        self.transformer_grouping.as_deref()
    }
    pub const fn combine_per_district(&self) -> bool {
        self.combine_per_district
    }
    /// The generic knobs ds-brain writes on every current entry: how many of
    /// the applied plan's levels the tree descends (`None` = every level),
    /// whether each transformer gets its own folder, and whether one combined
    /// report is added per first-level group. `None` when not recorded.
    pub const fn group_depth(&self) -> Option<u32> {
        self.group_depth
    }
    pub const fn transformer_folders(&self) -> Option<bool> {
        self.transformer_folders
    }
    pub const fn combine_per_group(&self) -> Option<bool> {
        self.combine_per_group
    }
    /// The recorded layout as the kernel's report layer reads it, so a host
    /// describes the archive in the one vocabulary (`describe()`), generic
    /// knobs first, then the current spelling, then the legacy twin.
    pub fn report_layout(&self) -> ds_command_kernel::report::ArchiveLayout {
        ds_command_kernel::report::ArchiveLayout {
            file_level: self.file_level.clone(),
            transformer_grouping: self.transformer_grouping.clone(),
            combine_per_district: Some(self.combine_per_district),
            combine_per_group: self.combine_per_group,
            group_depth: self.group_depth,
            transformer_folders: self.transformer_folders,
        }
    }
}

/// One registry row of a published compounded archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompoundedArchiveMember {
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundedArchive {
    composition: Option<Value>,
    artifact_index_state: Option<String>,
    archive_members: Vec<CompoundedArchiveMember>,
    stem: String,
    filename: String,
    gcs_path: String,
    download_url: Option<String>,
    created_at: Option<String>,
    created_by: Option<String>,
    status: Option<String>,
    transformer_count: u64,
    transformers: Vec<String>,
    groups: ArchiveGroups,
    individual_artifact_transformer_count: u64,
    missing_individual_artifact_count: u64,
    errors: Vec<String>,
    archive_layout: Option<CompoundedArchiveLayout>,
}

impl CompoundedArchive {
    pub fn composition(&self) -> Option<&Value> {
        self.composition.as_ref()
    }
    pub fn artifact_index_state(&self) -> Option<&str> {
        self.artifact_index_state.as_deref()
    }
    pub fn archive_members(&self) -> &[CompoundedArchiveMember] {
        &self.archive_members
    }
    pub fn stem(&self) -> &str {
        &self.stem
    }
    pub fn filename(&self) -> &str {
        &self.filename
    }
    pub fn gcs_path(&self) -> &str {
        &self.gcs_path
    }
    /// A short-lived signed download; present only when the backend signed it.
    pub fn download_url(&self) -> Option<&str> {
        self.download_url.as_deref()
    }
    pub fn created_at(&self) -> Option<&str> {
        self.created_at.as_deref()
    }
    pub fn created_by(&self) -> Option<&str> {
        self.created_by.as_deref()
    }
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }
    pub const fn transformer_count(&self) -> u64 {
        self.transformer_count
    }
    pub fn transformers(&self) -> &[String] {
        &self.transformers
    }
    /// How the archive is grouped, decoded by the kernel's report layer:
    /// ds-brain's generic groups and plan identity, or the pre-plan district
    /// pair on archives written before grouping plans.
    pub const fn groups(&self) -> &ArchiveGroups {
        &self.groups
    }
    /// First-level groups the archive covers.
    pub const fn group_count(&self) -> u64 {
        self.groups.count
    }
    pub const fn individual_artifact_transformer_count(&self) -> u64 {
        self.individual_artifact_transformer_count
    }
    pub const fn missing_individual_artifact_count(&self) -> u64 {
        self.missing_individual_artifact_count
    }
    pub fn errors(&self) -> &[String] {
        &self.errors
    }
    pub fn archive_layout(&self) -> Option<&CompoundedArchiveLayout> {
        self.archive_layout.as_ref()
    }
}

#[derive(Deserialize)]
struct ArchiveWire {
    #[serde(default)]
    composition: Option<Value>,
    #[serde(default)]
    artifact_index_state: Option<String>,
    #[serde(default)]
    archive_members: Vec<CompoundedArchiveMember>,
    stem: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    gcs_path: String,
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    created_by: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    transformer_count: u64,
    #[serde(default)]
    transformers: Vec<String>,
    #[serde(flatten)]
    grouping: ArchiveGroupingFields,
    #[serde(default)]
    individual_artifact_transformer_count: u64,
    #[serde(default)]
    missing_individual_artifact_count: u64,
    #[serde(default)]
    errors: Vec<String>,
    #[serde(default)]
    archive_layout: Option<Map<String, Value>>,
}

#[derive(Deserialize)]
struct ArchiveListWire {
    compounded_reports: Vec<ArchiveWire>,
    #[serde(default)]
    composition_template: Option<Value>,
    #[serde(default)]
    composition_schema: Option<Value>,
}

/// The archive registry and the server's own JSON authoring contract. Older
/// registries have no recipe metadata; callers must not reconstruct it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundedArchiveCatalog {
    archives: Vec<CompoundedArchive>,
    composition_template: Option<Value>,
    composition_schema: Option<Value>,
}

impl CompoundedArchiveCatalog {
    pub fn composition_template(&self) -> Option<&Value> {
        self.composition_template.as_ref()
    }
    pub fn composition_schema(&self) -> Option<&Value> {
        self.composition_schema.as_ref()
    }
}

impl std::ops::Deref for CompoundedArchiveCatalog {
    type Target = [CompoundedArchive];
    fn deref(&self) -> &Self::Target {
        &self.archives
    }
}

pub(crate) fn decode_compounded_list(
    response: TransportResponse,
    limit: usize,
) -> Result<CompoundedArchiveCatalog, ClientError> {
    let wire: ArchiveListWire = decode(response, limit)?;
    if wire.compounded_reports.len() > MAX_LIST
        || !valid_composition(wire.composition_template.as_ref())
        || !valid_composition(wire.composition_schema.as_ref())
    {
        return Err(unreadable());
    }
    let mut out = Vec::with_capacity(wire.compounded_reports.len());
    for entry in wire.compounded_reports {
        if !valid_composition(entry.composition.as_ref())
            || !bounded(&entry.stem, 512, false)
            || !opt_bounded(entry.artifact_index_state.as_deref(), 64)
            || entry.archive_members.len() > 10000
            || !entry.archive_members.iter().all(|member| {
                bounded(&member.path, 1024, false)
                    && !member.path.starts_with('/')
                    && !member.path.contains('\\')
                    && member
                        .path
                        .split('/')
                        .all(|part| !matches!(part, "" | "." | ".."))
            })
            || !bounded(&entry.filename, 1_024, true)
            || !bounded(&entry.gcs_path, 2_048, true)
            || !opt_bounded(entry.download_url.as_deref(), 8_192)
            || !opt_bounded(entry.created_at.as_deref(), 64)
            || !opt_bounded(entry.created_by.as_deref(), 320)
            || !opt_bounded(entry.status.as_deref(), 64)
            || entry.transformers.len() > MAX_LIST
            || !entry
                .transformers
                .iter()
                .all(|value| bounded(value, PROJECT_REPORT_MAX_TRANSFORMER_CHARS, false))
            || entry.errors.len() > MAX_LIST
            || !entry
                .errors
                .iter()
                .all(|value| bounded(value, MAX_TEXT_CHARS, true))
        {
            return Err(unreadable());
        }
        let groups = archive_groups::decode(&entry.grouping).map_err(|_| unreadable())?;
        let archive_layout = match entry.archive_layout {
            None => None,
            Some(layout) => {
                let file_level = match layout.get("file_level") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(value)) if bounded(value, 64, false) => Some(value.clone()),
                    Some(_) => return Err(unreadable()),
                };
                let transformer_grouping = match layout.get("transformer_grouping") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(value)) if bounded(value, 64, false) => Some(value.clone()),
                    Some(_) => return Err(unreadable()),
                };
                let combine_per_district = match layout.get("combine_per_district") {
                    None | Some(Value::Null) => false,
                    Some(Value::Bool(value)) => *value,
                    Some(_) => return Err(unreadable()),
                };
                let flag = |key: &str| match layout.get(key) {
                    None | Some(Value::Null) => Ok(None),
                    Some(Value::Bool(value)) => Ok(Some(*value)),
                    Some(_) => Err(unreadable()),
                };
                let transformer_folders = flag("transformer_folders")?;
                let combine_per_group = flag("combine_per_group")?;
                let group_depth = match layout.get("group_depth") {
                    None | Some(Value::Null) => None,
                    Some(value) => match value.as_u64() {
                        Some(depth) if depth <= MAX_ARCHIVE_GROUP_DEPTH => Some(depth as u32),
                        _ => return Err(unreadable()),
                    },
                };
                Some(CompoundedArchiveLayout {
                    file_level,
                    transformer_grouping,
                    combine_per_district,
                    group_depth,
                    transformer_folders,
                    combine_per_group,
                })
            }
        };
        out.push(CompoundedArchive {
            composition: entry.composition,
            artifact_index_state: nonempty(entry.artifact_index_state),
            archive_members: entry.archive_members,
            stem: entry.stem,
            filename: entry.filename,
            gcs_path: entry.gcs_path,
            download_url: nonempty(entry.download_url),
            created_at: nonempty(entry.created_at),
            created_by: nonempty(entry.created_by),
            status: nonempty(entry.status),
            transformer_count: entry.transformer_count,
            transformers: entry.transformers,
            groups,
            individual_artifact_transformer_count: entry.individual_artifact_transformer_count,
            missing_individual_artifact_count: entry.missing_individual_artifact_count,
            errors: entry.errors,
            archive_layout,
        });
    }
    Ok(CompoundedArchiveCatalog {
        archives: out,
        composition_template: wire.composition_template,
        composition_schema: wire.composition_schema,
    })
}

// ---------------------------------------------------------------------------
// Transformer inventory and retirement
// ---------------------------------------------------------------------------

fn valid_composition(value: Option<&Value>) -> bool {
    value.is_none_or(|value| value.is_object() && value.to_string().len() <= 64 * 1024)
}

/// The lifecycle state of one transformer document in the selected project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransformerLifecycle {
    /// A live document that reports, tiles, and listings include.
    Active,
    /// Excluded from every consumer, with a retirement record; restorable.
    Retired,
    /// Excluded without a retirement record (soft-deleted by another path);
    /// not restorable through retirement.
    Deleted,
    /// The requested name has no document.
    Missing,
}

impl TransformerLifecycle {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Retired => "retired",
            Self::Deleted => "deleted",
            Self::Missing => "missing",
        }
    }
}

/// What one document is: an ordinary transformer or a project-level input
/// (`mv_data`) that retirement refuses to touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformerKind {
    Transformer,
    ProjectLevel,
}

impl TransformerKind {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Transformer => "transformer",
            Self::ProjectLevel => "project_level",
        }
    }
}

/// The verified backup ds-brain wrote before it retired a transformer: the
/// exact storage object, its generation, and the digest and length of the
/// bytes it holds. A restore of a deleted transformer names this receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetirementBackup {
    bucket: String,
    object: String,
    generation: i64,
    sha256: String,
    byte_length: i64,
}

impl RetirementBackup {
    pub fn bucket(&self) -> &str {
        &self.bucket
    }
    pub fn object(&self) -> &str {
        &self.object
    }
    pub const fn generation(&self) -> i64 {
        self.generation
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub const fn byte_length(&self) -> i64 {
        self.byte_length
    }
}

/// The retirement record ds-brain keeps on a retired or restored document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetirementRecord {
    reason: Option<String>,
    retired_at: Option<u64>,
    retired_by: Option<String>,
    restored_at: Option<u64>,
    restored_by: Option<String>,
    backup: Option<RetirementBackup>,
}

impl RetirementRecord {
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
    pub const fn retired_at(&self) -> Option<u64> {
        self.retired_at
    }
    pub fn retired_by(&self) -> Option<&str> {
        self.retired_by.as_deref()
    }
    pub const fn restored_at(&self) -> Option<u64> {
        self.restored_at
    }
    pub fn restored_by(&self) -> Option<&str> {
        self.restored_by.as_deref()
    }
    /// The verified backup written before this retirement, when ds-brain
    /// recorded one.
    pub fn backup(&self) -> Option<&RetirementBackup> {
        self.backup.as_ref()
    }
}

/// One inventory row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformerInventoryRow {
    name: String,
    kind: TransformerKind,
    lifecycle: TransformerLifecycle,
    retirement: Option<RetirementRecord>,
}

impl TransformerInventoryRow {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub const fn kind(&self) -> TransformerKind {
        self.kind
    }
    pub const fn lifecycle(&self) -> TransformerLifecycle {
        self.lifecycle
    }
    pub fn retirement(&self) -> Option<&RetirementRecord> {
        self.retirement.as_ref()
    }
}

/// The bounded inventory of one project's transformer documents, or of the
/// exact requested names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformerInventory {
    project: String,
    requested: Vec<String>,
    active_count: u64,
    retired_count: u64,
    deleted_count: u64,
    rows: Vec<TransformerInventoryRow>,
}

impl TransformerInventory {
    pub fn project(&self) -> &str {
        &self.project
    }
    /// The exact names the request carried; empty for a whole-project read.
    pub fn requested(&self) -> &[String] {
        &self.requested
    }
    pub const fn active_count(&self) -> u64 {
        self.active_count
    }
    pub const fn retired_count(&self) -> u64 {
        self.retired_count
    }
    pub const fn deleted_count(&self) -> u64 {
        self.deleted_count
    }
    pub fn rows(&self) -> &[TransformerInventoryRow] {
        &self.rows
    }
}

#[derive(Deserialize)]
struct RetirementWire {
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    retired_at: Option<u64>,
    #[serde(default)]
    retired_by: Option<String>,
    #[serde(default)]
    restored_at: Option<u64>,
    #[serde(default)]
    restored_by: Option<String>,
    #[serde(default)]
    backup: Option<BackupWire>,
}

#[derive(Deserialize)]
struct BackupWire {
    bucket: String,
    object: String,
    generation: i64,
    sha256: String,
    byte_length: i64,
}

/// A backup receipt is all-or-nothing: a present receipt with a field out of
/// bounds is an unreadable answer, never a partial one.
fn decode_backup(wire: Option<BackupWire>) -> Result<Option<RetirementBackup>, ClientError> {
    let Some(wire) = wire else {
        return Ok(None);
    };
    let digest_ok = wire.sha256.len() == 64
        && wire
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if wire.bucket.is_empty()
        || wire.bucket.chars().count() > 222
        || wire.object.is_empty()
        || wire.object.chars().count() > 1024
        || wire.generation <= 0
        || wire.byte_length <= 0
        || !digest_ok
    {
        return Err(unreadable());
    }
    Ok(Some(RetirementBackup {
        bucket: wire.bucket,
        object: wire.object,
        generation: wire.generation,
        sha256: wire.sha256,
        byte_length: wire.byte_length,
    }))
}

#[derive(Deserialize)]
struct InventoryRowWire {
    name: String,
    #[serde(default = "default_kind")]
    kind: String,
    state: String,
    #[serde(default)]
    retirement: Option<RetirementWire>,
}

fn default_kind() -> String {
    "transformer".to_owned()
}

#[derive(Deserialize)]
struct InventoryWire {
    eds_project_id: String,
    #[serde(default)]
    requested: Vec<String>,
    #[serde(default)]
    active_count: u64,
    #[serde(default)]
    retired_count: u64,
    #[serde(default)]
    deleted_count: u64,
    transformers: Vec<InventoryRowWire>,
}

fn decode_retirement(wire: RetirementWire) -> Result<RetirementRecord, ClientError> {
    if !opt_bounded(wire.reason.as_deref(), PROJECT_REPORT_MAX_REASON_CHARS)
        || !opt_bounded(wire.retired_by.as_deref(), 320)
        || !opt_bounded(wire.restored_by.as_deref(), 320)
    {
        return Err(unreadable());
    }
    Ok(RetirementRecord {
        reason: nonempty(wire.reason),
        retired_at: wire.retired_at,
        retired_by: nonempty(wire.retired_by),
        restored_at: wire.restored_at,
        restored_by: nonempty(wire.restored_by),
        backup: decode_backup(wire.backup)?,
    })
}

pub(crate) fn decode_inventory(
    response: TransportResponse,
    limit: usize,
    project: &str,
    requested: &TransformerSet,
) -> Result<TransformerInventory, ClientError> {
    let wire: InventoryWire = decode(response, limit)?;
    if wire.eds_project_id != project
        || wire.requested != requested.names()
        || wire.transformers.len() > MAX_LIST
    {
        return Err(unreadable());
    }
    let mut rows = Vec::with_capacity(wire.transformers.len());
    for row in wire.transformers {
        if !bounded(&row.name, PROJECT_REPORT_MAX_TRANSFORMER_CHARS, false) {
            return Err(unreadable());
        }
        let kind = match row.kind.as_str() {
            "transformer" => TransformerKind::Transformer,
            "project_level" => TransformerKind::ProjectLevel,
            _ => return Err(unreadable()),
        };
        let lifecycle = match row.state.as_str() {
            "active" => TransformerLifecycle::Active,
            "retired" => TransformerLifecycle::Retired,
            "deleted" => TransformerLifecycle::Deleted,
            "missing" => TransformerLifecycle::Missing,
            _ => return Err(unreadable()),
        };
        let retirement = match row.retirement {
            None => None,
            Some(record) => Some(decode_retirement(record)?),
        };
        rows.push(TransformerInventoryRow {
            name: row.name,
            kind,
            lifecycle,
            retirement,
        });
    }
    if !requested.is_empty() && rows.len() != requested.len() {
        return Err(unreadable());
    }
    Ok(TransformerInventory {
        project: wire.eds_project_id,
        requested: wire.requested,
        active_count: wire.active_count,
        retired_count: wire.retired_count,
        deleted_count: wire.deleted_count,
        rows,
    })
}

// ---------------------------------------------------------------------------
// Transformer status rows
// ---------------------------------------------------------------------------

/// One transformer status row exactly as ds-brain sent it.
///
/// The row is deliberately NOT reshaped here. ds-brain's `TransformerStatus`
/// carries around forty optional members — `process_metadata`,
/// `report_metadata`, `layers`, `uploads`, `retry_capabilities` and the rest —
/// and what each one means belongs to the kernel modules that will read them,
/// not to a transport. So this crate proves only what a transport can prove:
/// the envelope is the closed shape, the row is an object, its `name` is one
/// bounded reference, and the whole answer fits inside the fixed bound.
///
/// The browser's client-only fields (`client_*`) do not exist on the wire and
/// are never synthesised here.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformerStatusRow {
    name: String,
    kind: Option<String>,
    row: Value,
}

impl TransformerStatusRow {
    /// The transformer document's own reference name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// The wire kind tag when the service sent one.
    ///
    /// ds-brain's status document carries no `kind` today — the inventory row
    /// is where a transformer's kind is stated — so this is `None` for every
    /// row the service currently sends. It is read rather than ignored so a
    /// row that gains the tag is carried through, still typed and bounded,
    /// without a decode change.
    pub fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }
    /// The whole row as sent, `name` and `kind` included.
    pub fn row(&self) -> &Value {
        &self.row
    }
}

/// The bounded status rows of one project's transformers, or of the exact
/// requested names.
///
/// A named read answers only the names that have a document: ds-brain omits a
/// name it cannot find rather than reporting it, so a shorter list than the
/// request is a legitimate answer and never a refusal — including the empty
/// list, which the service encodes as `null` (see `rows_or_null`).
#[derive(Debug, Clone, PartialEq)]
pub struct TransformerStatusList {
    transformers: Vec<TransformerStatusRow>,
}

impl TransformerStatusList {
    /// Re-admit a held observation using the status transport's bounded decoder.
    pub fn from_observation(rows: Value) -> Result<Self, ClientError> {
        decode_status(
            TransportResponse {
                status: 200,
                body: serde_json::to_vec(&serde_json::json!({"transformers":rows}))
                    .map_err(|_| unreadable())?,
            },
            crate::PROJECT_REPORT_RESPONSE_LIMIT,
        )
    }

    pub fn rows(&self) -> &[TransformerStatusRow] {
        &self.transformers
    }
    pub fn len(&self) -> usize {
        self.transformers.len()
    }
    pub fn is_empty(&self) -> bool {
        self.transformers.is_empty()
    }
}

#[derive(Deserialize)]
struct StatusWire {
    #[serde(deserialize_with = "rows_or_null")]
    transformers: Vec<Value>,
}

/// `transformers` must be present, and `null` there is the empty answer.
///
/// ds-brain builds the row list and then hands it to an additive projection
/// that returns `append([]TransformerStatus(nil), statuses...)` when there is
/// nothing to project (ds-brain
/// `internal/services/network_reporter_status_projection.go:51`), so a project
/// with no matching transformer document answers `{"transformers": null}` —
/// Go's encoding of a nil slice — rather than `[]`. That is the service's own
/// way of saying "no rows", and the browser has always read it as one
/// (`ds-web/src/lib/api/design.ts:616`). The native path reads it the same
/// way, because the whole point of this operation is that a consumer sees one
/// shape whether it runs in `ds` or in the application; refusing here would
/// turn every empty project into `auth_response_unreadable`. Every other
/// non-array is still a broken envelope, and a response carrying no
/// `transformers` member at all is not this action's answer.
fn rows_or_null<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Value>, D::Error> {
    Ok(Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default())
}

/// Decode one `list_transformers_status` answer.
///
/// The row count is bounded by `MAX_LIST`, not by
/// [`PROJECT_REPORT_MAX_TRANSFORMERS`]. The two bounds answer two different
/// questions and are deliberately different: 500 is how many names one
/// *request* may carry, which is a local promise about a request this crate
/// builds; 2,000 is how many rows an *answer* may contain, which is a bound on
/// what a project's own Firestore collection may hold — the same bound
/// [`decode_inventory`] applies to the same collection. An unnamed read asks
/// for the whole project and legitimately answers more rows than any single
/// request could have named. The real ceiling on this answer is the fixed
/// response byte cap, which `decode` applies before a single row is parsed.
pub(crate) fn decode_status(
    response: TransportResponse,
    limit: usize,
) -> Result<TransformerStatusList, ClientError> {
    // A non-array `transformers` other than the service's own `null`, a
    // missing one, or a body over the fixed cap never becomes a row: `decode`
    // refuses the bytes first.
    let wire: StatusWire = decode(
        response,
        limit.min(ds_command_kernel::transformer_status_store::MAX_RESPONSE_BYTES),
    )?;
    let rows =
        ds_command_kernel::transformer_status_store::admit_rows(Value::Array(wire.transformers))
            .map_err(|_| unreadable())?
            .into_iter()
            .map(|row| TransformerStatusRow {
                name: row["name"].as_str().expect("admitted name").into(),
                kind: row.get("kind").and_then(Value::as_str).map(str::to_owned),
                row,
            })
            .collect();
    Ok(TransformerStatusList { transformers: rows })
}

/// Which retirement write a receipt answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetirementAction {
    Retire,
    Restore,
}

impl RetirementAction {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Retire => "retire",
            Self::Restore => "restore",
        }
    }
}

/// The closed per-transformer refusal vocabulary of a retirement write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetirementRefusal {
    NotFound,
    AlreadyRetired,
    NotRetired,
    NoRetirementRecord,
    SpecialDocument,
    GovernanceLocked,
    NotOwner,
    Failed,
}

impl RetirementRefusal {
    pub const fn token(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::AlreadyRetired => "already_retired",
            Self::NotRetired => "not_retired",
            Self::NoRetirementRecord => "no_retirement_record",
            Self::SpecialDocument => "special_document",
            Self::GovernanceLocked => "governance_locked",
            Self::NotOwner => "not_owner",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "not_found" => Self::NotFound,
            "already_retired" => Self::AlreadyRetired,
            "not_retired" => Self::NotRetired,
            "no_retirement_record" => Self::NoRetirementRecord,
            "special_document" => Self::SpecialDocument,
            "governance_locked" => Self::GovernanceLocked,
            "not_owner" => Self::NotOwner,
            "failed" => Self::Failed,
            _ => return None,
        })
    }
}

/// One transformer's outcome inside a retirement receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetirementResult {
    name: String,
    applied: bool,
    at: Option<u64>,
    refusal: Option<RetirementRefusal>,
    error: Option<String>,
    backup: Option<RetirementBackup>,
}

impl RetirementResult {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub const fn applied(&self) -> bool {
        self.applied
    }
    pub const fn at(&self) -> Option<u64> {
        self.at
    }
    pub const fn refusal(&self) -> Option<RetirementRefusal> {
        self.refusal
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    /// The verified backup a successful retirement wrote first, as ds-brain
    /// reported it.
    pub fn backup(&self) -> Option<&RetirementBackup> {
        self.backup.as_ref()
    }
}

/// The receipt of one retirement or restoration write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetirementReceipt {
    project: String,
    action: RetirementAction,
    results: Vec<RetirementResult>,
    applied_count: u64,
    failed_count: u64,
}

impl RetirementReceipt {
    pub fn project(&self) -> &str {
        &self.project
    }
    pub const fn action(&self) -> RetirementAction {
        self.action
    }
    pub fn results(&self) -> &[RetirementResult] {
        &self.results
    }
    pub const fn applied_count(&self) -> u64 {
        self.applied_count
    }
    pub const fn failed_count(&self) -> u64 {
        self.failed_count
    }
}

#[derive(Deserialize)]
struct RetirementResultWire {
    name: String,
    status: String,
    #[serde(default)]
    at: Option<u64>,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    backup: Option<BackupWire>,
}

#[derive(Deserialize)]
struct RetirementReceiptWire {
    eds_project_id: String,
    action: String,
    results: Vec<RetirementResultWire>,
    #[serde(default)]
    applied_count: u64,
    #[serde(default)]
    failed_count: u64,
}

pub(crate) fn decode_retirement_receipt(
    response: TransportResponse,
    limit: usize,
    project: &str,
    action: RetirementAction,
    request: &RetirementRequest,
) -> Result<RetirementReceipt, ClientError> {
    let wire: RetirementReceiptWire = decode(response, limit)?;
    let expected_action = match action {
        RetirementAction::Retire => ACTION_RETIRE,
        RetirementAction::Restore => ACTION_RESTORE,
    };
    let applied_token = match action {
        RetirementAction::Retire => "retired",
        RetirementAction::Restore => "restored",
    };
    if wire.eds_project_id != project
        || wire.action != expected_action
        || wire.results.len() != request.transformers.len()
    {
        return Err(unreadable());
    }
    let mut results = Vec::with_capacity(wire.results.len());
    let mut applied = 0_u64;
    let mut failed = 0_u64;
    for (row, expected_name) in wire.results.into_iter().zip(request.transformers.names()) {
        if row.name != *expected_name
            || !opt_bounded(row.error.as_deref(), MAX_TEXT_CHARS)
            || !opt_bounded(row.error_code.as_deref(), 64)
        {
            return Err(unreadable());
        }
        let (is_applied, refusal) = match row.status.as_str() {
            value if value == applied_token => (true, None),
            "failed" => {
                let Some(code) = row.error_code.as_deref().and_then(RetirementRefusal::parse)
                else {
                    return Err(unreadable());
                };
                (false, Some(code))
            }
            _ => return Err(unreadable()),
        };
        if is_applied {
            applied += 1;
        } else {
            failed += 1;
        }
        results.push(RetirementResult {
            name: row.name,
            applied: is_applied,
            at: row.at,
            refusal,
            error: nonempty(row.error),
            backup: decode_backup(row.backup)?,
        });
    }
    if wire.applied_count != applied || wire.failed_count != failed {
        return Err(unreadable());
    }
    Ok(RetirementReceipt {
        project: wire.eds_project_id,
        action,
        results,
        applied_count: applied,
        failed_count: failed,
    })
}

// ---------------------------------------------------------------------------
// Deleted-transformer recovery
// ---------------------------------------------------------------------------

/// One deleted transformer's verified delete backup: the transformer and the
/// backup object its ledger row names, and — to restore — the exact generation
/// and SHA-256 a read returned. Built only through [`Self::read`] and
/// [`Self::restore`], which hold the bounds ds-brain enforces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedBackupRequest {
    transformers: TransformerSet,
    object: String,
    generation: Option<i64>,
    sha256: Option<String>,
}

fn sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl DeletedBackupRequest {
    fn new(
        transformer: String,
        object: String,
        generation: Option<i64>,
        sha256: Option<String>,
    ) -> Result<Self, ClientError> {
        let invalid = |message: &'static str| ClientError::new(ErrorKind::InvalidInput, message);
        let transformers = TransformerSet::new([transformer])?;
        if transformers.len() != 1 {
            return Err(invalid(
                "a backup read or restore names exactly one transformer",
            ));
        }
        if !bounded(&object, 1024, false) || object.contains('\\') {
            return Err(invalid(
                "a backup object is the ledger's transformers/… .dsgrid key, at most 1024 characters",
            ));
        }
        if generation.is_some_and(|generation| generation <= 0) {
            return Err(invalid("a backup generation is a positive integer"));
        }
        if sha256.as_deref().is_some_and(|digest| !sha256_hex(digest)) {
            return Err(invalid("a backup sha256 is 64 lowercase hex characters"));
        }
        Ok(Self {
            transformers,
            object,
            generation,
            sha256,
        })
    }

    /// Read one backup: the live generation, or the pinned one.
    pub fn read(
        transformer: String,
        object: String,
        generation: Option<i64>,
        sha256: Option<String>,
    ) -> Result<Self, ClientError> {
        Self::new(transformer, object, generation, sha256)
    }

    /// Restore from one backup, pinned exactly as a read returned it.
    pub fn restore(
        transformer: String,
        object: String,
        generation: i64,
        sha256: String,
    ) -> Result<Self, ClientError> {
        Self::new(transformer, object, Some(generation), Some(sha256))
    }

    pub fn transformer(&self) -> &str {
        &self.transformers.names()[0]
    }
    pub fn object(&self) -> &str {
        &self.object
    }
    pub const fn generation(&self) -> Option<i64> {
        self.generation
    }
    pub fn sha256(&self) -> Option<&str> {
        self.sha256.as_deref()
    }

    fn backup_value(&self) -> Value {
        let mut backup = Map::new();
        backup.insert("object".into(), Value::String(self.object.clone()));
        if let Some(generation) = self.generation {
            backup.insert("generation".into(), Value::from(generation));
        }
        if let Some(sha256) = &self.sha256 {
            backup.insert("sha256".into(), Value::String(sha256.clone()));
        }
        Value::Object(backup)
    }
}

/// A verified delete backup as ds-brain read it, and — when fetched — its
/// exact bytes, checked against the receipt's length and SHA-256. The
/// short-lived signed locator is spent inside the core and never returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedBackupRead {
    name: String,
    source_revision: i64,
    layers: Vec<String>,
    backup: RetirementBackup,
    expires_at: String,
    download_url: String,
    bytes: Option<Vec<u8>>,
}

impl DeletedBackupRead {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub const fn source_revision(&self) -> i64 {
        self.source_revision
    }
    pub fn layers(&self) -> &[String] {
        &self.layers
    }
    pub fn backup(&self) -> &RetirementBackup {
        &self.backup
    }
    pub fn expires_at(&self) -> &str {
        &self.expires_at
    }
    /// The verified `.dsgrid` bytes, when the read was asked to fetch them.
    pub fn bytes(&self) -> Option<&[u8]> {
        self.bytes.as_deref()
    }
}

/// The working document ds-brain created again from a verified backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedTransformerRestoration {
    name: String,
    state: String,
    restored_at: i64,
    restored_by: String,
    source_revision: i64,
    version: i64,
    layers: Vec<String>,
    backup: RetirementBackup,
}

impl DeletedTransformerRestoration {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn state(&self) -> &str {
        &self.state
    }
    pub const fn restored_at(&self) -> i64 {
        self.restored_at
    }
    pub fn restored_by(&self) -> &str {
        &self.restored_by
    }
    pub const fn source_revision(&self) -> i64 {
        self.source_revision
    }
    pub const fn version(&self) -> i64 {
        self.version
    }
    pub fn layers(&self) -> &[String] {
        &self.layers
    }
    pub fn backup(&self) -> &RetirementBackup {
        &self.backup
    }
}

#[derive(Deserialize)]
struct DeletedBackupReadWire {
    eds_project_id: String,
    action: String,
    name: String,
    source_revision: i64,
    #[serde(default)]
    layers: Vec<String>,
    backup: BackupWire,
    download_url: String,
    expires_at: String,
}

#[derive(Deserialize)]
struct RestorationWire {
    name: String,
    state: String,
    restored_at: i64,
    restored_by: String,
    source_revision: i64,
    version: i64,
    #[serde(default)]
    layers: Vec<String>,
    backup: BackupWire,
}

#[derive(Deserialize)]
struct DeletedRestoreWire {
    eds_project_id: String,
    action: String,
    restoration: RestorationWire,
}

fn layers_bounded(layers: &[String]) -> bool {
    layers.len() <= 256 && layers.iter().all(|layer| bounded(layer, 128, false))
}

/// The backup the server verified must be the one that was asked for: the
/// same object, and the pinned generation and digest when they were pinned.
fn backup_matches(backup: &RetirementBackup, request: &DeletedBackupRequest) -> bool {
    let object = request
        .object
        .strip_prefix(&format!("gs://{}/", backup.bucket))
        .unwrap_or(&request.object);
    backup.object == object
        && request
            .generation
            .is_none_or(|generation| generation == backup.generation)
        && request
            .sha256
            .as_deref()
            .is_none_or(|digest| digest == backup.sha256)
}

pub(crate) fn decode_deleted_backup_read(
    response: TransportResponse,
    limit: usize,
    project: &str,
    request: &DeletedBackupRequest,
) -> Result<DeletedBackupRead, ClientError> {
    let wire: DeletedBackupReadWire = decode(response, limit)?;
    let backup = decode_backup(Some(wire.backup))?.ok_or_else(unreadable)?;
    if wire.eds_project_id != project
        || wire.action != ACTION_READ_DELETED_BACKUP
        || wire.name != request.transformer()
        || wire.source_revision < 0
        || !layers_bounded(&wire.layers)
        || !bounded(&wire.expires_at, 64, false)
        || !backup_matches(&backup, request)
    {
        return Err(unreadable());
    }
    // The locator is checked once here, so no other host or object is ever
    // fetched with it.
    crate::grid_models::signed_object_locator(
        &wire.download_url,
        &backup.bucket,
        &backup.object,
        usize::try_from(backup.byte_length).map_err(|_| unreadable())?,
    )
    .map_err(|_| unreadable())?;
    Ok(DeletedBackupRead {
        name: wire.name,
        source_revision: wire.source_revision,
        layers: wire.layers,
        backup,
        expires_at: wire.expires_at,
        download_url: wire.download_url,
        bytes: None,
    })
}

pub(crate) fn decode_deleted_restoration(
    response: TransportResponse,
    limit: usize,
    project: &str,
    request: &DeletedBackupRequest,
) -> Result<DeletedTransformerRestoration, ClientError> {
    let wire: DeletedRestoreWire = decode(response, limit)?;
    let restoration = wire.restoration;
    let backup = decode_backup(Some(restoration.backup))?.ok_or_else(unreadable)?;
    if wire.eds_project_id != project
        || wire.action != ACTION_RESTORE_DELETED
        || restoration.name != request.transformer()
        || !matches!(restoration.state.as_str(), "active" | "retired")
        || restoration.restored_at <= 0
        || !bounded(&restoration.restored_by, 320, false)
        || restoration.source_revision < 0
        || restoration.version <= 0
        || !layers_bounded(&restoration.layers)
        || !backup_matches(&backup, request)
    {
        return Err(unreadable());
    }
    Ok(DeletedTransformerRestoration {
        name: restoration.name,
        state: restoration.state,
        restored_at: restoration.restored_at,
        restored_by: restoration.restored_by,
        source_revision: restoration.source_revision,
        version: restoration.version,
        layers: restoration.layers,
        backup,
    })
}

/// The recovery actions' closed refusal vocabulary, carried with the status:
/// ds-brain answers `{error_code, detail}` for each typed refusal and
/// `{detail}` for a capability or input refusal.
pub(crate) fn deleted_backup_refusal(response: &TransportResponse) -> ClientError {
    #[derive(Deserialize)]
    struct Wire {
        #[serde(default)]
        error_code: Option<String>,
        #[serde(default)]
        detail: Option<String>,
    }
    let wire: Option<Wire> = (response.body.len() <= 64 * 1024)
        .then(|| serde_json::from_slice(&response.body).ok())
        .flatten();
    let code = wire.as_ref().and_then(|wire| {
        wire.error_code.as_deref().filter(|code| {
            matches!(
                *code,
                "backup_pin_invalid"
                    | "special_document"
                    | "backup_not_found"
                    | "backup_unverified"
                    | "backup_identity_mismatch"
                    | "transformer_exists"
                    | "history_diverged"
                    | "backup_unavailable"
                    | "failed"
            )
        })
    });
    let detail = wire
        .as_ref()
        .and_then(|wire| wire.detail.as_deref())
        .filter(|detail| detail.chars().count() <= MAX_TEXT_CHARS);
    let (kind, message) = match response.status {
        400 | 404 | 409 | 422 => (
            ErrorKind::InvalidInput,
            "the deleted transformer's backup was refused",
        ),
        401 | 403 => (
            ErrorKind::AuthenticationRejected,
            "reading or restoring a deleted transformer's backup needs transformer.delete",
        ),
        _ => (
            ErrorKind::Transient,
            "deleted-transformer recovery is temporarily unavailable",
        ),
    };
    ClientError::with_service_refusal(
        kind,
        message,
        crate::ServiceRefusal::new(response.status, code, detail),
    )
}

/// Read one verified delete backup, and fetch its exact bytes when asked.
pub(crate) fn deleted_backup_read<T: crate::AuthorizedTransport>(
    profile: &crate::ClientProfile,
    transport: &mut T,
    authority: crate::authorized::Authority<'_>,
    ds_project: &str,
    request: &DeletedBackupRequest,
    fetch: bool,
) -> Result<DeletedBackupRead, ClientError> {
    let response = transport.project_report(crate::ProjectReportCall {
        profile,
        ds_project,
        operation: ProjectReportOperation::ReadDeletedBackup(request),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(deleted_backup_refusal(&response));
    }
    let mut read = decode_deleted_backup_read(
        response,
        crate::PROJECT_REPORT_RESPONSE_LIMIT,
        ds_project,
        request,
    )?;
    if fetch {
        fetch_deleted_backup(transport, &mut read)?;
    }
    Ok(read)
}

/// Create the deleted working document again from one pinned backup.
pub(crate) fn deleted_backup_restore<T: crate::AuthorizedTransport>(
    profile: &crate::ClientProfile,
    transport: &mut T,
    authority: crate::authorized::Authority<'_>,
    ds_project: &str,
    request: &DeletedBackupRequest,
) -> Result<DeletedTransformerRestoration, ClientError> {
    if request.generation.is_none() || request.sha256.is_none() {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "a restoration pins the backup generation and sha256 exactly as a read returned them",
        ));
    }
    let response = transport.project_report(crate::ProjectReportCall {
        profile,
        ds_project,
        operation: ProjectReportOperation::RestoreDeleted(request),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(deleted_backup_refusal(&response));
    }
    decode_deleted_restoration(
        response,
        crate::PROJECT_REPORT_RESPONSE_LIMIT,
        ds_project,
        request,
    )
}

/// Fetch the verified backup's bytes through the signed-object transport and
/// admit them only at the receipt's exact length and SHA-256.
pub(crate) fn fetch_deleted_backup<T: crate::AuthorizedTransport>(
    transport: &mut T,
    read: &mut DeletedBackupRead,
) -> Result<(), ClientError> {
    use sha2::{Digest, Sha256};
    let size = usize::try_from(read.backup.byte_length).map_err(|_| unreadable())?;
    let call = crate::grid_models::signed_object_locator(
        &read.download_url,
        &read.backup.bucket,
        &read.backup.object,
        size,
    )?;
    let mut response = transport.grid_model_bytes(call)?;
    if response.status != 200
        || response.body.len() != size
        || format!("{:x}", Sha256::digest(&response.body)) != read.backup.sha256
    {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "the fetched backup bytes do not match the verified receipt's length and SHA-256",
        ));
    }
    read.bytes = Some(std::mem::take(&mut response.body));
    Ok(())
}

// ---------------------------------------------------------------------------
// Authorized artifact downloads
// ---------------------------------------------------------------------------

/// The most stored objects one download may fetch.
pub const MAX_ARTIFACT_DOWNLOADS: usize = 200;

/// One stored object a download plan listed, by its durable identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactIdentity {
    bucket: String,
    object: String,
}

impl ArtifactIdentity {
    pub fn bucket(&self) -> &str {
        &self.bucket
    }
    /// The object path inside the bucket; also the file's place under a
    /// download directory, so two artifacts never share a name.
    pub fn object(&self) -> &str {
        &self.object
    }
    /// `gs://bucket/object`.
    pub fn locator(&self) -> String {
        format!("gs://{}/{}", self.bucket, self.object)
    }
}

fn percent_decoded(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The durable identity behind a listed artifact locator: `gs://bucket/object`,
/// a storage URL `https://storage.googleapis.com/bucket/object?…` (or its
/// browser form on `storage.cloud.google.com`), or a legacy
/// Firebase URL `https://firebasestorage.googleapis.com/v0/b/bucket/o/<object>?…`.
/// Any bearer query is dropped: only the identity is ever sent for signing.
pub fn artifact_identity(locator: &str) -> Option<ArtifactIdentity> {
    let (bucket, object) = if let Some(rest) = locator.strip_prefix("gs://") {
        let (bucket, object) = rest.split_once('/')?;
        (bucket.to_owned(), object.to_owned())
    } else {
        let url = url::Url::parse(locator).ok()?;
        if url.scheme() != "https" || url.port().is_some() || !url.username().is_empty() {
            return None;
        }
        match url.host_str()? {
            // `storage.cloud.google.com` is the browser (cookie-authenticated)
            // form of the same `/bucket/object` path, which older report rows
            // still list; it names exactly the same stored object.
            "storage.googleapis.com" | "storage.cloud.google.com" => {
                let path = percent_decoded(url.path().strip_prefix('/')?)?;
                let (bucket, object) = path.split_once('/')?;
                (bucket.to_owned(), object.to_owned())
            }
            "firebasestorage.googleapis.com" => {
                let rest = url.path().strip_prefix("/v0/b/")?;
                let (bucket, object) = rest.split_once("/o/")?;
                (percent_decoded(bucket)?, percent_decoded(object)?)
            }
            _ => return None,
        }
    };
    let plain = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && !part.contains('\\')
            && !part.chars().any(char::is_control)
    };
    (plain(&bucket)
        && !bucket.contains('/')
        && bucket.len() <= 222
        && object.len() <= 1024
        && object.split('/').all(plain))
    .then_some(ArtifactIdentity { bucket, object })
}

/// The distinct stored objects one download fetches, in plan order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactDownloadRequest {
    identities: Vec<ArtifactIdentity>,
}

impl ArtifactDownloadRequest {
    /// Every locator must name a stored object; one that does not is refused
    /// by name rather than skipped.
    pub fn new<I, S>(locators: I) -> Result<Self, ClientError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut identities: Vec<ArtifactIdentity> = Vec::new();
        for locator in locators {
            let locator = locator.as_ref();
            let identity = artifact_identity(locator).ok_or_else(|| {
                ClientError::new(
                    ErrorKind::InvalidInput,
                    "a listed artifact locator does not name one stored object",
                )
            })?;
            if !identities.contains(&identity) {
                identities.push(identity);
            }
        }
        if identities.is_empty() || identities.len() > MAX_ARTIFACT_DOWNLOADS {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "one download fetches 1 through 200 listed artifacts",
            ));
        }
        Ok(Self { identities })
    }
    pub fn identities(&self) -> &[ArtifactIdentity] {
        &self.identities
    }
}

/// One fetched artifact: its identity, exact bytes and their SHA-256.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactFile {
    pub identity: ArtifactIdentity,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

/// What one download did with each identity, in request order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactOutcome {
    /// Fetched and handed to the sink; the SHA-256 of the exact bytes.
    Fetched { bytes: u64, sha256: String },
    /// The signer declined to sign it for this caller, with its reason.
    Refused { reason: String },
    /// Signed, but the bytes did not arrive whole.
    Failed { reason: String },
}

#[derive(Deserialize)]
struct SignWire {
    #[serde(default)]
    urls: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    refused: std::collections::BTreeMap<String, String>,
    mode: String,
}

fn decode_signed(
    response: TransportResponse,
    request: &ArtifactDownloadRequest,
) -> Result<Vec<Result<String, String>>, ClientError> {
    let wire: SignWire = decode(response, crate::PROJECT_REPORT_RESPONSE_LIMIT)?;
    if wire.mode != "signed" || wire.urls.len() + wire.refused.len() != request.identities.len() {
        return Err(unreadable());
    }
    request
        .identities
        .iter()
        .map(|identity| {
            let key = identity.locator();
            match (wire.urls.get(&key), wire.refused.get(&key)) {
                (Some(url), None) => {
                    crate::grid_models::signed_object_locator(
                        url,
                        &identity.bucket,
                        &identity.object,
                        crate::grid_models::MAX_BYTES,
                    )
                    .map_err(|_| unreadable())?;
                    Ok(Ok(url.clone()))
                }
                (None, Some(reason)) if bounded(reason, 512, false) => Ok(Err(reason.clone())),
                _ => Err(unreadable()),
            }
        })
        .collect()
}

/// Sign every identity through ds-brain's authorized `sign_url` boundary
/// (membership-checked, one-hour V4 GETs), then fetch each signed object and
/// hand its exact bytes and SHA-256 to `sink` one at a time, so a large plan
/// never sits in memory. A fresh signature is spent here and never returned.
/// `sign_url` is a native `/report` action outside the declared profile
/// vocabulary, sent on the fixed route like `export_reports_only`.
pub(crate) fn download_artifacts<T: crate::AuthorizedTransport>(
    profile: &crate::ClientProfile,
    transport: &mut T,
    authority: crate::authorized::Authority<'_>,
    ds_project: &str,
    request: &ArtifactDownloadRequest,
    sink: &mut dyn FnMut(ArtifactFile) -> Result<(), ClientError>,
) -> Result<Vec<(ArtifactIdentity, ArtifactOutcome)>, ClientError> {
    use sha2::{Digest, Sha256};
    let response = transport.project_report(crate::ProjectReportCall {
        profile,
        ds_project,
        operation: ProjectReportOperation::SignUrls(request),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(deleted_backup_refusal(&response));
    }
    let signed = decode_signed(response, request)?;
    let mut outcomes = Vec::with_capacity(signed.len());
    for (identity, signed) in request.identities.iter().zip(signed) {
        let outcome = match signed {
            Err(reason) => ArtifactOutcome::Refused { reason },
            Ok(url) => {
                let call = crate::grid_models::signed_object_locator(
                    &url,
                    &identity.bucket,
                    &identity.object,
                    crate::grid_models::MAX_BYTES,
                )?;
                match transport.grid_model_bytes(call) {
                    Ok(mut response) if response.status == 200 && !response.body.is_empty() => {
                        let bytes = std::mem::take(&mut response.body);
                        let sha256 = format!("{:x}", Sha256::digest(&bytes));
                        let size = bytes.len() as u64;
                        sink(ArtifactFile {
                            identity: identity.clone(),
                            bytes,
                            sha256: sha256.clone(),
                        })?;
                        ArtifactOutcome::Fetched {
                            bytes: size,
                            sha256,
                        }
                    }
                    Ok(response) => ArtifactOutcome::Failed {
                        reason: format!("storage answered HTTP {}", response.status),
                    },
                    Err(error) => ArtifactOutcome::Failed {
                        reason: format!("{error:?}"),
                    },
                }
            }
        };
        outcomes.push((identity.clone(), outcome));
    }
    Ok(outcomes)
}

// ---------------------------------------------------------------------------
// Shared bounds
// ---------------------------------------------------------------------------

fn decode<T: for<'de> Deserialize<'de>>(
    response: TransportResponse,
    limit: usize,
) -> Result<T, ClientError> {
    if response.body.len() > limit {
        return Err(unreadable());
    }
    serde_json::from_slice(&response.body).map_err(|_| unreadable())
}

fn bounded(value: &str, max: usize, allow_empty: bool) -> bool {
    (allow_empty || !value.is_empty())
        && value.trim() == value
        && value.chars().count() <= max
        && !value
            .chars()
            .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
}

fn opt_bounded(value: Option<&str>, max: usize) -> bool {
    value.is_none_or(|value| bounded(value, max, true))
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the response is outside the closed project report contract",
    )
}

#[cfg(test)]
#[path = "project_report_tests.rs"]
mod tests;
