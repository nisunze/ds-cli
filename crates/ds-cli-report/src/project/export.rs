//! `ds report project export` — individual transformer reports, prints
//! included, from the governed inputs and the installed engine.
//!
//! No map, no room cache, no Desktop. The native user's audience-fenced
//! selected project supplies two things through fixed doors: the Network
//! Reporter input receipt ds-brain mints beside the project configuration
//! (country, the exact settings sheets, the reference snapshot), and each
//! transformer's exact saved layers with their revision. The installed
//! `ds-report` engine produces the artifacts locally — every geospatial and
//! tabular output the project's policy names, and every named print output
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
//! `--preview-layout` is the other state regime: draft state, not an
//! artifact. Each page is `ds_report_host::preview::execute` — the one
//! execution the desktop door reaches too — over the room this command
//! admits from the service answer and the holdings this machine keeps; the
//! answer is `ds_report_host::preview::answer`, so both doors give the same
//! `data` under the same envelope. A preview never seeds, never projects MV,
//! and writes one page with its run receipt per transformer, no batch receipt.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ds_cli_auth::{TransformerKind, TransformerLifecycle};
use ds_cli_contract::outcome::{ExitClass, Failure};
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::{
    envelope::Class,
    report::PublicationState,
    report_export::{InputReceipt, reportable_transformer},
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

use super::{LANE_ARG, TRANSFORMER_ARG};
use crate::{DISCOVERY_TIMEOUT, DS_REPORT, EXPORT_TIMEOUT};

const OUT_DIR_ARG: Arg = Arg::value(
    "out-dir",
    "<path>",
    "Fresh directory to write into: one folder per transformer plus the batch receipt.",
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
    "Acquire the printed transformer's missing map context first (provider cost); without it unheld context is omitted and named.",
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
const SERVER_STATE_DIR_ARG: Arg = Arg::value(
    "server-state-dir",
    "<absolute-path>",
    "Matching `ds server serve --state-dir` directory when the Server uses a custom state root.",
);

/// The staging directory a batch keeps below its output root while engines
/// run; each run's scratch is removed when it ends.
const STAGING_DIRECTORY: &str = ".staging";

const ENGINE_MISSING: Refusal = Refusal {
    code: "reporter_engine_missing",
    when: "`ds-report` is not installed next to `ds`",
    remedy: "install the desktop or headless package, or set DS_REPORT_BIN",
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
const INPUTS_INVALID: Refusal = Refusal {
    code: "report_inputs_invalid",
    when: "the input receipt, a transformer's saved layers or the output policy cannot be run as given",
    remedy: "run `ds report project settings`; it names the missing input and the repair",
};
const STAGING_FAILED: Refusal = Refusal {
    code: "report_staging_failed",
    when: "the output directory or its staging area could not be prepared",
    remedy: "check that --out-dir is writable and has space",
};
const OUTPUT_EXISTS: Refusal = Refusal {
    code: "report_output_exists",
    when: "--out-dir already holds a batch receipt or that transformer's folder",
    remedy: "choose a fresh --out-dir; a report is never overwritten",
};
const RESULT_INVALID: Refusal = Refusal {
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
    remedy: "install it from the desktop's Geographic Data page, or pass --admin-bounds",
};
const CONCURRENCY: Refusal = Refusal {
    code: "invalid_concurrency",
    when: "--concurrency is not a whole number from 1 through 64",
    remedy: "pass --concurrency between 1 and 64, or omit it",
};
const BATCH_EMPTY: Refusal = Refusal {
    code: "report_batch_empty",
    when: "the selected project has no active transformer to report",
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
const PUBLISH_LOCAL_ONLY: Refusal = Refusal {
    code: "report_publish_local_only",
    when: "a development reporter build produced artifacts, and a development build may not publish",
    remedy: "run a release reporter build, or pass --dry-run to accept local-only files",
};
const PUBLISH_SCOPE_CHANGED: Refusal = Refusal {
    code: "report_publish_scope_changed",
    when: "the native identity, lane, audience, project or credential generation changed before sealing",
    remedy: "repeat the export under the current native account and project",
};
const PUBLISH_ROOT: Refusal = Refusal {
    code: "report_publish_root_invalid",
    when: "the Server state root is unavailable, relative, or cannot hold a sealed publication",
    remedy: "use Server's default state root, or the same absolute --server-state-dir as ds server serve",
};
const CONTEXT_UNSUPPORTED: Refusal = Refusal {
    code: "print_context_unsupported",
    when: "a setup names a survey or local-layer context source (desktop-held; batch row)",
    remedy: "print that setup from the desktop, or drop the source from the setup",
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
const CONTEXT_TOO_LARGE: Refusal = Refusal {
    code: "print_context_too_large",
    when: "a preview's held context exceeds the engine's bounds",
    remedy: "narrow the draft's context buffers, or drop a context layer",
};
const HOLDINGS_STORE: Refusal = Refusal {
    code: "project_dataset_store_failed",
    when: "a preview could not read the geographic holdings this machine keeps",
    remedy: "`ds data project-cache status`; repair the geographic data root",
};

pub(super) const REFUSALS: &[Refusal] = &[
    super::NATIVE_PROFILE,
    super::NATIVE_PROFILE_DIGEST,
    super::NATIVE_PROFILE_UNSAFE,
    super::HEADLESS_SIGNED_OUT,
    super::HEADLESS_NO_PROJECT,
    super::PROJECT_CONTEXT_STALE,
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
    super::AUTH_UNREADABLE,
    super::NOT_FOUND,
    super::INVALID_SCOPE,
    super::RESERVED_IDENTITY,
    ENGINE_MISSING,
    CONTRACT_MISMATCH,
    TIMED_OUT,
    ENGINE_REFUSED,
    EXPORT_BLOCKED,
    INPUTS_INVALID,
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
    CONTEXT_TOO_LARGE,
    HOLDINGS_STORE,
    ds_cli_auth::DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL,
];

pub static COMMAND: Command = Command {
    id: "report.project.export",
    path: &["report", "project", "export"],
    contract: 1,
    summary: "Export all transformer reports and maps headlessly in parallel.",
    purpose: "Export active transformers and named print outputs with project-wide numbering, and PUBLISH them: every artifact enters the one publication queue. --dry-run is the only unpublished mode; its receipt says so. Setups use held context; --seed acquires missing context. Photos require a media grant.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        OUT_DIR_ARG,
        CONCURRENCY_ARG,
        ADMIN_BOUNDS_ARG,
        Arg::repeated(
            "print-layout",
            "<json-file>",
            "Local proof replacement for an existing project layout; preserves engineering inputs and paper identity, records source/recipe digests, and cannot publish.",
        ),
        Arg::value(
            "preview-layout",
            "<json-file>",
            "One SVG preview per transformer, with outlined text and template pens; any draft id or paper. Cannot publish or combine with --print-layout.",
        ),
        Arg::value(
            "context-vectors",
            "<dir>",
            "Verified data.city-vectors output to use as this batch’s OSM/Microsoft map context.",
        ),
        SEED_ARG,
        DRY_RUN_ARG,
        SERVER_STATE_DIR_ARG,
        LANE_ARG,
    ],
    output: "\
Lane, project, scope, engine identity, publication state, batch counts and receipt \
(partial_formats), context diagnostics and ordered transformer results: artifact \
inventory, failed_formats (output_id, code, remedy, layout knob: \
overflow/panels/row_mm), or typed error. `publication.stage` is `queued`, or \
`nothing_published` for a dry run.",
    examples: &[
        Example {
            command: "ds report project export --out-dir ./reports --output json",
            note: "Every active transformer, published; `.data.results[]` says what each did.",
            runnable: false,
        },
        Example {
            command: "ds report project export --transformer tx_a --transformer tx_b --out-dir ./reports --concurrency 2",
            note: "Two named transformers, two engines at once.",
            runnable: false,
        },
        Example {
            command: "ds report project export --transformer tx_a --out-dir ./reports --dry-run --output json",
            note: "Local files only; `.data.publication.published_nothing` is true.",
            runnable: false,
        },
        Example {
            command: "ds report project export --transformer tx_a --preview-layout ./draft.json --out-dir ./preview --output json",
            note: "tx_a's sheet as the draft composes it, with its pens, as one SVG page; `.data.preview` names the output.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &[],
    requires: Requires::Server,
    availability,
};

/// Both the native user client and the installed engine must be present.
fn availability() -> Availability {
    match ds_cli_auth::native_availability() {
        Availability::Available => DS_REPORT.availability(),
        unavailable => unavailable,
    }
}

/// `ds-report`, reached through the audited process boundary: one static
/// subcommand per method, typed paths as arguments, never an argument vector
/// from the host crate.
struct CliEngine;

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

/// The pauses between attempts when the network blinks: the desktop's own
/// tolerance for a weak link, so a batch of eighty sheets does not lose
/// seventy rows to one dropped second.
pub(super) const WEAK_NETWORK_DELAYS: &[std::time::Duration] = &[
    std::time::Duration::from_secs(2),
    std::time::Duration::from_secs(6),
    std::time::Duration::from_secs(15),
];

/// Retry `op` after each delay while its refusal is retryable (`unavailable`
/// or `conflict` — the world, not the request, has to change); any other
/// refusal, and the last retryable one, is returned as it came.
pub(super) fn with_weak_network<T>(
    delays: &[std::time::Duration],
    mut op: impl FnMut() -> Result<T, Failure>,
) -> Result<T, Failure> {
    let mut attempt = 0;
    loop {
        match op() {
            Ok(value) => return Ok(value),
            Err(failure) if failure.class().retryable() && attempt < delays.len() => {
                std::thread::sleep(delays[attempt]);
                attempt += 1;
            }
            Err(failure) => return Err(failure),
        }
    }
}

fn failure_to_host(failure: Failure) -> HostFailure {
    HostFailure {
        code: failure.code().to_string(),
        message: failure.message().to_string(),
        detail: failure.detail_value().cloned(),
    }
}

impl ReportEngine for CliEngine {
    fn build_info(&self) -> Result<Value, HostFailure> {
        DS_REPORT
            .call_json("build-info", &[], DISCOVERY_TIMEOUT)
            .map_err(failure_to_host)
    }

    fn export_transformer_report(
        &self,
        request: &Path,
        result: &Path,
    ) -> Result<EngineExit, HostFailure> {
        let args: Vec<OsString> = vec![
            OsString::from("--request"),
            request.into(),
            OsString::from("--result"),
            result.into(),
        ];
        let completed = DS_REPORT
            .call("export-transformer-report", &args, EXPORT_TIMEOUT)
            .map_err(failure_to_host)?;
        Ok(EngineExit {
            succeeded: completed.succeeded(),
            summary: bounded_summary(&completed.stderr, &completed.stdout),
        })
    }
}

/// A host failure that ends the whole command, in the CLI's own classes.
/// Every arm is a literal so the code is documented above.
fn host_failure(failure: HostFailure) -> Failure {
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
            Failure::unavailable("reporter_engine_missing", message).remedy(DS_REPORT.remedy)
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

fn require_same_context(
    expected_identity: &ds_cli_auth::ProviderIdentity,
    expected_project: &str,
    actual_identity: &ds_cli_auth::ProviderIdentity,
    actual_project: &str,
) -> Result<(), HostFailure> {
    if expected_identity != actual_identity || expected_project != actual_project {
        return Err(HostFailure::new(
            INPUTS_INVALID.code,
            "account, deployment audience or selected project changed while fetching report inputs; start a new batch",
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
fn seal_run_for_server(
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
        transformer_revision: receipt_revision(&run.receipt)?,
        input_base_fingerprint: receipt_text(&run.receipt, "input_base_fingerprint")?,
        room_content_sha256: receipt_text(&run.receipt, "room_content_sha256")?,
        client_run_id: &run.client_run_id,
        artifacts: &held,
        expected_formats: &formats,
        deadline,
        guard: &scope_guard,
    };
    match ds_sync_runtime::reports::seal(&queue.store, &queue.fence, &queue.root, &request) {
        Ok(SealOutcome::Sealed(sealed)) => Ok(SealedRun::Queued(Box::new(sealed))),
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
fn complete_proof_print_styles(
    lane: &str,
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
    let snapshot = ds_cli_auth::style_catalog(lane)?;
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
            .filter(|icon| !icon.is_empty())
        {
            if !sealed_symbols[icon].is_object() {
                unsealed_symbols.push(format!("{icon} (bound by {reference})"));
            }
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

fn verify_publish_scope(
    lane: &str,
    fence: &ds_cli_auth::LayerScopeFence,
    owner_uid: &str,
    project_id: &str,
) -> Result<(), Failure> {
    ds_cli_auth::verify_layer_scope_fence(lane, fence, owner_uid, project_id).map_err(|_| {
        Failure::conflict(
            PUBLISH_SCOPE_CHANGED.code,
            "the native publication scope changed before report artifacts could be sealed",
        )
        .remedy(PUBLISH_SCOPE_CHANGED.remedy)
    })
}

/// What `--preview-layout` asks for, read and bounded before any state is
/// touched: the draft.
struct PreviewRequest {
    layout: ds_command_kernel::printing::Layout,
}

fn preview_request(inputs: &Inputs, proofs: bool) -> Result<Option<PreviewRequest>, Failure> {
    let Some(path) = inputs.value("preview-layout") else {
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
    Ok(Some(PreviewRequest { layout }))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
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
    // A preview's own refusals are decided before a city-vectors directory
    // is read, so they are local whatever that directory holds.
    let preview_request = preview_request(inputs, !proof_paths.is_empty())?;
    let proofs_requested = !proof_paths.is_empty();
    let preview_requested = preview_request.is_some();
    let publish = !dry_run && !proofs_requested && !preview_requested;
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
    let publish_scope = publish
        .then(|| ds_cli_auth::capture_layer_scope_fence(lane))
        .transpose()?;
    let publish_queue = if publish {
        Some(PublicationQueue::open(
            lane,
            inputs.value("server-state-dir").map(Path::new),
        )?)
    } else {
        if inputs.value("server-state-dir").is_some() {
            return Err(Failure::invalid(
                "report_inputs_invalid",
                "--server-state-dir names the queue a publication enters, and this run publishes nothing",
            )
            .remedy("drop --dry-run (or --print-layout/--preview-layout), or remove --server-state-dir"));
        }
        None
    };

    // The lifecycle inventory is both the project identity and the scope:
    // every active saved transformer, or the exact names given with the state
    // each one is in.
    let inventory = ds_cli_auth::transformer_inventory(lane, &requested)?;
    let project_id = inventory.project_id().to_string();
    if let Some(fence) = publish_scope.as_ref() {
        verify_publish_scope(lane, fence, fence.uid(), &project_id)?;
    }
    let mut output = super::project_receipt(&inventory);
    let scope = super::scope_json(&requested, inventory.result());
    let mut lifecycle: BTreeMap<String, String> = BTreeMap::new();
    let mut active = Vec::new();
    for row in inventory.result().rows() {
        let state = if row.kind() != TransformerKind::Transformer {
            "project_level".to_string()
        } else {
            row.lifecycle().token().to_string()
        };
        if row.kind() == TransformerKind::Transformer
            && row.lifecycle() == TransformerLifecycle::Active
            && reportable_transformer(row.name()).is_ok()
        {
            active.push(row.name().to_string());
        }
        lifecycle.insert(row.name().to_string(), state);
    }
    let names: Vec<String> = if requested.is_empty() {
        active
    } else {
        requested.names().to_vec()
    };
    if names.is_empty() {
        return Err(Failure::conflict(
            "report_batch_empty",
            "the selected project has no active transformer to report",
        )
        .remedy(BATCH_EMPTY.remedy)
        .next("ds report project scope"));
    }

    // The project-wide input base: the receipt ds-brain mints beside the
    // fresh configuration. The kernel proves it before anything is staged.
    let configuration = ds_cli_auth::feeder_configuration_receipt(lane, None)?;
    require_same_context(
        inventory.identity(),
        &project_id,
        configuration.identity(),
        configuration.project_id(),
    )
    .map_err(host_failure)?;
    let receipt = InputReceipt::from_config(&configuration.result().document).map_err(|error| {
        Failure::invalid("report_inputs_invalid", error).remedy(INPUTS_INVALID.remedy)
    })?;
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
        let proof = ds_command_kernel::report_export::proof::with_layouts(&receipt, &layouts)
            .map_err(|e| Failure::invalid(INPUTS_INVALID.code, e))?;
        complete_proof_print_styles(lane, proof, &layouts)?
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
            let snapshot = ds_cli_auth::style_catalog(lane)?;
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
    let contexts = if local_context.is_some() || preview_request.is_some() {
        Vec::new()
    } else {
        selected_contexts(lane, &receipt, seed)?
    };
    let mv_buffer = contexts
        .iter()
        .filter_map(|c| match &c.source {
            ds_command_kernel::printing::PrintContextSource::ProjectDsgridMv {
                buffer_m, ..
            } => Some(*buffer_m),
            _ => None,
        })
        .reduce(f64::max);
    let mv_models = if mv_buffer.is_some() {
        super::mv_context::load(lane, inventory.identity(), &project_id)?
    } else {
        Vec::new()
    };
    output["mv_context_models"] = json!(mv_models.len());
    output["mv_context_sources"] = json!(super::mv_context::provenance(&mv_models));
    let mut context_warnings: Vec<Value> = Vec::new();
    let catalog: Vec<ds_project_data::ReferenceResource> = if contexts.is_empty() {
        Vec::new()
    } else {
        match ds_cli_auth::data_distribution(
            lane,
            &ds_cli_auth::DataDistributionRequest::ListDatasets {},
        )
        .map_err(|error| format!("{}: {}", error.code(), error.message()))
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
        principal: inventory.identity().uid().to_string(),
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
    let mut provider = ds_cli_data::project_cache::CliProvider { lane };
    let mut bundle_fetch = ds_cli_data::project_cache::bundle_fetch(lane);
    let mut transformer_context_notes: Vec<Value> = Vec::new();

    // Number the complete active inventory even for an explicitly selected subset.
    let complete_inventory = if requested.is_empty() {
        None
    } else {
        let full =
            ds_cli_auth::transformer_inventory(lane, &ds_cli_auth::TransformerSet::default())?;
        require_same_context(
            inventory.identity(),
            &project_id,
            full.identity(),
            full.project_id(),
        )
        .map_err(host_failure)?;
        Some(full)
    };
    let drawing_names = complete_inventory
        .as_ref()
        .unwrap_or(&inventory)
        .result()
        .rows()
        .iter()
        .filter(|row| {
            row.kind() == TransformerKind::Transformer
                && row.lifecycle() == TransformerLifecycle::Active
        })
        .map(|row| row.name().to_string())
        .collect::<Vec<_>>();
    let sheet_positions = ds_command_kernel::report_export::drawing_set_positions(&drawing_names)
        .map_err(|error| {
        Failure::invalid(INPUTS_INVALID.code, error).remedy(INPUTS_INVALID.remedy)
    })?;

    let processors = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let plan = batch_plan(&names, processors, resident_limit).map_err(|error| {
        Failure::invalid("report_inputs_invalid", error).remedy(INPUTS_INVALID.remedy)
    })?;

    let staging = out_dir.join(STAGING_DIRECTORY);
    // One transformer's room as the service answers it and this command
    // admits it: an active saved transformer of this project, fetched under
    // the batch's identity, with a positive saved revision. The refusal keeps
    // the CLI's own class, code and remedy: a preview ends with it as the
    // command's answer, a delivery records it as one row of the batch.
    let fetch_room =
        |name: &str| -> Result<(ds_cli_auth::HeadlessTransformerContext, i64), Failure> {
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
            // A weak link blinks; a room fetch that was refused by an outage is
            // asked again before the row is written off.
            let context = with_weak_network(WEAK_NETWORK_DELAYS, || {
                ds_cli_auth::transformer_context(lane, name)
            })?;
            require_same_context(
                inventory.identity(),
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
            let server_version = snapshot
                .metadata()
                .version()
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
                })?;
            Ok((context, server_version))
        };

    if let Some(request) = preview_request {
        return preview_pages(
            &request,
            &plan.names,
            &PreviewSettings {
                project_id: &project_id,
                receipt: &receipt,
                admin_bounds: admin_bounds.as_ref(),
                staging_root: &staging,
            },
            &holdings_scope,
            &sheet_positions,
            &out_dir,
            fetch_room,
            PreviewFacts {
                project: super::project_receipt(&inventory),
                lane,
                scope,
            },
        );
    }

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
    };
    let fetch = |name: &str| -> Result<TransformerReportInputs, HostFailure> {
        let (context, server_version) = fetch_room(name).map_err(failure_to_host)?;
        let snapshot = context.snapshot();
        let print_context = if let Some(context) = &local_context {
            ds_project_data::city_vectors::require_design_coverage(
                context,
                name,
                &serde_json::to_value(snapshot.layers())
                    .map_err(|error| HostFailure::new(INPUTS_INVALID.code, error.to_string()))?,
            )
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
                    let layers_value =
                        serde_json::to_value(snapshot.layers()).map_err(|error| {
                            HostFailure::new(INPUTS_INVALID.code, error.to_string())
                        })?;
                    let mut hosts = ds_project_data::Hosts {
                        provider: &mut provider,
                        fetch: &mut bundle_fetch,
                    };
                    let mode = if seed {
                        ds_project_data::Mode::Acquire(&mut hosts)
                    } else {
                        ds_project_data::Mode::Read
                    };
                    let context = ds_project_data::read_print_context(
                        root,
                        &holdings_scope,
                        name,
                        &layers_value,
                        &contexts,
                        &catalog,
                        mode,
                    )
                    .map_err(context_failure)?;
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
            super::mv_context::attach(
                print_context,
                &mv_models,
                name,
                &serde_json::to_value(snapshot.layers())
                    .map_err(|e| HostFailure::new(INPUTS_INVALID.code, e.to_string()))?,
                buffer,
            )
            .map_err(failure_to_host)?
        } else {
            print_context
        };
        let sheet = sheet_positions.get(name).copied();
        Ok(TransformerReportInputs {
            transformer: name.to_string(),
            server_version,
            layers: snapshot.layers().clone(),
            content_digest: snapshot.metadata().content_digest().map(str::to_string),
            selection: None,
            print_context,
            sheet,
        })
    };
    let outcome = run_batch(&CliEngine, &settings, &plan.names, fetch).map_err(host_failure)?;
    // Every run removed its own scratch; the empty staging root goes too.
    let _ = std::fs::remove_dir(&staging);
    if outcome.receipt["status"] == "failed" {
        return Err(
            Failure::failed("report_batch_failed", "no transformer report completed")
                .remedy(BATCH_FAILED.remedy)
                .detail(json!({
                    "out_dir": out_dir.display().to_string(),
                    "results": outcome.receipt["results"],
                })),
        );
    }

    let publication_state = if receipt.local_print_recipe.is_some() {
        Some(ds_command_kernel::report::PublicationState::LocalOnly)
    } else {
        outcome.engine.publication_state().ok()
    };
    let sealed_publications = if let (Some(fence), Some(queue)) =
        (publish_scope.as_ref(), publish_queue.as_ref())
    {
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
    output["publication_enqueued"] = json!(publish);
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
        _ => json!({
            "stage": "nothing_published",
            "published_nothing": true,
            "state": "dry_run",
            "reason": dry_run_reason(dry_run, proofs_requested, preview_requested),
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
    // What each completed print carried from outside its room, from the run
    // receipts the host wrote: the digest the engine verified, the layers, and
    // the selected layers this machine could not supply.
    if let Some(rows) = output["results"].as_array_mut() {
        for row in rows.iter_mut() {
            let Some(name) = row["transformer"].as_str().map(str::to_string) else {
                continue;
            };
            if let Some(run) = outcome.runs.iter().find(|run| run.transformer == name) {
                row["print_context"] = json!({
                    "sha256": run.receipt["print_context_sha256"],
                    "layers": run.receipt["print_context_layers"],
                    "omitted": run.receipt["print_context_omitted"],
                });
            }
        }
    }
    output["context"] = json!({
        "selected_layers": local_context.as_ref().map_or_else(
            || contexts.iter().map(|layer| layer.id.clone()).collect::<Vec<_>>(),
            |context| context.layers.clone(),
        ),
        "seeded": seed,
        "catalog_resources": catalog.len(),
        "warnings": context_warnings,
        "notes": transformer_context_notes,
    });
    Ok(output)
}

/// What this host says about itself beside the preview answer: the project
/// receipt, the lane and the scope the inventory resolved.
struct PreviewFacts<'a> {
    project: Value,
    lane: &'a str,
    scope: Value,
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
    names: &[String],
    settings: &PreviewSettings<'_>,
    holdings_scope: &ds_command_kernel::project_dataset_cache::Scope,
    sheet_positions: &BTreeMap<String, (u32, u32)>,
    out_dir: &Path,
    fetch_room: impl Fn(&str) -> Result<(ds_cli_auth::HeadlessTransformerContext, i64), Failure>,
    facts: PreviewFacts<'_>,
) -> Result<Value, Failure> {
    // The rooms this machine holds are read, never acquired: without a
    // geographic data root every room-read layer is a named omission on the
    // sheet, which is what a preview is for.
    let holdings = shared_root().ok().map(|root| Holdings {
        catalog: held_catalog_rooms(&root, holdings_scope),
        scope: holdings_scope.clone(),
        root,
    });
    let staging_failed = |message: String| {
        Failure::failed(STAGING_FAILED.code, message).remedy(STAGING_FAILED.remedy)
    };
    let mut pages: Vec<(PreviewPage, PathBuf)> = Vec::with_capacity(names.len());
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
        let (context, server_version) = fetch_room(name)?;
        let snapshot = context.snapshot();
        let room = HeldRoom {
            transformer: name.clone(),
            server_version,
            layers: snapshot.layers().clone(),
            content_digest: snapshot.metadata().content_digest().map(str::to_string),
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
        let page = ds_report_host::execute(
            &CliEngine,
            settings,
            &ds_report_host::PreviewRequest {
                transformer: name.clone(),
                layout: request.layout.clone(),
            },
            &held,
        )
        .map_err(preview_failure)?;
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
        std::fs::write(&page_path, page.page.as_bytes()).map_err(|error| {
            staging_failed(format!("could not write {}: {error}", page_path.display()))
        })?;
        let receipt_bytes = serde_json::to_vec_pretty(&page.receipt)
            .map_err(|error| staging_failed(error.to_string()))?;
        std::fs::write(&receipt_path, receipt_bytes).map_err(|error| {
            staging_failed(format!(
                "could not write {}: {error}",
                receipt_path.display()
            ))
        })?;
        pages.push((page, page_path));
    }
    // Every run removed its own scratch; the empty staging root goes too.
    let _ = std::fs::remove_dir(settings.staging_root);

    let placed: Vec<(&PreviewPage, PageRef<'_>)> = pages
        .iter()
        .map(|(page, path)| (page, PageRef::File(path.as_path())))
        .collect();
    let answer = ds_report_host::answer(settings.project_id, facts.lane, &placed);
    let mut data = facts.project;
    data["local_print_recipe"] = pages.first().map_or(Value::Null, |(page, _)| {
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

/// The context layers the selected printing setups need, decided once by the
/// kernel over the sealed sheets. `online` is exactly `--seed`: acquisition
/// happens before a renderer, never inside one.
///
/// The sealed sheets carry the export row but not the project's printing
/// setups (only the reporter's own input receipt does); the setups the
/// selection names are read exactly through the printing contract, the way
/// `ds report project settings` completes its sheets, so the kernel decides
/// over the same documents the engine will print with.
fn selected_contexts(
    lane: &str,
    receipt: &InputReceipt,
    online: bool,
) -> Result<Vec<ds_command_kernel::printing::PrintContextLayer>, Failure> {
    let invalid = |message: String| {
        Failure::invalid("report_inputs_invalid", message).remedy(INPUTS_INVALID.remedy)
    };
    let mut sheets: Value = serde_json::from_str(&receipt.sheets_json)
        .map_err(|error| invalid(format!("sealed sheets: {error}")))?;
    if sheets.get("printing_setups").is_none() {
        let named = ds_cli_report_named_setups(&sheets).map_err(invalid)?;
        let mut setups = Vec::with_capacity(named.len());
        for id in named {
            let setup = ds_cli_auth::printing(
                lane,
                false,
                &ds_cli_auth::PrintingRequest::Get { id: id.clone() },
            )?;
            setups.push(json!({"id": setup["id"], "revision": setup["revision"], "layout": setup["layout"]}));
        }
        sheets["printing_setups"] = Value::Array(setups);
    }
    let request = json!({"command": "context_preparation", "sheets": sheets, "online": online});
    let bytes = serde_json::to_vec(&request).map_err(|error| invalid(error.to_string()))?;
    let answer: Value =
        serde_json::from_str(&ds_command_kernel::report::evaluate(&bytes).map_err(invalid)?)
            .map_err(|error| invalid(error.to_string()))?;
    serde_json::from_value(answer["result"]["contexts"].clone())
        .map_err(|error| invalid(format!("selected context layers: {error}")))
}

/// The printing setup ids the stored output selection names, read with the
/// kernel's own readers under every stored shape.
fn ds_cli_report_named_setups(sheets: &Value) -> Result<Vec<String>, String> {
    use ds_command_kernel::report_formats::{
        named_print_output, normalize, output_setting_index, stored_output_selection, string_list,
    };
    let Some(rows) = sheets["project_settings"].as_array() else {
        return Ok(Vec::new());
    };
    let Some(index) = output_setting_index(rows) else {
        return Ok(Vec::new());
    };
    let value = &rows[index]["value"];
    let tokens = stored_output_selection(value)
        .and_then(|selection| selection.tokens())
        .unwrap_or_else(|_| string_list(value));
    Ok(normalize(&tokens)
        .iter()
        .filter_map(|token| named_print_output(token).map(|(_, id)| id.to_owned()))
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
mod tests {
    /// 09b78d74: a proof's refs the receipt never sealed are named all at
    /// once, with the source each was looked for in, instead of failing in
    /// the engine one ref at a time.
    #[test]
    fn unsealed_proof_styles_are_named_with_their_sources() {
        let mut layout = ds_command_kernel::printing::default_layout();
        layout.style_refs = [
            ("roads", "gt/roads_print"),
            ("cells", "gt/cell_boundaries_print"),
            ("kivu", "gt/kivu_lake_print"),
            ("again", "gt/cell_boundaries_print"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let sheets = serde_json::json!({"printing_styles": {"gt/roads_print": {"type": "line"}}});
        let unsealed = super::unsealed_print_style_refs(&sheets, std::slice::from_ref(&layout));
        assert_eq!(unsealed, ["gt/cell_boundaries_print", "gt/kivu_lake_print"]);
        assert!(super::unsealed_print_style_refs(
            &serde_json::json!({"printing_styles": {"gt/roads_print": {}, "gt/cell_boundaries_print": {}, "gt/kivu_lake_print": {}}}),
            std::slice::from_ref(&layout)
        )
        .is_empty());
        let held = "gt/cell_boundaries_print".to_string();
        let absent = "gt/kivu_lake_print".to_string();
        let refused = super::unsealed_print_styles_refusal(&[&held], &[&absent], "proof");
        assert_eq!(refused.code(), "report_inputs_invalid");
        let text = format!(
            "{} {}",
            refused.message(),
            refused.remedy_text().unwrap_or("")
        );
        for needle in [
            "SELECTED printing setups",
            "ds style list --query _print",
            "gt/cell_boundaries_print",
            "published nowhere",
            "gt/kivu_lake_print",
            "ds report project outputs set",
        ] {
            assert!(text.contains(needle), "{text}");
        }
    }
    /// The batch screen an operator actually reads. A run that delivered four
    /// artifacts and lost its A0 must not print as plain `ok`: on Gisagara
    /// that would be 86 `ok` rows over 59 missing sheets, the same work lost
    /// — now silently rather than loudly.
    #[test]
    fn a_partial_run_is_visible_on_the_screen_with_the_member_that_lost_it() {
        let data = serde_json::json!({
            "project": {"project_name": "Gisagara", "ds_project": "gisagara"},
            "lane": "stable",
            "batch": {
                "status": "completed", "completed": 2, "failed": 0,
                "partial_formats": 1, "concurrency": 4, "receipt": "/out/report-batch.json",
            },
            "engine": {"engine_version": "ds-network-reporter@0.1.0", "publication_state": "pending"},
            "results": [
                {"transformer": "upgrade_ruturo", "status": "ok",
                 "receipt": "upgrade_ruturo/report-run.json", "artifacts": 4,
                 "failed_formats": [{
                     "output_id": "pdf__a0-landscape-gisagara-cjic",
                     "format": "pdf__a0-landscape-gisagara-cjic",
                     "code": "format_build_failed",
                     "message": "table lv_schedule has 212 rows; needs 3 panels",
                     "remedy": "raise `panels` on `lv_schedule` in this printing setup",
                     "layout": {"element_id": "lv_schedule", "knob": "panels",
                                "detail": "212 rows need 3 panels; the layout permits 1"}
                 }]},
                {"transformer": "tx_b", "status": "ok", "receipt": "tx_b/report-run.json",
                 "artifacts": 5},
            ],
        });
        let screen = super::render(&data);
        assert!(screen.contains("1 partial"), "{screen}");
        assert!(screen.contains("partial upgrade_ruturo"), "{screen}");
        assert!(screen.contains("4 artifact(s), 1 lost"), "{screen}");
        assert!(
            screen.contains("pdf__a0-landscape-gisagara-cjic"),
            "{screen}"
        );
        assert!(screen.contains("panels on lv_schedule"), "{screen}");
        assert!(screen.contains("the layout permits 1"), "{screen}");
        assert!(screen.contains("fix: raise `panels`"), "{screen}");
        // A run that lost nothing still reads exactly as it did.
        assert!(screen.contains("  ok     tx_b"), "{screen}");
    }

    /// The naming is bounded, and the truncation says how much it did not
    /// print and where the rest is.
    #[test]
    fn a_batch_that_lost_many_formats_bounds_what_it_prints() {
        let row = |name: &str| {
            serde_json::json!({
                "transformer": name, "status": "ok", "receipt": format!("{name}/report-run.json"),
                "artifacts": 4,
                "failed_formats": [{"output_id": "pdf__a0", "format": "pdf__a0",
                    "code": "format_build_failed", "message": "m", "remedy": "r"}],
            })
        };
        let names: Vec<_> = (0..14).map(|i| format!("tx_{i:02}")).collect();
        let data = serde_json::json!({
            "project": {"project_name": "Gisagara", "ds_project": "gisagara"},
            "lane": "stable",
            "batch": {"status": "completed", "completed": 14, "failed": 0,
                      "partial_formats": 14, "concurrency": 4, "receipt": "/out/report-batch.json"},
            "engine": {"engine_version": "e", "publication_state": "pending"},
            "results": names.iter().map(|n| row(n)).collect::<Vec<_>>(),
        });
        let screen = super::render(&data);
        assert_eq!(screen.matches("           lost ").count(), 10, "{screen}");
        assert!(screen.contains("4 further lost format(s)"), "{screen}");
        // Every transformer is still named, bounded or not.
        assert!(
            names.iter().all(|name| screen.contains(name.as_str())),
            "{screen}"
        );
    }

    #[test]
    fn raster_only_contexts_resolve_the_named_layout_once() {
        for value in [
            serde_json::json!(["png__detail", "jpeg__detail", "pdf__detail"]),
            serde_json::json!({"schema":"ds.design-output-selection/v1","prints":[
                {"layout_id":"detail","enabled":true,"formats":["png","jpeg"]}
            ]}),
        ] {
            let sheets = serde_json::json!({"project_settings":[{"parameter":"design_export_format","value":value}]});
            assert_eq!(
                super::ds_cli_report_named_setups(&sheets).unwrap(),
                vec!["detail"]
            );
        }
    }

    #[test]
    fn a_blink_is_retried_but_a_wrong_request_is_not() {
        use std::cell::Cell;
        let calls = Cell::new(0);
        let value = super::with_weak_network(&[std::time::Duration::ZERO; 3], || {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err(ds_cli_contract::Failure::unavailable(
                    "auth_transient",
                    "blink",
                ))
            } else {
                Ok("room")
            }
        })
        .unwrap();
        assert_eq!((value, calls.get()), ("room", 3));
        let calls = Cell::new(0);
        let refused = super::with_weak_network(&[std::time::Duration::ZERO; 3], || {
            calls.set(calls.get() + 1);
            Err::<(), _>(ds_cli_contract::Failure::invalid(
                "report_inputs_invalid",
                "wrong",
            ))
        })
        .unwrap_err();
        assert_eq!((refused.code(), calls.get()), ("report_inputs_invalid", 1));
        // Four blinks in a row is an outage: the last refusal comes back.
        let calls = Cell::new(0);
        let outage = super::with_weak_network(&[std::time::Duration::ZERO; 3], || {
            calls.set(calls.get() + 1);
            Err::<(), _>(ds_cli_contract::Failure::unavailable(
                "auth_transient",
                "blink",
            ))
        })
        .unwrap_err();
        assert_eq!((outage.code(), calls.get()), ("auth_transient", 4));
    }

    use super::*;

    #[test]
    fn batch_inputs_cannot_cross_principal_lane_audience_or_project() {
        use ds_cli_auth::ProviderIdentity;
        let identity = ProviderIdentity::new("stable", &"a".repeat(64), "user-a").unwrap();
        assert!(require_same_context(&identity, "project-a", &identity, "project-a").is_ok());
        for other in [
            ProviderIdentity::new("stable", &"a".repeat(64), "user-b").unwrap(),
            ProviderIdentity::new("canary", &"a".repeat(64), "user-a").unwrap(),
            ProviderIdentity::new("stable", &"b".repeat(64), "user-a").unwrap(),
        ] {
            assert_eq!(
                require_same_context(&identity, "project-a", &other, "project-a")
                    .unwrap_err()
                    .code,
                INPUTS_INVALID.code
            );
        }
        assert!(require_same_context(&identity, "project-a", &identity, "project-b").is_err());
    }

    /// Every code the host crate can answer with is a literal arm here and a
    /// documented refusal, so a caller can plan for it from `--help`.
    #[test]
    fn every_host_code_is_mapped_and_documented() {
        for code in [
            ds_report_host::CODE_INPUTS_INVALID,
            ds_report_host::CODE_STAGING_FAILED,
            ds_report_host::CODE_ENGINE_REFUSED,
            ds_report_host::CODE_EXPORT_BLOCKED,
            ds_report_host::CODE_RESULT_INVALID,
            ds_report_host::CODE_OUTPUT_EXISTS,
            ds_report_host::CODE_ENGINE_IDENTITY_CHANGED,
            ds_report_host::CODE_ENGINE_IDENTITY_INVALID,
            ds_report_host::CODE_ADMIN_BOUNDS_UNAVAILABLE,
            DS_REPORT.missing_code,
            "callee_timed_out",
        ] {
            let failure = host_failure(HostFailure::new(code, "why").with_detail(json!({"k": 1})));
            assert_eq!(failure.code(), code, "{code} must survive the mapping");
            assert!(failure.remedy_text().is_some(), "{code} needs a remedy");
            assert_eq!(failure.detail_value().unwrap()["k"], 1);
            assert!(
                REFUSALS.iter().any(|refusal| refusal.code == code),
                "{code} is not documented"
            );
        }
        // An unknown host code never becomes an undocumented one.
        assert_eq!(
            host_failure(HostFailure::new("something_new", "why")).code(),
            "report_result_invalid"
        );
        assert!(
            REFUSALS
                .iter()
                .any(|refusal| refusal.code == NOT_ACTIVE.code)
        );
    }

    /// Every failure the holdings crate can answer with for one transformer
    /// becomes a batch row under a code this command documents.
    #[test]
    fn every_context_failure_is_a_documented_row_code() {
        use ds_project_data::Failure as Cause;
        for cause in [
            Cause::NotHeld("x".into()),
            Cause::AcquisitionFailed("x".into()),
            Cause::ProviderUnavailable("x".into()),
            Cause::BundleUnavailable("x".into()),
            Cause::Unsupported("x".into()),
            Cause::TooLarge("x".into()),
            Cause::CatalogInvalid("x".into()),
            Cause::Store("x".into()),
        ] {
            let failure = context_failure(cause);
            assert!(
                REFUSALS.iter().any(|refusal| refusal.code == failure.code),
                "{} is not documented",
                failure.code
            );
            let mapped = host_failure(failure.clone());
            assert_eq!(
                mapped.code(),
                failure.code,
                "{} must survive the mapping",
                failure.code
            );
            assert!(mapped.remedy_text().is_some());
        }
    }

    /// The preview classifies as the desktop door does: every code in the
    /// host crate's class table lands in the same class through
    /// `preview_failure`, and where this command's own `host_failure` table
    /// knows the code the two tables agree — one classification, two doors.
    #[test]
    fn preview_refusals_classify_as_the_kernel_class_table_does() {
        for (code, class) in ds_report_host::CLASS_TABLE {
            let refusal = PreviewRefusal::Host(HostFailure::new(code, "why"));
            assert_eq!(refusal.class(), *class);
            let failure = preview_failure(refusal.clone());
            assert_eq!(failure.code(), *code, "{code} must survive the mapping");
            assert_eq!(
                failure.class().token(),
                class.token(),
                "{code} classifies differently from the kernel"
            );
            assert_eq!(failure.class().retryable(), class.retryable());
            let own = host_failure(HostFailure::new(code, "why"));
            if own.code() == *code {
                assert_eq!(
                    own.class().token(),
                    class.token(),
                    "{code}: host_failure and CLASS_TABLE disagree"
                );
            }
            if REFUSALS.iter().any(|documented| documented.code == *code) {
                assert!(failure.remedy_text().is_some(), "{code} needs a remedy");
            }
        }
    }

    /// Every preview refusal this host can reach is documented. The kernel's
    /// room and capture admissions are not reachable here: the CLI admits
    /// the service's room itself and hands no captures, so those codes stay
    /// the desktop door's.
    #[test]
    fn every_reachable_preview_code_is_documented() {
        const DESKTOP_ONLY: &[&str] = &[
            "print_preview_room_unavailable",
            "print_preview_room_incomplete",
            "print_context_capture_invalid",
            "print_context_survey_empty",
            "print_context_capture_changed",
            "print_context_foreign_project",
            "print_context_capture_incomplete",
            "print_context_not_held_source",
            "print_context_omitted",
        ];
        for (code, _) in ds_report_host::CLASS_TABLE {
            if DESKTOP_ONLY.contains(code) {
                continue;
            }
            assert!(
                REFUSALS.iter().any(|documented| documented.code == *code),
                "{code} is not documented"
            );
        }
        // A kernel refusal keeps its message key and params for a localised
        // surface, and a host failure keeps its own detail.
        let kernel = preview_failure(PreviewRefusal::Kernel(
            ds_command_kernel::printing::preview::Refusal {
                code: "print_context_survey_empty".into(),
                message_key: "printing_context_survey_empty".into(),
                params: BTreeMap::from([("name".to_string(), "poles".to_string())]),
            },
        ));
        assert_eq!(kernel.code(), "print_context_survey_empty");
        assert_eq!(kernel.class(), ExitClass::InvalidInput);
        assert_eq!(
            kernel.detail_value().unwrap()["message_key"],
            "printing_context_survey_empty"
        );
        assert_eq!(kernel.detail_value().unwrap()["params"]["name"], "poles");
        let host = preview_failure(PreviewRefusal::Host(
            HostFailure::new("export_blocked", "blocked").with_detail(json!({"blockers": ["x"]})),
        ));
        assert_eq!(host.class(), ExitClass::Failed);
        assert_eq!(host.remedy_text(), Some(EXPORT_BLOCKED.remedy));
        assert_eq!(host.detail_value().unwrap()["blockers"][0], "x");
    }

    #[test]
    fn the_engine_summary_is_bounded_and_prefers_stderr() {
        assert_eq!(bounded_summary("bad\u{7} thing\n", "ignored"), "bad thing");
        assert_eq!(bounded_summary("   ", "stdout words"), "stdout words");
        assert_eq!(bounded_summary(&"x".repeat(5_000), "").len(), 2_000);
    }

    #[test]
    fn the_descriptor_stays_headless_and_bounded() {
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
        assert!(COMMAND.summary.len() <= 70);
        assert!(COMMAND.args.iter().all(|arg| arg.name != "project"));
        // Acceptance B: there is no `--publish`. An export publishes, and the
        // only way to publish nothing is to ask for a dry run. If this switch
        // ever comes back, every run that forgets it strands its artifacts
        // again.
        assert!(COMMAND.args.iter().all(|arg| arg.name != "publish"));
        assert!(COMMAND.args.iter().any(|arg| arg.name == "dry-run"));
        assert!(COMMAND.args.iter().any(|arg| arg.name == "seed"));
        assert!(
            !COMMAND
                .purpose
                .contains("External print context is omitted")
        );
        assert!(
            COMMAND
                .args
                .iter()
                .any(|arg| arg.name == "server-state-dir")
        );
        assert!(COMMAND.purpose.contains("named print output"));
    }

    #[test]
    fn release_reports_seal_only_verified_outputs_for_the_server_queue() {
        let root = tempfile::tempdir().unwrap();
        let report_dir = root.path().join("report");
        std::fs::create_dir(&report_dir).unwrap();
        let artifact = report_dir.join("report.xlsx");
        std::fs::write(&artifact, b"x").unwrap();
        let mut run = ds_report_host::RunOutcome {
            transformer: "tx-a".into(),
            client_run_id: "report-00000000000000000000000000000000".into(),
            artifact_dir: report_dir,
            receipt_path: root.path().join("report-run.json"),
            receipt: json!({
                "project_id": "project-a",
                "transformer": "tx-a",
                "transformer_revision": 3,
                "input_base_fingerprint": "a".repeat(64),
                "room_content_sha256": "b".repeat(64),
            }),
            artifacts: vec![ds_command_kernel::report_export::VerifiedArtifact {
                output_id: "xlsx".into(),
                format: "xlsx".into(),
                filename: "report.xlsx".into(),
                content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                size_bytes: 1,
                sha256: "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881".into(),
                paper_size: None,
                presentation: None,
            }],
            // A partial run: the workbook completed, the A0 sheet did not.
            // `--publish` seals what completed and queues nothing for what
            // did not — which is the whole reason the completed work is
            // allowed to leave the machine.
            failed: vec![ds_command_kernel::report_export::FailedFormat {
                output_id: "pdf__a0-landscape-gisagara-cjic".into(),
                format: "pdf__a0-landscape-gisagara-cjic".into(),
                code: "format_build_failed".into(),
                message: "table lv_schedule has 212 rows; needs 3 panels at the authored type size (maximum 1)".into(),
                remedy: "raise `panels` on `lv_schedule`".into(),
                layout: None,
            }],
            warnings: vec![],
            engine: ds_command_kernel::report_export::EngineIdentity {
                engine_version: format!("ds-network-reporter@0.1.0+{}", "c".repeat(40)),
                build_manifest_sha256: "d".repeat(64),
                package_version: "0.1.0".into(),
                source_sha: "c".repeat(40),
                profile: "release".into(),
            },
        };
        let fence = ds_command_kernel::sync_store::Fence {
            account: "owner-a".into(),
            deployment: "https://gateway.example".into(),
            install_id: "install-1".into(),
        };
        let queue_at = |name: &str| {
            PublicationQueue::at(
                root.path().join(name).join("report-artifacts"),
                ds_sync_runtime::open_store(&root.path().join(name).join("store.sqlite")).unwrap(),
                fence.clone(),
            )
        };
        let queue = queue_at("queue");
        let SealedRun::Queued(sealed) =
            seal_run_for_server(&run, "owner-a", "project-a", &queue, &|| Ok(())).unwrap()
        else {
            panic!("a first seal is queued")
        };
        assert_eq!(sealed.receipt.project_id, "project-a");
        assert_eq!(sealed.receipt.transformer, "tx-a");
        assert_eq!(
            sealed.row.state,
            ds_command_kernel::sync_store::ArtifactState::Held
        );
        // The store is the queue: the row is there, and the bytes it names
        // are the committed batch.
        let status = queue
            .store
            .lock()
            .unwrap()
            .queue(&fence, Some("project-a"), ds_sync_runtime::now_ms())
            .unwrap();
        assert_eq!(status.queued_batches, 1);
        assert_eq!(status.projects[0].transformers, vec!["tx-a".to_string()]);
        let batch = ds_report_artifacts::committed_batch(&queue.root, &sealed.receipt.batch_id)
            .unwrap()
            .expect("the batch the row names is committed");
        assert!(ds_report_artifacts::open_batch_output(&queue.root, &batch, "xlsx").is_ok());
        // Only what completed is queued; the format that failed is not
        // invented into the publication.
        assert!(
            ds_report_artifacts::open_batch_output(
                &queue.root,
                &batch,
                "pdf__a0-landscape-gisagara-cjic"
            )
            .is_err()
        );
        // The same run sealed again is the same row, and no second batch.
        let SealedRun::AlreadyRecorded { replay_key, .. } =
            seal_run_for_server(&run, "owner-a", "project-a", &queue, &|| Ok(())).unwrap()
        else {
            panic!("the same bytes are already recorded")
        };
        assert_eq!(replay_key, sealed.receipt.client_publish_id);
        assert_eq!(
            ds_report_artifacts::list_project_publication_batches_from_root(
                &queue.root,
                "project-a"
            )
            .unwrap()
            .len(),
            1
        );

        let rollback_queue = queue_at("rollback");
        let error = seal_run_for_server(&run, "owner-a", "project-a", &rollback_queue, &|| {
            Err(Failure::conflict(
                PUBLISH_SCOPE_CHANGED.code,
                "scope changed",
            ))
        })
        .unwrap_err();
        assert_eq!(error.code(), PUBLISH_SCOPE_CHANGED.code);
        assert!(
            ds_report_artifacts::list_project_publication_batches_from_root(
                &rollback_queue.root,
                "project-a",
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(
            rollback_queue
                .store
                .lock()
                .unwrap()
                .queue(&fence, None, ds_sync_runtime::now_ms())
                .unwrap()
                .queued_batches,
            0,
            "a refused seal records no row"
        );

        std::fs::write(root.path().join("report/report.xlsx"), b"corrupt").unwrap();
        let corrupt_queue = queue_at("corrupt");
        assert_eq!(
            seal_run_for_server(&run, "owner-a", "project-a", &corrupt_queue, &|| Ok(()))
                .unwrap_err()
                .code(),
            PUBLISH_ROOT.code,
        );
        assert!(
            ds_report_artifacts::list_project_publication_batches_from_root(
                &corrupt_queue.root,
                "project-a",
            )
            .unwrap()
            .is_empty()
        );

        run.engine.engine_version = ds_command_kernel::report::DEVELOPMENT_ENGINE.into();
        assert_eq!(
            seal_run_for_server(&run, "owner-a", "project-a", &queue_at("local"), &|| Ok(()))
                .unwrap_err()
                .code(),
            PUBLISH_LOCAL_ONLY.code,
        );
    }

    #[test]
    fn server_queue_root_is_the_exact_custom_server_state_child() {
        assert_eq!(
            server_report_artifacts_root("stable", Some(Path::new("/var/lib/ds"))).unwrap(),
            PathBuf::from("/var/lib/ds/report-artifacts"),
        );
        assert_eq!(
            server_report_artifacts_root("stable", Some(Path::new("relative")))
                .unwrap_err()
                .code(),
            PUBLISH_ROOT.code,
        );
    }
}
