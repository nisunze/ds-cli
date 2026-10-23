//! The Rust Solar path: prepare on the input page, then run from prepared input.
//!
//! This module is the native application boundary `ds-solar` draws. Two groups
//! of commands, and the difference between them is the point:
//!
//! * [`solar_native_prepare`] may reach the network. For each *selected* city it
//!   derives the weather key from that city's coordinates and the deployment's
//!   pinned provider, reads the local cache, makes at most one bounded
//!   authenticated call on a miss, and commits an immutable
//!   `PreparedSolarCityInput` into the approved workspace.
//! * [`solar_native_calculate`] runs the fixed offline task in the installed
//!   Solar executable. Its request contains prepared inputs, with no provider
//!   or acquisition capability. An input that has not been
//!   prepared comes back as `city_input_not_prepared`, and the page sends the
//!   operator to the field that is missing.
//!
//! The host owns admission, progress, cancellation and verified local commit;
//! the executable owns the calculation and report algorithms.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::NativeHost as AppHandle;
pub(crate) mod city_finals;
pub(crate) mod portfolio_jobs;
use ds_solar_contracts::RepresentativeGraphArtifactReceipt;
use ds_solar_contracts::artifact_names::{
    APD_DRAFT_EN_NAME, FINANCIAL_DRAFT_EN_NAME, FINANCIAL_DRAFT_NAME, JSON_MEDIA_TYPE,
    MARKDOWN_MEDIA_TYPE, NETWORK_DRAFT_EN_NAME, NETWORK_DRAFT_NAME, PARITY_DRAFT_NAME,
    PLANT_DRAFT_EN_NAME, PLANT_DRAFT_NAME, PORTFOLIO_DRAFT_NAME as RUNTIME_PORTFOLIO_DRAFT_NAME,
    PRODUCT_APD_NAME, REPORT_INPUT_NAME,
};
use ds_solar_contracts::{
    CityIdentity, PortfolioDocumentFormat, PortfolioDocumentOptions, PortfolioIdentity,
    PortfolioLanguage, PortfolioReportIntent, PortfolioRepresentativeStrategy, PortfolioRunInput,
    PreparedSolarCityInput, ReferenceCacheReceipt, ReferencePin, RunOptions, SnapshotRevision,
    SolarCityInput, SolarError, SystemType,
};
#[cfg(test)]
use ds_solar_contracts::{
    WeatherPin,
    artifact_names::{PORTFOLIO_CALCULATION_NAME, PORTFOLIO_CONTRIBUTION_NAME},
};
use ds_solar_host::Concurrency;
use ds_solar_host::{cache::DirectoryReferenceCache, cancel::CancelToken};
use ds_solar_reference_host::{
    BundleResolution, HttpReferenceProvider, ReferenceBundleProvider, ReferencePreparer, Refresh,
};
#[cfg(test)]
use ds_solar_runtime::run_cities;
#[cfg(test)]
use ds_solar_runtime::{CalculationOptions, SolarRuntime};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
mod bundles;
mod engine_batch;
mod finishing;
mod portfolio_finals;
mod publication;
pub use bundles::PortfolioReportBundleCommand;
pub use finishing::{ReportFinishCommand, ReportFinishSource};
pub use portfolio_finals::{PortfolioFinalImportCommand, PortfolioFinalSelector};
pub fn solar_native_finish_report(
    app: AppHandle,
    command: ReportFinishCommand,
) -> Result<serde_json::Value, String> {
    finishing::finish(app, command)
}

pub fn solar_native_publish_portfolio(
    app: AppHandle,
    command: PortfolioBatchCommand,
) -> Result<serde_json::Value, String> {
    publication::portfolio(app, command)
}

pub fn solar_native_import_portfolio_final(
    app: AppHandle,
    command: PortfolioFinalImportCommand,
) -> Result<ImportedFinal, String> {
    portfolio_finals::import(app, command)
}
pub fn solar_native_inspect_portfolio_final(
    app: AppHandle,
    command: PortfolioFinalSelector,
) -> Result<ImportedFinal, String> {
    portfolio_finals::inspect(app, command)
}

pub fn solar_native_portfolio_report_bundle(
    app: AppHandle,
    command: PortfolioReportBundleCommand,
) -> Result<serde_json::Value, String> {
    bundles::portfolio(app, command)
}

use crate::workspace::{
    resolve_workspace_handle, secure_existing_private_directory, secure_private_directory,
    valid_solar_root, valid_solar_segment, valid_workspace_handle,
};

/// Layout version for everything this module writes into a workspace. Bumping
/// it retires previously committed prepared inputs without deleting anything.
const NATIVE_LAYOUT_VERSION: &str = "v1";

/// The largest city snapshot the shell will normalise. The frontend already
/// bounds its own capture at 8 MiB; this is the same ceiling restated where the
/// bytes actually arrive.
const MAX_SNAPSHOT_BYTES: usize = 8 << 20;

/// The most cities one call may prepare or run. Large enough for any real
/// project selection, small enough that a malformed call cannot fan out.
const MAX_CITIES_PER_CALL: usize = 512;
/// A city run emits a small fixed chart family; never scan an arbitrary tree.
const MAX_CHART_DIRECTORY_ENTRIES_PER_CITY: usize = 512;

/// The Word rendering's file name inside a city directory.
pub const DOCX_NAME: &str = "apd-fr.docx";
/// Explicitly imported operator-interpreted final Markdown.
pub const FINAL_APD_NAME: &str = "apd-final-fr.md";
/// Explicitly imported English operator-interpreted final Markdown.
pub const FINAL_APD_EN_NAME: &str = "apd-final-en.md";
/// Its media type.
pub const DOCX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// The aggregate result's stable name at the root of a native run.
pub const PORTFOLIO_RESULT_NAME: &str = "portfolio-result.json";
/// Its media type.
pub const PORTFOLIO_RESULT_MEDIA_TYPE: &str = "application/json";
/// The calculation-backed aggregate draft's stable name at the root of a run.
pub const PORTFOLIO_DRAFT_NAME: &str = RUNTIME_PORTFOLIO_DRAFT_NAME;
/// The exact daily demand frame, retained as a local result projection for the
/// Input page. It is derived from the already-completed Rust pipeline; it is
/// not another editable input or a second demand calculation.
pub const LOAD_PROFILE_NAME: &str = "base-daily-load-profile.json";

/// How many cities prepare at once by default. Preparation is dominated by one
/// bounded HTTP call each, so this is a politeness limit on the weather service
/// rather than a CPU one.
const DEFAULT_PREPARE_CONCURRENCY: usize = 6;

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

/// One already-seeded city record, as the input page holds it.
///
/// `snapshot_json` is the exact body the frontend captured — from IndexedDB, or
/// from the existing authenticated project-data path on a miss. This module
/// never fetches it and never repairs it.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CitySnapshotInput {
    /// Canonical city context id.
    pub city_id: String,
    /// Human-facing name of the city record.
    pub display_name: String,
    /// The captured read-set, as canonical JSON.
    pub snapshot_json: String,
    /// Where the snapshot came from: `indexeddb` or `firestore`.
    pub source: String,
    /// When it was captured, RFC 3339.
    pub captured_at: String,
    /// The record's own last-update time, when the source reports one.
    #[serde(default)]
    pub record_updated_at_ms: Option<i64>,
    /// Opaque revision token from the source, when it has one.
    #[serde(default)]
    pub revision: Option<String>,
}

/// A request to prepare a selection of cities.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrepareCommand {
    /// The approved workspace this run belongs to.
    pub workspace_handle: String,
    /// Owning project.
    pub project_id: String,
    /// Root collection.
    pub root: String,
    /// The selected cities. Only these are prepared: a reference bundle is
    /// never downloaded for a city the operator may not run.
    pub cities: Vec<CitySnapshotInput>,
    /// `ds-solar-weather` base URL. Without it, preparation is cache-only and a
    /// miss reports `reference_unit_invalid` — the honest state to paint.
    #[serde(default)]
    pub weather_url: Option<String>,
    /// A bearer token for that service, minted by the caller.
    #[serde(default)]
    pub weather_token: Option<String>,
    /// Public API Gateway key when `weather_url` points at ds-brain.
    #[serde(default)]
    pub weather_api_key: Option<String>,
    /// How many cities to prepare at once.
    #[serde(default)]
    pub concurrency: Option<usize>,
    /// Re-download cached bundles instead of reusing them.
    ///
    /// A reference unit is a constant of its coordinates and its pins, so a
    /// cache hit is a complete skip and there is no time-to-live anywhere in
    /// this path. This flag is the deliberate refresh, and it is the only way
    /// to pick up a producer-side pin change — a pvlib upgrade or a revised
    /// catalog row is not visible from this side of the boundary.
    #[serde(default)]
    pub overwrite: bool,
    /// Output language recorded in each prepared input.
    #[serde(default)]
    pub language: Option<String>,
}

/// A request to report readiness without preparing anything.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadinessCommand {
    /// The approved workspace.
    pub workspace_handle: String,
    /// Root collection.
    pub root: String,
    /// The cities to report on.
    pub city_ids: Vec<String>,
}

/// A read-only request for the sealed annual generation profile prepared for
/// one city. It addresses the immutable input, not a calculation run.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationProfileCommand {
    pub workspace_handle: String,
    pub root: String,
    pub city_id: String,
}

/// One daily or monthly energy bucket from the producer's hourly simulation.
#[derive(Debug, Clone, Serialize)]
pub struct GenerationEnergyBucket {
    pub label: String,
    pub gross_energy_mwh: f64,
    pub net_energy_mwh: f64,
    pub inverter_loss_mwh: f64,
}

/// One hour of the mean 24-hour shape, averaged over all 365 simulated days.
#[derive(Debug, Clone, Serialize)]
pub struct GenerationHourShape {
    pub hour: usize,
    pub net_energy_kwh: f64,
    pub effective_irradiance_w_m2: f64,
}

/// Bounded, display-oriented projection of the verified ds-solar-weather
/// reference unit. Every generation value is aggregated from its exact 8,760
/// hourly rows; no PV model is recomputed in the desktop shell.
#[derive(Debug, Clone, Serialize)]
pub struct GenerationProfileView {
    pub schema_version: String,
    pub source: String,
    pub scope: String,
    pub city_id: String,
    pub prepared_at: String,
    pub input_digest: String,
    pub source_key: String,
    pub bundle_digest: String,
    pub algorithm_version: String,
    pub profile_version: String,
    pub rows: usize,
    pub city: String,
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: String,
    pub altitude_m: f64,
    pub annual_gross_energy_mwh: f64,
    pub annual_net_energy_mwh: f64,
    pub annual_inverter_loss_mwh: f64,
    pub capacity_factor_percent: f64,
    pub specific_yield_kwh_kwp_year: f64,
    pub reference_inverter_capacity_kw_ac: f64,
    pub reference_unit_dc_capacity_kw_dc: f64,
    pub module_model_id: String,
    pub inverter_model_id: String,
    pub dc_ac_ratio: f64,
    pub modules_per_string: i64,
    pub strings_per_reference_inverter: i64,
    pub modules_per_reference_inverter: i64,
    pub reported_module_capacity_w: f64,
    pub surface_tilt_degrees: f64,
    pub surface_azimuth_degrees: f64,
    pub annual_poa_kwh_m2_year: f64,
    pub weather_provider: String,
    pub weather_dataset: String,
    pub weather_dataset_kind: String,
    pub weather_source_start_year: i32,
    pub weather_source_end_year: i32,
    pub weather_content_digest: String,
    pub service_version: String,
    pub pvlib_version: String,
    pub daily: Vec<GenerationEnergyBucket>,
    pub monthly: Vec<GenerationEnergyBucket>,
    pub average_day: Vec<GenerationHourShape>,
}

/// A request to run already-prepared cities.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalculateCommand {
    /// The approved workspace.
    pub workspace_handle: String,
    /// Owning project.
    pub project_id: String,
    /// Root collection.
    pub root: String,
    /// The cities to run. Every one must already be prepared.
    pub city_ids: Vec<String>,
    /// The caller's run id. Echoed into results, excluded from their digests.
    pub run_id: String,
    /// Whether to render chart artifacts. Off means "Skip plots": the APD is
    /// still written, with no image references rather than broken ones.
    #[serde(default)]
    pub render_charts: bool,
    /// Whether to compose the selected deterministic report drafts.
    #[serde(default)]
    pub generate_documents: Option<bool>,
    /// Draft report intents composed from this calculation's typed output.
    #[serde(default)]
    pub report_drafts: Vec<String>,
    /// Omitted, null, or 0 selects automatic sizing in Rust's shared CPU pool.
    /// Explicit 1..=32 selects the city limit; 1 is strictly serial.
    #[serde(default)]
    #[schemars(range(min = 0, max = 32))]
    pub concurrency: Option<usize>,
}

/// Explicit portfolio execution over one exact, already-closed city batch.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PortfolioCalculateCommand {
    pub workspace_handle: String,
    pub project_id: String,
    pub root: String,
    pub source_run_id: String,
    pub run_id: String,
    pub portfolio_id: String,
    pub portfolio_name: String,
    pub membership_revision: String,
    /// Exact declared member order. Every member must exist in source_run_id.
    pub city_ids: Vec<String>,
    /// Deprecated caller assertions. New clients omit these: the native
    /// authority derives every calculation assumption from the sealed city
    /// projections and refuses cross-city disagreement. Retaining optional
    /// assertions lets an older paired CLI fail closed instead of silently
    /// changing the meaning of its request.
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub project_years: Option<u32>,
    #[serde(default)]
    pub discount_rate: Option<f64>,
    pub representative_strategy: String,
    #[serde(default)]
    pub representative_city_id: Option<String>,
    #[serde(default)]
    pub round_robin_start_index: Option<u64>,
    pub language: String,
    pub report_intents: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PortfolioBatchCommand {
    pub workspace_handle: String,
    pub root: String,
    pub run_id: String,
    pub expected_batch_id: String,
    pub expected_batch_digest: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PortfolioBatchArtifactCommand {
    pub workspace_handle: String,
    pub root: String,
    pub run_id: String,
    pub name: String,
    pub max_bytes: u64,
    pub expected_batch_id: String,
    pub expected_batch_digest: String,
    pub expected_media_type: String,
    pub expected_byte_count: u64,
    pub expected_content_digest: String,
}

/// Output-only selector for an explicit portfolio batch.
///
/// Unlike the generic bounded byte reader used by publication and CLI
/// projection, this selector is accepted only for the sealed result or one of
/// the deterministic Markdown reports. It can never address the run input.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PortfolioBatchOutputCommand {
    pub workspace_handle: String,
    pub root: String,
    pub run_id: String,
    pub name: String,
    pub expected_batch_id: String,
    pub expected_batch_digest: String,
    pub expected_media_type: String,
    pub expected_byte_count: u64,
    pub expected_content_digest: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortfolioSourceCityResultDigest {
    pub city_id: String,
    pub result_digest: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortfolioRunReport {
    pub run_id: String,
    /// `ds-solar-engine@0.1.0`. The aggregate publication row carries it as
    /// `engine_version`, exactly as a city run's report does — server-side build
    /// admission reads this, so a portfolio that could not report it could not
    /// publish at all.
    pub engine_version: String,
    /// Exact canonical build manifest used by server-side admission.
    pub engine_build_manifest_sha256: String,
    pub source_run_id: String,
    pub source_batch_id: String,
    pub source_batch_digest: String,
    pub source_city_result_digests: Vec<PortfolioSourceCityResultDigest>,
    /// Governed values derived from every sealed member projection. They are
    /// returned so receipts can explain the calculation without asking the
    /// operator to re-enter model data.
    pub currency: String,
    pub project_years: u32,
    pub discount_rate: f64,
    pub input_digest: String,
    pub result_digest: String,
    pub batch: ds_solar_contracts::SolarArtifactBatch,
    pub files: Vec<DocumentFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_error: Option<String>,
}

/// What the page shows for one city.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum CityState {
    /// Preparation is in flight. The page keeps working.
    Preparing,
    /// Everything this city needs is present and validated.
    Ready {
        /// Digest of the committed prepared input.
        input_digest: String,
        /// The weather cache entry it carries.
        weather_entry: String,
        /// Whether that weather came from the cache or the provider.
        weather_source: String,
        /// Bounded freshness metadata projected from the verified native
        /// reference cache. It contains no bundle bytes or filesystem path.
        reference_cache_receipt: ReferenceCacheReceipt,
    },
    /// Preparation cannot complete, and why — precisely.
    NotReady {
        /// Stable machine code, e.g. `city_input_incomplete`.
        code: String,
        /// Human-readable detail.
        message: String,
        /// Field paths this is about, when the error names any.
        fields: Vec<String>,
    },
}

/// One city's readiness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CityReadiness {
    /// Canonical city context id.
    pub city_id: String,
    /// What to show.
    #[serde(flatten)]
    pub state: CityState,
}

/// A preparation job, as the page polls it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareJob {
    /// Job handle, for polling.
    pub job_id: String,
    /// Monotonic sequence, so a stale poll is recognisable.
    pub seq: u64,
    /// Whether every city has settled.
    pub done: bool,
    /// How many cities have settled.
    pub settled: usize,
    /// How many cities were selected.
    pub total: usize,
    /// One entry per selected city, in the order they were submitted.
    pub cities: Vec<CityReadiness>,
}

/// One committed file, described the way the page and the catalog need it.
///
/// A name, a size and a digest — never a path. Reading, opening and saving go
/// through the native selector commands, which resolve the location inside the
/// approved workspace from durable ids. A private filesystem path is not
/// something a web view or an agent needs to hold.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentFile {
    /// The file name inside its committed directory, e.g. `apd-fr.md`.
    pub name: String,
    /// IANA media type.
    pub media_type: String,
    /// Size in bytes, as committed.
    pub byte_count: u64,
    /// `sha256:<hex>` over the committed bytes.
    pub content_digest: String,
}

/// What one city's document production produced.
#[derive(Debug, Clone, Serialize)]
pub struct CityDocuments {
    /// The complete assembled project result consumed by every dashboard.
    pub report_input: DocumentFile,
    /// The clean user-facing APD. Always present on a successful run with
    /// documents enabled — Markdown never depends on Pandoc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apd: Option<DocumentFile>,
    /// The frozen-parity draft, kept beside it as regression evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft: Option<DocumentFile>,
    /// Every requested deterministic draft, including APD when selected.
    pub drafts: Vec<DocumentFile>,
    /// Legacy Word rendering reopened from a historical closed run. New runs
    /// never populate this compatibility field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docx: Option<DocumentFile>,
    /// Legacy DOCX diagnostic reopened from historical state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docx_error: Option<String>,
    /// How many figures the run rendered for this city.
    pub chart_count: usize,
    /// How many `##` sections the primary draft carries.
    pub section_count: usize,
    /// How many figures the primary draft references.
    pub figure_count: usize,
    /// `sha256` over the committed thin `result.json` bytes, retained for
    /// diagnostics. Publication and dashboards use `report_input` above.
    pub result_file_digest: String,
    /// Every chart file this city committed, as logical names relative to the
    /// city directory. What the publication lane uploads, and what the document
    /// references.
    pub chart_names: Vec<String>,
}

/// The aggregate artifacts a successful native run committed at its root.
///
/// They are deliberately separate from city documents: a portfolio is one
/// calculation-backed aggregate, not a fake city. The caller gets opaque
/// selectors for these files through the portfolio artifact commands below.
#[derive(Debug, Clone, Serialize)]
pub struct PortfolioArtifacts {
    /// The sealed aggregate result.
    pub result: DocumentFile,
    /// The deterministic calculation-backed portfolio draft, when the engine
    /// produced one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft: Option<DocumentFile>,
}

/// Where one city's wall-clock went, in milliseconds.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CityTimings {
    /// The ten engine stages.
    pub pipeline_ms: u64,
    /// Building and rendering the chart inventory.
    pub charts_ms: u64,
    /// Composing the selected Markdown drafts.
    pub documents_ms: u64,
    /// Writing every artifact into the workspace.
    pub commit_ms: u64,
    /// Legacy timing field retained in the serialized receipt; always zero for
    /// current draft-only production.
    pub docx_ms: u64,
    /// Exact duration of each engine step, keyed by stable stage id.
    #[serde(default)]
    pub stage_ms: BTreeMap<String, u64>,
}

/// One city's outcome in a run.
#[derive(Debug, Clone, Serialize)]
pub struct CityRunReport {
    /// Canonical city context id.
    pub city_id: String,
    /// The sealed result digest, when it succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_digest: Option<String>,
    /// Stable machine code, when it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Human-readable detail, when it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// What this city's document production produced, when it succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documents: Option<CityDocuments>,
    /// Where this city's wall-clock went.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timings: Option<CityTimings>,
}

/// What one run produced.
#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    /// The caller's run id.
    pub run_id: String,
    /// The exact engine identity that computed this run, e.g.
    /// `ds-solar-engine@0.1.0`. Publication rows carry it as `engine_version`.
    pub engine_version: String,
    /// Exact `ds.engine-build/v1` manifest digest for admission and audit.
    pub engine_build_manifest_sha256: String,
    /// Per-city outcomes, in the order the caller selected them.
    pub cities: Vec<CityRunReport>,
    /// The aggregate result and draft committed at this run's root, when at
    /// least one city succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portfolio: Option<PortfolioArtifacts>,
    /// The closed batch over every artifact — results, documents and figures.
    pub batch: ds_solar_contracts::SolarArtifactBatch,
    /// The single record that enters the Sync Center for this run.
    pub sync_intent: ds_solar_contracts::SyncIntent,
    /// Separate local sync receipts and errors; successful compute stays usable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub publications: Vec<publication::CityPublication>,
    /// Total wall-clock for the whole run, milliseconds.
    pub elapsed_ms: u64,
}

// ---------------------------------------------------------------------------
// Live run progress
// ---------------------------------------------------------------------------

/// Where one city is, right now.
///
/// The phases a run really has, in the order it passes through them. A page
/// that showed one undifferentiated "running" for a five-minute multi-city run
/// would be telling the operator nothing they could act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CityPhase {
    /// Selected, not started: every worker slot is busy.
    Queued,
    /// The ten engine stages.
    Calculating,
    /// Building and rendering the chart inventory.
    RenderingPlots,
    /// Composing the APD documents.
    ComposingReport,
    /// Writing every artifact into the workspace.
    Committing,
    /// Rendering the Word document through Pandoc.
    /// Committed locally. Publication is a separate, lazy lifecycle.
    LocalReady,
    /// This city did not complete, and the report says why.
    Failed,
}

/// One city's live state.
#[derive(Debug, Clone, Serialize)]
pub struct CityProgress {
    /// Canonical city context id.
    pub city_id: String,
    /// Where it is.
    pub phase: CityPhase,
    /// The engine stage in flight, when one is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    /// 1-based index of that stage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
    /// How many stages the run plans.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_steps: Option<u32>,
}

