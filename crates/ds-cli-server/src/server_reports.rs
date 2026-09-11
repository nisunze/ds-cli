//! The Server adapter for the shared report publication producer.
//! The exporter commits bytes through ds-report-artifacts. This adapter only
//! connects that store to the authenticated runtime used by Desktop as well.
use crate::server_sync::ServerSyncSession;
use ds_sync_runtime::{
    Reads, Receipt, SyncHost, SyncRun, Trigger,
    kernel_sync::{Action, Wake},
};
use std::path::Path;

/// A local observation of the sealed publication inventory. The fingerprint is
/// derived from the shared producer rows, never by interpreting queue files in
/// this adapter; `ds-report-artifacts` remains their visibility authority.
pub struct Inventory {
    pub fingerprint: String,
}

/// What the shared kernel says this report pass requires next. Scheduler state
/// comes only from the re-planned kernel result, not receipt prose or a native
/// retry policy.
pub struct Pass {
    pub inventory: Inventory,
    pub retry_eligible: bool,
    pub offline: bool,
    pub wake_at_ms: Option<u64>,
}

fn root(database: &Path) -> Result<std::path::PathBuf, String> {
    Ok(database
        .parent()
        .ok_or("server database has no state directory")?
        .join("report-artifacts"))
}

fn rows(
    database: &Path,
    session: &ServerSyncSession,
) -> Result<Vec<ds_sync_runtime::LocalRow>, String> {
    ds_sync_runtime::reports::inventory(&root(database)?, session.project())
}

fn fingerprint(rows: &[ds_sync_runtime::LocalRow]) -> Result<String, String> {
    // LocalRow deliberately does not serialize: it is a runtime seam. Build a
    // complete canonical observation here instead of making a second artifact
    // parser or basing wake-up detection on one mutable filesystem timestamp.
    let rows = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "engine": row.identity.engine,
                "operation": row.identity.operation,
                "variant": row.identity.variant,
                "sha256": row.sha256,
                "sizeBytes": row.size_bytes,
                "baseRevision": row.base_revision,
                "readable": row.readable,
                "engineRelease": row.engine_release,
                "engineBuildManifestSha256": row.engine_build_manifest_sha256,
                "grantEngine": row.grant_engine,
                "resource": row.resource,
                "clientPublishId": row.client_publish_id,
                "outputs": row.outputs.iter().map(|output| serde_json::json!({
                    "outputId": output.output_id,
                    "format": output.format,
                    "contentType": output.content_type,
                    "sha256": output.sha256,
                    "sizeBytes": output.size_bytes,
                })).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_vec(&rows)
        .map(|bytes| ds_compute_runtime::digest(&bytes))
        .map_err(|error| error.to_string())
}

pub fn inventory(database: &Path, session: &ServerSyncSession) -> Result<Inventory, String> {
    let rows = rows(database, session)?;
    Ok(Inventory {
        fingerprint: fingerprint(&rows)?,
    })
}

pub fn drain(
    database: &Path,
    session: &ServerSyncSession,
    reads: &dyn Reads,
    trigger: Trigger,
) -> Result<Pass, String> {
    let root = root(database)?;
    let rows = ds_sync_runtime::reports::inventory(&root, session.project())?;
    let inventory = Inventory {
        fingerprint: fingerprint(&rows)?,
    };
    let upload =
        |handle: ds_report_artifacts::SealedArtifactHandle, output_id: &str, session_uri: &str| {
            ds_sync_runtime::transfer_verified_output(
                output_id,
                session_uri,
                handle.size_bytes,
                &handle.sha256,
                handle.file,
            )
        };
    let producer = ds_sync_runtime::reports::ReportProducer {
        root: &root,
        project: session.project(),
        upload: &upload,
    };
    session.with_host_for_project(session.project(), &producer, reads, |host| {
        let result = ds_sync_runtime::run::run_reports(host, session.project(), trigger);
        match result {
            Ok(run) => Ok(pass(inventory, run)),
            Err(error) => {
                // A failed heartbeat/head read happens before a planned action.
                // Keep that failure beside each pending row in the existing store,
                // with the same held/retry semantics as Desktop work-grant errors.
                host.local(session.project())?;
                for row in &rows {
                    host.record_receipt(
                        session.project(),
                        &Receipt::new("open_grant", "failed")
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
    let plan = run.replanned;
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_sync_runtime::kernel_sync::{Plan, RefreshRemote, Summary};

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
            receipts: Vec::new(),
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
    }

    #[test]
    fn scheduler_state_preserves_offline_and_kernel_deadline() {
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
    }
}
