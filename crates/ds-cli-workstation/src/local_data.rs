//! `ds workstation local-data` — what this machine holds for a project,
//! store by store, and the one clean action.
//!
//! The Server's state directory is the machine's local data: the lane's
//! sync store rows, the sealed report batches, the survey photos held
//! waiting or synced (each with its thumbnail) and the verified sync
//! downloads. This module reads counts and bytes off those stores and
//! hands them to the kernel (`ds_command_kernel::local_data`), which owns
//! the roster, retained-vs-cleanable and the clean plan; the browser answers
//! the same command over its own stores through the same kernel module.
//! Read-only like `ds report outbox status`: no session, no gateway, no
//! running Server. `clean` removes only what the kernel's plan names,
//! after `--yes`.

use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::local_data::{self, Host, Reading, Storage};
use ds_command_kernel::survey_moments::{self, SyncState};
use serde_json::{json, Value};

const PROJECT_ARG: Arg = Arg::value("project", "<project-id>", "Exact project whose local data is read.")
    .required();
const SERVER_STATE_DIR_ARG: Arg = Arg::value(
    "server-state-dir",
    "<absolute-path>",
    "Matching `ds server serve --state-dir` when the Server uses a custom state root.",
);
const LANE_ARG: Arg = Arg::value("lane", "<stable|canary>", "Native deployment lane.")
    .default("stable")
    .choices(&["stable", "canary"]);
const STORE_ARG: Arg = Arg::repeated(
    "store",
    "<store-id>",
    "Cleanable store to clean (repeatable); omit for every cleanable store.",
);

const ROOT_INVALID: Refusal = Refusal {
    code: "local_root_invalid",
    when: "the Server state root cannot be resolved from the lane and --server-state-dir",
    remedy: "omit --server-state-dir, or pass the same absolute path `ds server serve` uses",
};
const STORE_UNREADABLE: Refusal = Refusal {
    code: "local_store_unreadable",
    when: "the lane's sync store or a state subdirectory cannot be read on this machine",
    remedy: "check the Server state directory's permissions, or pass the same --server-state-dir as ds server serve",
};
const UNKNOWN_STORE: Refusal = Refusal {
    code: "unknown_store",
    when: "a --store is not a local store of this machine",
    remedy: "name a store id from `ds workstation local-data status`",
};
const STORE_RETAINED: Refusal = Refusal {
    code: "store_retained",
    when: "a --store names a retained store: this machine's only copy of work",
    remedy: "publish or sync the work first; only cleanable stores may be cleaned",
};
const NOTHING_TO_CLEAN: Refusal = Refusal {
    code: "nothing_to_clean",
    when: "no cleanable store holds anything for the project",
    remedy: "read the status first; a clean is offered only when a cleanable store holds something",
};

pub static STATUS_COMMAND: Command = Command {
    id: "workstation.local-data.status",
    path: &["workstation", "local-data", "status"],
    contract: 1,
    summary: "What this machine holds for a project, store by store.",
    purpose: "One row per local store of the Server's state root — sync store rows, sealed report batches, survey photos held waiting or synced (with their thumbnails), verified sync downloads — with count, bytes where the store tracks them (\"size not tracked\" otherwise), and whether the store is retained (the only copy) or cleanable (a replica). Needs no credential and no running Server; the browser answers the same command over its own stores.",
    chapter: Chapter::Workstation,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, SERVER_STATE_DIR_ARG, LANE_ARG],
    output: "`host`, `project`, `storage` (root, usage_bytes), `stores[]` (id, count, bytes, size_tracked, retention, note), `cleanable[]`, `cleanable_count`, `cleanable_bytes`, `refreshed_at_ms`.",
    examples: &[Example {
        command: "ds workstation local-data status --project <project-id> --lane canary --output json",
        note: "`.data.cleanable` names what `clean` would remove.",
        runnable: false,
    }],
    refusals: &[ROOT_INVALID, STORE_UNREADABLE],
    reference: Some("docs/reference/workstation.md"),
    search: &[
        "local data",
        "disk usage",
        "storage",
        "cache",
        "what does this machine hold",
        "space",
        "offline data",
    ],
    requires: Requires::Server,
    availability: || ds_cli_contract::spec::Availability::Available,
};

