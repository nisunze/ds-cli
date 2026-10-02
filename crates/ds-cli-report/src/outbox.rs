//! `ds report outbox` — the publication queue, in the open.
//!
//! A report that has been produced is not finished: its bytes are sealed
//! into this machine's artifact root and its row into the sync store, and it
//! is published from there. That queue used to be invisible, and then it was
//! read from the wrong place: the directory of sealed batches, which counted
//! every batch ever committed — published, lost or waiting — as waiting, and
//! took liveness from a file lock. The sync store is the queue (owner,
//! 2026-09-20): `held` is queued, `published` is this machine's copy of the
//! head, `conflict`/`refused` is lost, and a live lease on a project is its
//! pump.
//!
//! These commands are that queue's surface:
//!
//! * `status` — the store's own reading: how much is queued, how old the
//!   oldest is, why queued rows are still here, what stays and what the
//!   next pass frees, who is pumping, and what to run next. Read-only over
//!   the lane's store with no session, no gateway and no running Server —
//!   exactly the machines where a stopped queue goes unnoticed.
//! * `drain` — one pass now, through the SAME shared publication runner the
//!   background pump uses. There is deliberately no second pump: one
//!   delivery singleton, woken by hand. A pass never says nothing.
//! * `inventory` — the captured native owner's actual held report files,
//!   through the shared receipt/catalogue/SHA reader, without touching the
//!   queue or contacting a provider.
//!
//! Neither command decides anything. The queue's reading is
//! `ds_command_kernel::sync_store::queue_status`, and draining is
//! `ds_cli_server::server_reports::drain` — the runner the Server already
//! owns. This module resolves the store, names refusals and renders.

use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::sync_store::{Fence, QueueStatus};
use serde_json::{Value, json};

use crate::project::LANE_ARG;

/// The same flag `ds report project export` uses to name the queue a
/// publication enters. A report is sealed into the Server's state root, so
/// reading and draining that queue must be able to name the same root.
const SERVER_STATE_DIR_ARG: Arg = Arg::value(
    "server-state-dir",
    "<absolute-path>",
    "Matching `ds server serve --state-dir` directory when the Server uses a custom state root.",
);

const PROJECT_ARG: Arg = Arg::value(
    "project",
    "<exact-id>",
    "Narrow to one exact ds_project id; omit for every project queued on this machine.",
);

/// On a drain, naming a project is "Sync now" for it: the pass also runs on
/// a machine that has published nothing for that project, so a second
/// machine pulls what the first one published.
const DRAIN_PROJECT_ARG: Arg = Arg::value(
    "project",
    "<exact-id>",
    "Sync this ds_project id now, pulling what others published; omit for every queued project.",
);

const QUEUE_UNREADABLE: Refusal = Refusal {
    code: "report_outbox_unreadable",
    when: "the lane's sync store or artifact root cannot be read on this machine",
    remedy: "check the Server state directory's permissions, or pass the same --server-state-dir as ds server serve",
};

const QUEUE_ROOT_INVALID: Refusal = Refusal {
    code: "report_outbox_root_invalid",
    when: "the Server state root cannot be resolved from the lane and --server-state-dir",
    remedy: "omit --server-state-dir, or pass the same absolute path `ds server serve` uses",
};

const SERVER_UNREACHABLE: Refusal = Refusal {
    code: "report_outbox_server_unavailable",
    when: "the protected Server state holds no connection this machine can publish under",
    remedy: "run ds server serve on this machine, then retry the drain",
};

const DRAIN_FAILED: Refusal = Refusal {
    code: "report_outbox_drain_failed",
    when: "the shared publication runner could not complete a pass",
    remedy: "read `ds report outbox status`; a held row names its reason, a lease its worker",
};

/// How many receipts of one pass a drain answer carries per project.
const MAX_REPORTED_RECEIPTS: usize = 40;

const READ_REFUSALS: &[Refusal] = &[QUEUE_ROOT_INVALID, QUEUE_UNREADABLE];

const DRAIN_REFUSALS: &[Refusal] = &[
    QUEUE_ROOT_INVALID,
    QUEUE_UNREADABLE,
    SERVER_UNREACHABLE,
    DRAIN_FAILED,
    crate::project::NATIVE_PROFILE,
    crate::project::HEADLESS_SIGNED_OUT,
];

