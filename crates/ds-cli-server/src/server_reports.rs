//! The Server adapter for the shared report publication producer.
//! The exporter commits bytes through ds-report-artifacts. This adapter only
//! connects that store to the authenticated runtime used by Desktop as well.
use crate::server_sync::ServerSyncSession;
use ds_sync_runtime::{Reads, Receipt, SyncHost, Trigger};
use std::path::Path;

pub fn drain(
    database: &Path,
    session: &ServerSyncSession,
    reads: &dyn Reads,
) -> Result<(), String> {
    let root = database
        .parent()
        .ok_or("server database has no state directory")?
        .join("report-artifacts");
    let rows = ds_sync_runtime::reports::inventory(&root, session.project())?;
    if rows.is_empty() {
        return Ok(());
    }
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
        let result =
            ds_sync_runtime::run::run_reports(host, session.project(), Trigger::Manual).map(|_| ());
        if let Err(error) = &result {
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
        }
        result
    })
}
