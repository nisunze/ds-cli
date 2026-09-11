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
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ds_cli_auth::{TransformerKind, TransformerLifecycle};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::report_export::{InputReceipt, reportable_transformer};
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
];

pub static COMMAND: Command = Command {
    id: "report.project.export",
    path: &["report", "project", "export"],
    contract: 1,
    summary: "Produce transformer reports, prints included, headlessly.",
    purpose: "\
Restores the native user and produces, for its audience-fenced selected \
project, each transformer's individual report with the installed reporter \
engine: every geospatial and tabular output the project's policy names and \
every named print output, rendered from the saved printing setups. The inputs \
are governed — the reporter input receipt ds-brain mints beside the \
configuration and each transformer's exact saved layers — so no browser, room \
cache or Desktop is needed, and receipts carry the desktop's fingerprint and \
room digest. Without --transformer the scope is every active saved \
transformer. Independent transformers run under a bounded number of resident \
engines; one failure is one batch row. No project, URL or action override is \
accepted.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        OUT_DIR_ARG,
        CONCURRENCY_ARG,
        ADMIN_BOUNDS_ARG,
        LANE_ARG,
    ],
    output: "\
Lane and selected-project identity/status, the scope, the engine identity and \
publication state, the batch (status completed|partial|failed, counts, \
concurrency, receipt path) and one result per transformer in stable order: \
`ok` with its artifact count and `<transformer>/report-run.json`, or `error` \
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

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = super::transformer_set(inputs)?;
    let lane = inputs.require("lane")?;
    let out_dir = PathBuf::from(inputs.require("out-dir")?);
    let resident_limit = concurrency_limit(inputs)?;
    let explicit_asset = inputs.value("admin-bounds").map(PathBuf::from);

    // The lifecycle inventory is both the project identity and the scope:
    // every active saved transformer, or the exact names given with the state
    // each one is in.
    let inventory = ds_cli_auth::transformer_inventory(lane, &requested)?;
    let project_id = inventory.project_id().to_string();
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
        Ok(TransformerReportInputs {
            transformer: name.to_string(),
            server_version,
            layers: snapshot.layers().clone(),
            content_digest: snapshot.metadata().content_digest().map(str::to_string),
            selection: None,
        })
    };
    let outcome = run_batch(&CliEngine, &settings, &plan.names, fetch).map_err(host_failure)?;
    // Every run removed its own scratch; the empty staging root goes too.
    let _ = std::fs::remove_dir(&staging);

    let publication_state = outcome.engine.publication_state().ok();
    output["out_dir"] = json!(out_dir.display().to_string());
    output["scope"] = scope;
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
    Ok(output)
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
        assert!(COMMAND.purpose.contains("named print output"));
    }
}
