//! `ds report project export` — individual transformer reports, prints
//! included, from the governed inputs and the installed engine.
//!
//! No Desktop. The native user's audience-fenced
//! named project supplies two things through fixed doors: the Network
//! Reporter input receipt ds-brain mints beside the project configuration
//! (country, the exact settings sheets, the reference snapshot), and each
//! transformer's exact saved layers with their revision. The installed
//! `ds-report` engine produces the artifacts locally — every geospatial and
//! tabular output this run selects, and every explicitly named print output
//! rendered from the project's saved printing setups — and `ds-report-host`
//! stages, proves, places and receipts them. The decisions are the kernel's
//! (`report_export`), the same ones the desktop shell applies, so a receipt
//! written here carries the fingerprint and room digest the publication
//! contract admits against.
//!
//! Independent transformers run under a bounded number of resident engines;
//! rooms are fetched one at a time so the credential and the network stay
//! serial and memory stays bounded. One transformer's failure is one row of
//! the batch receipt, never the end of the batch.
//!
//! `--preview-layout` retains a draft page through the common native file
//! owner. Each page is `ds_report_host::preview::execute_artifact_retained` — the one
//! execution the desktop door reaches too — over the room this command
//! admits from the service answer and the holdings this machine keeps; the
//! answer is `ds_report_host::preview::answer`, so both doors give the same
//! `data` under the same envelope. A preview never seeds, never projects MV,
//! and writes one page with its run receipt per transformer, no batch receipt.
//!
//! `--preview-voltage-drop` branches before every remote read and publication
//! path. It admits one exact held saved analysis and room, then joins the same
//! scoped file-preview owner or renders that saved source into an A4 PDF.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ds_cli_auth::WeakNetwork;
use ds_cli_contract::outcome::{ExitClass, Failure};
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::{
    envelope::Class,
    report::PublicationState,
    report_export::{EngineIdentity, InputReceipt, reportable_transformer},
    report_formats::DesignOutputSelection,
};
use ds_report_artifacts::{VerifiedSidecarArtifact, confined_fs::HeldDirectory};
use ds_report_host::{
    BatchSettings, DEFAULT_RESIDENT_LIMIT, EngineExit, HeldContext, HeldRoom, HeldState, Holdings,
    HostFailure, MAX_RESIDENT_LIMIT, PageRef, PreviewPage, PreviewRefusal, PreviewSettings,
    ReportEngine, RunSettings, TransformerReportInputs, batch_plan, installed_admin_bounds_path,
    run_batch, shared_root, verify_admin_bounds_asset,
};
use ds_sync_runtime::reports::{SealOutcome, SealRequest};
use serde_json::{Value, json};

#[path = "held_export.rs"]
mod held_export;
#[path = "held_preview.rs"]
mod held_preview;

use super::reuse::{self, Decision, OutputPlan, RunScope};
use super::{LANE_ARG, TRANSFORMER_ARG};
use crate::EXPORT_TIMEOUT;

const OUT_DIR_ARG: Arg = Arg::value(
    "out-dir",
    "<path>",
    "Fresh delivery directory; saved A4 preview uses it only as temporary scratch.",
)
.required();
const CONCURRENCY_ARG: Arg = Arg::value(
    "concurrency",
    "<n>",
    "Engines resident at once, 1..64. Default: min(scope, processors, 4).",
);
const ADMIN_BOUNDS_ARG: Arg = Arg::value(
    "admin-bounds",
    "<path>",
    "Rwanda villages asset (.dsab). Default: the installed machine-shared asset.",
);
const SEED_ARG: Arg = Arg::switch(
    "seed",
    "Acquire missing map context (provider cost); name omissions otherwise.",
);
/// Publication is not optional, so there is no `--publish`.
///
/// It was a switch, and every run that forgot it wrote artifacts no queue
/// could see: one operator ended a day with 593 verified reports, 379 MB, on
/// a single PC, indistinguishable from reports that never ran. An export now
/// publishes. The ONLY way to produce nothing publishable is to say so, and
/// the receipt of that run says, in words, that it published nothing — so a
/// receipt can never be mistaken for a publication.
const DRY_RUN_ARG: Arg = Arg::switch(
    "dry-run",
    "Produce local files and publish NOTHING; the receipt says so.",
);
/// A publishing run reuses every non-print output the project already holds
/// from this run's exact input base and engine build (`super::reuse`);
/// `--force` generates the whole policy without consulting that evidence.
const FORCE_ARG: Arg = Arg::switch("force", "Regenerate all outputs; reuse none.");
/// A run that publishes nothing prints from what this machine holds: the
/// inputs, rooms, MV projections and survey forms an earlier connected run
/// kept. It reads from the service only what is not held, once; freshness is
/// checked where the project is opened or selected, never per print. The PDF
/// is always rendered fresh. `--refresh` reads everything again; a
/// publication always reads the service's heads.
/// How old a held survey form a held-first print reuses: any age. Only a
/// form this machine does not hold is read.
pub(super) const SURVEY_HELD_FIRST_AGE_SECONDS: u64 = u32::MAX as u64;
const REFRESH_ARG: Arg = Arg::switch(
    "refresh",
    "Re-read held inputs, rooms and MV models; publishing always does.",
);
const SERVER_STATE_DIR_ARG: Arg = Arg::value(
    "core-state-dir",
    "<absolute-path>",
    "Publication queue root for a publishing run. A dry-run or local layout proof opens no queue and ignores this path; its receipt says so.",
);
/// How the project's survey forms are read from this machine's held copy
/// (`ds survey entries read`'s refresh vocabulary, narrowed to the two a
/// report run means): `auto` refreshes a stale copy, `local` never goes online
/// for survey rows and names every form it does not hold.
const SURVEY_REFRESH_ARG: Arg = Arg::value(
    "survey-refresh",
    "<auto|local>",
    "auto refreshes held survey forms; local reads only what is held.",
)
.default("auto")
.choices(&["auto", "local"]);

/// The staging directory a batch keeps below its output root while engines
/// run; each run's scratch is removed when it ends.
const STAGING_DIRECTORY: &str = ".staging";

const ENGINE_MISSING: Refusal = Refusal {
    code: "reporter_engine_missing",
    when: "`ds-report` is not installed next to `ds`",
    remedy: "install the headless DS package, or set DS_REPORT_BIN to a built ds-report",
};
const CONTRACT_MISMATCH: Refusal = Refusal {
    code: "callee_contract_mismatch",
    when: "`build-info` did not return the engine's release identity",
    remedy: "update `ds` and the reporter to matching releases",
};
const TIMED_OUT: Refusal = Refusal {
    code: "callee_timed_out",
    when: "one transformer's export exceeded the 30-minute bound",
    remedy: "retry the named transformer alone, or investigate the engine",
};
const ENGINE_REFUSED: Refusal = Refusal {
    code: "engine_refused",
    when: "the engine failed before writing a typed result (a batch row carries it)",
    remedy: "read `error.detail.engine` for the engine's own message",
};
const EXPORT_BLOCKED: Refusal = Refusal {
    code: "export_blocked",
    when: "the engine produced NO format for a transformer (a batch row carries its blockers)",
    remedy: "read `error.detail.blockers`, fix the named input, and re-run that transformer",
};
pub(crate) const INPUTS_INVALID: Refusal = Refusal {
    code: "report_inputs_invalid",
    when: "the input receipt, a transformer's saved layers or the output policy cannot be run as given",
    remedy: "run `ds report project settings`; it names the missing input and the repair",
};
const TRANSFORMER_NATURE_UNAVAILABLE: Refusal = Refusal {
    code: "report_transformer_nature_unavailable",
    when: "a voltage-drop output is selected but the current project transformer_nature projection could not be verified",
    remedy: "retry while connected and signed in to the named project; do not publish a voltage-drop report without current governed tags",
};
pub(crate) const STAGING_FAILED: Refusal = Refusal {
    code: "report_staging_failed",
    when: "the output directory or its staging area could not be prepared",
    remedy: "check that --out-dir is writable and has space",
};
const OUTPUT_EXISTS: Refusal = Refusal {
    code: "report_output_exists",
    when: "--out-dir already holds a batch receipt or that transformer's folder",
    remedy: "choose a fresh --out-dir; a report is never overwritten",
};
pub(crate) const RESULT_INVALID: Refusal = Refusal {
    code: "report_result_invalid",
    when: "the engine's result or the bytes it declared are not this run's answer",
    remedy: "retry; if it persists, update `ds` and the reporter to matching releases",
};
const IDENTITY_CHANGED: Refusal = Refusal {
    code: "report_engine_identity_changed",
    when: "`ds-report` was replaced while a report was running",
    remedy: "retry once the installation is stable",
};
const ADMIN_BOUNDS: Refusal = Refusal {
    code: "admin_bounds_unavailable",
    when: "the Rwanda villages asset is not installed for the project's dataset snapshot",
    remedy: "install the governed Rwanda reference with ds workstation setup, or pass --admin-bounds",
};
const CONCURRENCY: Refusal = Refusal {
    code: "invalid_concurrency",
    when: "--concurrency is not a whole number from 1 through 64",
    remedy: "pass --concurrency between 1 and 64, or omit it",
};
const BATCH_EMPTY: Refusal = Refusal {
    code: "report_batch_empty",
    when: "the named project has no active transformer to report",
    remedy: "save a transformer first; `ds report project scope` lists the inventory",
};
const BATCH_FAILED: Refusal = Refusal {
    code: "report_batch_failed",
    when: "no transformer report completed; each result carries its failure",
    remedy: "read detail.results and fix the named inputs",
};
const NOT_ACTIVE: Refusal = Refusal {
    code: "transformer_not_active",
    when: "a named transformer is retired, deleted or missing",
    remedy: "`ds report project scope` shows the lifecycle; restore or drop the name",
};
pub(super) const PUBLISH_LOCAL_ONLY: Refusal = Refusal {
    code: "report_publish_local_only",
    when: "a development reporter build produced artifacts, and a development build may not publish",
    remedy: "run a release reporter build, or pass --dry-run to accept local-only files",
};
pub(super) const PUBLISH_SCOPE_CHANGED: Refusal = Refusal {
    code: "report_publish_scope_changed",
    when: "the identity, lane, audience, project or credential changed before sealing",
    remedy: "repeat the export under the current native account and project",
};
pub(super) const PUBLISH_ROOT: Refusal = Refusal {
    code: "report_publish_root_invalid",
    when: "the Server state root is unavailable, relative, or cannot hold a sealed publication",
    remedy: "use Server's default state root, or the same absolute --core-state-dir as ds core serve",
};
const CONTEXT_UNSUPPORTED: Refusal = Refusal {
    code: "print_context_unsupported",
    when: "a setup names a local-layer source with no complete headless capture (batch row)",
    remedy: "provide the exact layer through its owning project source before headless printing",
};
const CONTEXT_INVALID: Refusal = Refusal {
    code: "print_context_invalid",
    when: "the print context exceeds the engine's bounds or the holdings are unreadable (batch row)",
    remedy: "narrow the setup's context buffers, or repair the geographic data root",
};
const CONTEXT_CATALOG: Refusal = Refusal {
    code: "catalog_unavailable",
    when: "the reference catalogue could not be read; catalogue layers print from held rooms",
    remedy: "retry when connected",
};
const CONTEXT_BUNDLE: Refusal = Refusal {
    code: "reference_bundle_unavailable",
    when: "--seed needed a national bundle that is unpublished or failed to install (batch row)",
    remedy: "publish the dataset's bundle, then print again with --seed",
};
const CONTEXT_ACQUISITION: Refusal = Refusal {
    code: "project_dataset_acquisition_failed",
    when: "--seed acquired context the room refused (batch row)",
    remedy: "`ds data project-cache status` shows the last error",
};
const CONTEXT_LOCAL_SOURCE: Refusal = Refusal {
    code: "project_dataset_local_source_required",
    when: "an explicitly selected geographic producer needs a pinned local source",
    remedy: "publish the exact local-source manifest through ds data project-cache; preview may report the missing held context",
};
const PREVIEW_ARTIFACT_UNAVAILABLE: Refusal = Refusal {
    code: "print_preview_artifact_unavailable",
    when: "the managed preview manifest, scope, path or exact bytes cannot be admitted",
    remedy: "retain written artifacts and request the exact native preview again",
};
const PREVIEW_ARTIFACT_EXPIRED: Refusal = Refusal {
    code: "print_preview_artifact_expired",
    when: "the native preview file has passed its admitted expiry",
    remedy: "request its exact preview again; no read automatically renders or acquires",
};
pub(crate) const PREVIEW_JOB_CAPACITY: Refusal = Refusal {
    code: "print_preview_job_capacity",
    when: "two native MV preview jobs are active",
    remedy: "Consume a ready page or wait for one native MV job to finish.",
};
const PREVIEW_METADATA_LIMIT: Refusal = Refusal {
    code: "print_preview_metadata_limit",
    when: "preview diagnostics exceed the bounded response metadata",
    remedy: "read the native disk manifest named in detail",
};
const CONTEXT_TOO_LARGE: Refusal = Refusal {
    code: "print_context_too_large",
    when: "a preview's held context exceeds the engine's bounds",
    remedy: "narrow the draft's context buffers, or drop a context layer",
};
const NEIGHBOR_POINTS_UNAVAILABLE: Refusal = Refusal {
    code: "neighbor_transformer_points_unavailable",
    when: "a sheet selects neighbor transformer points or circuit context but an active room is missing, changed or has no verified point",
    remedy: "refresh the active transformer rooms for this project, then retry the sheet",
};
const HOLDINGS_STORE: Refusal = Refusal {
    code: "project_dataset_store_failed",
    when: "a preview could not read the geographic holdings this machine keeps",
    remedy: "`ds data project-cache status`; repair the geographic data root",
};

pub(super) const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "report_held_export_invalid",
        when: "Closed held export inputs, selected GIS formats or controls fail admission",
        remedy: "Preserve the captured room; provide an explicit SHP/KMZ/XLSX selection and the original report receipt",
    },
    Refusal {
        code: "report_held_export_source_unsaved",
        when: "Held room has no clean governed saved revision",
        remedy: "Save through the design owner before exporting; reporting never saves a design",
    },
    Refusal {
        code: "report_held_export_source_changed",
        when: "Held version or content differs from the saved source",
        remedy: "Explicitly refresh or save the held room through its design owner and retry",
    },
    Refusal {
        code: "report_held_export_configuration_changed",
        when: "Held receipt differs from admitted project configuration",
        remedy: "Explicitly refresh the project configuration and retry with its original receipt",
    },
    held_export::STATUS_NOT_HELD,
    ds_cli_auth::config_document::NOT_HELD,
    Refusal {
        code: "report_pending_outputs_require_sync",
        when: "A partial export would replace a publication with other pending outputs",
        remedy: "Sync the previous report publication before exporting this selection",
    },
    Refusal {
        code: "report_held_preview_invalid",
        when: "The closed held-preview payload, original receipt/media/context or controls fail admission",
        remedy: "Preserve the held snapshot and correct the named field; never reacquire or substitute current report rows",
    },
    Refusal {
        code: "project_context_changed",
        when: "The held preview project, Core lane, audience or explicit CRS scope differs from its captured owner",
        remedy: "Retain completed artifacts and capture the intended owner-window context again",
    },
    super::mv_context::NETWORK_ENGINE_MISSING,
    super::mv_context::NETWORK_ENGINE_INCOMPATIBLE,
    super::vd_preview::MISSING,
    super::vd_preview::STALE,
    super::NATIVE_PROFILE,
    super::NATIVE_PROFILE_DIGEST,
    super::NATIVE_PROFILE_UNSAFE,
    super::HEADLESS_SIGNED_OUT,
    super::RECONCILIATION_REQUIRED,
    super::NATIVE_STATE_UNSAFE,
    super::NATIVE_STATE_UNAVAILABLE,
    super::NATIVE_STATE_PROTECTION,
    super::NATIVE_STATE_ROOT,
    super::NATIVE_STATE_CONFLICT,
    super::NATIVE_CLEANUP,
    super::AUTH_CONTEXT_MISMATCH,
    super::AUTH_INPUT,
    super::AUTH_REJECTED,
    super::AUTH_REVOKED,
    super::AUTH_IDENTITY_MISMATCH,
    super::AUTH_TRANSIENT,
    ds_cli_auth::DEVICE_AUTH_TRANSIENT_REFUSAL,
    super::AUTH_UNREADABLE,
    super::NOT_FOUND,
    super::INVALID_SCOPE,
    super::RESERVED_IDENTITY,
    ENGINE_MISSING,
    crate::ENGINE_TASK_UNKNOWN,
    CONTRACT_MISMATCH,
    TIMED_OUT,
    ENGINE_REFUSED,
    EXPORT_BLOCKED,
    INPUTS_INVALID,
    TRANSFORMER_NATURE_UNAVAILABLE,
    STAGING_FAILED,
    OUTPUT_EXISTS,
    RESULT_INVALID,
    IDENTITY_CHANGED,
    ADMIN_BOUNDS,
    CONCURRENCY,
    BATCH_EMPTY,
    BATCH_FAILED,
    NOT_ACTIVE,
    PUBLISH_LOCAL_ONLY,
    PUBLISH_SCOPE_CHANGED,
    PUBLISH_ROOT,
    CONTEXT_UNSUPPORTED,
    CONTEXT_INVALID,
    CONTEXT_CATALOG,
    CONTEXT_BUNDLE,
    CONTEXT_ACQUISITION,
    CONTEXT_LOCAL_SOURCE,
    PREVIEW_ARTIFACT_UNAVAILABLE,
    PREVIEW_ARTIFACT_EXPIRED,
    PREVIEW_JOB_CAPACITY,
    PREVIEW_METADATA_LIMIT,
    CONTEXT_TOO_LARGE,
    NEIGHBOR_POINTS_UNAVAILABLE,
    HOLDINGS_STORE,
    ds_cli_auth::DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL,
    super::hold::INPUTS_NOT_HELD,
    super::hold::ROOM_NOT_HELD,
    PINNED_ROOM_INVALID,
];

pub(super) const PINNED_ROOM_INVALID: Refusal = Refusal {
    code: "pinned_room_invalid",
    when: "a --pin-room file is unreadable, repeats, names a transformer not printed, or its digest does not match its layers",
    remedy: "pin the exact held room a delivery receipt names: its transformer, revision and content digest",
};