/// A run, as the page polls it.
#[derive(Debug, Clone, Serialize)]
pub struct RunProgress {
    /// The caller's run id.
    pub run_id: String,
    /// Monotonic sequence, so a stale poll is recognisable.
    pub seq: u64,
    /// Whether the run has settled.
    pub done: bool,
    /// One entry per selected city, in the order they were submitted.
    pub cities: Vec<CityProgress>,
    /// What the run produced. Present exactly when `done` and it succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<RunReport>,
    /// Why the run did not produce anything, when it did not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The progress sink one city's stages report to.
struct CitySink {
    app: AppHandle,
    run_id: String,
    index: usize,
}

impl ds_solar_host::progress::ProgressSink for CitySink {
    fn emit(&self, event: ds_solar_contracts::ProgressEvent) {
        use ds_solar_contracts::ProgressPhase;
        let state = self.app.state();
        let seq = state.next_seq();
        state.update_run(&self.run_id, seq, |run| {
            let Some(city) = run.cities.get_mut(self.index) else {
                return;
            };
            match event.phase {
                ProgressPhase::RunStarted | ProgressPhase::StageStarted => {
                    city.phase = CityPhase::Calculating;
                    city.stage = event.stage.clone();
                    city.step = event.step;
                    city.total_steps = event.total_steps;
                }
                ProgressPhase::StageCompleted | ProgressPhase::StageSkipped => {
                    city.step = event.step;
                    city.total_steps = event.total_steps;
                }
                ProgressPhase::StageFailed => {
                    city.phase = CityPhase::Failed;
                    city.stage = event.stage.clone();
                }
                ProgressPhase::RunFinished => {
                    // The stages are done; charts and documents follow inside
                    // the same call, so the next honest phase is rendering.
                    if city.phase != CityPhase::Failed {
                        city.phase = CityPhase::RenderingPlots;
                        city.stage = None;
                    }
                }
            }
        });
    }
}

/// Routes each city's stage events to its own sink.
struct RunObserver {
    sinks: BTreeMap<String, CitySink>,
}

impl ds_solar_host::process::BatchProgressSink for RunObserver {
    fn emit(&self, event: ds_solar_contracts::prepared_batch::PreparedBatchProgress) {
        if let Some(sink) = self.sinks.get(&event.city_id) {
            ds_solar_host::progress::ProgressSink::emit(sink, event.event);
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Preparation jobs and run cancellation, shared across commands.
#[derive(Default)]
pub struct SolarNativeState {
    pub(crate) portfolio_jobs: Mutex<BTreeMap<String, portfolio_jobs::Entry>>,
    inputs: Mutex<()>,
    pub(crate) workspaces: Mutex<()>,
    jobs: Mutex<BTreeMap<String, PrepareJob>>,
    preparation_cancels: Mutex<BTreeMap<String, CancelToken>>,
    /// Kept separate from `jobs` so a long preparation cannot block a cancel.
    runs: Mutex<BTreeMap<String, CancelToken>>,
    /// Live run progress, polled by the page while a run is in flight.
    progress: Mutex<BTreeMap<String, RunProgress>>,
    sequence: AtomicU64,
}

impl SolarNativeState {
    fn next_seq(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn publish(&self, job: PrepareJob) -> Result<(), String> {
        let mut jobs = self
            .jobs
            .lock()
            .map_err(|_| "Solar preparation state is poisoned")?;
        while jobs.len() >= 32 {
            let oldest = jobs
                .iter()
                .find(|(_, job)| job.done)
                .map(|(key, _)| key.clone())
                .ok_or("This native context already has 32 active preparation jobs")?;
            jobs.remove(&oldest);
        }
        jobs.insert(job.job_id.clone(), job);
        Ok(())
    }

    fn publish_run(&self, run: RunProgress) {
        if let Ok(mut progress) = self.progress.lock() {
            // Bound the table: a desktop session runs a handful of batches, not
            // an unbounded stream of them.
            while progress.len() >= 16 {
                let Some(oldest) = progress
                    .iter()
                    .find(|(_, run)| run.done)
                    .map(|(key, _)| key.clone())
                else {
                    break;
                };
                progress.remove(&oldest);
            }
            progress.insert(run.run_id.clone(), run);
        }
    }

    fn update_run<F: FnOnce(&mut RunProgress)>(&self, run_id: &str, seq: u64, change: F) {
        if let Ok(mut progress) = self.progress.lock()
            && let Some(run) = progress.get_mut(run_id)
        {
            change(run);
            run.seq = seq;
        }
    }

    fn update<F: FnOnce(&mut PrepareJob)>(&self, job_id: &str, seq: u64, change: F) {
        // ds-web is on edition 2021, so no let-chains here.
        if let Ok(mut jobs) = self.jobs.lock()
            && let Some(job) = jobs.get_mut(job_id)
        {
            change(job);
            job.seq = seq;
        }
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Prepare a selection of cities. Returns as soon as the job is registered.
///
/// The work continues on a background pool so the page never freezes; poll
/// [`solar_native_prepare_progress`] to paint each city as it settles.
pub fn solar_native_prepare(
    app: AppHandle,
    state: &SolarNativeState,
    command: PrepareCommand,
) -> Result<PrepareJob, String> {
    start_preparation(app, state, command, false)
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrepareCapturedCommand {
    pub workspace_handle: String,
    pub project_id: String,
    pub root: String,
    pub city_ids: Vec<String>,
    pub concurrency: Option<usize>,
    #[serde(default)]
    pub overwrite: bool,
    pub language: Option<String>,
}
pub fn solar_native_prepare_captured(
    app: AppHandle,
    state: &SolarNativeState,
    command: PrepareCapturedCommand,
) -> Result<PrepareJob, String> {
    if app.snapshots.is_none() {
        return Err("This host has no governed Solar snapshot producer".into());
    }
    let cities = command
        .city_ids
        .into_iter()
        .map(|city_id| CitySnapshotInput {
            city_id,
            display_name: String::new(),
            snapshot_json: String::new(),
            source: String::new(),
            captured_at: String::new(),
            record_updated_at_ms: None,
            revision: None,
        })
        .collect();
    start_preparation(
        app,
        state,
        PrepareCommand {
            workspace_handle: command.workspace_handle,
            project_id: command.project_id,
            root: command.root,
            cities,
            concurrency: command.concurrency,
            overwrite: command.overwrite,
            language: command.language,
            weather_url: None,
            weather_token: None,
            weather_api_key: None,
        },
        true,
    )
}
fn start_preparation(
    app: AppHandle,
    state: &SolarNativeState,
    command: PrepareCommand,
    capture: bool,
) -> Result<PrepareJob, String> {
    let root = checked_root(&command.root)?;
    let project_id = checked_segment(&command.project_id, "project id")?;
    if root != format!("eds_project/{project_id}/eds_solar") {
        return Err("Solar preparation belongs to another project".into());
    }
    if command.concurrency.is_some_and(|n| !(1..=32).contains(&n)) {
        return Err("Solar preparation concurrency must be 1..32".into());
    }
    let cities = checked_cities(&command.cities)?;
    for city in &cities {
        checked_segment(&city.city_id, "city id")?;
    }
    let language = command.language.clone().unwrap_or_else(|| "fr".to_string());
    if !matches!(language.as_str(), "fr" | "en") {
        return Err("Solar preparation language must be fr or en".into());
    }
    let workspace = resolve_workspace_handle(&app, &command.workspace_handle)?;

    let job_id = uuid::Uuid::new_v4().to_string();
    let job = PrepareJob {
        job_id: job_id.clone(),
        seq: state.next_seq(),
        done: false,
        settled: 0,
        total: cities.len(),
        cities: cities
            .iter()
            .map(|city| CityReadiness {
                city_id: city.city_id.clone(),
                state: CityState::Preparing,
            })
            .collect(),
    };
    let prepared_dir = prepared_dir(&workspace, &root)?;
    let cache_dir = weather_cache_dir(&workspace)?;
    let durable_dir = secure_private_directory(&app.data_dir()?.join("solar-preparation-jobs"))?;
    let durable_path = durable_dir.join(format!("{job_id}.json"));
    let cancel = CancelToken::new();
    let mut active = state
        .preparation_cancels
        .lock()
        .map_err(|_| "Solar preparation cancellation state is poisoned")?;
    if active.len() >= 32 {
        return Err("This native context already has 32 active preparation jobs".into());
    }
    persist_preparation(&durable_path, &project_id, &root, &job)?;
    if let Err(error) = state.publish(job.clone()) {
        let _ = std::fs::remove_file(&durable_path);
        return Err(error);
    }
    active.insert(job_id.clone(), cancel.clone());
    drop(active);
    let concurrency = Concurrency::new(
        command
            .concurrency
            .unwrap_or(DEFAULT_PREPARE_CONCURRENCY)
            .max(1),
    );
    let weather_url = command.weather_url.clone();
    let weather_token = command.weather_token.clone();
    let weather_api_key = command.weather_api_key.clone();
    let overwrite = command.overwrite;

    // `State` borrows the invocation and cannot cross a thread boundary, but
    // `AppHandle` is `Send` and outlives every command, so the worker reaches
    // the same managed value through it.
    let worker_app = app.clone();
    std::thread::spawn(move || {
        let state = worker_app.state();
        let settled =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                let _inputs = state
                    .inputs
                    .lock()
                    .map_err(|_| "Solar input state is poisoned")?;
                prepare_all(
                    &cities,
                    &project_id,
                    &root,
                    &language,
                    &prepared_dir,
                    &cache_dir,
                    worker_app.references.as_deref(),
                    if capture {
                        worker_app.snapshots.as_deref()
                    } else {
                        None
                    },
                    Some(&cancel),
                    weather_url.as_deref(),
                    weather_token.as_deref(),
                    weather_api_key.as_deref(),
                    overwrite,
                    concurrency,
                    |index, readiness| {
                        state.update(&job_id, state.next_seq(), |job| {
                            if let Some(slot) = job.cities.get_mut(index) {
                                *slot = readiness.clone();
                            }
                            job.settled = job
                                .cities
                                .iter()
                                .filter(|c| !matches!(c.state, CityState::Preparing))
                                .count();
                        });
                        if let Ok(job) = solar_native_prepare_progress(&state, job_id.clone())
                            && persist_preparation(&durable_path, &project_id, &root, &job).is_err()
                        {
                            cancel.cancel();
                        }
                    },
                );
                Ok(())
            }))
            .unwrap_or_else(|_| {
                Err("Solar preparation worker panicked; committed inputs remain available".into())
            });
        state.update(&job_id, state.next_seq(), |job| {
            for city in &mut job.cities {
                if matches!(city.state, CityState::Preparing) {
                    *city = not_ready(
                        &city.city_id,
                        "preparation_failed",
                        settled
                            .as_ref()
                            .err()
                            .map(String::as_str)
                            .unwrap_or("Preparation did not settle"),
                        &[],
                    );
                }
            }
            job.done = true;
            job.settled = job.total;
        });
        if let Ok(job) = solar_native_prepare_progress(&state, job_id.clone())
            && let Err(error) = persist_preparation(&durable_path, &project_id, &root, &job)
        {
            state.update(&job_id, state.next_seq(), |job| {
                for city in &mut job.cities {
                    *city = not_ready(&city.city_id, "preparation_persistence_failed", &error, &[]);
                }
            });
        }
        if let Ok(mut active) = state.preparation_cancels.lock() {
            active.remove(&job_id);
        }
    });

    Ok(job)
}

/// Poll one preparation job.
pub fn solar_native_prepare_progress(
    state: &SolarNativeState,
    job_id: String,
) -> Result<PrepareJob, String> {
    state
        .jobs
        .lock()
        .map_err(|_| "Solar preparation state is poisoned".to_string())?
        .get(&job_id)
        .cloned()
        .ok_or_else(|| "That Solar preparation job is not active".to_string())
}

fn persist_preparation(
    path: &Path,
    project: &str,
    root: &str,
    job: &PrepareJob,
) -> Result<(), String> {
    write_json(
        path,
        &serde_json::json!({"schema":"ds-solar.durable-preparation/v1","project_id":project,"root":root,"job":job}),
    )
}
fn durable_preparation(
    app: &AppHandle,
    project: &str,
    job_id: &str,
) -> Result<serde_json::Value, String> {
    let job_id = checked_segment(job_id, "preparation job id")?;
    let directory =
        secure_existing_private_directory(&app.data_dir()?.join("solar-preparation-jobs"))?;
    let data: serde_json::Value = serde_json::from_slice(&read_bounded_regular_file(
        &directory.join(format!("{job_id}.json")),
        "preparation journal",
        2 << 20,
    )?)
    .map_err(|_| "Invalid preparation journal")?;
    if data["schema"] != "ds-solar.durable-preparation/v1"
        || data["project_id"] != project
        || data["root"] != format!("eds_project/{project}/eds_solar")
        || data["job"]["job_id"] != job_id
    {
        return Err("Preparation belongs to another project or job".into());
    }
    let job: PrepareJob = serde_json::from_value(data["job"].clone())
        .map_err(|_| "Invalid preparation journal projection")?;
    if job.total != job.cities.len()
        || job.total == 0
        || job.total > MAX_CITIES_PER_CALL
        || job.settled > job.total
        || job.done && job.settled != job.total
    {
        return Err("Invalid preparation journal counts".into());
    }
    let mut seen = BTreeSet::new();
    for city in &job.cities {
        checked_segment(&city.city_id, "preparation city id")?;
        if !seen.insert(&city.city_id) {
            return Err("Preparation journal repeats a city".into());
        }
    }
    serde_json::to_value(job).map_err(|_| "Cannot encode preparation journal".into())
}
pub fn preparation_progress(
    app: &AppHandle,
    project: &str,
    job_id: &str,
) -> Result<serde_json::Value, String> {
    let mut durable = durable_preparation(app, project, job_id)?;
    let state = app.state();
    if let Ok(job) = solar_native_prepare_progress(&state, job_id.into()) {
        return serde_json::to_value(job).map_err(|_| "Cannot encode preparation progress".into());
    }
    if durable["done"] != true {
        let cities = durable["cities"]
            .as_array_mut()
            .ok_or("Invalid preparation city inventory")?;
        if cities.len() > MAX_CITIES_PER_CALL {
            return Err("Preparation journal exceeds the city bound".into());
        }
        for city in cities.iter_mut() {
            if city["state"] == "preparing" {
                city["state"] = "not_ready".into();
                city["code"] = "preparation_interrupted".into();
                city["message"]="Preparation was interrupted before settlement; inspect readiness and start a new preparation job".into();
                city["fields"] = serde_json::json!([]);
            }
        }
        let total = cities.len();
        let next = durable["seq"]
            .as_u64()
            .and_then(|n| n.checked_add(1))
            .ok_or("Invalid interrupted preparation sequence")?;
        durable["seq"] = next.into();
        durable["done"] = true.into();
        durable["settled"] = total.into();
    }
    Ok(durable)
}
pub fn preparation_cancel(app: &AppHandle, project: &str, job_id: &str) -> Result<bool, String> {
    durable_preparation(app, project, job_id)?;
    let state = app.state();
    let active = state
        .preparation_cancels
        .lock()
        .map_err(|_| "Solar preparation cancellation state is poisoned")?;
    if let Some(token) = active.get(job_id) {
        token.cancel();
        return Ok(true);
    }
    Ok(false)
}

/// Report readiness for cities already committed to this workspace.
///
/// Reads only. It cannot fetch, cache or repair anything, so the page can call
/// it freely — on mount, after a selection change, after a return from offline.
pub fn solar_native_readiness(
    app: AppHandle,
    command: ReadinessCommand,
) -> Result<Vec<CityReadiness>, String> {
    let workspace = resolve_workspace_handle(&app, &command.workspace_handle)?;
    let root = checked_root(&command.root)?;
    if command.city_ids.len() > MAX_CITIES_PER_CALL {
        return Err("too many cities in one Solar readiness request".to_string());
    }
    if command.city_ids.is_empty() {
        return Ok(Vec::new());
    }
    let directory = prepared_dir(&workspace, &root)?;
    Ok(command
        .city_ids
        .iter()
        .map(|city_id| match checked_segment(city_id, "city id") {
            Err(message) => CityReadiness {
                city_id: city_id.clone(),
                state: CityState::NotReady {
                    code: "invalid_input".to_string(),
                    message,
                    fields: vec!["city_id".to_string()],
                },
            },
            Ok(city_id) => readiness_of(&directory, &city_id),
        })
        .collect())
}

/// Read the annual PV generation already sealed into one prepared city input.
/// The only work here is lossless time aggregation for display.
pub fn solar_native_generation_profile(
    app: AppHandle,
    command: GenerationProfileCommand,
) -> Result<GenerationProfileView, String> {
    let workspace = resolve_workspace_handle(&app, &command.workspace_handle)?;
    let root = checked_root(&command.root)?;
    let city_id = checked_segment(&command.city_id, "city id")?;
    let directory = prepared_dir(&workspace, &root)?;
    let input = read_prepared(&directory, &city_id).ok_or_else(|| {
        format!("{city_id} has not been prepared on the input page (city_input_not_prepared)")
    })?;
    ds_solar_contracts::require_prepared(&input)
        .map_err(|error| format!("{city_id}: {} ({})", error.message, error.code.as_str()))?;
    generation_profile_view(&city_id, &input)
}

fn generation_profile_view(
    city_id: &str,
    input: &PreparedSolarCityInput,
) -> Result<GenerationProfileView, String> {
    let unit = &input.reference_unit;
    let receipt = &unit.receipt;
    let profile = &unit.profile;
    let rows = profile.len();
    if rows != 8_760 {
        return Err(format!(
            "the prepared generation profile carries {rows} rows, not 8760"
        ));
    }

    let mut daily = Vec::with_capacity(365);
    for day in 0..365 {
        let start = day * 24;
        let end = start + 24;
        let timestamp = &profile.local_timestamp[start];
        daily.push(GenerationEnergyBucket {
            label: timestamp.get(0..10).unwrap_or(timestamp).to_string(),
            gross_energy_mwh: profile.gross_unit_ac_energy_mwh[start..end].iter().sum(),
            net_energy_mwh: profile.net_unit_energy_mwh[start..end].iter().sum(),
            inverter_loss_mwh: profile.unit_inverter_loss_mwh[start..end].iter().sum(),
        });
    }

    let month_days = [31_usize, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut day_offset = 0;
    let mut monthly = Vec::with_capacity(12);
    for (month, days) in month_days.into_iter().enumerate() {
        let buckets = &daily[day_offset..day_offset + days];
        monthly.push(GenerationEnergyBucket {
            label: format!("{:02}", month + 1),
            gross_energy_mwh: buckets.iter().map(|bucket| bucket.gross_energy_mwh).sum(),
            net_energy_mwh: buckets.iter().map(|bucket| bucket.net_energy_mwh).sum(),
            inverter_loss_mwh: buckets.iter().map(|bucket| bucket.inverter_loss_mwh).sum(),
        });
        day_offset += days;
    }

    let average_day = (0..24)
        .map(|hour| GenerationHourShape {
            hour,
            net_energy_kwh: profile
                .net_unit_energy_mwh
                .iter()
                .skip(hour)
                .step_by(24)
                .sum::<f64>()
                * 1_000.0
                / 365.0,
            effective_irradiance_w_m2: profile
                .effective_irradiance_w_m2
                .iter()
                .skip(hour)
                .step_by(24)
                .sum::<f64>()
                / 365.0,
        })
        .collect();

    let strings = &receipt.string_configuration;
    let dc_capacity = strings.reference_unit_dc_capacity_kw;
    Ok(GenerationProfileView {
        schema_version: "ds-solar.generation-profile/v1".to_string(),
        source: "ds-solar-weather".to_string(),
        scope: "one_reference_inverter_and_strings".to_string(),
        city_id: city_id.to_string(),
        prepared_at: input.prepared_at.clone(),
        input_digest: input.input_digest.clone(),
        source_key: receipt.source_key.clone(),
        bundle_digest: unit.manifest.bundle_digest.clone(),
        algorithm_version: receipt.algorithm_version.clone(),
        profile_version: receipt.profile_version.clone(),
        rows,
        city: receipt.site.city.clone(),
        country: receipt.site.country.clone(),
        latitude: receipt.site.latitude,
        longitude: receipt.site.longitude,
        timezone: receipt.site.timezone.clone(),
        altitude_m: receipt.site.altitude_m,
        annual_gross_energy_mwh: receipt.annual.unit_gross_energy_mwh,
        annual_net_energy_mwh: receipt.annual.unit_energy_mwh,
        annual_inverter_loss_mwh: receipt.annual.unit_inverter_loss_mwh,
        capacity_factor_percent: receipt.annual.capacity_factor * 100.0,
        specific_yield_kwh_kwp_year: if dc_capacity > 0.0 {
            receipt.annual.unit_energy_mwh * 1_000.0 / dc_capacity
        } else {
            0.0
        },
        reference_inverter_capacity_kw_ac: strings.reported_reference_inverter_capacity_kw,
        reference_unit_dc_capacity_kw_dc: dc_capacity,
        module_model_id: receipt.equipment.module.model_id.clone(),
        inverter_model_id: receipt.equipment.inverter.model_id.clone(),
        dc_ac_ratio: receipt.equipment.dc_ac_ratio,
        modules_per_string: strings.modules_per_string,
        strings_per_reference_inverter: strings.strings_per_reference_inverter,
        modules_per_reference_inverter: strings.modules_per_reference_inverter,
        reported_module_capacity_w: strings.reported_module_capacity_w,
        surface_tilt_degrees: receipt.mounting.surface_tilt_degrees,
        surface_azimuth_degrees: receipt.mounting.surface_azimuth_degrees,
        annual_poa_kwh_m2_year: receipt.solar_geometry.annual_poa_wh_m2 / 1_000.0,
        weather_provider: receipt.weather.provider.clone(),
        weather_dataset: receipt.weather.provider_dataset.clone(),
        weather_dataset_kind: receipt.weather.dataset_kind.clone(),
        weather_source_start_year: receipt.weather.source_period.start_year,
        weather_source_end_year: receipt.weather.source_period.end_year,
        weather_content_digest: receipt.weather.content_digest.clone(),
        service_version: receipt.runtime.service_version.clone(),
        pvlib_version: receipt.runtime.pvlib_version.clone(),
        daily,
        monthly,
        average_day,
    })
}

pub(crate) fn calculation_concurrency(requested: Option<usize>) -> Result<Concurrency, String> {
    match requested {
        None | Some(0) => Ok(Concurrency::default()),
        Some(n @ 1..=32) => Ok(Concurrency::new(n)),
        _ => Err(
            "Solar calculation concurrency must be automatic (null or 0), or between 1 and 32"
                .into(),
        ),
    }
}

/// Run already-prepared cities. Makes no intake or network call of any kind.
///
/// Returns as soon as the run is registered; the work continues on a background
/// thread so a five-city batch does not hold an IPC call open for minutes. Poll
/// [`solar_native_calculate_progress`] to paint each city's phase, and read the
/// finished [`RunReport`] from the same poll when it settles.
///
/// One operation, and it ends with the ten Rust stages, the chart inventory,
/// the selected deterministic report drafts, and one closed batch. Nothing here
/// returns bytes — the page receives names, sizes and digests, and reads what
/// it needs back through the selector commands.
pub fn solar_native_calculate(
    app: AppHandle,
    state: &SolarNativeState,
    command: CalculateCommand,
) -> Result<RunProgress, String> {
    let workspace = resolve_workspace_handle(&app, &command.workspace_handle)?;
    let root = checked_root(&command.root)?;
    let project_id = checked_text(&command.project_id, "project id")?;
    let run_id = checked_segment(&command.run_id, "run id")?;
    if root != format!("eds_project/{project_id}/eds_solar") && root != "eds_solar" {
        return Err("Solar calculation root belongs to another project".into());
    }
    if command.city_ids.is_empty() || command.city_ids.len() > MAX_CITIES_PER_CALL {
        return Err("a Solar run needs between one and 512 cities".to_string());
    }
    let concurrency = calculation_concurrency(command.concurrency)?;
    let settings = crate::settings::RunSettings::from(&command).validate()?;
    let report_drafts = settings.report_drafts;

    let directory = prepared_dir(&workspace, &root)?;
    let mut prepared: Vec<PreparedSolarCityInput> = Vec::with_capacity(command.city_ids.len());
    let mut publication_claims = BTreeMap::new();
    let mut selected = BTreeSet::new();
    let _inputs = state
        .inputs
        .lock()
        .map_err(|_| "Solar input state is poisoned")?;
    for city_id in &command.city_ids {
        let city_id = checked_segment(city_id, "city id")?;
        if !selected.insert(city_id.clone()) {
            return Err("Solar calculation cities must be unique".into());
        }
        let input = read_prepared(&directory, &city_id).ok_or_else(|| {
            format!("{city_id} has not been prepared on the input page (city_input_not_prepared)")
        })?;
        if input.city.identity.project_id != project_id
            || input.city.identity.root != root
            || input.city.identity.city_id != city_id
        {
            return Err("Prepared Solar input belongs to another calculation context".into());
        }
        // The local readiness check, before anything runs.
        ds_solar_contracts::require_prepared(&input)
            .map_err(|error| format!("{city_id}: {} ({})", error.message, error.code.as_str()))?;
        let provenance = read_bounded_regular_file(
            &publication_claim_path(&directory, &input),
            "captured publication claim",
            16 << 10,
        )
        .and_then(|bytes| {
            serde_json::from_slice(&bytes).map_err(|_| "Invalid captured publication claim".into())
        })
        .and_then(|claim| crate::sync::claim_provenance(&input, &claim));
        publication_claims.insert(city_id, provenance);
        prepared.push(input);
    }
    drop(_inputs);

    let output_dir = run_output_dir(&workspace, &root, &run_id)?;
    let durable_dir = secure_private_directory(&app.data_dir()?.join("solar-jobs"))?;
    let durable_path = durable_dir.join(format!("{run_id}.json"));
    let cancel = CancelToken::new();
    // Capture the installed build before admission. Queued work must not be
    // redirected by a module update, and a missing/incompatible executable
    // must not leave a durable run reservation behind.
    let scratch = secure_private_directory(&app.data_dir()?.join("solar-workers"))?;
    let engine = CapturedEngine::capture(app.capture_engine()?, |path| {
        ds_solar_host::process::BatchEngine::capture(
            path,
            &scratch,
            ds_solar_host::compute_parallelism(),
            &cancel,
        )
    })?;
    {
        let mut runs = state
            .runs
            .lock()
            .map_err(|_| "Solar run state is poisoned")?;
        if runs.contains_key(&run_id) {
            return Err("that Solar run id is already active".to_string());
        }
        if runs.len() >= 16 {
            return Err("This native context already has 16 active Solar runs".into());
        }
        if output_dir.join("batch.json").exists() || durable_path.exists() {
            return Err("that Solar run id already has durable work; use a new run id".into());
        }
        use std::io::Write as _;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut reservation = options
            .open(&durable_path)
            .map_err(|e| format!("Cannot reserve Solar run: {e}"))?;
        let initial = serde_json::json!({"schema":"ds-solar.durable-run/v1","project_id":project_id,"root":root,"run_id":run_id,"done":false,"cities":command.city_ids});
        reservation
            .write_all(&serde_json::to_vec(&initial).map_err(|e| e.to_string())?)
            .and_then(|_| reservation.sync_all())
            .map_err(|e| format!("Cannot persist Solar run admission: {e}"))?;
        runs.insert(run_id.clone(), cancel.clone());
    }

    let progress = RunProgress {
        run_id: run_id.clone(),
        seq: state.next_seq(),
        done: false,
        cities: prepared
            .iter()
            .map(|input| CityProgress {
                city_id: input.city.identity.city_id.clone(),
                phase: CityPhase::Queued,
                stage: None,
                step: None,
                total_steps: None,
            })
            .collect(),
        report: None,
        error: None,
    };
    state.publish_run(progress.clone());

    let render_charts = command.render_charts;
    let generate_documents = settings.generate_documents.unwrap_or(true);
    let worker_app = app.clone();
    let worker_run_id = run_id.clone();
    std::thread::spawn(move || {
        let state = worker_app.state();
        // The token leaves the table when this worker ends — panic included —
        // so a crashed run can never wedge its run id as "already active".
        let _run_guard = RunGuard {
            state: &state,
            run_id: worker_run_id.clone(),
        };
        let settled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            execute_run(
                &worker_app,
                &worker_run_id,
                &project_id,
                &root,
                &output_dir,
                prepared,
                &publication_claims,
                render_charts,
                generate_documents,
                report_drafts,
                concurrency,
                engine,
                &cancel,
            )
        }))
        .unwrap_or_else(|_| {
            Err("Solar calculation worker panicked; sealed artifacts remain available".into())
        });
        let seq = state.next_seq();
        state.update_run(&worker_run_id, seq, |run| {
            run.done = true;
            match settled {
                Ok(report) => {
                    for city in &report.cities {
                        if let Some(slot) = run
                            .cities
                            .iter_mut()
                            .find(|slot| slot.city_id == city.city_id)
                        {
                            slot.phase = if city.result_digest.is_some() {
                                CityPhase::LocalReady
                            } else {
                                CityPhase::Failed
                            };
                            slot.stage = None;
                        }
                    }
                    run.report = Some(report);
                }
                Err(error) => {
                    for slot in &mut run.cities {
                        if slot.phase != CityPhase::LocalReady {
                            slot.phase = CityPhase::Failed;
                        }
                    }
                    run.error = Some(error);
                }
            }
        });
        if let Ok(progress) = solar_native_calculate_progress(&state, worker_run_id.clone()) {
            let durable = serde_json::json!({"schema":"ds-solar.durable-run/v1","project_id":project_id,"root":root,"run_id":worker_run_id,"done":true,"progress":progress});
            if let Err(error) = write_json(&durable_path, &durable) {
                state.update_run(&worker_run_id, state.next_seq(), |run| {
                    run.error = Some(format!(
                        "Solar work settled but its durable receipt could not be written: {error}"
                    ));
                });
            }
        }
    });

    Ok(progress)
}

/// Poll one run. A snapshot, never a delta: one call tells you everything.
pub fn solar_native_calculate_progress(
    state: &SolarNativeState,
    run_id: String,
) -> Result<RunProgress, String> {
    state
        .progress
        .lock()
        .map_err(|_| "Solar run progress is poisoned".to_string())?
        .get(&run_id)
        .cloned()
        .ok_or_else(|| "that Solar run is not active in this session".to_string())
}

/// Read live progress, or the same durable result after a native host restart.
/// An admitted run without a settlement is reported interrupted; it is never
/// guessed to be still running or silently re-executed under the same id.
pub fn calculate_progress(
    app: &AppHandle,
    project: &str,
    run_id: &str,
) -> Result<serde_json::Value, String> {
    let run_id = checked_segment(run_id, "run id")?;
    if let Ok(progress) = solar_native_calculate_progress(&app.state(), run_id.clone()) {
        return serde_json::to_value(progress).map_err(|e| e.to_string());
    }
    let path = app
        .data_dir()?
        .join("solar-jobs")
        .join(format!("{run_id}.json"));
    let bytes = read_bounded_regular_file(&path, "durable Solar run", 32 * 1024 * 1024)?;
    let durable: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid durable Solar run")?;
    if durable["schema"] != "ds-solar.durable-run/v1"
        || durable["project_id"] != project
        || durable["run_id"] != run_id
    {
        return Err("Durable Solar run belongs to another context".into());
    }
    if durable["done"] == true {
        let progress = durable
            .get("progress")
            .ok_or("Durable Solar settlement is unavailable")?;
        if progress["run_id"] != run_id || progress["done"] != true {
            return Err("Invalid durable Solar settlement".into());
        }
        return Ok(progress.clone());
    }
    Ok(
        serde_json::json!({"run_id":run_id,"seq":0,"done":true,"status":"interrupted","cities":[],"error":"Native Server restarted before the Solar run settled; inspect its sealed artifacts and use a new run id to retry"}),
    )
}

/// Compute, commit and close one run. Runs on the worker thread.
#[allow(clippy::too_many_arguments)]
fn execute_run(
    app: &AppHandle,
    run_id: &str,
    project_id: &str,
    root: &str,
    output_dir: &Path,
    prepared: Vec<PreparedSolarCityInput>,
    publication_claims: &BTreeMap<
        String,
        Result<ds_compute_runtime::SolarSnapshotProvenance, String>,
    >,
    render_charts: bool,
    generate_documents: bool,
    report_drafts: Vec<String>,
    concurrency: Concurrency,
    engine: CapturedEngine<ds_solar_host::process::BatchEngine>,
    cancel: &CancelToken,
) -> Result<RunReport, String> {
    let started = std::time::Instant::now();
    let observer = RunObserver {
        sinks: prepared
            .iter()
            .enumerate()
            .map(|(index, input)| {
                let city_id = input.city.identity.city_id.clone();
                (
                    city_id.clone(),
                    CitySink {
                        app: app.clone(),
                        run_id: run_id.to_string(),
                        index,
                    },
                )
            })
            .collect(),
    };

    use ds_solar_contracts::prepared_batch::{
        PREPARED_BATCH_REQUEST_SCHEMA, PreparedBatchOptions, PreparedBatchRequest,
    };
    let request = PreparedBatchRequest {
        schema_version: PREPARED_BATCH_REQUEST_SCHEMA.into(),
        project_id: project_id.into(),
        root: root.into(),
        prepared,
        options: PreparedBatchOptions {
            run_id: Some(run_id.into()),
            render_charts,
            generate_documents,
            report_drafts,
            input_maps: BTreeMap::new(),
        },
        concurrency: Some(concurrency.get()),
    };
    let output = engine.execute(|engine| {
        engine
            .calculate(&request, cancel, Some(&observer))
            .map_err(|error| match &error.refusal {
                Some(refusal) => format!("{} ({})", refusal.message, refusal.code.as_str()),
                None => error.to_string(),
            })
    })?;

    let state = app.state();
    let seq = state.next_seq();
    state.update_run(run_id, seq, |run| {
        for slot in &mut run.cities {
            if slot.phase != CityPhase::Failed {
                slot.phase = CityPhase::Committing;
                slot.stage = None;
            }
        }
    });

    let committed = engine_batch::commit(output_dir, &output, cancel, |city_id, phase| {
        let seq = state.next_seq();
        state.update_run(run_id, seq, |run| {
            if let Some(slot) = run.cities.iter_mut().find(|slot| slot.city_id == city_id)
                && slot.phase != CityPhase::Failed
            {
                slot.phase = phase;
            }
        });
    })?;

    // This fixed task cannot implicitly aggregate a portfolio. Its unchanged
    // owner batch closes only after every verified city file has been committed.
    let batch = output.output.batch.clone();
    if cancel.is_cancelled() {
        return Err("Solar calculation was cancelled before batch closure".into());
    }
    write_json(&output_dir.join("batch.json"), &batch)?;
    let sync_intent = batch.sync_intent(rfc3339_now());
    write_json(&output_dir.join("sync-intent.json"), &sync_intent)?;
    let publications = publication::cities_completed(
        app,
        output_dir,
        &batch,
        publication_claims,
        &output.completion_build_manifest_sha256,
    );

    let by_city: BTreeMap<&str, &CommittedCity> = committed
        .iter()
        .map(|city| (city.city_id.as_str(), city))
        .collect();

    Ok(RunReport {
        run_id: run_id.to_string(),
        engine_version: format!("{}@{}", output.engine.name, output.engine.version),
        engine_build_manifest_sha256: output.completion_build_manifest_sha256.clone(),
        cities: output
            .output
            .cities
            .iter()
            .map(|city| match &city.outcome {
                Ok(outcome) => {
                    let committed = by_city.get(city.city_id.as_str());
                    CityRunReport {
                        city_id: city.city_id.clone(),
                        result_digest: Some(outcome.result.result_digest.clone()),
                        error_code: None,
                        error_message: None,
                        documents: committed.map(|city| city.documents.clone()),
                        timings: Some(CityTimings {
                            pipeline_ms: outcome.timings.pipeline_ms,
                            charts_ms: outcome.timings.charts_ms,
                            documents_ms: outcome.timings.documents_ms,
                            commit_ms: committed.map(|city| city.commit_ms).unwrap_or(0),
                            // Final document rendering is intentionally outside calculation.
                            docx_ms: 0,
                            // The runtime timing contract now owns three
                            // stable phases only. Per-stage timings are absent
                            // rather than reconstructed or fabricated here.
                            stage_ms: BTreeMap::new(),
                        }),
                    }
                }
                Err(error) => CityRunReport {
                    city_id: city.city_id.clone(),
                    result_digest: None,
                    error_code: Some(error.code.as_str().to_string()),
                    error_message: Some(error.message.clone()),
                    documents: None,
                    timings: None,
                },
            })
            .collect(),
        portfolio: None,
        batch,
        sync_intent,
        publications,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// A durable selector for one committed document.
///
/// Not a path. The run, the city and the file name are validated segments the
/// caller already holds from its own durable receipt; this module resolves the
/// location inside the approved workspace. That is what keeps an operator's
/// filesystem out of the web view and out of an agent's context.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentCommand {
    /// The approved workspace.
    pub workspace_handle: String,
    /// Root collection.
    pub root: String,
    /// The run that produced it.
    pub run_id: String,
    /// The city it belongs to.
    pub city_id: String,
    /// The document's file name, e.g. `apd-fr.md`.
    pub name: String,
    /// Where to start reading, for the bounded read.
    #[serde(default)]
    pub offset: Option<u64>,
}

/// A deliberately narrow selector for the calculated 24-hour demand frame.
///
/// The browser may read this one JSON view because it is a small, immutable
/// projection of the sealed city result.  It is not a general JSON-file read:
/// the name is fixed here, inside the native authority boundary.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoadProfileCommand {
    /// The approved workspace.
    pub workspace_handle: String,
    /// Root collection.
    pub root: String,
    /// The run that produced it.
    pub run_id: String,
    /// The city it belongs to.
    pub city_id: String,
    /// Where to start reading, for the bounded read.
    #[serde(default)]
    pub offset: Option<u64>,
}

/// Selector for importing one operator-interpreted Markdown final.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportFinalCommand {
    pub workspace_handle: String,
    pub root: String,
    pub run_id: String,
    pub city_id: String,
    /// Optional explicit path supplied by the paired CLI. The desktop UI omits
    /// it and gets the native picker. In either case Rust validates and reads
    /// the source; the web view never receives source bytes.
    #[serde(default)]
    pub source_path: Option<String>,
}

/// Inspect one independently committed final after interruption or restart.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FinalSelector {
    pub workspace_handle: String,
    pub root: String,
    pub run_id: String,
    pub city_id: String,
}

/// Files committed by one explicit final import.
#[derive(Debug, Clone, Serialize)]
pub struct ImportedFinal {
    pub markdown: DocumentFile,
    /// Independent closure; importing never changes the calculation batch.
    pub final_batch_id: String,
    pub final_batch_digest: String,
    pub source_batch_id: String,
    pub source_batch_digest: String,
    /// Local sync retention is separate from successful reviewed-file import.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_error: Option<String>,
    /// Legacy response field retained for old consumers; current imports keep
    /// exact Markdown only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docx: Option<DocumentFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docx_error: Option<String>,
}

/// A selector for exactly one byte artifact declared by a closed native batch.
///
/// `name` is logical (`cities/<city>/result.json`, never an operator path),
/// and native code checks it against the immutable `batch.json` before it
/// reads any bytes. This is the publication lane's authority boundary.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchArtifactCommand {
    /// The approved workspace.
    pub workspace_handle: String,
    /// Root collection.
    pub root: String,
    /// The closed run that committed the batch.
    pub run_id: String,
    /// One logical name declared in that batch.
    pub name: String,
    /// Hard cap for bytes crossing IPC into the resumable uploader.
    pub max_bytes: u64,
}

/// A request to list one run's committed documents.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunDocumentsCommand {
    /// The approved workspace.
    pub workspace_handle: String,
    /// Root collection.
    pub root: String,
    /// The run to list.
    pub run_id: String,
    /// Restrict to these cities. Empty means every city the run committed.
    #[serde(default)]
    pub city_ids: Vec<String>,
}

/// One city's documents as they exist on disk right now.
#[derive(Debug, Clone, Serialize)]
pub struct CityDocumentInventory {
    /// Canonical city context id.
    pub city_id: String,
    /// Every document file present, with its current size and digest.
    pub files: Vec<DocumentFile>,
    /// How many chart files the city directory holds.
    pub chart_count: usize,
}

/// Aggregate artifacts present at a run's root.
#[derive(Debug, Clone, Serialize)]
pub struct PortfolioArtifactInventory {
    /// Every known aggregate artifact present, with current bytes metadata.
    pub files: Vec<DocumentFile>,
    /// Verified operator-facing result/report files. Inputs and internal
    /// artifacts never become UI outputs through a filename guess.
    pub outputs: Vec<DocumentFile>,
}

/// One bounded slice of a text document.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentSlice {
    /// The document's name.
    pub name: String,
    /// Where this slice starts.
    pub offset: u64,
    /// How many bytes it carries.
    pub bytes_returned: u64,
    /// How many bytes the whole document has.
    pub bytes_total: u64,
    /// Where the next slice starts.
    pub next_offset: u64,
    /// Whether this slice reaches the end.
    pub complete: bool,
    /// The slice itself, as text.
    pub content: String,
}

/// The largest slice one read returns. Matches the paired bridge's own bound.
const MAX_DOCUMENT_SLICE: u64 = 64 * 1024;
/// The largest document this module will read at all.
const MAX_DOCUMENT_BYTES: u64 = 32 * 1024 * 1024;
/// The uploader's hard cap for one sealed native batch artifact.
const MAX_BATCH_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
/// A closed manifest is metadata, never an unbounded artifact transport.
const MAX_BATCH_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

/// Resolve the root of one native run from only opaque durable authority.
pub(crate) fn run_directory(
    app: &AppHandle,
    workspace_handle: &str,
    root: &str,
    run_id: &str,
) -> Result<PathBuf, String> {
    let workspace = resolve_workspace_handle(app, workspace_handle)?;
    let root = checked_root(root)?;
    let run_id = checked_segment(run_id, "run id")?;
    secure_existing_private_directory(&native_root(&workspace).join("runs").join(root).join(run_id))
}

