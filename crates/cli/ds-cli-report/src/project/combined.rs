//! `ds report project compounded` — the project's Compounded Report ZIP,
//! composed on this machine and queued for governed publication.
//!
//! Nothing here is computed in the cloud. The scope comes from the lifecycle
//! inventory; each participating transformer contributes the current
//! individual report set this machine committed (`ds report project export`
//! sealed it), checked against the transformer's saved revision; the overall
//! combined data set — and one per first-level applied group when asked — is
//! computed here by the same owner `ds report project combined` runs. The
//! kernel (`report::compounded`) plans the archive tree from the applied
//! `report_archive` grouping, the Reporter's closed `export-compounded-report`
//! task builds the ZIP by request and response, and the ZIP is sealed into the
//! Sync queue as `export-compounded_report`, which Sync publishes through the
//! compute-artifact door. ds-brain only stores it.
//!
//! The command REFUSES rather than shrink the archive: a room whose report is
//! missing here, or older than its saved revision, fails through
//! [`ds_command_kernel::combined_readiness`] — the predicate the Desktop
//! dialog refuses from — naming the rooms and the command that makes them
//! current. Nothing is sealed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ds_cli_auth::TransformerSet;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::combined_readiness::{self, RoomInput};
use ds_command_kernel::report::PublicationState;
use ds_command_kernel::report::compounded::{
    self as decisions, CombinedSet, Composition, GroupPaths, Layout, Member, PlanInput, StagedFile,
};
use ds_report_artifacts::{ArtifactReadAuthority, VerifiedSidecarArtifact};
use ds_sync_runtime::reports::{SealOutcome, SealRequest};
use serde_json::{Value, json};

use super::export::PublicationQueue;
use super::grouping::{self, GROUP_BY_ARG, WHERE_ARG};
use super::{LANE_ARG, PROJECT_ARG, TRANSFORMER_ARG};
use crate::EXPORT_TIMEOUT;

pub(super) const FILE_LEVEL_ARG: Arg = Arg::value(
    "file-level",
    "<transformer|sector|district|root>",
    "Requested folder level; district/sector folders need resolved administrative values.",
)
.default("transformer")
.choices(&["transformer", "sector", "district", "root"]);
pub(super) const COMBINE_PER_GROUP_ARG: Arg = Arg::switch(
    "combine-per-group",
    "Also file one combined set for each first-level applied report group.",
);
pub(super) const FORCE_ARG: Arg = Arg::switch(
    "force",
    "Recorded in the composition; individual reports come only from `report project export`.",
);
const STATE_DIR_ARG: Arg = Arg::value(
    "core-state-dir",
    "<absolute-path>",
    "Publication queue root; the same absolute path as ds core serve when overridden.",
);

pub(super) const ARGS: &[Arg] = &[
    Arg::value(
        "composition",
        "<json>",
        "ds.compounded-report/v1 JSON object (schema, transformers, archive_layout, force). Use instead of scope/layout/force flags.",
    ),
    TRANSFORMER_ARG,
    GROUP_BY_ARG,
    WHERE_ARG,
    FILE_LEVEL_ARG,
    COMBINE_PER_GROUP_ARG,
    FORCE_ARG,
    STATE_DIR_ARG,
    LANE_ARG,
    PROJECT_ARG,
];

pub(super) const PURPOSE: &str = "\
Compose the project ZIP on this machine and queue it for governed publication. \
Each active transformer contributes the current individual report held here \
(`report project export`); the combined set (and one per first-level group with \
--combine-per-group) is computed here. Applied report_archive grouping supplies \
the folders; Sync publishes the ZIP as export-compounded_report (`report outbox drain`). \
A missing or older room report refuses before any archive. --group-by/--where give one \
archive per leaf group (_unassigned holds untagged rooms). --composition takes \
the ds.compounded-report/v1 JSON instead of scope/layout/force flags. \
No URL or action override.";

pub(super) const OUTPUT: &str = "\
Lane, project ID, requested scope, and `archive_layout` — the layout \
asked for, never the tree achieved: unresolved administrative values collapse \
to `_unassigned/`. Then the receipt: status, `prefix`, `archives` (the sealed \
archive's locator), `publication` (operation, batch, publish id; `ds report \
outbox drain` publishes it and `ds report publication show` reads it \
back), individual coverage, missing individuals with \
causes, errors and registry-write failure. \
Grouped: `grouping` and one `groups` receipt per archive.";