pub static COMMAND: Command = Command {
    id: "report.project.export",
    path: &["report", "project", "export"],
    contract: 2,
    summary: "Export all transformer reports and maps headlessly in parallel.",
    purpose: "Defaults: SHP/KMZ/XLSX, plus saved voltage-drop JSON when the output selection exports it. Prints regenerate; data may reuse.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        Arg::value(
            "held-preview",
            "<json-file>",
            "Closed ds.report-held-preview/v1 exact caller-held transformer preview; no config/room/style acquisition or publication. Requires fresh absolute out-dir for private scratch.",
        ),
        Arg::value(
            "held-export",
            "<json-file>",
            "Closed ds.report-held-export/v1 SHP/KMZ/XLSX export pinned to held status and configuration only; queues artifacts via Core Sync. Dirty rooms refuse.",
        ),
        OUT_DIR_ARG,
        CONCURRENCY_ARG,
        ADMIN_BOUNDS_ARG,
        Arg::value(
            "selection",
            "<json-file>",
            "ds.design-output-selection/v1 run matrix; omitted uses the defaults.",
        ),
        Arg::repeated(
            "print-layout",
            "<json-file>",
            "Local layout proof; preserves inputs and paper, records digests, cannot publish.",
        ),
        Arg::value(
            "preview-layout",
            "<json-file>",
            "One outlined-text SVG per transformer; no publication or --print-layout.",
        ),
        Arg::value(
            "preview-retention",
            "<24h|7d>",
            "SVG/A4 lifetime; requires a preview mode, default 24h.",
        )
        .choices(&["24h", "7d"]),
        Arg::switch(
            "preview-voltage-drop",
            "Fixed A4 from exact held saved analysis; disk manifest only, no publication or remote reads.",
        ),
        Arg::value(
            "context-vectors",
            "<dir>",
            "Verified data.city-vectors map context for this batch.",
        ),
        Arg::repeated(
            "pin-room",
            "<room.json>",
            "Print one exact saved revision from a held room file instead of the head; its content digest must match its layers and is receipted.",
        ),
        SEED_ARG,
        DRY_RUN_ARG,
        FORCE_ARG,
        REFRESH_ARG,
        SERVER_STATE_DIR_ARG,
        SURVEY_REFRESH_ARG,
        LANE_ARG,
        super::PROJECT_ARG,
    ],
    output: "Lane/project, engine/receipt, results/artifacts and failed_formats[{format,output_id,remedy,partial_formats,overflow/panels/row_mm}]; reuse.reused[{output_id,work_id,sha256}], generate reasons, or every output was reused. A4 preview: common PDF file manifest and preview_cache lifetime/reused receipt; no publication.",
    examples: &[
        Example {
            command: "ds report project export --out-dir ./reports --output json --project <exact-id>",
            note: "Every active transformer, published; `.data.results[]` says what each did.",
            runnable: false,
        },
        Example {
            command: "ds report project export --transformer tx_a --out-dir ./proof --dry-run --core-state-dir /isolated/server-state --output json --project <exact-id>",
            note: "Local proof; state-dir ignored and no queue opened.",
            runnable: false,
        },
        Example {
            command: "ds report project export --transformer tx_a --preview-layout ./draft.json --out-dir ./preview --output json --project <exact-id>",
            note: "One SVG sheet; `.data.preview` names the output.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &[],
    requires: Requires::Server,
    availability,
};

/// Native identity supplies the lane; engine admission belongs to the request.
fn availability() -> Availability {
    // The declared command has no selected input lane during discovery.
    // Resolve that captured native lane at consumption, without a probe here;
    // discovery only asks whether any lane holds a Reporter.
    crate::reporter_availability()
}

/// The Reporter as this host's report engine: every method is one closed
/// task of the Kernel's Reporter client, over typed request/result paths.
/// This adapter names no subcommand and starts no process itself.
pub(crate) struct CliEngine;

impl CliEngine {
    /// The caller has already captured its native account/profile context.
    pub(crate) fn capture(lane: &str) -> Result<CapturedCliEngine, HostFailure> {
        crate::Reporter::capture_for_lane(lane)
            .map(CapturedCliEngine)
            .map_err(failure_to_host)
    }
}

pub(crate) struct CapturedCliEngine(crate::Reporter);

impl CapturedCliEngine {
    fn render(
        &self,
        task: crate::ReporterTask,
        request: &Path,
        result: &Path,
    ) -> Result<EngineExit, HostFailure> {
        let completed = self
            .0
            .run(task, request, result, EXPORT_TIMEOUT)
            .map_err(failure_to_host)?;
        Ok(EngineExit {
            succeeded: completed.succeeded(),
            summary: bounded_summary(completed.stderr(), completed.stdout()),
        })
    }
}

impl ReportEngine for CapturedCliEngine {
    fn build_info(&self) -> Result<Value, HostFailure> {
        self.0.build_info().map_err(failure_to_host)
    }
    fn export_transformer_report(
        &self,
        request: &Path,
        result: &Path,
    ) -> Result<EngineExit, HostFailure> {
        self.render(
            crate::ReporterTask::ExportTransformerReport,
            request,
            result,
        )
    }
    fn render_print_layout(
        &self,
        request: &Path,
        result: &Path,
    ) -> Result<EngineExit, HostFailure> {
        self.render(crate::ReporterTask::RenderPrintLayout, request, result)
    }
    fn render_grid_plan_profile(
        &self,
        request: &Path,
        result: &Path,
    ) -> Result<EngineExit, HostFailure> {
        self.render(crate::ReporterTask::RenderGridPlanProfile, request, result)
    }
    fn export_combined_transformer_report(
        &self,
        request: &Path,
        result: &Path,
    ) -> Result<EngineExit, HostFailure> {
        self.render(
            crate::ReporterTask::ExportCombinedTransformerReport,
            request,
            result,
        )
    }
    fn discover_combined_media_references(
        &self,
        request: &Path,
        result: &Path,
    ) -> Result<EngineExit, HostFailure> {
        self.render(
            crate::ReporterTask::DiscoverCombinedMediaReferences,
            request,
            result,
        )
    }
    fn launch_grid_preview(
        &self,
        request: &Path,
        result: &Path,
        _input_sha256: &str,
    ) -> Result<(), HostFailure> {
        // One fixed task over host-owned durable paths. Reporter owns its
        // process lock and failure journal, so CLI exit releases only a viewer.
        self.0
            .launch(crate::ReporterTask::RenderGridPlanProfile, request, result)
            .map_err(failure_to_host)
    }
}
/// Exact saved analysis admitted for one room; no producer work occurs here.
#[derive(Debug, Clone)]
enum BatchAnalysis {
    Ready { document: Vec<u8>, sha256: String },
    Missing,
    Stale,
}

fn fenced_batch_analysis(
    room: &ds_project_data::room_hold::Room,
    transformer: &str,
    version: Option<u64>,
    content_digest: Option<&str>,
    metadata: Option<&Value>,
    document: Option<&[u8]>,
) -> Result<BatchAnalysis, HostFailure> {
    if transformer != room.transformer {
        return Err(HostFailure::new(
            INPUTS_INVALID.code,
            "saved analysis belongs to another transformer",
        ));
    }
    if version.and_then(|v| i64::try_from(v).ok()) != room.version
        || content_digest != room.content_digest.as_deref()
    {
        return Ok(BatchAnalysis::Stale);
    }
    match document {
        Some(document) => {
            let sha256 = ds_command_kernel::report_export::sha256_hex(document);
            if metadata.and_then(|m| m["analysis_sha256"].as_str()) != Some(sha256.as_str()) {
                return Err(HostFailure::new(
                    INPUTS_INVALID.code,
                    "saved analysis bytes differ from the native read pin",
                ));
            }
            Ok(BatchAnalysis::Ready {
                document: document.to_vec(),
                sha256,
            })
        }
        None if metadata
            .is_some_and(|m| matches!(m["state"].as_str(), Some("stale" | "needs_reprocess"))) =>
        {
            Ok(BatchAnalysis::Stale)
        }
        None => Ok(BatchAnalysis::Missing),
    }
}

fn retain_batch_analysis(
    inputs: &mut super::hold::Inputs,
    scope: &ds_command_kernel::report_export::held::Scope,
    room: &ds_project_data::room_hold::Room,
    analysis: &BatchAnalysis,
) -> Result<(), HostFailure> {
    if let BatchAnalysis::Ready { document, sha256 } = analysis {
        let capture = ds_command_kernel::report_export::held::SavedAnalysis {
            scope: scope.clone(),
            transformer: room.transformer.clone(),
            version: room.version.unwrap_or_default(),
            content_digest: room.content_digest.clone().unwrap_or_default(),
            document: String::from_utf8(document.clone())
                .map_err(|e| HostFailure::new(INPUTS_INVALID.code, e.to_string()))?,
            sha256: sha256.clone(),
        };
        capture
            .validate(
                scope,
                &room.transformer,
                capture.version,
                &capture.content_digest,
            )
            .map_err(|e| HostFailure::new(INPUTS_INVALID.code, e))?;
        inputs
            .saved_analyses
            .insert(room.transformer.clone(), capture);
    } else {
        // A service-confirmed missing/stale result invalidates the old copy.
        inputs.saved_analyses.remove(&room.transformer);
    }
    Ok(())
}
fn held_batch_analysis(
    inputs: &super::hold::Inputs,
    scope: &ds_command_kernel::report_export::held::Scope,
    room: &ds_project_data::room_hold::Room,
) -> Result<BatchAnalysis, HostFailure> {
    let Some(capture) = inputs.saved_analyses.get(&room.transformer) else {
        return Ok(BatchAnalysis::Missing);
    };
    capture
        .validate(
            scope,
            &room.transformer,
            room.version.unwrap_or_default(),
            room.content_digest.as_deref().unwrap_or_default(),
        )
        .map_err(|e| HostFailure::new(INPUTS_INVALID.code, e))?;
    Ok(BatchAnalysis::Ready {
        document: capture.document.as_bytes().to_vec(),
        sha256: capture.sha256.clone(),
    })
}

fn batch_analysis_refusal(failure: &Failure) -> Option<BatchAnalysis> {
    let detail = failure.detail_value()?;
    if detail["http_status"] != 409 {
        return None;
    }
    // ds-client-core carries the data route's codes in the lowercase refusal
    // vocabulary; older receipts named them as the server spells them.
    match detail["service_code"]
        .as_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "transformer_analysis_missing" => Some(BatchAnalysis::Missing),
        "transformer_analysis_stale" => Some(BatchAnalysis::Stale),
        _ => None,
    }
}

/// Adds the CLI-owned saved-analysis input at the existing engine boundary.
/// The host owns the room, request and scratch lifetime; its siblings stay read-only.
struct SavedAnalysisEngine<'a> {
    analyses: &'a std::sync::Mutex<BTreeMap<String, BatchAnalysis>>,
    delegate: &'a dyn ReportEngine,
    report_format: &'static str,
}

fn stage_batch_analysis(
    request_path: &Path,
    analysis: &BatchAnalysis,
    report_format: &str,
) -> Result<(), HostFailure> {
    let invalid = |error: String| HostFailure::new(INPUTS_INVALID.code, error);
    let mut request: Value =
        serde_json::from_slice(&std::fs::read(request_path).map_err(|e| invalid(e.to_string()))?)
            .map_err(|e| invalid(e.to_string()))?;
    match analysis {
        BatchAnalysis::Ready { document, sha256 } => {
            if ds_command_kernel::report_export::sha256_hex(document) != *sha256 {
                return Err(invalid("saved analysis bytes differ from their pin".into()));
            }
            let source = request_path.with_file_name("saved-voltage-drop-analysis.json");
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&source)
                .map_err(|e| HostFailure::new(STAGING_FAILED.code, e.to_string()))?;
            use std::io::Write;
            file.write_all(document)
                .map_err(|e| HostFailure::new(STAGING_FAILED.code, e.to_string()))?;
            request["voltage_drop_result"] =
                json!({"path":source, "sha256":sha256, "report_format":report_format});
        }
        BatchAnalysis::Missing => request["voltage_drop_refusal"] = json!("missing"),
        BatchAnalysis::Stale => request["voltage_drop_refusal"] = json!("stale"),
    }
    std::fs::write(
        request_path,
        serde_json::to_vec(&request).map_err(|e| invalid(e.to_string()))?,
    )
    .map_err(|e| HostFailure::new(STAGING_FAILED.code, e.to_string()))
}

impl ReportEngine for SavedAnalysisEngine<'_> {
    fn build_info(&self) -> Result<Value, HostFailure> {
        self.delegate.build_info()
    }

    fn export_transformer_report(
        &self,
        request: &Path,
        result: &Path,
    ) -> Result<EngineExit, HostFailure> {
        let document: Value = serde_json::from_slice(
            &std::fs::read(request)
                .map_err(|e| HostFailure::new(INPUTS_INVALID.code, e.to_string()))?,
        )
        .map_err(|e| HostFailure::new(INPUTS_INVALID.code, e.to_string()))?;
        let name = document["transformer"].as_str().ok_or_else(|| {
            HostFailure::new(INPUTS_INVALID.code, "report request lacks a transformer")
        })?;
        let analysis = self
            .analyses
            .lock()
            .map_err(|_| HostFailure::new(INPUTS_INVALID.code, "saved analysis lock poisoned"))?
            .remove(name);
        if let Some(analysis) = analysis {
            stage_batch_analysis(request, &analysis, self.report_format)?;
        }
        self.delegate.export_transformer_report(request, result)
    }
}

/// The held A4 preview uses this same fixed saved-result injection; it never
/// invokes the analysis producer, nature projection or a cloud acquisition.
pub(super) fn render_saved_a4_preview(
    engine: &dyn ReportEngine,
    settings: &ds_report_host::RunSettings<'_>,
    room: &ds_report_host::TransformerReportInputs,
    saved: &ds_command_kernel::report_export::held::SavedAnalysis,
    format: &'static str,
) -> Result<ds_report_host::RunOutcome, HostFailure> {
    let analyses = std::sync::Mutex::new(BTreeMap::from([(
        room.transformer.clone(),
        BatchAnalysis::Ready {
            document: saved.document.as_bytes().to_vec(),
            sha256: saved.sha256.clone(),
        },
    )]));
    let captured = SavedAnalysisEngine {
        analyses: &analyses,
        delegate: engine,
        report_format: format,
    };
    ds_report_host::run_transformer_report(&captured, settings, room)
}

fn bounded_summary(stderr: &str, stdout: &str) -> String {
    let source = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    source
        .chars()
        .filter(|character| !character.is_control() || *character == '\n')
        .take(2_000)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Retry `op` under the kernel's weak-network schedule
/// ([`WeakNetwork::FIELD`], the one sign-in, token refresh and dataset
/// downloads share, so a batch of eighty sheets does not lose seventy rows to
/// one dropped second) while its refusal is retryable (`unavailable` or
/// `conflict` — the world, not the request, has to change); any other refusal,
/// and the last retryable one, is returned as it came.
pub(super) fn with_weak_network<T>(
    weak_network: &WeakNetwork,
    op: impl FnMut() -> Result<T, Failure>,
) -> Result<T, Failure> {
    weak_network
        .run(
            op,
            |outcome| matches!(outcome, Err(failure) if failure.class().retryable()),
        )
        .0
}

fn failure_to_host(failure: Failure) -> HostFailure {
    HostFailure {
        code: failure.code().to_string(),
        message: failure.message().to_string(),
        detail: failure.detail_value().cloned(),
    }
}

/// A host failure that ends the whole command, in the CLI's own classes.
/// Every arm is a literal so the code is documented above.
pub(crate) fn host_failure(failure: HostFailure) -> Failure {
    let message = failure.message.clone();
    let mapped = match failure.code.as_str() {
        "report_inputs_invalid" => {
            Failure::invalid("report_inputs_invalid", message).remedy(INPUTS_INVALID.remedy)
        }
        "report_staging_failed" => {
            Failure::failed("report_staging_failed", message).remedy(STAGING_FAILED.remedy)
        }
        "report_output_exists" => {
            Failure::conflict("report_output_exists", message).remedy(OUTPUT_EXISTS.remedy)
        }
        "report_result_invalid" => {
            Failure::failed("report_result_invalid", message).remedy(RESULT_INVALID.remedy)
        }
        "report_engine_identity_changed" => {
            Failure::failed("report_engine_identity_changed", message)
                .remedy(IDENTITY_CHANGED.remedy)
        }
        "admin_bounds_unavailable" => {
            Failure::unavailable("admin_bounds_unavailable", message).remedy(ADMIN_BOUNDS.remedy)
        }
        "export_blocked" => {
            Failure::failed("export_blocked", message).remedy(EXPORT_BLOCKED.remedy)
        }
        "engine_refused" => {
            Failure::failed("engine_refused", message).remedy(ENGINE_REFUSED.remedy)
        }
        "callee_contract_mismatch" => {
            Failure::failed("callee_contract_mismatch", message).remedy(CONTRACT_MISMATCH.remedy)
        }
        "callee_timed_out" => Failure::failed("callee_timed_out", message).remedy(TIMED_OUT.remedy),
        "reporter_engine_missing" => {
            Failure::unavailable("reporter_engine_missing", message).remedy(ENGINE_MISSING.remedy)
        }
        "unknown_task" => {
            Failure::unavailable("unknown_task", message).remedy(crate::ENGINE_TASK_UNKNOWN.remedy)
        }
        "print_context_unsupported" => Failure::invalid("print_context_unsupported", message)
            .remedy(CONTEXT_UNSUPPORTED.remedy),
        "print_context_invalid" => {
            Failure::failed("print_context_invalid", message).remedy(CONTEXT_INVALID.remedy)
        }
        "reference_bundle_unavailable" => {
            Failure::unavailable("reference_bundle_unavailable", message)
                .remedy(CONTEXT_BUNDLE.remedy)
        }
        "data_distribution_unavailable" => {
            Failure::unavailable("data_distribution_unavailable", message)
                .remedy(ds_cli_auth::DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL.remedy)
        }
        "project_dataset_acquisition_failed" => {
            Failure::failed("project_dataset_acquisition_failed", message)
                .remedy(CONTEXT_ACQUISITION.remedy)
        }
        "project_dataset_local_source_required" => {
            Failure::unavailable(CONTEXT_LOCAL_SOURCE.code, message)
                .remedy(CONTEXT_LOCAL_SOURCE.remedy)
        }
        "print_preview_artifact_unavailable" => {
            Failure::failed(PREVIEW_ARTIFACT_UNAVAILABLE.code, message)
                .remedy(PREVIEW_ARTIFACT_UNAVAILABLE.remedy)
        }
        "print_preview_artifact_expired" => {
            Failure::unavailable(PREVIEW_ARTIFACT_EXPIRED.code, message)
                .remedy(PREVIEW_ARTIFACT_EXPIRED.remedy)
        }
        "print_preview_job_capacity" => Failure::unavailable(PREVIEW_JOB_CAPACITY.code, message)
            .remedy(PREVIEW_JOB_CAPACITY.remedy),
        "print_preview_metadata_limit" => Failure::failed(PREVIEW_METADATA_LIMIT.code, message)
            .remedy(PREVIEW_METADATA_LIMIT.remedy),
        "catalog_unavailable" => {
            Failure::unavailable("catalog_unavailable", message).remedy(CONTEXT_CATALOG.remedy)
        }
        _ => Failure::internal("report_result_invalid", message),
    };
    match failure.detail {
        Some(detail) => mapped.detail(detail),
        None => mapped,
    }
}

/// A preview refusal that ends the command, classified as the desktop door
/// classifies it: the class is `ds_report_host::CLASS_TABLE`'s, the code and
/// sentence travel unchanged, the detail is the kernel's `{message_key,
/// params}` or the host failure's own, and the remedy is the one this command
/// documents for the code when it documents it. The code is not a literal
/// here on purpose — it is the kernel's, mapped to a class, never renamed.
fn preview_failure(refusal: PreviewRefusal) -> Failure {
    let class = match refusal.class() {
        Class::Internal => ExitClass::Internal,
        Class::InvalidInput => ExitClass::InvalidInput,
        Class::Unavailable => ExitClass::Unavailable,
        Class::Unauthorized => ExitClass::Unauthorized,
        Class::Conflict => ExitClass::Conflict,
        Class::Failed => ExitClass::Failed,
    };
    let code = refusal.code().to_string();
    let failure = Failure::new(class, code.as_str(), refusal.message());
    let failure = match REFUSALS.iter().find(|documented| documented.code == code) {
        Some(documented) => failure.remedy(documented.remedy),
        None => failure,
    };
    match refusal.detail() {
        Some(detail) => failure.detail(detail),
        None => failure,
    }
}

fn concurrency_limit(inputs: &Inputs) -> Result<usize, Failure> {
    let Some(raw) = inputs.value("concurrency") else {
        return Ok(DEFAULT_RESIDENT_LIMIT);
    };
    raw.trim()
        .parse::<usize>()
        .ok()
        .filter(|value| (1..=MAX_RESIDENT_LIMIT).contains(value))
        .ok_or_else(|| {
            Failure::invalid(
                "invalid_concurrency",
                format!("--concurrency {raw:?} is not a whole number from 1 through {MAX_RESIDENT_LIMIT}"),
            )
            .remedy(CONCURRENCY.remedy)
        })
}

/// Local refusal pass that must run before a project report touch restores
/// credentials or reconciles remote report state.
pub fn preflight(inputs: &Inputs) -> Result<(), Failure> {
    if inputs.value("held-export").is_some() {
        return held_export::preflight(inputs);
    }
    if inputs.value("held-preview").is_some() {
        return held_preview::preflight(inputs);
    }
    super::transformer_set(inputs)?;
    concurrency_limit(inputs)?;
    run_output_selection(inputs)?;
    Ok(())
}

fn run_output_selection(inputs: &Inputs) -> Result<DesignOutputSelection, Failure> {
    let explicit = inputs
        .value("selection")
        .map(|path| {
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .and_then(|file| file.take(256 * 1024 + 1).read_to_end(&mut bytes))
                .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e.to_string()))?;
            if bytes.len() > 256 * 1024 {
                return Err(Failure::invalid(
                    INPUTS_INVALID.code,
                    "selection exceeds 256 KiB",
                ));
            }
            serde_json::from_slice::<DesignOutputSelection>(&bytes)
                .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e.to_string()))
        })
        .transpose()?;
    // Without an explicit matrix the run names no print, so nothing printable
    // is acquired; `run_outputs` resolves its exact outputs from the project's
    // sealed settings once they are read.
    let defaults = explicit.is_none().then(|| {
        ds_command_kernel::report_formats::INVOCATION_DATA_DEFAULTS
            .iter()
            .map(|s| (*s).to_owned())
            .collect()
    });
    ds_command_kernel::report_formats::normalize_output_selection(explicit, defaults, None)
        .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e).remedy(INPUTS_INVALID.remedy))
}

