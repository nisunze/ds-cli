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

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ds_cli_auth::{TransformerKind, TransformerLifecycle};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::{
    report::PublicationState,
    report_export::{InputReceipt, reportable_transformer},
};
use ds_report_artifacts::{
    CommittedAuthorizedPublication, VerifiedSidecarArtifact, confined_fs::HeldDirectory,
};
use ds_report_host::{
    BatchSettings, DEFAULT_RESIDENT_LIMIT, EngineExit, HostFailure, MAX_RESIDENT_LIMIT,
    ReportEngine, RunSettings, TransformerReportInputs, batch_plan, installed_admin_bounds_path,
    run_batch, shared_root, verify_admin_bounds_asset,
};
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
    "Acquire missing selected map context for each printed transformer before rendering; spends provider cost and downloads bundles. Without it, unheld context is omitted and named.",
);
const PUBLISH_ARG: Arg = Arg::switch(
    "publish",
    "Seal verified outputs for the matching native Server sync pump; without it, writes local reports only.",
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
    when: "the engine refused a transformer or completed only part of it (a batch row carries its blockers)",
    remedy: "read `error.detail.blockers`, fix the named input, and re-run that transformer",
};
const INPUTS_INVALID: Refusal = Refusal {
    code: "report_inputs_invalid",
    when: "the reporter input receipt, a transformer's saved layers or the output policy cannot be run as given",
    remedy: "refresh the project configuration; `ds report project settings` shows the output policy",
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
    when: "--publish was requested from a development reporter build",
    remedy: "run a release reporter build, or omit --publish for local-only files",
};
const PUBLISH_SCOPE_CHANGED: Refusal = Refusal {
    code: "report_publish_scope_changed",
    when: "the native UID, lane, credential audience, selected project, or credential generation changed before sealed publication",
    remedy: "repeat the export under the current native account and project",
};
const PUBLISH_ROOT: Refusal = Refusal {
    code: "report_publish_root_invalid",
    when: "the Server state root is unavailable, relative, or cannot hold a sealed publication",
    remedy: "start Server with its default state root or pass the same absolute --server-state-dir used by ds server serve",
};
const CONTEXT_UNSUPPORTED: Refusal = Refusal {
    code: "print_context_unsupported",
    when: "a selected printing setup names a survey or local-layer context source, which no headless host can supply (a batch row carries it)",
    remedy: "print that setup from the desktop, or remove the source from the setup's context layers",
};
const CONTEXT_INVALID: Refusal = Refusal {
    code: "print_context_invalid",
    when: "the assembled print context exceeds the engine's bounds or this machine's holdings could not be read (a batch row carries it)",
    remedy: "narrow the setup's context buffers, or repair the geographic data storage root",
};
const CONTEXT_CATALOG: Refusal = Refusal {
    code: "catalog_unavailable",
    when: "the reference catalogue could not be read, so catalogue context layers were omitted from every print",
    remedy: "retry when connected; held rooms still print",
};
const CONTEXT_BUNDLE: Refusal = Refusal {
    code: "reference_bundle_unavailable",
    when: "--seed needed a national bundle the catalogue does not publish, or it could not be installed (a batch row carries it)",
    remedy: "publish the dataset's bundle, then print again with --seed",
};
const CONTEXT_PROVIDER: Refusal = Refusal {
    code: "dataset_provider_unavailable",
    when: "--seed could not reach the governed context provider (a batch row carries it)",
    remedy: "restore the connection and print again; held context is kept",
};
const CONTEXT_ACQUISITION: Refusal = Refusal {
    code: "project_dataset_acquisition_failed",
    when: "--seed acquired context the room refused (a batch row carries it)",
    remedy: "read the row's message; `ds data project-cache status` shows the dataset's last error",
};

const REFUSALS: &[Refusal] = &[
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
    CONTEXT_PROVIDER,
    CONTEXT_ACQUISITION,
    ds_cli_auth::DATA_DISTRIBUTION_UNAVAILABLE_REFUSAL,
    ds_cli_auth::REFERENCE_BUNDLE_DOWNLOAD_FAILED_REFUSAL,
];

