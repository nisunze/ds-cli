//! `ds report project combined` — the project's Combined Report, produced on
//! this machine and sealed into the existing Sync publication queue.
//!
//! The Combined Report is the `combined_transformer` data set: SHP, XLSX and
//! GeoJSONSeq, plus GPKG or Network Information when the project's output
//! selection names them. It is never printed. Every active LV transformer's
//! saved room is acquired through the same governed hold `report project
//! export` reads (a held room whose head has not moved is reused; only a
//! missing or moved one is read), staged one at a time and released, and the
//! installed Reporter aggregates them in its one combined task. The decisions
//! are the kernel's (`report_export::combined`): formats, room projection,
//! digests, request, result proof, the collision gate over the project's
//! stored collision result and the voltage-drop warnings. Publication is the
//! same seal an individual export uses, so the Sync Center publishes it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ds_cli_auth::TransformerSet;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::gateway_exchange::design::{
    ProjectConfigurationDocument, ProjectConfigurationRead,
};
use ds_command_kernel::report_export::{InputReceipt, combined as decisions};
use ds_report_host::HostFailure;
use ds_report_host::combined::{CombinedExtras, CombinedSettings, CombinedStaging};
use serde_json::{Value, json};

use super::export::{CliEngine, PublicationQueue, SealedRun};
use super::hold::{self, Heads, Hold, Link, Rooms};
use super::{LANE_ARG, PROJECT_ARG, TRANSFORMER_ARG};

const OUT_DIR_ARG: Arg = Arg::value(
    "out-dir",
    "<path>",
    "Fresh delivery directory; the package lands in <out-dir>/combined_transformer/.",
)
.required();
const DRY_RUN_ARG: Arg = Arg::switch(
    "dry-run",
    "Produce the local package and publish NOTHING; the receipt says so.",
);
const STATE_DIR_ARG: Arg = Arg::value(
    "core-state-dir",
    "<absolute-path>",
    "Publication queue root; the same absolute path as ds core serve when overridden.",
);
const REFRESH_ARG: Arg = Arg::switch(
    "refresh",
    "Re-read the project configuration this machine holds; publishing always does.",
);
/// The staging directory a run keeps below its output root while it runs.
const STAGING_DIRECTORY: &str = ".staging";
/// The most room failures one refusal names; the count is exact.
const MAX_NAMED_FAILURES: usize = 50;

const COLLISIONS_ROW: &str = "collisions";