pub static STATUS: Command = Command {
    id: "report.outbox.status",
    path: &["report", "outbox", "status"],
    contract: 1,
    summary: "Show the report publication queue: what waits, why, who pumps it.",
    purpose: "\
Reads this machine's report publication queue from the lane's sync store: \
what is queued, for how long and with what hold reason, what is this \
machine's published copy, what lost and will be freed by the next pass, and \
whether a live lease is pumping each project. Needs no credential, no \
project selection and no running Server.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, SERVER_STATE_DIR_ARG, LANE_ARG],
    output: "\
`queued_batches`, `queued_bytes`, `oldest_age_ms`, `held_batches`, \
`reclaimable_batches`; `projects[]` (queued, bytes, oldest age, rooms, \
`reasons`, `pumped`, `blocked`); `leases[]`; `bytes_lock`; `blocked` and `stuck` — whether anything \
needs a human — and `next`, the one command to run.",
    examples: &[
        Example {
            command: "ds report outbox status --output json",
            note: "`.data.stuck` answers \"is anything stuck?\"; `.data.next` says what to run.",
            runnable: true,
        },
        Example {
            command: "ds report outbox status --project <exact-id>",
            note: "The same reading, narrowed to one project.",
            runnable: false,
        },
    ],
    refusals: READ_REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &[
        "pending",
        "stuck",
        "unpublished",
        "backlog",
        "lease",
        "sealed",
    ],
    requires: Requires::Server,
    availability: || ds_cli_contract::spec::Availability::Available,
};