pub(super) const GROUPING_INVALID: Refusal = Refusal {
    code: "compounded_grouping_invalid",
    when: "the applied report_archive grouping plan is malformed or files a transformer twice",
    remedy: "read it with `ds design consumer-grouping read` and apply it again",
};
pub(super) const INPUTS_INVALID: Refusal = Refusal {
    code: "compounded_inputs_invalid",
    when: "the inputs cannot form one archive: shared path, unpinned file, or a receipt not from this plan",
    remedy: "report this build with the detail; nothing was sealed",
};
pub(super) const ARCHIVE_TOO_LARGE: Refusal = Refusal {
    code: "compounded_archive_too_large",
    when: "the composed ZIP exceeds the 64 MiB the project publication door admits",
    remedy: "narrow --transformer or use --group-by; nothing was sealed",
};
pub(super) const STORE_UNREADABLE: Refusal = Refusal {
    code: "compounded_store_unreadable",
    when: "the committed report store or a held report could not be read and verified",
    remedy: "regenerate the named room with `ds report project export --transformer <name>`, then retry",
};

pub static COMMAND: Command = Command {
    id: "report.project.compounded",
    path: &["report", "project", "compounded"],
    contract: 1,
    summary: "Compose a Compounded Report ZIP here and queue it (needs --yes).",
    purpose: PURPOSE,
    chapter: Chapter::Reports,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: ARGS,
    output: OUTPUT,
    examples: &[
        Example {
            command: "ds report project compounded --file-level sector --yes --output json --project <exact-id>",
            note: "`ds report outbox drain --project <exact-id>` then publishes it.",
            runnable: false,
        },
        Example {
            command: "ds report project compounded --group-by district --group-by city --where phase=i --yes --output json --project <exact-id>",
            note: "An archive per district/city pair, phase i only.",
            runnable: false,
        },
    ],
    refusals: super::COMBINED_REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &[
        "combined archive",
        "per city",
        "group by",
        "composition json",
        "archive recipe",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// One archive request: the closed composition, plus the legacy level it
/// was spelled with (the receipt reports what was asked for).
struct Request {
    composition: Composition,
    file_level: Option<&'static str>,
}

fn refused(refusal: decisions::Refusal) -> Failure {
    let declared = match refusal.code {
        decisions::GROUPING_INVALID => GROUPING_INVALID,
        decisions::ARCHIVE_TOO_LARGE => ARCHIVE_TOO_LARGE,
        decisions::COMPOSITION_INVALID => {
            return Failure::invalid("compounded_composition_invalid", refusal.message)
                .remedy("correct the JSON: schema, transformers, archive_layout and force");
        }
        _ => INPUTS_INVALID,
    };
    match declared.code {
        "compounded_grouping_invalid" => {
            Failure::conflict("compounded_grouping_invalid", refusal.message)
        }
        "compounded_archive_too_large" => {
            Failure::conflict("compounded_archive_too_large", refusal.message)
        }
        _ => Failure::failed("compounded_inputs_invalid", refusal.message),
    }
    .remedy(declared.remedy)
}

fn store_unreadable(message: impl Into<String>) -> Failure {
    Failure::failed("compounded_store_unreadable", message).remedy(STORE_UNREADABLE.remedy)
}

fn level_token(level: &str) -> &'static str {
    match level {
        "sector" => "sector",
        "district" => "district",
        "root" => "root",
        _ => "transformer",
    }
}

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let (lane, project) = (inputs.require("lane")?, inputs.require("project")?);
    let state = inputs.value("core-state-dir").map(Path::new);
    if let Some(composition) = composition_request(inputs)? {
        let request = Request {
            composition,
            file_level: None,
        };
        return publish(lane, project, state, &request, context);
    }
    let level = level_token(inputs.require("file-level")?);
    let layout =
        Layout::from_file_level(level, inputs.switch("combine-per-group")).map_err(refused)?;
    let force = inputs.switch("force");
    if let Some(requested) = grouping::requested(inputs)? {
        return run_grouped(
            lane, project, state, &requested, layout, level, force, context,
        );
    }
    let transformers = super::transformer_set(inputs)?;
    let request = Request {
        composition: Composition {
            transformers: transformers.names().to_vec(),
            archive_layout: layout,
            force,
        },
        file_level: Some(level),
    };
    publish(lane, project, state, &request, context)
}

/// Refuse conflicting grouped and transformer scopes before reconciliation.
pub fn preflight(inputs: &Inputs) -> Result<(), Failure> {
    if composition_request(inputs)?.is_some() {
        return Ok(());
    }
    if grouping::requested(inputs)?.is_none() {
        super::transformer_set(inputs)?;
    }
    Ok(())
}