/// The run's outputs: an explicit `--selection` exactly, otherwise the
/// kernel's invocation defaults over the project's sealed settings — SHP, KMZ
/// and XLSX, plus the saved voltage-drop JSON only when the project's output
/// selection exports it.
fn run_outputs(
    inputs: &Inputs,
    receipt: &InputReceipt,
    requested: DesignOutputSelection,
) -> Result<DesignOutputSelection, Failure> {
    if inputs.value("selection").is_some() {
        return Ok(requested);
    }
    let invalid =
        |e: String| Failure::invalid(INPUTS_INVALID.code, e).remedy(INPUTS_INVALID.remedy);
    ds_command_kernel::report_formats::invocation_selection(
        &receipt.sheets().map_err(invalid)?,
        None,
    )
    .map_err(invalid)
}

pub(super) fn require_same_context(
    expected_identity: &ds_cli_auth::ProviderIdentity,
    expected_project: &str,
    actual_identity: &ds_cli_auth::ProviderIdentity,
    actual_project: &str,
) -> Result<(), HostFailure> {
    if expected_identity != actual_identity || expected_project != actual_project {
        return Err(HostFailure::new(
            INPUTS_INVALID.code,
            "account, deployment audience or named project changed while fetching report inputs; start a new batch",
        ));
    }
    Ok(())
}

/// The report queue is a child of the exact Server state directory. Keeping
/// this resolver in `ds-compute-runtime` lets Server derive its database and
/// this command derive the adjacent sealed-artifact root without a CLI crate
/// dependency cycle or a second XDG interpretation.
pub fn server_report_artifacts_root(
    lane: &str,
    server_state_dir: Option<&Path>,
) -> Result<PathBuf, Failure> {
    ds_compute_runtime::server_state_directory(lane, server_state_dir)
        .map(|state| state.join("report-artifacts"))
        .map_err(|error| Failure::invalid(PUBLISH_ROOT.code, error).remedy(PUBLISH_ROOT.remedy))
}

fn receipt_text<'a>(receipt: &'a Value, field: &str) -> Result<&'a str, Failure> {
    receipt
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Failure::failed(
                PUBLISH_ROOT.code,
                format!("verified report receipt lacks {field}"),
            )
            .remedy(PUBLISH_ROOT.remedy)
        })
}

fn receipt_revision(receipt: &Value) -> Result<i64, Failure> {
    receipt
        .get("transformer_revision")
        .and_then(Value::as_i64)
        .filter(|revision| *revision > 0)
        .ok_or_else(|| {
            Failure::failed(
                PUBLISH_ROOT.code,
                "verified report receipt lacks a positive transformer revision",
            )
            .remedy(PUBLISH_ROOT.remedy)
        })
}

/// The one publication queue a seal enters: the lane's sync store (the
/// queue) under this machine's fence, and the artifact root beside it (the
/// bytes). Resolved exactly as the Server resolves its own, so an export, the
/// pump and `ds report outbox` never disagree about which queue they mean.
pub struct PublicationQueue {
    pub root: PathBuf,
    pub store: ds_sync_runtime::SharedStore,
    pub fence: ds_command_kernel::sync_store::Fence,
}

impl PublicationQueue {
    pub fn open(lane: &str, server_state_dir: Option<&Path>) -> Result<Self, Failure> {
        let state = ds_compute_runtime::server_state_directory(lane, server_state_dir).map_err(
            |error| Failure::invalid(PUBLISH_ROOT.code, error).remedy(PUBLISH_ROOT.remedy),
        )?;
        let store = ds_sync_runtime::open_store(&state.join("store.sqlite")).map_err(|error| {
            Failure::failed(PUBLISH_ROOT.code, error).remedy(PUBLISH_ROOT.remedy)
        })?;
        Ok(Self {
            root: state.join("report-artifacts"),
            store,
            fence: crate::outbox::fence(lane)?,
        })
    }

    /// A queue over any store and fence, for tests and for hosts that hold
    /// their own.
    pub fn at(
        root: PathBuf,
        store: ds_sync_runtime::SharedStore,
        fence: ds_command_kernel::sync_store::Fence,
    ) -> Self {
        Self { root, store, fence }
    }
}

/// What sealing one run into the queue established.
#[derive(Debug)]
pub enum SealedRun {
    Queued(Box<ds_sync_runtime::reports::Sealed>),
    /// The store already holds this exact publication for the room.
    AlreadyRecorded {
        replay_key: String,
        state: ds_command_kernel::sync_store::ArtifactState,
    },
}