pub static DRAIN: Command = Command {
    id: "report.outbox.drain",
    path: &["report", "outbox", "drain"],
    contract: 1,
    summary: "Publish the queued reports now (needs --yes).",
    purpose: "\
One publication pass over this machine's queued reports, through the same \
runner the Server's pump uses — never a second pump or queue. Safe to run \
twice: a publication already in the shared record is recognised by its \
client publish id. A row that lost has its bytes freed by the pass and says \
so. An offline pass changes nothing and says so. A dead holder's lease is freed.",
    chapter: Chapter::Reports,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[DRAIN_PROJECT_ARG, SERVER_STATE_DIR_ARG, LANE_ARG],
    output: "\
`before` and `after` queue readings, `drained`, and `projects[]`: per project \
`offline`, `retry_eligible`, `wake_at_ms`, `summary` (after the pass), \
`reclaimed` (batches, bytes), `lease` (freed or blocking), `idle` (why nothing moved) and `receipts`.",
    examples: &[Example {
        command: "ds report outbox drain --yes --output json",
        note: "`.data.drained` says what moved; `.data.projects[].receipts` what each row did.",
        runnable: false,
    }],
    refusals: DRAIN_REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &[
        "flush", "push", "retry", "unstick", "sync now", "send", "pull", "download",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

const INVENTORY_IDENTITY: Refusal = Refusal {
    code: "report_inventory_identity_unavailable",
    when: "the lane has no existing protected account/install, or its providers or protection disagree",
    remedy: "check this Server user's existing native account, install identity and owner-only config in the requested lane; this read never initializes them",
};
const INVENTORY_SCOPE: Refusal = Refusal {
    code: "report_inventory_scope_invalid",
    when: "project is not an exact path-segment id or transformer is not an individual reportable key",
    remedy: "name one exact project and individual transformer, without padding or path separators",
};
const INVENTORY_UNREADABLE: Refusal = Refusal {
    code: "report_inventory_unreadable",
    when: "the native store/catalogue cannot be read or a selected file has missing, corrupt or mismatched committed bytes",
    remedy: "check the matching Server state directory and preserve the failing committed files; retry the read after their owning workflow resolves the failure",
};
const INVENTORY_CHANGED: Refusal = Refusal {
    code: "report_inventory_identity_changed",
    when: "the protected account, lane deployment or install changed during the local inventory read",
    remedy: "retry under the same native Server identity after the concurrent account change finishes",
};

pub static INVENTORY: Command = Command {
    id: "report.outbox.inventory",
    path: &["report", "outbox", "inventory"],
    contract: 1,
    summary: "Inspect one transformer's verified held Server report files.",
    purpose: "Read actual committed local artifacts under the existing native account/install and explicit project. The shared Rust reader checks receipts and physical SHA/size; the projector preserves each output's producing run and publication state. Works without Desktop or a running Server. No provider refresh, cloud read, publication, queue touch or missing-state creation. Cloud publication remains unobserved; historical generation times remain null.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        crate::project::PROJECT_ARG,
        Arg::value(
            "transformer",
            "<exact-key>",
            "One individual reportable transformer key.",
        )
        .required(),
        SERVER_STATE_DIR_ARG,
        LANE_ARG,
    ],
    output: "`ds.report.inventory/v1`: owner/project/transformer/variant; active publication and ambiguity; nullable generated time; status/read facts; outputs with filename, producing batch/publication/run, local locator/SHA/size/format, publication phase and source state; local/published counts, missing outputs and complete observation. No file bytes or credentials. Empty local inventory is valid; absent store is not_observed; cloud_read is not_observed.",
    examples: &[Example {
        command: "ds report outbox inventory --project <exact-id> --transformer <exact-key> --output json",
        note: "Inspect held files without publishing; selected missing/corrupt bytes refuse instead of falling back to an old generation.",
        runnable: false,
    }],
    refusals: &[
        QUEUE_ROOT_INVALID,
        INVENTORY_IDENTITY,
        INVENTORY_SCOPE,
        INVENTORY_UNREADABLE,
        INVENTORY_CHANGED,
    ],
    reference: Some("docs/reference/report.md"),
    search: &[
        "local artifacts",
        "held files",
        "pdf",
        "sha256",
        "generation",
        "refresh",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn inventory_principal(lane: &str) -> Result<ds_cli_auth::HeadlessPrincipal, Failure> {
    ds_cli_auth::headless_principal_read_only(lane).map_err(|error| {
        Failure::unavailable(INVENTORY_IDENTITY.code, error.message())
            .remedy(INVENTORY_IDENTITY.remedy)
            .detail(json!({"native_code": error.code()}))
    })
}

pub fn inventory(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let state = server_state(inputs)?;
    let project = inputs.require("project")?;
    let transformer = inputs.require("transformer")?;
    if !ds_command_kernel::execution_context::valid_project(project) {
        return Err(Failure::invalid(INVENTORY_SCOPE.code, INVENTORY_SCOPE.when)
            .remedy(INVENTORY_SCOPE.remedy));
    }
    // Validate transformer/report scope through the same pure owner before
    // observing protected identity or reading any native bytes.
    use ds_command_kernel::report::inventory::{ReadFact, ReadState, Snapshot};
    let empty = Snapshot {
        owner: "scope-validation".into(),
        project: project.into(),
        transformer: transformer.into(),
        variant: "default".into(),
        expected_input_base_fingerprint: None,
        local_read: ReadFact {
            state: ReadState::NotObserved,
            error: None,
        },
        sync_read: ReadFact {
            state: ReadState::NotObserved,
            error: None,
        },
        cloud_read: ReadFact {
            state: ReadState::NotObserved,
            error: None,
        },
        local_sets: vec![],
        sync_artifacts: vec![],
        cloud_head: None,
        legacy_cloud: None,
        required_outputs: vec![],
    };
    ds_command_kernel::report::inventory::project(&empty).map_err(|error| {
        Failure::invalid(INVENTORY_SCOPE.code, error).remedy(INVENTORY_SCOPE.remedy)
    })?;
    let lane = inputs.require("lane")?;
    let principal = inventory_principal(lane)?;
    let fence = Fence {
        account: principal.account_uid().into(),
        deployment: principal.deployment().into(),
        install_id: principal.install_id().into(),
    };
    let store =
        ds_sync_store::Store::open_read_only(&state.join("store.sqlite")).map_err(|error| {
            Failure::failed(INVENTORY_UNREADABLE.code, error.to_string())
                .remedy(INVENTORY_UNREADABLE.remedy)
        })?;
    let rows = store
        .as_ref()
        .map(|store| {
            store.snapshot(
                &fence,
                &ds_command_kernel::sync_store::Scope::Project {
                    project: project.into(),
                },
            )
        })
        .transpose()
        .map_err(|error| {
            Failure::failed(INVENTORY_UNREADABLE.code, error.to_string())
                .remedy(INVENTORY_UNREADABLE.remedy)
        })?
        .map(|snapshot| snapshot.artifacts)
        .unwrap_or_default();
    let mut snapshot = ds_report_artifacts::inventory::read_snapshot(
        &state.join("report-artifacts"),
        principal.account_uid(),
        project,
        transformer,
        &rows,
    )
    .map_err(|error| {
        Failure::failed(INVENTORY_UNREADABLE.code, error).remedy(INVENTORY_UNREADABLE.remedy)
    })?;
    if store.is_none() {
        snapshot.sync_read.state = ds_command_kernel::report::inventory::ReadState::NotObserved;
    }
    let after = inventory_principal(lane)?;
    if principal.account_uid() != after.account_uid()
        || principal.deployment() != after.deployment()
        || principal.install_id() != after.install_id()
    {
        return Err(
            Failure::conflict(INVENTORY_CHANGED.code, INVENTORY_CHANGED.when)
                .remedy(INVENTORY_CHANGED.remedy),
        );
    }
    let view = ds_command_kernel::report::inventory::project(&snapshot).map_err(|error| {
        Failure::failed(INVENTORY_UNREADABLE.code, error).remedy(INVENTORY_UNREADABLE.remedy)
    })?;
    serde_json::to_value(view).map_err(|error| {
        Failure::failed(INVENTORY_UNREADABLE.code, error.to_string())
            .remedy(INVENTORY_UNREADABLE.remedy)
    })
}

pub fn render_inventory(data: &Value) -> String {
    let mut text = format!(
        "{}/{}: {} verified local files; cloud read {}\n",
        data["project"].as_str().unwrap_or(""),
        data["transformer"].as_str().unwrap_or(""),
        data["local_output_count"].as_u64().unwrap_or(0),
        data["cloud_read"]["state"]
            .as_str()
            .unwrap_or("not_observed"),
    );
    for output in data["outputs"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "  {} · {} · {} · source {}\n",
            output["output_id"].as_str().unwrap_or(""),
            output["filename"].as_str().unwrap_or(""),
            output["publication_phase"]
                .as_str()
                .unwrap_or("not_observed"),
            output["source_state"].as_str().unwrap_or("not_observed"),
        ));
    }
    text
}

fn server_state(inputs: &Inputs) -> Result<PathBuf, Failure> {
    ds_compute_runtime::server_state_directory(
        inputs.require("lane")?,
        inputs.value("server-state-dir").map(Path::new),
    )
    .map_err(|error| {
        Failure::invalid(QUEUE_ROOT_INVALID.code, error).remedy(QUEUE_ROOT_INVALID.remedy)
    })
}

fn unreadable(error: String) -> Failure {
    Failure::failed(QUEUE_UNREADABLE.code, error).remedy(QUEUE_UNREADABLE.remedy)
}

/// The store's fence on this machine: the native identity's account, the
/// lane's deployment and the registered install — read from protected
/// local state, no refresh and no network. This is how the Server derives
/// its own fence, so the two never read different queues.
pub fn fence(lane: &str) -> Result<Fence, Failure> {
    let principal = ds_cli_auth::headless_principal(lane)?;
    Ok(Fence {
        account: principal.account_uid().to_owned(),
        deployment: principal.deployment().to_owned(),
        install_id: principal.install_id().to_owned(),
    })
}

/// The queue as the lane's store holds it, read without a session or a
/// credential and without leaving state behind. The whole file is read —
/// every fence it holds — because one Server state root is one owner's and
/// a status must answer on a machine with no native identity to name a
/// fence. A machine with no store holds an empty queue.
fn queue(state: &Path, project: Option<&str>) -> Result<QueueStatus, Failure> {
    let now = ds_sync_runtime::now_ms();
    match ds_sync_store::Store::open_read_only(&state.join("store.sqlite"))
        .map_err(|e| unreadable(e.to_string()))?
    {
        Some(store) => store
            .queue_all(project, now)
            .map_err(|e| unreadable(e.to_string())),
        None => Ok(ds_command_kernel::sync_store::queue_status(
            &[],
            &[],
            &[],
            project,
            now,
        )),
    }
}

/// One queue reading, in this CLI's own vocabulary.
fn reading(status: &QueueStatus) -> Value {
    json!({
        "queued_batches": status.queued_batches,
        "queued_bytes": status.queued_bytes,
        "oldest_age_ms": status.oldest_age_ms,
        "held_batches": status.held_batches,
        "held_bytes": status.held_bytes,
        "reclaimable_batches": status.reclaimable_batches,
        "reclaimable_bytes": status.reclaimable_bytes,
        "projects": status
            .projects
            .iter()
            .map(|project| json!({
                "project": project.project_id,
                "queued_batches": project.batches,
                "queued_bytes": project.bytes,
                "oldest_produced_at_ms": project.oldest_produced_at_ms,
                "oldest_age_ms": project.oldest_age_ms,
                "transformers": project.transformers,
                "reasons": project.reasons,
                "held_batches": project.held_batches,
                "reclaimable_batches": project.reclaimable_batches,
                "pumped": project.pumped,
                "blocked": project.blocked,
            }))
            .collect::<Vec<_>>(),
        "leases": status
            .leases
            .iter()
            .map(|lease| json!({
                "project": lease.scope.project(),
                "worker": lease.worker_id,
                "expires_at_ms": lease.expires_at_ms,
            }))
            .collect::<Vec<_>>(),
    })
}

/// The artifact directory's own writer lock, as a fact about the bytes:
/// a seal or a discard waits on it. It is not the queue's liveness.
fn bytes_lock(state: &Path) -> Result<Value, Failure> {
    let lock = ds_report_artifacts::inspect_pending_upload_lock(&state.join("report-artifacts"))
        .map_err(unreadable)?;
    Ok(json!({
        "present": lock.present,
        "held": lock.held,
        "holder": lock.holder,
        "holder_reason": lock.holder_reason,
        "heartbeat_age_ms": lock.heartbeat_age_ms,
        "reclaimable": lock.reclaimable,
        "wedged": lock.wedged,
        "owner": lock.owner.as_ref().map(|owner| json!({
            "pid": owner.pid,
            "worker": owner.worker,
            "taken_at_ms": owner.taken_at_ms,
            "heartbeat_at_ms": owner.heartbeat_at_ms,
        })),
    }))
}

fn project(inputs: &Inputs) -> Option<String> {
    inputs.value("project").map(str::to_string)
}

pub fn status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let state = server_state(inputs)?;
    let status = queue(&state, project(inputs).as_deref())?;
    let mut value = reading(&status);
    let fields = json!({
        "bytes_lock": bytes_lock(&state)?,
        "stuck": status.stuck,
        "blocked": status.blocked,
        "next": status.next,
    });
    value
        .as_object_mut()
        .expect("the reading is an object")
        .extend(fields.as_object().expect("fields are an object").clone());
    Ok(value)
}