fn composition_request(inputs: &Inputs) -> Result<Option<Composition>, Failure> {
    let Some(raw) = inputs.value("composition") else {
        return Ok(None);
    };
    if !inputs.repeated("transformer").is_empty()
        || !inputs.repeated("group-by").is_empty()
        || !inputs.repeated("where").is_empty()
        || inputs
            .value("file-level")
            .is_some_and(|level| level != "transformer")
        || inputs.switch("combine-per-group")
        || inputs.switch("force")
    {
        return Err(Failure::invalid(
            "compounded_composition_conflict",
            "composition cannot be mixed with scope, grouping, layout or force flags",
        )
        .remedy("pass composition or ordinary flags"));
    }
    if raw.len() > decisions::MAX_COMPOSITION_BYTES {
        return Err(refused(decisions::Refusal {
            code: decisions::COMPOSITION_INVALID,
            message: "composition exceeds 64 KiB".into(),
        }));
    }
    let value: Value = serde_json::from_str(raw).map_err(|error| {
        refused(decisions::Refusal {
            code: decisions::COMPOSITION_INVALID,
            message: error.to_string(),
        })
    })?;
    Composition::parse(&value).map(Some).map_err(refused)
}

/// One leaf tag group per archive, run one after another through the SAME
/// publish a single run uses, each over that group's explicit scope.
///
/// Every group's scope is validated before the first archive is composed.
/// A refusal about one group's rooms lets the next group run; any other
/// refusal would refuse every group alike, so the rest are left `not_run`.
/// A run that did not publish every group never exits zero.
#[allow(clippy::too_many_arguments)]
fn run_grouped(
    lane: &str,
    project: &str,
    state: Option<&Path>,
    requested: &grouping::Requested,
    layout: Layout,
    level: &'static str,
    force: bool,
    context: &Context,
) -> Result<Value, Failure> {
    let grouped = grouping::resolve(lane, project, requested)?;
    let scopes = grouping::group_scopes(&grouped.plan)?;
    let summary = grouping::grouping_json(&grouped, false);
    let mut receipts: Vec<Value> = Vec::new();
    let mut refusals: Vec<Failure> = Vec::new();
    let mut stopped = false;
    for (group, scope) in grouped.plan.groups.iter().zip(scopes) {
        if stopped {
            receipts.push(grouping::not_run(group));
            continue;
        }
        let request = Request {
            composition: Composition {
                transformers: scope.names().to_vec(),
                archive_layout: layout,
                force,
            },
            file_level: Some(level),
        };
        match publish(lane, project, state, &request, context) {
            Ok(output) => receipts.push(grouping::published(group, &output)),
            Err(failure) => {
                receipts.push(grouping::refused(group, &failure));
                stopped = !grouping::group_scoped(&failure);
                refusals.push(failure);
            }
        }
    }
    let published = receipts.len() - refusals.len() - not_run_count(&receipts);
    if published == 0 && refusals.len() == 1 {
        // Exactly one attempt, and it refused: the caller needs THAT refusal,
        // with its own code, remedy and detail, not a summary of one.
        return Err(refusals.remove(0));
    }
    if published < receipts.len() {
        return Err(grouping::partial(summary, receipts));
    }
    let mut output = grouped.receipt;
    let fields = json!({
        "scope": {"mode": "grouped", "requested": []},
        "grouping": summary,
        "archive_layout": archive_layout(level, layout.combine_per_group),
        "force": force,
        "published_count": published,
        "groups": receipts,
    });
    output
        .as_object_mut()
        .expect("receipt is an object")
        .extend(fields.as_object().expect("fields are an object").clone());
    Ok(output)
}

fn not_run_count(receipts: &[Value]) -> usize {
    receipts
        .iter()
        .filter(|receipt| receipt["status"] == "not_run")
        .count()
}

/// A run's private scratch beside the Core state it publishes into: on disk,
/// never tmpfs, and removed on every path.
struct Stage(PathBuf);