/// Seal one verified run through the producer's seal: bytes, integrity
/// receipts and the store row in one acknowledged step. The guard is
/// re-checked at the durable visibility boundary; its refusal comes back
/// as itself.
pub(super) fn seal_run_for_server(
    run: &ds_report_host::RunOutcome,
    owner_uid: &str,
    project_id: &str,
    queue: &PublicationQueue,
    guard: &dyn Fn() -> Result<(), Failure>,
) -> Result<SealedRun, Failure> {
    if run
        .engine
        .publication_state()
        .map_err(|error| Failure::failed(PUBLISH_ROOT.code, error).remedy(PUBLISH_ROOT.remedy))?
        != PublicationState::Pending
    {
        return Err(Failure::conflict(
            PUBLISH_LOCAL_ONLY.code,
            "the reporter build is local-only and cannot enter the Server publication queue",
        )
        .remedy(PUBLISH_LOCAL_ONLY.remedy));
    }
    if receipt_text(&run.receipt, "project_id")? != project_id
        || receipt_text(&run.receipt, "transformer")? != run.transformer
    {
        return Err(Failure::failed(
            PUBLISH_ROOT.code,
            "verified report receipt no longer matches the authenticated project or transformer",
        )
        .remedy(PUBLISH_ROOT.remedy));
    }
    let artifact_dir = HeldDirectory::open_absolute(&run.artifact_dir)
        .map_err(|error| Failure::failed(PUBLISH_ROOT.code, error).remedy(PUBLISH_ROOT.remedy))?
        .ok_or_else(|| {
            Failure::failed(
                PUBLISH_ROOT.code,
                "verified report artifact directory disappeared",
            )
            .remedy(PUBLISH_ROOT.remedy)
        })?;
    let mut held = Vec::with_capacity(run.artifacts.len());
    let mut formats = Vec::with_capacity(run.artifacts.len());
    for artifact in &run.artifacts {
        let file = artifact_dir
            .open_regular_file(OsStr::new(&artifact.filename), "verified report artifact")
            .map_err(|error| {
                Failure::failed(
                    PUBLISH_ROOT.code,
                    format!(
                        "could not open verified report artifact {}: {error}",
                        artifact.filename
                    ),
                )
                .remedy(PUBLISH_ROOT.remedy)
            })?;
        formats.push(artifact.format.as_str());
        held.push(VerifiedSidecarArtifact {
            format: artifact.format.clone(),
            filename: artifact.filename.clone(),
            size_bytes: artifact.size_bytes,
            sha256: artifact.sha256.clone(),
            paper_size: artifact.paper_size.clone(),
            presentation: artifact.presentation.clone(),
            held_file: file,
        });
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    // The guard's own refusal travels back through the seal's string error:
    // the seal re-checks it at the one durable visibility boundary, and a
    // refusal there must come back as the code the guard raised.
    let refused: std::cell::RefCell<Option<Failure>> = std::cell::RefCell::new(None);
    let scope_guard = || match guard() {
        Ok(()) => Ok(()),
        Err(failure) => {
            let message = failure.message().to_owned();
            *refused.borrow_mut() = Some(failure);
            Err(message)
        }
    };
    let request = SealRequest {
        owner_uid,
        project_id,
        engine_version: &run.engine.engine_version,
        engine_build_manifest_sha256: &run.engine.build_manifest_sha256,
        transformer: &run.transformer,
        // A Combined Report spans many saved revisions; its batch carries the
        // kernel's fixed publication revision instead of one room's.
        transformer_revision: if run.transformer
            == ds_command_kernel::report_export::combined::IDENTITY
        {
            ds_command_kernel::report_export::combined::PUBLICATION_REVISION
        } else {
            receipt_revision(&run.receipt)?
        },
        input_base_fingerprint: receipt_text(&run.receipt, "input_base_fingerprint")?,
        room_content_sha256: receipt_text(&run.receipt, "room_content_sha256")?,
        client_run_id: &run.client_run_id,
        artifacts: &held,
        expected_formats: &formats,
        deadline,
        guard: &scope_guard,
    };
    match ds_sync_runtime::reports::seal(&queue.store, &queue.fence, &queue.root, &request) {
        Ok(SealOutcome::Sealed(sealed)) => Ok(SealedRun::Queued(sealed)),
        Ok(SealOutcome::AlreadyRecorded { replay_key, state }) => {
            Ok(SealedRun::AlreadyRecorded { replay_key, state })
        }
        Err(error) => Err(refused.borrow_mut().take().unwrap_or_else(|| {
            Failure::failed(PUBLISH_ROOT.code, error).remedy(PUBLISH_ROOT.remedy)
        })),
    }
}

/// Why a run published nothing, in one token. There are only three ways to
/// reach that state and each one is something the caller asked for, so the
/// receipt can always name it rather than leaving "no publication" to be
/// read as "publication failed".
/// The governed print style refs the given layouts bind that the receipt's
/// sealed `printing_styles` sheet does not carry, sorted, once each.
fn unsealed_print_style_refs(
    sheets: &Value,
    layouts: &[ds_command_kernel::printing::Layout],
) -> Vec<String> {
    let sealed = &sheets["printing_styles"];
    layouts
        .iter()
        .flat_map(|layout| layout.style_refs.values())
        .filter(|reference| !sealed[reference.as_str()].is_object())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The refusal for a draft or proof binding styles the sealed receipt lacks,
/// naming the two sources the styles were looked for in: the receipt ds-brain
/// seals for the SELECTED printing setups, and the governed catalogue.
fn unsealed_print_styles_refusal(held: &[&String], absent: &[&String], what: &str) -> Failure {
    let mut message = format!(
        "the {what} layout binds print styles the sealed project receipt does not carry; the local reporter resolves print styles from the receipt ds-brain seals for the SELECTED printing setups (`ds report project settings` → papers)"
    );
    if !held.is_empty() {
        message.push_str(&format!(
            "; held by the governed catalogue (`ds style list --query _print`) but not sealed: {}",
            held.iter()
                .map(|r| r.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !absent.is_empty() {
        message.push_str(&format!(
            "; published nowhere (neither the receipt nor the governed catalogue): {}",
            absent
                .iter()
                .map(|r| r.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Failure::invalid(INPUTS_INVALID.code, message).remedy(
        "select a setup that binds these styles with `ds report project outputs set` so the receipt seals them (a proof completes sealed-but-missing styles from the catalogue itself), or bind styles the selected setups already use (`ds report layout style-ref`); a style published nowhere must be published first",
    )
}

/// 09b78d74: a local proof (`--print-layout`) is a replacement for an
/// existing setup, and the receipt seals only the styles the SELECTED setups
/// bound when ds-brain minted it. A proof that binds a style the governed
/// catalogue lists today but the receipt does not carry failed in the engine
/// one ref at a time ("resolved print style X is missing"). The proof now
/// carries those styles from the same governed catalogue `ds style list`
/// reads, spliced into its local recipe (the provenance still names the
/// server's receipt), and refuses — naming the source it looked in — for a
/// style published nowhere, or a symbol style whose icon the receipt never
/// sealed (the engine needs the held vector asset, which only ds-brain seals).
pub(super) fn complete_proof_print_styles(
    lane: &str,
    project: &str,
    proof: InputReceipt,
    layouts: &[ds_command_kernel::printing::Layout],
) -> Result<InputReceipt, Failure> {
    let mut sheets = proof
        .sheets()
        .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e))?;
    let unsealed = unsealed_print_style_refs(&sheets, layouts);
    if unsealed.is_empty() {
        return Ok(proof);
    }
    let snapshot = ds_cli_auth::style_catalog(lane, project)?;
    let catalogue = snapshot.result().document();
    let (held, absent): (Vec<_>, Vec<_>) = unsealed
        .iter()
        .partition(|reference| catalogue["styles"][reference.as_str()].is_object());
    if !absent.is_empty() {
        return Err(unsealed_print_styles_refusal(&held, &absent, "proof"));
    }
    let sealed_symbols = sheets["printing_symbol_assets"].clone();
    let mut unsealed_symbols = Vec::new();
    for reference in &held {
        let document = catalogue["styles"][reference.as_str()].clone();
        if let Some(icon) = document["layout"]["icon-image"]
            .as_str()
            .filter(|icon| !icon.is_empty() && !sealed_symbols[*icon].is_object())
        {
            unsealed_symbols.push(format!("{icon} (bound by {reference})"));
        }
        if !sheets["printing_styles"].is_object() {
            sheets["printing_styles"] = json!({});
        }
        sheets["printing_styles"][reference.as_str()] = document;
    }
    if !unsealed_symbols.is_empty() {
        return Err(Failure::invalid(
            INPUTS_INVALID.code,
            format!(
                "the proof layout binds symbol styles whose print icons the sealed project receipt does not carry: {}; the engine draws icons from the vector assets ds-brain seals for the SELECTED printing setups",
                unsealed_symbols.join(", ")
            ),
        )
        .remedy("select a setup that uses these styles with `ds report project outputs set` so the receipt seals their icons, or bind the layer to a style whose icon the selected setups already use"));
    }
    let mut completed = proof;
    completed.sheets_json = serde_json::to_string(&sheets)
        .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e.to_string()))?;
    completed.sheets_sha256 =
        ds_command_kernel::report_export::sha256_hex(completed.sheets_json.as_bytes());
    completed
        .validate()
        .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e))?;
    Ok(completed)
}

fn dry_run_reason(dry_run: bool, proofs: bool, preview: bool) -> &'static str {
    if dry_run {
        "dry_run_requested"
    } else if preview {
        "preview_layout_is_draft_state"
    } else if proofs {
        "print_layout_proof_is_not_a_governed_recipe"
    } else {
        // Unreachable by construction: `publish` is false only for the three
        // cases above. Named rather than panicking, because a receipt that
        // cannot explain itself is the defect, not a crash.
        "unpublished"
    }
}

/// The receipt of a publishing run that sealed nothing because the service
/// could not be reached after the retry curve: its prints may carry held
/// inputs or held rooms nobody could prove current, so none is published.
fn withheld_offline(unreachable: &str) -> Value {
    json!({
        "stage": "nothing_published",
        "published_nothing": true,
        "state": "withheld_service_unreachable",
        "reason": "service_unreachable",
        "unreachable": unreachable,
        "note": "THIS RUN PUBLISHED NOTHING. The service could not be reached, so these prints may carry held inputs or rooms nobody could prove current, and a held print is never published as current. Re-run the export when connected.",
    })
}

pub(super) fn verify_publish_scope(
    lane: &str,
    fence: &ds_cli_auth::LayerScopeFence,
    owner_uid: &str,
    project_id: &str,
) -> Result<(), Failure> {
    ds_cli_auth::verify_layer_scope_fence_for_project(lane, fence, owner_uid, project_id).map_err(
        |_| {
            Failure::conflict(
                PUBLISH_SCOPE_CHANGED.code,
                "the native publication scope changed before report artifacts could be sealed",
            )
            .remedy(PUBLISH_SCOPE_CHANGED.remedy)
        },
    )
}

/// What `--preview-layout` asks for, read and bounded before any state is
/// touched: the draft.
struct PreviewRequest {
    layout: ds_command_kernel::printing::Layout,
    retention: ds_report_host::preview_artifact::Retention,
}

fn preview_request(inputs: &Inputs, proofs: bool) -> Result<Option<PreviewRequest>, Failure> {
    let Some(path) = inputs.value("preview-layout") else {
        if inputs.value("preview-retention").is_some() {
            return Err(Failure::invalid(
                INPUTS_INVALID.code,
                "--preview-retention requires --preview-layout",
            ));
        }
        return Ok(None);
    };
    if proofs {
        return Err(Failure::invalid(
            INPUTS_INVALID.code,
            "--preview-layout and --print-layout are different recipes; pass one",
        ));
    }
    if inputs.switch("seed") || inputs.value("context-vectors").is_some() {
        return Err(Failure::invalid(
            INPUTS_INVALID.code,
            "a preview reads held context and never acquires; --seed and --context-vectors belong to a delivery export",
        )
        .remedy("a preview reads held context; seed or pass city vectors with a delivery export"));
    }
    let meta = std::fs::metadata(path)
        .map_err(|e| Failure::invalid(INPUTS_INVALID.code, format!("{path}: {e}")))?;
    if !meta.is_file() || meta.len() > ds_command_kernel::printing::MAX_LAYOUT_BYTES as u64 {
        return Err(Failure::invalid(
            INPUTS_INVALID.code,
            "Preview layout exceeds its regular-file bound",
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|e| Failure::invalid(INPUTS_INVALID.code, format!("{path}: {e}")))?;
    let layout: ds_command_kernel::printing::Layout = serde_json::from_slice(&bytes)
        .map_err(|e| Failure::invalid(INPUTS_INVALID.code, format!("{path}: {e}")))?;
    let layout = ds_command_kernel::printing::upgrade(layout)
        .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e))?;
    let retention = inputs
        .value("preview-retention")
        .map(|value| {
            serde_json::from_value(json!(value))
                .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e.to_string()))
        })
        .transpose()?
        .unwrap_or_default();
    Ok(Some(PreviewRequest { layout, retention }))
}

/// The exact saved revisions a batch pins, by transformer: each held room
/// file names one transformer this batch prints, at most once, and its saved
/// content digest must be the digest of its layers, so a delivery prints
/// again from exactly the bytes its receipt names.
fn pinned_rooms(
    paths: &[String],
    names: &[String],
) -> Result<BTreeMap<String, (ds_project_data::room_hold::Room, String)>, Failure> {
    let refuse = |message: String| {
        Failure::invalid(PINNED_ROOM_INVALID.code, message).remedy(PINNED_ROOM_INVALID.remedy)
    };
    let mut pins = BTreeMap::new();
    for path in paths {
        let bytes = std::fs::read(path).map_err(|e| refuse(format!("{path}: {e}")))?;
        let room: ds_project_data::room_hold::Room = serde_json::from_slice(&bytes)
            .map_err(|e| refuse(format!("{path} is not a held room: {e}")))?;
        let digest = ds_command_kernel::report_export::jcs::layers_content_digest(&room.layers)
            .map_err(|e| refuse(format!("{path}: {e}")))?;
        if room.content_digest.as_deref() != Some(digest.as_str()) || room.version.is_none() {
            return Err(refuse(format!(
                "{path}: the saved revision and content digest of {} do not match its layers",
                room.transformer
            )));
        }
        if !names.contains(&room.transformer) {
            return Err(refuse(format!(
                "{path} pins {}, which this batch does not print",
                room.transformer
            )));
        }
        let sha = ds_command_kernel::report_export::sha256_hex(&bytes);
        if pins.insert(room.transformer.clone(), (room, sha)).is_some() {
            return Err(refuse(format!("{path} pins a transformer twice")));
        }
    }
    Ok(pins)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    if inputs.value("held-export").is_some() {
        return held_export::run(inputs);
    }
    if inputs.value("held-preview").is_some() {
        return held_preview::run(inputs);
    }
    if inputs.switch("preview-voltage-drop") {
        return super::vd_preview::run(inputs);
    }
    let started = std::time::Instant::now();
    let mut requested_outputs = run_output_selection(inputs)?;
    let requested = super::transformer_set(inputs)?;
    let lane = inputs.require("lane")?;
    let out_dir = PathBuf::from(inputs.require("out-dir")?);
    let resident_limit = concurrency_limit(inputs)?;
    let explicit_asset = inputs.value("admin-bounds").map(PathBuf::from);
    let seed = inputs.switch("seed");
    let proof_paths = inputs.repeated("print-layout");
    // Publication is the default. A local print-layout proof and an SVG
    // preview are draft state that the publication contract does not admit,
    // so they are dry runs by nature — not a silent local-only export, and
    // their receipt still says it published nothing.
    let dry_run = inputs.switch("dry-run");
    let force = inputs.switch("force");
    // A preview's own refusals are decided before a city-vectors directory
    // is read, so they are local whatever that directory holds.
    let preview_request = preview_request(inputs, !proof_paths.is_empty())?;
    let proofs_requested = !proof_paths.is_empty();
    let preview_requested = preview_request.is_some();
    // A batch never overwrites a report: an output directory that already
    // holds one refuses before anything is read.
    if !preview_requested {
        existing_output(&out_dir, requested.names())?;
    }
    let publish = !dry_run && !proofs_requested && !preview_requested;
    // A print that publishes nothing reads what this machine holds first
    // (`REFRESH_ARG`); a publication verifies against the service's heads.
    let held_first = !publish && !inputs.switch("refresh");
    let local_context = inputs
        .value("context-vectors")
        .map(|dir| {
            ds_project_data::city_vectors::print_context(Path::new(dir)).map_err(|e| {
                Failure::invalid("print_context_invalid", e)
                    .remedy("acquire and verify the city vectors into a fresh directory")
            })
        })
        .transpose()?;
    if seed && local_context.is_some() {
        return Err(Failure::invalid(
            "report_inputs_invalid",
            "--seed and --context-vectors select different context acquisition modes",
        )
        .remedy("choose existing verified city vectors or project context seeding"));
    }
    let project = inputs.require("project")?;
    let publish_scope = publish
        .then(|| ds_cli_auth::capture_layer_scope_fence_for_project(lane, project))
        .transpose()?;
    let publish_queue = if publish {
        Some(PublicationQueue::open(
            lane,
            inputs.value("core-state-dir").map(Path::new),
        )?)
    } else {
        // A proof never opens either the custom or default Server queue.
        // Retain the caller's path only as explicit receipt evidence below.
        None
    };

    // The identity this machine's copy is kept under, observed without the
    // network: a Server with no link still knows whose copy it prints from.
    let identity = ds_cli_auth::headless_identity_for_named_project(lane)?;
    let project_id = project.to_string();
    if let Some(fence) = publish_scope.as_ref() {
        verify_publish_scope(lane, fence, fence.uid(), &project_id)?;
    }
    let hold = super::hold::Hold::open(&identity, &project_id)?;
    let mut link = super::hold::Link::default();
    // The lifecycle inventory (the scope and the drawing numbering), the
    // receipt ds-brain mints beside the project configuration, and the
    // printing setups its output selection names: one set, read and held when
    // the service answers, this machine's held set when it cannot be reached.
    let (mut held_inputs, inputs_receipt) = project_inputs(
        lane,
        &project_id,
        &identity,
        &hold,
        &mut link,
        &requested_outputs,
        held_first,
    )?;
    let rows = super::hold::select(&held_inputs.rows, &requested);
    let project_receipt = json!({
        "lane": identity.lane(),
        "project": {"ds_project": project_id, "project_name": null, "status": null},
    });
    let mut output = project_receipt.clone();
    output["project_crs_capture"] = serde_json::to_value(&held_inputs.project_crs)
        .map_err(|e| Failure::invalid("report_inputs_invalid", e.to_string()))?;
    let scope = super::scope_rows_json(&requested, &rows);
    let mut lifecycle: BTreeMap<String, String> = BTreeMap::new();
    let mut active = Vec::new();
    for row in &rows {
        let state = if row.is_transformer() {
            row.state.clone()
        } else {
            "project_level".to_string()
        };
        if row.is_transformer() && row.is_active() && reportable_transformer(&row.name).is_ok() {
            active.push(row.name.clone());
        }
        lifecycle.insert(row.name.clone(), state);
    }
    let names: Vec<String> = if requested.is_empty() {
        active
    } else {
        requested.names().to_vec()
    };
    if names.is_empty() {
        return Err(Failure::conflict(
            "report_batch_empty",
            "the named project has no active transformer to report",
        )
        .remedy(BATCH_EMPTY.remedy)
        .next("ds report project scope"));
    }
    // A named transformer that is no active saved transformer of the project
    // refuses before anything is acquired, with every such name at once.
    let inactive: Vec<&str> = names
        .iter()
        .filter(|name| lifecycle.get(name.as_str()).map(String::as_str) != Some("active"))
        .map(String::as_str)
        .collect();
    if !inactive.is_empty() {
        let roster = if held_first {
            "this machine's held project roster; pass --refresh if it was added since"
        } else {
            "the project roster"
        };
        return Err(Failure::conflict(
            NOT_ACTIVE.code,
            format!(
                "{} is not an active saved transformer in {roster}",
                inactive.join(", ")
            ),
        )
        .remedy(NOT_ACTIVE.remedy)
        .next("ds report project scope"));
    }

    // The project-wide input base: the receipt ds-brain minted beside the
    // configuration. The kernel proves it before anything is staged.
    let receipt = InputReceipt::from_config(&held_inputs.configuration).map_err(|error| {
        Failure::invalid("report_inputs_invalid", error).remedy(INPUTS_INVALID.remedy)
    })?;
    // Original server authority remains provenance even for an explicit local proof.
    let server_sheets_sha256 = receipt.sheets_sha256.clone();
    requested_outputs = run_outputs(inputs, &receipt, requested_outputs)?;
    let receipt = if proof_paths.is_empty() {
        receipt
    } else {
        let mut layouts = Vec::new();
        for path in proof_paths {
            let meta = std::fs::metadata(path)
                .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e.to_string()))?;
            if !meta.is_file() || meta.len() > ds_command_kernel::printing::MAX_LAYOUT_BYTES as u64
            {
                return Err(Failure::invalid(
                    INPUTS_INVALID.code,
                    "Print proof layout exceeds its regular-file bound",
                ));
            }
            let bytes = std::fs::read(path)
                .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e.to_string()))?;
            layouts.push(
                serde_json::from_slice(&bytes)
                    .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e.to_string()))?,
            );
        }
        if inputs.value("selection").is_none() {
            requested_outputs.prints = layouts
                .iter()
                .map(|layout: &ds_command_kernel::printing::Layout| {
                    ds_command_kernel::report_formats::NamedPrintSelection {
                        layout_id: layout.id.clone(),
                        enabled: true,
                        formats: vec![ds_command_kernel::report_formats::PrintArtifactFormat::Pdf],
                    }
                })
                .collect();
        }
        let proof = ds_command_kernel::report_export::proof::with_layouts(&receipt, &layouts)
            .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e))?;
        complete_proof_print_styles(lane, &project_id, proof, &layouts)?
    };
    // Only the sanctioned kernel local-proof boundary may capture a different
    // effective authored layout. A normal delivery retains its original held capsule.
    let printing_style_capture = if receipt.local_print_recipe.is_some() {
        let current = ds_cli_auth::headless_identity_for_named_project(lane)?;
        require_same_context(&identity, &project_id, &current, &project_id)
            .map_err(host_failure)?;
        let table = ds_cli_auth::style_governance(
            lane,
            &project_id,
            &ds_command_kernel::style_governance::Command::Table,
        )?;
        let current = ds_cli_auth::headless_identity_for_named_project(lane)?;
        require_same_context(&identity, &project_id, &current, &project_id)
            .map_err(host_failure)?;
        let snapshot = serde_json::from_value(table).map_err(|e| {
            Failure::invalid(
                INPUTS_INVALID.code,
                format!("local proof style source: {e}"),
            )
        })?;
        let sheets = receipt
            .sheets()
            .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e))?;
        let layouts =
            effective_capture_layouts(&sheets, &held_inputs.setups, &rows, &requested_outputs)
                .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e))?;
        Some(
            ds_command_kernel::printing::style_capture::Capture::new(
                &snapshot,
                &ds_command_kernel::printing::style_capture::Scope {
                    project: &project_id,
                    lane,
                    principal: identity.uid(),
                    audience: identity.credential_audience_sha256(),
                },
                &server_sheets_sha256,
                &sheets,
                &layouts,
            )
            .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e))?,
        )
    } else {
        held_inputs.printing_style_capture.clone()
    };
    if let Some(request) = &preview_request {
        // A draft may bind styles the sealed receipt never carried; the
        // engine would refuse them one at a time. Name them all now, with
        // the source each was looked for in.
        let sheets = receipt
            .sheets()
            .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e))?;
        let unsealed = unsealed_print_style_refs(&sheets, std::slice::from_ref(&request.layout));
        if !unsealed.is_empty() {
            let snapshot = ds_cli_auth::style_catalog(lane, &project_id)?;
            let catalogue = snapshot.result().document();
            let (held, absent): (Vec<_>, Vec<_>) = unsealed
                .iter()
                .partition(|reference| catalogue["styles"][reference.as_str()].is_object());
            return Err(unsealed_print_styles_refusal(&held, &absent, "preview"));
        }
    }
    // A preview hands the governed receipt over as minted: the kernel rewrites
    // it for the draft inside `ds_report_host::preview::execute`.
    output["local_print_recipe"] = serde_json::to_value(&receipt.local_print_recipe)
        .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e.to_string()))?;
    let admin_bounds = if receipt.requires_admin_bounds() {
        let path = match explicit_asset {
            Some(path) => path,
            None => {
                let root = shared_root().map_err(|error| {
                    Failure::unavailable("admin_bounds_unavailable", error)
                        .remedy(ADMIN_BOUNDS.remedy)
                })?;
                installed_admin_bounds_path(&root, &receipt.reference_semantic_sha256)
            }
        };
        Some(
            verify_admin_bounds_asset(&path, &receipt.reference_semantic_sha256)
                .map_err(host_failure)?,
        )
    } else {
        if explicit_asset.is_some() {
            return Err(Failure::invalid(
                "report_inputs_invalid",
                format!(
                    "the project reports for {:?}, which stamps no Rwanda administrative bounds; drop --admin-bounds",
                    receipt.country
                ),
            )
            .remedy(INPUTS_INVALID.remedy));
        }
        None
    };

    // What every print in this batch needs from outside the room: the kernel's
    // one decision over the sealed sheets, with `--seed` as the only way an
    // acquisition may happen. Nothing selected means nothing is read.
    // A preview's context layers are the draft's own, decided inside
    // `execute`; nothing is selected, projected or acquired for it here.
    let (contexts, hidden_contexts) = if local_context.is_some() || preview_request.is_some() {
        (Vec::new(), Vec::new())
    } else {
        selected_contexts(&receipt, &held_inputs.setups, seed, &requested_outputs)?
    };
    let neighbor_points_selected = preview_request.is_none()
        && super::neighbor_points::selected(&receipt, &held_inputs.setups, &requested_outputs)
            .map_err(|error| {
                Failure::invalid(INPUTS_INVALID.code, error).remedy(INPUTS_INVALID.remedy)
            })?;
    let neighbor_networks_selected = preview_request.is_none()
        && super::neighbor_points::networks_selected(
            &receipt,
            &held_inputs.setups,
            &requested_outputs,
        )
        .map_err(|error| {
            Failure::invalid(INPUTS_INVALID.code, error).remedy(INPUTS_INVALID.remedy)
        })?;
    let neighbors_selected = neighbor_points_selected || neighbor_networks_selected;
    let mv_buffer = contexts
        .iter()
        .filter_map(|c| match &c.source {
            ds_command_kernel::printing::PrintContextSource::ProjectDsgridMv {
                buffer_m, ..
            } => Some(*buffer_m),
            _ => None,
        })
        .reduce(f64::max);
    let mut context_warnings: Vec<Value> = Vec::new();
    // The projections an earlier connected run kept: a held-first print reads
    // them as they are, a connected one reuses each unchanged model.
    let mv_models = if mv_buffer.is_some() {
        let held = super::mv_context::held(&hold);
        let (models, source) = match held {
            Some(models) if held_first => (models, "held"),
            held => match link.read(|| {
                super::mv_context::load(
                    lane,
                    &identity,
                    &project_id,
                    held.as_deref().unwrap_or(&[]),
                )
            })? {
                Some(models) => {
                    if let Err(error) = super::mv_context::hold_models(&hold, &models) {
                        output["mv_context_hold_warning"] = json!(error);
                    }
                    (models, "service")
                }
                None => match held {
                    Some(models) => (models, "held_offline"),
                    None => {
                        context_warnings.push(json!({
                            "code": super::hold::INPUTS_NOT_HELD.code,
                            "message": format!(
                                "the project's MV models could not be read ({}) and none are held; the prints carry no MV context",
                                link.unreachable().unwrap_or("unreachable"),
                            ),
                        }));
                        (Vec::new(), "none")
                    }
                },
            },
        };
        output["mv_context_source"] = json!(source);
        models
    } else {
        Vec::new()
    };
    output["mv_context_models"] = json!(mv_models.len());
    output["mv_context_sources"] = json!(super::mv_context::provenance(&mv_models));
    // A held-first print locates catalogue rooms from what it holds (below);
    // only `--seed`, a refresh or a publication reads the catalogue.
    let catalog: Vec<ds_project_data::ReferenceResource> = if contexts.is_empty()
        || (held_first && !seed)
    {
        Vec::new()
    } else {
        let read = link.read(|| {
            ds_cli_auth::data_distribution(
                lane,
                &project_id,
                &ds_cli_auth::DataDistributionRequest::ListDatasets {},
            )
        });
        match read
            .map_err(|error| format!("{}: {}", error.code(), error.message()))
            .and_then(|rows| {
                rows.ok_or_else(|| {
                    format!(
                        "the service could not be reached: {}",
                        link.unreachable().unwrap_or("unreachable")
                    )
                })
            })
            .and_then(|rows| {
                ds_project_data::validate_resources(&rows).map_err(|error| error.to_string())
            }) {
            Ok(rows) => rows,
            Err(reason) => {
                context_warnings.push(json!({
                    "code": CONTEXT_CATALOG.code,
                    "message": format!(
                        "the reference catalogue could not be read ({reason}); catalogue layers print from the rooms this machine already holds, and nothing is acquired for them"
                    ),
                }));
                Vec::new()
            }
        }
    };
    let holdings_root = if contexts.is_empty() {
        None
    } else {
        Some(shared_root().map_err(|error| {
            Failure::failed(CONTEXT_INVALID.code, error).remedy(CONTEXT_INVALID.remedy)
        })?)
    };
    let holdings_scope = ds_command_kernel::project_dataset_cache::Scope {
        principal: identity.uid().to_string(),
        project: project_id.clone(),
    };
    // Without a catalogue, a `catalog` context source can still be located: a
    // held room records the resource it subsets. Reading it needs no version
    // check — nothing is downloaded or acquired against a row nobody verified.
    let catalog = if catalog.is_empty() && !contexts.is_empty() {
        match holdings_root.as_deref() {
            Some(root) => held_catalog_rooms(root, &holdings_scope),
            None => Vec::new(),
        }
    } else {
        catalog
    };
    let mut provider = ds_cli_data::project_cache::CliProvider {
        lane,
        project: &project_id,
    };
    let mut bundle_fetch = ds_cli_data::project_cache::bundle_fetch(lane);
    let mut transformer_context_notes: Vec<Value> = Vec::new();
    // A source a selected layout hides entirely prints nothing and is not
    // acquired; the receipt says so rather than counting it as printed.
    for hidden in &hidden_contexts {
        transformer_context_notes.push(json!({"note": hidden["note"]}));
    }
    // With no link, `--seed` has nothing to acquire from: the prints read
    // the context this machine holds, and the receipt says so.
    let acquire = seed && link.unreachable().is_none();
    if seed && !acquire {
        transformer_context_notes.push(json!({
            "note": format!(
                "--seed acquired nothing: the service could not be reached ({}); context is what this machine holds",
                link.unreachable().unwrap_or("unreachable"),
            ),
        }));
    }

    // Number the complete active inventory even for an explicitly selected subset.
    let drawing_names = held_inputs
        .rows
        .iter()
        .filter(|row| row.is_transformer() && row.is_active())
        .map(|row| row.name.clone())
        .collect::<Vec<_>>();
    let sheet_positions = ds_command_kernel::report_export::drawing_set_positions(&drawing_names)
        .map_err(|error| {
        Failure::invalid(INPUTS_INVALID.code, error).remedy(INPUTS_INVALID.remedy)
    })?;

    // Which rooms this batch reads from the service: the kernel's plan over
    // what this machine holds and the heads the service reports. With no
    // link there are no heads, and an unknown head is no evidence of change.
    // The same rows are the only reuse evidence, kept whole and only when
    // the answer is for this account and this project.
    let all_transformers = ds_cli_auth::TransformerSet::default();
    let status_scope = if neighbors_selected {
        &all_transformers
    } else {
        &requested
    };
    let read_status = |link: &mut super::hold::Link| match link
        .read(|| ds_cli_auth::transformer_status_for_project(lane, project, status_scope))
    {
        Ok(Some(status)) => {
            let rows = status.result().rows();
            let evidence = (status.identity() == &identity && status.project_id() == project_id)
                .then(|| {
                    rows.iter()
                        .map(|row| (row.name().to_string(), row.row().clone()))
                        .collect::<BTreeMap<String, Value>>()
                });
            let heads = rows
                .iter()
                .map(|row| json!({"name": row.name(), "version": super::hold::head_version(row.row()), "layers": row.row().get("layers")}))
                .collect();
            (evidence, super::hold::Heads::Read(heads))
        }
        Ok(None) => (None, super::hold::Heads::Unreachable),
        Err(failure) => (
            None,
            super::hold::Heads::Refused(format!("{}: {}", failure.code(), failure.message())),
        ),
    };
    // A held-first print reads no heads: its held rooms print as held and only
    // a room this machine does not hold is read, once.
    let (status_rows, mut heads) = if held_first {
        (None, super::hold::Heads::Held)
    } else {
        read_status(&mut link)
    };

    // The survey forms the project appends to a delivered report, from the
    // core's held copy of each: refreshed once for the whole batch, then cut
    // per transformer by the kernel. Their photos become links under one
    // report grant, the same links the cloud export writes.
    let survey_forms =
        ds_command_kernel::report_export::survey_forms(&receipt).map_err(|error| {
            Failure::invalid(INPUTS_INVALID.code, error).remedy(INPUTS_INVALID.remedy)
        })?;

    // Which outputs this run may reuse (`super::reuse`): planned once per
    // transformer against the installed engine's one `build-info` and the
    // head revision its status row names. A transformer whose every output
    // is current leaves the batch; the rest hand the engine only what they
    // lack, or the full policy when there is no evidence.
    let same_publisher = publish_scope
        .as_ref()
        .is_some_and(|fence| fence.uid() == identity.uid());
    let withheld = reuse_withheld(
        publish,
        force,
        !survey_forms.is_empty(),
        status_rows.is_some(),
        same_publisher,
    );
    let selected_engine = CliEngine::capture(identity.lane()).map_err(host_failure)?;
    let mut engine_now = match withheld {
        None => Some(installed_engine(&selected_engine)?),
        Some(_) => None,
    };
    let (mut reuse_plans, mut unplanned, policy) = match (&engine_now, &status_rows) {
        (Some(engine), Some(rows)) => {
            let policy =
                reuse::policy_outputs(&receipt, Some(&requested_outputs)).map_err(|error| {
                    Failure::invalid(INPUTS_INVALID.code, error).remedy(INPUTS_INVALID.remedy)
                })?;
            let (plans, unplanned) = plan_reuse(
                &names,
                rows,
                &policy,
                &receipt,
                &engine.build_manifest_sha256,
            );
            (plans, unplanned, policy)
        }
        _ => (BTreeMap::new(), BTreeMap::new(), Vec::new()),
    };
    // A run that would generate nothing claims every output current, so that
    // claim rests on evidence read after the plan, not on the read it was
    // planned from: a save, a newer run's copy or a replaced engine landing
    // in between withdraws the transformer's reuse, and it runs the full
    // policy like any transformer without evidence. The batch that follows
    // then plans its rooms from these later heads.
    if !names.is_empty()
        && names.iter().all(|name| {
            matches!(
                reuse_plans.get(name).map(|plan| &plan.scope),
                Some(RunScope::AllReused)
            )
        })
    {
        let engine = installed_engine(&selected_engine)?;
        let (confirming, confirming_heads) = read_status(&mut link);
        heads = confirming_heads;
        let withdrawn = reconfirm_all_reused(
            &reuse_plans,
            confirming.as_ref(),
            &policy,
            &receipt,
            &engine.build_manifest_sha256,
        );
        for (name, reason) in withdrawn {
            reuse_plans.remove(&name);
            unplanned.insert(name, reason);
        }
        engine_now = Some(engine);
    }
    let names: Vec<String> = names
        .into_iter()
        .filter(|name| {
            !matches!(
                reuse_plans.get(name).map(|plan| &plan.scope),
                Some(RunScope::AllReused)
            )
        })
        .collect();
    if names.is_empty() {
        // Every requested output is the project's current copy, confirmed on
        // the later read above: no room is read, no engine runs, nothing is
        // sealed. The native scope is re-proved last, so a changed account
        // or project never reads as current.
        let fence = publish_scope.as_ref().ok_or_else(|| {
            Failure::conflict(
                PUBLISH_SCOPE_CHANGED.code,
                "no native publication scope was captured for a run that reuses the project's outputs",
            )
            .remedy(PUBLISH_SCOPE_CHANGED.remedy)
        })?;
        verify_publish_scope(lane, fence, fence.uid(), &project_id)?;
        output["out_dir"] = json!(out_dir.display().to_string());
        output["scope"] = scope;
        output["publication_enqueued"] = json!(false);
        output["publication"] = json!({
            "stage": "nothing_published",
            "published_nothing": true,
            "state": "all_outputs_current",
            "reason": "every_output_reused",
            "note": "Nothing was generated or published: the project already holds every requested output from this run's exact inputs and engine build. `reuse.transformers[]` names each copy; pass --force to regenerate.",
        });
        output["engine"] = engine_now.as_ref().map_or(Value::Null, |engine| {
            json!({
                "engine_version": engine.engine_version,
                "build_manifest_sha256": engine.build_manifest_sha256,
                "publication_state": engine.publication_state().ok(),
            })
        });
        output["batch"] = json!({
            "status": "reused",
            "requested_count": 0,
            "completed": 0,
            "failed": 0,
            "partial_formats": 0,
            "concurrency": 0,
            "receipt": null,
        });
        output["results"] = json!([]);
        output["rooms"] = json!({"reused": 0, "rooms_fetched": 0, "not_held": [], "heads": "read"});
        output["inputs"] = inputs_receipt;
        output["reuse"] = reuse_receipt(
            withheld,
            engine_now.as_ref(),
            &reuse_plans,
            &unplanned,
            &BTreeMap::new(),
        );
        return Ok(output);
    }
    // Only voltage-drop runs need the project-governed nature projection.
    // Keep it separate from the held/fetched room bytes; the report host first
    // verifies those exact bytes against their saved content digest.
    let output_policy =
        reuse::policy_outputs(&receipt, Some(&requested_outputs)).map_err(|error| {
            Failure::invalid(INPUTS_INVALID.code, error).remedy(INPUTS_INVALID.remedy)
        })?;
    let voltage_drop_selected = preview_request.is_none()
        && ds_command_kernel::report_formats::requires_voltage_drop_analysis(&output_policy);
    let held_scope = ds_command_kernel::report_export::held::Scope {
        project: project_id.clone(),
        lane: lane.to_owned(),
        principal: identity.uid().to_owned(),
        credential_audience_sha256: identity.credential_audience_sha256().to_owned(),
        sheets_sha256: server_sheets_sha256.clone(),
    };
    let transformer_natures = if voltage_drop_selected {
        // A held-first print reads the held projection when it covers the batch.
        let covered = held_first
            && held_inputs
                .transformer_natures
                .as_ref()
                .is_some_and(|capture| capture.values(&held_scope, &names).is_ok());
        let service_capture = if covered {
            None
        } else {
            link.read(|| {
                project_transformer_natures(lane, &project_id, &identity, &names, &held_scope)
            })?
        };
        let (capture, source) = match service_capture {
            Some(capture) => {
                held_inputs.transformer_natures = Some(capture.clone());
                if let Err(error) = hold.hold_inputs(&held_inputs) {
                    output["inputs_hold_warning"] = json!(error);
                }
                (Some(capture), "service")
            }
            None if covered || !publish => (held_inputs.transformer_natures.clone(), "held"),
            None => {
                return Err(transformer_nature_unavailable(
                    "publication requires the current governed tag projection",
                ));
            }
        };
        let capture = capture.ok_or_else(|| {
            transformer_nature_unavailable(
                "no matching governed transformer nature projection is held on this machine",
            )
        })?;
        let natures = capture
            .values(&held_scope, &names)
            .map_err(|error| invalid_transformer_nature_projection(&error))?;
        output["transformer_natures_source"] = json!(source);
        Some(natures)
    } else {
        None
    };
    if neighbors_selected && !held_first && status_rows.is_none() && link.unreachable().is_none() {
        return Err(Failure::unavailable(
            NEIGHBOR_POINTS_UNAVAILABLE.code,
            "the project-wide transformer status could not be verified for the neighboring point catalogue",
        )
        .remedy(NEIGHBOR_POINTS_UNAVAILABLE.remedy));
    }

    let processors = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let plan = batch_plan(&names, processors, resident_limit).map_err(|error| {
        Failure::invalid("report_inputs_invalid", error).remedy(INPUTS_INVALID.remedy)
    })?;

    // A sheet opts into the neighboring transformer marker layer through its
    // governed style binding. Build the project point catalogue once; held
    // rooms at their current heads cost no network read, and an unheld room is
    // acquired once before any of this batch's sheets render.
    let neighbor_catalogue = neighbors_selected
        .then(|| {
            super::neighbor_points::catalogue(
                lane,
                &project_id,
                &identity,
                &drawing_names,
                status_rows.as_ref(),
                &hold,
                &mut link,
                neighbor_networks_selected,
            )
        })
        .transpose()?;
    let (neighbor_markers, neighbor_networks) = match neighbor_catalogue {
        Some((points, rooms)) if neighbor_networks_selected => {
            (Some(points), Some(super::neighbor_points::networks(rooms)?))
        }
        Some((points, _)) => (Some(points), None),
        None => (None, None),
    };
    let neighbor_fields = if neighbor_networks_selected {
        super::neighbor_points::network_fields(
            &receipt,
            &held_inputs.setups,
            &requested_outputs,
            printing_style_capture.as_ref(),
        )
        .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e))?
    } else {
        BTreeMap::new()
    };
    if let Some(markers) = &neighbor_markers {
        let bytes = serde_json::to_vec(markers).map_err(|error| {
            Failure::failed(NEIGHBOR_POINTS_UNAVAILABLE.code, error.to_string())
                .remedy(NEIGHBOR_POINTS_UNAVAILABLE.remedy)
        })?;
        output["neighbor_transformers"] = json!({
            "points": markers.len(),
            "sha256": ds_command_kernel::report_export::sha256_hex(&bytes),
            "room_source": if status_rows.is_some() {
                "verified_or_refreshed"
            } else if held_first {
                "held"
            } else {
                "held_offline"
            },
            "circuits": neighbor_networks_selected,
            "selection": "all active project rooms; final focused map viewport clips context",
        });
    }

    let staging = out_dir.join(STAGING_DIRECTORY);
    let pins = pinned_rooms(inputs.repeated("pin-room"), &plan.names)?;
    if !pins.is_empty() {
        output["pinned_rooms"] = json!(
            pins.values()
                .map(|(room, sha)| json!({
                    "transformer": room.transformer, "version": room.version,
                    "content_digest": room.content_digest, "room_file_sha256": sha,
                }))
                .collect::<Vec<_>>()
        );
    }
    let analysis_hold = hold.clone();
    let captured_inputs = RefCell::new(held_inputs.clone());
    let rooms = RefCell::new(super::hold::Rooms::plan(hold, &plan.names, heads)?);
    let link = RefCell::new(link);
    // One transformer's room as this command admits it: an active saved
    // transformer of this project with a positive saved revision, from this
    // machine's hold when the plan reuses it, or fetched under the batch's
    // identity and held. The refusal keeps the CLI's own class, code and
    // remedy: a preview ends with it as the command's answer, a delivery
    // records it as one row of the batch.
    let fetch_room = |name: &str| -> Result<(ds_project_data::room_hold::Room, i64), Failure> {
        match lifecycle.get(name).map(String::as_str) {
            Some("active") => {}
            Some(state) => {
                return Err(Failure::conflict(
                    NOT_ACTIVE.code,
                    format!("{name} is {state}, not an active saved transformer"),
                )
                .remedy(NOT_ACTIVE.remedy));
            }
            None => {
                return Err(Failure::conflict(
                    NOT_ACTIVE.code,
                    format!("{name} is not in the project's transformer inventory"),
                )
                .remedy(NOT_ACTIVE.remedy));
            }
        }
        if let Some((room, _)) = pins.get(name) {
            let version = saved_revision(name, room.version.and_then(|v| u64::try_from(v).ok()))?;
            return Ok((room.clone(), version));
        }
        // A weak link blinks; the link asks a room fetch refused by an outage
        // again under the shared curve before the row is written off.
        let room = rooms.borrow_mut().room(name, &mut link.borrow_mut(), || {
            let context = ds_cli_auth::saved_transformer_context_for_project(lane, project, name)?;
            require_same_context(
                &identity,
                &project_id,
                context.identity(),
                context.snapshot().ds_project(),
            )
            .map_err(host_failure)?;
            let snapshot = context.snapshot();
            if snapshot.ds_project() != project_id || snapshot.transformer_name() != name {
                return Err(Failure::invalid(
                    INPUTS_INVALID.code,
                    format!("the service answered for another project or transformer than {name}"),
                )
                .remedy(INPUTS_INVALID.remedy));
            }
            // Only a saved revision is held, so a held room always has one.
            let version = saved_revision(name, snapshot.metadata().version())?;
            Ok(ds_project_data::room_hold::Room {
                transformer: name.to_string(),
                version: Some(version),
                content_digest: snapshot.metadata().content_digest().map(str::to_string),
                layers: snapshot.layers().clone(),
            })
        })?;
        let server_version =
            saved_revision(name, room.version.and_then(|v| u64::try_from(v).ok()))?;
        Ok((room, server_version))
    };

    if let Some(request) = preview_request {
        return preview_pages(
            &request,
            printing_style_capture.as_ref(),
            &plan.names,
            &PreviewSettings {
                project_id: &project_id,
                project_crs: held_inputs.project_crs.as_ref(),
                receipt: &receipt,
                admin_bounds: admin_bounds.as_ref(),
                staging_root: &staging,
            },
            &holdings_scope,
            &sheet_positions,
            &out_dir,
            fetch_room,
            PreviewFacts {
                project: project_receipt,
                lane,
                scope,
                identity: &identity,
            },
            inputs.value("core-state-dir").is_some(),
        );
    }

    // A held-first print reuses every held survey form, whatever its age, and
    // reads only a form this machine does not hold.
    let survey_refresh = match inputs.require("survey-refresh")? {
        "auto" if held_first => "held",
        refresh => refresh,
    };
    let survey_context = super::survey_context::load(
        lane,
        &holdings_scope,
        &identity,
        &contexts,
        survey_refresh,
        &mut link.borrow_mut(),
    )?;
    let room_contexts: Vec<_> = contexts
        .iter()
        .filter(|context| {
            !matches!(
                context.source,
                ds_command_kernel::printing::PrintContextSource::Survey { .. }
            )
        })
        .cloned()
        .collect();
    let (held_survey, survey_omitted, survey_grant) = survey_append_inputs(
        lane,
        &project_id,
        &survey_forms,
        survey_refresh,
        &identity,
        &mut link.borrow_mut(),
    )?;
    output["survey"] = survey_json(survey_refresh, &survey_forms, &held_survey, &survey_omitted);

    let settings = BatchSettings {
        run: RunSettings {
            project_id: &project_id,
            receipt: &receipt,
            admin_bounds: admin_bounds.as_ref(),
            staging_root: &staging,
            out_root: &out_dir,
        },
        lane,
        concurrency: plan.concurrency,
        media_grant: survey_grant.as_deref(),
    };
    // Subset runs whose room moved past the planned head: they ran in full.
    let mut room_moved: BTreeMap<String, i64> = BTreeMap::new();
    let analyses = std::sync::Mutex::new(BTreeMap::new());
    let engine = SavedAnalysisEngine {
        analyses: &analyses,
        delegate: &selected_engine,
        report_format: requested_outputs.voltage_drop_report_format().token(),
    };
    let fetch = |name: &str| -> Result<TransformerReportInputs, HostFailure> {
        let (room, server_version) = fetch_room(name).map_err(failure_to_host)?;
        let (selection, moved) = run_selection(reuse_plans.get(name), server_version);
        if moved {
            room_moved.insert(name.to_string(), server_version);
        }
        if voltage_drop_selected {
            let service_analysis = link
                .borrow_mut()
                .read(|| {
                    match ds_cli_auth::saved_transformer_analysis_for_project(lane, project, name) {
                        Ok(saved) => {
                            require_same_context(
                                &identity,
                                &project_id,
                                saved.identity(),
                                saved.project_id(),
                            )
                            .map_err(host_failure)?;
                            let saved = saved.result();
                            fenced_batch_analysis(
                                &room,
                                saved.snapshot.transformer_name(),
                                saved.snapshot.metadata().version(),
                                saved.snapshot.metadata().content_digest(),
                                saved.snapshot.voltage_drop_metadata(),
                                saved.document.as_deref(),
                            )
                            .map_err(host_failure)
                        }
                        Err(error) => match batch_analysis_refusal(&error) {
                            Some(refusal) => Ok(refusal),
                            None => Err(error),
                        },
                    }
                })
                .map_err(failure_to_host)?;
            let analysis = match service_analysis {
                Some(analysis) => {
                    let mut retained = captured_inputs.borrow_mut();
                    retain_batch_analysis(&mut retained, &held_scope, &room, &analysis)?;
                    let _ = analysis_hold.hold_inputs(&retained);
                    analysis
                }
                None if !publish => {
                    held_batch_analysis(&captured_inputs.borrow(), &held_scope, &room)?
                }
                None => BatchAnalysis::Missing,
            };
            analyses
                .lock()
                .map_err(|_| HostFailure::new(INPUTS_INVALID.code, "saved analysis lock poisoned"))?
                .insert(name.to_string(), analysis);
        }
        let layers_value = serde_json::to_value(&room.layers)
            .map_err(|error| HostFailure::new(INPUTS_INVALID.code, error.to_string()))?;
        let print_context = if let Some(context) = &local_context {
            ds_project_data::city_vectors::require_design_coverage(context, name, &layers_value)
                .map_err(|error| HostFailure::new(CONTEXT_INVALID.code, error))?;
            Some(ds_report_host::PrintContextBytes {
                bytes: context.document.clone().ok_or_else(|| {
                    HostFailure::new("print_context_invalid", "city context has no document")
                })?,
                sha256: context.sha256.clone().unwrap_or_default(),
                layers: context.layers.clone(),
                omitted: Vec::new(),
            })
        } else {
            match holdings_root.as_deref() {
                None => None,
                Some(root) => {
                    let mut hosts = ds_project_data::Hosts {
                        provider: &mut provider,
                        fetch: &mut bundle_fetch,
                    };
                    let mode = if acquire {
                        ds_project_data::Mode::Acquire(&mut hosts)
                    } else {
                        ds_project_data::Mode::Read
                    };
                    let context = ds_project_data::read_print_context(
                        root,
                        &holdings_scope,
                        name,
                        &layers_value,
                        &room_contexts,
                        &catalog,
                        mode,
                    )
                    .map_err(context_failure)?;
                    // Survey has no configured extent buffer. Its complete
                    // verified geometry follows the shared context/frame policy.
                    let context = if survey_context.is_empty() {
                        context
                    } else {
                        ds_project_data::held::attach_layers(
                            Some(context),
                            survey_context.clone(),
                            name,
                            &layers_value,
                            0.0,
                        )
                        .map_err(context_failure)?
                        .ok_or_else(|| {
                            HostFailure::new(
                                CONTEXT_INVALID.code,
                                "selected survey context capture disappeared",
                            )
                        })?
                    };
                    for warning in &context.warnings {
                        transformer_context_notes
                            .push(json!({"transformer": name, "note": warning}));
                    }
                    context
                        .document
                        .map(|bytes| ds_report_host::PrintContextBytes {
                            bytes,
                            sha256: context.sha256.unwrap_or_default(),
                            layers: context.layers,
                            omitted: context.omitted.iter().map(|o| o.layer.clone()).collect(),
                        })
                }
            }
        };
        let print_context = if let Some(buffer) = mv_buffer {
            super::mv_context::attach(print_context, &mv_models, name, &layers_value, buffer)
                .map_err(failure_to_host)?
        } else {
            print_context
        };
        let print_context = if let Some(networks) = &neighbor_networks {
            super::neighbor_points::attach_networks(name, networks, print_context, &neighbor_fields)
                .map_err(|error| HostFailure::new(CONTEXT_INVALID.code, error))?
        } else if let Some(markers) = &neighbor_markers {
            super::neighbor_points::attach(name, markers, print_context)
                .map_err(|error| HostFailure::new(CONTEXT_INVALID.code, error))?
        } else {
            print_context
        };
        let print_context = if let Some(capture) = &printing_style_capture {
            Some(ds_report_host::bind_print_styles(
                print_context,
                capture,
                &project_id,
                lane,
                identity.uid(),
                identity.credential_audience_sha256(),
                &receipt
                    .sheets()
                    .map_err(|e| HostFailure::new(INPUTS_INVALID.code, e))?,
                &server_sheets_sha256,
            )?)
        } else {
            print_context
        };
        let print_context = if let Some(capture) = &held_inputs.project_crs {
            Some(ds_report_host::bind_project_crs(
                print_context,
                capture,
                &project_id,
            )?)
        } else {
            print_context
        };
        let sheet = sheet_positions.get(name).copied();
        let survey = if survey_forms.is_empty() {
            None
        } else {
            Some(
                ds_command_kernel::report_export::survey_append(
                    &room.layers,
                    &held_survey,
                    &survey_omitted,
                )
                .map_err(|error| HostFailure::new(INPUTS_INVALID.code, error))?,
            )
        };
        Ok(TransformerReportInputs {
            transformer: name.to_string(),
            server_version,
            content_digest: room.content_digest.clone(),
            layers: room.layers,
            transformer_natures: transformer_natures.as_ref().map(|natures| {
                natures
                    .get(name)
                    .map(|nature| BTreeMap::from([(name.to_string(), nature.clone())]))
                    .unwrap_or_default()
            }),
            selection: if preview_requested {
                selection
            } else {
                Some(selection.unwrap_or_else(|| requested_outputs.clone()))
            },
            print_context,
            sheet,
            survey,
        })
    };
    let acquired_ms = started.elapsed().as_millis();
    let outcome = run_batch(&engine, &settings, &plan.names, fetch).map_err(host_failure)?;
    let rendered_ms = started.elapsed().as_millis();
    // Every run removed its own scratch; the empty staging root goes too.
    let _ = std::fs::remove_dir(&staging);
    if outcome.receipt["status"] == "failed" {
        return Err(
            Failure::failed("report_batch_failed", "no transformer report completed")
                .remedy(BATCH_FAILED.remedy)
                .detail(json!({
                    "out_dir": out_dir.display().to_string(),
                    "results": outcome.receipt["results"],
                    "rooms": rooms.borrow().receipt(),
                    "inputs": inputs_receipt,
                })),
        );
    }

    // The reuse plan compared the project's copies against one engine build;
    // a batch that ran under another would publish beside them outputs the
    // plan never compared, so nothing is sealed.
    if let Some(planned) = engine_now.as_ref()
        && reuse_plans
            .values()
            .any(|plan| !matches!(plan.scope, RunScope::Full))
        && outcome.engine != *planned
    {
        return Err(Failure::failed(
            IDENTITY_CHANGED.code,
            "`ds-report` was replaced after the reuse plan compared the project's outputs against its build; nothing was sealed",
        )
        .remedy(IDENTITY_CHANGED.remedy));
    }

    let publication_state = if receipt.local_print_recipe.is_some() {
        Some(ds_command_kernel::report::PublicationState::LocalOnly)
    } else {
        outcome.engine.publication_state().ok()
    };
    // A print made after the service could not be reached may carry held
    // inputs or a held room whose head nobody could check: it is never
    // sealed as the current publication.
    let unreachable = link.borrow().unreachable().map(str::to_owned);
    let sealed_publications = if let (Some(fence), Some(queue), None) = (
        publish_scope.as_ref(),
        publish_queue.as_ref(),
        unreachable.as_ref(),
    ) {
        let mut publications = Vec::with_capacity(outcome.runs.len());
        for run in &outcome.runs {
            // The seal is the durable effect: bytes and the store row in
            // one step. Re-probe the native UID/audience/project/
            // credential binding immediately before it, and again at its
            // visibility boundary through the guard.
            verify_publish_scope(lane, fence, fence.uid(), &project_id)?;
            let sealed = seal_run_for_server(run, fence.uid(), &project_id, queue, &|| {
                verify_publish_scope(lane, fence, fence.uid(), &project_id)
            })?;
            publications.push(match sealed {
                    SealedRun::Queued(sealed) => json!({
                        "state": "queued",
                        "batch_id": sealed.receipt.batch_id,
                        "client_publish_id": sealed.receipt.client_publish_id,
                        "transformer": sealed.receipt.transformer,
                        "outputs": sealed.artifacts.len(),
                        "bytes_locator": sealed.row.bytes_locator,
                        "superseded": sealed.superseded.as_ref().map(|(publish_id, bytes)| json!({
                            "client_publish_id": publish_id,
                            "bytes_freed": bytes,
                        })),
                    }),
                    SealedRun::AlreadyRecorded { replay_key, state } => json!({
                        "state": "already_recorded",
                        "client_publish_id": replay_key,
                        "store_state": state.as_str(),
                        "transformer": run.transformer,
                        "note": "the store already holds this exact publication for the room; nothing was sealed again",
                    }),
                });
        }
        Some(publications)
    } else {
        None
    };
    output["out_dir"] = json!(out_dir.display().to_string());
    output["scope"] = scope;
    output["publication_enqueued"] = json!(sealed_publications.is_some());
    output["publication"] = match (publish_queue, sealed_publications) {
        (Some(queue), Some(publications)) => json!({
            "stage": ds_command_kernel::report::PublicationStage::Queued.as_str(),
            "published_nothing": false,
            "state": "queued_for_server_sync",
            "root": queue.root.display().to_string(),
            "batches": publications,
            "note": "Sealed into the sync store and queued; the one publication queue drains it. `ds report outbox status` says where it is.",
        }),
        // Acceptance B, literally. A dry run's receipt has to say it published
        // nothing IN THOSE WORDS, because the failure being closed here is a
        // receipt that looked exactly like a publication.
        _ if publish => withheld_offline(unreachable.as_deref().unwrap_or("unreachable")),
        _ => json!({
            "stage": "nothing_published",
            "published_nothing": true,
            "state": "dry_run",
            "reason": dry_run_reason(dry_run, proofs_requested, preview_requested),
            "queue_opened": false,
            "server_state_dir_ignored": inputs.value("core-state-dir").is_some(),
            "note": "THIS RUN PUBLISHED NOTHING. The files are local only and no queue can see them. Re-run without --dry-run to publish, or `ds report project publish --from <out-dir>` to publish what is already on disk.",
        }),
    };
    output["engine"] = json!({
        "engine_version": outcome.engine.engine_version,
        "build_manifest_sha256": outcome.engine.build_manifest_sha256,
        "publication_state": publication_state,
    });
    output["batch"] = json!({
        "status": outcome.receipt["status"],
        "requested_count": outcome.receipt["requested_count"],
        "completed": outcome.receipt["completed"],
        "failed": outcome.receipt["failed"],
        // Runs that delivered artifacts and still lost a format. They are
        // counted as completed — their work is on disk and publishable — and
        // named here so a reader knows to look at `results[].failed_formats`.
        "partial_formats": outcome.receipt["partial_formats"],
        "concurrency": outcome.receipt["concurrency"],
        "receipt": outcome.receipt_path.display().to_string(),
    });
    output["results"] = outcome.receipt["results"].clone();
    if let Some(omitted) = outcome.receipt.get("survey_layers_omitted") {
        output["batch"]["survey_layers_omitted"] = omitted.clone();
    }
    // What each completed print carried from outside its room, from the run
    // receipts the host wrote: the digest the engine verified, the layers, and
    // the selected layers this machine could not supply.
    if let Some(rows) = output["results"].as_array_mut() {
        for row in rows.iter_mut() {
            let Some(name) = row["transformer"].as_str().map(str::to_string) else {
                continue;
            };
            // Where its room came from: this machine's hold, or the service.
            row["room_source"] = json!(rooms.borrow().source(&name));
            if voltage_drop_selected {
                row["transformer_nature"] = transformer_natures
                    .as_ref()
                    .and_then(|natures| natures.get(&name))
                    .map_or_else(
                        || json!({"status": "unassigned"}),
                        |nature| json!({"status": "resolved", "value": nature}),
                    );
            }
            if let Some(run) = outcome.runs.iter().find(|run| run.transformer == name) {
                row["print_context"] = json!({
                    "sha256": run.receipt["print_context_sha256"],
                    "layers": run.receipt["print_context_layers"],
                    "omitted": run.receipt["print_context_omitted"],
                });
            }
        }
    }
    output["rooms"] = rooms.borrow().receipt();
    output["inputs"] = inputs_receipt;
    output["reuse"] = reuse_receipt(
        withheld,
        engine_now.as_ref(),
        &reuse_plans,
        &unplanned,
        &room_moved,
    );
    output["context"] = json!({
        "selected_layers": local_context.as_ref().map_or_else(
            || contexts.iter().map(|layer| layer.id.clone()).collect::<Vec<_>>(),
            |context| context.layers.clone(),
        ),
        "seeded": seed,
        "catalog_resources": catalog.len(),
        "warnings": context_warnings,
        "hidden": hidden_contexts,
        "notes": transformer_context_notes,
    });
    // Where the time went: acquiring inputs, the batch (context and engine),
    // and writing what it produced.
    let total_ms = started.elapsed().as_millis();
    output["timing_ms"] = json!({
        "acquire": acquired_ms,
        "render": rendered_ms - acquired_ms,
        "write": total_ms - rendered_ms,
        "total": total_ms,
    });
    Ok(output)
}

