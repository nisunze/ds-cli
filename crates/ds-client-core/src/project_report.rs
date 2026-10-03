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
            Self::Compounded(_) | Self::ExportReports(_) => {
                crate::transport::PROJECT_REPORT_TIMEOUT_SECONDS
            }
            _ => crate::transport::PROJECT_REPORT_READ_TIMEOUT_SECONDS,
        }
    }

    pub(crate) fn body(&self, project: &str) -> Vec<u8> {
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

/// The longest `kind` tag a status row may carry.
const MAX_STATUS_KIND_CHARS: usize = 64;

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
    let wire: StatusWire = decode(response, limit)?;
    if wire.transformers.len() > MAX_LIST {
        return Err(unreadable());
    }
    let mut rows = Vec::with_capacity(wire.transformers.len());
    for row in wire.transformers {
        let Some(object) = row.as_object() else {
            return Err(unreadable());
        };
        let Some(name) = object.get("name").and_then(Value::as_str) else {
            return Err(unreadable());
        };
        if !bounded(name, PROJECT_REPORT_MAX_TRANSFORMER_CHARS, false) {
            return Err(unreadable());
        }
        let name = name.to_owned();
        let kind = match object.get("kind") {
            None | Some(Value::Null) => None,
            Some(Value::String(kind)) if bounded(kind, MAX_STATUS_KIND_CHARS, false) => {
                Some(kind.clone())
            }
            Some(_) => return Err(unreadable()),
        };
        rows.push(TransformerStatusRow { name, kind, row });
    }
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