impl Stage {
    fn begin(queue: &PublicationQueue, run: &str) -> Result<Self, Failure> {
        let parent = queue
            .root
            .parent()
            .ok_or_else(|| store_unreadable("the publication queue has no state directory"))?;
        let path = parent.join("compounded-staging").join(run);
        std::fs::create_dir_all(&path).map_err(|error| {
            Failure::failed(
                "report_staging_failed",
                format!("{}: {error}", path.display()),
            )
            .remedy(super::export::STAGING_FAILED.remedy)
        })?;
        Ok(Self(path))
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default()
}

/// The current saved revision of every transformer the status rows name.
fn saved_revisions(lane: &str, project: &str) -> Result<BTreeMap<String, i64>, Failure> {
    let status =
        ds_cli_auth::transformer_status_for_project(lane, project, &TransformerSet::default())
            .map_err(|failure| {
                Failure::unavailable(
                    super::COMBINED_READINESS_UNAVAILABLE.code,
                    format!(
                        "the saved transformer revisions could not be read ({}: {}), so no held report can be called current",
                        failure.code(),
                        failure.message()
                    ),
                )
                .remedy(super::COMBINED_READINESS_UNAVAILABLE.remedy)
            })?;
    Ok(status
        .result()
        .rows()
        .iter()
        .filter_map(|row| {
            super::hold::head_version(row.row())
                .as_i64()
                .map(|version| (row.name().to_owned(), version))
        })
        .collect())
}

fn cause(name: &str, code: &str, detail: String) -> Value {
    json!({"transformer": name, "code": code, "detail": detail})
}

/// Each participating room's current committed individual set, staged and
/// verified byte for byte; a room without one is a readiness cause.
fn individual_sets(
    queue: &PublicationQueue,
    owner: &str,
    project: &str,
    names: &[String],
    revisions: &BTreeMap<String, i64>,
    stage: &Path,
) -> Result<(Vec<Member>, Vec<Value>), Failure> {
    let batches =
        ds_report_artifacts::list_project_publication_batches_from_root(&queue.root, project)
            .map_err(store_unreadable)?;
    let (mut members, mut causes) = (Vec::new(), Vec::new());
    for name in names {
        let operation = format!("export-{name}");
        // Newest first: the room's standing committed generation.
        let Some(batch) = batches.iter().find(|batch| {
            batch.owner_uid == owner
                && batch.operation == operation
                && batch.variant == "default"
                && batch.printing_preview.is_none()
                && !batch.outputs.is_empty()
        }) else {
            causes.push(cause(
                name,
                "report_not_published",
                "this machine holds no individual report for the room".into(),
            ));
            continue;
        };
        let directory = stage.join("individual").join(name);
        std::fs::create_dir_all(&directory).map_err(|error| store_unreadable(error.to_string()))?;
        let mut files = Vec::with_capacity(batch.outputs.len());
        let mut revision = None;
        for output in &batch.outputs {
            let authority = ArtifactReadAuthority {
                owner_uid: owner.to_owned(),
                project_id: project.to_owned(),
                operation: operation.clone(),
                variant: "default".to_owned(),
                output_id: output.output_id.clone(),
                sha256: output.sha256.clone(),
                size_bytes: output.size_bytes,
                content_type: output.content_type.clone(),
            };
            let opened = ds_report_artifacts::open_artifact_from_root_with_gate(
                &queue.root,
                &output.locator,
                |receipt| authority.authorize(receipt, &output.locator),
            )
            .map_err(|error| store_unreadable(format!("{name}: {error}")))?;
            let held = opened.committed.transformer_revision;
            if revision.is_some_and(|seen| seen != held) {
                return Err(store_unreadable(format!(
                    "{name}: one committed set names two revisions"
                )));
            }
            revision = Some(held);
            if revisions.get(name) != Some(&held) {
                // Stale or unknown: no byte of this room is copied.
                break;
            }
            let target = directory.join(&output.filename);
            crate::inventory_copy::copy_verified(
                opened.file,
                &target,
                &output.sha256,
                output.size_bytes,
                &|| Ok(()),
            )
            .map_err(|failure| store_unreadable(format!("{name}: {}", failure.message())))?;
            files.push(StagedFile {
                filename: output.filename.clone(),
                source: target.display().to_string(),
                sha256: output.sha256.clone(),
                size_bytes: output.size_bytes,
            });
        }
        let held = revision.unwrap_or_default();
        match revisions.get(name) {
            Some(saved) if *saved == held => members.push(Member {
                transformer: name.clone(),
                revision: held,
                batch_id: batch.batch_id.clone(),
                files,
            }),
            Some(saved) => causes.push(cause(
                name,
                "report_stale",
                format!(
                    "the held report is of revision {held}; the saved design is at revision {saved}"
                ),
            )),
            None => causes.push(cause(
                name,
                "report_revision_unknown",
                "the room's saved revision could not be read".into(),
            )),
        }
    }
    Ok((members, causes))
}

/// One combined data set, computed here by the `report project combined`
/// owner over exactly these rooms, as a local package (it seals nothing).
fn combined_set(
    lane: &str,
    project: &str,
    state: Option<&Path>,
    names: &[String],
    group: Option<String>,
    out: &Path,
    context: &Context,
) -> Result<CombinedSet, Failure> {
    let mut tokens = vec![
        "--project".to_owned(),
        project.to_owned(),
        "--lane".to_owned(),
        lane.to_owned(),
        "--out-dir".to_owned(),
        out.display().to_string(),
        "--dry-run".to_owned(),
    ];
    if let Some(state) = state {
        tokens.extend(["--core-state-dir".to_owned(), state.display().to_string()]);
    }
    for name in names {
        tokens.extend(["--transformer".to_owned(), name.clone()]);
    }
    let inputs = ds_cli_contract::args::parse(&super::combined_export::COMMAND, &tokens)?;
    let result = super::combined_export::run(&inputs, context)?;
    let directory = PathBuf::from(result["outDir"].as_str().unwrap_or_default());
    let files = result["artifacts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|artifact| {
            let filename = artifact["filename"].as_str().unwrap_or_default().to_owned();
            StagedFile {
                source: directory.join(&filename).display().to_string(),
                filename,
                sha256: artifact["sha256"].as_str().unwrap_or_default().to_owned(),
                size_bytes: artifact["sizeBytes"].as_u64().unwrap_or_default(),
            }
        })
        .collect();
    Ok(CombinedSet {
        group,
        transformers: names.to_vec(),
        files,
    })
}