pub static CLEAN_COMMAND: Command = Command {
    id: "workstation.local-data.clean",
    path: &["workstation", "local-data", "clean"],
    contract: 1,
    summary: "Remove a project's cleanable local data, after confirmation.",
    purpose: "Removes only what the kernel's plan names: synced survey photos (their bundles) and verified sync downloads, for one project — replicas the machine can take again. Retained stores are refused by name. Answers what went, per store.",
    chapter: Chapter::Workstation,
    effect: Effect::ArtifactWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, STORE_ARG, SERVER_STATE_DIR_ARG, LANE_ARG],
    output: "`removed[]` (id, count, bytes), `count`, `bytes`, and the status after.",
    examples: &[Example {
        command: "ds workstation local-data clean --project <project-id> --store sync_downloads --lane canary --yes --output json",
        note: "Free the project's verified downloads; the next sync pass takes them again.",
        runnable: false,
    }],
    refusals: &[UNKNOWN_STORE, STORE_RETAINED, NOTHING_TO_CLEAN, ROOT_INVALID, STORE_UNREADABLE],
    reference: Some("docs/reference/workstation.md"),
    search: &["clean up", "free space", "clear cache", "delete local data", "remove downloads"],
    requires: Requires::Server,
    availability: || ds_cli_contract::spec::Availability::Available,
};

fn server_state(inputs: &Inputs) -> Result<PathBuf, Failure> {
    ds_compute_runtime::server_state_directory(
        inputs.require("lane")?,
        inputs.value("server-state-dir").map(Path::new),
    )
    .map_err(|error| Failure::invalid(ROOT_INVALID.code, error).remedy(ROOT_INVALID.remedy))
}

fn unreadable(error: impl std::fmt::Display) -> Failure {
    Failure::failed(STORE_UNREADABLE.code, error.to_string()).remedy(STORE_UNREADABLE.remedy)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Files and bytes below one directory; a missing directory holds nothing.
fn walk(root: &Path) -> (u64, u64) {
    fn visit(dir: &Path, count: &mut u64, bytes: &mut u64, depth: usize) {
        if depth > 16 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                visit(&entry.path(), count, bytes, depth + 1);
            } else if meta.is_file() {
                *count += 1;
                *bytes += meta.len();
            }
        }
    }
    let (mut count, mut bytes) = (0, 0);
    visit(root, &mut count, &mut bytes, 0);
    (count, bytes)
}

/// Every store's reading for one project off the state root.
fn readings(state: &Path, project: &str) -> Result<Vec<Reading>, Failure> {
    let database = state.join("store.sqlite");
    let store = ds_sync_store::Store::open_read_only(&database).map_err(unreadable)?;
    let (rows, batches, batch_bytes, media) = match &store {
        Some(store) => {
            let rows = store.rows_of_project(project).map_err(unreadable)?;
            let queue = store.queue_all(Some(project), now_ms()).map_err(unreadable)?;
            let (batches, bytes) = queue
                .projects
                .iter()
                .find(|entry| entry.project_id == project)
                .map_or((0, 0), |entry| (entry.batches, entry.bytes));
            let media = store.survey_media_of_project(project).map_err(unreadable)?;
            (rows, batches, bytes, media)
        }
        None => (0, 0, 0, Vec::new()),
    };
    let media_reading = |state: SyncState| {
        let held: Vec<_> = media.iter().filter(|record| record.state == state).collect();
        Reading {
            id: match state {
                SyncState::Waiting => "survey_media_waiting".into(),
                SyncState::Synced => "survey_media_synced".into(),
            },
            count: held.len() as u64,
            bytes: Some(held.iter().map(|record| record.size + record.thumbnail_size).sum()),
        }
    };
    let (download_count, download_bytes) = walk(&state.join("sync-downloads").join(project));
    Ok(vec![
        Reading {
            id: "sync_store".into(),
            count: rows,
            bytes: None,
        },
        Reading {
            id: "report_artifacts".into(),
            count: batches as u64,
            bytes: Some(batch_bytes),
        },
        media_reading(SyncState::Waiting),
        media_reading(SyncState::Synced),
        Reading {
            id: "sync_downloads".into(),
            count: download_count,
            bytes: Some(download_bytes),
        },
    ])
}