/// The project's survey forms as this machine holds them, for a whole batch:
/// each form refreshed once under the whole-form filter (the kernel's
/// `survey::hold` decides reuse, delta or a fresh read; `local` never reads),
/// the forms it could not hold with their reasons, and — when anything is
/// held — the report media grant the engine derives photo links under. A
/// form that cannot be held, or a grant that cannot be minted, never stops
/// the batch: the sheets are produced and the receipt names what they lack.
#[allow(clippy::type_complexity)]
fn survey_append_inputs(
    lane: &str,
    project: &str,
    forms: &[String],
    refresh: &str,
    identity: &ds_cli_auth::ProviderIdentity,
    link: &mut super::hold::Link,
) -> Result<
    (
        Vec<ds_command_kernel::report_export::HeldSurveyForm>,
        Vec<ds_command_kernel::report_export::SurveyFormOmission>,
        Option<Vec<u8>>,
    ),
    Failure,
> {
    use ds_command_kernel::report_export::survey::{
        SURVEY_LAYER_NOT_HELD_REASON, survey_layer_unrefreshed_reason,
    };
    use ds_command_kernel::report_export::{HeldSurveyForm, SurveyFormOmission};
    use ds_command_kernel::survey::hold::{self, Filter, Refresh};
    use ds_project_data::survey_hold::{self as store, HoldError};

    let omit_all = |reason: &str| {
        forms
            .iter()
            .map(|form| SurveyFormOmission {
                form: form.clone(),
                reason: reason.to_string(),
            })
            .collect::<Vec<_>>()
    };
    if forms.is_empty() {
        return Ok((Vec::new(), Vec::new(), None));
    }
    let policy = match refresh {
        "local" => Refresh::Local,
        "held" => Refresh::Auto {
            max_age_seconds: SURVEY_HELD_FIRST_AGE_SECONDS,
        },
        _ => Refresh::Auto {
            max_age_seconds: hold::DEFAULT_MAX_AGE_SECONDS,
        },
    };
    let root = match ds_layer_store::default_root() {
        Ok(root) => root,
        Err(error) => {
            let reason = survey_layer_unrefreshed_reason(&error);
            return Ok((Vec::new(), omit_all(&reason), None));
        }
    };
    let now = ds_command_kernel::time::OffsetDateTime::now_utc();
    let answer =
        |uid: &str, policy: Refresh, fetch: &mut dyn FnMut(&Value) -> Result<Vec<u8>, Failure>| {
            let scope = ds_command_kernel::project_dataset_cache::Scope {
                principal: uid.to_owned(),
                project: project.to_owned(),
            };
            let mut held = Vec::new();
            let mut omitted = Vec::new();
            for form in forms {
                // A weak link blinks; a refresh the world refused (the fetch's
                // own retryable class) is asked again before the form is written
                // off. The copy is left as it was by every failed attempt.
                let (read, _) = WeakNetwork::FIELD.run(
                || {
                    store::refresh(
                        &root,
                        &scope,
                        form,
                        &Filter::default(),
                        policy,
                        now,
                        |body| fetch(body),
                    )
                },
                |outcome| {
                    matches!(outcome, Err(HoldError::Fetch(failure)) if failure.class().retryable())
                },
            );
                let held_copy = match read {
                    Ok(copy) => store::rows(&root, &scope, &copy.manifest).and_then(|rows| {
                        HeldSurveyForm::new(copy.source.as_str(), &copy.manifest, rows)
                    }),
                    Err(HoldError::NotHeld) => Err(SURVEY_LAYER_NOT_HELD_REASON.to_string()),
                    Err(HoldError::Fetch(failure)) => Err(survey_layer_unrefreshed_reason(
                        &format!("{}: {}", failure.code(), failure.message()),
                    )),
                    Err(HoldError::Refused(message) | HoldError::Store(message)) => {
                        Err(survey_layer_unrefreshed_reason(&message))
                    }
                };
                match held_copy {
                    Ok(copy) => held.push(copy),
                    Err(reason) if reason == SURVEY_LAYER_NOT_HELD_REASON => {
                        omitted.push(SurveyFormOmission {
                            form: form.clone(),
                            reason,
                        })
                    }
                    Err(reason) => omitted.push(SurveyFormOmission {
                        form: form.clone(),
                        reason: if reason.starts_with("survey layer:") {
                            reason
                        } else {
                            survey_layer_unrefreshed_reason(&reason)
                        },
                    }),
                }
            }
            Ok::<_, Failure>((held, omitted))
        };
    let held_only = |reason: &'static str| {
        move |_: &Value| -> Result<Vec<u8>, Failure> {
            Err(Failure::unavailable(
                super::hold::INPUTS_NOT_HELD.code,
                reason,
            ))
        }
    };
    // A held-first print that holds every form reads them without the service,
    // not even a session refresh; a form it does not hold is read below, once.
    let local = if refresh == "held" {
        Some(answer(
            identity.uid(),
            Refresh::Local,
            &mut held_only("held first"),
        )?)
        .filter(|(_, omitted)| omitted.is_empty())
    } else {
        None
    };
    let (mut held, mut omitted) = match local {
        Some(local) => local,
        // With no link the forms are the copies this machine holds: nothing is
        // asked of the service, and no photo can be granted a link.
        None => match link.read(|| {
            ds_cli_auth::survey_hold(
                lane,
                project,
                |uid: &str, fetch: &mut dyn FnMut(&Value) -> Result<Vec<u8>, Failure>| {
                    answer(uid, policy, fetch)
                },
            )
        })? {
            Some(report) => report.into_result(),
            None => answer(
                identity.uid(),
                Refresh::Local,
                &mut held_only("the service could not be reached"),
            )?,
        },
    };
    if held.iter().all(|form| form.rows.is_empty()) {
        return Ok((held, omitted, None));
    }
    // One grant for the batch: the named project's photo tree, for the life
    // a delivered report's links carry.
    let grant = match link.read(|| ds_cli_auth::report_media_grant(lane, project)) {
        Ok(Some(grant)) => grant
            .result()
            .report_grant_document()
            .map_err(|error| error.to_string()),
        Ok(None) => Err(format!(
            "the service could not be reached: {}",
            link.unreachable().unwrap_or("unreachable")
        )),
        Err(failure) => Err(format!("{}: {}", failure.code(), failure.message())),
    };
    match grant {
        Ok(document) => Ok((held, omitted, Some(document))),
        Err(detail) => {
            // Without a grant a photo cannot become a link, and the engine
            // never writes a bare storage path: the held forms are named
            // omitted with the reason, and the sheets are still produced.
            let reason = format!("survey layer: its photo links could not be granted ({detail})");
            omitted.extend(held.drain(..).map(|form| SurveyFormOmission {
                form: form.form,
                reason: reason.clone(),
            }));
            Ok((held, omitted, None))
        }
    }
}