/// The Reporter's closed bundle task, by request and response, and the
/// identity of the build that ran it.
fn bundle(
    lane: &str,
    plan: &decisions::BundlePlan,
    stage: &Path,
) -> Result<
    (
        decisions::BundleReceipt,
        ds_command_kernel::report_export::EngineIdentity,
    ),
    Failure,
> {
    let engine = crate::Reporter::capture_for_lane(lane)?;
    let identity = ds_command_kernel::report_export::engine_identity(&engine.build_info()?)
        .map_err(|error| {
            Failure::failed("callee_contract_mismatch", error)
                .remedy("install matching ds and Reporter releases")
        })?;
    let request = stage.join("bundle-request.json");
    let result = stage.join("bundle-result.json");
    std::fs::write(
        &request,
        serde_json::to_vec(&plan.request).expect("a bundle plan encodes"),
    )
    .map_err(|error| store_unreadable(error.to_string()))?;
    let completed = engine.run(
        crate::ReporterTask::ExportCompoundedReport,
        &request,
        &result,
        EXPORT_TIMEOUT,
    )?;
    if !completed.succeeded() {
        return Err(completed.refusal());
    }
    let document: Value = std::fs::read(&result)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or_else(|| {
            Failure::failed("engine_refused", "the Reporter returned no bundle receipt")
                .remedy("read error.detail.engine for the Reporter's own code and message")
                .detail(json!({"engine": completed.stderr()}))
        })?;
    Ok((
        decisions::admit_receipt(plan, &document).map_err(refused)?,
        identity,
    ))
}

