//! Reconcile the named project's report publications before a CLI/MCP touch.
//!
//! The Server and the CLI use one store, one project lease and the same
//! `run_reports` pass. `Manual` is the kernel's always-read trigger: each
//! command gets its own remote-head observation, with no freshness timer.

#[cfg(target_os = "linux")]
use std::path::Path;

#[cfg(target_os = "linux")]
use ds_command_kernel::sync_store::Scope;
#[cfg(target_os = "linux")]
use ds_sync_runtime::Trigger;
use serde_json::{Value, json};

/// The command boundary decides what to do with a failed reconciliation.
/// Reads may return marked held data; publication must stop before its handler.
pub struct Touch {
    pub current: bool,
    pub receipt: Value,
}

/// One report-scoped pass for an exact project. Opening the Server's protected
/// connection record when it does not yet exist lets a CLI act without a
/// running Server; the SQLite store and project lease remain the same ones a
/// running Server uses. No second queue or publication protocol is created.
pub fn project(lane: &str, project: &str, state_dir: Option<&str>) -> Touch {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = state_dir;
        return unsupported_host(lane, project);
    }
    #[cfg(target_os = "linux")]
    {
        project_native(lane, project, state_dir)
    }
}

#[cfg(any(not(target_os = "linux"), test))]
fn unsupported_host(lane: &str, project: &str) -> Touch {
    Touch {
        current: false,
        receipt: json!({
            "schema": "ds.report-touch/v1",
            "project": project,
            "lane": lane,
            "engine": "network_reporter",
            "state": "not_checked",
            "reason": "report_sync_runtime_unavailable",
            "detail": "this build does not implement native report reconciliation on this operating system; use --dry-run for local observation exports or a supported headless Server for publication",
        }),
    }
}

#[cfg(test)]
mod host_tests {
    #[test]
    fn unsupported_host_names_the_missing_runtime_without_inventing_a_desktop() {
        let result = super::unsupported_host("stable", "project-a");
        assert!(!result.current);
        assert_eq!(result.receipt["project"], "project-a");
        assert_eq!(result.receipt["reason"], "report_sync_runtime_unavailable");
        assert!(!result.receipt.to_string().contains("Desktop"));
    }
}

#[cfg(target_os = "linux")]
fn project_native(lane: &str, project: &str, state_dir: Option<&str>) -> Touch {
    let state = ds_compute_runtime::server_state_directory(lane, state_dir.map(Path::new));
    let held_before = state
        .as_ref()
        .ok()
        .and_then(|state| read_at(state, lane, project));
    let attempt = state
        .as_ref()
        .map_err(|error| error.clone())
        .and_then(|state| {
            let connection = ds_cli_server::server_sync::native_connection(state, lane)?;
            let database = state.join("store.sqlite");
            let session = ds_cli_server::server_sync::ServerSyncSession::open(
                &database,
                &connection,
                project,
            )?;
            let lease = session.reclaim_abandoned_lease()?;
            let reads = ds_sync_runtime::VerifiedReads::new(state.join("sync-downloads"));
            let pass = ds_cli_server::solar_sync::with_producers(
                &database,
                session.identity(),
                project,
                None,
                |producers| {
                    ds_cli_server::server_reports::drain(
                        &session,
                        producers,
                        &reads,
                        Trigger::Manual,
                    )
                },
            )?;
            Ok((lease, pass))
        });
    let held_after = state
        .as_ref()
        .ok()
        .and_then(|state| read_at(state, lane, project));
    let blocked = state
        .as_ref()
        .ok()
        .and_then(|state| read_block(state, lane, project));
    let held_age_ms = held_after.and_then(|at| ds_sync_runtime::now_ms().checked_sub(at));
    match attempt {
        Ok((lease, pass)) => {
            let lease_held = lease.blocked_by.is_some()
                || pass
                    .receipts
                    .iter()
                    .any(|receipt| receipt.action == "lease");
            let failed = pass_failed(&pass);
            let current = pass_current(lease_held, &pass, held_after);
            let reason = if current {
                Value::Null
            } else if lease_held {
                json!("project_sync_lease_held")
            } else if pass.offline {
                json!("offline")
            } else if blocked.is_some() {
                json!("credential_refused")
            } else if failed {
                json!("report_sync_failed")
            } else {
                json!("report_sync_pending")
            };
            Touch {
                current,
                receipt: json!({
                    "schema": "ds.report-touch/v1",
                    "project": project,
                    "lane": lane,
                    "engine": "network_reporter",
                    "trigger": "manual",
                    "state": if current { "current" } else { "held" },
                    "reason": reason,
                    "blocked": blocked,
                    "head_read_at_ms": held_after,
                    "refreshed_remote": pass.refreshed_remote,
                    "held_age_ms": held_age_ms,
                    "previous_head_read_at_ms": held_before,
                    "summary": {
                        "uploads_pending": pass.summary.uploads,
                        "downloads_pending": pass.summary.downloads,
                        "conflicts": pass.summary.conflicts,
                        "refused": pass.summary.refused,
                    },
                    "receipts": pass.receipts.iter().take(40).map(|receipt| json!({
                        "action": receipt.action,
                        "outcome": receipt.outcome,
                        "detail": receipt.detail,
                    })).collect::<Vec<_>>(),
                    "more_receipts": pass.receipts.len().saturating_sub(40),
                }),
            }
        }
        Err(error) => Touch {
            current: false,
            receipt: json!({
                "schema": "ds.report-touch/v1",
                "project": project,
                "lane": lane,
                "engine": "network_reporter",
                "trigger": "manual",
                "state": "held",
                "reason": if blocked.is_some() { "credential_refused" } else { "report_sync_unavailable" },
                "detail": blocked.as_ref()
                    .and_then(|block| block["detail"].as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| error.chars().take(240).collect::<String>()),
                "blocked": blocked,
                "head_read_at_ms": held_after,
                "held_age_ms": held_age_ms,
                "previous_head_read_at_ms": held_before,
            }),
        },
    }
}