/// What the batch read of the project's survey forms: the refresh mode, the
/// forms held (and how each copy was read) and the forms named omitted.
fn survey_json(
    refresh: &str,
    forms: &[String],
    held: &[ds_command_kernel::report_export::HeldSurveyForm],
    omitted: &[ds_command_kernel::report_export::SurveyFormOmission],
) -> Value {
    json!({
        "refresh": refresh,
        "forms": forms,
        "held": held.iter().map(|form| json!({
            "form": form.form,
            "layer": ds_command_kernel::report_export::survey_layer_name(&form.form),
            "source": form.source,
            "refreshed_at": form.refreshed_at,
            "rows": form.rows.len(),
        })).collect::<Vec<_>>(),
        "omitted": omitted,
    })
}

/// What this host says about itself beside the preview answer: the project
/// receipt, the lane and the scope the inventory resolved.
struct PreviewFacts<'a> {
    project: Value,
    lane: &'a str,
    scope: Value,
    identity: &'a ds_cli_auth::ProviderIdentity,
}

/// The preview run: one page per transformer in scope, each through
/// `ds_report_host::preview::execute` over the room this command admits and
/// the holdings this machine keeps, written to
/// `<out-dir>/<transformer>/<filename>` with the run receipt beside it as
/// `report-run.json`. No batch receipt: a preview is draft state, and the
/// answer is `ds_report_host::preview::answer` — the same `data` the desktop
/// door gives — beside this host's own facts. Rooms are fetched one at a
/// time and pages rendered one at a time; the first refusal ends the run.
#[allow(clippy::too_many_arguments)]
fn preview_pages(
    request: &PreviewRequest,
    capture: Option<&ds_command_kernel::printing::style_capture::Capture>,
    names: &[String],
    settings: &PreviewSettings<'_>,
    holdings_scope: &ds_command_kernel::project_dataset_cache::Scope,
    sheet_positions: &BTreeMap<String, (u32, u32)>,
    out_dir: &Path,
    fetch_room: impl Fn(&str) -> Result<(ds_project_data::room_hold::Room, i64), Failure>,
    facts: PreviewFacts<'_>,
    server_state_dir_ignored: bool,
) -> Result<Value, Failure> {
    // The rooms this machine holds are read, never acquired: without a
    // geographic data root every room-read layer is a named omission on the
    // sheet, which is what a preview is for.
    let holdings = shared_root().ok().map(|root| Holdings {
        catalog: held_catalog_rooms(&root, holdings_scope),
        room_root: ds_layer_store::default_root().ok(),
        scope: holdings_scope.clone(),
        root,
    });
    let staging_failed = |message: String| {
        Failure::failed(STAGING_FAILED.code, message).remedy(STAGING_FAILED.remedy)
    };
    let result_room = ds_project_data::preview_room::Room {
        root: ds_layer_store::default_root().map_err(staging_failed)?,
        scope: holdings_scope.clone(),
        lane: facts.identity.lane().into(),
        credential_audience_sha256: facts.identity.credential_audience_sha256().into(),
    };
    let current = || {
        require_same_context(
            facts.identity,
            &holdings_scope.project,
            &ds_cli_auth::headless_identity_for_named_project(facts.lane)?,
            &holdings_scope.project,
        )
        .map_err(host_failure)
    };
    let selected_engine = CliEngine::capture(facts.identity.lane()).map_err(host_failure)?;
    let mut pages: Vec<(
        PreviewPage,
        PathBuf,
        ds_report_host::preview_artifact::Artifact,
    )> = Vec::with_capacity(names.len());
    for name in names {
        let folder = out_dir.join(name);
        let receipt_path = folder.join(ds_command_kernel::report_export::RUN_RECEIPT_FILE);
        if receipt_path.exists() {
            return Err(Failure::conflict(
                OUTPUT_EXISTS.code,
                format!("{} already holds a report run", folder.display()),
            )
            .remedy(OUTPUT_EXISTS.remedy));
        }
        let (held_room, server_version) = fetch_room(name)?;
        let room = HeldRoom {
            transformer: name.clone(),
            server_version,
            layers: held_room.layers,
            content_digest: held_room.content_digest,
            media_grant: None,
            sheet: sheet_positions.get(name).copied(),
        };
        let held = HeldState {
            room,
            context: HeldContext {
                captures: Vec::new(),
                retained: BTreeMap::new(),
                holdings: holdings.clone(),
            },
        };
        let captured = capture
            .map(|capture| capture.for_layout(&request.layout))
            .transpose()
            .map_err(|error| Failure::invalid(INPUTS_INVALID.code, error))?;
        current()?;
        let (page, artifact) = ds_report_host::execute_artifact_retained(
            &selected_engine,
            settings,
            &ds_report_host::PreviewRequest {
                transformer: name.clone(),
                layout: request.layout.clone(),
            },
            &held,
            captured.as_ref(),
            &result_room,
            request.retention,
        )
        .map_err(preview_failure)?;
        current()?;
        let page_path = folder.join(&page.filename);
        if page_path.exists() {
            return Err(Failure::conflict(
                OUTPUT_EXISTS.code,
                format!("{} already exists", page_path.display()),
            )
            .remedy(OUTPUT_EXISTS.remedy));
        }
        std::fs::create_dir_all(&folder).map_err(|error| {
            staging_failed(format!("could not create {}: {error}", folder.display()))
        })?;
        let page_path = std::fs::canonicalize(&folder)
            .map_err(|e| staging_failed(e.to_string()))?
            .join(&page.filename);
        ds_report_host::preview_artifact::copy(
            &result_room,
            &artifact,
            page_path
                .to_str()
                .ok_or_else(|| staging_failed("preview output path is not UTF-8".into()))?,
        )
        .map_err(host_failure)?;
        let receipt_bytes = serde_json::to_vec_pretty(&page.receipt)
            .map_err(|error| staging_failed(error.to_string()))?;
        std::fs::write(&receipt_path, receipt_bytes).map_err(|error| {
            staging_failed(format!(
                "could not write {}: {error}",
                receipt_path.display()
            ))
        })?;
        pages.push((page, page_path, artifact));
    }
    // Every run removed its own scratch; the empty staging root goes too.
    let _ = std::fs::remove_dir(settings.staging_root);

    let placed: Vec<(&PreviewPage, PageRef<'_>)> = pages
        .iter()
        .map(|(page, path, _)| (page, PageRef::File(path.as_path())))
        .collect();
    let mut answer = ds_report_host::answer(settings.project_id, facts.lane, &placed);
    answer["preview"]["artifact_schema"] = json!(ds_report_host::preview_artifact::SCHEMA);
    for (row, (_, path, artifact)) in answer["results"]
        .as_array_mut()
        .expect("native result rows")
        .iter_mut()
        .zip(&pages)
    {
        row["page"] = serde_json::to_value(artifact).map_err(|e| staging_failed(e.to_string()))?;
        row["out"] = json!(path);
    }
    let mut data = facts.project;
    data["local_print_recipe"] = pages.first().map_or(Value::Null, |(page, _, _)| {
        page.receipt["local_print_recipe"].clone()
    });
    data["out_dir"] = json!(out_dir.display().to_string());
    data["scope"] = facts.scope;
    data["publication_enqueued"] = json!(false);
    // A preview is draft state. It says so in the same words a dry run does,
    // so no receipt anywhere can be mistaken for a publication.
    data["publication"] = json!({
        "stage": "nothing_published",
        "published_nothing": true,
        "state": "dry_run",
        "reason": dry_run_reason(false, false, true),
        "queue_opened": false,
        "server_state_dir_ignored": server_state_dir_ignored,
        "note": "THIS RUN PUBLISHED NOTHING. A preview is a review file; export the governed recipe to publish.",
    });
    for member in ["preview", "results", "batch"] {
        data[member] = answer[member].clone();
    }
    Ok(data)
}

/// The catalogue rows this machine's held rooms stand in for, when the
/// catalogue itself cannot be read: identity and layer only, from the room's
/// own dataset parameters. Never downloadable, never verified — a read-only
/// stand-in that lets held subsets print.
fn held_catalog_rooms(
    root: &Path,
    scope: &ds_command_kernel::project_dataset_cache::Scope,
) -> Vec<ds_project_data::ReferenceResource> {
    let Ok(inventory) = ds_layer_store::project_dataset_cache::project_inventory(
        root,
        &scope.principal,
        &scope.project,
    ) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for room in inventory["rooms"].as_array().into_iter().flatten() {
        if room["provider"] != ds_command_kernel::project_dataset_cache::REFERENCE_CACHE_PROVIDER {
            continue;
        }
        let (Some(id), Some(layer), Some(country)) = (
            room["dataset_id"].as_str(),
            room["parameters"]["layer"].as_str(),
            room["parameters"]["country"].as_str(),
        ) else {
            continue;
        };
        rows.push(ds_project_data::ReferenceResource {
            id: id.to_string(),
            version: room["source_version"].as_str().unwrap_or("").to_string(),
            label: layer.to_string(),
            layer: layer.to_string(),
            country: country.to_string(),
            kind: "bigquery".to_string(),
            active: true,
            ..Default::default()
        });
    }
    rows
}

/// The project-level inputs a batch prints with, as one set: read from the
/// service and held when it answers every read, or this machine's held set,
/// whole, when it cannot be reached. A copy that cannot be kept never stops a
/// print; the receipt says so.
fn project_inputs(
    lane: &str,
    project: &str,
    identity: &ds_cli_auth::ProviderIdentity,
    hold: &super::hold::Hold,
    link: &mut super::hold::Link,
    selection: &DesignOutputSelection,
    held_first: bool,
) -> Result<(super::hold::Inputs, Value), Failure> {
    // The hold is kept per account and project; its CRS capture also records
    // the lane and credential audience it answers. A copy captured for any
    // other scope is not this print's input: the refusal names the member.
    let scoped = |inputs: &super::hold::Inputs| -> Result<(), Failure> {
        let Some(capture) = &inputs.project_crs else {
            return Ok(());
        };
        capture
            .validate_scope(
                project,
                lane,
                identity.uid(),
                identity.credential_audience_sha256(),
                None,
            )
            .map_err(|error| held_scope_refusal(capture, project, lane, identity, &error))
    };
    // A print that publishes nothing reads the held copy when it answers this
    // selection and was captured for this lane and credential; one that does
    // not (another lane's run held it last) is read again, once, below.
    if held_first
        && let Ok(inputs) = hold.inputs("held first")
        && held_inputs_cover(&inputs, selection)
        && scoped(&inputs).is_ok()
    {
        let receipt = json!({"source": "held", "read_at": inputs.read_at, "held_first": true});
        return Ok((inputs, receipt));
    }
    if let Some(mut inputs) = read_inputs(lane, project, identity, link, selection)? {
        // Retain previously admitted optional inputs only beside the unchanged
        // server receipt; use still requires their authenticated/source fences.
        if let Ok(previous) = hold.inputs("retaining optional held inputs")
            && previous.configuration == inputs.configuration
        {
            inputs.transformer_natures = previous.transformer_natures;
            inputs.saved_analyses = previous.saved_analyses;
        }
        let receipt = match hold.hold_inputs(&inputs) {
            Ok(()) => json!({"source": "service", "held": true}),
            Err(error) => json!({"source": "service", "held": false, "hold_error": error}),
        };
        return Ok((inputs, receipt));
    }
    let unreachable = link.unreachable().unwrap_or("unreachable").to_string();
    let inputs = hold.inputs(&unreachable)?;
    scoped(&inputs)?;
    let receipt = json!({"source": "held", "read_at": inputs.read_at, "unreachable": unreachable});
    Ok((inputs, receipt))
}

/// The held inputs cannot serve this print: name the captured member that
/// differs. Values are scope names, never credential material.
fn held_scope_refusal(
    capture: &ds_command_kernel::printing::project_crs::Capture,
    project: &str,
    lane: &str,
    identity: &ds_cli_auth::ProviderIdentity,
    error: &str,
) -> Failure {
    let field = if capture.project != project {
        "project"
    } else if capture.lane != lane {
        "lane"
    } else if capture.principal != identity.uid() {
        "principal"
    } else if capture.credential_audience_sha256 != identity.credential_audience_sha256() {
        "credential_audience_sha256"
    } else {
        "project_params"
    };
    let held_lane = if field == "lane" {
        json!(capture.lane)
    } else {
        Value::Null
    };
    Failure::unavailable(
        super::hold::INPUTS_NOT_HELD.code,
        format!(
            "the held print inputs were captured for another {field} and the service did not answer this print ({error})"
        ),
    )
    .detail(json!({"field": format!("project_crs.{field}"), "lane": lane, "held_lane": held_lane}))
    .remedy(super::hold::INPUTS_NOT_HELD.remedy)
}

/// Refuse an output directory that already holds a batch receipt or a named
/// transformer's folder: the batch would refuse it only after its work.
fn existing_output(out_dir: &Path, names: &[String]) -> Result<(), Failure> {
    let receipt = out_dir.join(ds_command_kernel::report_export::BATCH_RECEIPT_FILE);
    let existing = std::iter::once(receipt)
        .chain(names.iter().map(|name| out_dir.join(name)))
        .find(|path| std::fs::symlink_metadata(path).is_ok());
    match existing {
        Some(path) => Err(Failure::conflict(
            OUTPUT_EXISTS.code,
            format!(
                "{} already exists; choose a fresh --out-dir",
                path.display()
            ),
        )
        .remedy(OUTPUT_EXISTS.remedy)),
        None => Ok(()),
    }
}

/// Whether the held inputs answer `selection`: its named printing setups are
/// sealed in the receipt or held, and the held style capture covers every
/// layout it prints.
fn held_inputs_cover(held: &super::hold::Inputs, selection: &DesignOutputSelection) -> bool {
    let Ok(receipt) = InputReceipt::from_config(&held.configuration) else {
        return false;
    };
    let (Ok(sheets), Ok(named)) = (receipt.sheets(), ds_cli_report_named_setups(selection)) else {
        return false;
    };
    let sealed = sheets.get("printing_setups").is_some();
    if !sealed
        && !named
            .iter()
            .all(|id| held.setups.iter().any(|setup| setup["id"] == id.as_str()))
    {
        return false;
    }
    let Ok(layouts) = effective_capture_layouts(&sheets, &held.setups, &held.rows, selection)
    else {
        return false;
    };
    layouts.is_empty()
        || held.printing_style_capture.as_ref().is_some_and(|capture| {
            layouts
                .iter()
                .all(|layout| capture.for_layout(layout).is_ok())
        })
}

/// Read the governed transformer nature assignments for the rooms this batch
/// will actually run. A project without this active definition has no nature
/// to attach; other refusals remain refusals.
fn project_transformer_natures(
    lane: &str,
    project: &str,
    identity: &ds_cli_auth::ProviderIdentity,
    transformers: &[String],
    scope: &ds_command_kernel::report_export::held::Scope,
) -> Result<ds_command_kernel::report_export::held::TransformerNatures, Failure> {
    let request = ds_cli_auth::DesignTagsCommand::Projection {
        transformers: transformers.to_vec(),
        definitions: vec!["transformer_nature".to_string()],
    };
    let projection = match ds_cli_auth::design_tags(lane, project, &request) {
        Ok(projection) => projection,
        Err(failure) if failure.code() == ds_cli_auth::TAG_DEFINITION_UNKNOWN_REFUSAL.code => {
            return ds_command_kernel::report_export::held::TransformerNatures::new(
                scope.clone(),
                transformers,
                None,
            )
            .map_err(|e| invalid_transformer_nature_projection(&e));
        }
        Err(failure) => return Err(failure),
    };
    require_same_context(
        identity,
        project,
        projection.identity(),
        projection.project_id(),
    )
    .map_err(host_failure)?;
    let document = projection
        .result()
        .get("document")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_transformer_nature_projection("the projection has no document"))?;
    ds_command_kernel::report_export::held::TransformerNatures::new(
        scope.clone(),
        transformers,
        Some(document),
    )
    .map_err(|e| invalid_transformer_nature_projection(&e))
}