fn receipt_value(receipt: &ds_sync_runtime::Receipt) -> Value {
    let mut value = json!({
        "action": receipt.action,
        "outcome": receipt.outcome,
    });
    if let Some(identity) = &receipt.identity {
        value["operation"] = json!(identity.operation);
        value["engine"] = json!(identity.engine);
    }
    if let Some(detail) = &receipt.detail {
        value["detail"] = json!(detail.chars().take(240).collect::<String>());
    }
    if let Some(bytes) = receipt.bytes {
        value["bytes"] = json!(bytes);
    }
    if let Some(revision) = receipt.head_revision {
        value["head_revision"] = json!(revision);
    }
    value
}

pub fn drain(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let state = server_state(inputs)?;
    let lane = inputs.require("lane")?;
    let fence = fence(lane)?;
    let wanted = project(inputs);
    let before = queue(&state, wanted.as_deref())?;

    let connection = ds_cli_server::host::load_connection(&state).map_err(|error| {
        Failure::unavailable(SERVER_UNREACHABLE.code, error).remedy(SERVER_UNREACHABLE.remedy)
    })?;
    let database = state.join("store.sqlite");
    // The Sync Center reader the Server itself uses, anchored under the same
    // protected state directory. A drain must not invent a second download
    // cache beside the pump's.
    let reads = ds_sync_runtime::VerifiedReads::new(state.join("sync-downloads"));

    let mut passes = Vec::new();
    // The store's projects under this fence, plus any project whose batches
    // are on disk with no row yet, so the pass adopts them.
    let mut projects = ds_cli_server::server_reports::projects_with_publications(&database, &fence)
        .map_err(unreadable)?;
    // Naming a project is "Sync now" for it: the pass runs even with nothing
    // queued here, reads the record and pulls what this machine lacks.
    if let Some(wanted) = &wanted {
        projects.insert(wanted.clone());
    }
    for project_id in projects {
        if wanted.as_ref().is_some_and(|wanted| *wanted != project_id) {
            continue;
        }
        let pass = ds_cli_server::server_sync::ServerSyncSession::open(
            &database,
            &connection,
            &project_id,
        )
        .and_then(|session| {
            // A lease left by a process that is provably gone is freed first,
            // and a live one is named: a drain that moves nothing never
            // answers without saying who holds the project (b06fad17).
            let lease = session.reclaim_abandoned_lease()?;
            // `Manual` is the kernel's own name for "the operator pressed
            // Sync now". A hand-driven drain is exactly that, and it must not
            // borrow a scheduler trigger that changes retry policy.
            // Over the host's one producer set — report + Solar — so a lost
            // row of either engine this drain observes is freed by the
            // producer that owns its bytes.
            let pass = ds_cli_server::solar_sync::with_producers(
                &database,
                session.identity(),
                &project_id,
                None,
                |producers| {
                    ds_cli_server::server_reports::drain(
                        &session,
                        producers,
                        &reads,
                        ds_sync_runtime::Trigger::Manual,
                    )
                },
            )?;
            let receipts: Vec<Value> = pass
                .receipts
                .iter()
                .take(MAX_REPORTED_RECEIPTS)
                .map(receipt_value)
                .collect();
            Ok(json!({
                "project": project_id,
                "offline": pass.offline,
                "retry_eligible": pass.retry_eligible,
                "wake_at_ms": pass.wake_at_ms,
                "summary": {
                    "uploads": pass.summary.uploads,
                    "downloads": pass.summary.downloads,
                    "conflicts": pass.summary.conflicts,
                    "refused": pass.summary.refused,
                    "in_sync": pass.summary.in_sync,
                },
                "reclaimed": {
                    "batches": pass.reclaimed.batches,
                    "bytes": pass.reclaimed.bytes,
                },
                "lease": lease_value(&lease),
                "idle": pass.idle,
                "receipts": receipts,
                "more": pass.receipts.len().saturating_sub(MAX_REPORTED_RECEIPTS),
            }))
        })
        .map_err(|error| {
            Failure::failed(DRAIN_FAILED.code, format!("{project_id}: {error}"))
                .remedy(DRAIN_FAILED.remedy)
        })?;
        passes.push(pass);
    }

    let after = queue(&state, wanted.as_deref())?;
    Ok(json!({
        "before": reading(&before),
        "after": reading(&after),
        "drained": {
            "batches": before.queued_batches.saturating_sub(after.queued_batches),
            "bytes": before.queued_bytes.saturating_sub(after.queued_bytes),
        },
        "projects": passes,
        "bytes_lock": bytes_lock(&state)?,
        "stuck": after.stuck,
        "blocked": after.blocked,
        "next": after.next,
    }))
}