const ENGINE_MISSING: Refusal = Refusal {
    code: "reporter_engine_missing",
    when: "the Reporter addon is not installed for the selected lane",
    remedy: "install the lane's Reporter addon, or set DS_REPORT_BIN to a compatible one",
};
const COLLISIONS: Refusal = Refusal {
    code: "combined_report_collision_limit_exceeded",
    when: "the stored collision result exceeds the project's combined-report policy",
    remedy: "resolve the overlaps and publish a fresh collision result",
};
const ROOMS_UNAVAILABLE: Refusal = Refusal {
    code: "combined_report_rooms_unavailable",
    when: "saved rooms in scope could not be read or held; the detail names each",
    remedy: "retry while connected, save the named designs, or narrow --transformer",
};
const EMPTY: Refusal = Refusal {
    code: "combined_report_empty",
    when: "no transformer in scope holds a report feature",
    remedy: "save the LV designs first; `ds report project scope` lists the inventory",
};
const MEDIA_SCOPE: Refusal = Refusal {
    code: "combined_report_media_scope_unsupported",
    when: "the rooms reference photos stored under another project",
    remedy: "re-save those photos under this project, or narrow --transformer",
};
const MEDIA_GRANT: Refusal = Refusal {
    code: "combined_report_media_grant_unavailable",
    when: "the project's report media grant for referenced photos could not be obtained",
    remedy: "retry while connected",
};
const NOT_ACTIVE: Refusal = Refusal {
    code: "transformer_not_active",
    when: "a named transformer is retired, deleted or missing",
    remedy: "`ds report project scope` shows the lifecycle; restore or drop the name",
};
const EXPORT_BLOCKED: Refusal = Refusal {
    code: "export_blocked",
    when: "the Reporter built fewer than every planned format; detail.blockers names each",
    remedy: "fix the named inputs and retry; a partial set is never sealed",
};
const ENGINE_REFUSED: Refusal = Refusal {
    code: "engine_refused",
    when: "the Reporter refused the combined task before writing a result",
    remedy: "read error.detail.engine for the Reporter's own code and message",
};
const RESULT_INVALID: Refusal = Refusal {
    code: "report_result_invalid",
    when: "the Reporter's result or declared bytes are not this run's",
    remedy: "retry; if it persists, install matching ds and Reporter releases",
};
const OUTPUT_EXISTS: Refusal = Refusal {
    code: "report_output_exists",
    when: "--out-dir already holds a combined_transformer package",
    remedy: "choose a fresh --out-dir; a report is never overwritten",
};
const IDENTITY_CHANGED: Refusal = Refusal {
    code: "report_engine_identity_changed",
    when: "the Reporter was replaced while the combined report was running",
    remedy: "retry once the installation is stable",
};
const CONTRACT_MISMATCH: Refusal = Refusal {
    code: "callee_contract_mismatch",
    when: "the Reporter's build-info is not a release identity this ds admits",
    remedy: "install matching ds and Reporter releases",
};
const TIMED_OUT: Refusal = Refusal {
    code: "callee_timed_out",
    when: "the combined task exceeded its time bound",
    remedy: "retry, or narrow the scope with --transformer",
};
const ADMIN_BOUNDS: Refusal = Refusal {
    code: "admin_bounds_unavailable",
    when: "the governed Rwanda administrative reference is not installed",
    remedy: "install the governed Rwanda reference with ds workstation setup",
};
const UNSUPPORTED: Refusal = Refusal {
    code: "report_engine_unsupported",
    when: "the selected Reporter adapter has no combined task",
    remedy: "install a Reporter release with the combined task",
};

pub(super) const REFUSALS: &[Refusal] = &[
    super::NATIVE_PROFILE,
    super::NATIVE_PROFILE_DIGEST,
    super::NATIVE_PROFILE_UNSAFE,
    super::HEADLESS_SIGNED_OUT,
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
    COLLISIONS,
    ROOMS_UNAVAILABLE,
    EMPTY,
    MEDIA_SCOPE,
    MEDIA_GRANT,
    NOT_ACTIVE,
    ENGINE_MISSING,
    CONTRACT_MISMATCH,
    TIMED_OUT,
    UNSUPPORTED,
    ENGINE_REFUSED,
    EXPORT_BLOCKED,
    super::export::INPUTS_INVALID,
    super::export::STAGING_FAILED,
    OUTPUT_EXISTS,
    RESULT_INVALID,
    IDENTITY_CHANGED,
    ADMIN_BOUNDS,
    super::export::PUBLISH_LOCAL_ONLY,
    super::export::PUBLISH_SCOPE_CHANGED,
    super::export::PUBLISH_ROOT,
    hold::INPUTS_NOT_HELD,
    hold::ROOM_NOT_HELD,
    ds_cli_auth::config_document::CONTEXT_CHANGED,
    ds_cli_auth::config_document::STORE_REFUSED,
    ds_cli_auth::config_document::STALE,
    ds_cli_auth::config_document::ANSWER_INVALID,
];