pub static COMMAND: Command = Command {
    id: "report.project.export",
    path: &["report", "project", "export"],
    contract: 1,
    summary: "Produce transformer reports, prints included, headlessly.",
    purpose: "Export the selected project's saved transformers and named print outputs with the native reporter. Defaults to all active transformers; engines run concurrently and outputs are verified. Files stay local unless --publish seals them for Server sync, which is not cloud completion. Each print carries the map context its setups select from this machine's project rooms; --seed acquires what is not held first, so the first print request seeds. Photos require a media grant and currently refuse. Inspect batch rows and warnings. Details: docs/reference/report.md.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        OUT_DIR_ARG,
        CONCURRENCY_ARG,
        ADMIN_BOUNDS_ARG,
        SEED_ARG,
        PUBLISH_ARG,
        SERVER_STATE_DIR_ARG,
        LANE_ARG,
    ],
    output: "\
Lane and selected-project identity/status, the scope, the engine identity and \
publication state, the batch (status completed|partial|failed, counts, \
concurrency, receipt path) and one result per transformer in stable order: \
`ok` with its artifact count and `<transformer>/report-run.json`; --publish also \
reports the durable local Server-sync queue identity, or `error` \
with a typed code, message and detail.",
    examples: &[
        Example {
            command: "ds report project export --out-dir ./reports --output json",
            note: "Every active transformer; `.data.results[]` says what each produced.",
            runnable: false,
        },
        Example {
            command: "ds report project export --transformer tx_a --transformer tx_b --out-dir ./reports --concurrency 2",
            note: "Two named transformers, two engines at once.",
            runnable: false,
        },
        Example {
            command: "ds report project export --transformer tx_a --out-dir ./reports --publish --output json",
            note: "Queue one verified report for Server sync; publication is separate.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
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
        "dataset_provider_unavailable" => {
            Failure::unavailable("dataset_provider_unavailable", message)
                .remedy(CONTEXT_PROVIDER.remedy)
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

fn seal_run_for_server(
    run: &ds_report_host::RunOutcome,
    owner_uid: &str,
    project_id: &str,
    root: &Path,
    guard: &dyn Fn() -> Result<(), Failure>,
) -> Result<CommittedAuthorizedPublication, Failure> {
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
    let pending = ds_report_artifacts::promote_local_artifacts(
        root,
        &held,
        owner_uid,
        project_id,
        &run.engine.engine_version,
        &run.engine.build_manifest_sha256,
        &run.transformer,
        receipt_revision(&run.receipt)?,
        receipt_text(&run.receipt, "input_base_fingerprint")?,
        receipt_text(&run.receipt, "room_content_sha256")?,
        &run.client_run_id,
        &formats,
        deadline,
    )
    .map_err(|error| Failure::failed(PUBLISH_ROOT.code, error).remedy(PUBLISH_ROOT.remedy))?;
    // The pending batch retains rollback through all copy/rename/fsync work.
    // Recheck the captured scope at the only durable visibility boundary.
    guard()?;
    pending
        .commit(deadline)
        .map_err(|error| Failure::failed(PUBLISH_ROOT.code, error).remedy(PUBLISH_ROOT.remedy))
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

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = super::transformer_set(inputs)?;
    let lane = inputs.require("lane")?;
    let out_dir = PathBuf::from(inputs.require("out-dir")?);
    let resident_limit = concurrency_limit(inputs)?;
    let explicit_asset = inputs.value("admin-bounds").map(PathBuf::from);
    let seed = inputs.switch("seed");
    let publish = inputs.switch("publish");
    let publish_scope = publish
        .then(|| ds_cli_auth::capture_layer_scope_fence(lane))
        .transpose()?;
    let publish_root = if publish {
        Some(server_report_artifacts_root(
            lane,
            inputs.value("server-state-dir").map(Path::new),
        )?)
    } else {
        if inputs.value("server-state-dir").is_some() {
            return Err(Failure::invalid(
                "report_inputs_invalid",
                "--server-state-dir requires --publish",
            )
            .remedy("pass --publish, or remove --server-state-dir"));
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
    let contexts = selected_contexts(&receipt, seed)?;
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
                    "message": format!("catalogue context layers were omitted: {reason}"),
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
    let mut provider = ds_cli_data::project_cache::CliProvider { lane };
    let mut bundle_fetch = ds_cli_data::project_cache::bundle_fetch(lane);
    let mut transformer_context_notes: Vec<Value> = Vec::new();

    let processors = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let plan = batch_plan(&names, processors, resident_limit).map_err(|error| {
        Failure::invalid("report_inputs_invalid", error).remedy(INPUTS_INVALID.remedy)
    })?;

    let staging = out_dir.join(STAGING_DIRECTORY);
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
        match lifecycle.get(name).map(String::as_str) {
            Some("active") => {}
            Some(state) => {
                return Err(HostFailure::new(
                    NOT_ACTIVE.code,
                    format!("{name} is {state}, not an active saved transformer"),
                ));
            }
            None => {
                return Err(HostFailure::new(
                    NOT_ACTIVE.code,
                    format!("{name} is not in the project's transformer inventory"),
                ));
            }
        }
        let context = ds_cli_auth::transformer_context(lane, name).map_err(failure_to_host)?;
        require_same_context(
            inventory.identity(),
            &project_id,
            context.identity(),
            context.snapshot().ds_project(),
        )?;
        let snapshot = context.snapshot();
        if snapshot.ds_project() != project_id || snapshot.transformer_name() != name {
            return Err(HostFailure::new(
                INPUTS_INVALID.code,
                format!("the service answered for another project or transformer than {name}"),
            ));
        }
        let server_version = snapshot
            .metadata()
            .version()
            .and_then(|version| i64::try_from(version).ok())
            .filter(|version| *version > 0)
            .ok_or_else(|| {
                HostFailure::new(
                    INPUTS_INVALID.code,
                    format!(
                        "the service reports no saved revision for {name}; save it before reporting"
                    ),
                )
            })?;
        let print_context = match holdings_root.as_deref() {
            None => None,
            Some(root) => {
                let layers_value = serde_json::to_value(snapshot.layers())
                    .map_err(|error| HostFailure::new(INPUTS_INVALID.code, error.to_string()))?;
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
                    transformer_context_notes.push(json!({"transformer": name, "note": warning}));
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
        };
        Ok(TransformerReportInputs {
            transformer: name.to_string(),
            server_version,
            layers: snapshot.layers().clone(),
            content_digest: snapshot.metadata().content_digest().map(str::to_string),
            selection: None,
            print_context,
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

    let publication_state = outcome.engine.publication_state().ok();
    let sealed_publications =
        if let (Some(fence), Some(root)) = (publish_scope.as_ref(), publish_root.as_ref()) {
            let mut publications = Vec::with_capacity(outcome.runs.len());
            for run in &outcome.runs {
                // The publication marker is the durable effect. Re-probe the
                // native UID/audience/project/credential binding immediately
                // before each marker can become visible to the Server pump.
                verify_publish_scope(lane, fence, fence.uid(), &project_id)?;
                let committed = seal_run_for_server(run, fence.uid(), &project_id, root, &|| {
                    verify_publish_scope(lane, fence, fence.uid(), &project_id)
                })?;
                publications.push(json!({
                    "batch_id": committed.receipt.batch_id,
                    "client_publish_id": committed.receipt.client_publish_id,
                    "transformer": committed.receipt.transformer,
                    "outputs": committed.artifacts.len(),
                }));
            }
            Some(publications)
        } else {
            None
        };
    output["out_dir"] = json!(out_dir.display().to_string());
    output["scope"] = scope;
    output["publication_enqueued"] = json!(publish);
    if let (Some(root), Some(publications)) = (publish_root, sealed_publications) {
        output["publication"] = json!({
            "state": "queued_for_server_sync",
            "root": root.display().to_string(),
            "batches": publications,
            "note": "sealed locally; the matching native Server sync pump publishes when it next runs",
        });
    }
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
        "selected_layers": contexts.iter().map(|layer| layer.id.clone()).collect::<Vec<_>>(),
        "seeded": seed,
        "catalog_resources": catalog.len(),
        "warnings": context_warnings,
        "notes": transformer_context_notes,
    });
    Ok(output)
}

/// The context layers the selected printing setups need, decided once by the
/// kernel over the sealed sheets. `online` is exactly `--seed`: acquisition
/// happens before a renderer, never inside one.
fn selected_contexts(
    receipt: &InputReceipt,
    online: bool,
) -> Result<Vec<ds_command_kernel::printing::PrintContextLayer>, Failure> {
    let sheets: Value = serde_json::from_str(&receipt.sheets_json).map_err(|error| {
        Failure::invalid("report_inputs_invalid", format!("sealed sheets: {error}"))
            .remedy(INPUTS_INVALID.remedy)
    })?;
    let request = json!({"command": "context_preparation", "sheets": sheets, "online": online});
    let bytes = serde_json::to_vec(&request).map_err(|error| {
        Failure::invalid("report_inputs_invalid", error.to_string()).remedy(INPUTS_INVALID.remedy)
    })?;
    let answer: Value = serde_json::from_str(
        &ds_command_kernel::report::evaluate(&bytes).map_err(|error| {
            Failure::invalid("report_inputs_invalid", error).remedy(INPUTS_INVALID.remedy)
        })?,
    )
    .map_err(|error| {
        Failure::invalid("report_inputs_invalid", error.to_string()).remedy(INPUTS_INVALID.remedy)
    })?;
    serde_json::from_value(answer["contexts"].clone()).map_err(|error| {
        Failure::invalid(
            "report_inputs_invalid",
            format!("selected context layers: {error}"),
        )
        .remedy(INPUTS_INVALID.remedy)
    })
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
        Cause::ProviderUnavailable(_) => HostFailure::new(CONTEXT_PROVIDER.code, message),
        Cause::AcquisitionFailed(_) => HostFailure::new(CONTEXT_ACQUISITION.code, message),
        Cause::CatalogInvalid(_) => HostFailure::new(CONTEXT_CATALOG.code, message),
        Cause::NotHeld(_) | Cause::TooLarge(_) | Cause::Store(_) => {
            HostFailure::new(CONTEXT_INVALID.code, message)
        }
    }
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} ({}) · {} · batch {} · {} completed · {} failed · {} at once\n  engine {} ({})\n  {}\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["batch"]["status"].as_str().unwrap_or("?"),
        data["batch"]["completed"].as_u64().unwrap_or(0),
        data["batch"]["failed"].as_u64().unwrap_or(0),
        data["batch"]["concurrency"].as_u64().unwrap_or(0),
        data["engine"]["engine_version"].as_str().unwrap_or("?"),
        data["engine"]["publication_state"].as_str().unwrap_or("?"),
        data["batch"]["receipt"].as_str().unwrap_or(""),
    );
    for row in data["results"].as_array().into_iter().flatten() {
        let name = row["transformer"].as_str().unwrap_or("?");
        if row["status"] == "ok" {
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
    out
}

#[cfg(test)]
mod tests {
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
        assert!(COMMAND.args.iter().any(|arg| arg.name == "publish"));
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
            warnings: vec![],
            engine: ds_command_kernel::report_export::EngineIdentity {
                engine_version: format!("ds-network-reporter@0.1.0+{}", "c".repeat(40)),
                build_manifest_sha256: "d".repeat(64),
                package_version: "0.1.0".into(),
                source_sha: "c".repeat(40),
                profile: "release".into(),
            },
        };
        let queue = root.path().join("report-artifacts");
        let committed =
            seal_run_for_server(&run, "owner-a", "project-a", &queue, &|| Ok(())).unwrap();
        assert_eq!(committed.receipt.project_id, "project-a");
        assert_eq!(committed.receipt.transformer, "tx-a");
        let rows =
            ds_report_artifacts::list_project_publication_batches_from_root(&queue, "project-a")
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(ds_report_artifacts::open_batch_output(&queue, &rows[0], "xlsx").is_ok());

        let rollback_queue = root.path().join("rollback");
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
                &rollback_queue,
                "project-a",
            )
            .unwrap()
            .is_empty()
        );

        std::fs::write(root.path().join("report/report.xlsx"), b"corrupt").unwrap();
        let corrupt_queue = root.path().join("corrupt");
        assert_eq!(
            seal_run_for_server(&run, "owner-a", "project-a", &corrupt_queue, &|| Ok(()))
                .unwrap_err()
                .code(),
            PUBLISH_ROOT.code,
        );
        assert!(
            ds_report_artifacts::list_project_publication_batches_from_root(
                &corrupt_queue,
                "project-a",
            )
            .unwrap()
            .is_empty()
        );

        run.engine.engine_version = ds_command_kernel::report::DEVELOPMENT_ENGINE.into();
        assert_eq!(
            seal_run_for_server(
                &run,
                "owner-a",
                "project-a",
                &root.path().join("local"),
                &|| Ok(())
            )
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