pub(crate) fn portfolio_run_directory(
    app: &AppHandle,
    workspace_handle: &str,
    root: &str,
    run_id: &str,
) -> Result<PathBuf, String> {
    let workspace = resolve_workspace_handle(app, workspace_handle)?;
    let root = checked_root(root)?;
    let run_id = checked_segment(run_id, "portfolio run id")?;
    secure_existing_private_directory(
        &native_root(&workspace)
            .join("portfolio-runs")
            .join(root)
            .join(run_id),
    )
}

fn portfolio_batch_inventory_at(run_dir: &Path) -> Result<PortfolioArtifactInventory, String> {
    let batch = closed_batch_at(run_dir)?;
    portfolio_batch_inventory_from(run_dir, &batch)
}

fn portfolio_batch_inventory_from(
    run_dir: &Path,
    batch: &ds_solar_contracts::SolarArtifactBatch,
) -> Result<PortfolioArtifactInventory, String> {
    let mut files = Vec::with_capacity(batch.artifacts.len());
    for artifact in &batch.artifacts {
        if artifact.city_id.is_some() {
            return Err("a portfolio batch declared a city-owned output".into());
        }
        verified_batch_artifact_at(run_dir, batch, &artifact.name, MAX_BATCH_ARTIFACT_BYTES)?;
        files.push(DocumentFile {
            name: artifact.name.clone(),
            media_type: artifact.media_type.clone(),
            byte_count: artifact.byte_count,
            content_digest: artifact.content_digest.clone(),
        });
    }
    let outputs = batch
        .artifacts
        .iter()
        .zip(&files)
        .filter(|(artifact, _)| is_portfolio_batch_output(artifact))
        .map(|(_, file)| file.clone())
        .collect();
    Ok(PortfolioArtifactInventory { files, outputs })
}

/// Validate a logical batch name before resolving it below a native run.
///
/// The later batch lookup is the authority check; this structural check keeps
/// a hostile string from becoming a filesystem traversal before that lookup.
fn checked_batch_artifact_name(value: &str) -> Result<Vec<&str>, String> {
    if value.is_empty() || value.len() > 512 || value.starts_with('/') || value.ends_with('/') {
        return Err("that is not a usable native Solar batch artifact name".to_string());
    }
    let parts: Vec<&str> = value.split('/').collect();
    if parts.is_empty() || parts.iter().any(|part| !valid_solar_segment(part)) {
        return Err("that is not a usable native Solar batch artifact name".to_string());
    }
    Ok(parts)
}

/// Read a batch manifest only after its run selector has been resolved.
fn closed_batch_at(run_dir: &Path) -> Result<ds_solar_contracts::SolarArtifactBatch, String> {
    let run_dir = secure_existing_private_directory(run_dir)?;
    let path = committed_file(run_dir.join("batch.json"), "batch manifest")?;
    let bytes = read_bounded_regular_file(&path, "batch manifest", MAX_BATCH_MANIFEST_BYTES)?;
    let batch: ds_solar_contracts::SolarArtifactBatch = serde_json::from_slice(&bytes)
        .map_err(|error| format!("that Solar batch manifest is invalid: {error}"))?;
    batch.validate().map_err(|error| {
        format!(
            "that Solar batch manifest does not validate: {}",
            error.message
        )
    })?;
    Ok(batch)
}

fn require_batch_selector(
    batch: &ds_solar_contracts::SolarArtifactBatch,
    root: &str,
    run_id: &str,
) -> Result<(), String> {
    if batch.run.root != root
        || batch.run.run_id != run_id
        || (root.starts_with("eds_project/")
            && root.split('/').nth(1) != Some(batch.run.project_id.as_str()))
    {
        return Err("the sealed Solar batch belongs to another project, root or run".into());
    }
    Ok(())
}

fn require_expected_portfolio_batch(
    batch: &ds_solar_contracts::SolarArtifactBatch,
    expected_batch_id: &str,
    expected_batch_digest: &str,
) -> Result<(), String> {
    if batch.batch_id != expected_batch_id || batch.batch_digest != expected_batch_digest {
        return Err(
            "that explicit portfolio batch no longer matches the caller's sealed receipt"
                .to_string(),
        );
    }
    Ok(())
}

fn require_expected_portfolio_artifact(
    artifact: &ds_solar_contracts::BatchArtifact,
    expected_media_type: &str,
    expected_byte_count: u64,
    expected_content_digest: &str,
) -> Result<(), String> {
    if artifact.media_type != expected_media_type
        || artifact.byte_count != expected_byte_count
        || artifact.content_digest != expected_content_digest
    {
        return Err(
            "that explicit portfolio artifact no longer matches the caller's sealed receipt"
                .to_string(),
        );
    }
    Ok(())
}

/// Refuse anything except a regular artifact file after resolving its approved
/// logical location. This keeps every selector from accepting an arbitrary
/// operator path or a symlink in place of an expected output.
fn committed_file(path: PathBuf, label: &str) -> Result<PathBuf, String> {
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|error| format!("that Solar {label} is unavailable: {error}"))?;
    if !metadata.is_file() {
        return Err(format!("that Solar {label} is not a file"));
    }
    Ok(path)
}

/// Read a regular file under an explicit allocation ceiling. The metadata
/// check rejects an already-oversized file before allocation; `take(max + 1)`
/// catches growth or replacement between that check and the read.
fn read_bounded_regular_file(path: &Path, label: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    if max_bytes == 0 || max_bytes > MAX_BATCH_ARTIFACT_BYTES {
        return Err(format!("that Solar {label} read bound is invalid"));
    }
    let path_metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("that Solar {label} is unavailable: {error}"))?;
    if !path_metadata.is_file() {
        return Err(format!("that Solar {label} is not a file"));
    }
    let file = std::fs::File::open(path)
        .map_err(|error| format!("that Solar {label} is unreadable: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("that Solar {label} is unreadable: {error}"))?;
    if !metadata.is_file() {
        return Err(format!("that Solar {label} is not a file"));
    }
    if metadata.len() > max_bytes {
        return Err(format!("that Solar {label} exceeds its read bound"));
    }
    let capacity = usize::try_from(metadata.len())
        .map_err(|_| format!("that Solar {label} does not fit this platform's address space"))?;
    let read_limit = max_bytes
        .checked_add(1)
        .ok_or_else(|| format!("that Solar {label} read bound overflowed"))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("that Solar {label} is unreadable: {error}"))?;
    if bytes.len() as u64 > max_bytes || bytes.len() as u64 != metadata.len() {
        return Err(format!(
            "that Solar {label} changed during its bounded read"
        ));
    }
    Ok(bytes)
}

/// One artifact whose bytes were rechecked against the closed batch before use.
///
/// Keeping the descriptor and bytes together prevents the portfolio
/// selectors from accidentally validating one source and then describing or
/// previewing another. The generic batch reader below uses the same value.
struct VerifiedBatchArtifact {
    descriptor: ds_solar_contracts::BatchArtifact,
    bytes: Vec<u8>,
}

/// The one place a logical name is matched against the closed manifest.
///
/// The bounded readers and the streaming transfer door below must agree on
/// exactly which declaration a name resolves to, so neither carries its own
/// copy of this lookup.
fn declared_batch_artifact<'a>(
    batch: &'a ds_solar_contracts::SolarArtifactBatch,
    name: &str,
) -> Result<&'a ds_solar_contracts::BatchArtifact, String> {
    batch
        .artifacts
        .iter()
        .find(|artifact| artifact.name == name)
        .ok_or_else(|| "that artifact is not declared by the closed native Solar batch".to_string())
}

/// Resolve where one declared artifact's bytes live, without reading them.
///
/// `parts` has already been through [`checked_batch_artifact_name`], and the
/// parent directory is re-proved to be an unlinked private directory before the
/// file itself is accepted — so neither a hostile name nor a planted link can
/// substitute a different file for a declared one.
fn committed_batch_artifact_path(run_dir: &Path, parts: &[&str]) -> Result<PathBuf, String> {
    let path = parts
        .iter()
        .fold(run_dir.to_path_buf(), |path, part| path.join(part));
    let parent = path
        .parent()
        .ok_or_else(|| "that native Solar batch artifact has no parent directory".to_string())?;
    secure_existing_private_directory(parent)?;
    committed_file(path, "batch artifact")
}

/// Read one declared artifact and prove that its materialized bytes still match
/// the immutable batch declaration exactly.
fn verified_batch_artifact_at(
    run_dir: &Path,
    batch: &ds_solar_contracts::SolarArtifactBatch,
    name: &str,
    max_bytes: u64,
) -> Result<VerifiedBatchArtifact, String> {
    if max_bytes == 0 || max_bytes > MAX_BATCH_ARTIFACT_BYTES {
        return Err("the native Solar batch artifact read bound is invalid".to_string());
    }
    let run_dir = secure_existing_private_directory(run_dir)?;
    let parts = checked_batch_artifact_name(name)?;
    if !batch.artifacts.iter().any(|artifact| artifact.name == name) && is_final_artifact(name) {
        let (directory, final_batch) = final_context_at(&run_dir, batch, name)?;
        return verified_batch_artifact_at(&directory, &final_batch, name, max_bytes);
    }
    if !batch.artifacts.iter().any(|artifact| artifact.name == name)
        && portfolio_finals::source_name(name).is_some()
    {
        let (directory, final_batch) = portfolio_finals::context(&run_dir, batch, name)?;
        return verified_batch_artifact_at(&directory, &final_batch, name, max_bytes);
    }
    let declared = declared_batch_artifact(batch, name)?;
    if declared.byte_count > max_bytes {
        return Err(
            "that native Solar batch artifact exceeds the requested read bound".to_string(),
        );
    }
    let path = committed_batch_artifact_path(&run_dir, &parts)?;
    let bytes = read_bounded_regular_file(&path, "batch artifact", max_bytes)?;
    if bytes.len() as u64 != declared.byte_count
        || ds_solar_contracts::digest::sha256_hex(&bytes) != declared.content_digest
    {
        return Err(
            "that native Solar batch artifact no longer matches its closed manifest".to_string(),
        );
    }
    Ok(VerifiedBatchArtifact {
        descriptor: declared.clone(),
        bytes,
    })
}

// ---------------------------------------------------------------------------
// The streaming transfer door
// ---------------------------------------------------------------------------
//
// The readers above collect an artifact into a `Vec<u8>`, which is why they
// carry a 64 MiB allocation bound. Publication does not need the bytes in this
// process at all: it needs a verified handle it can stream from.
//
// So this door enforces the SAME closure — the approved workspace, the root,
// the run, the closed `batch.json`, the artifact's own declaration, and the
// caller's sealed receipt — and then hands back a file handle instead of a
// buffer. `MAX_BATCH_ARTIFACT_BYTES` deliberately does not apply: it bounds an
// allocation, and there is no allocation here. A publication above the server's
// own ceiling is still refused by ds-brain at `open`
// (`ds-transport-authority.md` §4.3 and §10).

/// A closed, manifest-verified Solar artifact opened for STREAMING transfer.
///
/// The transfer host re-hashes every byte it writes and refuses to report
/// success unless the result equals [`SealedSolarArtifact::sha256`], so this
/// handle carries exactly the two facts that verification needs.
pub struct SealedSolarArtifact {
    pub file: std::fs::File,
    pub size_bytes: u64,
    /// Bare lowercase hex. The manifest spells its digests `sha256:<hex>`; the
    /// prefixed form is compared against the caller's receipt *before* it is
    /// stripped, and the bare form is what a streaming digest produces.
    pub sha256: String,
}

/// The closure one native Solar city-batch transfer must satisfy before a byte
/// moves.
///
/// This is the browser adapter's closure restated where the bytes actually
/// are: the approved workspace, the root, the run, and one logical name the
/// closed batch declares — plus the sealed receipt the row was queued with, so
/// the transfer can prove the bytes it streams are the bytes it promised. Every
/// field is required: an absent expectation refuses rather than transferring an
/// unverified artifact.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SealedBatchArtifactAuthority {
    pub workspace_handle: String,
    pub root: String,
    pub run_id: String,
    pub name: String,
    pub expected_byte_count: u64,
    /// `sha256:<hex>`, exactly as the closed manifest spells it.
    pub expected_content_digest: String,
}

/// The aggregate sibling: the same closure over the `portfolio-runs/…`
/// namespace, plus the exact batch and artifact receipt the browser reader
/// already enforces.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SealedPortfolioBatchArtifactAuthority {
    pub workspace_handle: String,
    pub root: String,
    pub run_id: String,
    pub name: String,
    pub expected_batch_id: String,
    pub expected_batch_digest: String,
    pub expected_media_type: String,
    pub expected_byte_count: u64,
    pub expected_content_digest: String,
}

/// Refuse a publication that names no approved workspace before anything tries
/// to resolve one.
fn require_approved_workspace(handle: &str) -> Result<(), String> {
    if !valid_workspace_handle(handle) {
        return Err("that native Solar publication has no approved workspace".to_string());
    }
    Ok(())
}

/// Strip the manifest's digest prefix, refusing anything that is not the exact
/// `sha256:<64 lowercase hex>` spelling. A permissive parse here would hand the
/// transfer host a value its streaming digest could never equal, turning a
/// malformed manifest into an unexplained integrity failure.
fn bare_hex_digest(prefixed: &str) -> Result<&str, String> {
    prefixed
        .strip_prefix("sha256:")
        .filter(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| "that native Solar batch artifact declares no usable content digest".into())
}

/// Open one declared artifact for streaming, against the caller's own receipt.
fn open_sealed_declared_artifact(
    run_dir: &Path,
    batch: &ds_solar_contracts::SolarArtifactBatch,
    name: &str,
    expected_byte_count: u64,
    expected_content_digest: &str,
) -> Result<SealedSolarArtifact, String> {
    let run_dir = secure_existing_private_directory(run_dir)?;
    let parts = checked_batch_artifact_name(name)?;
    let declared = declared_batch_artifact(batch, name)?;
    if expected_byte_count == 0 {
        return Err("that native Solar publication has no sealed byte count".to_string());
    }
    if declared.byte_count != expected_byte_count
        || declared.content_digest != expected_content_digest
    {
        return Err(
            "that native Solar batch artifact no longer matches the caller's sealed receipt"
                .to_string(),
        );
    }
    let sha256 = bare_hex_digest(&declared.content_digest)?.to_string();
    let path = committed_batch_artifact_path(&run_dir, &parts)?;
    let file = std::fs::File::open(&path)
        .map_err(|error| format!("that Solar batch artifact is unreadable: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("that Solar batch artifact is unreadable: {error}"))?;
    // Re-read through the opened handle: the earlier `symlink_metadata` check
    // proved what was at the path, this proves what is at the other end of the
    // descriptor the transfer will actually stream from.
    if !metadata.is_file() {
        return Err("that Solar batch artifact is not a file".to_string());
    }
    if metadata.len() != declared.byte_count {
        return Err(
            "that native Solar batch artifact no longer matches its closed manifest".to_string(),
        );
    }
    Ok(SealedSolarArtifact {
        file,
        size_bytes: declared.byte_count,
        sha256,
    })
}

/// Open one city-batch artifact for streaming, from an approved workspace.
pub fn open_sealed_batch_artifact(
    app: &AppHandle,
    authority: &SealedBatchArtifactAuthority,
) -> Result<SealedSolarArtifact, String> {
    let run_dir = run_directory(
        app,
        &authority.workspace_handle,
        &authority.root,
        &authority.run_id,
    )?;
    open_sealed_batch_artifact_at(&run_dir, authority)
}

/// The path-resolved half, so the closure can be proved without a workspace.
///
/// The handle is re-checked here rather than only where it resolves a path: a
/// publication that carries no approved workspace must be REFUSED, never given
/// a defaulted one, and that has to hold for every caller of this function.
pub fn open_sealed_batch_artifact_at(
    run_dir: &Path,
    authority: &SealedBatchArtifactAuthority,
) -> Result<SealedSolarArtifact, String> {
    require_approved_workspace(&authority.workspace_handle)?;
    let batch = closed_batch_at(run_dir)?;
    require_batch_selector(&batch, &authority.root, &authority.run_id)?;
    if !batch
        .artifacts
        .iter()
        .any(|artifact| artifact.name == authority.name)
        && is_final_artifact(&authority.name)
    {
        let (directory, final_batch) = final_context_at(run_dir, &batch, &authority.name)?;
        return open_sealed_declared_artifact(
            &directory,
            &final_batch,
            &authority.name,
            authority.expected_byte_count,
            &authority.expected_content_digest,
        );
    }
    open_sealed_declared_artifact(
        run_dir,
        &batch,
        &authority.name,
        authority.expected_byte_count,
        &authority.expected_content_digest,
    )
}

/// Open one aggregate portfolio-batch artifact for streaming.
pub fn open_sealed_portfolio_batch_artifact(
    app: &AppHandle,
    authority: &SealedPortfolioBatchArtifactAuthority,
) -> Result<SealedSolarArtifact, String> {
    let run_dir = portfolio_run_directory(
        app,
        &authority.workspace_handle,
        &authority.root,
        &authority.run_id,
    )?;
    open_sealed_portfolio_batch_artifact_at(&run_dir, authority)
}

/// The path-resolved half. Order matches the bounded portfolio reader exactly:
/// the batch receipt, then portfolio ownership, then the artifact receipt.
pub fn open_sealed_portfolio_batch_artifact_at(
    run_dir: &Path,
    authority: &SealedPortfolioBatchArtifactAuthority,
) -> Result<SealedSolarArtifact, String> {
    require_approved_workspace(&authority.workspace_handle)?;
    let batch = closed_batch_at(run_dir)?;
    require_expected_portfolio_batch(
        &batch,
        &authority.expected_batch_id,
        &authority.expected_batch_digest,
    )?;
    let (final_directory, final_batch);
    let (run_dir, batch) = if !batch
        .artifacts
        .iter()
        .any(|artifact| artifact.name == authority.name)
        && portfolio_finals::source_name(&authority.name).is_some()
    {
        (final_directory, final_batch) =
            portfolio_finals::context(run_dir, &batch, &authority.name)?;
        (final_directory.as_path(), &final_batch)
    } else {
        (run_dir, &batch)
    };
    let declared = declared_batch_artifact(batch, &authority.name)?;
    if declared.city_id.is_some() {
        return Err("that is not a portfolio-level artifact".to_string());
    }
    require_expected_portfolio_artifact(
        declared,
        &authority.expected_media_type,
        authority.expected_byte_count,
        &authority.expected_content_digest,
    )?;
    open_sealed_declared_artifact(
        run_dir,
        batch,
        &authority.name,
        authority.expected_byte_count,
        &authority.expected_content_digest,
    )
}

fn is_explicit_portfolio_report_name(name: &str) -> bool {
    portfolio_finals::source_name(name).is_some()
        || matches!(
            name,
            "portfolio-draft-fr.md"
                | "portfolio-reseau-fr.md"
                | "portfolio-centrale-fr.md"
                | "portfolio-financier-fr.md"
                | "portfolio-apd-en.md"
                | "portfolio-draft-en.md"
                | "portfolio-network-en.md"
                | "portfolio-plant-en.md"
                | "portfolio-financial-en.md"
        )
}

/// Reopen a portfolio batch and accept only its operator-facing outputs.
///
/// The batch manifest is the authority for bytes and ownership. The fixed
/// output names plus exact role/media checks keep a caller from opening or
/// exporting the provenance input (or any future internal artifact) merely by
/// learning its logical name.
#[cfg(test)]
fn verified_portfolio_batch_output_at(
    run_dir: &Path,
    name: &str,
) -> Result<VerifiedBatchArtifact, String> {
    let batch = closed_batch_at(run_dir)?;
    verified_portfolio_batch_output_from(run_dir, &batch, name)
}

fn verified_portfolio_batch_output_from(
    run_dir: &Path,
    batch: &ds_solar_contracts::SolarArtifactBatch,
    name: &str,
) -> Result<VerifiedBatchArtifact, String> {
    if name != PORTFOLIO_RESULT_NAME && !is_explicit_portfolio_report_name(name) {
        return Err(
            "that artifact is not a declared explicit portfolio result or report".to_string(),
        );
    }
    let verified = verified_batch_artifact_at(run_dir, batch, name, MAX_DOCUMENT_BYTES)?;
    if !is_portfolio_batch_output(&verified.descriptor) {
        return Err(
            "that artifact is not a declared explicit portfolio result or report".to_string(),
        );
    }
    Ok(verified)
}

fn is_portfolio_batch_output(declared: &ds_solar_contracts::BatchArtifact) -> bool {
    let name = declared.name.as_str();
    let valid_result = name == PORTFOLIO_RESULT_NAME
        && declared.role == ds_solar_contracts::ArtifactRole::PortfolioResult
        && declared.media_type == PORTFOLIO_RESULT_MEDIA_TYPE;
    let valid_report = is_explicit_portfolio_report_name(name)
        && declared.role == ds_solar_contracts::ArtifactRole::Report
        && declared.media_type == MARKDOWN_MEDIA_TYPE;
    declared.city_id.is_none() && (valid_result || valid_report)
}

/// Resolve one committed document inside the approved workspace.
fn document_artifact(
    app: &AppHandle,
    command: &DocumentCommand,
) -> Result<VerifiedBatchArtifact, String> {
    let city_id = checked_segment(&command.city_id, "city id")?;
    let name = checked_segment(&command.name, "document name")?;
    let run_dir = run_directory(
        app,
        &command.workspace_handle,
        &command.root,
        &command.run_id,
    )?;
    let batch = closed_batch_at(&run_dir)?;
    require_batch_selector(&batch, &command.root, &command.run_id)?;
    verified_batch_artifact_at(
        &run_dir,
        &batch,
        &format!("cities/{city_id}/{name}"),
        MAX_DOCUMENT_BYTES,
    )
}

fn file_descriptor(name: &str, media_type: &str, bytes: &[u8]) -> DocumentFile {
    DocumentFile {
        name: name.to_string(),
        media_type: media_type.to_string(),
        byte_count: bytes.len() as u64,
        content_digest: ds_solar_contracts::digest::sha256_hex(bytes),
    }
}

fn read_text_bytes(
    name: String,
    offset: Option<u64>,
    bytes: &[u8],
) -> Result<DocumentSlice, String> {
    let total = bytes.len() as u64;
    if total > MAX_DOCUMENT_BYTES {
        return Err("that Solar document is larger than this shell will read".to_string());
    }
    let offset = offset.unwrap_or(0).min(total);
    let mut end = (offset + MAX_DOCUMENT_SLICE).min(total) as usize;
    let start = offset as usize;
    // Never split a UTF-8 character: back up to the nearest boundary.
    while end > start && std::str::from_utf8(&bytes[start..end]).is_err() {
        end -= 1;
    }
    let content = String::from_utf8_lossy(&bytes[start..end]).into_owned();
    Ok(DocumentSlice {
        name,
        offset,
        bytes_returned: (end as u64) - offset,
        bytes_total: total,
        next_offset: end as u64,
        complete: end as u64 >= total,
        content,
    })
}

/// List what one run actually committed, read from disk.
///
/// Reads only, and reads the workspace rather than a receipt, so a run from a
/// previous session resolves exactly as well as one that finished a second ago.
pub fn solar_native_run_documents(
    app: AppHandle,
    command: RunDocumentsCommand,
) -> Result<Vec<CityDocumentInventory>, String> {
    if command.city_ids.len() > MAX_CITIES_PER_CALL {
        return Err("too many cities in one Solar document listing".to_string());
    }
    let run_dir = run_directory(
        &app,
        &command.workspace_handle,
        &command.root,
        &command.run_id,
    )?;
    let cities_dir = secure_existing_private_directory(&run_dir.join("cities"))?;
    let wanted: Vec<String> = if command.city_ids.is_empty() {
        let entries = std::fs::read_dir(&cities_dir)
            .map_err(|error| format!("that Solar run has no committed cities: {error}"))?;
        let mut listed = Vec::new();
        for (index, entry) in entries.enumerate() {
            if index >= MAX_CITIES_PER_CALL {
                return Err("that Solar run has too many committed city entries".to_string());
            }
            let entry =
                entry.map_err(|error| format!("a committed Solar city is unreadable: {error}"))?;
            if !entry
                .file_type()
                .map_err(|error| format!("a committed Solar city is unreadable: {error}"))?
                .is_dir()
            {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if valid_solar_segment(&name) {
                listed.push(name);
            }
        }
        listed.sort();
        listed
    } else {
        command
            .city_ids
            .iter()
            .map(|city_id| checked_segment(city_id, "city id"))
            .collect::<Result<Vec<_>, _>>()?
    };

    let mut inventory = Vec::with_capacity(wanted.len());
    for city_id in wanted {
        let city_dir = secure_existing_private_directory(&cities_dir.join(&city_id))?;
        let mut files = Vec::new();
        for (name, media_type) in [
            (PRODUCT_APD_NAME, MARKDOWN_MEDIA_TYPE),
            (PARITY_DRAFT_NAME, MARKDOWN_MEDIA_TYPE),
            (APD_DRAFT_EN_NAME, MARKDOWN_MEDIA_TYPE),
            (NETWORK_DRAFT_NAME, MARKDOWN_MEDIA_TYPE),
            (NETWORK_DRAFT_EN_NAME, MARKDOWN_MEDIA_TYPE),
            (PLANT_DRAFT_NAME, MARKDOWN_MEDIA_TYPE),
            (PLANT_DRAFT_EN_NAME, MARKDOWN_MEDIA_TYPE),
            (FINANCIAL_DRAFT_NAME, MARKDOWN_MEDIA_TYPE),
            (FINANCIAL_DRAFT_EN_NAME, MARKDOWN_MEDIA_TYPE),
            (DOCX_NAME, DOCX_MEDIA_TYPE),
        ] {
            let path = city_dir.join(name);
            let Ok(bytes) = read_bounded_regular_file(&path, "document", MAX_DOCUMENT_BYTES) else {
                continue;
            };
            files.push(file_descriptor(name, media_type, &bytes));
        }
        inventory.push(CityDocumentInventory {
            city_id,
            chart_count: count_charts(&city_dir)?,
            files,
        });
    }
    Ok(inventory)
}

/// Preserve the caller's report order and reject duplicate intent rather than
/// silently changing the contract at the native boundary.
fn checked_portfolio_report_intents(
    values: Vec<String>,
) -> Result<Vec<PortfolioReportIntent>, String> {
    if values.is_empty() {
        return Err("select at least one portfolio report intent".into());
    }
    let mut intents = Vec::with_capacity(values.len());
    for value in values {
        let intent = match value.as_str() {
            "apd" => PortfolioReportIntent::Apd,
            "network" => PortfolioReportIntent::Network,
            "plant" => PortfolioReportIntent::Plant,
            "financial" => PortfolioReportIntent::Financial,
            _ => return Err(format!("unsupported portfolio report intent: {value}")),
        };
        if intents.contains(&intent) {
            return Err(format!(
                "portfolio report intent '{value}' was selected more than once"
            ));
        }
        intents.push(intent);
    }
    Ok(intents)
}

/// Every scalar/bookkeeping key the sealed graph section carries beside its
/// per-section graph objects.
///
/// This list is the reason `_start_index` needed a home: it is metadata, and
/// omitting it here made the section walk below read a bare integer as a graph
/// object and refuse every correct round-robin aggregate. Adding a key to the
/// engine's graph section without adding it here is the same bug again.
const PORTFOLIO_GRAPH_METADATA_KEYS: &[&str] = &[
    "_strategy",
    "_start_index",
    "_representative_cities",
    "_unavailable",
    "graph_count",
    "graphs",
];

/// Validate the sealed graph section's strategy metadata before anything reads
/// it as graph content.
///
/// `_start_index` belongs to `round_robin` alone: it is the member offset the
/// engine rotated from, so it is bounded by the result's own member count and
/// is refused outright under any other strategy. Checking it here — against the
/// member count the caller sealed — keeps the shell an independent verifier
/// rather than a second copy of the engine's own rule.
fn verify_portfolio_graph_strategy_metadata(
    object: &serde_json::Map<String, serde_json::Value>,
    member_count: u64,
) -> Result<(), String> {
    let strategy = object
        .get("_strategy")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            "portfolio representative graphs do not declare their strategy".to_string()
        })?;
    if !matches!(
        strategy,
        "first" | "round_robin" | "explicit" | "digest_seeded_random"
    ) {
        return Err(format!(
            "portfolio representative graph strategy '{strategy}' is unsupported"
        ));
    }
    let declared = object.get("_start_index");
    if strategy != "round_robin" {
        if declared.is_some() {
            return Err(format!(
                "portfolio graph strategy '{strategy}' cannot declare _start_index"
            ));
        }
        return Ok(());
    }
    let start_index = declared
        .ok_or_else(|| {
            "round-robin representative graphs require an unsigned _start_index".to_string()
        })?
        .as_u64()
        .ok_or_else(|| {
            "round-robin representative graph _start_index is not an unsigned integer".to_string()
        })?;
    if member_count == 0 || start_index >= member_count {
        return Err(format!(
            "round-robin representative graph start index {start_index} is outside {member_count} result members"
        ));
    }
    Ok(())
}

/// Reopen every graph referenced by the sealed portfolio result and prove its
/// bytes against the exact source-batch descriptor before the aggregate can be
/// committed. The result carries logical receipts, never filesystem paths;
/// this is the byte-level half of that contract.
fn verify_portfolio_graph_artifacts(
    value: &serde_json::Value,
    source_dir: &Path,
    source_batch: &ds_solar_contracts::SolarArtifactBatch,
    member_count: u64,
) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "portfolio representative_graphs is not an object".to_string())?;
    verify_portfolio_graph_strategy_metadata(object, member_count)?;
    let rows = object
        .get("graphs")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "portfolio representative_graphs.graphs is not an array".to_string())?;
    if rows.len() > 64
        || object
            .get("graph_count")
            .and_then(serde_json::Value::as_u64)
            != Some(rows.len() as u64)
    {
        return Err("portfolio representative graph count is invalid".into());
    }

    let mut logical_rows = BTreeSet::new();
    let mut artifact_names = BTreeSet::new();
    for row in rows {
        let row_object = row
            .as_object()
            .ok_or_else(|| "a portfolio representative graph row is not an object".to_string())?;
        let section = row_object
            .get("section")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "a portfolio representative graph row has no section".to_string())?;
        let graph_key = row_object
            .get("graph_key")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "a portfolio representative graph row has no graph_key".to_string())?;
        let receipt: RepresentativeGraphArtifactReceipt =
            serde_json::from_value(row_object.get("url").cloned().ok_or_else(|| {
                "a portfolio representative graph row has no receipt".to_string()
            })?)
            .map_err(|error| {
                format!("a portfolio representative graph receipt is invalid: {error}")
            })?;
        receipt.validate().map_err(|error| {
            format!("a portfolio representative graph receipt is invalid: {error}")
        })?;
        if receipt.source_batch_id != source_batch.batch_id
            || receipt.source_batch_digest != source_batch.batch_digest
            || row_object
                .get("city_id")
                .and_then(serde_json::Value::as_str)
                != Some(receipt.city_id.as_str())
            || !logical_rows.insert((section.to_string(), graph_key.to_string()))
            || !artifact_names.insert(receipt.name.clone())
        {
            return Err("portfolio representative graph provenance is inconsistent".into());
        }
        let section_row = object
            .get(section)
            .and_then(serde_json::Value::as_object)
            .and_then(|entries| entries.get(graph_key));
        if section_row != Some(row) {
            return Err(
                "portfolio representative graph index does not match its section row".into(),
            );
        }
        let verified = verified_batch_artifact_at(
            source_dir,
            source_batch,
            &receipt.name,
            MAX_BATCH_ARTIFACT_BYTES,
        )?;
        let descriptor = verified.descriptor;
        if descriptor.role != receipt.role
            || descriptor.city_id.as_deref() != Some(receipt.city_id.as_str())
            || descriptor.media_type != receipt.media_type
            || descriptor.byte_count != receipt.byte_count
            || descriptor.content_digest != receipt.content_digest
        {
            return Err(
                "portfolio representative graph bytes do not match their sealed receipt".into(),
            );
        }
    }

    for (section, entries) in object {
        if PORTFOLIO_GRAPH_METADATA_KEYS.contains(&section.as_str()) {
            continue;
        }
        let entries = entries.as_object().ok_or_else(|| {
            format!("portfolio representative graph section '{section}' is not an object")
        })?;
        for (graph_key, row) in entries {
            if !logical_rows.contains(&(section.clone(), graph_key.clone()))
                || !rows.iter().any(|candidate| candidate == row)
            {
                return Err(
                    "portfolio representative graph sections contain an unindexed row".into(),
                );
            }
        }
    }
    Ok(())
}

fn portfolio_cancel_checkpoint(cancel: &CancelToken) -> Result<(), String> {
    if cancel.is_cancelled() {
        return Err("portfolio calculation was cancelled (cancelled)".to_string());
    }
    Ok(())
}