/// One Compounded Report archive over one request's scope.
fn publish(
    lane: &str,
    project: &str,
    state: Option<&Path>,
    request: &Request,
    context: &Context,
) -> Result<Value, Failure> {
    let composition = &request.composition;
    let identity = ds_cli_auth::headless_identity_for_named_project(lane)?;
    let fence = ds_cli_auth::capture_layer_scope_fence_for_project(lane, project)?;
    super::export::verify_publish_scope(lane, &fence, fence.uid(), project)?;
    let queue = PublicationQueue::open(lane, state)?;

    // Scope: every active saved transformer, or the exact active names asked.
    let requested =
        TransformerSet::new(composition.transformers.iter().cloned()).map_err(|error| {
            Failure::invalid("invalid_transformer_scope", error.to_string())
                .remedy(super::INVALID_SCOPE.remedy)
        })?;
    let inventory =
        ds_cli_auth::transformer_inventory_for_project(lane, project, &TransformerSet::default())?;
    if inventory.identity() != &identity || inventory.project_id() != project {
        return Err(Failure::conflict(
            super::export::PUBLISH_SCOPE_CHANGED.code,
            "the account or project changed while reading the scope",
        )
        .remedy(super::export::PUBLISH_SCOPE_CHANGED.remedy));
    }
    let names =
        super::combined_export::scope_names(&super::hold::rows(inventory.result()), &requested)?;
    if names.is_empty() {
        return Err(Failure::conflict(
            super::COMBINED_NO_INPUTS.code,
            "no active LV transformer is in scope; no archive was composed",
        )
        .remedy(super::COMBINED_NO_INPUTS.remedy)
        .next("ds report project scope"));
    }

    // Currency: each room's committed set against its saved revision.
    let revisions = saved_revisions(lane, project)?;
    let run = format!("{}-{}", std::process::id(), now_ms());
    let stage = Stage::begin(&queue, &run)?;
    let (members, causes) = individual_sets(
        &queue,
        identity.uid(),
        project,
        &names,
        &revisions,
        &stage.0,
    )?;
    if let Some(failure) = readiness_refusal(&causes)? {
        return Err(failure);
    }

    // The tree, and the combined sets computed here.
    let groups = if composition.archive_layout.needs_grouping() {
        let plan = ds_cli_auth::applied_report_archive_grouping(lane, project)?;
        decisions::group_paths(plan.result().as_ref()).map_err(refused)?
    } else {
        GroupPaths::default()
    };
    let mut combined = vec![combined_set(
        lane,
        project,
        state,
        &names,
        None,
        &stage.0.join("combined"),
        context,
    )?];
    if composition.archive_layout.combine_per_group {
        for (index, (group, scope)) in decisions::first_level_groups(&groups, &names)
            .into_iter()
            .enumerate()
        {
            combined.push(combined_set(
                lane,
                project,
                state,
                &scope,
                Some(group),
                &stage.0.join(format!("combined-group-{index}")),
                context,
            )?);
        }
    }
    let output = stage.0.join(decisions::ARCHIVE_FILENAME);
    let plan = decisions::plan(&PlanInput {
        project,
        composition,
        participating: &names,
        members: &members,
        groups: &groups,
        combined: &combined,
        output: &output.display().to_string(),
    })
    .map_err(refused)?;
    let (receipt, engine) = bundle(identity.lane(), &plan, &stage.0)?;

    // Seal: bytes, receipts and the queue row in one acknowledged step.
    if engine
        .publication_state()
        .map_err(|error| store_unreadable(error.to_string()))?
        != PublicationState::Pending
    {
        return Err(Failure::conflict(
            super::export::PUBLISH_LOCAL_ONLY.code,
            "the reporter build is local-only and cannot enter the Server publication queue",
        )
        .remedy(super::export::PUBLISH_LOCAL_ONLY.remedy));
    }
    let guard = || -> Result<(), String> {
        let now = ds_cli_auth::headless_identity_for_named_project(lane)
            .map_err(|failure| failure.message().to_owned())?;
        if now != identity {
            return Err("the signed-in account changed before the archive was sealed".into());
        }
        super::export::verify_publish_scope(lane, &fence, fence.uid(), project)
            .map_err(|failure| failure.message().to_owned())
    };
    let (cached, mut publication) = seal_archive(
        &queue,
        identity.uid(),
        project,
        &engine,
        &plan,
        &receipt,
        &output,
        &run,
        &guard,
    )?;

    let mut output = super::project_receipt(&inventory);
    publication["operation"] = json!(decisions::OPERATION);
    publication["engine"] = json!("network_reporter");
    publication["sha256"] = json!(receipt.sha256);
    publication["size_bytes"] = json!(receipt.bytes);
    publication["next"] = json!(format!("ds report outbox drain --project {project}"));
    let fields = json!({
        "scope": {
            "mode": if composition.transformers.is_empty() { "all_active" } else { "explicit" },
            "requested": composition.transformers,
        },
        "archive_layout": match request.file_level {
            Some(level) => archive_layout(level, composition.archive_layout.combine_per_group),
            None => composition.archive_layout.describe(),
        },
        "composition": composition.recorded(&names),
        "force": composition.force,
        "status": "success",
        "prefix": plan.prefix,
        "archives": publication["locators"].clone(),
        "cached": cached,
        "individual_artifact_transformer_count": members.len(),
        "missing_individual_artifact_count": 0,
        "missing_individual_artifacts": [],
        "missing_individual_artifact_causes": [],
        "errors": [],
        "registry_write_failed": false,
        "registry_write_error": null,
        "publication": publication,
        "engine": {"release": engine.engine_version, "build_manifest_sha256": engine.build_manifest_sha256},
        "inputs": {
            "input_base_fingerprint": plan.input_fingerprint,
            "transformers": members.iter().map(|member| json!({
                "name": member.transformer, "revision": member.revision, "batch_id": member.batch_id,
                "files": member.files.len(),
            })).collect::<Vec<_>>(),
            "combined_sets": combined.len(),
            "grouping": groups.identity.clone().unwrap_or_else(|| json!({"applied": false})),
            "entries": plan.entry_count,
        },
    });
    output
        .as_object_mut()
        .expect("receipt is an object")
        .extend(fields.as_object().expect("fields are an object").clone());
    Ok(output)
}

