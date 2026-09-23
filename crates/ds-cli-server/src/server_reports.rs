//! The Server adapter for the shared report publication producer.
//! The exporter seals bytes and their row through `ds_sync_runtime::reports`.
//! This adapter only connects that store to the authenticated runtime used by
//! Desktop as well. The sync store is the queue: every reading here is of the
//! store's rows, never of the artifact directory. The producer a pass runs
//! over is the host's ONE set (`SolarActivity::with_producers`): report and
//! Solar, keyed by engine.
use crate::server_sync::ServerSyncSession;
use ds_sync_runtime::{
    Reads, Receipt, Reclaimed, SyncHost, SyncRun, Trigger,
    kernel_sync::{Action, Summary, Wake},
    store::{ArtifactRow, Fence},
};
use std::path::Path;

/// A local observation of one project's queue: a digest over the store's
/// rows for it, so the pump can tell "something was sealed or settled" from
/// "nothing changed" without a second index and without a filesystem
/// timestamp.
pub struct Inventory {
    pub fingerprint: String,
}

/// What one pass did and what the shared kernel says it requires next.
/// Scheduler state comes only from the re-planned kernel result, not receipt
/// prose or a native retry policy. A pass never says nothing: `reclaimed`,
/// `idle` and `summary` say what moved, what was freed, and why nothing
/// moved when nothing did.
pub struct Pass {
    pub inventory: Inventory,
    pub retry_eligible: bool,
    pub offline: bool,
    pub wake_at_ms: Option<u64>,
    /// What this pass freed on this machine.
    pub reclaimed: Reclaimed,
    /// Why this pass moved no bytes, when it moved none (`SyncRun::idle`).
    pub idle: Option<String>,
    /// The plan as it stands after the pass.
    pub summary: Summary,
    /// Every receipt of the pass, in order.
    pub receipts: Vec<Receipt>,
}

/// Where this host's committed report batches live: beside the store.
pub(crate) fn root(database: &Path) -> Result<std::path::PathBuf, String> {
    Ok(database
        .parent()
        .ok_or("server database has no state directory")?
        .join("report-artifacts"))
}

/// Which projects hold report publications on this host: the store's rows
/// under this fence, plus any project whose batches are on disk with no
/// row yet — sealed before the seal wrote its row — so a pass visits and
/// adopts them. A Server that serves several projects must be able to find
/// its pending work without being told which project it is "on".
pub fn projects_with_publications(
    database: &Path,
    fence: &Fence,
) -> Result<std::collections::BTreeSet<String>, String> {
    let store = ds_sync_runtime::open_store(database)?;
    let mut projects: std::collections::BTreeSet<String> =
        ds_sync_runtime::projects_of_fence(&store, fence)?
            .into_iter()
            .collect();
    projects.extend(ds_sync_runtime::reports::projects(&root(database)?, None)?);
    Ok(projects)
}