/// Removes only the fresh, exact `.partial` directory created by the current
/// portfolio invocation. Once the atomic rename succeeds the guard is
/// disarmed, so a terminal batch is never deleted by cancellation cleanup.
struct PendingPortfolioDirectory {
    path: PathBuf,
    armed: bool,
}

impl PendingPortfolioDirectory {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn commit(&mut self, committed_dir: &Path) -> Result<(), String> {
        std::fs::rename(&self.path, committed_dir)
            .map_err(|error| format!("portfolio batch could not be committed: {error}"))?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for PendingPortfolioDirectory {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Ok(metadata) = std::fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// Execute one ordered portfolio selection from one exact closed city batch.
/// City and portfolio batches live in separate directories and never share a
/// run id namespace, which avoids a source-batch digest cycle.
pub fn solar_native_calculate_portfolio(
    app: AppHandle,
    command: PortfolioCalculateCommand,
) -> Result<PortfolioRunReport, String> {
    calculate_portfolio_blocking(app, command)
}

/// The lane receipt and process capture have the same accepted-job lifetime.
/// Active-pointer changes cannot redirect this value. Keep private results inside
/// the postcheck; callers may adopt bytes only after execute returns successfully.
struct CapturedEngine<T> {
    selection: ds_cli_exec::CapturedExternal,
    process: T,
}
impl<T> CapturedEngine<T> {
    fn capture(
        selection: ds_cli_exec::CapturedExternal,
        capture: impl FnOnce(&Path) -> Result<T, ds_solar_host::process::ProcessFailure>,
    ) -> Result<Self, String> {
        let process = selection
            .with_verified_executable(capture)
            .map_err(|error| format!("{}: {error}", error.code()))?
            .map_err(|error| error.to_string())?;
        Ok(Self { selection, process })
    }

    fn execute<R>(self, task: impl FnOnce(T) -> Result<R, String>) -> Result<R, String> {
        self.selection
            .with_verified_executable(|_| task(self.process))
            .map_err(|error| format!("{}: {error}", error.code()))?
    }
}

fn capture_portfolio_engine(
    app: &AppHandle,
    cancel: &CancelToken,
) -> Result<CapturedEngine<ds_solar_host::process::PortfolioEngine>, String> {
    portfolio_cancel_checkpoint(cancel)?;
    let scratch = secure_private_directory(&app.data_dir()?.join("solar-workers"))?;
    CapturedEngine::capture(app.capture_engine()?, |path| {
        ds_solar_host::process::PortfolioEngine::capture(
            path,
            &scratch,
            ds_solar_host::compute_parallelism(),
            cancel,
        )
    })
}

fn calculate_portfolio_blocking(
    app: AppHandle,
    command: PortfolioCalculateCommand,
) -> Result<PortfolioRunReport, String> {
    let cancel = CancelToken::new();
    let engine = capture_portfolio_engine(&app, &cancel)?;
    calculate_portfolio_with_cancel(app, command, cancel, engine)
}

fn calculate_portfolio_with_cancel(
    app: AppHandle,
    command: PortfolioCalculateCommand,
    cancel: CancelToken,
    engine: CapturedEngine<ds_solar_host::process::PortfolioEngine>,
) -> Result<PortfolioRunReport, String> {
    let state = app.state();
    let workspace = resolve_workspace_handle(&app, &command.workspace_handle)?;
    let root = checked_root(&command.root)?;
    let source_run_id = checked_segment(&command.source_run_id, "source run id")?;
    let run_id = checked_segment(&command.run_id, "portfolio run id")?;
    {
        let mut runs = state
            .runs
            .lock()
            .map_err(|_| "Solar run state is poisoned".to_string())?;
        if runs.contains_key(&run_id) {
            return Err("that Solar run id is already active".to_string());
        }
        runs.insert(run_id.clone(), cancel.clone());
    }
    let _run_guard = RunGuard {
        state: &state,
        run_id: run_id.clone(),
    };
    portfolio_cancel_checkpoint(&cancel)?;
    let source_dir = secure_existing_private_directory(
        &native_root(&workspace)
            .join("runs")
            .join(&root)
            .join(&source_run_id),
    )?;
    let source_batch = closed_batch_at(&source_dir)?;
    if source_batch.run.project_id != command.project_id || source_batch.run.root != root {
        return Err("the source Solar batch does not belong to this project and root".into());
    }
    ds_solar_contracts::verify_portfolio_source_membership(&source_batch, &command.city_ids)
        .map_err(|error| error.message)?;
    let source_batch_id = source_batch.batch_id.clone();
    let source_batch_digest = source_batch.batch_digest.clone();
    let source_city_result_digests = command
        .city_ids
        .iter()
        .map(|city_id| {
            source_batch
                .run
                .cities
                .iter()
                .find(|city| &city.city_id == city_id)
                .expect("source membership was admitted")
        })
        .map(|city| PortfolioSourceCityResultDigest {
            city_id: city.city_id.clone(),
            result_digest: city.result_digest.clone(),
        })
        .collect::<Vec<_>>();

    let representative_strategy = match command.representative_strategy.as_str() {
        "first" => PortfolioRepresentativeStrategy::First,
        "digest_seeded_random" => PortfolioRepresentativeStrategy::DigestSeededRandom,
        "explicit" => PortfolioRepresentativeStrategy::Explicit {
            city_id: checked_text(
                command
                    .representative_city_id
                    .as_deref()
                    .unwrap_or_default(),
                "representative city id",
            )?,
        },
        "round_robin" => PortfolioRepresentativeStrategy::RoundRobin {
            start_index: command.round_robin_start_index.ok_or_else(|| {
                "round_robin representative strategy requires start index".to_string()
            })?,
        },
        _ => return Err("unsupported portfolio representative strategy".into()),
    };
    let language = match command.language.as_str() {
        "fr" => PortfolioLanguage::Fr,
        "en" => PortfolioLanguage::En,
        _ => return Err("unsupported portfolio language".into()),
    };
    let intents = checked_portfolio_report_intents(command.report_intents)?;
    use ds_solar_contracts::portfolio_task::{
        PORTFOLIO_TASK_REQUEST_SCHEMA, PortfolioTaskAssertions, PortfolioTaskRequest,
        PortfolioTaskSource,
    };
    let raw_batch = read_bounded_regular_file(
        &source_dir.join("batch.json"),
        "source batch",
        MAX_BATCH_ARTIFACT_BYTES,
    )?;
    if serde_json::from_slice::<ds_solar_contracts::SolarArtifactBatch>(&raw_batch)
        .map_err(|_| "Invalid source batch")?
        != source_batch
    {
        return Err("The source Solar batch changed before calculation".into());
    }
    let request = PortfolioTaskRequest {
        schema_version: PORTFOLIO_TASK_REQUEST_SCHEMA.into(),
        source: PortfolioTaskSource {
            directory: source_dir
                .to_str()
                .ok_or("The approved Solar workspace path is not UTF-8")?
                .into(),
            manifest: ds_solar_contracts::ArtifactEntry::from_bytes(
                "batch.json",
                JSON_MEDIA_TYPE,
                &raw_batch,
            ),
            batch: source_batch.clone(),
        },
        identity: PortfolioIdentity {
            project_id: command.project_id,
            root: root.clone(),
            portfolio_id: checked_text(&command.portfolio_id, "portfolio id")?,
            portfolio_name: checked_text(&command.portfolio_name, "portfolio name")?,
            membership_revision: checked_text(
                &command.membership_revision,
                "portfolio membership revision",
            )?,
        },
        cities: command.city_ids,
        run_id: run_id.clone(),
        representative_strategy,
        language,
        documents: PortfolioDocumentOptions {
            report_intents: intents,
            formats: vec![PortfolioDocumentFormat::Markdown],
        },
        assertions: PortfolioTaskAssertions {
            currency: command.currency,
            project_years: command.project_years,
            discount_rate: command.discount_rate,
        },
    };
    let verified = engine.execute(|engine| {
        engine
            .calculate(&request, &cancel)
            .map_err(|error| match &error.refusal {
                Some(refusal) => format!("{} ({})", refusal.message, refusal.code.as_str()),
                None if error.kind == ds_solar_host::process::ProcessFailureKind::Cancelled => {
                    "Solar portfolio calculation was cancelled (cancelled)".into()
                }
                None => error.to_string(),
            })
    })?;
    let output = &verified.task;
    let input = &output.input;
    let completion = serde_json::json!({"schema":"ds-solar.native-engine-completion/v1",
        "engine":output.engine,"build_manifest_sha256":output.completion_build_manifest_sha256,
        "request_sha256":output.request_sha256,"batch_id":output.batch.batch_id,"batch_digest":output.batch.batch_digest,
        "input_digest":input.input_digest,"result_digest":output.result.portfolio_digest});

    let governed_currency = input.assumptions.currency.as_str().to_string();
    let governed_project_years = input.assumptions.project_years;
    let governed_discount_rate = input.assumptions.discount_rate;
    portfolio_cancel_checkpoint(&cancel)?;
    let representative_graphs = output
        .result
        .sections
        .get("representative_graphs")
        .ok_or_else(|| "the portfolio result has no representative graph section".to_string())?;
    verify_portfolio_graph_artifacts(
        representative_graphs,
        &source_dir,
        &source_batch,
        output.result.city_count,
    )?;
    portfolio_cancel_checkpoint(&cancel)?;

    let parent =
        secure_private_directory(&native_root(&workspace).join("portfolio-runs").join(&root))?;
    let committed_dir = parent.join(&run_id);
    let staging_dir = parent.join(format!("{run_id}.partial"));
    if committed_dir.exists() {
        let committed_dir = secure_existing_private_directory(&committed_dir).map_err(|_| {
            "that portfolio run id does not resolve to a committed directory".to_string()
        })?;
        let existing = closed_batch_at(&committed_dir)?;
        if existing != output.batch {
            return Err(
                "that portfolio run id is already committed for different exact inputs".into(),
            );
        }
        let files = portfolio_batch_inventory_at(&committed_dir)?.files;
        write_json(&committed_dir.join("engine-completion.json"), &completion)?;
        let (publication, publication_error) = seal_portfolio_completion(
            &app,
            &command.workspace_handle,
            &root,
            &run_id,
            &existing,
            &output.completion_build_manifest_sha256,
        );
        return Ok(PortfolioRunReport {
            run_id,
            engine_version: format!("{}@{}", output.engine.name, output.engine.version),
            engine_build_manifest_sha256: output.completion_build_manifest_sha256.clone(),
            source_run_id,
            source_batch_id,
            source_batch_digest,
            source_city_result_digests,
            currency: governed_currency,
            project_years: governed_project_years,
            discount_rate: governed_discount_rate,
            input_digest: input.input_digest.clone(),
            result_digest: output.result.portfolio_digest.clone(),
            batch: existing,
            files,
            publication,
            publication_error,
        });
    }
    if staging_dir.exists() {
        return Err("that portfolio run id is already pending".into());
    }
    let staging_dir = secure_private_directory(&staging_dir)?;
    let mut pending = PendingPortfolioDirectory::new(staging_dir.clone());
    portfolio_cancel_checkpoint(&cancel)?;
    for file in &verified.outputs {
        if file.entry.name == "batch.json" {
            continue;
        }
        let parts = checked_batch_artifact_name(&file.entry.name)?;
        let path = parts
            .iter()
            .fold(staging_dir.clone(), |path, part| path.join(part));
        if let Some(parent) = path.parent() {
            secure_private_directory(parent)?;
        }
        let bytes = file.read(&cancel).map_err(|error| error.to_string())?;
        write_atomically(&path, &bytes)?;
        portfolio_cancel_checkpoint(&cancel)?;
    }
    // Verify every declared file before batch.json becomes the commit marker.
    for artifact in &output.batch.artifacts {
        let parts = checked_batch_artifact_name(&artifact.name)?;
        let path = parts
            .iter()
            .fold(staging_dir.clone(), |path, part| path.join(part));
        let bytes =
            read_bounded_regular_file(&path, "portfolio artifact", MAX_BATCH_ARTIFACT_BYTES)?;
        if bytes.len() as u64 != artifact.byte_count
            || ds_solar_contracts::digest::sha256_hex(&bytes) != artifact.content_digest
        {
            return Err(format!(
                "portfolio artifact {} failed digest verification",
                artifact.name
            ));
        }
        portfolio_cancel_checkpoint(&cancel)?;
    }
    portfolio_cancel_checkpoint(&cancel)?;
    write_json(
        &staging_dir.join("sync-intent.json"),
        &output.batch.sync_intent(rfc3339_now()),
    )?;
    portfolio_cancel_checkpoint(&cancel)?;
    write_json(&staging_dir.join("engine-completion.json"), &completion)?;
    write_json(&staging_dir.join("batch.json"), &output.batch)?;
    portfolio_cancel_checkpoint(&cancel)?;
    pending.commit(&committed_dir)?;
    let files = portfolio_batch_inventory_at(&committed_dir)?.files;
    let (publication, publication_error) = seal_portfolio_completion(
        &app,
        &command.workspace_handle,
        &root,
        &run_id,
        &output.batch,
        &output.completion_build_manifest_sha256,
    );
    Ok(PortfolioRunReport {
        run_id,
        engine_version: format!("{}@{}", output.engine.name, output.engine.version),
        engine_build_manifest_sha256: output.completion_build_manifest_sha256.clone(),
        source_run_id,
        source_batch_id,
        source_batch_digest,
        source_city_result_digests,
        currency: governed_currency,
        project_years: governed_project_years,
        discount_rate: governed_discount_rate,
        input_digest: input.input_digest.clone(),
        result_digest: output.result.portfolio_digest.clone(),
        batch: output.batch.clone(),
        files,
        publication,
        publication_error,
    })
}

fn seal_portfolio_completion(
    app: &AppHandle,
    workspace: &str,
    root: &str,
    run: &str,
    batch: &ds_solar_contracts::SolarArtifactBatch,
    manifest: &str,
) -> (Option<serde_json::Value>, Option<String>) {
    if app.publication.is_none() {
        return (None, None);
    }
    match publication::completed(
        app.clone(),
        PortfolioBatchCommand {
            workspace_handle: workspace.into(),
            root: root.into(),
            run_id: run.into(),
            expected_batch_id: batch.batch_id.clone(),
            expected_batch_digest: batch.batch_digest.clone(),
        },
        manifest,
    ) {
        Ok(receipt) => (Some(receipt["receipt"].clone()), None),
        Err(error) => (None, Some(error)),
    }
}

pub fn solar_native_portfolio_batch_artifacts(
    app: AppHandle,
    command: PortfolioBatchCommand,
) -> Result<PortfolioArtifactInventory, String> {
    let dir = portfolio_run_directory(
        &app,
        &command.workspace_handle,
        &command.root,
        &command.run_id,
    )?;
    let batch = closed_batch_at(&dir)?;
    require_expected_portfolio_batch(
        &batch,
        &command.expected_batch_id,
        &command.expected_batch_digest,
    )?;
    portfolio_batch_inventory_from(&dir, &batch)
}

pub fn solar_native_read_portfolio_batch_artifact(
    app: AppHandle,
    command: PortfolioBatchArtifactCommand,
) -> Result<Vec<u8>, String> {
    let dir = portfolio_run_directory(
        &app,
        &command.workspace_handle,
        &command.root,
        &command.run_id,
    )?;
    let batch = closed_batch_at(&dir)?;
    require_expected_portfolio_batch(
        &batch,
        &command.expected_batch_id,
        &command.expected_batch_digest,
    )?;
    let artifact = verified_batch_artifact_at(&dir, &batch, &command.name, command.max_bytes)?;
    if artifact.descriptor.city_id.is_some() {
        return Err("that is not a portfolio-level artifact".into());
    }
    require_expected_portfolio_artifact(
        &artifact.descriptor,
        &command.expected_media_type,
        command.expected_byte_count,
        &command.expected_content_digest,
    )?;
    Ok(artifact.bytes)
}

/// Open one result/report declared by an explicit portfolio batch.
pub fn solar_native_open_portfolio_batch_artifact(
    app: AppHandle,
    command: PortfolioBatchOutputCommand,
) -> Result<(), String> {
    let dir = portfolio_run_directory(
        &app,
        &command.workspace_handle,
        &command.root,
        &command.run_id,
    )?;
    let batch = closed_batch_at(&dir)?;
    require_expected_portfolio_batch(
        &batch,
        &command.expected_batch_id,
        &command.expected_batch_digest,
    )?;
    let artifact = verified_portfolio_batch_output_from(&dir, &batch, &command.name)?;
    require_expected_portfolio_artifact(
        &artifact.descriptor,
        &command.expected_media_type,
        command.expected_byte_count,
        &command.expected_content_digest,
    )?;
    open_verified_bytes(
        &app,
        &artifact.bytes,
        &artifact.descriptor.name,
        &artifact.descriptor.content_digest,
        "portfolio batch output",
    )
}

/// Copy verified result/report bytes to an operator-selected destination.
pub fn solar_native_save_portfolio_batch_artifact(
    app: AppHandle,
    command: PortfolioBatchOutputCommand,
) -> Result<bool, String> {
    let dir = portfolio_run_directory(
        &app,
        &command.workspace_handle,
        &command.root,
        &command.run_id,
    )?;
    let batch = closed_batch_at(&dir)?;
    require_expected_portfolio_batch(
        &batch,
        &command.expected_batch_id,
        &command.expected_batch_digest,
    )?;
    let artifact = verified_portfolio_batch_output_from(&dir, &batch, &command.name)?;
    require_expected_portfolio_artifact(
        &artifact.descriptor,
        &command.expected_media_type,
        command.expected_byte_count,
        &command.expected_content_digest,
    )?;
    save_verified_bytes(
        app,
        artifact.bytes,
        &artifact.descriptor.name,
        "portfolio batch output",
    )
}

/// How many chart files a committed city directory holds.
fn count_charts(city_dir: &Path) -> Result<usize, String> {
    let mut total = 0;
    let mut entries_seen = 0;
    for family in ["site", "plant", "finance"] {
        let family_dir = city_dir.join("plots").join(family);
        match std::fs::symlink_metadata(&family_dir) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "that Solar chart family cannot be inspected: {error}"
                ));
            }
            Ok(_) => {}
        }
        let family_dir = secure_existing_private_directory(&family_dir)?;
        let entries = std::fs::read_dir(&family_dir)
            .map_err(|error| format!("that Solar chart family is unreadable: {error}"))?;
        for entry in entries {
            if entries_seen >= MAX_CHART_DIRECTORY_ENTRIES_PER_CITY {
                return Err("that Solar city has too many chart directory entries".to_string());
            }
            entries_seen += 1;
            let entry =
                entry.map_err(|error| format!("a committed Solar chart is unreadable: {error}"))?;
            if !entry
                .file_type()
                .map_err(|error| format!("a committed Solar chart is unreadable: {error}"))?
                .is_file()
            {
                continue;
            }
            if entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
            {
                total += 1;
            }
        }
    }
    Ok(total)
}

/// Read one bounded slice of a committed text document.
///
/// Bounded rather than whole: a complete APD is hundreds of kilobytes and an
/// IPC response is not where a document belongs. Callers that want the file
/// itself use [`solar_native_save_document`],
/// which never move the bytes through the web view at all.
pub fn solar_native_read_document(
    app: AppHandle,
    command: DocumentCommand,
) -> Result<DocumentSlice, String> {
    let name = checked_segment(&command.name, "document name")?;
    if !name.to_ascii_lowercase().ends_with(".md") {
        return Err("only Markdown documents can be read as text".to_string());
    }
    let artifact = document_artifact(&app, &command)?;
    read_text_bytes(name, command.offset, &artifact.bytes)
}

/// Read the fixed local demand-frame projection for one calculated city.
///
/// This is purpose-built for Inputs → Energy Load Profile.  Keeping it
/// separate from [`solar_native_read_document`] preserves that command's
/// Markdown-only contract and avoids exposing arbitrary JSON files from an
/// approved workspace to the web view.
pub fn solar_native_read_load_profile(
    app: AppHandle,
    command: LoadProfileCommand,
) -> Result<DocumentSlice, String> {
    let document = DocumentCommand {
        workspace_handle: command.workspace_handle,
        root: command.root,
        run_id: command.run_id,
        city_id: command.city_id,
        name: LOAD_PROFILE_NAME.to_string(),
        offset: command.offset,
    };
    let artifact = document_artifact(&app, &document)?;
    read_text_bytes(
        LOAD_PROFILE_NAME.to_string(),
        document.offset,
        &artifact.bytes,
    )
}

/// Read the exact bytes of one artifact declared by a closed native batch.
///
/// This is intentionally distinct from document preview selectors: publication
/// needs PNG and JSON bytes too (and can reopen historical DOCX). The web view cannot turn it into a file
/// reader because both the closed manifest and its per-file digest fence every
/// request.
pub fn solar_native_read_batch_artifact(
    app: AppHandle,
    command: BatchArtifactCommand,
) -> Result<Vec<u8>, String> {
    let run_dir = run_directory(
        &app,
        &command.workspace_handle,
        &command.root,
        &command.run_id,
    )?;
    let batch = closed_batch_at(&run_dir)?;
    require_batch_selector(&batch, &command.root, &command.run_id)?;
    Ok(verified_batch_artifact_at(&run_dir, &batch, &command.name, command.max_bytes)?.bytes)
}

/// Package one exact sealed report, resolving every image through closed artifacts
/// or the host's captured immutable-media producer. A reviewed final retains its
/// independent closure and uses the unchanged calculation batch for its media.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportBundleCommand {
    pub workspace_handle: String,
    pub root: String,
    pub run_id: String,
    pub city_id: String,
    pub expected_batch_id: String,
    pub expected_batch_digest: String,
    pub name: String,
    pub destination: PathBuf,
}

pub fn solar_native_report_bundle(
    app: AppHandle,
    command: ReportBundleCommand,
) -> Result<serde_json::Value, String> {
    let directory = run_directory(
        &app,
        &command.workspace_handle,
        &command.root,
        &command.run_id,
    )?;
    let city = checked_segment(&command.city_id, "city id")?;
    let batch = closed_batch_at(&directory)?;
    require_batch_selector(&batch, &command.root, &command.run_id)?;
    require_expected_portfolio_batch(
        &batch,
        &command.expected_batch_id,
        &command.expected_batch_digest,
    )?;
    if ![
        PRODUCT_APD_NAME,
        PARITY_DRAFT_NAME,
        APD_DRAFT_EN_NAME,
        NETWORK_DRAFT_NAME,
        NETWORK_DRAFT_EN_NAME,
        PLANT_DRAFT_NAME,
        PLANT_DRAFT_EN_NAME,
        FINANCIAL_DRAFT_NAME,
        FINANCIAL_DRAFT_EN_NAME,
        FINAL_APD_NAME,
        FINAL_APD_EN_NAME,
    ]
    .contains(&command.name.as_str())
        && city_finals::source_name(&command.name).is_none()
    {
        return Err("Select a declared city report draft or reviewed final".into());
    }
    let logical = format!("cities/{city}/{}", command.name);
    let source = verified_batch_artifact_at(&directory, &batch, &logical, MAX_DOCUMENT_BYTES)?;
    if source.descriptor.city_id.as_deref() != Some(&city)
        || source.descriptor.role != ds_solar_contracts::ArtifactRole::Report
        || source.descriptor.media_type != MARKDOWN_MEDIA_TYPE
    {
        return Err("The selected artifact is not a city-owned Markdown report".into());
    }
    let markdown =
        std::str::from_utf8(&source.bytes).map_err(|_| "Report Markdown must be UTF-8")?;
    let plan = ds_command_kernel::solar_report_bundle::plan(&command.name, markdown)?;
    let mut media = BTreeMap::new();
    let mut total = 0usize;
    for image in &plan.images {
        let bytes = if image.reference.starts_with("gs://") {
            let provider = app
                .media
                .as_ref()
                .ok_or("This host has no governed report-media producer")?;
            provider.fetch(&city, &image.reference)?
        } else {
            let name = if let Some(relative) = image
                .reference
                .strip_prefix(&format!("ds-solar://city/{city}/"))
            {
                format!("cities/{city}/{relative}")
            } else {
                format!("cities/{city}/{}", image.reference)
            };
            let artifact = verified_batch_artifact_at(
                &directory,
                &batch,
                &name,
                ds_command_kernel::solar_report_bundle::MAX_IMAGE_BYTES as u64,
            )?;
            if artifact.descriptor.city_id.as_deref() != Some(&city)
                || artifact.descriptor.role != ds_solar_contracts::ArtifactRole::Chart
                || !matches!(
                    artifact.descriptor.media_type.as_str(),
                    "image/png" | "image/jpeg" | "image/webp"
                )
            {
                return Err(
                    "A report image does not belong to its declared city chart inventory".into(),
                );
            }
            artifact.bytes
        };
        if bytes.is_empty()
            || bytes.len() > ds_command_kernel::solar_report_bundle::MAX_IMAGE_BYTES
            || !(bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                || bytes.starts_with(b"\xff\xd8\xff")
                || bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP")
        {
            return Err("A referenced report image is not a bounded PNG, JPEG or WebP".into());
        }
        total = total
            .checked_add(bytes.len())
            .ok_or("Report media size overflow")?;
        if total > ds_command_kernel::solar_report_bundle::MAX_MEDIA_BYTES {
            return Err("Report media exceeds 128 MiB".into());
        }
        media.insert(image.reference.clone(), bytes);
    }
    let bytes = ds_command_kernel::solar_report_bundle::build(&command.name, markdown, &media)?;
    // Acquisition may take time. Fence the exact source authority again before export.
    let current = closed_batch_at(&directory)?;
    require_expected_portfolio_batch(
        &current,
        &command.expected_batch_id,
        &command.expected_batch_digest,
    )?;
    let reopened = verified_batch_artifact_at(&directory, &current, &logical, MAX_DOCUMENT_BYTES)?;
    if reopened.descriptor != source.descriptor || reopened.bytes != source.bytes {
        return Err("The report source changed during packaging".into());
    }
    let digest = ds_solar_contracts::digest::sha256_hex(&bytes);
    let mut receipt = crate::application::export_bytes(&command.destination, &digest, &bytes)?;
    receipt["source_batch_id"] = batch.batch_id.into();
    receipt["source_batch_digest"] = batch.batch_digest.into();
    receipt["source_content_digest"] = source.descriptor.content_digest.into();
    receipt["image_count"] = plan.images.len().into();
    receipt["media_byte_count"] = total.into();
    receipt["publication"] = "not_requested".into();
    Ok(receipt)
}

/// Copy one committed document to a location the operator picks.
///
/// The bytes are copied by the shell, so nothing of the document crosses IPC
/// and the operator's chosen destination never reaches the web view either.
pub fn solar_native_save_document(
    app: AppHandle,
    command: DocumentCommand,
) -> Result<bool, String> {
    let artifact = document_artifact(&app, &command)?;
    save_verified_bytes(app, artifact.bytes, &command.name, "document")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct FinalLineage {
    schema_version: String,
    source_batch_id: String,
    source_batch_digest: String,
    source_draft: ds_solar_contracts::BatchArtifact,
    source_report_input: ds_solar_contracts::BatchArtifact,
}

fn write_new_final_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("could not create final artifact: {e}"))?;
    std::io::Write::write_all(&mut file, bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("could not persist final artifact: {e}"))
}

fn is_final_artifact(name: &str) -> bool {
    let parts: Vec<_> = name.split('/').collect();
    matches!(parts.as_slice(), ["cities", _, report] if city_finals::source_name(report).is_some())
}

fn final_lineage_at(
    source_dir: &Path,
    source: &ds_solar_contracts::SolarArtifactBatch,
    city: &str,
) -> Result<FinalLineage, String> {
    let drafts = [PARITY_DRAFT_NAME, APD_DRAFT_EN_NAME]
        .into_iter()
        .map(|name| format!("cities/{city}/{name}"))
        .filter(|name| source.artifacts.iter().any(|a| &a.name == name))
        .collect::<Vec<_>>();
    let [draft] = drafts.as_slice() else {
        return Err(
            "the source batch must declare exactly one language-specific APD authoring draft"
                .into(),
        );
    };
    let draft = verified_batch_artifact_at(source_dir, source, draft, MAX_DOCUMENT_BYTES)?;
    let report = verified_batch_artifact_at(
        source_dir,
        source,
        &format!("cities/{city}/{REPORT_INPUT_NAME}"),
        MAX_BATCH_ARTIFACT_BYTES,
    )?;
    if draft.descriptor.role != ds_solar_contracts::ArtifactRole::Report
        || draft.descriptor.media_type != MARKDOWN_MEDIA_TYPE
        || draft.descriptor.city_id.as_deref() != Some(city)
        || report.descriptor.role != ds_solar_contracts::ArtifactRole::ReportInput
        || report.descriptor.media_type != "application/json"
        || report.descriptor.city_id.as_deref() != Some(city)
    {
        return Err(
            "the final source draft or report input has an invalid sealed declaration".into(),
        );
    }
    Ok(FinalLineage {
        schema_version: "ds-solar.final-lineage/v1".into(),
        source_batch_id: source.batch_id.clone(),
        source_batch_digest: source.batch_digest.clone(),
        source_draft: draft.descriptor,
        source_report_input: report.descriptor,
    })
}

/// Resolve an independently closed final against the unchanged source batch.
fn final_context_at(
    source_dir: &Path,
    source: &ds_solar_contracts::SolarArtifactBatch,
    name: &str,
) -> Result<(PathBuf, ds_solar_contracts::SolarArtifactBatch), String> {
    let parts = checked_batch_artifact_name(name)?;
    if !is_final_artifact(name) {
        return Err("that is not an interpreted final".into());
    }
    let city = parts[1];
    if !matches!(parts[2], FINAL_APD_NAME | FINAL_APD_EN_NAME) {
        return city_finals::context(source_dir, source, city, parts[2]);
    }
    let directory = secure_existing_private_directory(&source_dir.join("finals").join(city))?;
    let batch = closed_batch_at(&directory)?;
    let source_city = source
        .run
        .cities
        .iter()
        .find(|c| c.city_id == city)
        .ok_or("final source city is not declared")?;
    let mut expected_run = source.run.clone();
    expected_run.cities = vec![source_city.clone()];
    if batch.run != expected_run || batch.artifacts.len() != 2 {
        return Err("the final closure does not match its source run and city".into());
    }
    let provenance = verified_batch_artifact_at(
        &directory,
        &batch,
        &format!("cities/{city}/source.json"),
        MAX_DOCUMENT_BYTES,
    )?;
    if provenance.descriptor.role != ds_solar_contracts::ArtifactRole::CityReportModel
        || provenance.descriptor.media_type != "application/json"
    {
        return Err("the final provenance declaration is invalid".into());
    }
    let lineage: FinalLineage =
        serde_json::from_slice(&provenance.bytes).map_err(|_| "the final provenance is invalid")?;
    if lineage != final_lineage_at(source_dir, source, city)? {
        return Err(
            "the final no longer matches its pinned source batch, draft and report input".into(),
        );
    }
    let final_file = declared_batch_artifact(&batch, name)?;
    if final_file.role != ds_solar_contracts::ArtifactRole::Report
        || final_file.media_type != MARKDOWN_MEDIA_TYPE
        || final_file.city_id.as_deref() != Some(city)
    {
        return Err("the final report declaration is invalid".into());
    }
    Ok((directory, batch))
}

pub fn solar_native_inspect_final(
    app: AppHandle,
    command: FinalSelector,
) -> Result<ImportedFinal, String> {
    let directory = run_directory(
        &app,
        &command.workspace_handle,
        &command.root,
        &command.run_id,
    )?;
    let city = checked_segment(&command.city_id, "city id")?;
    let source = closed_batch_at(&directory)?;
    require_batch_selector(&source, &command.root, &command.run_id)?;
    let lineage = final_lineage_at(&directory, &source, &city)?;
    let name = if lineage.source_draft.name.ends_with(APD_DRAFT_EN_NAME) {
        FINAL_APD_EN_NAME
    } else {
        FINAL_APD_NAME
    };
    let logical = format!("cities/{city}/{name}");
    let (_, batch) = final_context_at(&directory, &source, &logical)?;
    let artifact = verified_batch_artifact_at(&directory, &source, &logical, MAX_DOCUMENT_BYTES)?;
    Ok(ImportedFinal {
        markdown: file_descriptor(name, MARKDOWN_MEDIA_TYPE, &artifact.bytes),
        final_batch_id: batch.batch_id,
        final_batch_digest: batch.batch_digest,
        source_batch_id: source.batch_id,
        source_batch_digest: source.batch_digest,
        publication: None,
        publication_error: None,
        docx: None,
        docx_error: None,
    })
}

