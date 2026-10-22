//! Fixed selected-project operations behind ds-brain's `/report` action gate.
//!
//! These are the map-independent project operations: the compounded report
//! deliverable, the cloud computation of individual transformer reports, the
//! registry of compounded archives, the project transformer inventory, the
//! transformer status rows every Design read is built from, and reversible
//! transformer retirement. None of them consumes a Desktop room, a
//! browser store, or a visible map project; the selected
//! project is injected by the host from its audience-fenced context and the
//! core owns every request key, action spelling, bound, and deadline.

use std::collections::HashSet;

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
const MAX_CAUSE_CODE_CHARS: usize = 128;
const MAX_LIST: usize = 2_000;
const MAX_ARCHIVES: usize = 64;

const COMPOUNDED_TRANSFORMER: &str = "combined_transformer";
const COMPOUNDED_REPORT_TYPE: &str = "compounded_reports";
pub(crate) const ACTION_COMPOUNDED: &str = "download_transfo";
pub(crate) const ACTION_COMPOUNDED_LIST: &str = "list_compounded_reports";
pub(crate) const ACTION_INVENTORY: &str = "transformer_inventory";
pub(crate) const ACTION_LIST_STATUS: &str = "list_transformers_status";
pub(crate) const ACTION_RETIRE: &str = "retire_transformer";
pub(crate) const ACTION_RESTORE: &str = "restore_transformer";
/// Deleted-transformer recovery: read one verified delete backup, or create the
/// deleted working document again from it. Like `export_reports_only` below,
/// ds-brain serves both on the fixed `/report` route while they are not
/// declared in [`crate::PROJECT_REPORT_ACTIONS`]; declaring them would be a
/// client-profile schema bump (a release event), so the core sends them on the
/// same route without widening the profile identity.
pub(crate) const ACTION_READ_DELETED_BACKUP: &str = "read_transformer_backup";
pub(crate) const ACTION_RESTORE_DELETED: &str = "restore_deleted_transformer";
/// The authorized signer for stored objects a status read or listing named.
pub(crate) const ACTION_SIGN_URL: &str = "sign_url";
/// The cloud individual-report computation. It is one of the four browser-lane
/// `/report` actions the routing descriptor names as served by ds-brain but
/// not yet declared in [`crate::PROJECT_REPORT_ACTIONS`]; declaring it there
/// is a descriptor edit plus a profile-schema bump (a release event that
/// re-keys every installed catalogue and device credential), so the core
/// sends it on the same fixed route without widening the profile identity.
pub(crate) const ACTION_EXPORT_REPORTS: &str = "export_reports_only";
/// The preview-pinned pole-material catalogue repair (`ds design materials
/// preview|apply`): the kernel builds the fenced request
/// (`design_config::material_propagation_request`) and judges the receipt;
/// this route carries it under `material_propagation`, as the browser's
/// `propagateConfigMaterials` did. Gated on `network.template.propagate`.
pub(crate) const ACTION_PROPAGATE_MATERIALS: &str = "propagate_config_materials";

/// Folder level for individual transformer artifacts inside a compounded ZIP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFileLevel {
    Transformer,
    Sector,
    District,
    Root,
}

impl ReportFileLevel {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Transformer => "transformer",
            Self::Sector => "sector",
            Self::District => "district",
            Self::Root => "root",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "transformer" => Some(Self::Transformer),
            "sector" => Some(Self::Sector),
            "district" => Some(Self::District),
            "root" => Some(Self::Root),
            _ => None,
        }
    }
}

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

/// Match ds-brain's `CanonicalTransformerReferenceName`: lowercase ASCII
/// reference keys, separators/punctuation folded to `_`, no leading digit,
/// and the legacy aggregate aliases collapsed to one key.
///
/// One compounded report request: an explicit scope (empty = every active
/// saved transformer), the archive layout, and the force bit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundedReportRequest {
    transformers: TransformerSet,
    file_level: ReportFileLevel,
    combine_per_district: bool,
    force: bool,
    composition: Option<Value>,
}

impl CompoundedReportRequest {
    pub const fn new(
        transformers: TransformerSet,
        file_level: ReportFileLevel,
        combine_per_district: bool,
        force: bool,
    ) -> Self {
        Self {
            transformers,
            file_level,
            combine_per_district,
            force,
            composition: None,
        }
    }