#[cfg(target_os = "linux")]
fn read_block(state: &Path, lane: &str, project: &str) -> Option<Value> {
    let fence = crate::outbox::fence(lane).ok()?;
    let store = ds_sync_store::Store::open_read_only(&state.join("store.sqlite")).ok()??;
    let status = store
        .queue(&fence, Some(project), ds_sync_runtime::now_ms())
        .ok()?;
    let block = status.blocked?;
    Some(json!({
        "cause": block.cause,
        "since_ms": block.since_ms,
        "detail": block.detail,
        "next": status.next,
    }))
}

#[cfg(target_os = "linux")]
fn pass_failed(pass: &ds_cli_server::server_reports::Pass) -> bool {
    pass.receipts.iter().any(|receipt| {
        matches!(receipt.outcome.as_str(), "failed" | "credential_refused")
            || matches!(receipt.action, "lease" | "reclaim_failed" | "store")
    })
}

#[cfg(target_os = "linux")]
fn pass_current(
    lease_blocked: bool,
    pass: &ds_cli_server::server_reports::Pass,
    head_read_at_ms: Option<u64>,
) -> bool {
    // A live lease means this pass did not read. The shared plan owns
    // offline and pending-action counts; a local receipt cannot turn any of
    // those into a claim of current report data.
    !lease_blocked
        && pass.refreshed_remote
        && !pass.offline
        && !pass_failed(pass)
        && pass.summary.uploads == 0
        && pass.summary.downloads == 0
        && head_read_at_ms.is_some()
}

#[cfg(target_os = "linux")]
fn read_at(state: &Path, lane: &str, project: &str) -> Option<u64> {
    let fence = crate::outbox::fence(lane).ok()?;
    let store = ds_sync_store::Store::open_read_only(&state.join("store.sqlite")).ok()??;
    store
        .heads_read_at_ms(
            &fence,
            &Scope::Project {
                project: project.to_owned(),
            },
        )
        .ok()?
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use ds_sync_runtime::{Receipt, Reclaimed, kernel_sync::Summary};

    fn pass() -> ds_cli_server::server_reports::Pass {
        ds_cli_server::server_reports::Pass {
            inventory: ds_cli_server::server_reports::Inventory {
                fingerprint: "a".repeat(64),
            },
            refreshed_remote: true,
            retry_eligible: false,
            offline: false,
            wake_at_ms: None,
            reclaimed: Reclaimed {
                batches: 0,
                bytes: 0,
            },
            idle: None,
            summary: Summary::default(),
            receipts: Vec::new(),
        }
    }

    #[test]
    fn a_touch_is_current_only_after_this_pass_can_finish_its_scoped_work() {
        let mut pass = pass();
        assert!(pass_current(false, &pass, Some(123)));
        assert!(!pass_current(true, &pass, Some(123)));
        assert!(!pass_current(false, &pass, None));
        pass.refreshed_remote = false;
        assert!(!pass_current(false, &pass, Some(123)));
        pass.refreshed_remote = true;
        pass.summary.downloads = 1;
        assert!(!pass_current(false, &pass, Some(123)));
        pass.summary.downloads = 0;
        pass.receipts
            .push(Receipt::new("upload", "credential_refused"));
        assert!(!pass_current(false, &pass, Some(123)));
        pass.receipts.clear();
        pass.receipts.push(Receipt::new("lease", "held"));
        assert!(!pass_current(false, &pass, Some(123)));
        pass.receipts.clear();
        pass.offline = true;
        assert!(!pass_current(false, &pass, Some(123)));
    }
}