fn fingerprint(rows: &[ArtifactRow]) -> Result<String, String> {
    // A complete canonical observation of the store's rows for the project:
    // identity, replay key, state, digest and when the row last moved. Any
    // seal, receipt or reclaim changes it; nothing on disk does.
    let rows = rows
        .iter()
        .filter(|row| row.identity.engine == ds_sync_runtime::reports::ENGINE)
        .map(|row| {
            serde_json::json!({
                "engine": row.identity.engine,
                "operation": row.identity.operation,
                "variant": row.identity.variant,
                "replayKey": row.replay_key,
                "state": row.state.as_str(),
                "sha256": row.sha256,
                "readable": row.readable,
                "updatedAtMs": row.updated_at_ms,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_vec(&rows)
        .map(|bytes| ds_compute_runtime::digest(&bytes))
        .map_err(|error| error.to_string())
}

pub fn inventory(session: &ServerSyncSession) -> Result<Inventory, String> {
    Ok(Inventory {
        fingerprint: fingerprint(&session.rows()?)?,
    })
}

/// One pass of the report protocol over `producer` — the host's whole
/// producer set, so a lost row of any engine this pass observes is freed
/// by the producer that owns its bytes (`ds_sync_runtime::Producers`).
pub fn drain(
    session: &ServerSyncSession,
    producer: &dyn ds_sync_runtime::Producer,
    reads: &dyn Reads,
    trigger: Trigger,
) -> Result<Pass, String> {
    session.with_host_for_project(session.project(), producer, reads, |host| {
        let result = ds_sync_runtime::run::run_reports(host, session.project(), trigger);
        match result {
            Ok(run) => Ok(pass(inventory(session)?, run)),
            Err(error) => {
                // A failed heartbeat or head read happens before any
                // publication is attempted. Each queued row stays held and
                // carries that cause, so the reader sees why it did not move.
                host.local(session.project())?;
                for row in session.rows()?.iter().filter(|row| {
                    row.identity.engine == ds_sync_runtime::reports::ENGINE
                        && matches!(row.state.as_str(), "held" | "uploading")
                }) {
                    host.record_receipt(
                        session.project(),
                        &Receipt::new("upload", "failed")
                            .about(&row.identity)
                            .detail(error.clone()),
                    )?;
                }
                Err(error)
            }
        }
    })
}

fn pass(inventory: Inventory, run: SyncRun) -> Pass {
    let idle = run.idle();
    let SyncRun {
        replanned: plan,
        receipts,
        reclaimed,
        ..
    } = run;
    Pass {
        inventory,
        retry_eligible: plan.summary.uploads != 0,
        offline: plan
            .actions
            .iter()
            .any(|action| matches!(action, Action::Offline)),
        wake_at_ms: match plan.wake {
            Wake::Event => None,
            Wake::At { at_ms, .. } => Some(at_ms),
        },
        reclaimed,
        idle,
        summary: plan.summary,
        receipts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_sync_runtime::kernel_sync::{Plan, RefreshRemote};

    fn inventory() -> Inventory {
        Inventory {
            fingerprint: "a".repeat(64),
        }
    }

    fn run(replanned_actions: Vec<Action>, uploads: usize, replanned_wake: Wake) -> SyncRun {
        SyncRun {
            schema: "ds.sync-run/v1",
            project: "project-a".into(),
            trigger: Trigger::LocalChange,
            plan: Plan {
                schema: "ds.sync-plan/v1",
                project: "project-a".into(),
                personal: false,
                actions: Vec::new(),
                refresh_remote: RefreshRemote {
                    needed: false,
                    reason: "upload_first",
                },
                wake: Wake::Event,
                grant_valid: false,
                summary: Summary::default(),
            },
            receipts: vec![Receipt::new("nothing", "in_sync")],
            replanned: Plan {
                schema: "ds.sync-plan/v1",
                project: "project-a".into(),
                personal: false,
                actions: replanned_actions,
                refresh_remote: RefreshRemote {
                    needed: false,
                    reason: "upload_first",
                },
                wake: replanned_wake,
                grant_valid: false,
                summary: Summary {
                    uploads,
                    ..Summary::default()
                },
            },
            reclaimed: Reclaimed {
                batches: 1,
                bytes: 42,
            },
        }
    }

    #[test]
    fn scheduler_state_comes_from_kernel_pending_actions_not_receipt_text() {
        let pass = pass(
            inventory(),
            run(
                vec![Action::Upload {
                    identity: ds_sync_runtime::kernel_sync::Identity {
                        engine: "network_reporter".into(),
                        operation: "tx-a".into(),
                        variant: "default".into(),
                    },
                    sha256: "a".repeat(64),
                    size_bytes: 1,
                    reason: "absent_remotely",
                }],
                1,
                Wake::Event,
            ),
        );
        assert!(pass.retry_eligible);
        assert!(!pass.offline);
        assert_eq!(pass.wake_at_ms, None);
        assert_eq!(pass.summary.uploads, 1);
        assert_eq!(pass.reclaimed.batches, 1);
        assert_eq!(pass.receipts.len(), 1);
    }

    #[test]
    fn scheduler_state_preserves_offline_and_kernel_deadline_and_a_pass_says_why_it_moved_nothing()
    {
        let pass = pass(
            inventory(),
            run(
                vec![Action::Offline],
                0,
                Wake::At {
                    at_ms: 42,
                    reason: "grant_renewal",
                },
            ),
        );
        assert!(!pass.retry_eligible);
        assert!(pass.offline);
        assert_eq!(pass.wake_at_ms, Some(42));
        let idle = pass.idle.expect("nothing moved, so the pass says why");
        assert!(idle.contains("1 batch reclaimed"), "{idle}");
    }

    #[test]
    fn the_fingerprint_follows_the_rows_not_the_disk() {
        use ds_sync_runtime::store::{ArtifactState, Scope};
        let row = |state: ArtifactState, updated: u64| ArtifactRow {
            scope: Scope::Project {
                project: "p".into(),
            },
            identity: ds_sync_runtime::kernel_sync::Identity {
                engine: "network_reporter".into(),
                operation: "export-tx".into(),
                variant: "default".into(),
            },
            sha256: "a".repeat(64),
            size_bytes: 1,
            produced_at_ms: 1,
            base_revision: None,
            input_base_fingerprint: None,
            engine_release: "r".into(),
            engine_build_manifest_sha256: None,
            grant_engine: None,
            resource: None,
            client_publish_id: "k".into(),
            outputs: Vec::new(),
            bytes_locator: "l".into(),
            readable: true,
            state,
            state_reason: None,
            replay_key: "k".into(),
            updated_at_ms: updated,
            transfer_state: None,
        };
        let held = fingerprint(&[row(ArtifactState::Held, 1)]).unwrap();
        assert_eq!(held, fingerprint(&[row(ArtifactState::Held, 1)]).unwrap());
        assert_ne!(
            held,
            fingerprint(&[row(ArtifactState::Published, 2)]).unwrap()
        );
        assert_ne!(held, fingerprint(&[]).unwrap());
    }
}