    /// The same JSON object the browser sends to the governed orchestrator.
    /// It stays nested under composition: never an action/project/body override.
    pub fn from_composition(composition: Value) -> Result<Self, ClientError> {
        if !composition.is_object() || composition.to_string().len() > 64 * 1024 {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "composition must be one JSON object of at most 64 KiB",
            ));
        }
        let names: Vec<String> = serde_json::from_value(composition["transformers"].clone())
            .map_err(|_| {
                ClientError::new(
                    ErrorKind::InvalidInput,
                    "composition requires a transformers array",
                )
            })?;
        let transformers = TransformerSet::new(names)?;
        if transformers
            .names()
            .iter()
            .any(|name| ds_command_kernel::report::is_reserved_report_identity(name))
        {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "composition requires real transformer identities",
            ));
        }
        let mut request = Self::new(
            transformers,
            ReportFileLevel::Transformer,
            composition["archive_layout"]["combine_per_group"]
                .as_bool()
                .unwrap_or(false),
            composition["force"].as_bool().unwrap_or(false),
        );
        request.composition = Some(composition);
        Ok(request)
    }

    pub fn composition(&self) -> Option<&Value> {
        self.composition.as_ref()
    }

    pub fn transformers(&self) -> &TransformerSet {
        &self.transformers
    }
    pub const fn file_level(&self) -> ReportFileLevel {
        self.file_level
    }
    pub const fn combine_per_district(&self) -> bool {
        self.combine_per_district
    }
    pub const fn force(&self) -> bool {
        self.force
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
    Compounded(&'a CompoundedReportRequest),
    CompoundedList,
    /// Compute the individual reports of exact transformers in the cloud and
    /// publish them to the project. The set is never empty: ds-brain routes an
    /// empty list to the retired generic proxy instead of refusing it.
    ExportReports(&'a TransformerSet),
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
            Self::Compounded(_) => ACTION_COMPOUNDED,
            Self::CompoundedList => ACTION_COMPOUNDED_LIST,
            Self::ExportReports(_) => ACTION_EXPORT_REPORTS,
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

    /// The compounded deliverable and the individual-report computation are
    /// fanned out by ds-brain's Go orchestrator to the cloud reporter only on
    /// the Fast lane; every other action is lane-neutral.
    pub(crate) const fn processing_lane(&self) -> Option<&'static str> {
        match self {
            Self::Compounded(_) | Self::ExportReports(_) => Some("fast"),
            _ => None,
        }
    }

    pub(crate) const fn timeout_seconds(&self) -> u64 {
        match self {
            Self::ListStatus(_) => {
                ds_command_kernel::transformer_status_store::READ_TIMEOUT_SECONDS
            }
            Self::Compounded(_) | Self::ExportReports(_) => {
                crate::transport::PROJECT_REPORT_TIMEOUT_SECONDS
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
            Self::Compounded(request) => {
                body.insert(
                    "transfo".into(),
                    Value::String(COMPOUNDED_TRANSFORMER.into()),
                );
                body.insert(
                    "report_type".into(),
                    Value::String(COMPOUNDED_REPORT_TYPE.into()),
                );
                if let Some(composition) = &request.composition {
                    body.insert("composition".into(), composition.clone());
                    return serde_json::to_vec(&body).expect("a JSON body encodes");
                }
                body.insert(
                    "transformers".into(),
                    Value::Array(
                        request
                            .transformers
                            .names()
                            .iter()
                            .map(|name| Value::String(name.clone()))
                            .collect(),
                    ),
                );
                let mut layout = Map::new();
                layout.insert(
                    "file_level".into(),
                    Value::String(request.file_level.token().into()),
                );
                layout.insert(
                    "combine_per_district".into(),
                    Value::Bool(request.combine_per_district),
                );
                body.insert("archive_layout".into(), Value::Object(layout));
                if request.force {
                    body.insert("force".into(), Value::Bool(true));
                }
            }
            Self::CompoundedList => {
                body.insert("include_artifacts".into(), Value::Bool(true));
            }
            Self::ExportReports(set) => {
                body.insert("transformers".into(), names_value(set));
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
// Compounded report receipt
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundedReportStatus {
    Success,
    Partial,
}

impl CompoundedReportStatus {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Partial => "partial",
        }
    }
}

/// One bounded, typed reason an individual transformer's artifacts are absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingIndividualArtifactCause {
    transformer: String,
    code: String,
    detail: String,
}

impl MissingIndividualArtifactCause {
    pub fn transformer(&self) -> &str {
        &self.transformer
    }
    pub fn code(&self) -> &str {
        &self.code
    }
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// The publication receipt of one compounded report run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundedReportReceipt {
    composition: Option<Value>,
    project: String,
    status: CompoundedReportStatus,
    prefix: String,
    archive_paths: Vec<String>,
    cached: bool,
    errors: Vec<String>,
    individual_artifact_transformer_count: u64,
    missing_individual_artifact_count: u64,
    missing_individual_artifacts: Vec<String>,
    missing_individual_artifact_causes: Vec<MissingIndividualArtifactCause>,
    registry_write_failed: bool,
    registry_write_error: Option<String>,
}

impl CompoundedReportReceipt {
    pub fn composition(&self) -> Option<&Value> {
        self.composition.as_ref()
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub const fn status(&self) -> CompoundedReportStatus {
        self.status
    }
    /// The archive stem; also the registry key for the archive list.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
    /// Cloud archive locators (`gs://…`) exactly as published; never signed.
    pub fn archive_paths(&self) -> &[String] {
        &self.archive_paths
    }
    pub const fn cached(&self) -> bool {
        self.cached
    }
    pub fn errors(&self) -> &[String] {
        &self.errors
    }
    pub const fn individual_artifact_transformer_count(&self) -> u64 {
        self.individual_artifact_transformer_count
    }
    pub const fn missing_individual_artifact_count(&self) -> u64 {
        self.missing_individual_artifact_count
    }
    pub fn missing_individual_artifacts(&self) -> &[String] {
        &self.missing_individual_artifacts
    }
    pub fn missing_individual_artifact_causes(&self) -> &[MissingIndividualArtifactCause] {
        &self.missing_individual_artifact_causes
    }
    pub const fn registry_write_failed(&self) -> bool {
        self.registry_write_failed
    }
    pub fn registry_write_error(&self) -> Option<&str> {
        self.registry_write_error.as_deref()
    }
}

#[derive(Deserialize)]
struct MissingIndividualArtifactCauseWire {
    transformer: String,
    code: String,
    detail: String,
}

#[derive(Deserialize)]
struct CompoundedWire {
    #[serde(default)]
    composition: Option<Value>,
    status: String,
    transformer: String,
    prefix: String,
    #[serde(default)]
    download_files_urls: Vec<String>,
    #[serde(default)]
    cached: bool,
    #[serde(default)]
    errors: Vec<String>,
    #[serde(default)]
    individual_artifact_transformer_count: u64,
    #[serde(default)]
    missing_individual_artifact_count: u64,
    #[serde(default)]
    missing_individual_artifacts: Vec<String>,
    #[serde(default)]
    missing_individual_artifact_causes: Vec<MissingIndividualArtifactCauseWire>,
    #[serde(default)]
    registry_write_failed: bool,
    #[serde(default)]
    registry_write_error: Option<String>,
}

pub(crate) fn decode_compounded(
    response: TransportResponse,
    limit: usize,
    project: &str,
) -> Result<CompoundedReportReceipt, ClientError> {
    let wire: CompoundedWire = decode(response, limit)?;
    let status = match wire.status.as_str() {
        "success" => CompoundedReportStatus::Success,
        "partial" => CompoundedReportStatus::Partial,
        _ => return Err(unreadable()),
    };
    if !valid_composition(wire.composition.as_ref())
        || wire.transformer != COMPOUNDED_TRANSFORMER
        || !bounded(&wire.prefix, 512, false)
        || wire.download_files_urls.len() > MAX_ARCHIVES
        || !wire
            .download_files_urls
            .iter()
            .all(|value| bounded(value, 2_048, false))
        || wire.errors.len() > MAX_LIST
        || !wire
            .errors
            .iter()
            .all(|value| bounded(value, MAX_TEXT_CHARS, true))
        || wire.missing_individual_artifacts.len() > MAX_LIST
        || !wire
            .missing_individual_artifacts
            .iter()
            .all(|value| bounded(value, PROJECT_REPORT_MAX_TRANSFORMER_CHARS, false))
        || wire.missing_individual_artifact_causes.len() > MAX_LIST
        || !wire
            .registry_write_error
            .as_deref()
            .is_none_or(|value| bounded(value, MAX_TEXT_CHARS, true))
    {
        return Err(unreadable());
    }
    let missing_names = wire
        .missing_individual_artifacts
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if usize::try_from(wire.missing_individual_artifact_count).ok()
        != Some(wire.missing_individual_artifacts.len())
        || missing_names.len() != wire.missing_individual_artifacts.len()
    {
        return Err(unreadable());
    }
    if !wire.missing_individual_artifact_causes.is_empty() {
        let cause_names = wire
            .missing_individual_artifact_causes
            .iter()
            .map(|cause| cause.transformer.as_str())
            .collect::<HashSet<_>>();
        if cause_names.len() != wire.missing_individual_artifact_causes.len()
            || cause_names != missing_names
        {
            return Err(unreadable());
        }
    }
    let mut causes = Vec::with_capacity(wire.missing_individual_artifact_causes.len());
    for cause in wire.missing_individual_artifact_causes {
        if !bounded(
            &cause.transformer,
            PROJECT_REPORT_MAX_TRANSFORMER_CHARS,
            false,
        ) || !bounded(&cause.code, MAX_CAUSE_CODE_CHARS, false)
            || !bounded(&cause.detail, MAX_TEXT_CHARS, true)
        {
            return Err(unreadable());
        }
        causes.push(MissingIndividualArtifactCause {
            transformer: cause.transformer,
            code: cause.code,
            detail: cause.detail,
        });
    }
    // The receipt is impossible when an archive URL is advertised for a run
    // that produced no individual artifact at all; ds-brain refuses that
    // before publication and ds-web refuses it on read. A cache hit reuses a
    // verified archive and legitimately omits the counts.
    if !wire.cached
        && wire.individual_artifact_transformer_count == 0
        && !wire.download_files_urls.is_empty()
    {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "the compounded report receipt advertises an archive with zero individual transformer artifacts",
        ));
    }
    Ok(CompoundedReportReceipt {
        composition: wire.composition,
        project: project.to_owned(),
        status,
        prefix: wire.prefix,
        archive_paths: wire.download_files_urls,
        cached: wire.cached,
        errors: wire.errors,
        individual_artifact_transformer_count: wire.individual_artifact_transformer_count,
        missing_individual_artifact_count: wire.missing_individual_artifact_count,
        missing_individual_artifacts: wire.missing_individual_artifacts,
        missing_individual_artifact_causes: causes,
        registry_write_failed: wire.registry_write_failed,
        registry_write_error: nonempty(wire.registry_write_error),
    })
}

// ---------------------------------------------------------------------------
// Cloud individual-report computation receipt
// ---------------------------------------------------------------------------

/// What the cloud did with one requested transformer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportReportOutcome {
    /// The reporter computed and published the individual report.
    Success,
    /// Nothing was computed: the artifacts were fresh, or the work was not
    /// claimed. `reason` says which.
    Skipped,
    /// The reporter refused or failed; `error` is ds-brain's bounded sentence.
    Error,
}

impl ExportReportOutcome {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Skipped => "skipped",
            Self::Error => "error",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "success" => Some(Self::Success),
            "skipped" => Some(Self::Skipped),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// One transformer's row of the cloud computation receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReportResult {
    name: String,
    outcome: ExportReportOutcome,
    reason: Option<String>,
    error: Option<String>,
}

impl ExportReportResult {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub const fn outcome(&self) -> ExportReportOutcome {
        self.outcome
    }
    /// Why nothing was computed; present only on a skipped row.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
    /// ds-brain's bounded failure sentence; present only on an error row.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

/// The receipt of one cloud individual-report computation: every requested
/// transformer, in request order, with what the cloud did to it. `partial`
/// is the route's own verdict (HTTP 207) and holds exactly when a row errored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReportsReceipt {
    project: String,
    partial: bool,
    results: Vec<ExportReportResult>,
}

impl ExportReportsReceipt {
    pub fn project(&self) -> &str {
        &self.project
    }
    /// `true` when at least one transformer errored (the route answered 207).
    pub const fn partial(&self) -> bool {
        self.partial
    }
    pub fn results(&self) -> &[ExportReportResult] {
        &self.results
    }
    fn names_with(&self, outcome: ExportReportOutcome) -> impl Iterator<Item = &str> {
        self.results
            .iter()
            .filter(move |row| row.outcome == outcome)
            .map(|row| row.name.as_str())
    }
    /// Transformers whose individual report the cloud computed and published.
    pub fn computed(&self) -> Vec<&str> {
        self.names_with(ExportReportOutcome::Success).collect()
    }
    pub fn skipped(&self) -> Vec<&str> {
        self.names_with(ExportReportOutcome::Skipped).collect()
    }
    pub fn failed(&self) -> Vec<&str> {
        self.names_with(ExportReportOutcome::Error).collect()
    }
}

/// ds-brain's batch envelope for `export_reports_only` (and its two list
/// siblings), exactly as `handleCombinedReportBatch` writes it. Closed: every
/// member is one this decoder reads, and the three name lists are the same
/// partition of `results` the decoder re-derives.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportReportsWire {
    message: String,
    included: Vec<String>,
    skipped: Vec<String>,
    failed: Vec<String>,
    results: Vec<ExportReportResultWire>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportReportResultWire {
    name: String,
    status: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
}

pub(crate) fn decode_export_reports(
    response: TransportResponse,
    limit: usize,
    project: &str,
    request: &TransformerSet,
) -> Result<ExportReportsReceipt, ClientError> {
    let partial = match response.status {
        200 => false,
        207 => true,
        _ => return Err(unreadable()),
    };
    let wire: ExportReportsWire = decode(response, limit)?;
    // The route's sentence is fixed per status; anything else is not this route.
    let expected_message = if partial {
        "Partial success"
    } else {
        "Success"
    };
    if wire.message != expected_message || wire.results.len() != request.len() {
        return Err(unreadable());
    }
    let mut results = Vec::with_capacity(wire.results.len());
    for (row, expected_name) in wire.results.into_iter().zip(request.names()) {
        if row.name != *expected_name
            || !opt_bounded(row.reason.as_deref(), MAX_TEXT_CHARS)
            || !opt_bounded(row.error_message.as_deref(), MAX_TEXT_CHARS)
        {
            return Err(unreadable());
        }
        let Some(outcome) = ExportReportOutcome::parse(&row.status) else {
            return Err(unreadable());
        };
        // A skipped row carries its reason and an error row its sentence;
        // the other members are absent on the wire, never both present.
        let (reason, error) = match outcome {
            ExportReportOutcome::Success => (None, None),
            ExportReportOutcome::Skipped => (nonempty(row.reason), None),
            ExportReportOutcome::Error => (None, nonempty(row.error_message)),
        };
        results.push(ExportReportResult {
            name: row.name,
            outcome,
            reason,
            error,
        });
    }
    let receipt = ExportReportsReceipt {
        project: project.to_owned(),
        partial,
        results,
    };
    // The three lists are the partition of `results`, in order, and the 207
    // is exactly "some row errored"; a receipt that says otherwise is not
    // one this route wrote.
    if !same_names(&wire.included, &receipt.computed())
        || !same_names(&wire.skipped, &receipt.skipped())
        || !same_names(&wire.failed, &receipt.failed())
        || partial != !wire.failed.is_empty()
    {
        return Err(unreadable());
    }
    Ok(receipt)
}

fn same_names(wire: &[String], names: &[&str]) -> bool {
    wire.len() == names.len() && wire.iter().zip(names).all(|(a, b)| a == b)
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
mod composition_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn archive_catalog_retains_server_authoring_json_and_does_not_invent_legacy_recipes() {
        let response = |body: Value| TransportResponse {
            status: 200,
            body: serde_json::to_vec(&body).unwrap(),
        };
        let template = json!({"schema":"ds.compounded-report/v1","transformers":[],
            "archive_layout":{"group_depth":null,"transformer_folders":true,"combine_per_group":false},"force":false});
        let schema = json!({"type":"object","additionalProperties":false});
        let read = decode_compounded_list(response(json!({"compounded_reports":[],"composition_template":template,"composition_schema":schema})), 1_000_000).unwrap();
        assert!(read.is_empty());
        assert_eq!(read.composition_template(), Some(&template));
        assert_eq!(read.composition_schema(), Some(&schema));
        let legacy =
            decode_compounded_list(response(json!({"compounded_reports":[]})), 1_000_000).unwrap();
        assert_eq!(legacy.composition_template(), None);
        assert_eq!(legacy.composition_schema(), None);
        for malformed in [json!([]), json!("x".repeat(65536))] {
            assert!(decode_compounded_list(
                response(json!({"compounded_reports":[],"composition_template":malformed})),
                1_000_000
            )
            .is_err());
        }
    }
    #[test]
    fn json_composition_is_nested_under_the_captured_project_and_action() {
        let composition = json!({"schema":"ds.compounded-report/v1","transformers":["tx_a"],
            "archive_layout":{"group_depth":null,"transformer_folders":true,"combine_per_group":false},
            "force":false});
        let request = CompoundedReportRequest::from_composition(composition.clone()).unwrap();
        let body: Value =
            serde_json::from_slice(&ProjectReportOperation::Compounded(&request).body("project_a"))
                .unwrap();
        assert_eq!(body["eds_project_id"], "project_a");
        assert_eq!(body["action"], "download_transfo");
        assert_eq!(body["composition"], composition);
        assert!(body.get("transformers").is_none());
        assert!(body.get("archive_layout").is_none());
        assert!(body.get("force").is_none());
        assert!(CompoundedReportRequest::from_composition(json!([])).is_err());
        assert!(CompoundedReportRequest::from_composition(
            json!({"transformers":["combined_transformer"]})
        )
        .is_err());
    }
}

#[cfg(test)]
mod deleted_backup_tests {
    use super::*;
    use serde_json::json;
    use sha2::{Digest, Sha256};

    const BUCKET: &str = "ds-backups";
    const OBJECT: &str = "transformers/1788000000-p1-tx_a.dsgrid";

    fn ok(body: Value) -> TransportResponse {
        TransportResponse {
            status: 200,
            body: serde_json::to_vec(&body).unwrap(),
        }
    }

    fn receipt(bytes: &[u8]) -> Value {
        json!({"bucket": BUCKET, "object": OBJECT, "generation": 17,
               "sha256": format!("{:x}", Sha256::digest(bytes)), "byte_length": bytes.len()})
    }

    fn read_body(bytes: &[u8], url: &str) -> Value {
        json!({"eds_project_id": "p1", "action": "read_transformer_backup", "name": "tx_a",
               "source_revision": 4, "layers": ["lv_poles", "lv_lines"], "backup": receipt(bytes),
               "download_url": url, "expires_at": "2026-10-09T10:15:00Z"})
    }

    fn signed() -> String {
        format!(
            "https://storage.googleapis.com/{BUCKET}/{OBJECT}?X-Goog-Signature=abc&generation=17"
        )
    }

    #[test]
    fn recovery_rides_the_fixed_route_without_widening_the_declared_profile_actions() {
        assert!(!crate::PROJECT_REPORT_ACTIONS.contains(&ACTION_READ_DELETED_BACKUP));
        assert!(!crate::PROJECT_REPORT_ACTIONS.contains(&ACTION_RESTORE_DELETED));
        let read = DeletedBackupRequest::read("tx_a".into(), OBJECT.into(), None, None).unwrap();
        let body: Value =
            serde_json::from_slice(&ProjectReportOperation::ReadDeletedBackup(&read).body("p1"))
                .unwrap();
        assert_eq!(
            body,
            json!({"action": "read_transformer_backup", "eds_project_id": "p1",
                   "transformers": ["tx_a"], "backup": {"object": OBJECT}})
        );
        let digest = "a".repeat(64);
        let restore =
            DeletedBackupRequest::restore("tx_a".into(), OBJECT.into(), 17, digest.clone())
                .unwrap();
        let body: Value =
            serde_json::from_slice(&ProjectReportOperation::RestoreDeleted(&restore).body("p1"))
                .unwrap();
        assert_eq!(body["action"], "restore_deleted_transformer");
        assert_eq!(
            body["backup"],
            json!({"object": OBJECT, "generation": 17, "sha256": digest})
        );
    }

    #[test]
    fn requests_hold_the_servers_bounds_before_a_round_trip() {
        for (object, generation, sha) in [
            ("", None, None),
            (" transformers/a.dsgrid", None, None),
            ("transformers\\a.dsgrid", None, None),
            (OBJECT, Some(0), None),
            (OBJECT, None, Some("ABC")),
        ] {
            assert!(
                DeletedBackupRequest::read(
                    "tx_a".into(),
                    object.into(),
                    generation,
                    sha.map(Into::into)
                )
                .is_err(),
                "{object:?} {generation:?} {sha:?}"
            );
        }
        assert!(DeletedBackupRequest::read(String::new(), OBJECT.into(), None, None).is_err());
    }

    #[test]
    fn a_read_is_admitted_only_for_the_asked_object_and_an_exact_storage_locator() {
        let bytes = b"native dsgrid bytes".to_vec();
        let request = DeletedBackupRequest::read("tx_a".into(), OBJECT.into(), None, None).unwrap();
        let read =
            decode_deleted_backup_read(ok(read_body(&bytes, &signed())), 1 << 20, "p1", &request)
                .expect("verified read");
        assert_eq!(read.backup().generation(), 17);
        assert_eq!(read.layers(), ["lv_poles", "lv_lines"]);
        assert!(read.bytes().is_none());
        assert!(!format!("{:?}", read.backup()).contains("Signature"));
        let gs = DeletedBackupRequest::read(
            "tx_a".into(),
            format!("gs://{BUCKET}/{OBJECT}"),
            Some(17),
            None,
        )
        .unwrap();
        assert!(
            decode_deleted_backup_read(ok(read_body(&bytes, &signed())), 1 << 20, "p1", &gs)
                .is_ok()
        );
        for url in [
            format!("https://example.com/{BUCKET}/{OBJECT}?sig=1"),
            format!("https://storage.googleapis.com/{BUCKET}/transformers/other.dsgrid?sig=1"),
            format!("https://storage.googleapis.com/other/{OBJECT}?sig=1"),
            format!("http://storage.googleapis.com/{BUCKET}/{OBJECT}?sig=1"),
        ] {
            assert_eq!(
                decode_deleted_backup_read(ok(read_body(&bytes, &url)), 1 << 20, "p1", &request)
                    .unwrap_err()
                    .kind(),
                ErrorKind::UnreadableResponse,
                "{url}"
            );
        }
        let pinned_elsewhere =
            DeletedBackupRequest::read("tx_a".into(), OBJECT.into(), Some(18), None).unwrap();
        assert!(decode_deleted_backup_read(
            ok(read_body(&bytes, &signed())),
            1 << 20,
            "p1",
            &pinned_elsewhere
        )
        .is_err());
        assert!(decode_deleted_backup_read(
            ok(read_body(&bytes, &signed())),
            1 << 20,
            "p2",
            &request
        )
        .is_err());
    }

    struct BackupBytes {
        status: u16,
        body: Vec<u8>,
        calls: usize,
    }
    macro_rules! no_other_call {
        ($name:ident, $call:ident) => {
            fn $name(
                &mut self,
                _: crate::$call<'_>,
            ) -> Result<crate::TransportResponse, crate::TransportError> {
                panic!("a backup fetch made an unrelated call")
            }
        };
    }
    impl crate::AuthorizedTransport for BackupBytes {
        no_other_call!(list_projects, ProjectListCall);
        no_other_call!(transformer_context, TransformerContextCall);
        no_other_call!(project_forms, ProjectFormsCall);
        no_other_call!(project_form_editor, ProjectFormEditorCall);
        no_other_call!(solar_snapshot, SolarSnapshotCall);
        no_other_call!(survey_query, SurveyQueryCall);
        no_other_call!(survey_entries_select, SurveyEntriesSelectCall);
        no_other_call!(survey_entries_changes, SurveyEntriesChangesCall);
        no_other_call!(survey_entry_create, SurveyEntryCreateCall);
        no_other_call!(project_data, ProjectDataCall);
        no_other_call!(upload_bytes, UploadBytesCall);
        no_other_call!(survey_control, SurveyControlCall);
        no_other_call!(styles, StylesCall);
        no_other_call!(layers, LayersCall);
        no_other_call!(tiles, TileCall);
        no_other_call!(project_report, ProjectReportCall);
        fn grid_model_bytes(
            &mut self,
            call: crate::grid_models::DownloadCall<'_>,
        ) -> Result<crate::TransportResponse, crate::TransportError> {
            self.calls += 1;
            assert_eq!(call.url(), signed());
            Ok(crate::TransportResponse::new(
                self.status,
                self.body.clone(),
            ))
        }
    }

    #[test]
    fn fetched_backup_bytes_are_admitted_only_at_the_receipts_length_and_digest() {
        let bytes = b"native dsgrid bytes".to_vec();
        let request = DeletedBackupRequest::read("tx_a".into(), OBJECT.into(), None, None).unwrap();
        let decode = || {
            decode_deleted_backup_read(ok(read_body(&bytes, &signed())), 1 << 20, "p1", &request)
                .unwrap()
        };
        let mut good = BackupBytes {
            status: 200,
            body: bytes.clone(),
            calls: 0,
        };
        let mut read = decode();
        fetch_deleted_backup(&mut good, &mut read).expect("verified bytes");
        assert_eq!(read.bytes(), Some(bytes.as_slice()));
        let mut altered = bytes.clone();
        altered[0] ^= 1;
        for (status, body) in [
            (200, altered),
            (200, bytes[1..].to_vec()),
            (403, bytes.clone()),
        ] {
            let mut transport = BackupBytes {
                status,
                body,
                calls: 0,
            };
            let mut read = decode();
            assert!(fetch_deleted_backup(&mut transport, &mut read).is_err());
            assert!(read.bytes().is_none());
            assert_eq!(transport.calls, 1);
        }
    }

    #[test]
    fn a_restoration_is_the_asked_transformer_from_the_pinned_backup() {
        let bytes = b"native dsgrid bytes".to_vec();
        let backup = receipt(&bytes);
        let digest = backup["sha256"].as_str().unwrap().to_owned();
        let request =
            DeletedBackupRequest::restore("tx_a".into(), OBJECT.into(), 17, digest).unwrap();
        let body = |state: &str, generation: i64| {
            let mut backup = backup.clone();
            backup["generation"] = json!(generation);
            json!({"eds_project_id": "p1", "action": "restore_deleted_transformer",
                   "restoration": {"name": "tx_a", "state": state, "restored_at": 1788000000000_i64,
                                   "restored_by": "owner@example.com", "source_revision": 4,
                                   "version": 5, "layers": ["lv_poles"], "backup": backup}})
        };
        let restored = decode_deleted_restoration(ok(body("active", 17)), 1 << 20, "p1", &request)
            .expect("restored");
        assert_eq!(restored.version(), 5);
        assert_eq!(restored.backup().generation(), 17);
        assert!(
            decode_deleted_restoration(ok(body("retired", 17)), 1 << 20, "p1", &request).is_ok()
        );
        assert!(
            decode_deleted_restoration(ok(body("deleted", 17)), 1 << 20, "p1", &request).is_err()
        );
        assert!(
            decode_deleted_restoration(ok(body("active", 18)), 1 << 20, "p1", &request).is_err()
        );
    }

    #[test]
    fn recovery_refusals_keep_the_closed_code_and_the_servers_sentence() {
        let refused = |status: u16, body: &str| {
            deleted_backup_refusal(&TransportResponse {
                status,
                body: body.as_bytes().to_vec(),
            })
        };
        let exists = refused(
            409,
            r#"{"error_code":"transformer_exists","detail":"a transformer document with this name exists"}"#,
        );
        assert_eq!(exists.kind(), ErrorKind::InvalidInput);
        assert_eq!(
            exists.service_refusal().unwrap().code(),
            Some("transformer_exists")
        );
        let denied = refused(403, r#"{"detail":"requires transformer.delete"}"#);
        assert_eq!(denied.kind(), ErrorKind::AuthenticationRejected);
        assert_eq!(denied.service_refusal().unwrap().code(), None);
        let unknown = refused(422, r#"{"error_code":"made_up","detail":"x"}"#);
        assert_eq!(unknown.service_refusal().unwrap().code(), None);
        assert_eq!(refused(503, "").kind(), ErrorKind::Transient);
    }
}

#[cfg(test)]
mod artifact_download_tests {
    use super::*;
    use serde_json::json;
    use sha2::{Digest, Sha256};

    const BUCKET: &str = "ds-prod.firebasestorage.app";

    #[test]
    fn identities_come_from_gs_storage_and_legacy_firebase_locators_without_their_bearer_query() {
        let gs =
            artifact_identity(&format!("gs://{BUCKET}/p1/report_lv/TX 1/report.xlsx")).unwrap();
        assert_eq!(gs.object(), "p1/report_lv/TX 1/report.xlsx");
        let signed = artifact_identity(&format!(
            "https://storage.googleapis.com/{BUCKET}/p1/report_lv/TX%201/report.xlsx?X-Goog-Signature=abc"
        ))
        .unwrap();
        assert_eq!(signed, gs);
        let firebase = artifact_identity(&format!(
            "https://firebasestorage.googleapis.com/v0/b/{BUCKET}/o/p1%2Freport_lv%2FTX%201%2Freport.xlsx?alt=media&token=secret"
        ))
        .unwrap();
        assert_eq!(firebase, gs);
        assert!(!firebase.locator().contains("token"));
        // The browser form older report rows list (Kayonza-Gatsibo's plan) is
        // the same object, under the same scheme and authority checks.
        let browser = artifact_identity(&format!(
            "https://storage.cloud.google.com/{BUCKET}/p1/report_lv/TX%201/report.xlsx"
        ))
        .unwrap();
        assert_eq!(browser, gs);
        for bad in [
            "http://storage.cloud.google.com/bucket/a.pdf",
            "https://storage.cloud.google.com:8443/bucket/a.pdf",
            "https://user@storage.cloud.google.com/bucket/a.pdf",
            "https://storage.cloud.google.com/bucket",
            "https://storage.cloud.google.com/bucket/../a.pdf",
            "",
            "gs://bucket",
            "gs://bucket/a/../b.pdf",
            "gs://bucket//b.pdf",
            "http://storage.googleapis.com/bucket/a.pdf",
            "https://example.com/bucket/a.pdf",
            "https://storage.googleapis.com/bucket/a%ZZ.pdf",
            "file:///etc/passwd",
        ] {
            assert!(artifact_identity(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_request_names_each_object_once_and_refuses_an_unusable_or_unbounded_plan() {
        let one = format!("gs://{BUCKET}/p1/a.pdf");
        let request = ArtifactDownloadRequest::new([one.clone(), one.clone()]).unwrap();
        assert_eq!(request.identities().len(), 1);
        assert!(ArtifactDownloadRequest::new(Vec::<String>::new()).is_err());
        assert!(ArtifactDownloadRequest::new([one.clone(), "nope".into()]).is_err());
        let many: Vec<String> = (0..=MAX_ARTIFACT_DOWNLOADS)
            .map(|index| format!("gs://{BUCKET}/p1/{index}.pdf"))
            .collect();
        assert!(ArtifactDownloadRequest::new(many).is_err());
        let body: Value =
            serde_json::from_slice(&ProjectReportOperation::SignUrls(&request).body("p1")).unwrap();
        assert_eq!(
            body,
            json!({"action": "sign_url", "eds_project_id": "p1", "paths": [one]})
        );
        assert!(!crate::PROJECT_REPORT_ACTIONS.contains(&ACTION_SIGN_URL));
    }

    struct SignAndFetch {
        signed: Value,
        objects: std::collections::BTreeMap<String, (u16, Vec<u8>)>,
        fetched: Vec<String>,
    }
    macro_rules! no_other_call {
        ($name:ident, $call:ident) => {
            fn $name(
                &mut self,
                _: crate::$call<'_>,
            ) -> Result<crate::TransportResponse, crate::TransportError> {
                panic!("an artifact download made an unrelated call")
            }
        };
    }
    impl crate::AuthorizedTransport for SignAndFetch {
        no_other_call!(list_projects, ProjectListCall);
        no_other_call!(transformer_context, TransformerContextCall);
        no_other_call!(project_forms, ProjectFormsCall);
        no_other_call!(project_form_editor, ProjectFormEditorCall);
        no_other_call!(solar_snapshot, SolarSnapshotCall);
        no_other_call!(survey_query, SurveyQueryCall);
        no_other_call!(survey_entries_select, SurveyEntriesSelectCall);
        no_other_call!(survey_entries_changes, SurveyEntriesChangesCall);
        no_other_call!(survey_entry_create, SurveyEntryCreateCall);
        no_other_call!(project_data, ProjectDataCall);
        no_other_call!(upload_bytes, UploadBytesCall);
        no_other_call!(survey_control, SurveyControlCall);
        no_other_call!(styles, StylesCall);
        no_other_call!(layers, LayersCall);
        no_other_call!(tiles, TileCall);
        fn project_report(
            &mut self,
            call: crate::ProjectReportCall<'_>,
        ) -> Result<crate::TransportResponse, crate::TransportError> {
            assert_eq!(call.operation.action(), "sign_url");
            Ok(crate::TransportResponse::new(
                200,
                serde_json::to_vec(&self.signed).unwrap(),
            ))
        }
        fn grid_model_bytes(
            &mut self,
            call: crate::grid_models::DownloadCall<'_>,
        ) -> Result<crate::TransportResponse, crate::TransportError> {
            self.fetched.push(call.url().to_owned());
            let (status, body) = self.objects.get(call.url()).cloned().expect("signed url");
            Ok(crate::TransportResponse::new(status, body))
        }
    }

    fn signed_url(object: &str) -> String {
        format!("https://storage.googleapis.com/{BUCKET}/{object}?X-Goog-Signature=fresh")
    }

    type Downloaded = (
        Result<Vec<(ArtifactIdentity, ArtifactOutcome)>, ClientError>,
        Vec<ArtifactFile>,
    );

    fn download(transport: &mut SignAndFetch, request: &ArtifactDownloadRequest) -> Downloaded {
        let profile = crate::data_distribution::tests::profile_for_tests();
        let mut files = Vec::new();
        let outcome = download_artifacts(
            &profile,
            transport,
            crate::authorized::Authority {
                bearer_token: "bearer",
                uid: "uid",
                email: "user@example.com",
                device_id: None,
            },
            "p1",
            request,
            &mut |file| {
                files.push(file);
                Ok(())
            },
        );
        (outcome, files)
    }

    #[test]
    fn every_listed_artifact_is_signed_fetched_byte_exact_refused_or_failed_by_name() {
        let report = b"PK\x03\x04 workbook".to_vec();
        let gs = |object: &str| format!("gs://{BUCKET}/{object}");
        let request = ArtifactDownloadRequest::new([
            gs("p1/report_lv/tx_a/report.xlsx"),
            gs("p1/report_lv/tx_b/report.xlsx"),
            gs("other/report_lv/tx_c/report.xlsx"),
        ])
        .unwrap();
        let mut transport = SignAndFetch {
            signed: json!({"mode": "signed",
                "urls": {gs("p1/report_lv/tx_a/report.xlsx"): signed_url("p1/report_lv/tx_a/report.xlsx"),
                         gs("p1/report_lv/tx_b/report.xlsx"): signed_url("p1/report_lv/tx_b/report.xlsx")},
                "refused": {gs("other/report_lv/tx_c/report.xlsx"): "not a member of the project"}}),
            objects: [
                (
                    signed_url("p1/report_lv/tx_a/report.xlsx"),
                    (200, report.clone()),
                ),
                (
                    signed_url("p1/report_lv/tx_b/report.xlsx"),
                    (404, Vec::new()),
                ),
            ]
            .into_iter()
            .collect(),
            fetched: Vec::new(),
        };
        let (outcomes, files) = download(&mut transport, &request);
        let outcomes = outcomes.expect("download");
        let digest = format!("{:x}", Sha256::digest(&report));
        assert_eq!(
            outcomes[0].1,
            ArtifactOutcome::Fetched {
                bytes: report.len() as u64,
                sha256: digest.clone()
            }
        );
        assert!(
            matches!(&outcomes[1].1, ArtifactOutcome::Failed { reason } if reason.contains("404"))
        );
        assert_eq!(
            outcomes[2].1,
            ArtifactOutcome::Refused {
                reason: "not a member of the project".into()
            }
        );
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].bytes, report);
        assert_eq!(files[0].sha256, digest);
        assert_eq!(
            transport.fetched.len(),
            2,
            "a refused identity is never fetched"
        );
    }

    #[test]
    fn a_signer_answer_that_omits_an_identity_or_signs_another_object_is_unreadable() {
        let gs = format!("gs://{BUCKET}/p1/a.pdf");
        let request = ArtifactDownloadRequest::new([gs.clone()]).unwrap();
        for signed in [
            json!({"mode": "signed", "urls": {}}),
            json!({"mode": "signed", "urls": {gs.clone(): signed_url("p1/b.pdf")}}),
            json!({"mode": "signed", "urls": {gs.clone(): "https://example.com/p1/a.pdf"}}),
            json!({"mode": "unsigned", "urls": {gs.clone(): signed_url("p1/a.pdf")}}),
        ] {
            let mut transport = SignAndFetch {
                signed,
                objects: Default::default(),
                fetched: Vec::new(),
            };
            let (outcome, files) = download(&mut transport, &request);
            assert_eq!(outcome.unwrap_err().kind(), ErrorKind::UnreadableResponse);
            assert!(files.is_empty() && transport.fetched.is_empty());
        }
    }
}