/// Seal the composed archive into the publication queue as
/// `export-compounded_report`: bytes, receipts and the queue row in one
/// acknowledged step. Sync publishes it through the compute-artifact door.
#[allow(clippy::too_many_arguments)]
pub(super) fn seal_archive(
    queue: &PublicationQueue,
    owner: &str,
    project: &str,
    engine: &ds_command_kernel::report_export::EngineIdentity,
    plan: &decisions::BundlePlan,
    receipt: &decisions::BundleReceipt,
    output: &Path,
    run: &str,
    guard: &dyn Fn() -> Result<(), String>,
) -> Result<(bool, Value), Failure> {
    let held_file =
        std::fs::File::open(output).map_err(|error| store_unreadable(error.to_string()))?;
    let artifacts = [VerifiedSidecarArtifact {
        format: decisions::ARCHIVE_FORMAT.to_owned(),
        filename: decisions::ARCHIVE_FILENAME.to_owned(),
        size_bytes: receipt.bytes,
        sha256: receipt.sha256.clone(),
        paper_size: None,
        presentation: None,
        held_file,
    }];
    // The publication receipt grammar names every report run `report-…`.
    let client_run_id = format!("report-{}-{run}", plan.prefix);
    let sealed = ds_sync_runtime::reports::seal(
        &queue.store,
        &queue.fence,
        &queue.root,
        &SealRequest {
            owner_uid: owner,
            project_id: project,
            engine_version: &engine.engine_version,
            engine_build_manifest_sha256: &engine.build_manifest_sha256,
            transformer: decisions::IDENTITY,
            transformer_revision: ds_command_kernel::report_export::combined::PUBLICATION_REVISION,
            input_base_fingerprint: &plan.input_fingerprint,
            room_content_sha256: &receipt.sha256,
            client_run_id: &client_run_id,
            artifacts: &artifacts,
            expected_formats: &[decisions::ARCHIVE_FORMAT],
            deadline: Instant::now() + Duration::from_secs(30),
            guard,
        },
    )
    .map_err(|error| {
        Failure::failed(super::export::PUBLISH_ROOT.code, error)
            .remedy(super::export::PUBLISH_ROOT.remedy)
    })?;
    Ok(match sealed {
        SealOutcome::Sealed(sealed) => (
            false,
            json!({
                "queued": true,
                "already_recorded": false,
                "batch_id": sealed.receipt.batch_id,
                "client_publish_id": sealed.receipt.client_publish_id,
                "locators": sealed.artifacts.iter().map(|artifact| artifact.locator.clone()).collect::<Vec<_>>(),
            }),
        ),
        SealOutcome::AlreadyRecorded { replay_key, state } => (
            true,
            json!({
                "queued": true,
                "already_recorded": true,
                "client_publish_id": replay_key,
                "state": state.as_str(),
                "locators": [],
            }),
        ),
    })
}