/// Import exact reviewed Markdown into an independent immutable final closure.
/// The optional Desktop picker is a platform adapter; Server callers use a path.
pub fn solar_native_import_final(
    app: AppHandle,
    command: ImportFinalCommand,
) -> Result<Option<ImportedFinal>, String> {
    let run_dir = run_directory(
        &app,
        &command.workspace_handle,
        &command.root,
        &command.run_id,
    )?;
    let city = checked_segment(&command.city_id, "city id")?;
    let source_batch = closed_batch_at(&run_dir)?;
    require_batch_selector(&source_batch, &command.root, &command.run_id)?;
    let lineage = final_lineage_at(&run_dir, &source_batch, &city)?;
    let source = if let Some(path) = command.source_path {
        PathBuf::from(path)
    } else {
        let Some(path) = app.platform.choose_final()? else {
            return Ok(None);
        };
        path
    };
    if source.extension().and_then(|value| value.to_str()) != Some("md") {
        return Err("the interpreted final must have a .md extension".into());
    }
    let bytes = read_bounded_regular_file(&source, "interpreted final", 2 * 1024 * 1024)?;
    if bytes.is_empty() {
        return Err("the interpreted final must not be empty".into());
    }
    let final_markdown =
        std::str::from_utf8(&bytes).map_err(|_| "the interpreted final must be UTF-8 Markdown")?;
    let draft = verified_batch_artifact_at(
        &run_dir,
        &source_batch,
        &lineage.source_draft.name,
        MAX_DOCUMENT_BYTES,
    )?;
    let lint = ds_solar_contracts::lint_final(
        std::str::from_utf8(&draft.bytes).map_err(|_| "the APD draft is not UTF-8 Markdown")?,
        final_markdown,
    );
    if !lint.ok() {
        return Err(format!(
            "the interpreted final violates its authoring draft contract:\n{}",
            lint.render()
        ));
    }
    let final_name = if lineage.source_draft.name.ends_with(APD_DRAFT_EN_NAME) {
        FINAL_APD_EN_NAME
    } else {
        FINAL_APD_NAME
    };
    let logical = format!("cities/{city}/{final_name}");
    let parent = secure_private_directory(&run_dir.join("finals"))?;
    let committed = parent.join(&city);
    if std::fs::symlink_metadata(&committed).is_ok()
        || source_batch
            .artifacts
            .iter()
            .any(|a| is_final_artifact(&a.name) && a.city_id.as_deref() == Some(&city))
    {
        return Err(
            "this source run already has an interpreted final; start a new run to replace it"
                .into(),
        );
    }
    // A fresh staging directory is private and removed on every failure. The
    // non-empty committed directory makes concurrent imports fail atomically.
    let staging = parent.join(format!(".final-{}", uuid::Uuid::new_v4()));
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let mut builder = builder;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&staging)
        .map_err(|e| format!("could not stage the final: {e}"))?;
    let mut pending = PendingPortfolioDirectory::new(staging.clone());
    let city_dir = secure_private_directory(&staging.join("cities").join(&city))?;
    let provenance = json_bytes(&lineage)?;
    write_new_final_file(&city_dir.join(final_name), &bytes)?;
    write_new_final_file(&city_dir.join("source.json"), &provenance)?;
    let mut identity = source_batch.run.clone();
    identity.cities = vec![
        source_batch
            .run
            .cities
            .iter()
            .find(|c| c.city_id == city)
            .ok_or("the source city is missing")?
            .clone(),
    ];
    let artifacts = [
        (
            logical.clone(),
            MARKDOWN_MEDIA_TYPE,
            &bytes,
            ds_solar_contracts::ArtifactRole::Report,
        ),
        (
            format!("cities/{city}/source.json"),
            "application/json",
            &provenance,
            ds_solar_contracts::ArtifactRole::CityReportModel,
        ),
    ]
    .into_iter()
    .map(|(name, media, bytes, role)| {
        let entry = ds_solar_contracts::ArtifactEntry::from_bytes(name, media, bytes);
        ds_solar_contracts::BatchArtifact::from_entry(&entry, role, Some(&city))
    })
    .collect();
    let final_batch = ds_solar_contracts::SolarArtifactBatch::close(identity, artifacts)
        .map_err(|e| e.message)?;
    write_new_final_file(&staging.join("batch.json"), &json_bytes(&final_batch)?)?;
    write_new_final_file(
        &staging.join("sync-intent.json"),
        &json_bytes(&final_batch.sync_intent(rfc3339_now()))?,
    )?;
    for artifact in &final_batch.artifacts {
        verified_batch_artifact_at(
            &staging,
            &final_batch,
            &artifact.name,
            MAX_BATCH_ARTIFACT_BYTES,
        )?;
    }
    // Fence the original manifest again immediately before exposing the final.
    if closed_batch_at(&run_dir)? != source_batch
        || final_lineage_at(&run_dir, &source_batch, &city)? != lineage
    {
        return Err("the source batch changed during final import".into());
    }
    #[cfg(unix)]
    for directory in [&city_dir, &staging.join("cities"), &staging] {
        std::fs::File::open(directory)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("could not persist final directory: {error}"))?;
    }
    pending.commit(&committed)?;
    #[cfg(unix)]
    std::fs::File::open(&parent)
        .and_then(|file| file.sync_all())
        .map_err(|error| {
            format!("the final committed but directory persistence failed: {error}")
        })?;
    let (publication, publication_error) = publication::city_final_completed(
        &app,
        &run_dir,
        &source_batch,
        &final_batch,
        &city,
        final_name,
    );
    Ok(Some(ImportedFinal {
        markdown: file_descriptor(final_name, MARKDOWN_MEDIA_TYPE, &bytes),
        final_batch_id: final_batch.batch_id,
        final_batch_digest: final_batch.batch_digest,
        source_batch_id: source_batch.batch_id,
        source_batch_digest: source_batch.batch_digest,
        publication,
        publication_error,
        docx: None,
        docx_error: None,
    }))
}

fn open_committed_file(app: &AppHandle, path: &Path, label: &str) -> Result<(), String> {
    app.platform.open(path, label)
}

/// Open a content-addressed snapshot of bytes already verified against their
/// batch receipt. Opening the mutable run path directly would reintroduce a
/// verification/use race after all selector checks had succeeded.
fn open_verified_bytes(
    app: &AppHandle,
    bytes: &[u8],
    fallback_name: &str,
    content_digest: &str,
    label: &str,
) -> Result<(), String> {
    let name = checked_segment(fallback_name, "open snapshot name")?;
    if ds_solar_contracts::digest::sha256_hex(bytes) != content_digest {
        return Err(format!(
            "that Solar {label} changed before its verified snapshot could be opened"
        ));
    }
    let digest = content_digest
        .strip_prefix("sha256:")
        .ok_or_else(|| format!("that Solar {label} has an invalid content digest"))?;
    let app_data = app.data_dir()?;
    let directory = secure_private_directory(&app_data.join("solar-open-snapshots"))?;
    let snapshot = directory.join(format!("{digest}-{name}"));
    write_atomically(&snapshot, bytes)?;
    let reopened = read_bounded_regular_file(&snapshot, "open snapshot", MAX_DOCUMENT_BYTES)?;
    if reopened != bytes {
        return Err(format!(
            "that Solar {label} snapshot changed before it could be opened"
        ));
    }
    open_committed_file(app, &snapshot, label)
}

/// Save bytes that were already verified against their closed batch. Unlike a
/// second path copy, this cannot race a replacement between verification and
/// the operator choosing a destination.
fn save_verified_bytes(
    app: AppHandle,
    bytes: Vec<u8>,
    fallback_name: &str,
    label: &str,
) -> Result<bool, String> {
    app.platform.save(&bytes, fallback_name, label)
}

/// The installed engine's honest identity: what it is, which build, and exactly
/// which pipeline stages this build runs. A stage absent from `stages` has not
/// been ported from the frozen Python implementation.
#[derive(Debug, Clone, Serialize)]
pub struct NativeEngineIdentity {
    pub name: String,
    pub version: String,
    pub stages: Vec<&'static str>,
}

pub fn solar_native_engine() -> Result<NativeEngineIdentity, String> {
    native_engine_identity(ds_solar_host::process::installed_engine_path()?)
}

pub fn solar_native_engine_for_host(app: &AppHandle) -> Result<NativeEngineIdentity, String> {
    app.capture_engine()?
        .with_verified_executable(|path| native_engine_identity(path.to_owned()))
        .map_err(|error| format!("{}: {error}", error.code()))?
}

fn native_engine_identity(path: PathBuf) -> Result<NativeEngineIdentity, String> {
    let identity =
        ds_solar_host::process::selected_engine_identity(&path).map_err(|error| error.to_string())?;
    Ok(NativeEngineIdentity {
        name: identity.name,
        version: identity.version,
        stages: ds_solar_contracts::PLANNED_STAGES
            .iter()
            .map(|stage| stage.as_str())
            .collect(),
    })
}

/// Ask an active run to stop between stages.
pub fn solar_native_cancel(state: &SolarNativeState, run_id: String) -> Result<bool, String> {
    let runs = state
        .runs
        .lock()
        .map_err(|_| "Solar run state is poisoned".to_string())?;
    match runs.get(&run_id) {
        Some(token) => {
            token.cancel();
            Ok(true)
        }
        None => Ok(false),
    }
}

// ---------------------------------------------------------------------------
// Preparation
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
struct GovernedReferenceProvider(HttpReferenceProvider);
impl ReferenceBundleProvider for GovernedReferenceProvider {
    fn fetch(
        &self,
        request: &ds_solar_reference_host::provider::ReferenceRequest,
    ) -> ds_solar_contracts::SolarResult<ds_solar_contracts::BundleBytes> {
        self.0.fetch(request)
    }
}

#[allow(clippy::too_many_arguments)]
fn prepare_all(
    cities: &[CitySnapshotInput],
    project_id: &str,
    root: &str,
    language: &str,
    prepared_dir: &Path,
    cache_dir: &Path,
    native_provider: Option<&dyn ReferenceBundleProvider>,
    snapshots: Option<&dyn crate::SnapshotProvider>,
    cancel: Option<&CancelToken>,
    weather_url: Option<&str>,
    weather_token: Option<&str>,
    weather_api_key: Option<&str>,
    overwrite: bool,
    concurrency: Concurrency,
    on_settled: impl Fn(usize, &CityReadiness) + Send + Sync,
) -> Vec<CityReadiness> {
    let cache = DirectoryReferenceCache::new(cache_dir);
    // A provider that cannot be constructed must not silently degrade the
    // whole preparation to cache-only: the failure is kept, and any city
    // that then misses the cache names it instead of claiming the weather
    // simply is not there.
    let mut provider_failure: Option<String> = None;
    let provider: Option<GovernedReferenceProvider> = match weather_url {
        None => None,
        Some(url) => match HttpReferenceProvider::new(url, weather_token.map(str::to_owned)) {
            Ok(provider) => Some(GovernedReferenceProvider(
                provider.with_api_key(weather_api_key.map(str::to_owned)),
            )),
            Err(error) => {
                let message = format!("reference provider unavailable: {error}");
                tracing_warn(&message);
                provider_failure = Some(message);
                None
            }
        },
    };

    let mut preparer = ReferencePreparer::new()
        .with_cache(&cache)
        .with_pin(ReferencePin::current())
        .with_refresh(if overwrite {
            Refresh::Overwrite
        } else {
            Refresh::UseCache
        });
    if let Some(provider) = native_provider {
        preparer = preparer.with_provider(provider);
    } else if let Some(provider) = provider.as_ref() {
        let provider: &dyn ReferenceBundleProvider = provider;
        preparer = preparer.with_provider(provider);
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(concurrency.get())
        .thread_name(|index| format!("ds-solar-prepare-{index}"))
        .build();
    // Reported from inside the map, so a city appears on the page the moment it
    // settles rather than when the slowest one does. The callback receives the
    // city's *position*, so completion order never reaches the display order.
    let prepare_one = |(index, city): (usize, &CitySnapshotInput)| {
        let mut readiness = if cancel.is_some_and(CancelToken::is_cancelled) {
            not_ready(
                &city.city_id,
                "run_cancelled",
                "Preparation was cancelled",
                &[],
            )
        } else {
            let captured = snapshots.map(|provider| {
                provider
                    .capture(&city.city_id)
                    .and_then(|captured| captured_city(city, root, captured))
            });
            match captured {
                Some(Ok((captured, publication))) => prepare_city_captured(
                    &captured,
                    PreparationScope { project_id, root },
                    language,
                    prepared_dir,
                    &preparer,
                    cancel,
                    publication.as_ref(),
                ),
                Some(Err(error)) => not_ready(&city.city_id, "solar_capture_failed", &error, &[]),
                None => prepare_city(
                    city,
                    project_id,
                    root,
                    language,
                    prepared_dir,
                    &preparer,
                    cancel,
                ),
            }
        };
        if let Some(failure) = provider_failure.as_deref()
            && let CityState::NotReady { message, .. } = &mut readiness.state
            && message.contains("no provider is available")
        {
            message.push_str("; ");
            message.push_str(failure);
        }
        on_settled(index, &readiness);
        (index, readiness)
    };

    let mut settled: Vec<(usize, CityReadiness)> = match pool {
        Ok(pool) => pool.install(|| cities.par_iter().enumerate().map(prepare_one).collect()),
        // A pool that will not build is not a reason to fail every city.
        Err(_) => cities.iter().enumerate().map(prepare_one).collect(),
    };
    settled.sort_by_key(|(index, _)| *index);
    settled
        .into_iter()
        .map(|(_, readiness)| readiness)
        .collect()
}

pub(crate) fn captured_city(
    city: &CitySnapshotInput,
    root: &str,
    captured: crate::CapturedCitySnapshot,
) -> Result<(CitySnapshotInput, Option<crate::CapturedPublication>), String> {
    if captured.snapshot_json.len() > MAX_SNAPSHOT_BYTES
        || captured.input_base_fingerprint.len() != 64
        || !captured
            .input_base_fingerprint
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(
            "The governed Solar capture exceeds its bound or has no exact fingerprint".into(),
        );
    }
    chrono::DateTime::parse_from_rfc3339(&captured.captured_at)
        .map_err(|_| "The governed Solar capture has an invalid read time")?;
    let doc: serde_json::Value = serde_json::from_str(&captured.snapshot_json)
        .map_err(|_| "Invalid governed Solar snapshot")?;
    let p = &doc["_provenance"];
    if p["root"] != root
        || p["context"] != city.city_id
        || p["input_base_fingerprint"] != captured.input_base_fingerprint
    {
        return Err(
            "The governed Solar capture belongs to another city, project or fingerprint".into(),
        );
    }
    Ok((
        CitySnapshotInput {
            city_id: city.city_id.clone(),
            display_name: doc["_root"]["display_name"]
                .as_str()
                .unwrap_or(&city.city_id)
                .into(),
            snapshot_json: captured.snapshot_json,
            source: "ds-brain".into(),
            captured_at: captured.captured_at,
            record_updated_at_ms: None,
            revision: Some(captured.input_base_fingerprint),
        },
        captured.publication,
    ))
}

pub(crate) fn normalize_city_input(
    city: &CitySnapshotInput,
    project_id: &str,
    root: &str,
    language: &str,
) -> ds_solar_contracts::SolarResult<SolarCityInput> {
    checked_segment(&city.city_id, "city id")
        .map_err(|e| SolarError::new(ds_solar_contracts::SolarErrorCode::InvalidInput, e))?;
    if city.snapshot_json.len() > MAX_SNAPSHOT_BYTES {
        return Err(SolarError::new(
            ds_solar_contracts::SolarErrorCode::InvalidInput,
            "Solar snapshot exceeds its bound",
        ));
    }
    let snapshot: serde_json::Value = serde_json::from_str(&city.snapshot_json).map_err(|e| {
        SolarError::new(
            ds_solar_contracts::SolarErrorCode::InvalidInput,
            format!("the captured city snapshot is not valid JSON: {e}"),
        )
        .with_fields(["snapshot_json".into()])
    })?;
    let input = SolarCityInput::from_city_snapshot(
        CityIdentity {
            project_id: project_id.to_string(),
            root: root.to_string(),
            city_id: city.city_id.clone(),
            display_name: if city.display_name.trim().is_empty() {
                city.city_id.clone()
            } else {
                city.display_name.clone()
            },
        },
        SnapshotRevision {
            source: city.source.clone(),
            captured_at: city.captured_at.clone(),
            record_updated_at_ms: city.record_updated_at_ms,
            revision: city.revision.clone(),
        },
        RunOptions {
            system_types: vec![
                SystemType::Hybrid,
                SystemType::SolarBattery,
                SystemType::ThermalOnly,
            ],
            language: language.to_string(),
            demand_noise_seed: None,
        },
        &snapshot,
    )?;
    ds_solar_contracts::network_form::prepare_input(input, None)
}

struct PreparationScope<'a> {
    project_id: &'a str,
    root: &'a str,
}

fn prepare_city(
    city: &CitySnapshotInput,
    project_id: &str,
    root: &str,
    language: &str,
    prepared_dir: &Path,
    preparer: &ReferencePreparer<'_>,
    cancel: Option<&CancelToken>,
) -> CityReadiness {
    prepare_city_captured(
        city,
        PreparationScope { project_id, root },
        language,
        prepared_dir,
        preparer,
        cancel,
        None,
    )
}

fn prepare_city_captured(
    city: &CitySnapshotInput,
    scope: PreparationScope<'_>,
    language: &str,
    prepared_dir: &Path,
    preparer: &ReferencePreparer<'_>,
    cancel: Option<&CancelToken>,
    publication: Option<&crate::CapturedPublication>,
) -> CityReadiness {
    if cancel.is_some_and(CancelToken::is_cancelled) {
        return not_ready(
            &city.city_id,
            "run_cancelled",
            "Preparation was cancelled before acquisition",
            &[],
        );
    }
    let city_id = match checked_segment(&city.city_id, "city id") {
        Ok(value) => value,
        Err(message) => {
            return not_ready(&city.city_id, "invalid_input", &message, &["city_id"]);
        }
    };
    if city.snapshot_json.len() > MAX_SNAPSHOT_BYTES {
        return not_ready(
            &city_id,
            "invalid_input",
            "the captured city snapshot is larger than this shell will normalise",
            &["snapshot_json"],
        );
    }

    let input = match normalize_city_input(city, scope.project_id, scope.root, language) {
        Ok(input) => input,
        Err(error) => return from_error(&city_id, &error),
    };
    let prepared = match preparer.prepare(input, rfc3339_now()) {
        Ok(prepared) => prepared,
        Err(error) => return from_error(&city_id, &error),
    };

    if cancel.is_some_and(CancelToken::is_cancelled) {
        return not_ready(
            &city_id,
            "run_cancelled",
            "Preparation was cancelled before input commit",
            &[],
        );
    }
    if let Some(publication) = publication {
        let sealed = crate::sync::capture_claim(&prepared.input, publication).and_then(|claim| {
            write_json(
                &publication_claim_path(prepared_dir, &prepared.input),
                &claim,
            )
        });
        if let Err(message) = sealed {
            return not_ready(&city_id, "solar_publication_capture_invalid", &message, &[]);
        }
    }
    if let Err(message) = write_prepared(prepared_dir, &city_id, &prepared.input) {
        return not_ready(&city_id, "artifact_failed", &message, &[]);
    }

    ready_city(
        city_id,
        &prepared.input,
        match prepared.resolution {
            BundleResolution::Cache => "cache",
            BundleResolution::Provider => "provider",
        },
    )
}

fn readiness_of(directory: &Path, city_id: &str) -> CityReadiness {
    match read_prepared(directory, city_id) {
        None => not_ready(
            city_id,
            "city_input_not_prepared",
            "this city has not been prepared on the input page",
            &[],
        ),
        Some(input) => match ds_solar_contracts::require_prepared(&input) {
            Ok(()) => ready_city(city_id.to_string(), &input, "cache"),
            Err(error) => from_error(city_id, &error),
        },
    }
}

/// Produce the sole ready-state projection used by both a newly committed city
/// and a later readiness re-read. The prepared input remains the authority;
/// this serialises only its bounded cache receipt.
fn ready_city(
    city_id: String,
    input: &PreparedSolarCityInput,
    weather_source: &str,
) -> CityReadiness {
    CityReadiness {
        city_id,
        state: CityState::Ready {
            input_digest: input.input_digest.clone(),
            weather_entry: input.weather_key.cache_entry(),
            weather_source: weather_source.to_string(),
            reference_cache_receipt: input.reference_cache_receipt(),
        },
    }
}

// ---------------------------------------------------------------------------
// Workspace layout
// ---------------------------------------------------------------------------

fn native_root(workspace: &Path) -> PathBuf {
    workspace.join("solar-rust").join(NATIVE_LAYOUT_VERSION)
}

fn prepared_dir(workspace: &Path, root: &str) -> Result<PathBuf, String> {
    secure_private_directory(&native_root(workspace).join("prepared").join(root))
}
/// Import the owner-validated prepared artifact produced by headless intake.
/// A project check precedes any workspace effect. Existing prepared inputs
/// require their exact digest; city runs have already captured their copies.
pub fn import_prepared(
    app: &AppHandle,
    workspace_handle: &str,
    project: &str,
    file: &Path,
    expected: Option<&str>,
) -> Result<serde_json::Value, String> {
    let bytes = read_bounded_regular_file(file, "prepared Solar input", 32 * 1024 * 1024)?;
    let input: PreparedSolarCityInput =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid prepared Solar input")?;
    input.validate().map_err(|e| e.message)?;
    let root = format!("eds_project/{project}/eds_solar");
    if input.city.identity.project_id != project || input.city.identity.root != root {
        return Err("Prepared Solar input belongs to another project".into());
    }
    let city = checked_segment(&input.city.identity.city_id, "city id")?;
    let workspace = resolve_workspace_handle(app, workspace_handle)?;
    let state = app.state();
    let _inputs = state
        .inputs
        .lock()
        .map_err(|_| "Solar input state is poisoned")?;
    let directory = prepared_dir(&workspace, &root)?;
    if let Some(previous) = read_prepared(&directory, &city) {
        if previous.input_digest == input.input_digest {
            return Ok(
                serde_json::json!({"status":"unchanged","city_id":city,"input_digest":input.input_digest}),
            );
        }
        if expected != Some(previous.input_digest.as_str()) {
            return Err(
                "Prepared Solar input changed; pin the previous input digest before replacing it"
                    .into(),
            );
        }
    } else if prepared_path(&directory, &city).exists() {
        return Err(
            "The held prepared Solar input is corrupt; it cannot be silently replaced".into(),
        );
    } else if expected.is_some() {
        return Err("The expected prepared Solar input is unavailable".into());
    }
    write_json(&prepared_path(&directory, &city), &input)?;
    Ok(serde_json::json!({"status":"imported","city_id":city,"input_digest":input.input_digest}))
}

pub(crate) fn weather_cache_dir(workspace: &Path) -> Result<PathBuf, String> {
    secure_private_directory(&native_root(workspace).join("weather-cache"))
}

fn run_output_dir(workspace: &Path, root: &str, run_id: &str) -> Result<PathBuf, String> {
    secure_private_directory(&native_root(workspace).join("runs").join(root).join(run_id))
}

fn prepared_path(directory: &Path, city_id: &str) -> PathBuf {
    directory.join(format!("{city_id}.prepared.json"))
}

fn publication_claim_path(directory: &Path, input: &PreparedSolarCityInput) -> PathBuf {
    directory.join(format!(
        "{}.{}.publication.json",
        input.city.identity.city_id,
        input
            .input_digest
            .strip_prefix("sha256:")
            .unwrap_or(&input.input_digest)
    ))
}

fn read_prepared(directory: &Path, city_id: &str) -> Option<PreparedSolarCityInput> {
    let raw = read_bounded_regular_file(
        &prepared_path(directory, city_id),
        "prepared input",
        32 << 20,
    )
    .ok()?;
    serde_json::from_slice(&raw).ok()
}

/// Commit one prepared input. Written through a sibling temporary file so a
/// crash mid-write cannot leave a truncated document that later reads as ready.
fn write_prepared(
    directory: &Path,
    city_id: &str,
    input: &PreparedSolarCityInput,
) -> Result<(), String> {
    let path = prepared_path(directory, city_id);
    let mut body = serde_json::to_vec(input)
        .map_err(|error| format!("prepared input could not be encoded: {error}"))?;
    body.push(b'\n');
    let temporary = path.with_extension("json.partial");
    std::fs::write(&temporary, &body)
        .map_err(|error| format!("prepared input could not be written: {error}"))?;
    std::fs::rename(&temporary, &path)
        .map_err(|error| format!("prepared input could not be committed: {error}"))
}

/// What one city's commit produced.
struct CommittedCity {
    city_id: String,
    documents: CityDocuments,
    commit_ms: u64,
}

/// Commit one run's artifacts into the workspace.
///
/// Order is the contract: every file this run produced lands on disk first, the
/// Calculation commits deterministic drafts only. Interpretation, final
/// document rendering, and publication have separate explicit boundaries.
#[cfg(test)]
fn commit_run(
    output_dir: &Path,
    output: &ds_solar_runtime::RunOutput,
    on_phase: impl Fn(&str, CityPhase),
) -> Result<Vec<CommittedCity>, String> {
    let mut committed: Vec<CommittedCity> = Vec::new();
    for city in &output.cities {
        let Ok(outcome) = &city.outcome else { continue };
        let commit_started = std::time::Instant::now();
        on_phase(&city.city_id, CityPhase::Committing);
        let city_dir = secure_private_directory(&output_dir.join("cities").join(&city.city_id))?;
        write_atomically(
            &city_dir.join(REPORT_INPUT_NAME),
            &outcome.report_input.bytes,
        )?;
        let report_input = file_descriptor(
            REPORT_INPUT_NAME,
            &outcome.report_input.entry.media_type,
            &outcome.report_input.bytes,
        );
        if report_input.byte_count != outcome.report_input.entry.byte_count
            || report_input.content_digest != outcome.report_input.entry.content_digest
        {
            return Err("the report-input metadata does not match its committed bytes".to_string());
        }
        write_json(&city_dir.join("result.json"), &outcome.result)?;
        let projection = city
            .portfolio_projection
            .as_ref()
            .ok_or_else(|| format!("{} has no portfolio projection declaration", city.city_id))?
            .as_ref()
            .map_err(|error| {
                format!(
                    "{} is not eligible for a closed portfolio: {} ({})",
                    city.city_id,
                    error.message,
                    error.code.as_str()
                )
            })?;
        write_json(
            &city_dir.join(PORTFOLIO_CONTRIBUTION_NAME),
            &projection.contribution,
        )?;
        write_json(
            &city_dir.join(PORTFOLIO_CALCULATION_NAME),
            &projection.calculation,
        )?;
        for name in [PORTFOLIO_CONTRIBUTION_NAME, PORTFOLIO_CALCULATION_NAME] {
            let logical_name = format!("cities/{}/{name}", city.city_id);
            let declared = output
                .batch
                .artifacts
                .iter()
                .find(|artifact| artifact.name == logical_name)
                .ok_or_else(|| format!("the Solar batch did not declare {logical_name}"))?;
            let bytes = read_bounded_regular_file(
                &city_dir.join(name),
                "committed portfolio projection",
                MAX_BATCH_ARTIFACT_BYTES,
            )?;
            if declared.byte_count != bytes.len() as u64
                || declared.content_digest != ds_solar_contracts::digest::sha256_hex(&bytes)
            {
                return Err(format!(
                    "the committed {logical_name} does not match its batch declaration"
                ));
            }
        }
        write_atomically(
            &city_dir.join(LOAD_PROFILE_NAME),
            &json_bytes(&daily_load_profile_frame(
                &outcome.daily_load_profile.load_w,
                &outcome.daily_load_profile.load_w_actual,
            )?)?,
        )?;
        if let Some(model) = &outcome.report_model {
            write_json(&city_dir.join("report-model.json"), model)?;
        }
        let mut chart_names: Vec<String> = Vec::new();
        if let Some(artifacts) = &outcome.artifacts {
            for chart in &artifacts.charts {
                let path = city_dir.join(&chart.name);
                if let Some(parent) = path.parent() {
                    secure_private_directory(parent)?;
                }
                write_atomically(&path, &chart.bytes)?;
                chart_names.push(chart.name.clone());
            }
        }
        chart_names.sort();
        let chart_count = chart_names.len();
        // Hashed from the file as committed, not from the value in memory: the
        // publication lane uploads these exact bytes and the server fences on
        // their digest.
        let result_file_digest = {
            let bytes = read_bounded_regular_file(
                &city_dir.join("result.json"),
                "committed result",
                MAX_BATCH_ARTIFACT_BYTES,
            )?;
            ds_solar_contracts::digest::sha256_hex(&bytes)
                .strip_prefix("sha256:")
                .unwrap_or_default()
                .to_string()
        };
        // Documents last among the local writes, beside the figures they
        // reference relatively — so the Markdown is never on disk pointing at
        // an image that is not.
        on_phase(&city.city_id, CityPhase::ComposingReport);
        let mut documents = CityDocuments {
            report_input,
            apd: None,
            draft: None,
            drafts: Vec::new(),
            docx: None,
            docx_error: None,
            chart_count,
            section_count: 0,
            figure_count: 0,
            result_file_digest,
            chart_names,
        };
        for document in &outcome.documents {
            let name = document.name().to_string();
            if !valid_solar_segment(&name) {
                return Err(format!("{name} is not a usable document name"));
            }
            write_atomically(&city_dir.join(&name), &document.bytes)?;
            let file = DocumentFile {
                name: name.clone(),
                media_type: document.entry.media_type.clone(),
                byte_count: document.entry.byte_count,
                content_digest: document.entry.content_digest.clone(),
            };
            if name == PRODUCT_APD_NAME {
                let markdown = String::from_utf8_lossy(&document.bytes);
                documents.section_count = markdown
                    .lines()
                    .filter(|line| line.starts_with("## "))
                    .count();
                documents.figure_count = markdown.matches("![").count();
                documents.apd = Some(file);
            } else if [PARITY_DRAFT_NAME, APD_DRAFT_EN_NAME].contains(&name.as_str()) {
                let markdown = String::from_utf8_lossy(&document.bytes);
                documents.section_count = markdown
                    .lines()
                    .filter(|line| line.starts_with("## "))
                    .count();
                documents.figure_count = markdown.matches("![").count();
                documents.draft = Some(file.clone());
                documents.drafts.push(file);
            } else if [
                NETWORK_DRAFT_NAME,
                NETWORK_DRAFT_EN_NAME,
                PLANT_DRAFT_NAME,
                PLANT_DRAFT_EN_NAME,
                FINANCIAL_DRAFT_NAME,
                FINANCIAL_DRAFT_EN_NAME,
            ]
            .contains(&name.as_str())
            {
                documents.drafts.push(file);
            }
        }
        let commit_ms = commit_started.elapsed().as_millis() as u64;

        on_phase(&city.city_id, CityPhase::LocalReady);
        committed.push(CommittedCity {
            city_id: city.city_id.clone(),
            documents,
            commit_ms,
        });
    }
    Ok(committed)
}

/// The narrow local projection the read-only Energy Load Profile presents.
///
/// The legacy UI names this `02_site_prep/base_daily_load_profile_df`; retain
/// that frame shape at the adapter boundary so the UI consumes measured Rust
/// output rather than reverse-engineering it from a summary or recomputing
/// it from editable inputs. It intentionally is not a batch artifact: the
/// sealed `result.json` is the published engineering result, while this is a
/// small local view of values already attested by that calculation.
#[cfg(test)]
fn daily_load_profile_frame(
    load_w: &[f64],
    load_w_actual: &[f64],
) -> Result<serde_json::Value, String> {
    const HOURS: usize = 24;
    if load_w.len() != HOURS || load_w_actual.len() != HOURS {
        return Err(format!(
            "the native daily load profile must carry {HOURS} design and actual hourly values"
        ));
    }
    Ok(serde_json::json!({
        "schema_version": "ds-solar.daily-load-profile/v1",
        "kind": "frame",
        "rows": HOURS,
        "columns": {
            "hour": { "values": (0..HOURS).collect::<Vec<_>>() },
            "load_w": { "values": load_w },
            "load_w_actual": { "values": load_w_actual },
        },
    }))
}

/// Write one artifact through a sibling temporary file.
///
/// A crash mid-write must not leave a truncated file that a later read — or a
/// resumed upload — treats as the committed artifact.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension("partial");
    std::fs::write(&temporary, bytes)
        .map_err(|error| format!("could not write {path:?}: {error}"))?;
    std::fs::rename(&temporary, path).map_err(|error| format!("could not commit {path:?}: {error}"))
}

