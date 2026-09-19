//! `ds report outbox` — the publication queue, in the open.
//!
//! A report that has been produced is not finished: its bytes are sealed into
//! a local queue and published from there. That queue used to be invisible.
//! An operator could produce reports all day, watch the cloud call every room
//! `stale`, and have no way to see that hundreds of sealed artifacts were
//! waiting on his own machine behind a lock whose holder had died — no count,
//! no age, no holder, no command. The only cure anyone had was to find a
//! hidden file and delete it.
//!
//! These two commands are that queue's surface:
//!
//! * `status` — how much is queued, how old the oldest is, whether the lock
//!   is held and by whom, whether the holder is alive, and what to run next.
//!   Credential-free and read-only: it answers on a headless box with no
//!   session and no running Server, which is exactly where a wedged pump goes
//!   unnoticed.
//! * `drain` — push the queue now, through the SAME shared publication runner
//!   the background pump uses. There is deliberately no second pump: one
//!   delivery singleton, woken by hand.
//!
//! Neither command decides anything. The queue's own reading lives in
//! `ds_report_artifacts::queue_status`, and draining is
//! `ds_cli_server::server_reports::drain` — the runner the Server already
//! owns. This module resolves the queue root, names refusals and renders.

use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
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
    when: "the publication queue directory cannot be read on this machine",
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
    remedy: "read `ds report outbox status`; a held lock names its holder",
};

const READ_REFUSALS: &[Refusal] = &[QUEUE_ROOT_INVALID, QUEUE_UNREADABLE];

const DRAIN_REFUSALS: &[Refusal] = &[
    QUEUE_ROOT_INVALID,
    QUEUE_UNREADABLE,
    SERVER_UNREACHABLE,
    DRAIN_FAILED,
];

pub static STATUS: Command = Command {
    id: "report.outbox.status",
    path: &["report", "outbox", "status"],
    contract: 1,
    summary: "Show the report publication queue: what is waiting, and what holds it.",
    purpose: "\
Reads this machine's own report publication queue — the sealed artifacts a \
produced report enters before it reaches the shared store: how much is \
waiting, for how long, and whether the queue's lock is held by a process \
that is still alive. Needs no credential, no project selection and no \
running Server.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, SERVER_STATE_DIR_ARG, LANE_ARG],
    output: "\
`queued_batches`, `queued_bytes` and `oldest_age_ms` for the machine, \
`projects[]` (counts, oldest age, rooms), `lock` (held, holder `live`/\
`gone`/`unknown` with its evidence, heartbeat age, reclaimable, releases \
already performed), `stuck` — whether anything needs a human — and `next`, \
the one command to run.",
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
        "lock",
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
runner the Server's pump uses — never a second pump or queue. Safe to \
run twice: a batch already in the shared store is recognised by its client \
publish id. A lock left by a process provably gone is released by the pass. \
An offline pass changes nothing and says so.",
    chapter: Chapter::Reports,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, SERVER_STATE_DIR_ARG, LANE_ARG],
    output: "\
`before` and `after` queue readings (batches, bytes, oldest age), `drained`, \
and `projects[]`: per project `offline`, `retry_eligible`, `wake_at_ms` and \
whether its sealed inventory changed. Plus `reclaimed_lock` when an \
abandoned lock was released.",
    examples: &[Example {
        command: "ds report outbox drain --yes --output json",
        note: "`.data.drained` says what moved.",
        runnable: false,
    }],
    refusals: DRAIN_REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &["flush", "push", "retry", "unstick", "sync now", "send"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// The queue root for this lane, derived exactly as the Server derives its
/// own database directory — one resolver, so `ds report project export`,
/// the Server pump and this command can never disagree about which queue
/// they are talking about.
fn queue_root(inputs: &Inputs) -> Result<PathBuf, Failure> {
    server_state(inputs).map(|state| state.join("report-artifacts"))
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

/// One queue reading, in this CLI's own vocabulary.
fn reading(status: &ds_report_artifacts::QueueStatus) -> Value {
    json!({
        "queued_batches": status.queued_batches,
        "queued_bytes": status.queued_bytes,
        "oldest_age_ms": status.oldest_age_ms,
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
            }))
            .collect::<Vec<_>>(),
    })
}