fn invalid_transformer_nature_projection(reason: &str) -> Failure {
    Failure::invalid(
        INPUTS_INVALID.code,
        format!("invalid transformer_nature projection: {reason}"),
    )
    .remedy(INPUTS_INVALID.remedy)
}

fn transformer_nature_unavailable(reason: &str) -> Failure {
    Failure::unavailable(
        TRANSFORMER_NATURE_UNAVAILABLE.code,
        format!("could not verify the named project's transformer_nature assignments: {reason}"),
    )
    .remedy(TRANSFORMER_NATURE_UNAVAILABLE.remedy)
}

/// The inputs as the service answers them now, or `None` when it cannot be
/// reached. Every read is taken under the identity the hold is kept for.
fn read_inputs(
    lane: &str,
    project: &str,
    identity: &ds_cli_auth::ProviderIdentity,
    link: &mut super::hold::Link,
    selection: &DesignOutputSelection,
) -> Result<Option<super::hold::Inputs>, Failure> {
    use ds_command_kernel::report_export::{INPUT_RECEIPT_MEMBER, INPUT_RECEIPT_REFUSAL_MEMBER};
    let Some(inventory) = link.read(|| {
        ds_cli_auth::transformer_inventory_for_project(
            lane,
            project,
            &ds_cli_auth::TransformerSet::default(),
        )
    })?
    else {
        return Ok(None);
    };
    require_same_context(
        identity,
        project,
        inventory.identity(),
        inventory.project_id(),
    )
    .map_err(host_failure)?;
    let Some(configuration) =
        link.read(|| ds_cli_auth::feeder_configuration_for_project(lane, project))?
    else {
        return Ok(None);
    };
    require_same_context(
        identity,
        project,
        configuration.identity(),
        configuration.project_id(),
    )
    .map_err(host_failure)?;
    // Only the receipt members are held: they are all a print reads of the
    // configuration, and the refusal member keeps an unusable receipt's
    // sentence when there is one.
    let document = &configuration.result().document;
    let configuration: serde_json::Map<String, Value> =
        [INPUT_RECEIPT_MEMBER, INPUT_RECEIPT_REFUSAL_MEMBER]
            .into_iter()
            .filter_map(|member| {
                document
                    .get(member)
                    .map(|v| (member.to_string(), v.clone()))
            })
            .collect();
    let configuration = Value::Object(configuration);
    let receipt = InputReceipt::from_config(&configuration).map_err(|error| {
        Failure::invalid("report_inputs_invalid", error).remedy(INPUTS_INVALID.remedy)
    })?;
    let Some(setups) = named_printing_setups(lane, project, &receipt, link, selection)? else {
        return Ok(None);
    };
    let Some(directory) = link.read(|| ds_cli_auth::project_directory(lane))? else {
        return Ok(None);
    };
    require_same_context(identity, project, directory.identity(), project).map_err(host_failure)?;
    let project_crs = directory
        .project_params(project)
        .filter(|params| params["crs"].is_object())
        .map(|params| {
            ds_command_kernel::printing::project_crs::Capture::new(
                project,
                lane,
                identity.uid(),
                identity.credential_audience_sha256(),
                params.clone(),
            )
        })
        .transpose()
        .map_err(|e| Failure::invalid("report_inputs_invalid", e).remedy(INPUTS_INVALID.remedy))?;
    let rows = super::hold::rows(inventory.result());
    let source_sheets = receipt
        .sheets()
        .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e))?;
    let captured_layouts = effective_capture_layouts(&source_sheets, &setups, &rows, selection)
        .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e))?;
    let printing_style_capture = if captured_layouts.is_empty() {
        None
    } else {
        let current = ds_cli_auth::headless_identity_for_named_project(lane)?;
        require_same_context(identity, project, &current, project).map_err(host_failure)?;
        let Some(table) = link.read(|| {
            ds_cli_auth::style_governance(
                lane,
                project,
                &ds_command_kernel::style_governance::Command::Table,
            )
        })?
        else {
            return Ok(None);
        };
        let current = ds_cli_auth::headless_identity_for_named_project(lane)?;
        require_same_context(identity, project, &current, project).map_err(host_failure)?;
        let snapshot = serde_json::from_value(table).map_err(|e| {
            Failure::invalid(INPUTS_INVALID.code, format!("print style source: {e}"))
        })?;
        Some(
            ds_command_kernel::printing::style_capture::Capture::new(
                &snapshot,
                &ds_command_kernel::printing::style_capture::Scope {
                    project,
                    lane,
                    principal: identity.uid(),
                    audience: identity.credential_audience_sha256(),
                },
                &receipt.sheets_sha256,
                &source_sheets,
                &captured_layouts,
            )
            .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e))?,
        )
    };
    Ok(Some(super::hold::Inputs {
        project_crs,
        printing_style_capture,
        transformer_natures: None,
        saved_analyses: BTreeMap::new(),
        rows,
        configuration,
        setups,
        read_at: ds_command_kernel::time::OffsetDateTime::now_utc()
            .format(&ds_command_kernel::time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
    }))
}