fn json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let mut body =
        serde_json::to_vec_pretty(value).map_err(|error| format!("could not encode: {error}"))?;
    body.push(b'\n');
    Ok(body)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let body = json_bytes(value)?;
    write_atomically(path, &body)
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn checked_segment(value: &str, field: &str) -> Result<String, String> {
    if valid_solar_segment(value) {
        Ok(value.to_string())
    } else {
        Err(format!("Solar {field} is not a usable path segment"))
    }
}

fn checked_root(value: &str) -> Result<String, String> {
    if valid_solar_root(value) {
        Ok(value.to_string())
    } else {
        Err("Solar root must be eds_solar or eds_project/<safe-id>/eds_solar".to_string())
    }
}

fn checked_text(value: &str, field: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 200 {
        return Err(format!("Solar {field} is required"));
    }
    Ok(trimmed.to_string())
}

fn checked_cities(cities: &[CitySnapshotInput]) -> Result<Vec<CitySnapshotInput>, String> {
    if cities.is_empty() {
        return Err("select at least one city to prepare".to_string());
    }
    if cities.len() > MAX_CITIES_PER_CALL {
        return Err("too many cities in one Solar preparation request".to_string());
    }
    let mut seen = std::collections::BTreeSet::new();
    for city in cities {
        if !seen.insert(city.city_id.as_str()) {
            return Err(format!("{} appears twice in the selection", city.city_id));
        }
    }
    Ok(cities.to_vec())
}

fn not_ready(city_id: &str, code: &str, message: &str, fields: &[&str]) -> CityReadiness {
    CityReadiness {
        city_id: city_id.to_string(),
        state: CityState::NotReady {
            code: code.to_string(),
            message: message.to_string(),
            fields: fields.iter().map(|field| (*field).to_string()).collect(),
        },
    }
}

fn from_error(city_id: &str, error: &SolarError) -> CityReadiness {
    CityReadiness {
        city_id: city_id.to_string(),
        state: CityState::NotReady {
            code: error.code.as_str().to_string(),
            message: error.message.clone(),
            fields: error.fields.clone(),
        },
    }
}

fn rfc3339_now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn tracing_warn(message: &str) {
    eprintln!("[solar-native] {message}");
}