fn lock_reading(lock: &ds_report_artifacts::PendingUploadLockStatus) -> Value {
    json!({
        "present": lock.present,
        "held": lock.held,
        "holder": lock.holder,
        "holder_reason": lock.holder_reason,
        "heartbeat_age_ms": lock.heartbeat_age_ms,
        "reclaimable": lock.reclaimable,
        "owner": lock.owner.as_ref().map(|owner| json!({
            "pid": owner.pid,
            "worker": owner.worker,
            "taken_at_ms": owner.taken_at_ms,
            "heartbeat_at_ms": owner.heartbeat_at_ms,
        })),
        "recent_reclaims": lock
            .recent_reclaims
            .iter()
            .map(|reclaim| json!({
                "at_ms": reclaim.at_ms,
                "reason": reclaim.reason,
                "previous_pid": reclaim.previous_pid,
                "previous_worker": reclaim.previous_worker,
                "reclaimed_by_pid": reclaim.reclaimed_by_pid,
            }))
            .collect::<Vec<_>>(),
    })
}

fn project(inputs: &Inputs) -> Option<String> {
    inputs.value("project").map(str::to_string)
}

pub fn status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let root = queue_root(inputs)?;
    let status =
        ds_report_artifacts::queue_status(&root, project(inputs).as_deref()).map_err(unreadable)?;
    let mut value = reading(&status);
    let fields = json!({
        "lock": lock_reading(&status.lock),
        "stuck": status.stuck,
        "next": status.next,
    });
    value
        .as_object_mut()
        .expect("the reading is an object")
        .extend(fields.as_object().expect("fields are an object").clone());
    Ok(value)
}