/// A project's sync lease as the drain found it: freed from a dead holder,
/// or still held by a named worker until a named time.
fn lease_value(lease: &ds_cli_server::server_sync::LeaseReading) -> Value {
    json!({
        "reclaimed_from": lease.reclaimed_from,
        "blocked_by": lease.blocked_by.as_ref().map(|held| json!({
            "worker": held.worker_id,
            "expires_at_ms": held.expires_at_ms,
            "holder_running": held.holder_running,
        })),
    })
}

pub fn render(data: &Value) -> String {
    let reading = if data["after"].is_object() {
        &data["after"]
    } else {
        data
    };
    let mut out = format!(
        "{} batch(es) queued · {} · oldest {} · {} held · {} reclaimable\n",
        reading["queued_batches"].as_u64().unwrap_or(0),
        bytes(reading["queued_bytes"].as_u64().unwrap_or(0)),
        age(reading["oldest_age_ms"].as_u64().unwrap_or(0)),
        reading["held_batches"].as_u64().unwrap_or(0),
        reading["reclaimable_batches"].as_u64().unwrap_or(0),
    );
    for project in reading["projects"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<28} {} batch(es) · {} · oldest {}{}{}\n",
            project["project"].as_str().unwrap_or("?"),
            project["queued_batches"].as_u64().unwrap_or(0),
            bytes(project["queued_bytes"].as_u64().unwrap_or(0)),
            age(project["oldest_age_ms"].as_u64().unwrap_or(0)),
            if project["pumped"].as_bool().unwrap_or(false) {
                " · pumped"
            } else {
                ""
            },
            project["reasons"]
                .as_array()
                .filter(|reasons| !reasons.is_empty())
                .map(|reasons| {
                    format!(
                        " · held: {}",
                        reasons
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("; ")
                    )
                })
                .unwrap_or_default(),
        ));
        if let Some(cause) = project["blocked"]["cause"].as_str() {
            out.push_str(&format!(
                "    blocked: {cause} · {}\n",
                project["blocked"]["detail"]
                    .as_str()
                    .unwrap_or("credential refused"),
            ));
        }
    }
    if data["after"].is_object() {
        out.push_str(&format!(
            "  drained {} batch(es) · {}\n",
            data["drained"]["batches"].as_u64().unwrap_or(0),
            bytes(data["drained"]["bytes"].as_u64().unwrap_or(0)),
        ));
        for pass in data["projects"].as_array().into_iter().flatten() {
            let project = pass["project"].as_str().unwrap_or("?");
            if let Some(worker) = pass["lease"]["reclaimed_from"].as_str() {
                out.push_str(&format!(
                    "  {project}: freed the lease of {worker}, whose process is gone\n"
                ));
            }
            let held = &pass["lease"]["blocked_by"];
            if let Some(worker) = held["worker"].as_str() {
                let state = match held["holder_running"].as_bool() {
                    Some(true) => "running",
                    Some(false) => "not running",
                    None => "not observable here",
                };
                out.push_str(&format!(
                    "  {project}: blocked by the lease of {worker} ({state}) until {} ms since epoch; nothing of this project was drained\n",
                    held["expires_at_ms"].as_u64().unwrap_or(0)
                ));
            }
        }
        for project in data["projects"].as_array().into_iter().flatten() {
            let summary = &project["summary"];
            out.push_str(&format!(
                "  {:<28} {}{} · {} in sync, {} to upload, {} in conflict · reclaimed {} batch(es)\n",
                project["project"].as_str().unwrap_or("?"),
                if project["offline"].as_bool().unwrap_or(false) {
                    "offline; the queue keeps its work and retries"
                } else {
                    project["idle"].as_str().unwrap_or("published")
                },
                if project["retry_eligible"].as_bool().unwrap_or(false) {
                    " · retry pending"
                } else {
                    ""
                },
                summary["in_sync"].as_u64().unwrap_or(0),
                summary["uploads"].as_u64().unwrap_or(0),
                summary["conflicts"].as_u64().unwrap_or(0),
                project["reclaimed"]["batches"].as_u64().unwrap_or(0),
            ));
            for receipt in project["receipts"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "    {}:{}{}\n",
                    receipt["action"].as_str().unwrap_or("?"),
                    receipt["outcome"].as_str().unwrap_or("?"),
                    receipt["operation"]
                        .as_str()
                        .map(|operation| format!(" {operation}"))
                        .unwrap_or_default(),
                ));
            }
        }
    }
    let lock = &data["bytes_lock"];
    if lock["held"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "  bytes lock: held · holder {}{}\n",
            lock["holder"].as_str().unwrap_or("none"),
            match lock["owner"]["pid"].as_u64() {
                Some(pid) => format!(" (pid {pid})"),
                None => String::new(),
            },
        ));
    }
    out.push_str(&format!(
        "  stuck: {} · next: {}\n",
        data["stuck"].as_bool().unwrap_or(false),
        data["next"].as_str().unwrap_or("-"),
    ));
    out
}

fn bytes(count: u64) -> String {
    match count {
        0 => "0 B".to_string(),
        count if count < 1024 * 1024 => format!("{} KB", count.div_ceil(1024)),
        count => format!("{} MB", count.div_ceil(1024 * 1024)),
    }
}

/// An age an operator reads at a glance. A queue's age is the whole point:
/// "weeks" is the number that told the owner his machine had become a private
/// store instead of a buffer.
fn age(elapsed_ms: u64) -> String {
    let seconds = elapsed_ms / 1_000;
    match seconds {
        0 => "-".to_string(),
        seconds if seconds < 90 => format!("{seconds}s"),
        seconds if seconds < 5_400 => format!("{}m", seconds / 60),
        seconds if seconds < 172_800 => format!("{}h", seconds / 3_600),
        seconds => format!("{}d", seconds / 86_400),
    }
}