/// A run's cancellation token leaves the table when the run ends, whether it
/// succeeded, failed or unwound.
struct RunGuard<'a> {
    state: &'a SolarNativeState,
    run_id: String,
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut runs) = self.state.runs.lock() {
            runs.remove(&self.run_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeHost;

    fn snapshot_input(city_id: &str, snapshot_json: &str) -> CitySnapshotInput {
        CitySnapshotInput {
            city_id: city_id.to_string(),
            display_name: String::new(),
            snapshot_json: snapshot_json.to_string(),
            source: "indexeddb".to_string(),
            captured_at: "2026-08-20T00:00:00Z".to_string(),
            record_updated_at_ms: None,
            revision: None,
        }
    }

    fn preparer() -> ReferencePreparer<'static> {
        // No cache and no provider: preparation can only fail, which is what
        // these tests are about — the failures happen *before* the bundle.
        ReferencePreparer::new()
    }

    struct GovernedFixtureSnapshots;
    impl crate::SnapshotProvider for GovernedFixtureSnapshots {
        fn capture(&self, city: &str) -> Result<crate::CapturedCitySnapshot, String> {
            let mut doc: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../../../ds-solar/fixtures/city/aderm_bere.snapshot.json"),
                )
                .unwrap(),
            )
            .unwrap();
            doc["_provenance"]["root"] = "eds_project/arjgpydw_aderm_loc7/eds_solar".into();
            doc["_provenance"]["context"] = city.into();
            doc["_provenance"]["input_base_fingerprint"] = "b".repeat(64).into();
            Ok(crate::CapturedCitySnapshot {
                snapshot_json: doc.to_string(),
                input_base_fingerprint: "b".repeat(64),
                captured_at: "2026-09-17T00:00:00Z".into(),
                publication: Some(crate::CapturedPublication {
                    source_snapshot_sha256: ds_compute_runtime::digest(doc.to_string().as_bytes()),
                    snapshot_receipt_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
                    // Expiry cannot prevent local computation and sealing.
                    snapshot_receipt_expires_at: "2026-09-18T00:00:00Z".into(),
                }),
            })
        }
    }
    struct FixtureReferenceProvider(AtomicU64);
    impl ReferenceBundleProvider for FixtureReferenceProvider {
        fn fetch(
            &self,
            _: &ds_solar_reference_host::provider::ReferenceRequest,
        ) -> ds_solar_contracts::SolarResult<ds_solar_contracts::BundleBytes> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../../ds-solar/fixtures/reference/aderm_bere");
            let read = |name: &str| std::fs::read(path.join(name)).unwrap();
            Ok(ds_solar_contracts::BundleBytes {
                weather_json: read("weather.json"),
                reference_unit_json: read("reference-unit.json"),
                reference_unit_parquet: read("reference-unit.parquet"),
                manifest_json: read("manifest.json"),
            })
        }
    }
    fn wait_preparation(host: &NativeHost, job: &str) -> serde_json::Value {
        for _ in 0..1500 {
            let p = preparation_progress(host, "arjgpydw_aderm_loc7", job).unwrap();
            if p["done"] == true {
                return p;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("Native preparation did not settle");
    }
    pub(super) struct LocalPublicationStore(pub(super) ds_sync_runtime::SharedStore);
    impl LocalPublicationStore {
        pub(super) fn fence() -> ds_command_kernel::sync_store::Fence {
            ds_command_kernel::sync_store::Fence {
                account: "native-offline-test".into(),
                deployment: "https://gateway.example".into(),
                install_id: "native-offline-test".into(),
            }
        }
    }
    impl crate::PublicationProvider for LocalPublicationStore {
        fn retain(
            &self,
            publication: crate::sync::Publication,
        ) -> Result<serde_json::Value, String> {
            let fence = Self::fence();
            let project = publication.project.clone();
            let producer = ds_sync_runtime::retained_publication::RetainedProducer {
                store: &self.0,
                fence: &fence,
                project: &project,
            };
            crate::sync::retain(&self.0, &fence, publication, &|previous| {
                ds_sync_runtime::Producer::reclaim(
                    &producer,
                    &project,
                    &ds_sync_runtime::rows::local_row_from_store(previous),
                )
            })
        }
        fn observe(
            &self,
            publication: &crate::sync::Publication,
        ) -> Result<serde_json::Value, String> {
            crate::sync::recorded(&self.0, &Self::fence(), publication)
        }
    }
    fn wait_city_run(host: &NativeHost, run: &str) -> RunReport {
        for _ in 0..3000 {
            let progress = solar_native_calculate_progress(&host.state(), run.into()).unwrap();
            if progress.done {
                assert!(progress.error.is_none(), "{:?}", progress.error);
                return progress.report.unwrap();
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("Native offline calculation did not settle");
    }
    #[test]
    fn server_captures_and_prepares_cold_inputs_and_recovers_durable_progress() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("authority");
        let references = std::sync::Arc::new(FixtureReferenceProvider(AtomicU64::new(0)));
        let host = NativeHost::headless(data.clone())
            .unwrap()
            .with_snapshot_provider(std::sync::Arc::new(GovernedFixtureSnapshots))
            .with_reference_provider(references.clone());
        let workspace =
            crate::workspace::solar_native_prepare_default_workspace(host.clone()).unwrap();
        let request = serde_json::json!({"operation":"prepare_captured","command":{"workspace_handle":workspace.workspace_handle,"project_id":"arjgpydw_aderm_loc7","root":"eds_project/arjgpydw_aderm_loc7/eds_solar","city_ids":["aderm_bere"],"concurrency":1}});
        let launched = crate::application::execute(
            host.clone(),
            "arjgpydw_aderm_loc7",
            &serde_json::to_vec(&request).unwrap(),
        )
        .unwrap();
        let id = launched["value"]["job_id"].as_str().unwrap();
        let cold = wait_preparation(&host, id);
        assert_eq!(cold["cities"][0]["state"], "ready", "{cold}");
        assert_eq!(cold["cities"][0]["weather_source"], "provider");
        assert_eq!(references.0.load(Ordering::SeqCst), 1);
        let restarted = NativeHost::headless(data).unwrap();
        let durable = preparation_progress(&restarted, "arjgpydw_aderm_loc7", id).unwrap();
        assert_eq!(durable, cold);
        assert!(preparation_progress(&restarted, "foreign", id).is_err());
        assert!(!preparation_cancel(&restarted, "arjgpydw_aderm_loc7", id).unwrap());
        let second = crate::application::execute(
            host.clone(),
            "arjgpydw_aderm_loc7",
            &serde_json::to_vec(&request).unwrap(),
        )
        .unwrap();
        let warm = wait_preparation(&host, second["value"]["job_id"].as_str().unwrap());
        assert_eq!(warm["cities"][0]["state"], "ready");
        assert_eq!(warm["cities"][0]["weather_source"], "cache");
        assert_eq!(references.0.load(Ordering::SeqCst), 1);
        // Reopened with no capture/reference providers: the prepared weather,
        // PV inputs and publication claim are enough, even after receipt expiry.
        let queue = std::sync::Arc::new(LocalPublicationStore(
            ds_sync_runtime::open_store(&tmp.path().join("offline.sqlite")).unwrap(),
        ));
        let offline = restarted.clone().with_publication_provider(queue.clone());
        let command = CalculateCommand {
            workspace_handle: workspace.workspace_handle.clone(),
            project_id: "arjgpydw_aderm_loc7".into(),
            root: "eds_project/arjgpydw_aderm_loc7/eds_solar".into(),
            city_ids: vec!["aderm_bere".into()],
            run_id: "offline-city".into(),
            render_charts: false,
            generate_documents: Some(true),
            report_drafts: vec![
                "apd".into(),
                "network".into(),
                "plant".into(),
                "financial".into(),
            ],
            concurrency: Some(1),
        };
        solar_native_calculate(offline.clone(), &offline.state(), command.clone()).unwrap();
        let report = wait_city_run(&offline, "offline-city");
        assert_eq!(report.publications.len(), 5, "{:?}", report.publications);
        assert!(
            report
                .publications
                .iter()
                .all(|p| p.error.is_none() && p.publication.as_ref().unwrap()["state"] == "held"),
            "{:?}",
            report.publications
        );
        let fence = LocalPublicationStore::fence();
        let rows = queue
            .0
            .lock()
            .unwrap()
            .snapshot(
                &fence,
                &ds_sync_runtime::rows::store_scope("arjgpydw_aderm_loc7"),
            )
            .unwrap()
            .artifacts;
        assert_eq!(rows.len(), 5);
        let calculation = rows
            .iter()
            .find(|r| r.identity.operation == "calculate-aderm_bere")
            .unwrap();
        let producer = ds_sync_runtime::retained_publication::RetainedProducer {
            store: &queue.0,
            fence: &fence,
            project: "arjgpydw_aderm_loc7",
        };
        let local = ds_sync_runtime::rows::local_row_from_store(calculation);
        assert_eq!(
            ds_compute_runtime::digest(&producer.output(&local, "report-input").unwrap()),
            local.outputs[0].sha256
        );
        let mut declaration =
            ds_sync_runtime::rows::open_declaration("arjgpydw_aderm_loc7", &local);
        ds_sync_runtime::Producer::amend_open_declaration(
            &producer,
            "arjgpydw_aderm_loc7",
            &mut declaration,
        )
        .unwrap();
        assert_eq!(
            declaration["snapshot_receipt_id"],
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
        );
        let directory = prepared_dir(
            &resolve_workspace_handle(&offline, &workspace.workspace_handle).unwrap(),
            &command.root,
        )
        .unwrap();
        let input = read_prepared(&directory, "aderm_bere").unwrap();
        let path = publication_claim_path(&directory, &input);
        let mut swapped: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        swapped["prepared_input_digest"] = serde_json::json!(format!("sha256:{}", "f".repeat(64)));
        write_json(&path, &swapped).unwrap();
        solar_native_calculate(
            offline.clone(),
            &offline.state(),
            CalculateCommand {
                run_id: "offline-invalid-claim".into(),
                generate_documents: Some(false),
                ..command
            },
        )
        .unwrap();
        let unpublishable = wait_city_run(&offline, "offline-invalid-claim");
        assert!(unpublishable.cities[0].result_digest.is_some());
        assert_eq!(unpublishable.publications.len(), 1);
        assert!(
            unpublishable.publications[0]
                .error
                .as_deref()
                .unwrap()
                .contains("claim_invalid_or_stale")
        );
        assert_eq!(
            queue
                .0
                .lock()
                .unwrap()
                .snapshot(
                    &fence,
                    &ds_sync_runtime::rows::store_scope("arjgpydw_aderm_loc7")
                )
                .unwrap()
                .artifacts
                .len(),
            5
        );
        let interrupted = PrepareJob {
            job_id: uuid::Uuid::new_v4().to_string(),
            seq: 1,
            done: false,
            settled: 0,
            total: 1,
            cities: vec![CityReadiness {
                city_id: "aderm_bere".into(),
                state: CityState::Preparing,
            }],
        };
        let path = secure_existing_private_directory(
            &restarted.data_dir().unwrap().join("solar-preparation-jobs"),
        )
        .unwrap()
        .join(format!("{}.json", interrupted.job_id));
        persist_preparation(
            &path,
            "arjgpydw_aderm_loc7",
            "eds_project/arjgpydw_aderm_loc7/eds_solar",
            &interrupted,
        )
        .unwrap();
        let recovered =
            preparation_progress(&restarted, "arjgpydw_aderm_loc7", &interrupted.job_id).unwrap();
        assert_eq!(recovered["done"], true);
        assert_eq!(recovered["cities"][0]["code"], "preparation_interrupted");
    }
    #[test]
    fn server_preparation_cancellation_stops_reference_fetch_and_input_commit() {
        struct BlockingSnapshots {
            entered: std::sync::Arc<std::sync::Barrier>,
            release: std::sync::Arc<std::sync::Barrier>,
        }
        impl crate::SnapshotProvider for BlockingSnapshots {
            fn capture(&self, city: &str) -> Result<crate::CapturedCitySnapshot, String> {
                self.entered.wait();
                self.release.wait();
                crate::SnapshotProvider::capture(&GovernedFixtureSnapshots, city)
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let entered = std::sync::Arc::new(std::sync::Barrier::new(2));
        let release = std::sync::Arc::new(std::sync::Barrier::new(2));
        let references = std::sync::Arc::new(FixtureReferenceProvider(AtomicU64::new(0)));
        let host = NativeHost::headless(tmp.path().join("authority"))
            .unwrap()
            .with_reference_provider(references.clone())
            .with_snapshot_provider(std::sync::Arc::new(BlockingSnapshots {
                entered: entered.clone(),
                release: release.clone(),
            }));
        let workspace =
            crate::workspace::solar_native_prepare_default_workspace(host.clone()).unwrap();
        let command = PrepareCapturedCommand {
            workspace_handle: workspace.workspace_handle.clone(),
            project_id: "arjgpydw_aderm_loc7".into(),
            root: "eds_project/arjgpydw_aderm_loc7/eds_solar".into(),
            city_ids: vec!["aderm_bere".into()],
            concurrency: Some(1),
            overwrite: false,
            language: None,
        };
        let job = solar_native_prepare_captured(host.clone(), &host.state(), command).unwrap();
        entered.wait();
        assert!(preparation_cancel(&host, "arjgpydw_aderm_loc7", &job.job_id).unwrap());
        release.wait();
        let done = wait_preparation(&host, &job.job_id);
        assert_eq!(done["cities"][0]["code"], "run_cancelled");
        assert_eq!(references.0.load(Ordering::SeqCst), 0);
        let path = resolve_workspace_handle(&host, &workspace.workspace_handle).unwrap();
        assert!(
            read_prepared(
                &prepared_dir(&path, "eds_project/arjgpydw_aderm_loc7/eds_solar").unwrap(),
                "aderm_bere"
            )
            .is_none()
        );
    }

    #[test]
    fn cold_preparation_calls_native_provider_without_caller_credentials() {
        struct Provider(AtomicU64);
        impl ReferenceBundleProvider for Provider {
            fn fetch(
                &self,
                _: &ds_solar_reference_host::provider::ReferenceRequest,
            ) -> ds_solar_contracts::SolarResult<ds_solar_contracts::BundleBytes> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(ds_solar_contracts::SolarError::new(
                    ds_solar_contracts::SolarErrorCode::ReferenceUnitInvalid,
                    "native authority reached",
                ))
            }
        }
        let path = std::env::temp_dir().join(format!("solar-cold-{}", uuid::Uuid::new_v4()));
        let snapshot = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../../ds-solar/fixtures/city/aderm_bere.snapshot.json"),
        )
        .unwrap();
        let provider = Provider(AtomicU64::new(0));
        let readiness = prepare_all(
            &[snapshot_input("aderm_bere", &snapshot)],
            "p",
            "eds_project/p/eds_solar",
            "fr",
            &path.join("prepared"),
            &path.join("cache"),
            Some(&provider),
            None,
            None,
            None,
            None,
            None,
            false,
            Concurrency::new(1),
            |_, _| (),
        );
        assert_eq!(provider.0.load(Ordering::SeqCst), 1);
        match &readiness[0].state {
            CityState::NotReady { message, .. } => {
                assert!(message.contains("native authority reached"))
            }
            _ => panic!("a refused acquisition must not become ready"),
        }
        assert!(!path.join("prepared").exists());
        if path.exists() {
            std::fs::remove_dir_all(path).unwrap();
        }
    }

    #[test]
    fn a_city_id_that_could_escape_the_workspace_is_refused() {
        for hostile in ["../escape", "a/b", "", ".", "..", "CON", " leading"] {
            assert!(
                checked_segment(hostile, "city id").is_err(),
                "{hostile:?} should not be a usable path segment"
            );
        }
        assert!(checked_segment("aderm_bere", "city id").is_ok());
    }

    #[test]
    fn portfolio_report_intents_preserve_order_and_reject_duplicates() {
        assert_eq!(
            checked_portfolio_report_intents(vec!["financial".to_string(), "apd".to_string(),]),
            Ok(vec![
                PortfolioReportIntent::Financial,
                PortfolioReportIntent::Apd,
            ]),
        );
        let duplicate =
            checked_portfolio_report_intents(vec!["network".to_string(), "network".to_string()])
                .expect_err("a duplicate report intent must be refused");
        assert!(duplicate.contains("selected more than once"));
    }

    #[test]
    fn portfolio_graph_receipts_reopen_exact_source_bytes() {
        let source_dir = scratch_dir("portfolio-graph-receipt");
        let city_id = "alpha";
        let name = "cities/alpha/plots/site/combined_irradiance_and_temperature.png";
        let bytes = b"\x89PNG\r\nsealed-chart";
        let path = source_dir.join(name);
        std::fs::create_dir_all(path.parent().expect("chart parent")).expect("chart parent");
        std::fs::write(&path, bytes).expect("chart bytes");
        let batch = ds_solar_contracts::SolarArtifactBatch::close(
            ds_solar_contracts::BatchRunIdentity {
                project_id: "project-test".to_string(),
                root: "eds_project/project-test/eds_solar".to_string(),
                run_id: "source-run".to_string(),
                cities: vec![ds_solar_contracts::BatchCity {
                    city_id: city_id.to_string(),
                    input_digest: ds_solar_contracts::digest::sha256_hex(b"input"),
                    result_digest: ds_solar_contracts::digest::sha256_hex(b"result"),
                }],
            },
            vec![ds_solar_contracts::BatchArtifact {
                name: name.to_string(),
                role: ds_solar_contracts::ArtifactRole::Chart,
                media_type: "image/png".to_string(),
                byte_count: bytes.len() as u64,
                content_digest: ds_solar_contracts::digest::sha256_hex(bytes),
                city_id: Some(city_id.to_string()),
            }],
        )
        .expect("source batch closes");
        let receipt = serde_json::json!({
            "schema": "ds-solar.graph-artifact-receipt/v1",
            "source_batch_id": batch.batch_id,
            "source_batch_digest": batch.batch_digest,
            "name": name,
            "role": "chart",
            "city_id": city_id,
            "media_type": "image/png",
            "byte_count": bytes.len(),
            "content_digest": ds_solar_contracts::digest::sha256_hex(bytes),
        });
        let row = serde_json::json!({
            "section": "site",
            "graph_key": "combined_irradiance_and_temperature_image",
            "city_id": city_id,
            "city": "Alpha",
            "plot_key": "combined_irradiance_and_temperature_image",
            "url": receipt,
        });
        let graphs = serde_json::json!({
            "_strategy": "explicit",
            "_representative_cities": [city_id],
            "graph_count": 1,
            "graphs": [row.clone()],
            "site": { "combined_irradiance_and_temperature_image": row },
            "_unavailable": [],
        });

        verify_portfolio_graph_artifacts(&graphs, &source_dir, &batch, 1)
            .expect("sealed chart resolves");
        std::fs::write(&path, b"tampered").expect("tamper chart");
        assert!(verify_portfolio_graph_artifacts(&graphs, &source_dir, &batch, 1).is_err());
        std::fs::remove_dir_all(source_dir).expect("remove scratch directory");
    }

    /// One sealed single-graph section, reusable by the strategy cases below.
    ///
    /// The graph bytes are real and the receipt is exact, so every refusal a
    /// case observes is attributable to the metadata or provenance it changed —
    /// not to a chart that never resolved in the first place.
    fn sealed_graph_fixture(
        label: &str,
    ) -> (
        PathBuf,
        ds_solar_contracts::SolarArtifactBatch,
        serde_json::Value,
    ) {
        let source_dir = scratch_dir(label);
        let city_id = "alpha";
        let name = "cities/alpha/plots/site/combined_irradiance_and_temperature.png";
        let bytes = b"\x89PNG\r\nsealed-chart";
        let path = source_dir.join(name);
        std::fs::create_dir_all(path.parent().expect("chart parent")).expect("chart parent");
        std::fs::write(&path, bytes).expect("chart bytes");
        let batch = ds_solar_contracts::SolarArtifactBatch::close(
            ds_solar_contracts::BatchRunIdentity {
                project_id: "project-test".to_string(),
                root: "eds_project/project-test/eds_solar".to_string(),
                run_id: "source-run".to_string(),
                cities: vec![ds_solar_contracts::BatchCity {
                    city_id: city_id.to_string(),
                    input_digest: ds_solar_contracts::digest::sha256_hex(b"input"),
                    result_digest: ds_solar_contracts::digest::sha256_hex(b"result"),
                }],
            },
            vec![ds_solar_contracts::BatchArtifact {
                name: name.to_string(),
                role: ds_solar_contracts::ArtifactRole::Chart,
                media_type: "image/png".to_string(),
                byte_count: bytes.len() as u64,
                content_digest: ds_solar_contracts::digest::sha256_hex(bytes),
                city_id: Some(city_id.to_string()),
            }],
        )
        .expect("source batch closes");
        let row = serde_json::json!({
            "section": "site",
            "graph_key": "combined_irradiance_and_temperature_image",
            "city_id": city_id,
            "city": "Alpha",
            "plot_key": "combined_irradiance_and_temperature_image",
            "url": {
                "schema": "ds-solar.graph-artifact-receipt/v1",
                "source_batch_id": batch.batch_id,
                "source_batch_digest": batch.batch_digest,
                "name": name,
                "role": "chart",
                "city_id": city_id,
                "media_type": "image/png",
                "byte_count": bytes.len(),
                "content_digest": ds_solar_contracts::digest::sha256_hex(bytes),
            },
        });
        (source_dir, batch, row)
    }

    fn round_robin_graphs(
        row: &serde_json::Value,
        start_index: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "_strategy": "round_robin",
            "_start_index": start_index,
            "_representative_cities": ["alpha"],
            "graph_count": 1,
            "graphs": [row.clone()],
            "site": { "combined_irradiance_and_temperature_image": row.clone() },
            "_unavailable": [],
        })
    }

    /// The exact defect 025907e0 reported: `_start_index` is round-robin
    /// metadata, and reading it as a graph section refused every correct
    /// rotated aggregate the engine produces.
    #[test]
    fn round_robin_start_index_is_bounded_metadata_not_a_graph_section() {
        let (source_dir, batch, row) = sealed_graph_fixture("portfolio-graph-round-robin");

        verify_portfolio_graph_artifacts(
            &round_robin_graphs(&row, serde_json::json!(1)),
            &source_dir,
            &batch,
            2,
        )
        .expect("a rotated aggregate with bounded _start_index verifies");

        for wrong_type in [
            serde_json::json!("1"),
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!(null),
            serde_json::json!({ "start": 1 }),
        ] {
            let error = verify_portfolio_graph_artifacts(
                &round_robin_graphs(&row, wrong_type.clone()),
                &source_dir,
                &batch,
                2,
            )
            .expect_err("a non-unsigned start index is refused");
            assert!(
                error.contains("_start_index is not an unsigned integer"),
                "{wrong_type} refused as {error}"
            );
        }

        let unbounded = verify_portfolio_graph_artifacts(
            &round_robin_graphs(&row, serde_json::json!(2)),
            &source_dir,
            &batch,
            2,
        )
        .expect_err("a start index outside the member count is refused");
        assert!(
            unbounded.contains("outside 2 result members"),
            "{unbounded}"
        );

        let mut missing = round_robin_graphs(&row, serde_json::json!(0));
        missing
            .as_object_mut()
            .expect("graph section")
            .remove("_start_index");
        let absent = verify_portfolio_graph_artifacts(&missing, &source_dir, &batch, 2)
            .expect_err("round-robin without a start index is refused");
        assert!(
            absent.contains("require an unsigned _start_index"),
            "{absent}"
        );

        std::fs::remove_dir_all(source_dir).expect("remove scratch directory");
    }

    #[test]
    fn start_index_belongs_to_round_robin_alone() {
        let (source_dir, batch, row) = sealed_graph_fixture("portfolio-graph-strategy");

        for strategy in ["first", "explicit", "digest_seeded_random"] {
            let mut graphs = round_robin_graphs(&row, serde_json::json!(0));
            graphs
                .as_object_mut()
                .expect("graph section")
                .insert("_strategy".into(), serde_json::json!(strategy));
            let error = verify_portfolio_graph_artifacts(&graphs, &source_dir, &batch, 2)
                .expect_err("only round-robin may declare a start index");
            assert!(error.contains("cannot declare _start_index"), "{error}");

            graphs
                .as_object_mut()
                .expect("graph section")
                .remove("_start_index");
            verify_portfolio_graph_artifacts(&graphs, &source_dir, &batch, 2).unwrap_or_else(
                |error| panic!("{strategy} without _start_index verifies: {error}"),
            );
        }

        let mut unsupported = round_robin_graphs(&row, serde_json::json!(0));
        unsupported
            .as_object_mut()
            .expect("graph section")
            .insert("_strategy".into(), serde_json::json!("newest"));
        let error = verify_portfolio_graph_artifacts(&unsupported, &source_dir, &batch, 2)
            .expect_err("an unknown strategy is refused");
        assert!(error.contains("is unsupported"), "{error}");

        let mut undeclared = round_robin_graphs(&row, serde_json::json!(0));
        undeclared
            .as_object_mut()
            .expect("graph section")
            .remove("_strategy");
        let error = verify_portfolio_graph_artifacts(&undeclared, &source_dir, &batch, 2)
            .expect_err("a graph section must declare its strategy");
        assert!(error.contains("do not declare their strategy"), "{error}");

        std::fs::remove_dir_all(source_dir).expect("remove scratch directory");
    }

    /// Metadata validation must not become a way past the provenance half: a
    /// rotated aggregate still has to reopen its exact source-batch bytes.
    #[test]
    fn round_robin_graphs_still_prove_their_source_member_provenance() {
        let (source_dir, batch, row) = sealed_graph_fixture("portfolio-graph-provenance");

        let mut foreign_batch = round_robin_graphs(&row, serde_json::json!(0));
        let foreign_digest = ds_solar_contracts::digest::sha256_hex(b"other");
        let foreign_id = foreign_digest
            .strip_prefix("sha256:")
            .unwrap_or(&foreign_digest)[..32]
            .to_string();
        foreign_batch["graphs"][0]["url"]["source_batch_id"] = serde_json::json!(foreign_id);
        foreign_batch["site"]["combined_irradiance_and_temperature_image"] =
            foreign_batch["graphs"][0].clone();
        let error = verify_portfolio_graph_artifacts(&foreign_batch, &source_dir, &batch, 2)
            .expect_err("a receipt from another source batch is refused");
        assert!(error.contains("provenance is inconsistent"), "{error}");

        let mut foreign_city = round_robin_graphs(&row, serde_json::json!(0));
        foreign_city["graphs"][0]["city_id"] = serde_json::json!("beta");
        foreign_city["site"]["combined_irradiance_and_temperature_image"] =
            foreign_city["graphs"][0].clone();
        let error = verify_portfolio_graph_artifacts(&foreign_city, &source_dir, &batch, 2)
            .expect_err("a row naming a member its receipt does not is refused");
        assert!(error.contains("provenance is inconsistent"), "{error}");

        let mut unindexed = round_robin_graphs(&row, serde_json::json!(0));
        unindexed["site"]["monthly_average_daily_energy_image"] = row.clone();
        let error = verify_portfolio_graph_artifacts(&unindexed, &source_dir, &batch, 2)
            .expect_err("a section row absent from the index is refused");
        assert!(error.contains("unindexed row"), "{error}");

        std::fs::remove_dir_all(source_dir).expect("remove scratch directory");
    }

    #[test]
    fn batch_artifact_names_cannot_escape_a_closed_run() {
        assert_eq!(
            checked_batch_artifact_name("cities/aderm_bere/result.json"),
            Ok(vec!["cities", "aderm_bere", "result.json"]),
        );
        for hostile in [
            "../result.json",
            "cities/../result.json",
            "/etc/passwd",
            "cities/aderm_bere/",
            "",
            "cities//result.json",
        ] {
            assert!(
                checked_batch_artifact_name(hostile).is_err(),
                "{hostile:?} must not escape the batch selector"
            );
        }
    }

    #[test]
    fn bounded_file_read_refuses_an_oversized_sparse_file() {
        let directory = scratch_dir("bounded-read-oversized");
        let path = directory.join("oversized.json");
        let file = std::fs::File::create(&path).expect("sparse file");
        file.set_len(9).expect("oversized sparse file");

        let error = read_bounded_regular_file(&path, "test artifact", 8)
            .expect_err("an oversized file must be refused");
        assert!(error.contains("exceeds its read bound"), "{error}");
        std::fs::remove_dir_all(directory).expect("remove scratch directory");
    }

    fn close_test_batch(
        run_dir: &Path,
        artifacts: Vec<ds_solar_contracts::BatchArtifact>,
    ) -> ds_solar_contracts::SolarArtifactBatch {
        let batch = ds_solar_contracts::SolarArtifactBatch::close(
            ds_solar_contracts::BatchRunIdentity {
                project_id: "project-test".to_string(),
                root: "eds_project/project-test/eds_solar".to_string(),
                run_id: "run-test".to_string(),
                cities: Vec::new(),
            },
            artifacts,
        )
        .expect("test batch closes");
        write_json(&run_dir.join("batch.json"), &batch).expect("test batch commits");
        batch
    }

    #[cfg(unix)]
    #[test]
    fn closed_batch_refuses_a_symlinked_run_ancestor() {
        let root = scratch_dir("closed-batch-linked-root");
        let outside = scratch_dir("closed-batch-linked-outside");
        let run_dir = outside.join("run");
        std::fs::create_dir(&run_dir).expect("outside run");
        let result = b"{}\n";
        std::fs::write(run_dir.join(PORTFOLIO_RESULT_NAME), result).expect("result writes");
        close_test_batch(
            &run_dir,
            vec![test_artifact(
                PORTFOLIO_RESULT_NAME,
                ds_solar_contracts::ArtifactRole::PortfolioResult,
                PORTFOLIO_RESULT_MEDIA_TYPE,
                result,
            )],
        );
        let planted = root.join("runs");
        std::os::unix::fs::symlink(&outside, &planted).expect("planted run ancestor");

        let error = closed_batch_at(&planted.join("run"))
            .expect_err("a batch below a linked ancestor must be refused");
        assert!(
            error.contains("linked or non-directory ancestor"),
            "{error}"
        );
        std::fs::remove_dir_all(root).expect("remove linked root");
        std::fs::remove_dir_all(outside).expect("remove outside root");
    }

    fn test_artifact(
        name: &str,
        role: ds_solar_contracts::ArtifactRole,
        media_type: &str,
        bytes: &[u8],
    ) -> ds_solar_contracts::BatchArtifact {
        ds_solar_contracts::BatchArtifact {
            name: name.to_string(),
            role,
            media_type: media_type.to_string(),
            byte_count: bytes.len() as u64,
            content_digest: ds_solar_contracts::digest::sha256_hex(bytes),
            city_id: None,
        }
    }

    #[test]
    fn explicit_portfolio_selectors_pin_batch_and_artifact_receipts() {
        let run_dir = scratch_dir("portfolio-selector-provenance");
        let bytes = b"{\"sealed\":true}\n";
        std::fs::write(run_dir.join(PORTFOLIO_RESULT_NAME), bytes).expect("result writes");
        let batch = close_test_batch(
            &run_dir,
            vec![test_artifact(
                PORTFOLIO_RESULT_NAME,
                ds_solar_contracts::ArtifactRole::PortfolioResult,
                PORTFOLIO_RESULT_MEDIA_TYPE,
                bytes,
            )],
        );
        let artifact = &batch.artifacts[0];

        require_expected_portfolio_batch(&batch, &batch.batch_id, &batch.batch_digest)
            .expect("exact batch receipt matches");
        require_expected_portfolio_artifact(
            artifact,
            &artifact.media_type,
            artifact.byte_count,
            &artifact.content_digest,
        )
        .expect("exact artifact receipt matches");
        assert!(
            require_expected_portfolio_batch(&batch, &"0".repeat(32), &batch.batch_digest,)
                .is_err()
        );
        assert!(
            require_expected_portfolio_batch(
                &batch,
                &batch.batch_id,
                &format!("sha256:{}", "0".repeat(64)),
            )
            .is_err()
        );
        assert!(
            require_expected_portfolio_artifact(
                artifact,
                &artifact.media_type,
                artifact.byte_count + 1,
                &artifact.content_digest,
            )
            .is_err()
        );
        assert!(
            require_expected_portfolio_artifact(
                artifact,
                &artifact.media_type,
                artifact.byte_count,
                &format!("sha256:{}", "0".repeat(64)),
            )
            .is_err()
        );
        std::fs::remove_dir_all(run_dir).expect("remove scratch directory");
    }

    #[test]
    fn cancelled_portfolio_staging_is_removed_before_commit() {
        let parent = scratch_dir("portfolio-cancel-cleanup");
        let staging = parent.join("run.partial");
        std::fs::create_dir(&staging).expect("staging creates");
        std::fs::write(staging.join("portfolio-result.json"), b"partial")
            .expect("partial artifact writes");
        {
            let _pending = PendingPortfolioDirectory::new(staging.clone());
            let cancel = CancelToken::new();
            cancel.cancel();
            assert!(portfolio_cancel_checkpoint(&cancel).is_err());
        }
        assert!(!staging.exists(), "cancelled staging must be removed");

        let second = parent.join("committed.partial");
        let committed = parent.join("committed");
        std::fs::create_dir(&second).expect("second staging creates");
        let mut pending = PendingPortfolioDirectory::new(second);
        pending.commit(&committed).expect("staging commits");
        drop(pending);
        assert!(
            committed.is_dir(),
            "committed batches must survive the guard"
        );
        std::fs::remove_dir_all(parent).expect("remove scratch directory");
    }

    #[test]
    fn explicit_portfolio_output_selector_refuses_inputs_and_wrong_declarations() {
        let run_dir = scratch_dir("portfolio-output-selector");
        let input = b"{\"schema_version\":\"ds-solar.portfolio-run-input/v1\"}\n";
        let result = b"{\"schema_version\":\"ds-solar.portfolio-result/v2\"}\n";
        let report = b"# Sealed portfolio network report\n";
        let false_report = b"# Wrongly declared report\n";
        for (name, bytes) in [
            ("portfolio-run-input.json", input.as_slice()),
            (PORTFOLIO_RESULT_NAME, result.as_slice()),
            ("portfolio-network-en.md", report.as_slice()),
            ("portfolio-plant-en.md", false_report.as_slice()),
        ] {
            std::fs::write(run_dir.join(name), bytes).expect("portfolio output writes");
        }
        close_test_batch(
            &run_dir,
            vec![
                test_artifact(
                    "portfolio-run-input.json",
                    ds_solar_contracts::ArtifactRole::PortfolioRunInput,
                    PORTFOLIO_RESULT_MEDIA_TYPE,
                    input,
                ),
                test_artifact(
                    PORTFOLIO_RESULT_NAME,
                    ds_solar_contracts::ArtifactRole::PortfolioResult,
                    PORTFOLIO_RESULT_MEDIA_TYPE,
                    result,
                ),
                test_artifact(
                    "portfolio-network-en.md",
                    ds_solar_contracts::ArtifactRole::Report,
                    MARKDOWN_MEDIA_TYPE,
                    report,
                ),
                test_artifact(
                    "portfolio-plant-en.md",
                    ds_solar_contracts::ArtifactRole::PortfolioResult,
                    MARKDOWN_MEDIA_TYPE,
                    false_report,
                ),
            ],
        );

        assert_eq!(
            verified_portfolio_batch_output_at(&run_dir, PORTFOLIO_RESULT_NAME)
                .expect("sealed result is selectable")
                .bytes,
            result,
        );
        assert_eq!(
            verified_portfolio_batch_output_at(&run_dir, "portfolio-network-en.md")
                .expect("sealed report is selectable")
                .bytes,
            report,
        );
        for refused in [
            "portfolio-run-input.json",
            "portfolio-plant-en.md",
            "batch.json",
            "../portfolio-result.json",
        ] {
            assert!(
                verified_portfolio_batch_output_at(&run_dir, refused).is_err(),
                "{refused} must not be exposed as an operator-facing output"
            );
        }
        let batch = closed_batch_at(&run_dir).unwrap();
        let inventory = portfolio_batch_inventory_from(&run_dir, &batch).unwrap();
        assert_eq!(inventory.files.len(), 4);
        assert_eq!(
            inventory
                .outputs
                .iter()
                .map(|file| file.name.as_str())
                .collect::<Vec<_>>(),
            ["portfolio-network-en.md", PORTFOLIO_RESULT_NAME]
        );
        std::fs::remove_dir_all(&run_dir).ok();
    }

    #[test]
    fn a_project_solar_root_is_accepted_without_relaxing_path_confinement() {
        assert_eq!(
            checked_root("eds_project/arjgpydw_aderm_loc7/eds_solar").as_deref(),
            Ok("eds_project/arjgpydw_aderm_loc7/eds_solar"),
        );
        assert_eq!(checked_root("eds_solar").as_deref(), Ok("eds_solar"));
        for hostile in [
            "eds_project/../eds_solar",
            "eds_project/project/eds_solar/extra",
            "/eds_project/project/eds_solar",
            "eds_project/project/other",
            "eds_project/project.with-dot/eds_solar",
        ] {
            assert!(
                checked_root(hostile).is_err(),
                "{hostile:?} should be refused"
            );
        }
    }

    #[test]
    fn a_selection_must_be_non_empty_and_distinct() {
        assert!(checked_cities(&[]).is_err());
        let duplicated = vec![snapshot_input("a", "{}"), snapshot_input("a", "{}")];
        assert!(checked_cities(&duplicated).is_err());
        assert!(checked_cities(&[snapshot_input("a", "{}")]).is_ok());
    }

    #[test]
    fn an_oversized_snapshot_is_refused_before_it_is_parsed() {
        let huge = "x".repeat(MAX_SNAPSHOT_BYTES + 1);
        let readiness = prepare_city(
            &snapshot_input("aderm_bere", &huge),
            "project",
            "solar",
            "fr",
            Path::new("/nonexistent"),
            &preparer(),
            None,
        );
        match readiness.state {
            CityState::NotReady { code, fields, .. } => {
                assert_eq!(code, "invalid_input");
                assert_eq!(fields, vec!["snapshot_json".to_string()]);
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_snapshot_that_is_not_json_names_the_field() {
        let readiness = prepare_city(
            &snapshot_input("aderm_bere", "{not json"),
            "project",
            "solar",
            "fr",
            Path::new("/nonexistent"),
            &preparer(),
            None,
        );
        match readiness.state {
            CityState::NotReady { code, fields, .. } => {
                assert_eq!(code, "invalid_input");
                assert_eq!(fields, vec!["snapshot_json".to_string()]);
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn an_incomplete_record_is_reported_field_by_field_not_reconstructed() {
        // A seeded record missing the coordinates the engine computes with.
        let body = serde_json::json!({
            "00_templates": {},
            "01_city_inputs": {
                "city": { "city": "Bere", "country": "Tchad", "tz": "Africa/Ndjamena" },
                "design_options": { "project_scope": "plant_and_network" }
            }
        })
        .to_string();
        let readiness = prepare_city(
            &snapshot_input("aderm_bere", &body),
            "project",
            "solar",
            "fr",
            Path::new("/nonexistent"),
            &preparer(),
            None,
        );
        match readiness.state {
            CityState::NotReady { code, fields, .. } => {
                assert_eq!(code, "city_input_incomplete");
                assert!(fields.contains(&"01_city_inputs.city.latitude".to_string()));
                assert!(fields.contains(&"01_city_inputs.city.longitude".to_string()));
                assert!(fields.contains(&"01_city_inputs.city.altitude".to_string()));
            }
            other => panic!("expected an incomplete record, got {other:?}"),
        }
    }

    #[test]
    fn a_complete_record_with_no_weather_anywhere_reports_weather_unavailable() {
        // The record is complete — including the PV configuration used to
        // derive its reference-unit cache key — and there is simply nothing
        // to compute against yet. This is the state the input page paints on
        // a cold machine.
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/fixtures");
        let body = std::fs::read_to_string(fixtures.join("city/aderm_bere.snapshot.json"))
            .expect("complete city fixture");
        let readiness = prepare_city(
            &snapshot_input("aderm_bere", &body),
            "project",
            "solar",
            "fr",
            Path::new("/nonexistent"),
            &preparer(),
            None,
        );
        match readiness.state {
            CityState::NotReady { code, message, .. } => {
                // The record is complete; what is missing is the sealed
                // reference unit — the weather *and* the PV simulation
                // computed from it, which now arrive together or not at all.
                assert_eq!(code, "reference_unit_invalid");
                assert!(message.contains("pvgis"), "{message}");
            }
            other => panic!("expected reference_unit_invalid, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // The committed run
    // -----------------------------------------------------------------------

    /// One prepared corpus city, read from the ds-solar fixture set.
    ///
    /// A sibling-repo relative path on purpose: these are the same seven cities
    /// the engine's parity suite is frozen against, so a desktop test that
    /// passes here is testing the shipped engine and not a hand-written stub.
    fn prepared_fixture(city: &str) -> PreparedSolarCityInput {
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/fixtures");
        let snapshot: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(fixtures.join(format!("city/{city}.snapshot.json")))
                .expect("city fixture"),
        )
        .expect("city fixture is JSON");
        let input = SolarCityInput::from_city_snapshot(
            CityIdentity {
                project_id: "ds-solar-fixtures".to_string(),
                root: "solar".to_string(),
                city_id: city.to_string(),
                display_name: snapshot["_root"]["display_name"]
                    .as_str()
                    .unwrap_or(city)
                    .to_string(),
            },
            SnapshotRevision {
                source: "frozen-fixture".to_string(),
                captured_at: "2026-08-20T00:00:00Z".to_string(),
                record_updated_at_ms: None,
                revision: None,
            },
            RunOptions::default(),
            &snapshot,
        )
        .expect("the fixture snapshot normalises");
        let weather: ds_solar_contracts::WeatherDataset = serde_json::from_str(
            &std::fs::read_to_string(fixtures.join(format!("weather/{city}.weather.json")))
                .expect("weather fixture"),
        )
        .expect("weather fixture is JSON");
        let key = ds_solar_contracts::WeatherKey::new(
            input.site.latitude,
            input.site.longitude,
            weather.provenance.timezone.clone(),
            WeatherPin::pvgis_tmy(),
        )
        .expect("the weather key derives");
        let directory = fixtures.join("reference").join(city);
        let read =
            |name: &str| std::fs::read(directory.join(name)).expect("the sealed reference fixture");
        let reference =
            ds_solar_contracts::reference_unit::open(&ds_solar_contracts::BundleBytes {
                weather_json: read("weather.json"),
                reference_unit_json: read("reference-unit.json"),
                reference_unit_parquet: read("reference-unit.parquet"),
                manifest_json: read("manifest.json"),
            })
            .expect("the sealed reference unit verifies")
            .reference_unit;
        PreparedSolarCityInput::commit(input, key, weather, reference, "2026-08-20T00:00:00Z")
            .expect("the prepared input commits")
    }

    #[test]
    fn ready_states_project_the_same_bounded_cache_receipt_on_prepare_and_reread() {
        let input = prepared_fixture("aderm_bere");
        let expected = input.reference_cache_receipt();

        // `ready_city` is the one projection a fresh prepare uses after its
        // atomic commit. It must not carry cache bytes or a private path.
        let fresh = ready_city("aderm_bere".to_string(), &input, "provider");
        let fresh_json = serde_json::to_value(&fresh).expect("ready state serialises");
        assert_eq!(fresh_json["state"], "ready");
        assert_eq!(
            fresh_json["reference_cache_receipt"],
            serde_json::to_value(&expected).expect("receipt serialises")
        );
        for forbidden in ["path", "bytes", "weather_json", "reference_unit_parquet"] {
            assert!(
                fresh_json["reference_cache_receipt"]
                    .get(forbidden)
                    .is_none(),
                "cache receipt must not expose {forbidden}"
            );
        }

        // The durable readiness path reads the same committed input back and
        // must faithfully re-project the receipt rather than inventing a
        // current/default pin or a new timestamp.
        let directory = scratch_dir("cache-receipt-reread");
        write_prepared(&directory, "aderm_bere", &input).expect("prepared input commits");
        let reread = readiness_of(&directory, "aderm_bere");
        let reread_json = serde_json::to_value(&reread).expect("reread state serialises");
        assert_eq!(reread_json["state"], "ready");
        assert_eq!(
            reread_json["reference_cache_receipt"],
            serde_json::to_value(expected).expect("receipt serialises")
        );
        std::fs::remove_dir_all(directory).expect("scratch directory removes");
    }

    #[test]
    fn generation_profile_decodes_the_sealed_8760_hours_without_recomputing_pv() {
        let input = prepared_fixture("aderm_bere");
        let view = generation_profile_view("aderm_bere", &input)
            .expect("the verified reference unit projects");

        assert_eq!(view.schema_version, "ds-solar.generation-profile/v1");
        assert_eq!(view.source, "ds-solar-weather");
        assert_eq!(view.scope, "one_reference_inverter_and_strings");
        assert_eq!(view.rows, 8_760);
        assert_eq!(view.daily.len(), 365);
        assert_eq!(view.monthly.len(), 12);
        assert_eq!(view.average_day.len(), 24);
        assert_eq!(view.modules_per_reference_inverter, 238);
        assert_eq!(view.modules_per_string, 17);
        assert_eq!(view.strings_per_reference_inverter, 14);

        let daily_net: f64 = view.daily.iter().map(|bucket| bucket.net_energy_mwh).sum();
        let monthly_net: f64 = view
            .monthly
            .iter()
            .map(|bucket| bucket.net_energy_mwh)
            .sum();
        assert!((daily_net - view.annual_net_energy_mwh).abs() < 1e-9);
        assert!((monthly_net - view.annual_net_energy_mwh).abs() < 1e-9);
        assert!(view.specific_yield_kwh_kwp_year > 0.0);
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("ds-solar-native-{name}-{nonce}"));
        std::fs::create_dir_all(&directory).expect("scratch directory");
        directory
    }

    fn project_fixture(project: &str) -> PreparedSolarCityInput {
        project_fixture_city(project, "aderm_bere")
    }

    pub(super) fn project_fixture_city(project: &str, city: &str) -> PreparedSolarCityInput {
        let mut input = prepared_fixture(city);
        input.city.identity.project_id = project.into();
        input.city.identity.root = format!("eds_project/{project}/eds_solar");
        input.city.content_digest = input.city.compute_content_digest().unwrap();
        input.input_digest = input.compute_input_digest().unwrap();
        input.validate().unwrap();
        input
    }

    #[test]
    fn final_import_keeps_calculation_closed_and_reopens_verified_bytes_after_restart() {
        use ds_solar_contracts::{
            ArtifactRole, BatchArtifact, BatchCity, BatchRunIdentity, SolarArtifactBatch,
        };
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("authority");
        let queue = std::sync::Arc::new(LocalPublicationStore(
            ds_sync_runtime::open_store(&tmp.path().join("sync.sqlite")).unwrap(),
        ));
        let host = NativeHost::headless(data.clone())
            .unwrap()
            .with_publication_provider(queue.clone());
        let workspace =
            crate::workspace::solar_native_prepare_default_workspace(host.clone()).unwrap();
        let path = resolve_workspace_handle(&host, &workspace.workspace_handle).unwrap();
        let root = "eds_project/project-test/eds_solar";
        let source_dir = secure_private_directory(
            &native_root(&path)
                .join("runs")
                .join(root)
                .join("source-final"),
        )
        .unwrap();
        let city_dir = secure_private_directory(&source_dir.join("cities/town")).unwrap();
        let draft = "# APD\n\n![Chart](chart.png)\n![Map](gs://maps/eds_project/project-test/eds_solar/town/network_plots/_immutable/123e4567-e89b-42d3-a456-426614174001?generation=1)\n\n## Résumé exécutif\n\n<!-- LLM_OUTPUT_START -->\n[LLM interpretation will be inserted here]\n<!-- LLM_OUTPUT_END -->\n\n## A.6 Finance\n\nDonnées validées.\n";
        let final_text = draft.replace(
            "[LLM interpretation will be inserted here]",
            "Résumé vérifié.",
        );
        assert!(ds_solar_contracts::lint_final(draft, &final_text).ok());
        let report = b"{\"source\":\"sealed\"}";
        let png = b"\x89PNG\r\n\x1a\nverified-chart";
        write_new_final_file(&city_dir.join("chart.png"), png).unwrap();
        write_new_final_file(&city_dir.join(PARITY_DRAFT_NAME), draft.as_bytes()).unwrap();
        write_new_final_file(&city_dir.join(REPORT_INPUT_NAME), report).unwrap();
        let entry = |name: &str, media: &str, bytes: &[u8], role| {
            BatchArtifact::from_entry(
                &ds_solar_contracts::ArtifactEntry::from_bytes(
                    format!("cities/town/{name}"),
                    media,
                    bytes,
                ),
                role,
                Some("town"),
            )
        };
        let batch = SolarArtifactBatch::close(
            BatchRunIdentity {
                project_id: "project-test".into(),
                root: root.into(),
                run_id: "source-final".into(),
                cities: vec![BatchCity {
                    city_id: "town".into(),
                    input_digest: ds_solar_contracts::digest::sha256_hex(b"input"),
                    result_digest: ds_solar_contracts::digest::sha256_hex(b"result"),
                }],
            },
            vec![
                entry("chart.png", "image/png", png, ArtifactRole::Chart),
                entry(
                    PARITY_DRAFT_NAME,
                    "text/markdown",
                    draft.as_bytes(),
                    ArtifactRole::Report,
                ),
                entry(
                    REPORT_INPUT_NAME,
                    "application/json",
                    report,
                    ArtifactRole::ReportInput,
                ),
            ],
        )
        .unwrap();
        write_json(&source_dir.join("batch.json"), &batch).unwrap();
        write_json(
            &source_dir.join("sync-intent.json"),
            &batch.sync_intent("2026-09-17T00:00:00Z"),
        )
        .unwrap();
        let original_manifest = std::fs::read(source_dir.join("batch.json")).unwrap();
        let original_intent = std::fs::read(source_dir.join("sync-intent.json")).unwrap();
        let final_path = tmp.path().join("reviewed.md");
        std::fs::write(&final_path, final_text.as_bytes()).unwrap();
        let command = ImportFinalCommand {
            workspace_handle: workspace.workspace_handle.clone(),
            root: root.into(),
            run_id: "source-final".into(),
            city_id: "town".into(),
            source_path: Some(final_path.to_string_lossy().into()),
        };
        // Changed source bytes are refused before committing any final.
        std::fs::write(city_dir.join(PARITY_DRAFT_NAME), b"changed").unwrap();
        assert!(solar_native_import_final(host.clone(), command.clone()).is_err());
        assert!(!source_dir.join("finals/town").exists());
        std::fs::write(city_dir.join(PARITY_DRAFT_NAME), draft.as_bytes()).unwrap();
        let imported = solar_native_import_final(host.clone(), command.clone())
            .unwrap()
            .unwrap();
        assert!(
            imported.publication_error.is_none(),
            "{:?}",
            imported.publication_error
        );
        assert_eq!(imported.publication.as_ref().unwrap()["state"], "held");
        let fence = LocalPublicationStore::fence();
        let held = queue
            .0
            .lock()
            .unwrap()
            .snapshot(&fence, &ds_sync_runtime::rows::store_scope("project-test"))
            .unwrap()
            .artifacts;
        assert_eq!(held.len(), 1);
        assert_eq!(held[0].identity.operation, "report-town");
        assert_eq!(held[0].identity.variant, "apd-final-fr");
        assert_eq!(
            held[0].input_base_fingerprint.as_deref(),
            Some(ds_compute_runtime::digest(report).as_str())
        );
        assert_eq!(
            held[0].engine_build_manifest_sha256.as_deref(),
            Some(ds_solar_contracts::EngineIdentity::publication_manifest_sha256())
        );
        let producer = ds_sync_runtime::retained_publication::RetainedProducer {
            store: &queue.0,
            fence: &fence,
            project: "project-test",
        };
        let local = ds_sync_runtime::rows::local_row_from_store(&held[0]);
        assert_eq!(
            producer.output(&local, "apd-final-fr").unwrap(),
            final_text.as_bytes()
        );
        assert_ne!(imported.final_batch_digest, batch.batch_digest);
        assert_eq!(imported.source_batch_digest, batch.batch_digest);
        assert_eq!(
            std::fs::read(source_dir.join("batch.json")).unwrap(),
            original_manifest
        );
        assert_eq!(
            std::fs::read(source_dir.join("sync-intent.json")).unwrap(),
            original_intent
        );
        assert!(solar_native_import_final(host, command).is_err());
        let restarted = NativeHost::headless(data).unwrap();
        let inspected = solar_native_inspect_final(
            restarted.clone(),
            FinalSelector {
                workspace_handle: workspace.workspace_handle.clone(),
                root: root.into(),
                run_id: "source-final".into(),
                city_id: "town".into(),
            },
        )
        .unwrap();
        assert_eq!(inspected.final_batch_digest, imported.final_batch_digest);
        assert!(
            inspected.publication.is_none(),
            "inspection must never adopt or enqueue a final"
        );
        assert_eq!(
            queue
                .0
                .lock()
                .unwrap()
                .snapshot(&fence, &ds_sync_runtime::rows::store_scope("project-test"))
                .unwrap()
                .artifacts
                .len(),
            1
        );
        assert_eq!(
            inspected.markdown.content_digest,
            imported.markdown.content_digest
        );
        let request = serde_json::json!({"operation":"final_inspect","command":{"workspace_handle":workspace.workspace_handle,"root":root,"run_id":"source-final","city_id":"town"}});
        let receipt = crate::application::execute(
            restarted.clone(),
            "project-test",
            &serde_json::to_vec(&request).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["value"]["final_batch_id"], imported.final_batch_id);
        let destination = crate::workspace::secure_private_directory(&tmp.path().join("exports"))
            .unwrap()
            .join("final.zip");
        let package = serde_json::json!({"operation":"report_bundle","command":{
            "workspace_handle":workspace.workspace_handle,"root":root,"run_id":"source-final","city_id":"town",
            "expected_batch_id":batch.batch_id,"expected_batch_digest":batch.batch_digest,
            "name":imported.markdown.name,"destination":destination
        }});
        // No placeholder images, incomplete output, or Desktop fallback.
        assert!(
            crate::application::execute(
                restarted.clone(),
                "project-test",
                &serde_json::to_vec(&package).unwrap()
            )
            .is_err()
        );
        assert!(!destination.exists());
        struct Maps;
        impl crate::MediaProvider for Maps {
            fn fetch(&self, city: &str, reference: &str) -> Result<Vec<u8>, String> {
                assert_eq!(city, "town");
                assert!(reference.ends_with("?generation=1"));
                Ok(b"\x89PNG\r\n\x1a\nverified-map".to_vec())
            }
        }
        let packaged = crate::application::execute(
            restarted
                .clone()
                .with_media_provider(std::sync::Arc::new(Maps)),
            "project-test",
            &serde_json::to_vec(&package).unwrap(),
        )
        .unwrap();
        assert_eq!(packaged["value"]["image_count"], 2);
        struct Renderer;
        impl crate::ReportRenderer for Renderer {
            fn render(
                &self,
                copy: ds_command_kernel::solar_report_bundle::RenderCopy,
                format: crate::ReportFormat,
            ) -> Result<Vec<u8>, String> {
                assert!(copy.markdown.contains("Résumé vérifié."));
                assert!(copy.markdown.contains("media/") && !copy.markdown.contains("gs://"));
                assert_eq!(copy.media.len(), 2);
                Ok(match format {
                    crate::ReportFormat::Docx => b"PK\x03\x04fixture".to_vec(),
                    crate::ReportFormat::Pdf => b"%PDF-1.7\nfixture".to_vec(),
                })
            }
        }
        let finishing = restarted
            .clone()
            .with_media_provider(std::sync::Arc::new(Maps))
            .with_report_renderer(std::sync::Arc::new(Renderer));
        for format in ["docx", "pdf"] {
            let output = destination
                .parent()
                .unwrap()
                .join(format!("final.{format}"));
            let finish = serde_json::json!({"operation":"report_finish","command":{
                "source":{"scope":"city","batch":{"workspace_handle":workspace.workspace_handle,"root":root,"run_id":"source-final",
                    "expected_batch_id":batch.batch_id,"expected_batch_digest":batch.batch_digest},"city_id":"town","name":imported.markdown.name},
                "format":format,"destination":output}});
            let receipt = crate::application::execute(
                finishing.clone(),
                "project-test",
                &serde_json::to_vec(&finish).unwrap(),
            )
            .unwrap();
            let bytes = std::fs::read(&output).unwrap();
            assert_eq!(
                receipt["value"]["content_digest"],
                ds_solar_contracts::digest::sha256_hex(&bytes)
            );
            assert_eq!(
                receipt["value"]["source_content_digest"],
                imported.markdown.content_digest
            );
            assert_eq!(receipt["value"]["publication"], "not_requested");
            assert_eq!(receipt["value"]["presentation_copy"], true);
            assert!(
                crate::application::execute(
                    finishing.clone(),
                    "project-test",
                    &serde_json::to_vec(&finish).unwrap()
                )
                .is_err()
            );
        }
        struct ChangingRenderer {
            source: PathBuf,
        }
        impl crate::ReportRenderer for ChangingRenderer {
            fn render(
                &self,
                _copy: ds_command_kernel::solar_report_bundle::RenderCopy,
                _format: crate::ReportFormat,
            ) -> Result<Vec<u8>, String> {
                std::fs::write(&self.source, b"changed while rendering").unwrap();
                Ok(b"PK\x03\x04fixture".to_vec())
            }
        }
        let logical = format!("cities/town/{}", imported.markdown.name);
        let (final_directory, _) = final_context_at(&source_dir, &batch, &logical).unwrap();
        let final_path = final_directory.join(&logical);
        let output = destination.parent().unwrap().join("changed-source.docx");
        let finish = serde_json::json!({"operation":"report_finish","command":{
            "source":{"scope":"city","batch":{"workspace_handle":workspace.workspace_handle,"root":root,"run_id":"source-final",
                "expected_batch_id":batch.batch_id,"expected_batch_digest":batch.batch_digest},"city_id":"town","name":imported.markdown.name},
            "format":"docx","destination":output}});
        let error = crate::application::execute(
            restarted
                .clone()
                .with_media_provider(std::sync::Arc::new(Maps))
                .with_report_renderer(std::sync::Arc::new(ChangingRenderer {
                    source: final_path.clone(),
                })),
            "project-test",
            &serde_json::to_vec(&finish).unwrap(),
        )
        .unwrap_err();
        assert!(error.contains("closed manifest"));
        assert!(!output.exists());
        std::fs::write(final_path, final_text.as_bytes()).unwrap();
        assert_eq!(
            packaged["value"]["source_content_digest"],
            imported.markdown.content_digest
        );
        assert!(
            std::fs::read(&destination)
                .unwrap()
                .starts_with(b"PK\x03\x04")
        );
        assert_eq!(
            std::fs::read(source_dir.join("batch.json")).unwrap(),
            original_manifest
        );

        let name = format!("cities/town/{}", imported.markdown.name);
        let read = BatchArtifactCommand {
            workspace_handle: workspace.workspace_handle.clone(),
            root: root.into(),
            run_id: "source-final".into(),
            name: name.clone(),
            max_bytes: 2 << 20,
        };
        assert_eq!(
            solar_native_read_batch_artifact(restarted.clone(), read.clone()).unwrap(),
            final_text.as_bytes()
        );
        let mut stream = open_sealed_batch_artifact_at(
            &source_dir,
            &SealedBatchArtifactAuthority {
                workspace_handle: workspace.workspace_handle,
                root: root.into(),
                run_id: "source-final".into(),
                name: name.clone(),
                expected_byte_count: imported.markdown.byte_count,
                expected_content_digest: imported.markdown.content_digest,
            },
        )
        .unwrap();
        let mut bytes = Vec::new();
        stream.file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, final_text.as_bytes());
        std::fs::write(city_dir.join(REPORT_INPUT_NAME), b"changed").unwrap();
        assert!(solar_native_read_batch_artifact(restarted.clone(), read.clone()).is_err());
        std::fs::write(city_dir.join(REPORT_INPUT_NAME), report).unwrap();
        std::fs::write(source_dir.join("finals/town").join(&name), b"changed").unwrap();
        assert!(solar_native_read_batch_artifact(restarted.clone(), read.clone()).is_err());
        assert_eq!(
            std::fs::read(source_dir.join("batch.json")).unwrap(),
            original_manifest
        );
    }

    #[test]
    fn prepared_import_is_project_checked_and_compare_and_swap_is_atomic() {
        let directory = scratch_dir("prepared-cas");
        let host = NativeHost::headless(directory.join("authority")).unwrap();
        let workspace =
            crate::workspace::solar_native_prepare_default_workspace(host.clone()).unwrap();
        let reopened = crate::workspace::solar_native_prepare_default_workspace(
            NativeHost::headless(directory.join("authority")).unwrap(),
        )
        .unwrap();
        assert_eq!(workspace.workspace_handle, reopened.workspace_handle);
        let original = project_fixture("project_a");
        let file = directory.join("original.json");
        write_json(&file, &original).unwrap();
        assert!(
            import_prepared(&host, &workspace.workspace_handle, "project_b", &file, None).is_err()
        );
        assert_eq!(
            import_prepared(&host, &workspace.workspace_handle, "project_a", &file, None).unwrap()
                ["status"],
            "imported"
        );
        assert_eq!(
            import_prepared(&host, &workspace.workspace_handle, "project_a", &file, None).unwrap()
                ["status"],
            "unchanged"
        );

        let mut changed = original.clone();
        changed.city.identity.display_name = "Revised city".into();
        changed.city.content_digest = changed.city.compute_content_digest().unwrap();
        changed.input_digest = changed.compute_input_digest().unwrap();
        changed.validate().unwrap();
        let replacement = directory.join("replacement.json");
        write_json(&replacement, &changed).unwrap();
        assert!(
            import_prepared(
                &host,
                &workspace.workspace_handle,
                "project_a",
                &replacement,
                None
            )
            .is_err()
        );
        let mut contenders = Vec::new();
        for n in 0..2 {
            let mut contender = changed.clone();
            contender.city.identity.display_name = format!("Contender {n}");
            contender.city.content_digest = contender.city.compute_content_digest().unwrap();
            contender.input_digest = contender.compute_input_digest().unwrap();
            let path = directory.join(format!("contender-{n}.json"));
            write_json(&path, &contender).unwrap();
            let host = host.clone();
            let handle = workspace.workspace_handle.clone();
            let expected = original.input_digest.clone();
            contenders.push(std::thread::spawn(move || {
                import_prepared(&host, &handle, "project_a", &path, Some(&expected))
            }));
        }
        assert_eq!(
            contenders
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .filter(Result::is_ok)
                .count(),
            1
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn refused_launch_releases_admission_and_settlement_survives_restart() {
        let directory = scratch_dir("durable-run");
        let data = directory.join("authority");
        let host = NativeHost::headless(data.clone()).unwrap();
        let workspace =
            crate::workspace::solar_native_prepare_default_workspace(host.clone()).unwrap();
        let file = directory.join("prepared.json");
        write_json(&file, &project_fixture("project_a")).unwrap();
        import_prepared(&host, &workspace.workspace_handle, "project_a", &file, None).unwrap();
        let second = directory.join("second-prepared.json");
        write_json(
            &second,
            &project_fixture_city("project_a", "aderm_beinamar"),
        )
        .unwrap();
        import_prepared(
            &host,
            &workspace.workspace_handle,
            "project_a",
            &second,
            None,
        )
        .unwrap();
        let third = directory.join("third-prepared.json");
        write_json(&third, &project_fixture_city("project_a", "aderm_gagal")).unwrap();
        import_prepared(
            &host,
            &workspace.workspace_handle,
            "project_a",
            &third,
            None,
        )
        .unwrap();
        let mut command = CalculateCommand {
            workspace_handle: workspace.workspace_handle,
            project_id: "project_a".into(),
            root: "eds_project/project_a/eds_solar".into(),
            city_ids: vec![
                "aderm_bere".into(),
                "aderm_beinamar".into(),
                "aderm_gagal".into(),
            ],
            run_id: "durable-test".into(),
            render_charts: false,
            generate_documents: Some(false),
            report_drafts: vec!["bad-intent".into()],
            concurrency: Some(0),
        };
        assert!(solar_native_calculate(host.clone(), &host.state(), command.clone()).is_err());
        assert!(!data.join("solar-jobs/durable-test.json").exists());
        command.report_drafts.clear();
        solar_native_calculate(host.clone(), &host.state(), command.clone()).unwrap();
        assert!(solar_native_calculate(host.clone(), &host.state(), command.clone()).is_err());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !data.join("solar-jobs/durable-test.json").exists()
            || serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(data.join("solar-jobs/durable-test.json")).unwrap(),
            )
            .unwrap()["done"]
                != true
        {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let restarted = NativeHost::headless(data).unwrap();
        let receipt = calculate_progress(&restarted, "project_a", "durable-test").unwrap();
        assert_eq!(receipt["done"], true);
        assert!(receipt["report"]["cities"][0]["result_digest"].is_string());
        assert!(calculate_progress(&restarted, "project_b", "durable-test").is_err());
        let dashboard_request = serde_json::json!({"operation":"dashboard","command":{
            "workspace_handle":command.workspace_handle,"root":command.root,"run_id":command.run_id,
            "expected_batch_id":receipt["report"]["batch"]["batch_id"],
            "expected_batch_digest":receipt["report"]["batch"]["batch_digest"],
            "section":"site","city_id":"aderm_bere"}});
        let dashboard =
            serde_json::from_value::<crate::application::Request>(dashboard_request.clone())
                .unwrap()
                .execute(restarted.clone(), "project_a")
                .unwrap();
        assert_eq!(dashboard["value"]["dashboard"]["section"], "site");
        assert!(
            dashboard["value"]["html"]
                .as_str()
                .unwrap()
                .contains("<!doctype html>")
        );
        let mut stale = dashboard_request;
        stale["command"]["expected_batch_digest"] = serde_json::json!("sha256:foreign");
        assert!(
            serde_json::from_value::<crate::application::Request>(stale)
                .unwrap()
                .execute(restarted.clone(), "project_a")
                .unwrap_err()
                .contains("pinned batch")
        );
        let portfolio_command = PortfolioCalculateCommand {
            workspace_handle: command.workspace_handle.clone(),
            project_id: command.project_id.clone(),
            root: command.root.clone(),
            source_run_id: command.run_id.clone(),
            run_id: "portfolio-dashboard".into(),
            portfolio_id: "portfolio_a".into(),
            portfolio_name: "Two cities".into(),
            membership_revision:
                "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
            city_ids: vec!["aderm_beinamar".into(), "aderm_bere".into()],
            currency: None,
            project_years: None,
            discount_rate: None,
            representative_strategy: "first".into(),
            representative_city_id: None,
            round_robin_start_index: None,
            language: "fr".into(),
            report_intents: vec!["apd".into(), "financial".into()],
        };
        let portfolio =
            solar_native_calculate_portfolio(restarted.clone(), portfolio_command.clone()).unwrap();
        let job = portfolio_jobs::start(restarted.clone(), "project_a", portfolio_command.clone())
            .unwrap();
        assert!(!job.done);
        assert!(portfolio_jobs::cancel(&restarted, "project_b", &job.job_id).is_err());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let settled = loop {
            let progress = portfolio_jobs::progress(&restarted, "project_a", &job.job_id).unwrap();
            if progress.done {
                break progress;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "portfolio worker did not settle"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!(
            settled.phase,
            portfolio_jobs::Phase::Ready,
            "{:?}",
            settled.error
        );
        assert_eq!(
            settled.result.as_ref().unwrap()["batch"]["batch_digest"],
            portfolio.batch.batch_digest
        );
        let worker_restart = NativeHost::headless(restarted.data_dir().unwrap()).unwrap();
        let recovered =
            portfolio_jobs::progress(&worker_restart, "project_a", &job.job_id).unwrap();
        assert_eq!(recovered.phase, portfolio_jobs::Phase::Ready);
        assert_eq!(recovered.result, settled.result);
        assert_eq!(portfolio.source_city_result_digests.len(), 2);
        assert_eq!(
            portfolio.source_city_result_digests[0].city_id,
            "aderm_beinamar"
        );
        let portfolio_request = serde_json::json!({"operation":"dashboard","command":{
            "workspace_handle":command.workspace_handle,"root":command.root,"run_id":"portfolio-dashboard",
            "expected_batch_id":portfolio.batch.batch_id,"expected_batch_digest":portfolio.batch.batch_digest,
            "section":"portfolio_finance","portfolio_id":"portfolio_a",
            "membership_revision":"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}});
        let dashboard =
            serde_json::from_value::<crate::application::Request>(portfolio_request.clone())
                .unwrap()
                .execute(restarted.clone(), "project_a")
                .unwrap();
        assert_eq!(
            dashboard["value"]["dashboard"]["section"],
            "portfolio_finance"
        );
        assert!(
            dashboard["value"]["html"]
                .as_str()
                .unwrap()
                .contains("<svg")
        );
        for section in ["portfolio_site", "portfolio_plant", "portfolio_boq"] {
            let mut request = portfolio_request.clone();
            request["command"]["section"] = serde_json::json!(section);
            let dashboard = serde_json::from_value::<crate::application::Request>(request)
                .unwrap()
                .execute(restarted.clone(), "project_a")
                .unwrap();
            assert_eq!(dashboard["value"]["dashboard"]["section"], section);
            assert!(
                dashboard["value"]["dashboard"]["derived"]["tables"]
                    .as_array()
                    .is_some_and(|tables| !tables.is_empty())
            );
            assert!(
                dashboard["value"]["html"]
                    .as_str()
                    .unwrap()
                    .contains("<table>")
            );
            assert!(
                !dashboard["value"]["html"]
                    .as_str()
                    .unwrap()
                    .contains("<script")
            );
        }
        let report_name = portfolio
            .files
            .iter()
            .find(|file| {
                file.name == "portfolio-financier-fr.md" && file.media_type == MARKDOWN_MEDIA_TYPE
            })
            .unwrap()
            .name
            .clone();
        let zip_path = directory.join("portfolio-financial.zip");
        let package_request = serde_json::json!({"operation":"portfolio_report_bundle","command":{
            "batch":{"workspace_handle":command.workspace_handle,"root":command.root,"run_id":portfolio.run_id,
                "expected_batch_id":portfolio.batch.batch_id,"expected_batch_digest":portfolio.batch.batch_digest},
            "source_run_id":command.run_id,"expected_source_batch_id":portfolio.source_batch_id,
            "expected_source_batch_digest":portfolio.source_batch_digest,"name":report_name,"destination":zip_path}});
        let package =
            serde_json::from_value::<crate::application::Request>(package_request.clone())
                .unwrap()
                .execute(restarted.clone(), "project_a")
                .unwrap();
        assert!(package["value"]["image_count"].as_u64().unwrap() > 0);
        let bytes = std::fs::read(&zip_path).unwrap();
        assert!(bytes.starts_with(b"PK"));
        assert_eq!(
            package["value"]["content_digest"],
            ds_solar_contracts::digest::sha256_hex(&bytes)
        );
        let mut stale = package_request;
        stale["command"]["expected_source_batch_digest"] =
            serde_json::json!(format!("sha256:{}", "e".repeat(64)));
        let refused_destination = directory.join("refused-portfolio.zip");
        stale["command"]["destination"] = serde_json::json!(refused_destination);
        assert!(
            serde_json::from_value::<crate::application::Request>(stale)
                .unwrap()
                .execute(restarted.clone(), "project_a")
                .is_err()
        );
        assert!(!refused_destination.exists());
        struct TestPublication(std::sync::atomic::AtomicUsize, ds_sync_runtime::SharedStore);
        impl crate::PublicationProvider for TestPublication {
            fn retain(
                &self,
                publication: crate::sync::Publication,
            ) -> Result<serde_json::Value, String> {
                assert!(
                    publication
                        .row
                        .outputs
                        .iter()
                        .any(|output| output.output_id == "portfolio-financier-fr")
                );
                assert!(
                    publication
                        .row
                        .outputs
                        .iter()
                        .any(|output| output.format == "png")
                );
                for output in &publication.outputs {
                    let declaration = publication
                        .row
                        .outputs
                        .iter()
                        .find(|item| item.output_id == output.output_id)
                        .unwrap();
                    assert_eq!(declaration.size_bytes, output.bytes.len() as u64);
                    assert_eq!(
                        declaration.sha256,
                        ds_solar_contracts::digest::hex_sha256(&output.bytes)
                    );
                }
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let fence = ds_command_kernel::sync_store::Fence {
                    account: "native-test".into(),
                    deployment: "https://gateway.example".into(),
                    install_id: "native-test".into(),
                };
                let producer = ds_sync_runtime::retained_publication::RetainedProducer {
                    store: &self.1,
                    fence: &fence,
                    project: &publication.project,
                };
                let sealed = ds_sync_runtime::retained_publication::seal(
                    &self.1,
                    &fence,
                    &publication.project,
                    &publication.row,
                    publication.declaration,
                    publication.outputs,
                    &|previous| {
                        ds_sync_runtime::Producer::reclaim(
                            &producer,
                            &publication.project,
                            &ds_sync_runtime::rows::local_row_from_store(previous),
                        )
                    },
                )?;
                let (key, state) = match sealed {
                    ds_sync_runtime::seal::Recorded::Recorded { row, .. } => {
                        (row.client_publish_id, row.state)
                    }
                    ds_sync_runtime::seal::Recorded::AlreadyRecorded { replay_key, state } => {
                        (replay_key, state)
                    }
                    ds_sync_runtime::seal::Recorded::Refused(refusal) => return Err(refusal.detail),
                };
                Ok(crate::sync::receipt(
                    &publication.project,
                    &publication.row.client_publish_id,
                    &key,
                    state,
                ))
            }
            fn observe(
                &self,
                publication: &crate::sync::Publication,
            ) -> Result<serde_json::Value, String> {
                let fence = ds_command_kernel::sync_store::Fence {
                    account: "native-test".into(),
                    deployment: "https://gateway.example".into(),
                    install_id: "native-test".into(),
                };
                crate::sync::recorded(&self.1, &fence, publication)
            }
        }
        let publisher = std::sync::Arc::new(TestPublication(
            std::sync::atomic::AtomicUsize::new(0),
            ds_sync_runtime::open_store(&directory.join("native-sync.sqlite")).unwrap(),
        ));
        let publishing = restarted
            .clone()
            .with_publication_provider(publisher.clone());
        let publish_request = serde_json::json!({"operation":"portfolio_publish","command":{
            "workspace_handle":command.workspace_handle,"root":command.root,"run_id":portfolio.run_id,
            "expected_batch_id":portfolio.batch.batch_id,"expected_batch_digest":portfolio.batch.batch_digest}});
        let unsealed =
            serde_json::from_value::<crate::application::Request>(publish_request.clone())
                .unwrap()
                .execute(publishing.clone(), "project_a")
                .unwrap_err();
        assert!(unsealed.contains("publication_not_sealed"), "{unsealed}");
        assert_eq!(publisher.0.load(std::sync::atomic::Ordering::SeqCst), 0);
        let completion = publication::completed(
            publishing.clone(),
            serde_json::from_value(publish_request["command"].clone()).unwrap(),
            &portfolio.engine_build_manifest_sha256,
        )
        .unwrap();
        assert_eq!(completion["receipt"]["publication"]["state"], "held");
        let published =
            serde_json::from_value::<crate::application::Request>(publish_request.clone())
                .unwrap()
                .execute(publishing.clone(), "project_a")
                .unwrap();
        assert_eq!(
            published["value"]["receipt"]["publication"]["state"],
            "held"
        );
        assert!(
            published["value"]["receipt"]["output_count"]
                .as_u64()
                .unwrap()
                > 2
        );
        let mut moved = publish_request.clone();
        moved["command"]["expected_batch_digest"] =
            serde_json::json!(format!("sha256:{}", "f".repeat(64)));
        assert!(
            serde_json::from_value::<crate::application::Request>(moved)
                .unwrap()
                .execute(publishing.clone(), "project_a")
                .is_err()
        );
        let portfolio_directory = portfolio_run_directory(
            &publishing,
            &command.workspace_handle,
            &command.root,
            &portfolio.run_id,
        )
        .unwrap();
        let input_path = portfolio_directory.join("portfolio-run-input.json");
        let sealed_input = std::fs::read(&input_path).unwrap();
        std::fs::write(&input_path, b"changed portfolio authority").unwrap();
        assert!(
            serde_json::from_value::<crate::application::Request>(publish_request)
                .unwrap()
                .execute(publishing.clone(), "project_a")
                .is_err()
        );
        std::fs::write(input_path, sealed_input).unwrap();
        assert_eq!(publisher.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        let automatic = solar_native_calculate_portfolio(
            publishing,
            PortfolioCalculateCommand {
                run_id: "portfolio-auto-seal".into(),
                ..portfolio_command
            },
        )
        .unwrap();
        assert!(
            automatic.publication_error.is_none(),
            "{:?}",
            automatic.publication_error
        );
        assert_eq!(
            automatic.publication.unwrap()["publication"]["state"],
            "held"
        );
        assert_eq!(publisher.0.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(solar_native_calculate(restarted.clone(), &restarted.state(), command).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn portfolio_final_keeps_its_source_unchanged_and_reopens_after_restart() {
        use ds_solar_contracts::{ArtifactRole, BatchCity, BatchRunIdentity, SolarArtifactBatch};
        let directory = scratch_dir("portfolio-final");
        let data = directory.join("data");
        let host = NativeHost::headless(data.clone()).unwrap();
        let workspace =
            crate::workspace::solar_native_prepare_default_workspace(host.clone()).unwrap();
        let workspace_path = resolve_workspace_handle(&host, &workspace.workspace_handle).unwrap();
        let root = "eds_project/project-test/eds_solar";
        let source_dir = secure_private_directory(
            &native_root(&workspace_path)
                .join("portfolio-runs")
                .join(root)
                .join("portfolio-source"),
        )
        .unwrap();
        let draft = "# Portfolio financier\n\n![Chart](plots/portfolio/finance.png)\n\n## Résumé exécutif\n\n<!-- LLM_OUTPUT_START -->\n[LLM interpretation will be inserted here]\n<!-- LLM_OUTPUT_END -->\n\n## Finance\n\nDonnées validées.\n";
        let reviewed = draft.replace(
            "[LLM interpretation will be inserted here]",
            "Résumé financier vérifié.",
        );
        let png = b"\x89PNG\r\n\x1a\nverified-chart";
        secure_private_directory(&source_dir.join("plots/portfolio")).unwrap();
        let files = [
            (
                "portfolio-financier-fr.md",
                "text/markdown",
                draft.as_bytes(),
                ArtifactRole::Report,
            ),
            (
                "portfolio-run-input.json",
                "application/json",
                b"{}".as_slice(),
                ArtifactRole::PortfolioRunInput,
            ),
            (
                PORTFOLIO_RESULT_NAME,
                "application/json",
                b"{}".as_slice(),
                ArtifactRole::PortfolioResult,
            ),
            (
                "plots/portfolio/finance.png",
                "image/png",
                png.as_slice(),
                ArtifactRole::PortfolioChart,
            ),
        ];
        let mut artifacts = Vec::new();
        for (name, media, bytes, role) in files {
            write_new_final_file(&source_dir.join(name), bytes).unwrap();
            artifacts.push(test_artifact(name, role, media, bytes));
        }
        let batch = SolarArtifactBatch::close(
            BatchRunIdentity {
                project_id: "project-test".into(),
                root: root.into(),
                run_id: "portfolio-source".into(),
                cities: ["a", "b"]
                    .into_iter()
                    .map(|city| BatchCity {
                        city_id: city.into(),
                        input_digest: ds_solar_contracts::digest::sha256_hex(city.as_bytes()),
                        result_digest: ds_solar_contracts::digest::sha256_hex(city.as_bytes()),
                    })
                    .collect(),
            },
            artifacts,
        )
        .unwrap();
        write_json(&source_dir.join("batch.json"), &batch).unwrap();
        write_json(
            &source_dir.join("sync-intent.json"),
            &batch.sync_intent("2026-09-17T00:00:00Z"),
        )
        .unwrap();
        let before_batch = std::fs::read(source_dir.join("batch.json")).unwrap();
        let before_intent = std::fs::read(source_dir.join("sync-intent.json")).unwrap();
        let input = directory.join("reviewed.md");
        write_new_final_file(&input, reviewed.as_bytes()).unwrap();
        let request = serde_json::json!({"operation":"portfolio_final_import","command":{
            "batch":{"workspace_handle":workspace.workspace_handle,"root":root,"run_id":"portfolio-source","expected_batch_id":batch.batch_id,"expected_batch_digest":batch.batch_digest},
            "source_name":"portfolio-financier-fr.md","source_path":input}});
        let imported = serde_json::from_value::<crate::application::Request>(request.clone())
            .unwrap()
            .execute(host.clone(), "project-test")
            .unwrap();
        assert_ne!(imported["value"]["final_batch_digest"], batch.batch_digest);
        assert_eq!(
            std::fs::read(source_dir.join("batch.json")).unwrap(),
            before_batch
        );
        assert_eq!(
            std::fs::read(source_dir.join("sync-intent.json")).unwrap(),
            before_intent
        );
        assert!(
            serde_json::from_value::<crate::application::Request>(request.clone())
                .unwrap()
                .execute(host, "project-test")
                .is_err()
        );
        let restarted = NativeHost::headless(data).unwrap();
        let inspect = serde_json::json!({"operation":"portfolio_final_inspect","command":{"batch":request["command"]["batch"],"name":"portfolio-financier-final-fr.md"}});
        let recovered = serde_json::from_value::<crate::application::Request>(inspect)
            .unwrap()
            .execute(restarted.clone(), "project-test")
            .unwrap();
        assert_eq!(recovered["value"], imported["value"]);
        let bytes = solar_native_read_portfolio_batch_artifact(
            restarted,
            PortfolioBatchArtifactCommand {
                workspace_handle: workspace.workspace_handle,
                root: root.into(),
                run_id: "portfolio-source".into(),
                name: "portfolio-financier-final-fr.md".into(),
                max_bytes: MAX_DOCUMENT_BYTES,
                expected_batch_id: batch.batch_id,
                expected_batch_digest: batch.batch_digest,
                expected_media_type: "text/markdown".into(),
                expected_byte_count: reviewed.len() as u64,
                expected_content_digest: ds_solar_contracts::digest::sha256_hex(
                    reviewed.as_bytes(),
                ),
            },
        )
        .unwrap();
        assert_eq!(bytes, reviewed.as_bytes());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn admitted_work_without_a_settlement_is_reported_interrupted_after_restart() {
        let directory = scratch_dir("interrupted-run");
        let host = NativeHost::headless(directory.clone()).unwrap();
        secure_private_directory(&directory.join("solar-jobs")).unwrap();
        write_json(&directory.join("solar-jobs/stopped.json"), &serde_json::json!({"schema":"ds-solar.durable-run/v1","project_id":"project_a","root":"eds_project/project_a/eds_solar","run_id":"stopped","done":false})).unwrap();
        let receipt = calculate_progress(&host, "project_a", "stopped").unwrap();
        assert_eq!(receipt["status"], "interrupted");
        assert_eq!(receipt["done"], true);
        assert!(receipt.get("report").is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn run_one_city(
        city: &str,
        output_dir: &Path,
    ) -> (ds_solar_runtime::RunOutput, Vec<CommittedCity>) {
        let prepared = vec![prepared_fixture(city)];
        let runtime = SolarRuntime::new();
        let output = run_cities(
            &runtime,
            &prepared,
            &CalculationOptions::default()
                .with_run_id("run-test")
                .full_output(),
            ds_solar_runtime::Concurrency::serial(),
            "ds-solar-fixtures",
            "solar",
        )
        .expect("the corpus city runs");
        let committed = commit_run(output_dir, &output, |_, _| {}).expect("the run commits");
        (output, committed)
    }

    #[test]
    fn standalone_batch_commits_the_same_city_bytes_and_documents_as_the_owner() {
        use ds_solar_contracts::prepared_batch::{
            PREPARED_BATCH_REQUEST_SCHEMA, PreparedBatchOptions, PreparedBatchRequest,
        };
        let root = scratch_dir("standalone-native-parity");
        let linked = secure_private_directory(&root.join("owner")).unwrap();
        let native = secure_private_directory(&root.join("native")).unwrap();
        let (expected, expected_commits) = run_one_city("aderm_bere", &linked);
        let request = PreparedBatchRequest {
            schema_version: PREPARED_BATCH_REQUEST_SCHEMA.into(),
            project_id: "ds-solar-fixtures".into(),
            root: "solar".into(),
            prepared: vec![prepared_fixture("aderm_bere")],
            options: PreparedBatchOptions {
                run_id: Some("run-test".into()),
                render_charts: true,
                generate_documents: true,
                ..Default::default()
            },
            concurrency: Some(1),
        };
        let cancel = CancelToken::new();
        let engine = ds_solar_host::process::BatchEngine::capture(
            &ds_solar_host::process::installed_engine_path().unwrap(),
            &root,
            ds_solar_host::compute_parallelism(),
            &cancel,
        )
        .unwrap();
        let events = Mutex::new(Vec::new());
        let sink = |event| events.lock().unwrap().push(event);
        let output = engine.calculate(&request, &cancel, Some(&sink)).unwrap();
        assert_eq!(output.output.batch, expected.batch);
        let commits = engine_batch::commit(&native, &output, &cancel, |_, _| {}).unwrap();
        assert_eq!(
            json_bytes(&commits[0].documents).unwrap(),
            json_bytes(&expected_commits[0].documents).unwrap()
        );
        for file in &output.outputs {
            assert_eq!(
                std::fs::read(linked.join(&file.entry.name)).unwrap(),
                std::fs::read(native.join(&file.entry.name)).unwrap(),
                "{}",
                file.entry.name,
            );
        }
        assert_eq!(output.completion_build_manifest_sha256.len(), 64);
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|event| event.event.phase == ds_solar_contracts::ProgressPhase::RunFinished)
        );
        assert!(
            !native.join("batch.json").exists(),
            "only the host closes its final local batch"
        );
        let interrupted = secure_private_directory(&root.join("cancelled")).unwrap();
        cancel.cancel();
        assert!(
            engine_batch::commit(&interrupted, &output, &cancel, |_, _| {})
                .err()
                .unwrap()
                .contains("cancelled")
        );
        assert!(!interrupted.join("cities/aderm_bere/result.json").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_committed_run_lays_out_result_documents_and_every_referenced_figure() {
        let output_dir = scratch_dir("layout");
        let (output, committed) = run_one_city("aderm_bere", &output_dir);
        let city_dir = output_dir.join("cities").join("aderm_bere");

        for name in [
            "result.json",
            LOAD_PROFILE_NAME,
            "report-model.json",
            PARITY_DRAFT_NAME,
            PORTFOLIO_CONTRIBUTION_NAME,
            PORTFOLIO_CALCULATION_NAME,
        ] {
            assert!(city_dir.join(name).is_file(), "{name} was not committed");
        }
        // No temporary file survived the atomic writes.
        assert!(
            std::fs::read_dir(&city_dir)
                .expect("city directory")
                .filter_map(|entry| entry.ok())
                .all(|entry| !entry.file_name().to_string_lossy().ends_with(".partial")),
            "a partial write was left behind"
        );

        let city = &committed[0];
        assert!(city.documents.apd.is_none());
        assert!(city.documents.draft.is_some());
        assert!(
            city.documents.section_count >= 9,
            "{} sections",
            city.documents.section_count
        );
        assert!(
            city.documents.figure_count >= 20,
            "{} figures",
            city.documents.figure_count
        );
        assert_eq!(
            city.documents.chart_count,
            count_charts(&city_dir).expect("chart count")
        );
        assert!(city.documents.chart_count >= 23);

        let profile: serde_json::Value = serde_json::from_slice(
            &std::fs::read(city_dir.join(LOAD_PROFILE_NAME)).expect("daily profile reads"),
        )
        .expect("daily profile is JSON");
        assert_eq!(profile["schema_version"], "ds-solar.daily-load-profile/v1");
        assert_eq!(profile["rows"], 24);
        assert_eq!(
            profile["columns"]["load_w"]["values"]
                .as_array()
                .map(Vec::len),
            Some(24)
        );

        // Every image the committed APD draft references exists beside it, at the
        // size and digest the batch advertises.
        let markdown =
            std::fs::read_to_string(city_dir.join(PARITY_DRAFT_NAME)).expect("the APD draft reads");
        let batch_by_name: BTreeMap<&str, &ds_solar_contracts::BatchArtifact> = output
            .batch
            .artifacts
            .iter()
            .map(|artifact| (artifact.name.as_str(), artifact))
            .collect();
        let mut referenced = 0_usize;
        for fragment in markdown.split("![").skip(1) {
            let Some(open) = fragment.find("](") else {
                continue;
            };
            let Some(close) = fragment[open + 2..].find(')') else {
                continue;
            };
            let target = &fragment[open + 2..open + 2 + close];
            if !target.starts_with("plots/") {
                continue;
            }
            let path = city_dir.join(target);
            assert!(path.is_file(), "{target} is referenced but not committed");
            let bytes = std::fs::read(&path).expect("the figure reads");
            let logical = format!("cities/aderm_bere/{target}");
            let artifact = batch_by_name
                .get(logical.as_str())
                .unwrap_or_else(|| panic!("{logical} is not in the batch"));
            assert_eq!(artifact.byte_count, bytes.len() as u64, "{target} size");
            assert_eq!(
                artifact.content_digest,
                ds_solar_contracts::digest::sha256_hex(&bytes),
                "{target} digest"
            );
            referenced += 1;
        }
        assert!(
            referenced >= 19,
            "only {referenced} figures were referenced"
        );
        std::fs::remove_dir_all(&output_dir).ok();
    }

    #[test]
    fn the_committed_documents_carry_the_digests_the_batch_advertises() {
        let output_dir = scratch_dir("digests");
        let (output, _) = run_one_city("aderm_gagal", &output_dir);
        // Per-city reports only. The root-level portfolio result and draft are
        // verified by `a_closed_native_batch_only_references_materialized_portfolio_artifacts`.
        let reports: Vec<&ds_solar_contracts::BatchArtifact> = output
            .batch
            .artifacts
            .iter()
            .filter(|artifact| artifact.role == ds_solar_contracts::ArtifactRole::Report)
            .filter(|artifact| artifact.city_id.is_some())
            .collect();
        assert_eq!(reports.len(), 1, "one selected deterministic draft");
        for artifact in reports {
            let relative = artifact
                .name
                .strip_prefix("cities/aderm_gagal/")
                .expect("the artifact is namespaced by city");
            let bytes = std::fs::read(output_dir.join("cities/aderm_gagal").join(relative))
                .expect("the document reads");
            assert_eq!(artifact.byte_count, bytes.len() as u64);
            assert_eq!(
                artifact.content_digest,
                ds_solar_contracts::digest::sha256_hex(&bytes)
            );
        }
        std::fs::remove_dir_all(&output_dir).ok();
    }

    #[test]
    fn a_city_only_batch_does_not_invent_root_portfolio_artifacts() {
        let output_dir = scratch_dir("city-only-batch");
        let (output, _committed) = run_one_city("aderm_bere", &output_dir);

        // City calculation no longer implies portfolio membership. Until an
        // explicit portfolio input is supplied, neither the runtime nor the
        // desktop shell may derive aggregate identity from a successful city.
        assert!(output.portfolio.is_none());
        assert!(output.portfolio_document.is_none());
        let batch = output.batch.clone();
        write_json(&output_dir.join("batch.json"), &batch).expect("batch writes last");

        let reopened: ds_solar_contracts::SolarArtifactBatch = serde_json::from_slice(
            &std::fs::read(output_dir.join("batch.json")).expect("batch reads"),
        )
        .expect("batch JSON parses");
        reopened.validate().expect("the re-opened batch verifies");

        assert!(
            reopened
                .artifacts
                .iter()
                .all(|artifact| artifact.city_id.is_some())
        );
        assert!(!output_dir.join(PORTFOLIO_RESULT_NAME).exists());
        assert!(!output_dir.join(PORTFOLIO_DRAFT_NAME).exists());
        std::fs::remove_dir_all(&output_dir).ok();
    }

    #[test]
    fn the_calculation_pipeline_never_renders_a_final_document() {
        let output_dir = scratch_dir("no-final-documents");
        let (_, committed) = run_one_city("aderm_bere", &output_dir);
        let city = &committed[0];
        assert!(city.documents.apd.is_none());
        assert!(city.documents.draft.is_some());
        assert!(city.documents.docx.is_none());
        assert!(city.documents.docx_error.is_none());
        assert!(
            !output_dir
                .join("cities/aderm_bere")
                .join(DOCX_NAME)
                .exists()
        );
        std::fs::remove_dir_all(&output_dir).ok();
    }

    #[test]
    fn a_committed_run_is_reopened_from_the_workspace_alone() {
        let output_dir = scratch_dir("reopen");
        let (_, committed) = run_one_city("aderm_bere", &output_dir);
        let city = &committed[0];
        let city_dir = output_dir.join("cities").join("aderm_bere");

        // What `solar_native_run_documents` reads: the directory, not a
        // receipt. A run from a previous session resolves the same way.
        let mut files: Vec<(String, u64, String)> = Vec::new();
        for name in [PRODUCT_APD_NAME, PARITY_DRAFT_NAME, DOCX_NAME] {
            let Ok(bytes) = std::fs::read(city_dir.join(name)) else {
                continue;
            };
            files.push((
                name.to_string(),
                bytes.len() as u64,
                ds_solar_contracts::digest::sha256_hex(&bytes),
            ));
        }
        assert_eq!(files.len(), 1, "reopened {files:?}");

        let draft = city.documents.draft.as_ref().expect("the APD draft");
        let reopened = files
            .iter()
            .find(|(name, _, _)| name == PARITY_DRAFT_NAME)
            .expect("the APD draft is on disk");
        assert_eq!(reopened.1, draft.byte_count);
        assert_eq!(reopened.2, draft.content_digest);
        assert_eq!(
            count_charts(&city_dir).expect("chart count"),
            city.documents.chart_count
        );
        std::fs::remove_dir_all(&output_dir).ok();
    }

    #[test]
    fn an_atomic_write_leaves_no_partial_behind_and_replaces_in_place() {
        let directory = scratch_dir("atomic");
        let path = directory.join("apd-fr.md");
        write_atomically(&path, b"first").expect("first write");
        assert_eq!(std::fs::read(&path).expect("read"), b"first");
        write_atomically(&path, b"second").expect("second write");
        assert_eq!(std::fs::read(&path).expect("read"), b"second");
        assert!(!path.with_extension("partial").exists());
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn the_workspace_layout_keeps_roots_and_runs_apart() {
        let workspace = Path::new("/tmp/ds-web-workspace");
        let native = native_root(workspace);
        assert!(native.ends_with(Path::new("solar-rust").join(NATIVE_LAYOUT_VERSION)));
        assert_ne!(
            prepared_path(&native.join("prepared").join("solar"), "aderm_bere"),
            prepared_path(&native.join("prepared").join("other"), "aderm_bere"),
        );
    }
}

#[cfg(all(test, unix))]
mod engine_integrity_tests;