pub static COMMAND: Command = Command {
    id: "report.project.combined",
    path: &["report", "project", "combined"],
    contract: 1,
    summary: "Produce the Combined Report data set locally and queue it.",
    purpose: "Builds the combined_transformer SHP, XLSX and GeoJSONSeq (plus GPKG or Network Information when the project's outputs name them; never a PDF) here from every active LV room, re-reading only moved rooms, and seals the set into the Sync queue unless --dry-run. A dry run uses the configuration this machine holds unless --refresh. Refuses before compute past the collision policy; missing voltage drop is only a warning.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        OUT_DIR_ARG,
        DRY_RUN_ARG,
        REFRESH_ARG,
        STATE_DIR_ARG,
        LANE_ARG,
        PROJECT_ARG,
    ],
    output: "ds.report-combined-result/v1: scope, rooms, formats, artifacts with digests (Sync locators when queued), publicationQueued, input fingerprint, collision gate, voltage-drop warnings, timings.",
    examples: &[
        Example {
            command: "ds report project combined --out-dir ./combined --dry-run --output json --project <exact-id>",
            note: "Local package only; `.data.publicationQueued` is false.",
            runnable: false,
        },
        Example {
            command: "ds report project combined --out-dir ./combined --output json --project <exact-id>",
            note: "Sealed into Sync; `ds report outbox drain` or the Sync Center publishes it.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &[
        "combined stats",
        "all transformers",
        "data set",
        "lv summary",
    ],
    requires: Requires::Server,
    availability: crate::reporter_availability,
};

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid(super::export::INPUTS_INVALID.code, message)
        .remedy(super::export::INPUTS_INVALID.remedy)
}

/// The run's output root, absolute and without an existing package.
fn out_root(inputs: &Inputs) -> Result<PathBuf, Failure> {
    let raw = PathBuf::from(inputs.require("out-dir")?);
    let root = if raw.is_absolute() {
        raw
    } else {
        std::env::current_dir()
            .map_err(|error| {
                Failure::failed(super::export::STAGING_FAILED.code, error.to_string())
                    .remedy(super::export::STAGING_FAILED.remedy)
            })?
            .join(raw)
    };
    if std::fs::symlink_metadata(root.join(decisions::IDENTITY)).is_ok() {
        return Err(Failure::conflict(
            OUTPUT_EXISTS.code,
            format!(
                "{} already holds a combined_transformer package",
                root.display()
            ),
        )
        .remedy(OUTPUT_EXISTS.remedy));
    }
    Ok(root)
}

/// Local refusals before any credential is restored or any state is read.
pub fn preflight(inputs: &Inputs) -> Result<(), Failure> {
    super::transformer_set(inputs)?;
    out_root(inputs)?;
    Ok(())
}

/// Literal arms only, so every code this command constructs is declared.
fn host_failure(failure: HostFailure) -> Failure {
    let message = failure.message.clone();
    let mapped = match failure.code.as_str() {
        "report_inputs_invalid" => invalid(message),
        "report_staging_failed" => Failure::failed("report_staging_failed", message)
            .remedy(super::export::STAGING_FAILED.remedy),
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
        "report_engine_unsupported" => {
            Failure::unavailable("report_engine_unsupported", message).remedy(UNSUPPORTED.remedy)
        }
        _ => Failure::internal("report_result_invalid", message),
    };
    match failure.detail {
        Some(detail) => mapped.detail(detail),
        None => mapped,
    }
}

fn same_context(
    expected: &ds_cli_auth::ProviderIdentity,
    project: &str,
    actual: &ds_cli_auth::ProviderIdentity,
    actual_project: &str,
) -> Result<(), Failure> {
    super::export::require_same_context(expected, project, actual, actual_project)
        .map_err(host_failure)
}

/// How a run reads the project configuration. Core's retained configuration
/// (the document `design config read` holds, forgotten by every native
/// configuration save) answers a run that publishes nothing, so an unchanged
/// rerun does not download the whole configuration again; the room heads are
/// still read from the one status listing. A publication, or `--refresh`,
/// reads it once from the service and retains it.
pub(super) fn configuration_read(publish: bool, refresh: bool) -> ProjectConfigurationRead {
    if publish || refresh {
        ProjectConfigurationRead::Refresh
    } else {
        ProjectConfigurationRead::Acquire
    }
}