pub fn drain(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let state = server_state(inputs)?;
    let root = state.join("report-artifacts");
    let wanted = project(inputs);
    let before = ds_report_artifacts::queue_status(&root, wanted.as_deref()).map_err(unreadable)?;
    // First, unwedge. The uploader never takes the queue's lock, so a marker
    // left by a process that is provably gone would otherwise sit there until
    // someone happened to export again. This is what makes "restart and the
    // queue moves" true with no human step — and it releases nothing whose
    // holder is alive or merely unprovable.
    let reclaimed = ds_report_artifacts::reclaim_pending_upload_lock(&root).map_err(unreadable)?;

    let connection = ds_cli_server::host::load_connection(&state).map_err(|error| {
        Failure::unavailable(SERVER_UNREACHABLE.code, error).remedy(SERVER_UNREACHABLE.remedy)
    })?;
    let database = state.join("store.sqlite");
    // The Sync Center reader the Server itself uses, anchored under the same
    // protected state directory. A drain must not invent a second download
    // cache beside the pump's.
    let reads = ds_sync_runtime::VerifiedReads::new(state.join("sync-downloads"));

    let mut passes = Vec::new();
    for project_id in ds_report_artifacts::queued_projects(&root).map_err(unreadable)? {
        if wanted.as_ref().is_some_and(|wanted| *wanted != project_id) {
            continue;
        }
        let pass = ds_cli_server::server_sync::ServerSyncSession::open(
            &database,
            &connection,
            &project_id,
        )
        .and_then(|session| {
            let before = ds_cli_server::server_reports::inventory(&database, &session)?;
            // `Manual` is the kernel's own name for "the operator pressed
            // Sync now". A hand-driven drain is exactly that, and it must not
            // borrow a scheduler trigger that changes retry policy.
            let pass = ds_cli_server::server_reports::drain(
                &database,
                &session,
                &reads,
                ds_sync_runtime::Trigger::Manual,
            )?;
            Ok(json!({
                "project": project_id,
                "offline": pass.offline,
                "retry_eligible": pass.retry_eligible,
                "wake_at_ms": pass.wake_at_ms,
                "inventory_changed": before.fingerprint != pass.inventory.fingerprint,
            }))
        })
        .map_err(|error| {
            Failure::failed(DRAIN_FAILED.code, format!("{project_id}: {error}"))
                .remedy(DRAIN_REFUSALS[3].remedy)
        })?;
        passes.push(pass);
    }

    let after = ds_report_artifacts::queue_status(&root, wanted.as_deref()).map_err(unreadable)?;
    let mut value = json!({
        "before": reading(&before),
        "after": reading(&after),
        "drained": {
            "batches": before.queued_batches.saturating_sub(after.queued_batches),
            "bytes": before.queued_bytes.saturating_sub(after.queued_bytes),
        },
        "projects": passes,
        "lock": lock_reading(&after.lock),
        "stuck": after.stuck,
        "next": after.next,
    });
    // A lock this pass released is reported, never silent: a reclaim an
    // operator cannot see is the next invisible failure.
    if let Some(reclaim) = reclaimed.as_ref() {
        value["reclaimed_lock"] = json!({
            "reason": reclaim.reason,
            "previous_pid": reclaim.previous_pid,
            "previous_worker": reclaim.previous_worker,
            "at_ms": reclaim.at_ms,
        });
    }
    Ok(value)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} batch(es) queued · {} · oldest {}\n",
        data["queued_batches"]
            .as_u64()
            .or_else(|| data["after"]["queued_batches"].as_u64())
            .unwrap_or(0),
        bytes(
            data["queued_bytes"]
                .as_u64()
                .or_else(|| data["after"]["queued_bytes"].as_u64())
                .unwrap_or(0)
        ),
        age(data["oldest_age_ms"]
            .as_u64()
            .or_else(|| data["after"]["oldest_age_ms"].as_u64())
            .unwrap_or(0)),
    );
    let projects = data["projects"].as_array().cloned().unwrap_or_default();
    for project in &projects {
        if let Some(count) = project["queued_batches"].as_u64() {
            out.push_str(&format!(
                "  {:<28} {count} batch(es) · {} · oldest {}\n",
                project["project"].as_str().unwrap_or("?"),
                bytes(project["queued_bytes"].as_u64().unwrap_or(0)),
                age(project["oldest_age_ms"].as_u64().unwrap_or(0)),
            ));
        } else {
            out.push_str(&format!(
                "  {:<28} {}{}\n",
                project["project"].as_str().unwrap_or("?"),
                if project["offline"].as_bool().unwrap_or(false) {
                    "offline; the queue keeps its work and retries"
                } else if project["inventory_changed"].as_bool().unwrap_or(false) {
                    "published"
                } else {
                    "nothing left to publish"
                },
                if project["retry_eligible"].as_bool().unwrap_or(false) {
                    " · retry pending"
                } else {
                    ""
                },
            ));
        }
    }
    let lock = &data["lock"];
    if lock["held"].as_bool().unwrap_or(false) || lock["present"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "  lock: {} · holder {}{}\n",
            if lock["held"].as_bool().unwrap_or(false) {
                "held"
            } else {
                "free"
            },
            lock["holder"].as_str().unwrap_or("none"),
            match lock["owner"]["pid"].as_u64() {
                Some(pid) => format!(
                    " (pid {pid}{})",
                    lock["owner"]["worker"]
                        .as_str()
                        .filter(|worker| !worker.is_empty())
                        .map(|worker| format!(", {worker}"))
                        .unwrap_or_default()
                ),
                None => String::new(),
            },
        ));
    }
    if let Some(reclaim) = data.get("reclaimed_lock").filter(|value| !value.is_null()) {
        out.push_str(&format!(
            "  released an abandoned lock from pid {}: {}\n",
            reclaim["previous_pid"].as_u64().unwrap_or(0),
            reclaim["reason"].as_str().unwrap_or("holder gone"),
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