/// Capture and final rendering share the same effective override precedence.
fn effective_capture_layouts(
    sheets: &Value,
    additional: &[Value],
    rows: &[super::hold::Row],
    selection: &DesignOutputSelection,
) -> Result<Vec<ds_command_kernel::printing::Layout>, String> {
    use ds_command_kernel::printing::{Layout, overrides};
    let selected: BTreeSet<_> = ds_cli_report_named_setups(selection)?.into_iter().collect();
    let settings = sheets["project_settings"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let records: Vec<_> = settings
        .iter()
        .filter(|r| r["parameter"] == "printing_overrides")
        .collect();
    if records.len() > 1 {
        return Err("print_style_capture_invalid: duplicate printing_overrides".into());
    }
    let patches = serde_json::from_value::<overrides::TransformerOverrides>(
        sheets
            .get("printing_overrides")
            .cloned()
            .or_else(|| records.first().map(|r| r["value"].clone()))
            .unwrap_or_else(|| json!({})),
    )
    .map_err(|e| format!("print_style_capture_invalid: {e}"))?;
    let setups = sheets["printing_setups"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(additional);
    let mut layouts = BTreeMap::new();
    for setup in setups {
        if !selected.contains(setup["id"].as_str().unwrap_or_default()) {
            continue;
        }
        let layout: Layout = serde_json::from_value(setup["layout"].clone())
            .map_err(|e| format!("print_style_capture_invalid: {e}"))?;
        layouts.insert(
            ds_command_kernel::printing::style_capture::layout_digest(&layout)?,
            layout.clone(),
        );
        for row in rows
            .iter()
            .filter(|r| r.kind == "transformer" && r.state == "active")
        {
            let effective = overrides::effective(&layout, &row.name, &patches)?;
            layouts.insert(
                ds_command_kernel::printing::style_capture::layout_digest(&effective)?,
                effective,
            );
        }
    }
    Ok(layouts.into_values().collect())
}

/// The printing setups this run selects, read exactly
/// through the printing contract, the way `ds report project settings`
/// completes its sheets, so the kernel decides over the same documents the
/// engine prints with. The sealed sheets carry the export row but not the
/// setups (only the reporter's own input receipt does). `None` when the
/// service could not be reached.
fn named_printing_setups(
    lane: &str,
    project: &str,
    receipt: &InputReceipt,
    link: &mut super::hold::Link,
    selection: &DesignOutputSelection,
) -> Result<Option<Vec<Value>>, Failure> {
    let invalid = |message: String| {
        Failure::invalid("report_inputs_invalid", message).remedy(INPUTS_INVALID.remedy)
    };
    let sheets: Value = serde_json::from_str(&receipt.sheets_json)
        .map_err(|error| invalid(format!("sealed sheets: {error}")))?;
    if sheets.get("printing_setups").is_some() {
        return Ok(Some(Vec::new()));
    }
    let mut setups = Vec::new();
    for id in ds_cli_report_named_setups(selection).map_err(invalid)? {
        let Some(setup) = link.read(|| {
            ds_cli_auth::printing(
                lane,
                false,
                Some(project),
                &ds_cli_auth::PrintingRequest::Get { id: id.clone() },
            )
        })?
        else {
            return Ok(None);
        };
        setups.push(
            json!({"id": setup["id"], "revision": setup["revision"], "layout": setup["layout"]}),
        );
    }
    Ok(Some(setups))
}

/// The context layers the selected printing setups need, decided once by the
/// kernel over the sealed sheets and the setups the batch holds. `online` is
/// exactly `--seed`: acquisition happens before a renderer, never inside one.
fn selected_contexts(
    receipt: &InputReceipt,
    setups: &[Value],
    online: bool,
    selection: &DesignOutputSelection,
) -> Result<
    (
        Vec<ds_command_kernel::printing::PrintContextLayer>,
        Vec<Value>,
    ),
    Failure,
> {
    let invalid = |message: String| {
        Failure::invalid("report_inputs_invalid", message).remedy(INPUTS_INVALID.remedy)
    };
    let mut sheets: Value = serde_json::from_str(&receipt.sheets_json)
        .map_err(|error| invalid(format!("sealed sheets: {error}")))?;
    if sheets.get("printing_setups").is_none() {
        sheets["printing_setups"] = Value::Array(setups.to_vec());
    }
    let request = json!({"command": "context_preparation", "sheets": sheets, "online": online, "selection": selection});
    let bytes = serde_json::to_vec(&request).map_err(|error| invalid(error.to_string()))?;
    let answer: Value =
        serde_json::from_str(&ds_command_kernel::report::evaluate(&bytes).map_err(invalid)?)
            .map_err(|error| invalid(error.to_string()))?;
    let contexts = serde_json::from_value(answer["result"]["contexts"].clone())
        .map_err(|error| invalid(format!("selected context layers: {error}")))?;
    let hidden = answer["result"]["hidden"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok((contexts, hidden))
}

/// A room's saved revision, positive, or the refusal that says to save it.
fn saved_revision(name: &str, version: Option<u64>) -> Result<i64, Failure> {
    version
        .and_then(|version| i64::try_from(version).ok())
        .filter(|version| *version > 0)
        .ok_or_else(|| {
            Failure::invalid(
                INPUTS_INVALID.code,
                format!(
                    "the service reports no saved revision for {name}; save it before reporting"
                ),
            )
            .remedy(INPUTS_INVALID.remedy)
        })
}

/// Only the current run's explicit/default selection names printing inputs.
fn ds_cli_report_named_setups(selection: &DesignOutputSelection) -> Result<Vec<String>, String> {
    let tokens = selection.tokens()?;
    Ok(tokens
        .iter()
        .filter_map(|token| {
            ds_command_kernel::report_formats::named_print_output(token)
                .map(|(_, id)| id.to_owned())
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect())
}

/// A holdings failure for one transformer, as the batch row records it. The
/// crate's own code is kept where this command declares it; the two the
/// document itself can raise are folded under `print_context_invalid`.
fn context_failure(error: ds_project_data::Failure) -> HostFailure {
    use ds_project_data::Failure as Cause;
    let message = error.message().to_string();
    match error {
        Cause::Unsupported(_) => HostFailure::new(CONTEXT_UNSUPPORTED.code, message),
        Cause::BundleUnavailable(_) => HostFailure::new(CONTEXT_BUNDLE.code, message),
        Cause::ProviderUnavailable(_) => HostFailure::new(
            ds_cli_auth::DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL.code,
            message,
        ),
        Cause::AcquisitionFailed(_) => HostFailure::new(CONTEXT_ACQUISITION.code, message),
        Cause::LocalResourceRequired(_) => HostFailure::new(CONTEXT_LOCAL_SOURCE.code, message),
        Cause::CatalogInvalid(_) => HostFailure::new(CONTEXT_CATALOG.code, message),
        // A print read never names a retired row (printing's catalogue
        // selection excludes it), so reaching here is the document's own defect.
        Cause::NotHeld(_) | Cause::TooLarge(_) | Cause::Store(_) | Cause::Retired(_) => {
            HostFailure::new(CONTEXT_INVALID.code, message)
        }
    }
}

/// The batch as one screen, or the preview's pages: a delivery prints its
/// concurrency, engine identity and batch receipt; a preview has none of
/// those and prints its draft's output id and each page's path.
/// The installed engine's identity from one `build-info`, read the way the
/// batch reads it.
fn installed_engine(engine: &dyn ReportEngine) -> Result<EngineIdentity, Failure> {
    let document = engine.build_info().map_err(host_failure)?;
    ds_command_kernel::report_export::engine_identity(&document).map_err(|error| {
        Failure::failed(CONTRACT_MISMATCH.code, error).remedy(CONTRACT_MISMATCH.remedy)
    })
}

/// Why this run consults no reuse evidence, or `None` when it may.
fn reuse_withheld(
    publish: bool,
    force: bool,
    survey_appended: bool,
    evidence_read: bool,
    same_publisher: bool,
) -> Option<&'static str> {
    if force {
        Some("forced")
    } else if !publish {
        // A local run's out-dir is its whole answer; a reused output would
        // be missing from it.
        Some("run_publishes_nothing")
    } else if survey_appended {
        // The input base fingerprints no survey row a report appends.
        Some("survey_append_freshness_unproven")
    } else if !evidence_read {
        // Unreachable, refused, or answered for another account or project.
        Some("status_unread")
    } else if !same_publisher {
        Some("publisher_identity_changed")
    } else {
        None
    }
}

/// One transformer's reuse decision, made at the head revision its status
/// row named: a subset run must read that revision.
#[derive(Debug)]
struct TransformerReuse {
    head_version: i64,
    scope: RunScope,
    outputs: Vec<OutputPlan>,
}

/// Plan every named transformer against its status row. No row, no saved
/// head revision or a row not shaped as ds-brain writes it is no evidence:
/// that transformer is left unplanned (it runs the full policy) and the
/// reason is kept for the receipt.
fn plan_reuse(
    names: &[String],
    rows: &BTreeMap<String, Value>,
    policy: &[String],
    receipt: &InputReceipt,
    engine_build: &str,
) -> (BTreeMap<String, TransformerReuse>, BTreeMap<String, String>) {
    let mut planned = BTreeMap::new();
    let mut unplanned = BTreeMap::new();
    for name in names {
        let attempt = || -> Result<TransformerReuse, String> {
            let row = rows.get(name).ok_or("the status read returned no row")?;
            let head_version = super::hold::head_version(row)
                .as_i64()
                .filter(|version| *version > 0)
                .ok_or("the status row names no saved head revision")?;
            let evidence = reuse::evidence_from_status_row(row)
                .map_err(|error| format!("evidence refused: {error}"))?;
            let input_base = reuse::current_input_base(name, head_version, receipt)?;
            let current = reuse::CurrentInputs {
                input_base_fingerprint: &input_base,
                engine_build_manifest_sha256: engine_build,
            };
            let outputs = reuse::plan_outputs(policy, current, &evidence, false)?;
            Ok(TransformerReuse {
                head_version,
                scope: reuse::run_scope(&outputs)?,
                outputs,
            })
        };
        match attempt() {
            Ok(plan) => {
                planned.insert(name.clone(), plan);
            }
            Err(reason) => {
                unplanned.insert(name.clone(), reason);
            }
        }
    }
    (planned, unplanned)
}

/// Which all-reused claims a status read made after the plan no longer
/// supports, each with why. A claim stands only when that read is this
/// account's and project's answer (`confirming`), names the same head
/// revision, and plans every output again to the same copy from the same
/// run under the engine build installed now. No answer, a missing row, a
/// moved head, a replaced copy or engine: the claim is withdrawn.
fn reconfirm_all_reused(
    plans: &BTreeMap<String, TransformerReuse>,
    confirming: Option<&BTreeMap<String, Value>>,
    policy: &[String],
    receipt: &InputReceipt,
    engine_build: &str,
) -> BTreeMap<String, String> {
    let mut withdrawn = BTreeMap::new();
    for (name, planned) in plans {
        if planned.scope != RunScope::AllReused {
            continue;
        }
        let reason = match confirming {
            None => Some(
                "the confirming status read was unavailable or not this account's and project's answer"
                    .to_owned(),
            ),
            Some(rows) => {
                let (again, refused) = plan_reuse(
                    std::slice::from_ref(name),
                    rows,
                    policy,
                    receipt,
                    engine_build,
                );
                match again.get(name) {
                    None => Some(refused.get(name).cloned().unwrap_or_default()),
                    Some(now) if now.head_version != planned.head_version => Some(format!(
                        "head revision {} became {}",
                        planned.head_version, now.head_version
                    )),
                    Some(now) if now.outputs == planned.outputs => None,
                    Some(now) => Some(
                        now.outputs
                            .iter()
                            .zip(&planned.outputs)
                            .find(|(now, then)| now != then)
                            .map_or_else(
                                || "the planned outputs changed".to_owned(),
                                |(now, _)| match &now.decision {
                                    Decision::Generate { reason } => format!(
                                        "{} must now be generated ({})",
                                        now.output_id,
                                        json!(reason).as_str().unwrap_or_default()
                                    ),
                                    Decision::Reuse { .. } => {
                                        format!("{}'s copy was replaced", now.output_id)
                                    }
                                },
                            ),
                    ),
                }
            }
        };
        if let Some(reason) = reason {
            withdrawn.insert(
                name.clone(),
                format!("reuse withdrawn at confirmation: {reason}"),
            );
        }
    }
    withdrawn
}

/// The selection one transformer's run hands the engine, and whether its
/// room moved: the planned subset while the room is the revision the plan
/// was made at, otherwise the full policy so no reused copy sits beside
/// outputs of a newer revision.
fn run_selection(
    plan: Option<&TransformerReuse>,
    server_version: i64,
) -> (Option<DesignOutputSelection>, bool) {
    match plan {
        Some(TransformerReuse {
            head_version,
            scope: RunScope::Subset(selection),
            ..
        }) if *head_version == server_version => (Some(selection.clone()), false),
        Some(TransformerReuse {
            scope: RunScope::Subset(_),
            ..
        }) => (None, true),
        _ => (None, false),
    }
}

/// What the reuse plan decided, one compact row per planned transformer:
/// reused copies with their bytes and producing run, and the generated
/// outputs grouped by reason.
fn reuse_receipt(
    withheld: Option<&str>,
    engine: Option<&EngineIdentity>,
    plans: &BTreeMap<String, TransformerReuse>,
    unplanned: &BTreeMap<String, String>,
    room_moved: &BTreeMap<String, i64>,
) -> Value {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut rows = Vec::with_capacity(plans.len());
    for (name, plan) in plans {
        let moved = room_moved.get(name);
        let mut reused = Vec::new();
        let mut generate: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        for output in &plan.outputs {
            let reason = match (&output.decision, moved) {
                (Decision::Reuse { sha256, work_id }, None) => {
                    reused.push(json!({
                        "output_id": output.output_id,
                        "sha256": sha256,
                        "work_id": work_id,
                    }));
                    continue;
                }
                (Decision::Reuse { .. }, Some(_)) => "room_moved".to_owned(),
                (Decision::Generate { reason }, _) => {
                    json!(reason).as_str().unwrap_or_default().to_owned()
                }
            };
            generate.entry(reason).or_default().push(&output.output_id);
        }
        let scope = match (&plan.scope, moved) {
            (_, Some(_)) => "full_room_moved",
            (RunScope::Full, None) => "full",
            (RunScope::Subset(_), None) => "subset",
            (RunScope::AllReused, None) => "all_reused",
        };
        *counts.entry(scope).or_default() += 1;
        let mut row = json!({
            "transformer": name,
            "scope": scope,
            "head_version": plan.head_version,
            "reused": reused,
            "generate": generate,
        });
        if let Some(version) = moved {
            row["room_version"] = json!(version);
        }
        rows.push(row);
    }
    json!({
        "mode": match withheld {
            None => "incremental",
            Some("forced") => "forced",
            Some(_) => "not_applied",
        },
        "withheld": withheld,
        "engine_build_manifest_sha256": engine.map(|engine| engine.build_manifest_sha256.as_str()),
        "counts": counts,
        "transformers": rows,
        "no_evidence": unplanned,
    })
}

pub fn render(data: &Value) -> String {
    let preview = data["preview"].is_object();
    let mut out = format!(
        "project {} ({}) · {} · batch {} · {} completed · {} failed",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["batch"]["status"].as_str().unwrap_or("?"),
        data["batch"]["completed"].as_u64().unwrap_or(0),
        data["batch"]["failed"].as_u64().unwrap_or(0),
    );
    // A run that delivered its artifacts and still lost a format counts as
    // completed. If this screen said only "86 completed" an operator would
    // ship a sheet set with 59 sheets missing and never be told: the loss has
    // to be as visible as the delivery.
    let partial = data["batch"]["partial_formats"].as_u64().unwrap_or(0);
    if partial > 0 {
        out.push_str(&format!(" · {partial} partial"));
    }
    // Whether this run's work left the machine is not a detail to be found in
    // JSON. An operator who reads only this line must not be able to believe
    // a dry run published something.
    if data["publication"]["published_nothing"] == Value::Bool(true) {
        out.push_str(" · PUBLISHED NOTHING");
    } else if data["publication"]["stage"].is_string() {
        out.push_str(&format!(
            " · publication {}",
            data["publication"]["stage"].as_str().unwrap_or("?")
        ));
    }
    if preview {
        out.push_str(&format!(
            "\n  preview {} (layout {})\n",
            data["preview"]["output_id"].as_str().unwrap_or("?"),
            data["preview"]["layout_id"].as_str().unwrap_or("?"),
        ));
    } else {
        out.push_str(&format!(
            " · {} at once\n  engine {} ({})\n  {}\n",
            data["batch"]["concurrency"].as_u64().unwrap_or(0),
            data["engine"]["engine_version"].as_str().unwrap_or("?"),
            data["engine"]["publication_state"].as_str().unwrap_or("?"),
            data["batch"]["receipt"].as_str().unwrap_or(""),
        ));
    }
    // Every lost format is named, and the naming is bounded: past this many
    // the screen says how many it did not print and where the rest are.
    const MAX_LOST_LISTED: usize = 10;
    let mut listed = 0_usize;
    let mut more = 0_usize;
    for row in data["results"].as_array().into_iter().flatten() {
        let name = row["transformer"].as_str().unwrap_or("?");
        let lost = row["failed_formats"].as_array().map_or(&[][..], |l| l);
        if let Some(path) = row["page"]["path"].as_str() {
            out.push_str(&format!("  ok     {name:<28} page  {path}\n"));
        } else if row["status"] == "ok" && !lost.is_empty() {
            // The artifacts are delivered and publishable, so this is not an
            // error row — but it is not `ok` either, and it says what is
            // missing and which member of the printing setup decides it.
            // One column narrower than `ok`/`error` so the columns line up.
            out.push_str(&format!(
                "  partial {name:<27} {} artifact(s), {} lost  {}\n",
                row["artifacts"].as_u64().unwrap_or(0),
                lost.len(),
                row["receipt"].as_str().unwrap_or(""),
            ));
            for format in lost {
                if listed >= MAX_LOST_LISTED {
                    more += 1;
                    continue;
                }
                listed += 1;
                let knob = &format["layout"];
                let member = match (knob["knob"].as_str(), knob["element_id"].as_str()) {
                    (Some(knob), Some(element)) => format!(" · {knob} on {element}"),
                    (Some(knob), None) => format!(" · {knob}"),
                    _ => String::new(),
                };
                out.push_str(&format!(
                    "           lost {} · {}{}\n             {}\n             fix: {}\n",
                    format["output_id"].as_str().unwrap_or("?"),
                    format["code"].as_str().unwrap_or("?"),
                    member,
                    knob["detail"]
                        .as_str()
                        .or_else(|| format["message"].as_str())
                        .unwrap_or(""),
                    format["remedy"].as_str().unwrap_or(""),
                ));
            }
        } else if row["status"] == "ok" {
            out.push_str(&format!(
                "  ok     {name:<28} {} artifact(s)  {}\n",
                row["artifacts"].as_u64().unwrap_or(0),
                row["receipt"].as_str().unwrap_or(""),
            ));
        } else {
            out.push_str(&format!(
                "  error  {name:<28} {}: {}\n",
                row["error"]["code"].as_str().unwrap_or("?"),
                row["error"]["message"].as_str().unwrap_or(""),
            ));
        }
        if let Some(layers) = row["survey_layers"].as_array() {
            let carried = layers
                .iter()
                .filter_map(|layer| {
                    Some(format!(
                        "{} ({})",
                        layer["form"].as_str()?,
                        layer["features"].as_u64()?
                    ))
                })
                .collect::<Vec<_>>();
            if !carried.is_empty() {
                out.push_str(&format!(
                    "           survey appended: {}\n",
                    carried.join(", ")
                ));
            }
        }
        if let Some(forms) = row["survey_layers_omitted"]["forms"].as_array() {
            let names = forms.iter().filter_map(Value::as_str).collect::<Vec<_>>();
            if !names.is_empty() {
                out.push_str(&format!(
                    "           survey omitted: {}\n",
                    names.join(", ")
                ));
            }
        }
    }
    if more > 0 {
        out.push_str(&format!(
            "  more   {more} further lost format(s) — every one is in \
             results[].failed_formats and in each run's report-run.json\n"
        ));
    }
    out
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "export_reuse_tests.rs"]
mod reuse_wiring;