/// The configuration receipt members a combined run reads, and nothing else.
pub(super) fn receipt_members(document: &ProjectConfigurationDocument) -> Value {
    use ds_command_kernel::report_export::{INPUT_RECEIPT_MEMBER, INPUT_RECEIPT_REFUSAL_MEMBER};
    Value::Object(
        [INPUT_RECEIPT_MEMBER, INPUT_RECEIPT_REFUSAL_MEMBER]
            .into_iter()
            .filter_map(|member| {
                document
                    .additional
                    .get(member)
                    .map(|value| (member.to_string(), value.clone()))
            })
            .collect(),
    )
}

/// The lifecycle inventory and the project's configuration receipt members:
/// the inventory read now and the configuration as `configuration_read`
/// says, or this machine's held copy when the service cannot be reached.
/// A combined run reads only these two; it never rewrites the held print
/// inputs an individual export keeps.
fn project_inputs(
    lane: &str,
    project: &str,
    identity: &ds_cli_auth::ProviderIdentity,
    hold: &Hold,
    link: &mut Link,
    mode: ProjectConfigurationRead,
) -> Result<(Vec<hold::Row>, Value, Value), Failure> {
    let inventory = link.read(|| {
        ds_cli_auth::transformer_inventory_for_project(lane, project, &TransformerSet::default())
    })?;
    let configuration = match &inventory {
        Some(_) => link.read(|| ds_cli_auth::config_document::read(lane, project, mode))?,
        None => None,
    };
    if let (Some(inventory), Some(configuration)) = (inventory, configuration) {
        same_context(
            identity,
            project,
            inventory.identity(),
            inventory.project_id(),
        )?;
        // The retained read fences its own protected principal; this run's
        // account must still be the one that read it.
        let now = ds_cli_auth::headless_identity_for_named_project(lane)?;
        same_context(identity, project, &now, &configuration.project)?;
        return Ok((
            hold::rows(inventory.result()),
            receipt_members(&configuration.document),
            json!({"source": "service", "configuration": configuration.observation}),
        ));
    }
    let unreachable = link.unreachable().unwrap_or("unreachable").to_string();
    let held = hold.inputs(&unreachable)?;
    Ok((
        held.rows,
        held.configuration,
        json!({"source": "held", "read_at": held.read_at, "unreachable": unreachable}),
    ))
}