/// Turn the per-room causes into the kernel's readiness question.
///
/// A room this machine holds no report for, and a room whose held report is
/// older than its saved revision, both read as "no current report here"; the
/// predicate's remedy for them is `ds report project export`.
pub(super) fn readiness_rooms(causes: &[Value]) -> Vec<RoomInput> {
    causes
        .iter()
        .map(|cause| {
            let name = cause["transformer"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            match cause["code"].as_str().unwrap_or_default() {
                "report_stale" => RoomInput {
                    name,
                    report_state: Some("stale".into()),
                    ..Default::default()
                },
                _ => RoomInput {
                    name,
                    report_state: Some("none".into()),
                    ..Default::default()
                },
            }
        })
        .collect()
}

/// The refusal a set of unready rooms produces, before anything is composed.
pub(super) fn readiness_refusal(causes: &[Value]) -> Result<Option<Failure>, Failure> {
    if causes.is_empty() {
        return Ok(None);
    }
    let rooms = readiness_rooms(causes);
    let verdict = combined_readiness::evaluate_value(&rooms, combined_readiness::DEFAULT_SHOWN)
        .map_err(|error| {
            Failure::unavailable(
                super::COMBINED_READINESS_UNAVAILABLE.code,
                format!("The Compounded Report readiness predicate could not answer: {error}"),
            )
            .remedy(super::COMBINED_READINESS_UNAVAILABLE.remedy)
        })?;
    let Some(refusal) = verdict.get("refusal").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let command = refusal["command"]
        .as_str()
        .unwrap_or("ds report project scope");
    let named: Vec<&str> = refusal["rooms"]["names"]
        .as_array()
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let more = refusal["rooms"]["more"].as_u64().unwrap_or(0);
    let mut message = format!(
        "The Compounded Report requires current reports for {} room(s): {}",
        refusal["rooms"]["total"]
            .as_u64()
            .unwrap_or(named.len() as u64),
        named.join(", "),
    );
    if more > 0 {
        message.push_str(&format!(" and {more} more"));
    }
    message.push_str(". No archive was composed.");
    // The kernel decides WHICH refusal applies; each one is constructed here
    // from its own declared `Refusal`. A token this build does not know is
    // the conservative one, "these rooms are not current".
    let failure = match refusal["token"].as_str().unwrap_or_default() {
        "combined_inputs_publication_pending" => {
            Failure::conflict(super::COMBINED_INPUTS_PUBLICATION_PENDING.code, message)
        }
        "combined_inputs_empty" => Failure::conflict(super::COMBINED_INPUTS_EMPTY.code, message),
        "combined_no_inputs" => Failure::conflict(super::COMBINED_NO_INPUTS.code, message),
        _ => Failure::conflict(super::COMBINED_INPUTS_NOT_CURRENT.code, message),
    };
    Ok(Some(
        failure
            .remedy(format!("run `{command}`, then run this command again"))
            .next(command.to_string())
            .next("ds report project scope")
            .detail(json!({"readiness": verdict, "causes": causes})),
    ))
}

/// The requested layout, in the report layer's own words.
///
/// `combine_per_group` is the current name for the choice the registry has
/// always recorded as `combine_per_district`; both are reported so a caller
/// reading a fresh receipt and one reading an old registry row see the same
/// archive described the same way.
pub(super) fn archive_layout(file_level: &str, combine_per_group: bool) -> Value {
    // The vocabulary is the shared fold's; the RECORDED members are this
    // receipt's own and win, so describing an archive can never restate what
    // was asked for.
    let mut layout = super::archive_layout_vocabulary(Some(file_level), None, combine_per_group);
    let recorded = json!({
        "file_level": file_level,
        "combine_per_group": combine_per_group,
    });
    layout
        .as_object_mut()
        .expect("layout is an object")
        .extend(recorded.as_object().expect("recorded is an object").clone());
    layout
}

pub fn render(data: &Value) -> String {
    if data["groups"].is_array() {
        return render_grouped(data);
    }
    let mut out = String::new();
    out.push_str(&format!(
        "project {} · {} · {} · Compounded Report {} · {} individual artifact(s), {} missing\n",
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["status"].as_str().unwrap_or("?"),
        data["prefix"].as_str().unwrap_or("?"),
        data["individual_artifact_transformer_count"]
            .as_u64()
            .unwrap_or(0),
        data["missing_individual_artifact_count"]
            .as_u64()
            .unwrap_or(0),
    ));
    if let Some(archives) = data["archives"].as_array() {
        for archive in archives {
            out.push_str(&format!("  {}\n", archive.as_str().unwrap_or("?")));
        }
    }
    if let Some(next) = data["publication"]["next"].as_str() {
        out.push_str(&format!("  queued for Sync; publish now with `{next}`\n"));
    }
    out
}

/// One line per group archive, in the order they were published.
fn render_grouped(data: &Value) -> String {
    let groups = data["groups"].as_array().cloned().unwrap_or_default();
    let mut out = format!(
        "project {} · {} · {} Compounded Report archive(s) of {} group(s) over {} transformer(s)\n",
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["published_count"].as_u64().unwrap_or(0),
        groups.len(),
        data["grouping"]["transformer_count"].as_u64().unwrap_or(0),
    );
    for group in &groups {
        out.push_str(&format!(
            "  {:<40} {:>4} · {} · {}\n",
            super::scope::path_line(&group["path"]),
            group["transformer_count"].as_u64().unwrap_or(0),
            group["status"].as_str().unwrap_or("?"),
            group["prefix"].as_str().unwrap_or("-"),
        ));
    }
    out
}

#[cfg(test)]
#[path = "combined_tests.rs"]
mod tests;