fn storage(state: &Path, readings: &[Reading]) -> Storage {
    Storage {
        usage_bytes: Some(readings.iter().filter_map(|reading| reading.bytes).sum()),
        quota_bytes: None,
        root: Some(state.to_string_lossy().into_owned()),
    }
}

pub fn status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let state = server_state(inputs)?;
    let readings = readings(&state, project)?;
    let storage = storage(&state, &readings);
    Ok(json!(local_data::answer(Host::Server, project, &readings, storage, now_ms())))
}

/// The kernel's refusal, re-raised under the code it named and the class
/// and remedy this surface declares for it.
fn refused(refusal: local_data::Refusal) -> Failure {
    let message = refusal.message;
    match refusal.code {
        "unknown_store" => Failure::invalid("unknown_store", message).remedy(UNKNOWN_STORE.remedy),
        "store_retained" => Failure::invalid("store_retained", message).remedy(STORE_RETAINED.remedy),
        "confirmation_required" => {
            Failure::invalid("confirmation_required", message).remedy(refusal.remedy)
        }
        _ => Failure::conflict("nothing_to_clean", message).remedy(NOTHING_TO_CLEAN.remedy),
    }
}

pub fn clean(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let state = server_state(inputs)?;
    let before = readings(&state, project)?;
    let named: Vec<String> = inputs.repeated("store").to_vec();
    // Dispatch already required --yes for this effect; the kernel's plan is
    // asked with that confirmation so a refusal names what would have gone.
    let plan = local_data::clean_plan(Host::Server, project, &before, &named, context.confirmed)
        .map_err(refused)?;
    let mut removed = Vec::new();
    for row in &plan.stores {
        match row.id.as_str() {
            "survey_media_synced" => {
                let mut store =
                    ds_sync_store::Store::open(&state.join("store.sqlite")).map_err(unreadable)?;
                let paths = store
                    .survey_media_remove_state(project, SyncState::Synced)
                    .map_err(unreadable)?;
                for path in &paths {
                    let dir = state
                        .join(survey_moments::MEDIA_ROOT)
                        .join(project)
                        .join(survey_moments::media_key(path));
                    let _ = std::fs::remove_dir_all(dir);
                }
                removed.push(json!({"id": row.id, "count": paths.len(), "bytes": row.bytes}));
            }
            "sync_downloads" => {
                let dir = state.join("sync-downloads").join(project);
                if dir.exists() {
                    std::fs::remove_dir_all(&dir).map_err(unreadable)?;
                }
                removed.push(json!({"id": row.id, "count": row.count, "bytes": row.bytes}));
            }
            other => {
                return Err(Failure::internal(
                    "clean_store_unhandled",
                    format!("the kernel planned a clean of {other}, which this host cannot perform"),
                ))
            }
        }
    }
    let after = readings(&state, project)?;
    let storage = storage(&state, &after);
    Ok(json!({
        "removed": removed,
        "count": plan.count,
        "bytes": plan.bytes,
        "status": local_data::answer(Host::Server, project, &after, storage, now_ms()),
    }))
}

pub fn render_status(data: &Value) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{} · {}\n",
        data["project"].as_str().unwrap_or(""),
        data["storage"]["root"].as_str().unwrap_or("")
    ));
    for row in data["stores"].as_array().into_iter().flatten() {
        let bytes = row["bytes"]
            .as_u64()
            .map_or("size not tracked".to_string(), |bytes| format!("{:.1} MiB", bytes as f64 / 1048576.0));
        out.push_str(&format!(
            "{:<22} {:>8} {:>18} {}\n",
            row["id"].as_str().unwrap_or(""),
            row["count"],
            bytes,
            row["retention"].as_str().unwrap_or("")
        ));
    }
    if let Some(cleanable) = data["cleanable"].as_array().filter(|list| !list.is_empty()) {
        out.push_str(&format!(
            "cleanable: {} ({} items)\n",
            cleanable.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),
            data["cleanable_count"]
        ));
    }
    out
}

pub fn render_clean(data: &Value) -> String {
    let mut out = String::new();
    for row in data["removed"].as_array().into_iter().flatten() {
        out.push_str(&format!("removed {:<22} {} items\n", row["id"].as_str().unwrap_or(""), row["count"]));
    }
    out.push_str(&render_status(&data["status"]));
    out
}