/// The run's scope: every active LV transformer, or the exact names asked
/// for, each of which must be active.
pub(super) fn scope_names(
    rows: &[hold::Row],
    requested: &TransformerSet,
) -> Result<Vec<String>, Failure> {
    let active = |row: &hold::Row| {
        row.is_transformer()
            && row.is_active()
            && ds_command_kernel::report_export::reportable_transformer(&row.name).is_ok()
    };
    if requested.is_empty() {
        let mut names: Vec<String> = rows
            .iter()
            .filter(|row| active(row))
            .map(|row| row.name.clone())
            .collect();
        names.sort();
        names.dedup();
        return Ok(names);
    }
    let selected = hold::select(rows, requested);
    let inactive: Vec<String> = selected
        .iter()
        .filter(|row| !active(row))
        .map(|row| format!("{} ({})", row.name, row.state))
        .collect();
    if !inactive.is_empty() {
        return Err(Failure::conflict(
            NOT_ACTIVE.code,
            format!("not active saved transformers: {}", inactive.join(", ")),
        )
        .remedy(NOT_ACTIVE.remedy));
    }
    let mut names: Vec<String> = selected.into_iter().map(|row| row.name).collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// The project's status rows as heads for the room plan, the rows by name,
/// and whether the read answered at all.
fn status(
    lane: &str,
    project: &str,
    identity: &ds_cli_auth::ProviderIdentity,
    link: &mut Link,
) -> (Heads, Option<BTreeMap<String, Value>>) {
    match link.read(|| {
        ds_cli_auth::transformer_status_for_project(lane, project, &TransformerSet::default())
    }) {
        Ok(Some(status)) if status.identity() == identity && status.project_id() == project => {
            let rows = status.result().rows();
            let heads = rows
                .iter()
                .map(|row| {
                    json!({
                        "name": row.name(),
                        "version": hold::head_version(row.row()),
                        "layers": row.row().get("layers"),
                    })
                })
                .collect();
            let by_name = rows
                .iter()
                .map(|row| (row.name().to_string(), row.row().clone()))
                .collect();
            (Heads::Read(heads), Some(by_name))
        }
        Ok(Some(_)) => (
            Heads::Refused("the status answer belongs to another account or project".into()),
            None,
        ),
        Ok(None) => (Heads::Unreachable, None),
        Err(failure) => (
            Heads::Refused(format!("{}: {}", failure.code(), failure.message())),
            None,
        ),
    }
}

fn collision_refusal(gate: &decisions::CollisionGate) -> Failure {
    Failure::conflict(COLLISIONS.code, decisions::COLLISION_MESSAGE)
        .remedy(COLLISIONS.remedy)
        .next("ds design collisions")
        .detail(json!({
            "collision_rows": gate.collision_rows,
            "max_collision_rows": gate.policy.max_collision_rows,
            "allow_collisions": gate.policy.allow_with_collisions,
            "reason": gate.reason,
            "project_settings": [decisions::COLLISION_ALLOW_SETTING, decisions::COLLISION_MAX_SETTING],
        }))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    preflight(inputs)?;
    let started = Instant::now();
    let requested = super::transformer_set(inputs)?;
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let root = out_root(inputs)?;
    let publish = !inputs.switch("dry-run");
    let fence = publish
        .then(|| ds_cli_auth::capture_layer_scope_fence_for_project(lane, project))
        .transpose()?;
    let queue = publish
        .then(|| PublicationQueue::open(lane, inputs.value("core-state-dir").map(Path::new)))
        .transpose()?;
    let identity = ds_cli_auth::headless_identity_for_named_project(lane)?;
    if let Some(fence) = &fence {
        super::export::verify_publish_scope(lane, fence, fence.uid(), project)?;
    }
    let hold = Hold::open(&identity, project)?;
    let mut link = Link::default();

    let (rows, configuration, inputs_receipt) = project_inputs(
        lane,
        project,
        &identity,
        &hold,
        &mut link,
        configuration_read(publish, inputs.switch("refresh")),
    )?;
    let receipt = InputReceipt::from_config(&configuration).map_err(invalid)?;
    let names = scope_names(&rows, &requested)?;
    if names.is_empty() {
        return Err(Failure::conflict(
            EMPTY.code,
            "the named project has no active LV transformer to combine",
        )
        .remedy(EMPTY.remedy));
    }
    let sheets = receipt.sheets().map_err(invalid)?;
    let (heads, status_rows) = status(lane, project, &identity, &mut link);
    let gate = decisions::collision_gate(
        &sheets,
        status_rows.as_ref().map(|rows| rows.get(COLLISIONS_ROW)),
    );
    if !gate.allowed {
        return Err(collision_refusal(&gate));
    }
    let voltage_drop = status_rows
        .as_ref()
        .map(|rows| decisions::voltage_drop_warnings(rows, &names));

    let admin_bounds = if receipt.requires_admin_bounds() {
        let shared = ds_report_host::shared_root().map_err(|error| {
            Failure::unavailable(ADMIN_BOUNDS.code, error).remedy(ADMIN_BOUNDS.remedy)
        })?;
        let path = ds_report_host::installed_admin_bounds_path(
            &shared,
            &receipt.reference_semantic_sha256,
        );
        Some(
            ds_report_host::verify_admin_bounds_asset(&path, &receipt.reference_semantic_sha256)
                .map_err(host_failure)?,
        )
    } else {
        None
    };
    let engine = CliEngine::capture(identity.lane()).map_err(host_failure)?;
    let staging_root = root.join(STAGING_DIRECTORY);
    let mut staging = CombinedStaging::begin(&CombinedSettings {
        project_id: project,
        receipt: &receipt,
        admin_bounds: admin_bounds.as_ref(),
        staging_root: &staging_root,
        out_root: &root,
    })
    .map_err(host_failure)?;

    let mut warnings: Vec<String> = Vec::new();
    let natures = match link
        .read(|| hold::transformer_nature_view(lane, project, &identity, &names))
    {
        Ok(Some(natures)) => natures,
        Ok(None) => {
            warnings.push(format!(
                "transformer natures were not read ({}); each room keeps its saved nature",
                link.unreachable().unwrap_or("unreachable")
            ));
            None
        }
        Err(failure) => {
            warnings.push(format!(
                "transformer natures could not be verified ({}: {}); each room keeps its saved nature",
                failure.code(),
                failure.message()
            ));
            None
        }
    };

    let mut rooms = Rooms::plan(hold.clone(), &names, heads)?;
    let mut failures: Vec<Value> = Vec::new();
    for name in &names {
        let room = rooms.room(name, &mut link, || {
            let context = ds_cli_auth::saved_transformer_context_for_project(lane, project, name)?;
            same_context(
                &identity,
                project,
                context.identity(),
                context.snapshot().ds_project(),
            )?;
            let snapshot = context.snapshot();
            if snapshot.transformer_name() != name {
                return Err(invalid(format!(
                    "the service answered for another transformer than {name}"
                )));
            }
            Ok(ds_project_data::room_hold::Room {
                transformer: name.clone(),
                version: snapshot
                    .metadata()
                    .version()
                    .and_then(|version| i64::try_from(version).ok()),
                content_digest: snapshot.metadata().content_digest().map(str::to_owned),
                layers: snapshot.layers().clone(),
            })
        });
        let staged = room.and_then(|room| {
            let revision = room.version.filter(|version| *version > 0).ok_or_else(|| {
                invalid(format!(
                    "{name} has no saved revision; save it before reporting"
                ))
            })?;
            staging
                .add_room(name, revision, room.content_digest.as_deref(), &room.layers)
                .map_err(host_failure)
        });
        if let Err(failure) = staged {
            failures.push(json!({
                "transformer": name,
                "code": failure.code(),
                "message": failure.message(),
            }));
        }
    }
    if !failures.is_empty() {
        let count = failures.len();
        let named = failures
            .iter()
            .take(MAX_NAMED_FAILURES)
            .filter_map(|failure| failure["transformer"].as_str())
            .collect::<Vec<_>>()
            .join(", ");
        failures.truncate(MAX_NAMED_FAILURES);
        return Err(Failure::unavailable(
            ROOMS_UNAVAILABLE.code,
            format!(
                "{count} of {} rooms could not be staged ({named}); no combined report was produced",
                names.len(),
            ),
        )
        .remedy(ROOMS_UNAVAILABLE.remedy)
        .detail(json!({"count": count, "rooms": failures, "acquisition": rooms.receipt()})));
    }
    if staging.staged_rooms() == 0 {
        return Err(Failure::conflict(
            EMPTY.code,
            format!(
                "none of the {} transformers in scope holds a report feature",
                names.len()
            ),
        )
        .remedy(EMPTY.remedy)
        .detail(json!({"empty_rooms": staging.empty_rooms()})));
    }
    let acquisition_ms = started.elapsed().as_millis() as u64;

    let media = staging.media_scope(&engine).map_err(host_failure)?;
    let grant = if media.reference_count == 0 {
        None
    } else {
        let foreign = media.foreign_projects(project);
        if !foreign.is_empty() {
            return Err(Failure::conflict(
                MEDIA_SCOPE.code,
                format!(
                    "{} photo reference(s) are stored under other projects: {}",
                    media.reference_count,
                    foreign.join(", ")
                ),
            )
            .remedy(MEDIA_SCOPE.remedy)
            .detail(json!({"source_projects": media.source_projects})));
        }
        let document = match link.read(|| ds_cli_auth::report_media_grant(lane, project)) {
            Ok(Some(grant)) => {
                same_context(&identity, project, grant.identity(), grant.project_id())?;
                grant
                    .result()
                    .report_grant_document()
                    .map_err(|error| error.to_string())
            }
            Ok(None) => Err(format!(
                "the service could not be reached: {}",
                link.unreachable().unwrap_or("unreachable")
            )),
            Err(failure) => Err(format!("{}: {}", failure.code(), failure.message())),
        };
        Some(document.map_err(|detail| {
            Failure::unavailable(
                MEDIA_GRANT.code,
                format!(
                    "{} photo reference(s) need the project's report media grant ({detail})",
                    media.reference_count
                ),
            )
            .remedy(MEDIA_GRANT.remedy)
        })?)
    };

    let participating = staging.staged_rooms();
    let empty_rooms = staging.empty_rooms().to_vec();
    let client_run_id = staging.client_run_id().to_owned();
    let compute_started = Instant::now();
    let outcome = staging
        .run(
            &engine,
            &CombinedExtras {
                media_grant: grant.as_deref(),
                transformer_natures: natures.as_ref(),
            },
        )
        .map_err(host_failure)?;
    let compute_ms = compute_started.elapsed().as_millis() as u64;
    // The staging root is this run's scratch; remove it when empty.
    let _ = std::fs::remove_dir(&staging_root);

    let seal_started = Instant::now();
    let publication = match (&queue, &fence) {
        (Some(queue), Some(fence)) => {
            let guard = || {
                let now = ds_cli_auth::headless_identity_for_named_project(lane)?;
                same_context(&identity, project, &now, project)?;
                super::export::verify_publish_scope(lane, fence, fence.uid(), project)
            };
            Some(super::export::seal_run_for_server(
                &outcome.run,
                identity.uid(),
                project,
                queue,
                &guard,
            )?)
        }
        _ => None,
    };
    let seal_ms = seal_started.elapsed().as_millis() as u64;

    let mut output = json!({
        "schema": "ds.report-combined-result/v1",
        "project": project,
        "lane": identity.lane(),
        "transformer": decisions::IDENTITY,
        "outDir": root.join(decisions::IDENTITY).display().to_string(),
        "scope": {
            "mode": if requested.is_empty() { "all_active" } else { "explicit" },
            "transformers": names.len(),
            "participating": participating,
            "emptyRooms": empty_rooms,
        },
        "inputs": inputs_receipt,
        "rooms": rooms.receipt(),
        "formats": outcome.run.artifacts.iter().map(|artifact| artifact.output_id.clone()).collect::<Vec<_>>(),
        "clientRunId": client_run_id,
        "engineVersion": outcome.run.engine.engine_version,
        "engineBuildManifestSha256": outcome.run.engine.build_manifest_sha256,
        "inputBaseFingerprint": outcome.input_base_fingerprint,
        "roomContentSha256": outcome.room_content_sha256,
        "layersAggregated": outcome.result.layers_aggregated,
        "featuresAggregated": outcome.result.features_aggregated,
        "collisions": gate,
        "voltageDrop": match &voltage_drop {
            Some(found) => json!({
                "observed": true,
                "warningCount": found.len(),
                "warnings": found.iter().take(decisions::MAX_VOLTAGE_DROP_WARNINGS).collect::<Vec<_>>(),
            }),
            None => json!({"observed": false, "warningCount": null, "warnings": []}),
        },
        "engineeringBlockers": outcome.result.engineering_blockers,
        // Missing or invalid facts the workbook itemizes on its Dirty
        // Categories sheet; null from a Reporter that predates them.
        "engineeringDirtyFacts": outcome.result.dirty_facts,
        "transformerNatures": natures.is_some(),
        "mediaReferences": media.reference_count,
        "transformerReviews": "not_included",
        "timings": {
            "acquisitionMs": acquisition_ms,
            "computeMs": compute_ms,
            "sealMs": seal_ms,
            "reporter": outcome.result.phase_timings,
        },
    });
    if !empty_rooms.is_empty() {
        warnings.push(format!(
            "{} transformer(s) hold no report feature and contributed nothing: {}",
            empty_rooms.len(),
            empty_rooms.join(", ")
        ));
    }
    if let Some(found) = voltage_drop.as_ref().filter(|found| !found.is_empty()) {
        warnings.push(format!(
            "{} transformer(s) have no usable saved voltage-drop analysis (missing, stale or not calculated); the Combined Report selects no voltage-drop output, so voltageDrop.warnings names them instead",
            found.len()
        ));
    }
    let findings = outcome.result.engineering_blockers
        + outcome
            .result
            .dirty_facts
            .as_ref()
            .map_or(0, |facts| facts.features as u64);
    if findings > 0 {
        warnings.push(format!(
            "the combined workbook carries {findings} engineering finding(s) on its Dirty Categories sheet (engineeringDirtyFacts names each transformer's fields); review them before delivery"
        ));
    }
    output["warnings"] = json!(warnings);
    match publication {
        Some(SealedRun::Queued(sealed)) => {
            output["publicationQueued"] = json!(true);
            output["batchId"] = json!(sealed.receipt.batch_id);
            output["clientPublishId"] = json!(sealed.receipt.client_publish_id);
            output["artifacts"] = json!(sealed.artifacts);
        }
        Some(SealedRun::AlreadyRecorded { replay_key, state }) => {
            let queue = queue.as_ref().expect("a sealed run opened its queue");
            let batch = ds_report_artifacts::list_project_publication_batches_from_root(
                &queue.root,
                project,
            )
            .map_err(invalid)?
            .into_iter()
            .find(|batch| batch.client_publish_id == replay_key && batch.owner_uid == identity.uid())
            .ok_or_else(|| {
                Failure::failed(
                    RESULT_INVALID.code,
                    format!("the Sync queue already holds this publication ({}) but its batch is not committed here", state.as_str()),
                )
                .remedy(RESULT_INVALID.remedy)
            })?;
            output["publicationQueued"] = json!(true);
            output["alreadyRecorded"] = json!(true);
            output["batchId"] = json!(batch.batch_id);
            output["clientPublishId"] = json!(batch.client_publish_id);
            output["artifacts"] = json!(batch.outputs);
        }
        None => {
            output["publicationQueued"] = json!(false);
            output["publication"] = json!({
                "stage": "nothing_published",
                "published_nothing": true,
                "reason": "dry_run",
                "note": "--dry-run produced the local package and published nothing; run without it to seal the set into Sync.",
            });
            output["artifacts"] = json!(
                outcome
                    .run
                    .artifacts
                    .iter()
                    .map(|artifact| json!({
                        "outputId": artifact.output_id,
                        "format": artifact.format,
                        "filename": artifact.filename,
                        "contentType": artifact.content_type,
                        "sha256": artifact.sha256,
                        "sizeBytes": artifact.size_bytes,
                    }))
                    .collect::<Vec<_>>()
            );
        }
    }
    Ok(output)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} · {} · Combined Report · {} of {} transformer(s) · {}\n",
        data["project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["scope"]["participating"].as_u64().unwrap_or(0),
        data["scope"]["transformers"].as_u64().unwrap_or(0),
        if data["publicationQueued"] == true {
            "queued for Sync"
        } else {
            "published nothing"
        },
    );
    for artifact in data["artifacts"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<44} {:>12} bytes\n",
            artifact["filename"].as_str().unwrap_or("?"),
            artifact["sizeBytes"].as_u64().unwrap_or(0),
        ));
    }
    if let Some(count) = data["voltageDrop"]["warningCount"]
        .as_u64()
        .filter(|count| *count > 0)
    {
        out.push_str(&format!(
            "  {count} transformer(s) without a usable saved voltage-drop analysis\n"
        ));
    }
    for warning in data["warnings"].as_array().into_iter().flatten() {
        out.push_str(&format!("  warning: {}\n", warning.as_str().unwrap_or("?")));
    }
    out
}

#[cfg(test)]
#[path = "combined_export_tests.rs"]
mod tests;
