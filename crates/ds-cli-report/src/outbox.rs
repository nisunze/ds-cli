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
//! These two commands are that queue's surface:
//!
//! * `status` — the store's own reading: how much is queued, how old the
//!   oldest is, why queued rows are still here, what stays and what the
//!   next pass frees, who is pumping, and what to run next. Read-only over
//!   the lane's store with no session, no gateway and no running Server —
//!   exactly the machines where a stopped queue goes unnoticed.
//! * `drain` — one pass now, through the SAME shared publication runner the
//!   background pump uses. There is deliberately no second pump: one
//!   delivery singleton, woken by hand. A pass never says nothing.
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
`reasons`, `pumped`); `leases[]`; `bytes_lock`; `stuck` — whether anything \
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
so. An offline pass changes nothing and says so.",
    chapter: Chapter::Reports,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, SERVER_STATE_DIR_ARG, LANE_ARG],
    output: "\
`before` and `after` queue readings, `drained`, and `projects[]`: per project \
`offline`, `retry_eligible`, `wake_at_ms`, `summary` (after the pass), \
`reclaimed` (batches, bytes), `idle` (why nothing moved) and `receipts`.",
    examples: &[Example {
        command: "ds report outbox drain --yes --output json",
        note: "`.data.drained` says what moved; `.data.projects[].receipts` what each row did.",
        runnable: false,
    }],
    refusals: DRAIN_REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &["flush", "push", "retry", "unstick", "sync now", "send"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

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
    let projects = ds_cli_server::server_reports::projects_with_publications(&database, &fence)
        .map_err(unreadable)?;
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
        "next": after.next,
    }))
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
    }
    if data["after"].is_object() {
        out.push_str(&format!(
            "  drained {} batch(es) · {}\n",
            data["drained"]["batches"].as_u64().unwrap_or(0),
            bytes(data["drained"]["bytes"].as_u64().unwrap_or(0)),
        ));
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
